//! Rule families across columns. The source columns are listed in the rule payload header (see `mod`);
//! they are always columns that are not themselves derived from other columns.
//!
//! ```text
//! 7 int relation    nulls | op:u8 (0 a+b, 1 a-b, 2 a*b, 3 alpha*a+beta: zz zz) | residual zigzag varints
//!                   (target minus prediction, wrapping; prediction 0 where a source is null)
//! 8 float relation  nulls | op:u8 (0 a*b, 1 a+b, 2 a-b, 3 a/b; int sources converted with `as f64`) | round:u8
//!                   (0xFF none, else s: prediction rounded half away from zero to s decimals) | xor residual (u64 planes)
//! 9 lookup          kind:u8 (0 shift k: varint, target[i] = a[i-k]; 1 functional dependency: n_keys | map =
//!                   embedded v4 column, one target value per distinct source value in first-appearance order)
//!                   | n_exc | exception rows (delta varints) | exception values (embedded v4 column)
//! ```

use kore_core::{ColumnData, KoreError};
use super::stream::{self, bad, put_varint, put_zz, Levels, Rd};
use crate::DType;

pub const INT_REL: u8 = 7;
pub const FLOAT_REL: u8 = 8;
pub const LOOKUP: u8 = 9;

const POW10: [f64; 16] = [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15];
const MAX_SHIFT: usize = 8;
/// Columns beyond this many are not searched for relations (the pair/triple search grows quadratically).
pub const MAX_CROSS_COLS: usize = 64;

/// A fully encoded cross-column candidate.
pub struct Cand { pub target: usize, pub family: u8, pub deps: Vec<usize>, pub body: Vec<u8> }

fn sample_rows(n: usize) -> Vec<usize> {
    if n <= 4096 { return (0..n).collect(); }
    let mut r: Vec<usize> = (0..2048).collect();
    let step = n / 2048;
    r.extend((0..2048).map(|k| 2048 + k * (n - 2048) / 2048).filter(|&i| i < n));
    let _ = step;
    r
}

// ── int relations ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
enum IOp { Sum, Diff, Prod, Affine(i64, i64) }

fn ipred(op: IOp, a: Option<i64>, b: Option<i64>) -> i64 {
    match op {
        IOp::Sum => match (a, b) { (Some(a), Some(b)) => a.wrapping_add(b), _ => 0 },
        IOp::Diff => match (a, b) { (Some(a), Some(b)) => a.wrapping_sub(b), _ => 0 },
        IOp::Prod => match (a, b) { (Some(a), Some(b)) => a.wrapping_mul(b), _ => 0 },
        IOp::Affine(al, be) => a.map_or(0, |a| al.wrapping_mul(a).wrapping_add(be)),
    }
}

fn int_body(t: &[Option<i64>], a: &[Option<i64>], b: Option<&[Option<i64>]>, op: IOp, lv: Levels) -> Vec<u8> {
    let mut out = vec![];
    stream::put_nulls(&mut out, &t.iter().map(|x| x.is_none()).collect::<Vec<_>>(), lv);
    match op {
        IOp::Sum => out.push(0), IOp::Diff => out.push(1), IOp::Prod => out.push(2),
        IOp::Affine(al, be) => { out.push(3); put_zz(&mut out, al); put_zz(&mut out, be); }
    }
    let mut r = Vec::with_capacity(t.len() * 2);
    for i in 0..t.len() {
        if let Some(x) = t[i] { put_zz(&mut r, x.wrapping_sub(ipred(op, a[i], b.map_or(None, |b| b[i])))); }
    }
    stream::put_stream(&mut out, &r, lv);
    out
}

