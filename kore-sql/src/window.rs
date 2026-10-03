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

struct Part {
    /// original row indices in window order
    rows: Vec<usize>,
    /// peer group start / end (positions, inclusive) for each position
    peer_lo: Vec<usize>,
    peer_hi: Vec<usize>,
}

/// Evaluate `func` OVER `spec` for each of `n` rows. `ev(expr, row)` evaluates an expression for a row.
pub fn evaluate(func: &WindowFn, spec: &WindowSpec, n: usize, ev: &mut dyn FnMut(&Expr, usize) -> V) -> Result<Vec<V>, KoreError> {
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
    let mut parts: Vec<Part> = Vec::with_capacity(part_rows.len());
    for mut rows in part_rows {
        if !spec.order_by.is_empty() { rows.sort_by(|&a, &b| cmp_rows(a, b)); }
        let m = rows.len();
        let mut peer_lo = vec![0usize; m];
        let mut peer_hi = vec![m.saturating_sub(1); m];
        if !spec.order_by.is_empty() {
            for p in 1..m { peer_lo[p] = if cmp_rows(rows[p - 1], rows[p]) == Ordering::Equal { peer_lo[p - 1] } else { p }; }
            for p in (0..m.saturating_sub(1)).rev() { peer_hi[p] = if cmp_rows(rows[p], rows[p + 1]) == Ordering::Equal { peer_hi[p + 1] } else { p }; }
        }
        parts.push(Part { rows, peer_lo, peer_hi });
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
    let frame_of = |part: &Part, p: usize| -> Option<(usize, usize)> {
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
