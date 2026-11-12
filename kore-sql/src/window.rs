//! Window functions with Spark SQL semantics: partitions, ORDER BY with NULLS FIRST/LAST, peers, the default
//! frame (`RANGE UNBOUNDED PRECEDING .. CURRENT ROW` when ordered, the whole partition otherwise), explicit
//! `ROWS` and `RANGE` frames (numeric offsets), ranking / distribution functions, LAG/LEAD with defaults,
//! FIRST/LAST/NTH_VALUE and every aggregate. All of it works on `ExprVal` through an evaluation callback.

use std::cmp::Ordering;
use std::collections::HashMap;

use kore_core::KoreError;

use crate::aggs::{self, AggInput};
use crate::ast::{Expr, FrameBound, FrameMode, OrderByItem, WindowFn, WindowSpec};
use crate::executor::ExprVal;
use crate::scalar::num;

type V = ExprVal;

fn err(msg: impl Into<String>) -> KoreError { KoreError::InvalidArgument(msg.into()) }

/// Compare two sort-key values honouring DESC and the NULL placement (default: NULLs first ascending, last descending).
pub fn cmp_keys(a: &V, b: &V, item: &OrderByItem) -> Ordering {
    let nulls_first = item.nulls_first.unwrap_or(!item.desc);
    match (matches!(a, V::Null), matches!(b, V::Null)) {
        (true, true) => Ordering::Equal,
        (true, false) => if nulls_first { Ordering::Less } else { Ordering::Greater },
        (false, true) => if nulls_first { Ordering::Greater } else { Ordering::Less },
        (false, false) => {
            let o = aggs::total_cmp(a, b);
            if item.desc { o.reverse() } else { o }
        }
    }
}

fn const_i64(e: &Expr, ev: &mut dyn FnMut(&Expr, usize) -> V) -> Result<i64, KoreError> {
    match ev(e, 0) {
        V::Int(i) if i >= 0 => Ok(i),
        V::Float(f) if f >= 0.0 && f.fract() == 0.0 => Ok(f as i64),
        other => Err(err(format!("window frame offset must be a non-negative integer, got {:?}", other))),
    }
}

struct SlowPart {
    /// original row indices in window order
    rows: Vec<usize>,
    /// peer group start / end (positions, inclusive) for each position
    peer_lo: Vec<usize>,
    peer_hi: Vec<usize>,
}

