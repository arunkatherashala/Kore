//! Window functions with SQL semantics the generic `kore-window` crate lacks: the default frame
//! (`RANGE UNBOUNDED PRECEDING .. CURRENT ROW` with peers when ORDER BY is present, whole partition
//! otherwise), explicit `ROWS` frames, real NULLs, and LAG/LEAD/FIRST_VALUE/LAST_VALUE that keep
//! the source column's type. Returns `Ok(None)` for shapes it does not cover so the caller can fall back.

use std::cmp::Ordering;
use std::collections::HashMap;

use kore_core::{Column, ColumnData, DataBlock, KoreError, Value};

use crate::ast::{AggFunc, Expr, FrameBound, FrameMode, WindowFn, WindowSpec};

fn find_col<'a>(b: &'a DataBlock, name: &str) -> Option<&'a Column> {
    b.columns.iter().find(|c| c.name == name)
        .or_else(|| b.columns.iter().find(|c| c.name.rsplit('.').next() == Some(name.rsplit('.').next().unwrap_or(name))))
}

fn cmp_val(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => {
            let f = |v: &Value| match v { Value::Int(i) => *i as f64, Value::Float(f) => *f, _ => f64::NAN };
            f(a).partial_cmp(&f(b)).unwrap_or(Ordering::Equal)
        }
    }
}

fn col_of(e: &Expr) -> Option<&str> {
    match e { Expr::Col(c) | Expr::QualCol(_, c) => Some(c.as_str()), _ => None }
}

fn offset_of(e: &Expr) -> Option<usize> {
    match e { Expr::Int(i) if *i >= 0 => Some(*i as usize), _ => None }
}

/// Row `i` of `src` gathered into a new column (None = NULL), keeping the type.
fn gather(src: &Column, idx: &[Option<usize>], name: &str) -> Column {
    let data = match &src.data {
        ColumnData::Int64(v) => ColumnData::Int64(idx.iter().map(|i| i.and_then(|i| v[i])).collect()),
        ColumnData::Float64(v) => ColumnData::Float64(idx.iter().map(|i| i.and_then(|i| v[i])).collect()),
        ColumnData::Bool(v) => ColumnData::Bool(idx.iter().map(|i| i.and_then(|i| v[i])).collect()),
        other => ColumnData::Str(idx.iter().map(|i| i.and_then(|i| other.get_str(i).map(str::to_string))).collect()),
    };
    Column { name: name.to_string(), data }
}

/// Partitions as lists of row indices sorted by the ORDER BY of the spec.
fn partitions(block: &DataBlock, spec: &WindowSpec) -> Option<Vec<Vec<usize>>> {
    let n = block.num_rows;
    let pcols: Vec<&Column> = spec.partition_by.iter().map(|e| col_of(e).and_then(|c| find_col(block, c))).collect::<Option<_>>()?;
    let ocols: Vec<(&Column, bool)> = spec.order_by.iter().map(|o| find_col(block, &o.col).map(|c| (c, o.desc))).collect::<Option<_>>()?;
    let mut groups: HashMap<Vec<String>, Vec<usize>> = HashMap::new();
    let mut order: Vec<Vec<String>> = Vec::new();
    for r in 0..n {
        let key: Vec<String> = pcols.iter().map(|c| format!("{:?}", c.data.get_value(r))).collect();
        groups.entry(key.clone()).or_insert_with(|| { order.push(key); Vec::new() }).push(r);
    }
    let mut out = Vec::with_capacity(order.len());
    for k in order {
        let mut rows = groups.remove(&k).unwrap();
        if !ocols.is_empty() {
            rows.sort_by(|&a, &b| {
                for (c, desc) in &ocols {
                    let o = cmp_val(&c.data.get_value(a), &c.data.get_value(b));
                    // NULLs sort first ascending and last descending
                    let o = if *desc { o.reverse() } else { o };
                    if o != Ordering::Equal { return o; }
                }
                Ordering::Equal
            });
        }
        out.push(rows);
    }
    Some(out)
}

/// Frame `[lo, hi]` (positions in the sorted partition) for position `p`; None when empty.
fn frame_range(spec: &WindowSpec, p: usize, m: usize, peer_lo: usize, peer_hi: usize) -> Result<Option<(usize, usize)>, KoreError> {
    let (mode, start, end) = match &spec.frame {
        Some(f) => (f.mode.clone(), f.start.clone(), f.end.clone()),
        None if spec.order_by.is_empty() => (FrameMode::Rows, FrameBound::UnboundedPreceding, FrameBound::UnboundedFollowing),
        None => (FrameMode::Range, FrameBound::UnboundedPreceding, FrameBound::CurrentRow),
    };
    let off = |e: &Expr| offset_of(e).ok_or_else(|| KoreError::InvalidArgument("window frame offset must be a non-negative integer".into()));
    let (p, m) = (p as i64, m as i64);
    let (lo, hi) = match mode {
        FrameMode::Rows => {
            let lo = match &start {
                FrameBound::UnboundedPreceding => 0,
                FrameBound::Preceding(k) => p - off(k)? as i64,
                FrameBound::CurrentRow => p,
                FrameBound::Following(k) => p + off(k)? as i64,
                FrameBound::UnboundedFollowing => m,
            };
            let hi = match &end {
                FrameBound::UnboundedPreceding => -1,
                FrameBound::Preceding(k) => p - off(k)? as i64,
                FrameBound::CurrentRow => p,
                FrameBound::Following(k) => p + off(k)? as i64,
                FrameBound::UnboundedFollowing => m - 1,
            };
            (lo, hi)
        }
        FrameMode::Range => {
            let lo = match &start {
                FrameBound::UnboundedPreceding => 0,
                FrameBound::CurrentRow => peer_lo as i64,
                _ => return Err(KoreError::InvalidArgument("RANGE frames with offsets are not supported".into())),
            };
            let hi = match &end {
                FrameBound::UnboundedFollowing => m - 1,
                FrameBound::CurrentRow => peer_hi as i64,
                _ => return Err(KoreError::InvalidArgument("RANGE frames with offsets are not supported".into())),
            };
            (lo, hi)
        }
    };
    let (lo, hi) = (lo.max(0), hi.min(m - 1));
    Ok(if lo > hi { None } else { Some((lo as usize, hi as usize)) })
}