fn int_decode(rd: &mut Rd, n: usize, deps: &[&ColumnData]) -> Result<ColumnData, KoreError> {
    let nulls = stream::get_nulls(rd, n)?;
    let op = match rd.u8()? { 0 => IOp::Sum, 1 => IOp::Diff, 2 => IOp::Prod, 3 => IOp::Affine(rd.zz()?, rd.zz()?), _ => return Err(bad("unknown int relation")) };
    let want = if matches!(op, IOp::Affine(..)) { 1 } else { 2 };
    if deps.len() != want { return Err(bad("wrong number of source columns")); }
    let src = |k: usize| match deps[k] { ColumnData::Int64(v) => Ok(v), _ => Err(bad("int relation needs int sources")) };
    let a = src(0)?;
    let b = if want == 2 { Some(src(1)?) } else { None };
    let m = nulls.iter().filter(|&&x| !x).count();
    let r = stream::get_zz_stream(rd, m)?;
    let mut k = 0;
    Ok(ColumnData::Int64((0..n).map(|i| if nulls[i] { None } else {
        let x = ipred(op, a[i], b.map_or(None, |b| b[i])).wrapping_add(r[k]);
        k += 1;
        Some(x)
    }).collect()))
}

// ── float relations ──────────────────────────────────────────────────────────

fn as_f64(c: &ColumnData) -> Option<Vec<Option<f64>>> {
    match c {
        ColumnData::Float64(v) => Some(v.clone()),
        ColumnData::Int64(v) => Some(v.iter().map(|x| x.map(|x| x as f64)).collect()),
        _ => None,
    }
}

fn fpred(op: u8, round: u8, a: Option<f64>, b: Option<f64>) -> u64 {
    let (a, b) = match (a, b) { (Some(a), Some(b)) => (a, b), _ => return 0 };
    let x = match op { 0 => a * b, 1 => a + b, 2 => a - b, _ => a / b };
    let x = if round == 0xFF { x } else { let p = POW10[round as usize]; (x * p).round() / p };
    x.to_bits()
}

fn float_body(t: &[Option<f64>], a: &[Option<f64>], b: &[Option<f64>], op: u8, round: u8, lv: Levels) -> Vec<u8> {
    let mut out = vec![];
    stream::put_nulls(&mut out, &t.iter().map(|x| x.is_none()).collect::<Vec<_>>(), lv);
    out.push(op);
    out.push(round);
    let r: Vec<u64> = (0..t.len()).filter_map(|i| t[i].map(|x| x.to_bits() ^ fpred(op, round, a[i], b[i]))).collect();
    stream::put_u64_stream(&mut out, &r, lv);
    out
}

fn float_decode(rd: &mut Rd, n: usize, deps: &[&ColumnData]) -> Result<ColumnData, KoreError> {
    let nulls = stream::get_nulls(rd, n)?;
    let op = rd.u8()?;
    let round = rd.u8()?;
    if op > 3 || (round != 0xFF && round as usize >= POW10.len()) { return Err(bad("unknown float relation")); }
    if deps.len() != 2 { return Err(bad("wrong number of source columns")); }
    let a = as_f64(deps[0]).ok_or_else(|| bad("float relation needs numeric sources"))?;
    let b = as_f64(deps[1]).ok_or_else(|| bad("float relation needs numeric sources"))?;
    let m = nulls.iter().filter(|&&x| !x).count();
    let r = stream::get_u64_stream(rd, m)?;
    let mut k = 0;
    Ok(ColumnData::Float64((0..n).map(|i| if nulls[i] { None } else {
        let x = f64::from_bits(fpred(op, round, a[i], b[i]) ^ r[k]);
        k += 1;
        Some(x)
    }).collect()))
}

// ── lookup: shift and functional dependency ──────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Cell<'a> { Null, I(i64), F(u64), B(bool), S(&'a str) }

fn cells(c: &ColumnData) -> Option<Vec<Cell<'_>>> {
    Some(match c {
        ColumnData::Int64(v) => v.iter().map(|x| x.map_or(Cell::Null, Cell::I)).collect(),
        ColumnData::Float64(v) => v.iter().map(|x| x.map_or(Cell::Null, |f| Cell::F(f.to_bits()))).collect(),
        ColumnData::Bool(v) => v.iter().map(|x| x.map_or(Cell::Null, Cell::B)).collect(),
        ColumnData::Str(v) => v.iter().map(|x| x.as_deref().map_or(Cell::Null, Cell::S)).collect(),
        ColumnData::StrDict { .. } => return None,
    })
}

