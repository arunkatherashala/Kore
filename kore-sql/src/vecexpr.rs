//! Column-at-a-time evaluation of filters and numeric expressions.
//!
//! The row interpreter (`executor::eval_expr`) allocates a `String` for every string cell it touches and
//! dispatches on the expression tree for every row. These routines walk the tree once per chunk of rows and
//! loop over the raw column data, with SQL three-valued logic for predicates. Chunks of `CHUNK` rows are
//! evaluated in parallel and keep their temporaries cache-sized. They return `None` for anything they do
//! not cover, and the caller falls back to the row interpreter.

use kore_core::{ColumnData, DataBlock};
use rayon::prelude::*;
use crate::ast::*;

const FALSE: u8 = 0;
const TRUE: u8 = 1;
const NULL: u8 = 2;
const CHUNK: usize = 65_536;

fn find_col<'a>(block: &'a DataBlock, name: &str) -> Option<&'a ColumnData> {
    block.columns.iter().find(|c| {
        c.name == name || {
            let (cn, nm) = (c.name.len(), name.len());
            cn > nm && c.name.as_bytes()[cn - nm - 1] == b'.' && &c.name[cn - nm..] == name
        }
    }).map(|c| &c.data)
}

fn col_of<'a>(e: &Expr, block: &'a DataBlock) -> Option<&'a ColumnData> {
    match e {
        Expr::Col(c) => find_col(block, c),
        Expr::QualCol(t, c) => find_col(block, &format!("{t}.{c}")),
        _ => None,
    }
}

/// Rows `lo..hi` of the block; operands are slices of that window, indexed from 0.
#[derive(Clone, Copy)]
struct Win { lo: usize, hi: usize }

impl Win {
    fn len(self) -> usize { self.hi - self.lo }
}

