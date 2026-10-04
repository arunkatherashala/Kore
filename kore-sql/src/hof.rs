//! Higher-order functions over arrays and maps: TRANSFORM, FILTER, EXISTS, FORALL, AGGREGATE / REDUCE,
//! ZIP_WITH, ARRAY_SORT with a comparator, MAP_FILTER, TRANSFORM_KEYS / TRANSFORM_VALUES.
//!
//! A lambda is parsed as `FuncCall("__LAMBDA", [Str(param).., body])`. It is applied by substituting the
//! parameters with literals in the body and evaluating the result against the current row, so the body can use
//! any scalar expression and even columns of the row.

use kore_core::DataBlock;

use crate::arrays;
use crate::ast::Expr;
use crate::ast_walk::map_expr;
use crate::executor::{eval_expr, ExprVal};
use crate::maps;

type V = ExprVal;

pub fn has_lambda(args: &[Expr]) -> bool {
    args.iter().any(|a| matches!(a, Expr::FuncCall { name, .. } if name == "__LAMBDA"))
}

struct Lambda<'a> { params: Vec<&'a str>, body: &'a Expr }

fn lambda(e: &Expr) -> Option<Lambda<'_>> {
    match e {
        Expr::FuncCall { name, args } if name == "__LAMBDA" && !args.is_empty() => {
            let params = args[..args.len() - 1].iter().filter_map(|a| if let Expr::Str(s) = a { Some(s.as_str()) } else { None }).collect();
            Some(Lambda { params, body: &args[args.len() - 1] })
        }
        _ => None,
    }
}

fn lit(v: &V) -> Expr {
    match v {
        V::Int(i) => Expr::Int(*i),
        V::Float(f) => Expr::Float(*f),
        V::Str(s) => Expr::Str(s.clone()),
        V::Bool(b) => Expr::Bool(*b),
        V::Null => Expr::Null,
    }
}

impl Lambda<'_> {
    fn apply(&self, vals: &[V], block: &DataBlock, row: usize) -> V {
        let bound = map_expr(self.body, &mut |x| match x {
            Expr::Col(c) => self.params.iter().position(|p| p.eq_ignore_ascii_case(c)).and_then(|i| vals.get(i)).map(lit),
            // an inner lambda keeps its own parameters
            _ => None,
        });
        eval_expr(&bound, block, row)
    }
}

fn fail(msg: &str) -> V { crate::scalar::set_error(msg); V::Null }

/// Merge sort with a comparator that need not be a total order (a user lambda can be anything, and
/// `slice::sort_by` may panic on such comparators).
fn merge_sort(items: Vec<V>, cmp: &mut dyn FnMut(&V, &V) -> i64) -> Vec<V> {
    if items.len() <= 1 { return items; }
    let mid = items.len() / 2;
    let right = merge_sort(items[mid..].to_vec(), cmp);
    let left = merge_sort(items[..mid].to_vec(), cmp);
    let (mut out, mut i, mut j) = (Vec::with_capacity(left.len() + right.len()), 0, 0);
    while i < left.len() && j < right.len() {
        if cmp(&left[i], &right[j]) <= 0 { out.push(left[i].clone()); i += 1; } else { out.push(right[j].clone()); j += 1; }
    }
    out.extend_from_slice(&left[i..]);
    out.extend_from_slice(&right[j..]);
    out
}