fn to_column(dtype: DType, c: &[Cell]) -> Result<ColumnData, KoreError> {
    let wrong = || bad("lookup value of the wrong type");
    Ok(match dtype {
        DType::I64 => ColumnData::Int64(c.iter().map(|x| match x { Cell::Null => Ok(None), Cell::I(v) => Ok(Some(*v)), _ => Err(wrong()) }).collect::<Result<_, _>>()?),
        DType::F64 => ColumnData::Float64(c.iter().map(|x| match x { Cell::Null => Ok(None), Cell::F(v) => Ok(Some(f64::from_bits(*v))), _ => Err(wrong()) }).collect::<Result<_, _>>()?),
        DType::Bool => ColumnData::Bool(c.iter().map(|x| match x { Cell::Null => Ok(None), Cell::B(v) => Ok(Some(*v)), _ => Err(wrong()) }).collect::<Result<_, _>>()?),
        DType::Str => ColumnData::Str(c.iter().map(|x| match x { Cell::Null => Ok(None), Cell::S(v) => Ok(Some(v.to_string())), _ => Err(wrong()) }).collect::<Result<_, _>>()?),
        _ => return Err(bad("lookup target type not supported")),
    })
}

fn same_kind(a: &ColumnData, b: &ColumnData) -> bool { std::mem::discriminant(a) == std::mem::discriminant(b) }

/// First-appearance key list and per key the most frequent target (ties: first seen).
fn fd_map<'a>(src: &[Cell<'a>], tgt: &[Cell<'a>], rows: impl Iterator<Item = usize>, max_keys: usize) -> Option<(Vec<Cell<'a>>, std::collections::HashMap<Cell<'a>, usize>, Vec<Cell<'a>>)> {
    use std::collections::HashMap;
    let mut key_id: HashMap<Cell, usize> = HashMap::new();
    let mut keys = vec![];
    let mut counts: Vec<HashMap<Cell, (u32, usize)>> = vec![];
    for (ord, i) in rows.enumerate() {
        let n = key_id.len();
        let k = *key_id.entry(src[i]).or_insert(n);
        if k == keys.len() { keys.push(src[i]); counts.push(HashMap::new()); if keys.len() > max_keys { return None; } }
        let e = counts[k].entry(tgt[i]).or_insert((0, ord));
        e.0 += 1;
    }
    let vals = counts.iter().map(|m| *m.iter().max_by(|a, b| a.1.0.cmp(&b.1.0).then(b.1.1.cmp(&a.1.1))).unwrap().0).collect();
    Some((keys, key_id, vals))
}

fn lookup_body(tgt_col: &ColumnData, t: &[Cell], pred: &[Cell], head: Vec<u8>, lv: Levels) -> Result<Vec<u8>, KoreError> {
    let mut out = head;
    let exc: Vec<usize> = (0..t.len()).filter(|&i| t[i] != pred[i]).collect();
    put_varint(&mut out, exc.len() as u64);
    if !exc.is_empty() {
        let mut pb = vec![];
        let mut prev = 0usize;
        for (j, &i) in exc.iter().enumerate() { put_varint(&mut pb, (if j == 0 { i } else { i - prev - 1 }) as u64); prev = i; }
        stream::put_stream(&mut out, &pb, lv);
        let vals: Vec<Cell> = exc.iter().map(|&i| t[i]).collect();
        stream::put_v4col(&mut out, &to_column(crate::codec::dtype_of(tgt_col), &vals)?, lv)?;
    }
    Ok(out)
}