/// A numeric or string operand, borrowing column data where possible.
enum Val<'a> {
    I(&'a [Option<i64>]),
    F(&'a [Option<f64>]),
    Num(f64),
    Owned(Vec<Option<f64>>),
    S(&'a [Option<String>]),
    D(&'a [u8], &'a [String]),
    Text(&'a str),
}

impl Val<'_> {
    #[inline]
    fn num(&self, i: usize) -> Option<f64> {
        match self {
            Val::I(v) => v[i].map(|x| x as f64),
            Val::F(v) => v[i],
            Val::Num(c) => Some(*c),
            Val::Owned(v) => v[i],
            _ => None,
        }
    }
    #[inline]
    fn text(&self, i: usize) -> Option<&str> {
        match self {
            Val::S(v) => v[i].as_deref(),
            Val::D(codes, dict) => if codes[i] == u8::MAX { None } else { dict.get(codes[i] as usize).map(|s| s.as_str()) },
            Val::Text(t) => Some(t),
            _ => None,
        }
    }
    fn is_num(&self) -> bool { matches!(self, Val::I(_) | Val::F(_) | Val::Num(_) | Val::Owned(_)) }
    fn is_text(&self) -> bool { matches!(self, Val::S(_) | Val::D(..) | Val::Text(_)) }
}

fn operand<'a>(e: &'a Expr, block: &'a DataBlock, w: Win) -> Option<Val<'a>> {
    match e {
        Expr::Int(i) => Some(Val::Num(*i as f64)),
        Expr::Float(f) => Some(Val::Num(*f)),
        Expr::Str(s) => Some(Val::Text(s.as_str())),
        Expr::Col(_) | Expr::QualCol(..) => match col_of(e, block)? {
            ColumnData::Int64(v) => Some(Val::I(&v[w.lo..w.hi])),
            ColumnData::Float64(v) => Some(Val::F(&v[w.lo..w.hi])),
            ColumnData::Str(v) => Some(Val::S(&v[w.lo..w.hi])),
            ColumnData::StrDict { codes, dict } => Some(Val::D(&codes[w.lo..w.hi], dict)),
            ColumnData::Bool(_) => None,
        },
        Expr::BinOp { op: BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul | BinOpKind::Div | BinOpKind::Mod, .. }
        | Expr::Case { .. } => num_win(e, block, w).map(Val::Owned),
        _ => None,
    }
}

fn cmp_ok(op: &BinOpKind, a: f64, b: f64) -> bool {
    match op {
        BinOpKind::Eq => (a - b).abs() < 1e-10,
        BinOpKind::Ne => (a - b).abs() >= 1e-10,
        BinOpKind::Lt => a < b,
        BinOpKind::Le => a <= b,
        BinOpKind::Gt => a > b,
        BinOpKind::Ge => a >= b,
        _ => false,
    }
}

fn cmp_text(op: &BinOpKind, a: &str, b: &str) -> bool {
    match op {
        BinOpKind::Eq => a == b,
        BinOpKind::Ne => a != b,
        BinOpKind::Lt => a < b,
        BinOpKind::Le => a <= b,
        BinOpKind::Gt => a > b,
        BinOpKind::Ge => a >= b,
        _ => false,
    }
}

fn compare(op: &BinOpKind, l: &Val, r: &Val, n: usize) -> Option<Vec<u8>> {
    if l.is_num() && r.is_num() {
        Some((0..n).map(|i| match (l.num(i), r.num(i)) {
            (Some(a), Some(b)) => if cmp_ok(op, a, b) { TRUE } else { FALSE },
            _ => NULL,
        }).collect())
    } else if l.is_text() && r.is_text() {
        Some((0..n).map(|i| match (l.text(i), r.text(i)) {
            (Some(a), Some(b)) => if cmp_text(op, a, b) { TRUE } else { FALSE },
            _ => NULL,
        }).collect())
    } else {
        None
    }
}

fn not(v: Vec<u8>) -> Vec<u8> {
    v.into_iter().map(|x| match x { TRUE => FALSE, FALSE => TRUE, o => o }).collect()
}

fn and3(l: &[u8], r: &[u8]) -> Vec<u8> {
    l.iter().zip(r).map(|(&a, &b)| if a == FALSE || b == FALSE { FALSE } else if a == TRUE && b == TRUE { TRUE } else { NULL }).collect()
}

/// Three-valued truth value of `e` for the rows in `w`, or None if the expression is not covered.
fn tri<'a>(e: &'a Expr, block: &'a DataBlock, w: Win) -> Option<Vec<u8>> {
    let n = w.len();
    match e {
        Expr::Bool(b) => Some(vec![if *b { TRUE } else { FALSE }; n]),
        Expr::Not(x) => Some(not(tri(x, block, w)?)),
        Expr::BinOp { op: BinOpKind::And, left, right } => Some(and3(&tri(left, block, w)?, &tri(right, block, w)?)),
        Expr::BinOp { op: BinOpKind::Or, left, right } => {
            let (l, r) = (tri(left, block, w)?, tri(right, block, w)?);
            Some(l.iter().zip(&r).map(|(&a, &b)| if a == TRUE || b == TRUE { TRUE } else if a == FALSE && b == FALSE { FALSE } else { NULL }).collect())
        }
        Expr::BinOp { op: op @ (BinOpKind::Eq | BinOpKind::Ne | BinOpKind::Lt | BinOpKind::Le | BinOpKind::Gt | BinOpKind::Ge), left, right } => {
            let (l, r) = (operand(left, block, w)?, operand(right, block, w)?);
            compare(op, &l, &r, n)
        }
        Expr::Between { expr, low, high, negated } => {
            let (v, lo, hi) = (operand(expr, block, w)?, operand(low, block, w)?, operand(high, block, w)?);
            let ge = compare(&BinOpKind::Ge, &v, &lo, n)?;
            let le = compare(&BinOpKind::Le, &v, &hi, n)?;
            let both = and3(&ge, &le);
            Some(if *negated { not(both) } else { both })
        }
        Expr::In { expr, values, negated } => {
            let v = operand(expr, block, w)?;
            let out: Vec<u8> = if v.is_num() {
                let mut set = Vec::with_capacity(values.len());
                for x in values {
                    match x { Expr::Int(i) => set.push(*i as f64), Expr::Float(f) => set.push(*f), _ => return None }
                }
                if set.len() > 8 {
                    // long lists (decorrelated IN subqueries): exact-value hash set instead of a linear scan
                    let key = |f: f64| if f == 0.0 { 0.0f64.to_bits() } else { f.to_bits() };
                    let hs: std::collections::HashSet<u64> = set.iter().map(|&f| key(f)).collect();
                    (0..n).map(|i| match v.num(i) {
                        None => NULL,
                        Some(a) => if hs.contains(&key(a)) { TRUE } else { FALSE },
                    }).collect()
                } else {
                    (0..n).map(|i| match v.num(i) {
                        None => NULL,
                        Some(a) => if set.iter().any(|b| (a - b).abs() < 1e-10) { TRUE } else { FALSE },
                    }).collect()
                }
            } else if v.is_text() {
                let mut set = std::collections::HashSet::with_capacity(values.len());
                for x in values {
                    match x { Expr::Str(s) => { set.insert(s.as_str()); } _ => return None }
                }
                (0..n).map(|i| match v.text(i) {
                    None => NULL,
                    Some(a) => if set.contains(a) { TRUE } else { FALSE },
                }).collect()
            } else {
                return None;
            };
            Some(if *negated { not(out) } else { out })
        }
        Expr::Like { expr, pattern, negated } => {
            let (Expr::Str(p), v) = (pattern.as_ref(), operand(expr, block, w)?) else { return None };
            if !v.is_text() { return None; }
            let out: Vec<u8> = (0..n).map(|i| match v.text(i) {
                None => NULL,
                Some(s) => if crate::executor::like_match(s, p) { TRUE } else { FALSE },
            }).collect();
            Some(if *negated { not(out) } else { out })
        }
        Expr::IsNull(x) | Expr::IsNotNull(x) => {
            let want_null = matches!(e, Expr::IsNull(_));
            let pick = |is_null: bool| if is_null == want_null { TRUE } else { FALSE };
            Some(match col_of(x, block)? {
                ColumnData::Int64(v) => v[w.lo..w.hi].iter().map(|c| pick(c.is_none())).collect(),
                ColumnData::Float64(v) => v[w.lo..w.hi].iter().map(|c| pick(c.is_none())).collect(),
                ColumnData::Bool(v) => v[w.lo..w.hi].iter().map(|c| pick(c.is_none())).collect(),
                ColumnData::Str(v) => v[w.lo..w.hi].iter().map(|c| pick(c.is_none())).collect(),
                ColumnData::StrDict { codes, .. } => codes[w.lo..w.hi].iter().map(|&c| pick(c == u8::MAX)).collect(),
            })
        }
        _ => None,
    }
}

fn windows(n: usize) -> Vec<Win> {
    (0..n).step_by(CHUNK).map(|lo| Win { lo, hi: (lo + CHUNK).min(n) }).collect()
}

/// Rows for which the predicate is TRUE (NULL counts as not matching), or None when not covered.
pub fn filter_mask(pred: &Expr, block: &DataBlock) -> Option<Vec<bool>> {
    let n = block.num_rows;
    let wins = windows(n);
    // decide coverage on the first chunk (cheap) before fanning out
    let first = tri(pred, block, *wins.first()?)?;
    let mut parts: Vec<Vec<bool>> = vec![first.into_iter().map(|v| v == TRUE).collect()];
    let rest: Vec<Option<Vec<bool>>> = wins[1..].par_iter()
        .map(|&w| tri(pred, block, w).map(|v| v.into_iter().map(|x| x == TRUE).collect()))
        .collect();
    for r in rest { parts.push(r?); }
    let mut out = Vec::with_capacity(n);
    for p in parts { out.extend(p); }
    Some(out)
}

fn num_win(e: &Expr, block: &DataBlock, w: Win) -> Option<Vec<Option<f64>>> {
    let n = w.len();
    match e {
        Expr::Int(i) => Some(vec![Some(*i as f64); n]),
        Expr::Float(f) => Some(vec![Some(*f); n]),
        Expr::Null => Some(vec![None; n]),
        Expr::Col(_) | Expr::QualCol(..) => match col_of(e, block)? {
            ColumnData::Int64(v) => Some(v[w.lo..w.hi].iter().map(|x| x.map(|i| i as f64)).collect()),
            ColumnData::Float64(v) => Some(v[w.lo..w.hi].to_vec()),
            _ => None,
        },
        Expr::BinOp { op: op @ (BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul | BinOpKind::Div | BinOpKind::Mod), left, right } => {
            let (l, r) = (num_win(left, block, w)?, num_win(right, block, w)?);
            Some(l.iter().zip(&r).map(|(a, b)| match (a, b) {
                // x / 0 and x % 0 are NULL (Spark semantics), not inf / NaN
                (Some(_), Some(b)) if *b == 0.0 && matches!(op, BinOpKind::Div | BinOpKind::Mod) => None,
                (Some(a), Some(b)) => Some(match op {
                    BinOpKind::Add => a + b,
                    BinOpKind::Sub => a - b,
                    BinOpKind::Mul => a * b,
                    BinOpKind::Div => a / b,
                    _ => a % b,
                }),
                _ => None,
            }).collect())
        }
        Expr::Case { operand: None, branches, else_val } => {
            let mut out: Vec<Option<f64>> = vec![None; n];
            let mut decided = vec![false; n];
            for (cond, val) in branches {
                let c = tri(cond, block, w)?;
                let v = num_win(val, block, w)?;
                for i in 0..n {
                    if !decided[i] && c[i] == TRUE {
                        decided[i] = true;
                        out[i] = v[i];
                    }
                }
            }
            if let Some(ev) = else_val {
                let v = num_win(ev, block, w)?;
                for i in 0..n {
                    if !decided[i] { out[i] = v[i]; }
                }
            }
            Some(out)
        }
        _ => None,
    }
}

/// Numeric value of `e` for every row (NULL = None), or None when the expression is not covered.
/// Mirrors the row interpreter: arithmetic yields floats and propagates NULL.
pub fn num_vec(e: &Expr, block: &DataBlock) -> Option<Vec<Option<f64>>> {
    let n = block.num_rows;
    let wins = windows(n);
    let first = num_win(e, block, *wins.first()?)?;
    let rest: Vec<Option<Vec<Option<f64>>>> = wins[1..].par_iter().map(|&w| num_win(e, block, w)).collect();
    let mut out = Vec::with_capacity(n);
    out.extend(first);
    for r in rest { out.extend(r?); }
    Some(out)
}