pub fn call(name: &str, args: &[Expr], block: &DataBlock, row: usize) -> V {
    let first = eval_expr(&args[0], block, row);
    let lam = |i: usize| args.get(i).and_then(lambda);
    match name {
        "TRANSFORM" | "FILTER" | "EXISTS" | "FORALL" => {
            let Some(l) = lam(1) else { return fail(&format!("{name} needs a lambda as its second argument")) };
            let Some(items) = arrays::decode(&first) else { return V::Null };
            let call_l = |i: usize, x: &V| -> V {
                if l.params.len() >= 2 { l.apply(&[x.clone(), V::Int(i as i64)], block, row) } else { l.apply(&[x.clone()], block, row) }
            };
            match name {
                "TRANSFORM" => arrays::encode(&items.iter().enumerate().map(|(i, x)| call_l(i, x)).collect::<Vec<_>>()),
                "FILTER" => arrays::encode(&items.iter().enumerate().filter(|(i, x)| matches!(call_l(*i, x), V::Bool(true))).map(|(_, x)| x.clone()).collect::<Vec<_>>()),
                "EXISTS" => {
                    let mut unknown = false;
                    for (i, x) in items.iter().enumerate() {
                        match call_l(i, x) { V::Bool(true) => return V::Bool(true), V::Bool(false) => {}, _ => unknown = true }
                    }
                    if unknown { V::Null } else { V::Bool(false) }
                }
                _ => {
                    let mut unknown = false;
                    for (i, x) in items.iter().enumerate() {
                        match call_l(i, x) { V::Bool(false) => return V::Bool(false), V::Bool(true) => {}, _ => unknown = true }
                    }
                    if unknown { V::Null } else { V::Bool(true) }
                }
            }
        }
        "AGGREGATE" | "REDUCE" => {
            let Some(merge) = lam(2) else { return fail("AGGREGATE needs a merge lambda as its third argument") };
            let Some(items) = arrays::decode(&first) else { return V::Null };
            let mut acc = eval_expr(&args[1], block, row);
            for x in &items { acc = merge.apply(&[acc, x.clone()], block, row); }
            match lam(3) { Some(fin) => fin.apply(&[acc], block, row), None => acc }
        }
        "ZIP_WITH" => {
            let Some(l) = lam(2) else { return fail("ZIP_WITH needs a lambda as its third argument") };
            let second = eval_expr(&args[1], block, row);
            let (Some(a), Some(b)) = (arrays::decode(&first), arrays::decode(&second)) else { return V::Null };
            let n = a.len().max(b.len());
            let out: Vec<V> = (0..n).map(|i| l.apply(&[a.get(i).cloned().unwrap_or(V::Null), b.get(i).cloned().unwrap_or(V::Null)], block, row)).collect();
            arrays::encode(&out)
        }
        "ARRAY_SORT" => {
            let Some(l) = lam(1) else { return V::Null };
            let Some(items) = arrays::decode(&first) else { return V::Null };
            let sorted = merge_sort(items, &mut |x, y| match l.apply(&[x.clone(), y.clone()], block, row) {
                V::Int(i) => i,
                V::Float(f) => f.signum() as i64,
                _ => 0,
            });
            arrays::encode(&sorted)
        }
        "MAP_FILTER" | "TRANSFORM_KEYS" | "TRANSFORM_VALUES" => {
            let Some(l) = lam(1) else { return fail(&format!("{name} needs a lambda as its second argument")) };
            let Some(pairs) = maps::decode(&first) else { return V::Null };
            match name {
                "MAP_FILTER" => maps::encode(&pairs.into_iter().filter(|(k, v)| matches!(l.apply(&[k.clone(), v.clone()], block, row), V::Bool(true))).collect::<Vec<_>>()),
                "TRANSFORM_KEYS" => maps::encode(&pairs.into_iter().map(|(k, v)| (l.apply(&[k, v.clone()], block, row), v)).collect::<Vec<_>>()),
                _ => maps::encode(&pairs.into_iter().map(|(k, v)| { let nv = l.apply(&[k.clone(), v], block, row); (k, nv) }).collect::<Vec<_>>()),
            }
        }
        "MAP_ZIP_WITH" => {
            let Some(l) = lam(2) else { return fail("MAP_ZIP_WITH needs a lambda as its third argument") };
            let second = eval_expr(&args[1], block, row);
            let (Some(a), Some(b)) = (maps::decode(&first), maps::decode(&second)) else { return V::Null };
            let mut keys: Vec<V> = a.iter().map(|(k, _)| k.clone()).collect();
            for (k, _) in &b { if !keys.iter().any(|x| crate::scalar::eq_vals(x, k) == Some(true)) { keys.push(k.clone()); } }
            maps::encode(&keys.into_iter().map(|k| {
                let v = l.apply(&[k.clone(), maps::lookup(&a, &k), maps::lookup(&b, &k)], block, row);
                (k, v)
            }).collect::<Vec<_>>())
        }
        _ => V::Null,
    }
}

pub fn names() -> &'static [&'static str] {
    &["__LAMBDA", "__POSEXPLODE", "__EXPLODE_OUTER", "__POSEXPLODE_OUTER", "TRANSFORM", "FILTER", "EXISTS", "FORALL", "AGGREGATE", "REDUCE", "ZIP_WITH", "MAP_FILTER", "TRANSFORM_KEYS", "TRANSFORM_VALUES", "MAP_ZIP_WITH"]
}