fn shifted<T: Clone>(v: &[Option<T>], k: usize) -> Vec<Option<T>> {
    (0..v.len()).map(|i| if i >= k { v[i - k].clone() } else { None }).collect()
}

fn lookup_decode(rd: &mut Rd, dtype: DType, n: usize, deps: &[&ColumnData]) -> Result<ColumnData, KoreError> {
    if deps.len() != 1 { return Err(bad("wrong number of source columns")); }
    if deps[0].len() != n { return Err(bad("source column length")); }
    let col = match rd.u8()? {
        0 => {
            let k = rd.count(MAX_SHIFT)?;
            if crate::codec::dtype_of(deps[0]) != dtype { return Err(bad("shift source type differs")); }
            match deps[0] {
                ColumnData::Int64(v) => ColumnData::Int64(shifted(v, k)),
                ColumnData::Float64(v) => ColumnData::Float64(shifted(v, k)),
                ColumnData::Bool(v) => ColumnData::Bool(shifted(v, k)),
                ColumnData::Str(v) => ColumnData::Str(shifted(v, k)),
                _ => return Err(bad("lookup source type not supported")),
            }
        }
        1 => {
            let src = cells(deps[0]).ok_or_else(|| bad("lookup source type not supported"))?;
            let nk = rd.count(n)?;
            let map_col = stream::get_v4col(rd, dtype, nk)?;
            let vals = cells(&map_col).ok_or_else(|| bad("map type"))?;
            let mut key_id: std::collections::HashMap<Cell, usize> = std::collections::HashMap::new();
            let mut pred = Vec::with_capacity(n);
            for &s in &src {
                let len = key_id.len();
                let k = *key_id.entry(s).or_insert(len);
                pred.push(*vals.get(k).ok_or_else(|| bad("more source values than map entries"))?);
            }
            if key_id.len() != nk { return Err(bad("map size does not match the source column")); }
            to_column(dtype, &pred)?
        }
        _ => return Err(bad("unknown lookup kind")),
    };
    let ne = rd.count(n)?;
    finish(rd, n, col, ne)
}

fn finish(rd: &mut Rd, n: usize, mut col: ColumnData, ne: usize) -> Result<ColumnData, KoreError> {
    let dtype = crate::codec::dtype_of(&col);
    if ne > 0 {
        let gaps = stream::get_uint_stream(rd, ne, n as u64)?;
        let mut pos = Vec::with_capacity(ne);
        let mut prev: Option<usize> = None;
        for g in gaps {
            let i = match prev { None => g as usize, Some(q) => q.checked_add(1 + g as usize).ok_or_else(|| bad("overflow"))? };
            if i >= n { return Err(bad("exception row out of range")); }
            pos.push(i);
            prev = Some(i);
        }
        let ex = stream::get_v4col(rd, dtype, ne)?;
        match (&mut col, ex) {
            (ColumnData::Int64(c), ColumnData::Int64(e)) => for (p, v) in pos.iter().zip(e) { c[*p] = v; },
            (ColumnData::Float64(c), ColumnData::Float64(e)) => for (p, v) in pos.iter().zip(e) { c[*p] = v; },
            (ColumnData::Bool(c), ColumnData::Bool(e)) => for (p, v) in pos.iter().zip(e) { c[*p] = v; },
            (ColumnData::Str(c), ColumnData::Str(e)) => for (p, v) in pos.iter().zip(e) { c[*p] = v; },
            _ => return Err(bad("exception values of the wrong type")),
        }
    }
    Ok(col)
}


pub fn decode(family: u8, rd: &mut Rd, dtype: DType, n: usize, deps: &[&ColumnData]) -> Result<ColumnData, KoreError> {
    let need = |want: DType| if dtype == want { Ok(()) } else { Err(bad("rule family does not match the column type")) };
    match family {
        INT_REL => { need(DType::I64)?; int_decode(rd, n, deps) }
        FLOAT_REL => { need(DType::F64)?; float_decode(rd, n, deps) }
        LOOKUP => lookup_decode(rd, dtype, n, deps),
        f => Err(bad(format!("unknown rule family {f}"))),
    }
}

