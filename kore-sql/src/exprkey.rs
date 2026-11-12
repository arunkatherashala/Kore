//! Join keys that are expressions of ONE input: `a.rn = b.rn + 1`, `substr(a.x,1,2) = b.y`, `a.d * 2 = b.e`.
//! The expression is evaluated column-at-a-time into a temporary key column which the ordinary hash paths use.
//! Only shapes whose row-evaluator semantics are reproduced exactly are handled (integer arithmetic wraps like
//! the evaluator's, int/float mixes become DOUBLE, text only through SUBSTR of a text column); anything else
//! returns None and the caller keeps the old plan. The original equality always stays as an exact re-check.

use crate::ast::{BinOpKind, Expr};
use kore_core::{ColumnData, DataBlock};

/// Resolve a column reference the way the join code does (qualified exact, then suffix, then case-insensitive).
fn lookup<'a>(block: &'a DataBlock, e: &Expr) -> Option<&'a ColumnData> {
    let name = crate::rewrite::col_ref(e)?;
    let found = if name.contains('.') {
        block.columns.iter().find(|c| c.name == name).or_else(|| block.columns.iter().find(|c| c.name.eq_ignore_ascii_case(&name)))
    } else {
        let suffix = format!(".{name}");
        block.columns.iter().find(|c| c.name == name)
            .or_else(|| block.columns.iter().find(|c| c.name.ends_with(&suffix)))
            .or_else(|| block.columns.iter().find(|c| c.name.eq_ignore_ascii_case(&name)))
    };
    // a lazily loaded block holds empty placeholders for columns it has not filled yet
    found.filter(|c| c.data.len() == block.num_rows).map(|c| &c.data)
}

enum V<'a> { I(std::borrow::Cow<'a, [Option<i64>]>), F(std::borrow::Cow<'a, [Option<f64>]>), S(ColumnData), IC(i64), FC(f64) }

fn eval<'a>(e: &Expr, block: &'a DataBlock) -> Option<V<'a>> {
    use std::borrow::Cow;
    match e {
        Expr::Col(_) | Expr::QualCol(..) => match lookup(block, e)? {
            ColumnData::Int64(v) => Some(V::I(Cow::Borrowed(v))),
            ColumnData::Float64(v) => Some(V::F(Cow::Borrowed(v))),
            _ => None,
        },
        Expr::Int(i) => Some(V::IC(*i)),
        Expr::Float(f) => Some(V::FC(*f)),
        Expr::BinOp { op: op @ (BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul), left, right } => {
            let (l, r) = (eval(left, block)?, eval(right, block)?);
            let n = block.num_rows;
            let fop = |a: f64, b: f64| match op { BinOpKind::Add => a + b, BinOpKind::Sub => a - b, _ => a * b };
            let iop = |a: i64, b: i64| match op { BinOpKind::Add => a.wrapping_add(b), BinOpKind::Sub => a.wrapping_sub(b), _ => a.wrapping_mul(b) };
            let as_f = |v: &V<'_>| -> Option<Vec<Option<f64>>> {
                Some(match v {
                    V::I(x) => x.iter().map(|o| o.map(|i| i as f64)).collect(),
                    V::F(x) => x.to_vec(),
                    V::IC(c) => vec![Some(*c as f64); n],
                    V::FC(c) => vec![Some(*c); n],
                    V::S(_) => return None,
                })
            };
            match (&l, &r) {
                (V::IC(a), V::IC(b)) => Some(V::IC(iop(*a, *b))),
                (V::I(a), V::IC(b)) => Some(V::I(Cow::Owned(a.iter().map(|x| x.map(|x| iop(x, *b))).collect()))),
                (V::IC(a), V::I(b)) => Some(V::I(Cow::Owned(b.iter().map(|x| x.map(|x| iop(*a, x))).collect()))),
                (V::I(a), V::I(b)) => Some(V::I(Cow::Owned(a.iter().zip(b.iter()).map(|(x, y)| match (x, y) { (Some(x), Some(y)) => Some(iop(*x, *y)), _ => None }).collect()))),
                (V::S(_), _) | (_, V::S(_)) => None,
                _ => {
                    let (a, b) = (as_f(&l)?, as_f(&r)?);
                    Some(V::F(Cow::Owned(a.iter().zip(b.iter()).map(|(x, y)| match (x, y) { (Some(x), Some(y)) => Some(fop(*x, *y)), _ => None }).collect())))
                }
            }
        }
        Expr::FuncCall { name, args } if (name.eq_ignore_ascii_case("SUBSTR") || name.eq_ignore_ascii_case("SUBSTRING")) && (args.len() == 2 || args.len() == 3) => {
            let Expr::Int(start) = &args[1] else { return None };
            if *start < 1 { return None; }
            let start = (*start - 1) as usize;
            let len = match args.get(2) { None => None, Some(Expr::Int(l)) => Some((*l).max(0) as usize), _ => return None };
            let sub = |s: &str| crate::vecexpr::sub_slice(s, start, len).to_string();
            match lookup(block, &args[0])? {
                ColumnData::Str(v) => Some(V::S(ColumnData::Str(v.iter().map(|s| s.as_deref().map(sub)).collect()))),
                ColumnData::StrDict { codes, dict } => Some(V::S(ColumnData::Str(codes.iter().map(|&c| {
                    if c == u8::MAX { None } else { dict.get(c as usize).map(|s| sub(s)) }
                }).collect()))),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The expression as a key column of `block` (None = shape not covered). Constants are not keys.
pub(crate) fn key_column(e: &Expr, block: &DataBlock) -> Option<ColumnData> {
    match eval(e, block)? {
        V::I(v) => Some(ColumnData::Int64(v.into_owned())),
        V::F(v) => Some(ColumnData::Float64(v.into_owned())),
        V::S(d) => Some(d),
        V::IC(_) | V::FC(_) => None,
    }
}