/// Reference (original, quadratic) implementation, kept for A/B testing (`testing::set_slow_window`).
fn evaluate_slow(func: &WindowFn, spec: &WindowSpec, n: usize, ev: &mut dyn FnMut(&Expr, usize) -> V) -> Result<Vec<V>, KoreError> {
    // ── partitions ──
    let mut part_index: HashMap<String, usize> = HashMap::new();
    let mut part_rows: Vec<Vec<usize>> = Vec::new();
    for r in 0..n {
        let key = if spec.partition_by.is_empty() { String::new() } else {
            let vals: Vec<V> = spec.partition_by.iter().map(|e| ev(e, r)).collect();
            aggs::row_key(&vals)
        };
        let idx = *part_index.entry(key).or_insert_with(|| { part_rows.push(Vec::new()); part_rows.len() - 1 });
        part_rows[idx].push(r);
    }

    // ── ordering ──
    let okeys: Vec<Vec<V>> = spec.order_by.iter().map(|o| (0..n).map(|r| ev(&o.expr, r)).collect()).collect();
    let cmp_rows = |a: usize, b: usize| -> Ordering {
        for (k, item) in spec.order_by.iter().enumerate() {
            let o = cmp_keys(&okeys[k][a], &okeys[k][b], item);
            if o != Ordering::Equal { return o; }
        }
        Ordering::Equal
    };
    let mut parts: Vec<SlowPart> = Vec::with_capacity(part_rows.len());
    for mut rows in part_rows {
        if !spec.order_by.is_empty() { rows.sort_by(|&a, &b| cmp_rows(a, b)); }
        let m = rows.len();
        let mut peer_lo = vec![0usize; m];
        let mut peer_hi = vec![m.saturating_sub(1); m];
        if !spec.order_by.is_empty() {
            for p in 1..m { peer_lo[p] = if cmp_rows(rows[p - 1], rows[p]) == Ordering::Equal { peer_lo[p - 1] } else { p }; }
            for p in (0..m.saturating_sub(1)).rev() { peer_hi[p] = if cmp_rows(rows[p], rows[p + 1]) == Ordering::Equal { peer_hi[p + 1] } else { p }; }
        }
        parts.push(SlowPart { rows, peer_lo, peer_hi });
    }

    let mut out: Vec<V> = vec![V::Null; n];

    // ── frame helper ──
    let frame_spec = spec.frame.clone();
    let ordered = !spec.order_by.is_empty();
    // RANGE offsets are numbers (`10 PRECEDING`) or calendar intervals (`INTERVAL 7 DAYS PRECEDING` over a date)
    #[derive(Clone, Copy)]
    enum Off { None, Num(i64), Interval(i64, &'static str) }
    let parse_off = |b: &FrameBound, ev: &mut dyn FnMut(&Expr, usize) -> V| -> Result<Off, KoreError> {
        match b {
            FrameBound::Preceding(e) | FrameBound::Following(e) => match e.as_ref() {
                Expr::FuncCall { name, args } if name == "INTERVAL" => {
                    let k = const_i64_f(&args[0], ev)?;
                    let unit = match args.get(1) { Some(Expr::Str(u)) => crate::datetime::norm_unit(u), _ => None }
                        .ok_or_else(|| err("unsupported interval unit in window frame"))?;
                    Ok(Off::Interval(k, unit))
                }
                other => Ok(Off::Num(const_i64_f(other, ev)?)),
            },
            _ => Ok(Off::None),
        }
    };
    let (start_off, end_off) = match &frame_spec {
        Some(f) => (parse_off(&f.start, ev)?, parse_off(&f.end, ev)?),
        None => (Off::None, Off::None),
    };
    let interval_frame = matches!(start_off, Off::Interval(..)) || matches!(end_off, Off::Interval(..));
    // dates behind an interval frame, per row
    let range_dts: Vec<Option<crate::datetime::Dt>> = if interval_frame && spec.order_by.len() == 1 {
        okeys[0].iter().map(crate::scalar::to_dt).collect()
    } else { Vec::new() };
    // order key transformed so that it ascends in window order (descending keys are negated)
    let range_keys: Option<Vec<Option<f64>>> = match &frame_spec {
        Some(f) if f.mode == FrameMode::Range
            && !matches!((&f.start, &f.end), (FrameBound::UnboundedPreceding | FrameBound::CurrentRow, FrameBound::UnboundedFollowing | FrameBound::CurrentRow)) => {
            if spec.order_by.len() != 1 { return Err(err("RANGE frame with an offset needs exactly one ORDER BY expression")); }
            let desc = spec.order_by[0].desc;
            let mut ks = Vec::with_capacity(n);
            for (r, v) in okeys[0].iter().enumerate() {
                match v {
                    V::Null => ks.push(None),
                    other => {
                        let x = if interval_frame {
                            range_dts[r].map(|d| d.epoch_secs() as f64)
                                .ok_or_else(|| err("an INTERVAL RANGE frame needs a date or timestamp ORDER BY expression"))?
                        } else {
                            num(other).ok_or_else(|| err("RANGE frame with an offset needs a numeric ORDER BY expression"))?
                        };
                        ks.push(Some(if desc { -x } else { x }));
                    }
                }
            }
            Some(ks)
        }
        _ => None,
    };
    // frame boundary value (in the transformed key space) `off` before / after the current row
    let desc_key = spec.order_by.first().map_or(false, |o| o.desc);
    let shift = |off: Off, preceding: bool, row: usize, c: f64| -> f64 {
        match off {
            Off::None => c,
            Off::Num(k) => if preceding { c - k as f64 } else { c + k as f64 },
            Off::Interval(k, unit) => {
                let Some(d) = range_dts.get(row).copied().flatten() else { return c };
                let sign = if preceding { -1 } else { 1 };
                let signed = if desc_key { -sign } else { sign };
                let moved = crate::datetime::add_unit(&d, unit, signed * k).unwrap_or(d);
                let secs = moved.epoch_secs() as f64;
                if desc_key { -secs } else { secs }
            }
        }
    };
    let rows_off = |o: Off| -> i64 { match o { Off::Num(k) => k, _ => 0 } };

    // Frame [lo, hi] (positions) of position p in partition `part`; None when empty.
    let frame_of = |part: &SlowPart, p: usize| -> Option<(usize, usize)> {
        let m = part.rows.len() as i64;
        let pi = p as i64;
        let (lo, hi) = match &frame_spec {
            None => if ordered { (0, part.peer_hi[p] as i64) } else { (0, m - 1) },
            Some(f) => match f.mode {
                FrameMode::Rows => {
                    let lo = match &f.start {
                        FrameBound::UnboundedPreceding => 0,
                        FrameBound::Preceding(_) => pi - rows_off(start_off),
                        FrameBound::CurrentRow => pi,
                        FrameBound::Following(_) => pi + rows_off(start_off),
                        FrameBound::UnboundedFollowing => m,
                    };
                    let hi = match &f.end {
                        FrameBound::UnboundedPreceding => -1,
                        FrameBound::Preceding(_) => pi - rows_off(end_off),
                        FrameBound::CurrentRow => pi,
                        FrameBound::Following(_) => pi + rows_off(end_off),
                        FrameBound::UnboundedFollowing => m - 1,
                    };
                    (lo, hi)
                }
                FrameMode::Groups => {
                    // frame measured in peer groups (rows with equal ORDER BY keys): walk group by group from the current one
                    let mi = m as usize;
                    let back = |start: usize, k: i64| -> Option<usize> {
                        let mut x = start;
                        for _ in 0..k { if x == 0 { return None; } x = part.peer_lo[x - 1]; }
                        Some(x)
                    };
                    let fwd = |end: usize, k: i64| -> Option<usize> {
                        let mut x = end;
                        for _ in 0..k { if x + 1 >= mi { return None; } x = part.peer_hi[x + 1]; }
                        Some(x)
                    };
                    let lo = match &f.start {
                        FrameBound::UnboundedPreceding => 0,
                        FrameBound::CurrentRow => part.peer_lo[p] as i64,
                        FrameBound::Preceding(_) => back(part.peer_lo[p], rows_off(start_off)).map_or(0, |x| x as i64),
                        FrameBound::Following(_) => fwd(part.peer_hi[p], rows_off(start_off)).map_or(m, |x| part.peer_lo[x] as i64),
                        FrameBound::UnboundedFollowing => m,
                    };
                    let hi = match &f.end {
                        FrameBound::UnboundedPreceding => -1,
                        FrameBound::CurrentRow => part.peer_hi[p] as i64,
                        FrameBound::Preceding(_) => back(part.peer_lo[p], rows_off(end_off)).map_or(-1, |x| part.peer_hi[x] as i64),
                        FrameBound::Following(_) => fwd(part.peer_hi[p], rows_off(end_off)).map_or(m - 1, |x| x as i64),
                        FrameBound::UnboundedFollowing => m - 1,
                    };
                    (lo, hi)
                }
                FrameMode::Range => match &range_keys {
                    None => {
                        let lo = match &f.start { FrameBound::UnboundedPreceding => 0, FrameBound::CurrentRow => part.peer_lo[p] as i64, _ => 0 };
                        let hi = match &f.end { FrameBound::UnboundedFollowing => m - 1, FrameBound::CurrentRow => part.peer_hi[p] as i64, _ => m - 1 };
                        (lo, hi)
                    }
                    Some(ks) => {
                        let cur = ks[part.rows[p]];
                        match cur {
                            // a NULL key only has the NULL peers in its frame
                            None => (part.peer_lo[p] as i64, part.peer_hi[p] as i64),
                            Some(c) => {
                                // non-NULL positions form one contiguous run
                                let keys: Vec<Option<f64>> = part.rows.iter().map(|&r| ks[r]).collect();
                                let nn_lo = keys.iter().position(|k| k.is_some()).unwrap_or(0);
                                let nn_hi = keys.iter().rposition(|k| k.is_some()).unwrap_or(0);
                                let kv = |i: usize| keys[i].unwrap_or(0.0);
                                let first_ge = |bound: f64| -> i64 {
                                    let (mut a, mut b) = (nn_lo, nn_hi + 1);
                                    while a < b { let mid = (a + b) / 2; if kv(mid) >= bound { b = mid } else { a = mid + 1 } }
                                    a as i64
                                };
                                let last_le = |bound: f64| -> i64 {
                                    let (mut a, mut b) = (nn_lo, nn_hi + 1);
                                    while a < b { let mid = (a + b) / 2; if kv(mid) > bound { b = mid } else { a = mid + 1 } }
                                    a as i64 - 1
                                };
                                let lo = match &f.start {
                                    FrameBound::UnboundedPreceding => 0,
                                    FrameBound::CurrentRow => part.peer_lo[p] as i64,
                                    FrameBound::Preceding(_) => first_ge(shift(start_off, true, part.rows[p], c)),
                                    FrameBound::Following(_) => first_ge(shift(start_off, false, part.rows[p], c)),
                                    FrameBound::UnboundedFollowing => m,
                                };
                                let hi = match &f.end {
                                    FrameBound::UnboundedFollowing => m - 1,
                                    FrameBound::CurrentRow => part.peer_hi[p] as i64,
                                    FrameBound::Preceding(_) => last_le(shift(end_off, true, part.rows[p], c)),
                                    FrameBound::Following(_) => last_le(shift(end_off, false, part.rows[p], c)),
                                    FrameBound::UnboundedPreceding => -1,
                                };
                                (lo, hi)
                            }
                        }
                    }
                },
            },
        };
        let (lo, hi) = (lo.max(0), hi.min(m - 1));
        if lo > hi { None } else { Some((lo as usize, hi as usize)) }
    };

    match func {
        WindowFn::RowNumber => for part in &parts { for (p, &r) in part.rows.iter().enumerate() { out[r] = V::Int(p as i64 + 1); } },
        WindowFn::Rank => for part in &parts { for (p, &r) in part.rows.iter().enumerate() { out[r] = V::Int(part.peer_lo[p] as i64 + 1); } },
        WindowFn::DenseRank => for part in &parts {
            let mut dense = 0i64;
            for (p, &r) in part.rows.iter().enumerate() {
                if part.peer_lo[p] == p { dense += 1; }
                out[r] = V::Int(dense);
            }
        },
        WindowFn::PercentRank => for part in &parts {
            let m = part.rows.len();
            for (p, &r) in part.rows.iter().enumerate() {
                out[r] = V::Float(if m <= 1 { 0.0 } else { part.peer_lo[p] as f64 / (m - 1) as f64 });
            }
        },
        WindowFn::CumeDist => for part in &parts {
            let m = part.rows.len() as f64;
            for (p, &r) in part.rows.iter().enumerate() { out[r] = V::Float((part.peer_hi[p] + 1) as f64 / m); }
        },
        WindowFn::Ntile(k) => {
            let k = match ev(k, 0) { V::Int(i) if i > 0 => i as usize, _ => return Err(err("NTILE needs a positive integer")) };
            for part in &parts {
                let m = part.rows.len();
                let (base, extra) = (m / k, m % k);
                for (p, &r) in part.rows.iter().enumerate() {
                    let bucket = if base == 0 { p + 1 }
                        else if p < extra * (base + 1) { p / (base + 1) + 1 }
                        else { extra + (p - extra * (base + 1)) / base + 1 };
                    out[r] = V::Int(bucket as i64);
                }
            }
        }
        WindowFn::Lag { expr, offset, default } | WindowFn::Lead { expr, offset, default } => {
            let lag = matches!(func, WindowFn::Lag { .. });
            let k = match ev(offset, 0) { V::Int(i) => i, V::Float(f) => f as i64, _ => 1 };
            let vals: Vec<V> = (0..n).map(|r| ev(expr, r)).collect();
            for part in &parts {
                let m = part.rows.len() as i64;
                for (p, &r) in part.rows.iter().enumerate() {
                    let q = if lag { p as i64 - k } else { p as i64 + k };
                    out[r] = if q >= 0 && q < m { vals[part.rows[q as usize]].clone() }
                        else { default.as_ref().map(|d| ev(d, r)).unwrap_or(V::Null) };
                }
            }
        }
        WindowFn::FirstValue(e) | WindowFn::LastValue(e) | WindowFn::FirstValueIgnoreNulls(e) | WindowFn::LastValueIgnoreNulls(e) => {
            let first = matches!(func, WindowFn::FirstValue(_) | WindowFn::FirstValueIgnoreNulls(_));
            let ignore = matches!(func, WindowFn::FirstValueIgnoreNulls(_) | WindowFn::LastValueIgnoreNulls(_));
            let vals: Vec<V> = (0..n).map(|r| ev(e, r)).collect();
            for part in &parts {
                for (p, &r) in part.rows.iter().enumerate() {
                    let Some((lo, hi)) = frame_of(part, p) else { continue };
                    let pick = |i: usize| &vals[part.rows[i]];
                    out[r] = if ignore {
                        let found = if first { (lo..=hi).map(pick).find(|v| !matches!(v, V::Null)) } else { (lo..=hi).rev().map(pick).find(|v| !matches!(v, V::Null)) };
                        found.cloned().unwrap_or(V::Null)
                    } else {
                        pick(if first { lo } else { hi }).clone()
                    };
                }
            }
        }
        WindowFn::NthValue { expr, n: nth } => {
            let k = match ev(nth, 0) { V::Int(i) if i > 0 => i as usize, _ => return Err(err("NTH_VALUE needs a positive integer")) };
            let vals: Vec<V> = (0..n).map(|r| ev(expr, r)).collect();
            for part in &parts {
                for (p, &r) in part.rows.iter().enumerate() {
                    if let Some((lo, hi)) = frame_of(part, p) {
                        if lo + k - 1 <= hi { out[r] = vals[part.rows[lo + k - 1]].clone(); }
                    }
                }
            }
        }
        WindowFn::CumSum(e) => {
            let agg = Expr::Agg { func: crate::ast::AggFunc::Sum, expr: e.clone() };
            let inp = aggs::prepare(&agg, n, ev).ok_or_else(|| err("bad CUMSUM"))?;
            for part in &parts {
                for (p, &r) in part.rows.iter().enumerate() { out[r] = aggs::aggregate(&inp, &part.rows[..=p])?; }
            }
        }
        WindowFn::Agg { .. } | WindowFn::AggX { .. } => {
            let agg_expr = match func {
                WindowFn::Agg { func: f, expr } => Expr::Agg { func: f.clone(), expr: expr.clone() },
                WindowFn::AggX { name, args, filter } => Expr::AggX { name: name.clone(), args: args.clone(), distinct: false, filter: filter.clone() },
                _ => unreachable!(),
            };
            let inp = aggs::prepare(&agg_expr, n, ev).ok_or_else(|| err("bad window aggregate"))?;
            let fast = !inp.distinct && inp.filter.is_none() && inp.args.len() <= 1 && matches!(inp.name.as_str(), "SUM" | "AVG" | "COUNT");
            for part in &parts {
                let m = part.rows.len();
                // prefix sums over the partition in window order
                let (mut ps, mut pc, mut pni) = (vec![0.0f64; m + 1], vec![0usize; m + 1], vec![0usize; m + 1]);
                // exact integer prefix sums (wrapping) for SUM over BIGINT values
                let mut pi = vec![0i64; m + 1];
                if fast {
                    for (i, &r) in part.rows.iter().enumerate() {
                        let iv = if inp.star { 0 } else if let V::Int(x) = &inp.args[0][r] { *x } else { 0 };
                        pi[i + 1] = pi[i].wrapping_add(iv);
                        let (x, is_int) = if inp.star { (Some(1.0), true) } else {
                            let v = &inp.args[0][r];
                            (if inp.name == "COUNT" { if matches!(v, V::Null) { None } else { Some(1.0) } } else { num(v) }, matches!(v, V::Int(_)))
                        };
                        ps[i + 1] = ps[i] + x.unwrap_or(0.0);
                        pc[i + 1] = pc[i] + x.is_some() as usize;
                        pni[i + 1] = pni[i] + (x.is_some() && !is_int) as usize;
                    }
                }
                for (p, &r) in part.rows.iter().enumerate() {
                    let Some((lo, hi)) = frame_of(part, p) else {
                        out[r] = if inp.name == "COUNT" { V::Int(0) } else { V::Null };
                        continue;
                    };
                    out[r] = if fast {
                        let cnt = pc[hi + 1] - pc[lo];
                        let sum = ps[hi + 1] - ps[lo];
                        match inp.name.as_str() {
                            "COUNT" => V::Int(cnt as i64),
                            _ if cnt == 0 => V::Null,
                            "SUM" => if pni[hi + 1] - pni[lo] == 0 { V::Int(pi[hi + 1].wrapping_sub(pi[lo])) } else { V::Float(sum) },
                            _ => V::Float(sum / cnt as f64),
                        }
                    } else {
                        aggs::aggregate(&inp, &part.rows[lo..=hi])?
                    };
                }
            }
        }
    }
    Ok(out)
}

// ───────────────────────── fast implementation ─────────────────────────

use rayon::prelude::*;

/// Below this many rows everything runs on the calling thread.
const PAR_MIN_ROWS: usize = 4096;
/// A partition at least this big is sorted with the parallel stable sort.
const PAR_SORT_MIN: usize = 32768;

/// Typed, allocation-free grouping key (same equivalence as `aggs::key_of`: integral floats equal ints, NULLs equal each other).
#[derive(PartialEq, Eq, Hash)]
enum K<'a> { Null, I(i64), F(u64), NaN, S(&'a str), B(bool) }