pub fn describe(family: u8, rd: &mut Rd, n: usize, names: &[&str]) -> String {
    let nm = |k: usize| names.get(k).copied().unwrap_or("?").to_string();
    match family {
        INT_REL => {
            if stream::get_nulls(rd, n).is_err() { return "?".into(); }
            match rd.u8() {
                Ok(0) => format!("= {} + {}", nm(0), nm(1)), Ok(1) => format!("= {} - {}", nm(0), nm(1)),
                Ok(2) => format!("= {} * {}", nm(0), nm(1)),
                Ok(3) => { let a = rd.zz().unwrap_or(0); let b = rd.zz().unwrap_or(0); if a == 1 && b == 0 { format!("= {} + offset residual", nm(0)) } else { format!("= {a}*{} {b:+}", nm(0)) } }
                _ => "?".into(),
            }
        }
        FLOAT_REL => {
            if stream::get_nulls(rd, n).is_err() { return "?".into(); }
            let op = rd.u8().unwrap_or(9);
            let round = rd.u8().unwrap_or(0xFF);
            let o = ["*", "+", "-", "/"].get(op as usize).copied().unwrap_or("?");
            let r = if round == 0xFF { String::new() } else { format!(", rounded to {round} decimals") };
            format!("= {} {o} {} (f64){r}", nm(0), nm(1))
        }
        LOOKUP => match rd.u8() {
            Ok(0) => format!("= {} shifted by {}", nm(0), rd.varint().unwrap_or(0)),
            Ok(1) => format!("= lookup({}) ({} keys)", nm(0), rd.varint().unwrap_or(0)),
            _ => "?".into(),
        },
        _ => "?".into(),
    }
}

// ── search ───────────────────────────────────────────────────────────────────

/// Relation candidates encoded in full per target and family group: the best by sample hit rate.
const TOP_RELATIONS: usize = 8;

/// The `TOP_RELATIONS` candidates with the highest sample hit rate (ties: search order), encoded.
fn top<'a>(mut lazy: Vec<(f64, Box<dyn FnOnce() -> Cand + 'a>)>) -> Vec<Cand> {
    let mut idx: Vec<usize> = (0..lazy.len()).collect();
    idx.sort_by(|&x, &y| lazy[y].0.partial_cmp(&lazy[x].0).unwrap().then(x.cmp(&y)));
    idx.truncate(TOP_RELATIONS);
    idx.sort_unstable();
    let mut out = vec![];
    for (k, (_, f)) in lazy.drain(..).enumerate() { if idx.contains(&k) { out.push(f()); } }
    out
}

fn hit_rate(rows: &[usize], t: impl Fn(usize) -> Option<bool>) -> f64 {
    let (mut h, mut k) = (0usize, 0usize);
    for &i in rows { if let Some(x) = t(i) { k += 1; h += x as usize; } }
    if k == 0 { 0.0 } else { h as f64 / k as f64 }
}

fn find_scale(v: &[Option<f64>], rows: &[usize]) -> Option<u8> {
    let s: Vec<f64> = rows.iter().filter_map(|&i| v[i]).collect();
    if s.is_empty() { return None; }
    (0..POW10.len()).find(|&k| {
        let p = POW10[k];
        s.iter().filter(|&&x| { let y = x * p; y.is_finite() && y.abs() < 9e15 && ((y.round() as i64) as f64 / p).to_bits() == x.to_bits() }).count() * 8 >= s.len() * 7
    }).map(|k| k as u8)
}