pub fn apply(block: &DataBlock, func: &WindowFn, spec: &WindowSpec, out_name: &str) -> Result<Option<DataBlock>, KoreError> {
    let n = block.num_rows;
    let Some(parts) = partitions(block, spec) else { return Ok(None) };
    let ocols: Vec<&Column> = spec.order_by.iter().filter_map(|o| find_col(block, &o.col)).collect();

    // Peer boundaries (rows equal on every ORDER BY key) per sorted partition.
    let peers = |rows: &[usize]| -> (Vec<usize>, Vec<usize>) {
        let m = rows.len();
        let mut lo = vec![0usize; m];
        let mut hi = vec![m.saturating_sub(1); m];
        if ocols.is_empty() { return (lo, hi); }
        let same = |a: usize, b: usize| ocols.iter().all(|c| cmp_val(&c.data.get_value(rows[a]), &c.data.get_value(rows[b])) == Ordering::Equal);
        for p in 1..m { lo[p] = if same(p - 1, p) { lo[p - 1] } else { p }; }
        for p in (0..m.saturating_sub(1)).rev() { hi[p] = if same(p, p + 1) { hi[p + 1] } else { p }; }
        (lo, hi)
    };

    let new_col = match func {
        WindowFn::Agg { func: agg, expr } => {
            if !matches!(agg, AggFunc::Sum | AggFunc::Avg | AggFunc::Count | AggFunc::Min | AggFunc::Max) { return Ok(None); }
            let star = matches!(expr.as_ref(), Expr::Star) || matches!(expr.as_ref(), Expr::Col(c) if c == "*");
            let vals: Vec<Option<f64>> = if star {
                if !matches!(agg, AggFunc::Count) { return Ok(None); }
                vec![Some(1.0); n]
            } else {
                match crate::vecexpr::num_vec(expr, block) { Some(v) => v, None => return Ok(None) }
            };
            let mut out: Vec<Option<f64>> = vec![None; n];
            for rows in &parts {
                let m = rows.len();
                let (plo, phi) = peers(rows);
                // prefix sums / counts over non-NULL values for SUM, AVG, COUNT
                let mut ps = vec![0.0f64; m + 1];
                let mut pc = vec![0usize; m + 1];
                for (i, &r) in rows.iter().enumerate() {
                    ps[i + 1] = ps[i] + vals[r].unwrap_or(0.0);
                    pc[i + 1] = pc[i] + vals[r].is_some() as usize;
                }
                for p in 0..m {
                    let Some((lo, hi)) = frame_range(spec, p, m, plo[p], phi[p])? else {
                        out[rows[p]] = if matches!(agg, AggFunc::Count) { Some(0.0) } else { None };
                        continue;
                    };
                    let cnt = pc[hi + 1] - pc[lo];
                    out[rows[p]] = match agg {
                        AggFunc::Count => Some(cnt as f64),
                        _ if cnt == 0 => None,
                        AggFunc::Sum => Some(ps[hi + 1] - ps[lo]),
                        AggFunc::Avg => Some((ps[hi + 1] - ps[lo]) / cnt as f64),
                        AggFunc::Min => rows[lo..=hi].iter().filter_map(|&r| vals[r]).reduce(f64::min),
                        _ => rows[lo..=hi].iter().filter_map(|&r| vals[r]).reduce(f64::max),
                    };
                }
            }
            Column { name: out_name.to_string(), data: ColumnData::Float64(out) }
        }
        WindowFn::Lag { expr, offset } | WindowFn::Lead { expr, offset } => {
            let (Some(c), Some(k)) = (col_of(expr).and_then(|c| find_col(block, c)), offset_of(offset)) else { return Ok(None) };
            let lag = matches!(func, WindowFn::Lag { .. });
            let mut src: Vec<Option<usize>> = vec![None; n];
            for rows in &parts {
                for p in 0..rows.len() {
                    let q = if lag { p.checked_sub(k) } else { Some(p + k).filter(|&q| q < rows.len()) };
                    src[rows[p]] = q.map(|q| rows[q]);
                }
            }
            gather(c, &src, out_name)
        }
        WindowFn::FirstValue(expr) | WindowFn::LastValue(expr) => {
            let Some(c) = col_of(expr).and_then(|c| find_col(block, c)) else { return Ok(None) };
            let first = matches!(func, WindowFn::FirstValue(_));
            let mut src: Vec<Option<usize>> = vec![None; n];
            for rows in &parts {
                let m = rows.len();
                let (plo, phi) = peers(rows);
                for p in 0..m {
                    if let Some((lo, hi)) = frame_range(spec, p, m, plo[p], phi[p])? {
                        src[rows[p]] = Some(rows[if first { lo } else { hi }]);
                    }
                }
            }
            gather(c, &src, out_name)
        }
        _ => return Ok(None),
    };
    let mut cols = block.columns.clone();
    cols.push(new_col);
    Ok(Some(DataBlock { columns: cols, num_rows: n }))
}