#[inline]
fn kof(v: &V) -> K<'_> {
    match v {
        V::Null => K::Null,
        V::Int(i) => K::I(*i),
        V::Float(f) => {
            if f.is_nan() { K::NaN }
            else if f.fract() == 0.0 && f.abs() < 9.0e15 { K::I(*f as i64) }
            else { K::F(f.to_bits()) }
        }
        V::Str(s) => K::S(s),
        V::Bool(b) => K::B(*b),
    }
}

/// Dictionary codes of a column in first-appearance order.
fn dict_codes(col: &[V]) -> (Vec<u32>, usize) {
    let mut map: HashMap<K, u32> = HashMap::new();
    let mut codes = Vec::with_capacity(col.len());
    for v in col {
        let next = map.len() as u32;
        codes.push(*map.entry(kof(v)).or_insert(next));
    }
    (codes, map.len())
}

enum KData<'a> { Int(Vec<i64>), Flt(Vec<f64>), Gen(&'a [V]) }

/// One ORDER BY key with a typed comparator (NULL placement and DESC identical to `cmp_keys`).
struct KCol<'a> { d: KData<'a>, nulls: Option<Vec<bool>>, desc: bool, nulls_first: bool }

impl<'a> KCol<'a> {
    fn new(col: &'a [V], item: &OrderByItem) -> KCol<'a> {
        let nulls_first = item.nulls_first.unwrap_or(!item.desc);
        let has_null = col.iter().any(|v| matches!(v, V::Null));
        let nulls = if has_null { Some(col.iter().map(|v| matches!(v, V::Null)).collect()) } else { None };
        let all_int = col.iter().all(|v| matches!(v, V::Int(_) | V::Null));
        let all_flt = col.iter().all(|v| matches!(v, V::Float(_) | V::Null));
        let d = if all_int {
            KData::Int(col.iter().map(|v| if let V::Int(i) = v { *i } else { 0 }).collect())
        } else if all_flt {
            KData::Flt(col.iter().map(|v| if let V::Float(f) = v { *f } else { 0.0 }).collect())
        } else { KData::Gen(col) };
        KCol { d, nulls, desc: item.desc, nulls_first }
    }
    #[inline]
    fn cmp(&self, a: usize, b: usize) -> Ordering {
        if let Some(nl) = &self.nulls {
            match (nl[a], nl[b]) {
                (true, true) => return Ordering::Equal,
                (true, false) => return if self.nulls_first { Ordering::Less } else { Ordering::Greater },
                (false, true) => return if self.nulls_first { Ordering::Greater } else { Ordering::Less },
                _ => {}
            }
        }
        let o = match &self.d {
            KData::Int(v) => v[a].cmp(&v[b]),
            KData::Flt(v) => v[a].total_cmp(&v[b]),
            KData::Gen(v) => aggs::total_cmp(&v[a], &v[b]),
        };
        if self.desc { o.reverse() } else { o }
    }
}

struct NPart {
    /// original row indices in window order
    rows: Vec<usize>,
    peer_lo: Vec<usize>,
    peer_hi: Vec<usize>,
}

/// Per-partition lookup tables for RANGE-with-offset and GROUPS frames.
#[derive(Default)]
struct Ctx {
    keys: Vec<Option<f64>>,
    nn_lo: usize,
    nn_hi: usize,
    gid: Vec<usize>,
    gstart: Vec<usize>,
    gend: Vec<usize>,
}

fn pmap<R: Send, F: Fn(&NPart) -> R + Sync + Send>(parts: &[NPart], n: usize, f: F) -> Vec<R> {
    if n >= PAR_MIN_ROWS && parts.len() > 1 { parts.par_iter().map(f).collect() } else { parts.iter().map(f).collect() }
}

/// MIN/MAX are only evaluated with sliding structures when the comparison is a genuine total order on this column.
fn homogeneous(col: &[V]) -> bool {
    let (mut ints, mut flts, mut strs, mut bools) = (false, false, false, false);
    let mut date_len: Option<usize> = None;
    let mut date_mixed = false;
    for v in col {
        match v {
            V::Null => {}
            V::Int(_) => ints = true,
            V::Float(_) => flts = true,
            V::Bool(_) => bools = true,
            V::Str(s) => {
                strs = true;
                if s.len() >= 8 && s.as_bytes()[4] == b'-' {
                    match date_len { None => date_len = Some(s.len()), Some(l) if l != s.len() => date_mixed = true, _ => {} }
                }
            }
        }
    }
    (ints as u8 + flts as u8 + strs as u8 + bools as u8) <= 1 && !date_mixed
}

fn monotone(frames: &[(usize, usize)]) -> bool {
    let (mut pl, mut ph) = (0usize, 0usize);
    for &(l, h) in frames {
        if l > h { continue; }
        if l < pl || h < ph { return false; }
        pl = l; ph = h;
    }
    true
}

/// Evaluate `func` OVER `spec` for each of `n` rows. `ev(expr, row)` evaluates an expression for a row.
/// Linear / n log n: typed partition keys, typed sorts, per-partition parallelism, sliding frames.
pub fn evaluate(func: &WindowFn, spec: &WindowSpec, n: usize, ev: &mut dyn FnMut(&Expr, usize) -> V) -> Result<Vec<V>, KoreError> {
    if crate::testing::slow_window() { return evaluate_slow(func, spec, n, ev); }

    // ── partitions: typed dictionary codes, first-appearance order ──
    let (pids, nparts): (Vec<u32>, usize) = if spec.partition_by.is_empty() { (Vec::new(), usize::from(n > 0)) } else {
        let mut cols: Vec<Vec<V>> = spec.partition_by.iter().map(|_| Vec::with_capacity(n)).collect();
        for r in 0..n { for (k, e) in spec.partition_by.iter().enumerate() { cols[k].push(ev(e, r)); } }
        let (mut ids, mut cnt) = dict_codes(&cols[0]);
        for c in &cols[1..] {
            let (codes, _) = dict_codes(c);
            let mut map: HashMap<(u32, u32), u32> = HashMap::new();
            for (i, id) in ids.iter_mut().enumerate() {
                let next = map.len() as u32;
                *id = *map.entry((*id, codes[i])).or_insert(next);
            }
            cnt = map.len();
        }
        (ids, cnt)
    };
    let part_rows: Vec<Vec<usize>> = if spec.partition_by.is_empty() {
        if n == 0 { Vec::new() } else { vec![(0..n).collect()] }
    } else {
        let mut counts = vec![0usize; nparts];
        for &p in &pids { counts[p as usize] += 1; }
        let mut v: Vec<Vec<usize>> = counts.iter().map(|&c| Vec::with_capacity(c)).collect();
        for (r, &p) in pids.iter().enumerate() { v[p as usize].push(r); }
        v
    };

    // ── ordering ──
    let okeys: Vec<Vec<V>> = spec.order_by.iter().map(|o| (0..n).map(|r| ev(&o.expr, r)).collect()).collect();
    let kcols: Vec<KCol> = okeys.iter().zip(spec.order_by.iter()).map(|(c, it)| KCol::new(c, it)).collect();
    let cmp_rows = |a: usize, b: usize| -> Ordering {
        for k in &kcols {
            let o = k.cmp(a, b);
            if o != Ordering::Equal { return o; }
        }
        Ordering::Equal
    };
    let ordered = !spec.order_by.is_empty();
    let build = |mut rows: Vec<usize>| -> NPart {
        if ordered {
            if rows.len() >= PAR_SORT_MIN { rows.par_sort_by(|&a, &b| cmp_rows(a, b)); } else { rows.sort_by(|&a, &b| cmp_rows(a, b)); }
        }
        let m = rows.len();
        let mut peer_lo = vec![0usize; m];
        let mut peer_hi = vec![m.saturating_sub(1); m];
        if ordered {
            for p in 1..m { peer_lo[p] = if cmp_rows(rows[p - 1], rows[p]) == Ordering::Equal { peer_lo[p - 1] } else { p }; }
            for p in (0..m.saturating_sub(1)).rev() { peer_hi[p] = if cmp_rows(rows[p], rows[p + 1]) == Ordering::Equal { peer_hi[p + 1] } else { p }; }
        }
        NPart { rows, peer_lo, peer_hi }
    };
    let parts: Vec<NPart> = if n >= PAR_MIN_ROWS && part_rows.len() > 1 { part_rows.into_par_iter().map(build).collect() } else { part_rows.into_iter().map(build).collect() };

    let mut out: Vec<V> = vec![V::Null; n];

    // ── frame helper ──
    let frame_spec = spec.frame.clone();
    #[derive(Clone, Copy)]
    enum Off { None, Num(i64), Interval(i64, &'static str) }
    let parse_off = |b: &FrameBound, ev: &mut dyn FnMut(&Expr, usize) -> V| -> Result<Off, KoreError> {
        match b {
            FrameBound::Preceding(e) | FrameBound::Following(e) => match e.as_ref() {
                Expr::FuncCall { name, args } if name == "INTERVAL" => {
                    let k = const_i64_f(&args[0], ev)?;
                    let unit = match args.get(1) { Some(Expr::Str(u)) => crate::datetime::norm_unit(u), _ => None }
                        .ok_or_else(|| err("unsupported interval unit in window frame"))?;
                    Ok(Off::Interval(k, unit))
                }
                other => Ok(Off::Num(const_i64_f(other, ev)?)),
            },
            _ => Ok(Off::None),
        }
    };
    let (start_off, end_off) = match &frame_spec {
        Some(f) => (parse_off(&f.start, ev)?, parse_off(&f.end, ev)?),
        None => (Off::None, Off::None),
    };
    let interval_frame = matches!(start_off, Off::Interval(..)) || matches!(end_off, Off::Interval(..));
    let range_dts: Vec<Option<crate::datetime::Dt>> = if interval_frame && spec.order_by.len() == 1 {
        okeys[0].iter().map(crate::scalar::to_dt).collect()
    } else { Vec::new() };
    let range_keys: Option<Vec<Option<f64>>> = match &frame_spec {
        Some(f) if f.mode == FrameMode::Range
            && !matches!((&f.start, &f.end), (FrameBound::UnboundedPreceding | FrameBound::CurrentRow, FrameBound::UnboundedFollowing | FrameBound::CurrentRow)) => {
            if spec.order_by.len() != 1 { return Err(err("RANGE frame with an offset needs exactly one ORDER BY expression")); }
            let desc = spec.order_by[0].desc;
            let mut ks = Vec::with_capacity(n);
            for (r, v) in okeys[0].iter().enumerate() {
                match v {
                    V::Null => ks.push(None),
                    other => {
                        let x = if interval_frame {
                            range_dts[r].map(|d| d.epoch_secs() as f64)
                                .ok_or_else(|| err("an INTERVAL RANGE frame needs a date or timestamp ORDER BY expression"))?
                        } else {
                            num(other).ok_or_else(|| err("RANGE frame with an offset needs a numeric ORDER BY expression"))?
                        };
                        ks.push(Some(if desc { -x } else { x }));
                    }
                }
            }
            Some(ks)
        }
        _ => None,
    };
    let desc_key = spec.order_by.first().map_or(false, |o| o.desc);
    let shift = |off: Off, preceding: bool, row: usize, c: f64| -> f64 {
        match off {
            Off::None => c,
            Off::Num(k) => if preceding { c - k as f64 } else { c + k as f64 },
            Off::Interval(k, unit) => {
                let Some(d) = range_dts.get(row).copied().flatten() else { return c };
                let sign = if preceding { -1 } else { 1 };
                let signed = if desc_key { -sign } else { sign };
                let moved = crate::datetime::add_unit(&d, unit, signed * k).unwrap_or(d);
                let secs = moved.epoch_secs() as f64;
                if desc_key { -secs } else { secs }
            }
        }
    };
    let rows_off = |o: Off| -> i64 { match o { Off::Num(k) => k, _ => 0 } };
    let groups_mode = matches!(&frame_spec, Some(f) if f.mode == FrameMode::Groups);

    let make_ctx = |part: &NPart| -> Ctx {
        let mut c = Ctx::default();
        let m = part.rows.len();
        if let Some(ks) = &range_keys {
            c.keys = part.rows.iter().map(|&r| ks[r]).collect();
            c.nn_lo = c.keys.iter().position(|k| k.is_some()).unwrap_or(0);
            c.nn_hi = c.keys.iter().rposition(|k| k.is_some()).unwrap_or(0);
        }
        if groups_mode {
            c.gid = vec![0; m];
            for p in 0..m {
                if part.peer_lo[p] == p { c.gstart.push(p); c.gend.push(part.peer_hi[p]); }
                c.gid[p] = c.gstart.len() - 1;
            }
        }
        c
    };

    // Frame [lo, hi] (positions) of position p; None when empty.
    let frame_of = |part: &NPart, ctx: &Ctx, p: usize| -> Option<(usize, usize)> {
        let m = part.rows.len() as i64;
        let pi = p as i64;
        let (lo, hi) = match &frame_spec {
            None => if ordered { (0, part.peer_hi[p] as i64) } else { (0, m - 1) },
            Some(f) => match f.mode {
                FrameMode::Rows => {
                    let lo = match &f.start {
                        FrameBound::UnboundedPreceding => 0,
                        FrameBound::Preceding(_) => pi.saturating_sub(rows_off(start_off)),
                        FrameBound::CurrentRow => pi,
                        FrameBound::Following(_) => pi.saturating_add(rows_off(start_off)),
                        FrameBound::UnboundedFollowing => m,
                    };
                    let hi = match &f.end {
                        FrameBound::UnboundedPreceding => -1,
                        FrameBound::Preceding(_) => pi.saturating_sub(rows_off(end_off)),
                        FrameBound::CurrentRow => pi,
                        FrameBound::Following(_) => pi.saturating_add(rows_off(end_off)),
                        FrameBound::UnboundedFollowing => m - 1,
                    };
                    (lo, hi)
                }
                FrameMode::Groups => {
                    let ng = ctx.gstart.len() as i64;
                    let g = ctx.gid[p] as i64;
                    // `k` groups back / forward from the current one
                    let back = |k: i64| -> Option<usize> { if k > g { None } else { Some((g - k) as usize) } };
                    let fwd = |k: i64| -> Option<usize> { if k >= ng - g { None } else { Some((g + k) as usize) } };
                    let lo = match &f.start {
                        FrameBound::UnboundedPreceding => 0,
                        FrameBound::CurrentRow => part.peer_lo[p] as i64,
                        FrameBound::Preceding(_) => back(rows_off(start_off)).map_or(0, |x| ctx.gstart[x] as i64),
                        FrameBound::Following(_) => fwd(rows_off(start_off)).map_or(m, |x| ctx.gstart[x] as i64),
                        FrameBound::UnboundedFollowing => m,
                    };
                    let hi = match &f.end {
                        FrameBound::UnboundedPreceding => -1,
                        FrameBound::CurrentRow => part.peer_hi[p] as i64,
                        FrameBound::Preceding(_) => back(rows_off(end_off)).map_or(-1, |x| ctx.gend[x] as i64),
                        FrameBound::Following(_) => fwd(rows_off(end_off)).map_or(m - 1, |x| ctx.gend[x] as i64),
                        FrameBound::UnboundedFollowing => m - 1,
                    };
                    (lo, hi)
                }
                FrameMode::Range => match &range_keys {
                    None => {
                        let lo = match &f.start { FrameBound::UnboundedPreceding => 0, FrameBound::CurrentRow => part.peer_lo[p] as i64, _ => 0 };
                        let hi = match &f.end { FrameBound::UnboundedFollowing => m - 1, FrameBound::CurrentRow => part.peer_hi[p] as i64, _ => m - 1 };
                        (lo, hi)
                    }
                    Some(_) => {
                        let cur = ctx.keys[p];
                        match cur {
                            None => (part.peer_lo[p] as i64, part.peer_hi[p] as i64),
                            Some(c) => {
                                let keys = &ctx.keys;
                                let (nn_lo, nn_hi) = (ctx.nn_lo, ctx.nn_hi);
                                let kv = |i: usize| keys[i].unwrap_or(0.0);
                                let first_ge = |bound: f64| -> i64 {
                                    let (mut a, mut b) = (nn_lo, nn_hi + 1);
                                    while a < b { let mid = (a + b) / 2; if kv(mid) >= bound { b = mid } else { a = mid + 1 } }
                                    a as i64
                                };
                                let last_le = |bound: f64| -> i64 {
                                    let (mut a, mut b) = (nn_lo, nn_hi + 1);
                                    while a < b { let mid = (a + b) / 2; if kv(mid) > bound { b = mid } else { a = mid + 1 } }
                                    a as i64 - 1
                                };
                                let row = part.rows[p];
                                let lo = match &f.start {
                                    FrameBound::UnboundedPreceding => 0,
                                    FrameBound::CurrentRow => part.peer_lo[p] as i64,
                                    FrameBound::Preceding(_) => first_ge(shift(start_off, true, row, c)),
                                    FrameBound::Following(_) => first_ge(shift(start_off, false, row, c)),
                                    FrameBound::UnboundedFollowing => m,
                                };
                                let hi = match &f.end {
                                    FrameBound::UnboundedFollowing => m - 1,
                                    FrameBound::CurrentRow => part.peer_hi[p] as i64,
                                    FrameBound::Preceding(_) => last_le(shift(end_off, true, row, c)),
                                    FrameBound::Following(_) => last_le(shift(end_off, false, row, c)),
                                    FrameBound::UnboundedPreceding => -1,
                                };
                                (lo, hi)
                            }
                        }
                    }
                },
            },
        };
        let (lo, hi) = (lo.max(0), hi.min(m - 1));
        if lo > hi { None } else { Some((lo as usize, hi as usize)) }
    };
    // all frames of a partition; (1, 0) marks an empty one
    let frames_of = |part: &NPart, ctx: &Ctx| -> Vec<(usize, usize)> {
        (0..part.rows.len()).map(|p| frame_of(part, ctx, p).unwrap_or((1, 0))).collect()
    };

    // write per-position results back to the original row order
    fn scatter(out: &mut [V], parts: &[NPart], res: Vec<Vec<V>>) {
        for (part, vs) in parts.iter().zip(res) {
            for (&r, v) in part.rows.iter().zip(vs) { out[r] = v; }
        }
    }

    match func {
        WindowFn::RowNumber => {
            let res = pmap(&parts, n, |part| (0..part.rows.len()).map(|p| V::Int(p as i64 + 1)).collect::<Vec<_>>());
            scatter(&mut out, &parts, res);
        }
        WindowFn::Rank => {
            let res = pmap(&parts, n, |part| (0..part.rows.len()).map(|p| V::Int(part.peer_lo[p] as i64 + 1)).collect::<Vec<_>>());
            scatter(&mut out, &parts, res);
        }
        WindowFn::DenseRank => {
            let res = pmap(&parts, n, |part| {
                let mut dense = 0i64;
                (0..part.rows.len()).map(|p| { if part.peer_lo[p] == p { dense += 1; } V::Int(dense) }).collect::<Vec<_>>()
            });
            scatter(&mut out, &parts, res);
        }
        WindowFn::PercentRank => {
            let res = pmap(&parts, n, |part| {
                let m = part.rows.len();
                (0..m).map(|p| V::Float(if m <= 1 { 0.0 } else { part.peer_lo[p] as f64 / (m - 1) as f64 })).collect::<Vec<_>>()
            });
            scatter(&mut out, &parts, res);
        }
        WindowFn::CumeDist => {
            let res = pmap(&parts, n, |part| {
                let m = part.rows.len() as f64;
                (0..part.rows.len()).map(|p| V::Float((part.peer_hi[p] + 1) as f64 / m)).collect::<Vec<_>>()
            });
            scatter(&mut out, &parts, res);
        }
        WindowFn::Ntile(k) => {
            let k = match ev(k, 0) { V::Int(i) if i > 0 => i as usize, _ => return Err(err("NTILE needs a positive integer")) };
            let res = pmap(&parts, n, |part| {
                let m = part.rows.len();
                let (base, extra) = (m / k, m % k);
                (0..m).map(|p| {
                    let bucket = if base == 0 { p + 1 }
                        else if p < extra * (base + 1) { p / (base + 1) + 1 }
                        else { extra + (p - extra * (base + 1)) / base + 1 };
                    V::Int(bucket as i64)
                }).collect::<Vec<_>>()
            });
            scatter(&mut out, &parts, res);
        }
        WindowFn::Lag { expr, offset, default } | WindowFn::Lead { expr, offset, default } => {
            let lag = matches!(func, WindowFn::Lag { .. });
            let k = match ev(offset, 0) { V::Int(i) => i, V::Float(f) => f as i64, _ => 1 };
            let vals: Vec<V> = (0..n).map(|r| ev(expr, r)).collect();
            for part in &parts {
                let m = part.rows.len() as i64;
                for (p, &r) in part.rows.iter().enumerate() {
                    let q = if lag { (p as i64).saturating_sub(k) } else { (p as i64).saturating_add(k) };
                    out[r] = if q >= 0 && q < m { vals[part.rows[q as usize]].clone() }
                        else { default.as_ref().map(|d| ev(d, r)).unwrap_or(V::Null) };
                }
            }
        }
        WindowFn::FirstValue(e) | WindowFn::LastValue(e) | WindowFn::FirstValueIgnoreNulls(e) | WindowFn::LastValueIgnoreNulls(e) => {
            let first = matches!(func, WindowFn::FirstValue(_) | WindowFn::FirstValueIgnoreNulls(_));
            let ignore = matches!(func, WindowFn::FirstValueIgnoreNulls(_) | WindowFn::LastValueIgnoreNulls(_));
            let vals: Vec<V> = (0..n).map(|r| ev(e, r)).collect();
            let res = pmap(&parts, n, |part| {
                let m = part.rows.len();
                let ctx = make_ctx(part);
                let pick = |i: usize| &vals[part.rows[i]];
                // next / previous non-NULL position
                let (mut nn_next, mut nn_prev) = (Vec::new(), Vec::new());
                if ignore {
                    nn_next = vec![m; m + 1];
                    for i in (0..m).rev() { nn_next[i] = if matches!(pick(i), V::Null) { nn_next[i + 1] } else { i }; }
                    nn_prev = vec![-1i64; m];
                    for i in 0..m { nn_prev[i] = if matches!(pick(i), V::Null) { if i > 0 { nn_prev[i - 1] } else { -1 } } else { i as i64 }; }
                }
                (0..m).map(|p| {
                    let Some((lo, hi)) = frame_of(part, &ctx, p) else { return V::Null };
                    if ignore {
                        if first {
                            let j = nn_next[lo];
                            if j <= hi { pick(j).clone() } else { V::Null }
                        } else {
                            let j = nn_prev[hi];
                            if j >= lo as i64 { pick(j as usize).clone() } else { V::Null }
                        }
                    } else { pick(if first { lo } else { hi }).clone() }
                }).collect::<Vec<_>>()
            });
            scatter(&mut out, &parts, res);
        }
        WindowFn::NthValue { expr, n: nth } => {
            let k = match ev(nth, 0) { V::Int(i) if i > 0 => i as usize, _ => return Err(err("NTH_VALUE needs a positive integer")) };
            let vals: Vec<V> = (0..n).map(|r| ev(expr, r)).collect();
            let res = pmap(&parts, n, |part| {
                let ctx = make_ctx(part);
                (0..part.rows.len()).map(|p| match frame_of(part, &ctx, p) {
                    Some((lo, hi)) if lo.saturating_add(k - 1) <= hi => vals[part.rows[lo + k - 1]].clone(),
                    _ => V::Null,
                }).collect::<Vec<_>>()
            });
            scatter(&mut out, &parts, res);
        }
        WindowFn::CumSum(e) => {
            let agg = Expr::Agg { func: crate::ast::AggFunc::Sum, expr: e.clone() };
            let inp = aggs::prepare(&agg, n, ev).ok_or_else(|| err("bad CUMSUM"))?;
            let zero = std::iter::empty::<f64>().sum::<f64>();
            let res = pmap(&parts, n, |part| {
                let (mut live, mut all_int, mut isum, mut fsum) = (0usize, true, 0i64, zero);
                part.rows.iter().map(|&r| {
                    let v = &inp.args[0][r];
                    if !matches!(v, V::Null) {
                        live += 1;
                        if let V::Int(i) = v { isum = isum.wrapping_add(*i); } else { all_int = false; }
                        if let Some(x) = num(v) { fsum += x; }
                    }
                    if live == 0 { V::Null } else if all_int { V::Int(isum) } else { V::Float(fsum) }
                }).collect::<Vec<_>>()
            });
            scatter(&mut out, &parts, res);
        }
        WindowFn::Agg { .. } | WindowFn::AggX { .. } => {
            let agg_expr = match func {
                WindowFn::Agg { func: f, expr } => Expr::Agg { func: f.clone(), expr: expr.clone() },
                WindowFn::AggX { name, args, filter } => Expr::AggX { name: name.clone(), args: args.clone(), distinct: false, filter: filter.clone() },
                _ => unreachable!(),
            };
            let inp = aggs::prepare(&agg_expr, n, ev).ok_or_else(|| err("bad window aggregate"))?;
            let plain = !inp.distinct && inp.filter.is_none() && inp.args.len() <= 1;
            let fast = plain && matches!(inp.name.as_str(), "SUM" | "AVG" | "COUNT");
            let minmax = !inp.distinct && inp.filter.is_none() && !inp.args.is_empty() && matches!(inp.name.as_str(), "MIN" | "MAX");
            let is_min = inp.name == "MIN";
            let homog = minmax && homogeneous(&inp.args[0]);
            let cdist = inp.distinct && inp.name == "COUNT" && inp.filter.is_none() && !inp.star && inp.args.len() == 1;
            let res: Vec<Result<Vec<V>, KoreError>> = pmap(&parts, n, |part| {
                let m = part.rows.len();
                let ctx = make_ctx(part);
                let frames = frames_of(part, &ctx);
                if fast {
                    // prefix sums over the partition in window order
                    let (mut ps, mut pc, mut pni) = (vec![0.0f64; m + 1], vec![0usize; m + 1], vec![0usize; m + 1]);
                    // exact integer prefix sums (wrapping) for SUM over BIGINT values
                    let mut pi = vec![0i64; m + 1];
                    for (i, &r) in part.rows.iter().enumerate() {
                        let iv = if inp.star { 0 } else if let V::Int(x) = &inp.args[0][r] { *x } else { 0 };
                        pi[i + 1] = pi[i].wrapping_add(iv);
                        let (x, is_int) = if inp.star { (Some(1.0), true) } else {
                            let v = &inp.args[0][r];
                            (if inp.name == "COUNT" { if matches!(v, V::Null) { None } else { Some(1.0) } } else { num(v) }, matches!(v, V::Int(_)))
                        };
                        ps[i + 1] = ps[i] + x.unwrap_or(0.0);
                        pc[i + 1] = pc[i] + x.is_some() as usize;
                        pni[i + 1] = pni[i] + (x.is_some() && !is_int) as usize;
                    }
                    return Ok(frames.iter().map(|&(lo, hi)| {
                        if lo > hi { return if inp.name == "COUNT" { V::Int(0) } else { V::Null }; }
                        let cnt = pc[hi + 1] - pc[lo];
                        let sum = ps[hi + 1] - ps[lo];
                        match inp.name.as_str() {
                            "COUNT" => V::Int(cnt as i64),
                            _ if cnt == 0 => V::Null,
                            "SUM" => if pni[hi + 1] - pni[lo] == 0 { V::Int(pi[hi + 1].wrapping_sub(pi[lo])) } else { V::Float(sum) },
                            _ => V::Float(sum / cnt as f64),
                        }
                    }).collect());
                }
                if minmax {
                    let col = &inp.args[0];
                    let at = |i: usize| &col[part.rows[i]];
                    let better = |v: &V, b: &V| { let o = aggs::total_cmp(v, b); if is_min { o == Ordering::Less } else { o == Ordering::Greater } };
                    if homog && monotone(&frames) {
                        // monotonic deque: strictly-better pops only, so the front is the first extreme in the frame
                        let mut dq: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
                        let mut next = 0usize;
                        return Ok(frames.iter().map(|&(lo, hi)| {
                            if lo > hi { return V::Null; }
                            while next <= hi {
                                let v = at(next);
                                if !matches!(v, V::Null) {
                                    while let Some(&b) = dq.back() { if better(v, at(b)) { dq.pop_back(); } else { break; } }
                                    dq.push_back(next);
                                }
                                next += 1;
                            }
                            while let Some(&f) = dq.front() { if f < lo { dq.pop_front(); } else { break; } }
                            dq.front().map_or(V::Null, |&i| at(i).clone())
                        }).collect());
                    }
                    // exact left fold per frame; a frame that only grows to the right is extended from the previous one
                    let mut prev: Option<(usize, usize, Option<usize>)> = None;
                    return Ok(frames.iter().map(|&(lo, hi)| {
                        if lo > hi { return V::Null; }
                        let (mut best, from) = match prev {
                            Some((pl, ph, pb)) if pl == lo && ph <= hi => (pb, ph + 1),
                            _ => (None, lo),
                        };
                        for i in from..=hi {
                            let v = at(i);
                            if matches!(v, V::Null) { continue; }
                            best = match best { None => Some(i), Some(b) => if better(v, at(b)) { Some(i) } else { Some(b) } };
                        }
                        prev = Some((lo, hi, best));
                        best.map_or(V::Null, |i| at(i).clone())
                    }).collect());
                }
                if cdist && monotone(&frames) {
                    let col = &inp.args[0];
                    let mut map: HashMap<K, u32> = HashMap::new();
                    let (mut cl, mut ch) = (0usize, 0usize);
                    let mut distinct = 0i64;
                    return Ok(frames.iter().map(|&(lo, hi)| {
                        if lo > hi { return V::Int(0); }
                        while ch <= hi {
                            let v = &col[part.rows[ch]];
                            if !matches!(v, V::Null) { let c = map.entry(kof(v)).or_insert(0); *c += 1; if *c == 1 { distinct += 1; } }
                            ch += 1;
                        }
                        while cl < lo {
                            let v = &col[part.rows[cl]];
                            if !matches!(v, V::Null) {
                                let c = map.get_mut(&kof(v)).expect("present");
                                *c -= 1;
                                if *c == 0 { distinct -= 1; }
                            }
                            cl += 1;
                        }
                        V::Int(distinct)
                    }).collect());
                }
                // generic aggregates: frames equal to the previous one reuse its result
                let mut res: Vec<V> = Vec::with_capacity(m);
                let mut prev: Option<((usize, usize), V)> = None;
                for &(lo, hi) in &frames {
                    let v = if lo > hi {
                        if inp.name == "COUNT" { V::Int(0) } else { V::Null }
                    } else if let Some((f, pv)) = &prev {
                        if *f == (lo, hi) { pv.clone() } else { aggs::aggregate(&inp, &part.rows[lo..=hi])? }
                    } else { aggs::aggregate(&inp, &part.rows[lo..=hi])? };
                    if lo <= hi { prev = Some(((lo, hi), v.clone())); }
                    res.push(v);
                }
                Ok(res)
            });
            let mut all = Vec::with_capacity(res.len());
            for r in res { all.push(r?); }
            scatter(&mut out, &parts, all);
        }
    }
    Ok(out)
}

fn const_i64_f(e: &Expr, ev: &mut dyn FnMut(&Expr, usize) -> V) -> Result<i64, KoreError> { const_i64(e, ev) }

/// Merge a `WINDOW w AS (..)` definition into a spec that refers to it (`OVER w` / `OVER (w ORDER BY ..)`).
pub fn resolve_spec(spec: &WindowSpec, defs: &[(String, WindowSpec)]) -> Result<WindowSpec, KoreError> {
    let Some(base) = &spec.base else { return Ok(spec.clone()) };
    let def = defs.iter().find(|(n, _)| n.eq_ignore_ascii_case(base))
        .ok_or_else(|| err(format!("window '{base}' is not defined")))?;
    let mut merged = resolve_spec(&def.1, defs)?;
    if !spec.partition_by.is_empty() { merged.partition_by = spec.partition_by.clone(); }
    if !spec.order_by.is_empty() { merged.order_by = spec.order_by.clone(); }
    if spec.frame.is_some() { merged.frame = spec.frame.clone(); }
    merged.base = None;
    Ok(merged)
}

/// Aggregate input type re-exported for the general path.
pub type WindowAggInput = AggInput;