/// Every cross-column candidate that passes a sample test (at least half of the sampled target rows
/// predicted exactly; for lookups at most 1/8 distinct keys and 80 % consistency), encoded in full.
pub fn search(cols: &[&ColumnData], lv: Levels) -> Vec<Cand> {
    use rayon::prelude::*;
    let k = cols.len().min(MAX_CROSS_COLS);
    if k < 2 { return vec![]; }
    let n = cols[0].len();
    if n == 0 { return vec![]; }
    let rows = sample_rows(n);
    let cell: Vec<Option<Vec<Cell>>> = cols[..k].iter().map(|c| cells(c)).collect();
    (0..k).into_par_iter().flat_map_iter(|t| {
        let mut out: Vec<Cand> = vec![];
        let tc = cols[t];
        // int relations
        if let ColumnData::Int64(tv) = tc {
            let ints: Vec<usize> = (0..k).filter(|&j| j != t && matches!(cols[j], ColumnData::Int64(_))).collect();
            let iv = |j: usize| match cols[j] { ColumnData::Int64(v) => v, _ => unreachable!() };
            // the target's own delta stream on the sample: the bar an offset relation must clear
            let own: usize = {
                let mut prev = 0i64;
                let mut r = vec![];
                for &i in &rows { if let Some(y) = tv[i] { stream::put_zz(&mut r, y.wrapping_sub(prev)); prev = y; } }
                stream::score(&r)
            };
            let mut lazy: Vec<(f64, Box<dyn FnOnce() -> Cand + '_>)> = vec![];
            for (x, &a) in ints.iter().enumerate() {
                let av = iv(a);
                // offset relation t = a + r (an order rule such as ship_date = sold_date + lag): kept as a
                // candidate when the residuals t - a on the sample code to under 80 % of the target's deltas
                {
                    let mut r = vec![];
                    for &i in &rows { if let Some(y) = tv[i] { stream::put_zz(&mut r, y.wrapping_sub(av[i].unwrap_or(0))); } }
                    if stream::score(&r) * 10 < own * 8 {
                        lazy.push((0.25, Box::new(move || Cand { target: t, family: INT_REL, deps: vec![a], body: int_body(tv, av, None, IOp::Affine(1, 0), lv) })));
                    }
                }
                // affine of one column, fitted on two sample rows with different source values
                let pts: Vec<(i64, i64)> = rows.iter().filter_map(|&i| match (av[i], tv[i]) { (Some(p), Some(q)) => Some((p, q)), _ => None }).take(64).collect();
                if let Some(&(x0, y0)) = pts.first() {
                    if let Some(&(x1, y1)) = pts.iter().find(|p| p.0 != x0) {
                        let (dx, dy) = (x1 as i128 - x0 as i128, y1 as i128 - y0 as i128);
                        if dy % dx == 0 {
                            let al = dy / dx;
                            let be = y0 as i128 - al * x0 as i128;
                            if let (Ok(al), Ok(be)) = (i64::try_from(al), i64::try_from(be)) {
                                let op = IOp::Affine(al, be);
                                let h = hit_rate(&rows, |i| tv[i].map(|y| y == ipred(op, av[i], None)));
                                if !(al == 1 && be == 0) && h >= 0.5 {
                                    lazy.push((h, Box::new(move || Cand { target: t, family: INT_REL, deps: vec![a], body: int_body(tv, av, None, op, lv) })));
                                }
                            }
                        }
                    }
                }
                for &b in &ints[x..] {
                    let mut ops = vec![(IOp::Diff, a, b)];
                    if a != b { ops.push((IOp::Diff, b, a)); }
                    ops.push((IOp::Sum, a, b));
                    ops.push((IOp::Prod, a, b));
                    for (op, p, q) in ops {
                        if p == q && matches!(op, IOp::Diff) { continue; }
                        let (pv, qv) = (iv(p), iv(q));
                        let h = hit_rate(&rows, |i| tv[i].map(|y| y == ipred(op, pv[i], qv[i])));
                        if h >= 0.5 {
                            lazy.push((h, Box::new(move || Cand { target: t, family: INT_REL, deps: vec![p, q], body: int_body(tv, pv, Some(qv), op, lv) })));
                        }
                    }
                }
            }
            out.extend(top(lazy));
        }
        // float relations
        if let ColumnData::Float64(tv) = tc {
            let nums: Vec<usize> = (0..k).filter(|&j| j != t && matches!(cols[j], ColumnData::Int64(_) | ColumnData::Float64(_))).collect();
            let fv: Vec<Vec<Option<f64>>> = nums.iter().map(|&j| as_f64(cols[j]).unwrap()).collect();
            let mut rounds = vec![0xFFu8];
            if let Some(s) = find_scale(tv, &rows) { rounds.push(s); }
            let mut lazy: Vec<(f64, Box<dyn FnOnce() -> Cand + '_>)> = vec![];
            for x in 0..nums.len() {
                for y in 0..nums.len() {
                    for op in 0u8..4 {
                        // + and * are symmetric: only x <= y
                        if (op == 0 || op == 1) && y < x { continue; }
                        if x == y && op >= 2 { continue; }
                        // the rounding variant with the most exact rows on the sample (ties: unrounded)
                        let (a, b) = (&fv[x], &fv[y]);
                        let mut best: Option<(f64, u8)> = None;
                        for &rd in &rounds {
                            let h = hit_rate(&rows, |i| tv[i].map(|z| z.to_bits() == fpred(op, rd, a[i], b[i])));
                            if h >= 0.5 && best.map_or(true, |(bh, _)| h > bh) { best = Some((h, rd)); }
                        }
                        if let Some((h, rd)) = best {
                            let deps = vec![nums[x], nums[y]];
                            lazy.push((h, Box::new(move || Cand { target: t, family: FLOAT_REL, deps, body: float_body(tv, a, b, op, rd, lv) })));
                        }
                    }
                }
            }
            out.extend(top(lazy));
        }
        // lookups
        if let Some(tcell) = &cell[t] {
            // a constant target gains nothing from a lookup (its own encoding is already a few bytes)
            let constant = rows.iter().all(|&i| tcell[i] == tcell[rows[0]]);
            for a in (0..k).filter(|_| !constant) {
                if a == t { continue; }
                let Some(sc) = &cell[a] else { continue };
                if same_kind(tc, cols[a]) {
                    for s in 0..=MAX_SHIFT.min(n.saturating_sub(1)) {
                        if hit_rate(&rows, |i| if i >= s { Some(tcell[i] == sc[i - s]) } else { None }) >= 0.5 {
                            let pred: Vec<Cell> = (0..n).map(|i| if i >= s { sc[i - s] } else { Cell::Null }).collect();
                            let mut head = vec![0u8];
                            put_varint(&mut head, s as u64);
                            if let Ok(b) = lookup_body(tc, tcell, &pred, head, lv) { out.push(Cand { target: t, family: LOOKUP, deps: vec![a], body: b }); }
                            break;
                        }
                    }
                }
                // functional dependency
                let Some((_, sid, svals)) = fd_map(sc, tcell, rows.iter().copied(), rows.len() / 8 + 1) else { continue };
                if hit_rate(&rows, |i| Some(tcell[i] == svals[sid[&sc[i]]])) < 0.8 { continue; }
                let Some((keys, key_id, vals)) = fd_map(sc, tcell, 0..n, n / 4 + 1) else { continue };
                let pred: Vec<Cell> = sc.iter().map(|s| vals[key_id[s]]).collect();
                let hits = (0..n).filter(|&i| pred[i] == tcell[i]).count();
                if hits * 10 < n * 8 { continue; }
                let mut head = vec![1u8];
                put_varint(&mut head, keys.len() as u64);
                let Ok(map) = to_column(crate::codec::dtype_of(tc), &vals) else { continue };
                if stream::put_v4col(&mut head, &map, lv).is_err() { continue; }
                if let Ok(b) = lookup_body(tc, tcell, &pred, head, lv) { out.push(Cand { target: t, family: LOOKUP, deps: vec![a], body: b }); }
            }
        }
        out
    }).collect()
}
