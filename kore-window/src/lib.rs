//! KORE Layer 27 — Window Functions
//!
//! Implements the full set of SQL window functions against DataBlock:
//!   Numbering:   ROW_NUMBER, RANK, DENSE_RANK, NTILE(n)
//!   Navigation:  LAG(col, n), LEAD(col, n), FIRST_VALUE, LAST_VALUE
//!   Aggregate:   SUM/AVG/COUNT/MIN/MAX OVER (running / full-partition)
//!
//! # SQL usage (via kore-sql integration)
//! ```sql
//! SELECT region,
//!        revenue,
//!        ROW_NUMBER() OVER (PARTITION BY region ORDER BY revenue DESC) AS rn,
//!        SUM(revenue)  OVER (PARTITION BY region)                      AS region_total,
//!        LAG(revenue, 1) OVER (ORDER BY date)                         AS prev_revenue
//! FROM sales
//! ```

use kore_core::{Column, ColumnData, DataBlock, KoreError};
use std::collections::HashMap;

// ── Window frame types ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WindowFrame {
    pub start: FrameBound,
    pub end: FrameBound,
}

#[derive(Debug, Clone)]
pub enum FrameBound {
    UnboundedPreceding,
    Preceding(usize),
    CurrentRow,
    Following(usize),
    UnboundedFollowing,
}

// ── Window function variants ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum WindowFn {
    RowNumber,
    Rank,
    DenseRank,
    PercentRank,
    CumeDist,
    Ntile(usize),
    Lag  { col: String, offset: usize },
    Lead { col: String, offset: usize },
    // Aggregate over partition / frame
    Sum(String),
    Avg(String),
    Count(String),
    Min(String),
    Max(String),
    CumSum(String),   // running sum (UNBOUNDED PRECEDING … CURRENT ROW)
    CumAvg(String),
    FirstValue(String),
    LastValue(String),
}

// ── Order item ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WinOrder {
    pub col:  String,
    pub desc: bool,
}

// ── Main entry point ──────────────────────────────────────────────────────────

/// Compute a single window function over `block`, appending a new column
/// named `output_col`.  Returns the block with the extra column attached.
pub fn apply_window(
    block:       &DataBlock,
    partition_by: &[String],
    order_by:     &[WinOrder],
    func:        &WindowFn,
    output_col:  &str,
) -> Result<DataBlock, KoreError> {
    use rayon::prelude::*;

    let n = block.num_rows;

    // 1. Build partition groups
    let partitions = build_partitions(block, partition_by, order_by, n)?;

    // 2. Parallel: each partition computes independently.
    //    Use COMPACT partition-sized Vec (not n-sized scratch) — eliminates
    //    48MB zero-init + random-write per partition.
    let partial: Vec<Result<(Vec<usize>, Vec<f64>), KoreError>> = partitions
        .par_iter()
        .map(|(_key, sorted_indices)| {
            // Compact: only m values, sequential writes, cache-friendly
            let values = compute_fn_values_framed(block, sorted_indices, func, None, order_by)?;
            Ok((sorted_indices.clone(), values))
        })
        .collect();

    // 3. Merge: write each partition's values back to the correct positions
    let mut result: Vec<f64> = vec![0.0; n];
    for p in partial {
        let (indices, vals) = p?;
        for (&idx, &val) in indices.iter().zip(vals.iter()) {
            result[idx] = val;
        }
    }

    let new_col = Column {
        name: output_col.to_string(),
        data: ColumnData::Float64(result.into_iter().map(Some).collect()),
    };
    let mut cols = block.columns.clone();
    cols.push(new_col);
    Ok(DataBlock { columns: cols, num_rows: n })
}

/// Apply multiple window functions in one pass, each appending a column.
pub fn apply_windows(
    block:   &DataBlock,
    windows: &[(Vec<String>, Vec<WinOrder>, WindowFn, String)],
    //          ↑partition    ↑order         ↑func        ↑output_name
) -> Result<DataBlock, KoreError> {
    let mut cur = block.clone();
    for (part, ord, func, name) in windows {
        cur = apply_window(&cur, part, ord, func, name)?;
    }
    Ok(cur)
}

/// Compute a window function with an optional frame clause.
/// When `frame` is `Some`, aggregate functions (SUM, AVG, COUNT, MIN, MAX)
/// compute over only the frame range for each row within its partition.
pub fn apply_window_framed(
    block:        &DataBlock,
    partition_by: &[String],
    order_by:     &[WinOrder],
    func:         &WindowFn,
    frame:        Option<&WindowFrame>,
    output_col:   &str,
) -> Result<DataBlock, KoreError> {
    use rayon::prelude::*;

    let n = block.num_rows;
    let partitions = build_partitions(block, partition_by, order_by, n)?;

    let partial: Vec<Result<(Vec<usize>, Vec<f64>), KoreError>> = partitions
        .par_iter()
        .map(|(_key, sorted_indices)| {
            let values = compute_fn_values_framed(block, sorted_indices, func, frame, order_by)?;
            Ok((sorted_indices.clone(), values))
        })
        .collect();

    let mut result: Vec<f64> = vec![0.0; n];
    for p in partial {
        let (indices, vals) = p?;
        for (&idx, &val) in indices.iter().zip(vals.iter()) {
            result[idx] = val;
        }
    }

    let new_col = Column {
        name: output_col.to_string(),
        data: ColumnData::Float64(result.into_iter().map(Some).collect()),
    };
    let mut cols = block.columns.clone();
    cols.push(new_col);
    Ok(DataBlock { columns: cols, num_rows: n })
}

// ── Partition builder ─────────────────────────────────────────────────────────

/// Returns a map: partition_key → Vec<row_index in original block>
/// where the inner Vec is sorted by order_by.
/// Uses u128 FNV hash keys — eliminates String allocation per row.
fn build_partitions(
    block:        &DataBlock,
    partition_by: &[String],
    order_by:     &[WinOrder],
    n:            usize,
) -> Result<Vec<(String, Vec<usize>)>, KoreError> {
    // Pre-locate partition columns once
    let pcols: Vec<Option<&Column>> = partition_by.iter()
        .map(|name| block.columns.iter().find(|c| c.name == *name || c.name.ends_with(&format!(".{name}"))))
        .collect();

    // Group rows using u128 FNV hash — zero String allocation per row
    let mut groups: HashMap<u128, Vec<usize>> = HashMap::with_capacity(n.min(1024));
    let mut key_to_str: HashMap<u128, String> = HashMap::new();  // one String per distinct group

    for i in 0..n {
        let key: u128 = if partition_by.is_empty() {
            0u128
        } else {
            let mut k: u128 = 0xcbf29ce484222325_cbf29ce484222325u128;
            for (gi, col_opt) in pcols.iter().enumerate() {
                let v: u64 = match col_opt {
                    None => 0,
                    Some(col) => match &col.data {
                        ColumnData::Int64(v)   => v.get(i).and_then(|x| *x).unwrap_or(i64::MIN) as u64,
                        ColumnData::Float64(v) => v.get(i).and_then(|x| *x).map(|f| f.to_bits()).unwrap_or(0),
                        ColumnData::Bool(v)    => v.get(i).and_then(|x| *x).unwrap_or(false) as u64,
                        ColumnData::Str(v)     => {
                            let s = v.get(i).and_then(|x| x.as_deref()).unwrap_or("");
                            let mut h: u64 = 14695981039346656037;
                            for b in s.bytes() { h ^= b as u64; h = h.wrapping_mul(1099511628211); }
                            h
                        }
                        ColumnData::StrDict { codes, dict } => {
                            let c = codes.get(i).copied().unwrap_or(u8::MAX);
                            let s = if c == u8::MAX { "" } else { dict.get(c as usize).map(|x| x.as_str()).unwrap_or("") };
                            let mut h: u64 = 14695981039346656037;
                            for b in s.bytes() { h ^= b as u64; h = h.wrapping_mul(1099511628211); }
                            h
                        }
                    }
                };
                k = k.wrapping_add(v as u128)
                     .wrapping_mul(0x9e3779b97f4a7c15_f39cc0605cedc835u128)
                     .rotate_left((gi as u32 * 11 + 7) % 127);
            }
            k
        };
        // Store readable key only once per distinct group (for downstream compatibility)
        key_to_str.entry(key).or_insert_with(|| {
            if partition_by.is_empty() { "__all__".to_string() }
            else { partition_key(block, partition_by, i) }
        });
        groups.entry(key).or_default().push(i);
    }

    // Sort each partition by order_by and return (readable_key, sorted_indices)
    let result: Vec<(String, Vec<usize>)> = groups.into_iter()
        .map(|(k, mut idxs)| {
            if !order_by.is_empty() { sort_indices(block, &mut idxs, order_by); }
            (key_to_str.remove(&k).unwrap_or_default(), idxs)
        })
        .collect();

    Ok(result)
}

fn col_lookup<'a>(block: &'a DataBlock, name: &str) -> Option<&'a Column> {
    block.column(name)
        .or_else(|| block.columns.iter().find(|c| c.name.ends_with(&format!(".{}", name))))
}

fn partition_key(block: &DataBlock, cols: &[String], row: usize) -> String {
    cols.iter().map(|c| {
        col_lookup(block, c)
            .map(|col| match &col.data {
                ColumnData::Int64(v)   => v.get(row).and_then(|x| *x).map(|i| i.to_string()).unwrap_or_default(),
                ColumnData::Float64(v) => v.get(row).and_then(|x| *x).map(|f| format!("{f:.6}")).unwrap_or_default(),
                ColumnData::Bool(v)    => v.get(row).and_then(|x| *x).map(|b| b.to_string()).unwrap_or_default(),
                ColumnData::Str(v)     => v.get(row).and_then(|x| x.as_deref()).unwrap_or("").to_string(),
                ColumnData::StrDict { codes, dict } => {
                    let c = codes.get(row).copied().unwrap_or(u8::MAX);
                    if c == u8::MAX { String::new() } else { dict.get(c as usize).cloned().unwrap_or_default() }
                }
            })
            .unwrap_or_default()
    })
    .collect::<Vec<_>>()
    .join("\x00")
}

fn sort_indices(block: &DataBlock, indices: &mut Vec<usize>, order_by: &[WinOrder]) {
    use rayon::slice::ParallelSliceMut;

    let m = indices.len();
    if m <= 1 || order_by.is_empty() { return; }

    // ── Fast path: single numeric order-by column ─────────────────────────────
    // Schwartzian transform: pre-extract keys → parallel sort → write back.
    // Eliminates random column access per comparison; LLVM vectorizes extraction.
    if order_by.len() == 1 {
        let ord = &order_by[0];
        if let Some(col) = col_lookup(block, &ord.col) {
            let mut keyed: Vec<(f64, usize)> = indices.iter().map(|&i| {
                let k = match &col.data {
                    ColumnData::Float64(v) => v.get(i).and_then(|x| *x).unwrap_or(f64::NEG_INFINITY),
                    ColumnData::Int64(v)   => v.get(i).and_then(|x| *x).unwrap_or(i64::MIN) as f64,
                    ColumnData::Bool(v)    => v.get(i).and_then(|x| *x).unwrap_or(false) as i32 as f64,
                    ColumnData::Str(_)        => 0.0,  // handled in fallback
                    ColumnData::StrDict { .. } => 0.0,  // handled in fallback
                };
                (k, i)
            }).collect();

            // Rayon parallel sort — uses all 8 cores
            if ord.desc {
                keyed.par_sort_unstable_by(|a, b|
                    b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            } else {
                keyed.par_sort_unstable_by(|a, b|
                    a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            }
            for (j, (_, idx)) in keyed.into_iter().enumerate() { indices[j] = idx; }
            return;
        }
    }

    // ── Fallback: multi-column or String ORDER BY (sequential) ────────────────
    indices.sort_unstable_by(|&a, &b| {
        for ord in order_by {
            if let Some(col) = col_lookup(block, &ord.col) {
                let cmp = match &col.data {
                    ColumnData::Int64(v) => {
                        let va = v.get(a).and_then(|x| *x).unwrap_or(i64::MIN);
                        let vb = v.get(b).and_then(|x| *x).unwrap_or(i64::MIN);
                        va.cmp(&vb)
                    }
                    ColumnData::Float64(v) => {
                        let va = v.get(a).and_then(|x| *x).unwrap_or(f64::NAN);
                        let vb = v.get(b).and_then(|x| *x).unwrap_or(f64::NAN);
                        va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal)
                    }
                    ColumnData::Str(v) => {
                        let sa = v.get(a).and_then(|x| x.as_deref()).unwrap_or("");
                        let sb = v.get(b).and_then(|x| x.as_deref()).unwrap_or("");
                        sa.cmp(sb)
                    }
                    ColumnData::StrDict { codes, dict } => {
                        let ca = codes.get(a).copied().unwrap_or(u8::MAX);
                        let cb = codes.get(b).copied().unwrap_or(u8::MAX);
                        let sa = if ca == u8::MAX { "" } else { dict.get(ca as usize).map(|s| s.as_str()).unwrap_or("") };
                        let sb = if cb == u8::MAX { "" } else { dict.get(cb as usize).map(|s| s.as_str()).unwrap_or("") };
                        sa.cmp(sb)
                    }
                    ColumnData::Bool(v) => {
                        let ba = v.get(a).and_then(|x| *x).unwrap_or(false) as u8;
                        let bb = v.get(b).and_then(|x| *x).unwrap_or(false) as u8;
                        ba.cmp(&bb)
                    }
                };
                let cmp = if ord.desc { cmp.reverse() } else { cmp };
                if cmp != std::cmp::Ordering::Equal { return cmp; }
            }
        }
        std::cmp::Ordering::Equal
    });
}

// ── Function evaluation ───────────────────────────────────────────────────────

/// Compute window function values into a COMPACT Vec<f64> of length m.
/// No n-sized scratch buffer — sequential writes, cache-friendly.
#[allow(dead_code)]
fn compute_fn_values(
    block:  &DataBlock,
    sorted: &[usize],
    func:   &WindowFn,
) -> Result<Vec<f64>, KoreError> {
    compute_fn_values_framed(block, sorted, func, None, &[])
}

/// Compute window function values with optional frame and order-by context.
fn compute_fn_values_framed(
    block:    &DataBlock,
    sorted:   &[usize],
    func:     &WindowFn,
    frame:    Option<&WindowFrame>,
    order_by: &[WinOrder],
) -> Result<Vec<f64>, KoreError> {
    let m = sorted.len();
    let mut values = vec![0.0f64; m];

    match func {
        WindowFn::RowNumber => {
            for i in 0..m { values[i] = (i + 1) as f64; }
        }
        WindowFn::Rank => {
            if m > 0 {
                values[0] = 1.0;
                for i in 1..m {
                    if rows_equal_on_order(block, sorted[i], sorted[i - 1], order_by) {
                        values[i] = values[i - 1];
                    } else {
                        values[i] = (i + 1) as f64;
                    }
                }
            }
        }
        WindowFn::DenseRank => {
            if m > 0 {
                values[0] = 1.0;
                let mut dense = 1.0;
                for i in 1..m {
                    if !rows_equal_on_order(block, sorted[i], sorted[i - 1], order_by) {
                        dense += 1.0;
                    }
                    values[i] = dense;
                }
            }
        }
        WindowFn::PercentRank | WindowFn::CumeDist => {
            for i in 0..m { values[i] = (i + 1) as f64; }
        }
        WindowFn::Ntile(buckets) => {
            let n = m as f64;
            for i in 0..m {
                values[i] = ((i as f64 / n) * (*buckets as f64)).floor() + 1.0;
            }
        }
        WindowFn::Lag { col, offset } => {
            let vals = extract_f64(block, col, sorted);
            for i in 0..m {
                values[i] = if i >= *offset { vals[i - offset] } else { f64::NAN };
            }
        }
        WindowFn::Lead { col, offset } => {
            let vals = extract_f64(block, col, sorted);
            for i in 0..m {
                values[i] = if i + offset < m { vals[i + offset] } else { f64::NAN };
            }
        }
        WindowFn::Sum(col) => {
            let vals = extract_f64(block, col, sorted);
            if let Some(fr) = frame {
                for i in 0..m {
                    let (lo, hi) = frame_bounds(fr, i, m);
                    values[i] = vals[lo..=hi].iter().filter(|v| !v.is_nan()).sum();
                }
            } else {
                let total: f64 = vals.iter().filter(|v| !v.is_nan()).sum();
                for i in 0..m { values[i] = total; }
            }
        }
        WindowFn::Avg(col) => {
            let vals = extract_f64(block, col, sorted);
            if let Some(fr) = frame {
                for i in 0..m {
                    let (lo, hi) = frame_bounds(fr, i, m);
                    let slice: Vec<f64> = vals[lo..=hi].iter().copied().filter(|v| !v.is_nan()).collect();
                    values[i] = if slice.is_empty() { f64::NAN } else { slice.iter().sum::<f64>() / slice.len() as f64 };
                }
            } else {
                let good: Vec<f64> = vals.iter().copied().filter(|v| !v.is_nan()).collect();
                let avg = if good.is_empty() { f64::NAN }
                          else { good.iter().sum::<f64>() / good.len() as f64 };
                for i in 0..m { values[i] = avg; }
            }
        }
        WindowFn::Count(col) => {
            let vals = extract_f64(block, col, sorted);
            if let Some(fr) = frame {
                for i in 0..m {
                    let (lo, hi) = frame_bounds(fr, i, m);
                    values[i] = vals[lo..=hi].iter().filter(|v| !v.is_nan()).count() as f64;
                }
            } else {
                let cnt = vals.iter().filter(|v| !v.is_nan()).count() as f64;
                for i in 0..m { values[i] = cnt; }
            }
        }
        WindowFn::Min(col) => {
            let vals = extract_f64(block, col, sorted);
            if let Some(fr) = frame {
                for i in 0..m {
                    let (lo, hi) = frame_bounds(fr, i, m);
                    values[i] = vals[lo..=hi].iter().copied().filter(|v| !v.is_nan()).fold(f64::INFINITY, f64::min);
                }
            } else {
                let min = vals.iter().copied().filter(|v| !v.is_nan()).fold(f64::INFINITY, f64::min);
                for i in 0..m { values[i] = min; }
            }
        }
        WindowFn::Max(col) => {
            let vals = extract_f64(block, col, sorted);
            if let Some(fr) = frame {
                for i in 0..m {
                    let (lo, hi) = frame_bounds(fr, i, m);
                    values[i] = vals[lo..=hi].iter().copied().filter(|v| !v.is_nan()).fold(f64::NEG_INFINITY, f64::max);
                }
            } else {
                let max = vals.iter().copied().filter(|v| !v.is_nan()).fold(f64::NEG_INFINITY, f64::max);
                for i in 0..m { values[i] = max; }
            }
        }
        WindowFn::CumSum(col) => {
            let vals = extract_f64(block, col, sorted);
            let mut running = 0.0f64;
            for (i, v) in vals.iter().enumerate() {
                if !v.is_nan() { running += v; }
                values[i] = running;
            }
        }
        _ => {
            // Fallback: use old compute_fn_for_partition
            let n_total = block.num_rows;
            let mut scratch = vec![0.0f64; n_total];
            compute_fn_for_partition(block, sorted, func, &mut scratch)?;
            for (i, &orig_i) in sorted.iter().enumerate() { values[i] = scratch[orig_i]; }
        }
    }
    Ok(values)
}

fn compute_fn_for_partition(
    block:   &DataBlock,
    sorted:  &[usize],
    func:    &WindowFn,
    result:  &mut Vec<f64>,
) -> Result<(), KoreError> {
    let m = sorted.len();

    match func {
        WindowFn::RowNumber => {
            for (rank, &orig_i) in sorted.iter().enumerate() {
                result[orig_i] = (rank + 1) as f64;
            }
        }
        WindowFn::Rank => {
            // rows with equal order keys get the same rank; next rank skips
            // (needs order-by values to detect ties — simplified: each row distinct)
            for (rank, &orig_i) in sorted.iter().enumerate() {
                result[orig_i] = (rank + 1) as f64;
            }
        }
        WindowFn::DenseRank => {
            for (rank, &orig_i) in sorted.iter().enumerate() {
                result[orig_i] = (rank + 1) as f64;
            }
        }
        WindowFn::PercentRank => {
            let n = m as f64;
            for (rank, &orig_i) in sorted.iter().enumerate() {
                result[orig_i] = if n <= 1.0 { 0.0 } else { rank as f64 / (n - 1.0) };
            }
        }
        WindowFn::CumeDist => {
            let n = m as f64;
            for (rank, &orig_i) in sorted.iter().enumerate() {
                result[orig_i] = (rank + 1) as f64 / n;
            }
        }
        WindowFn::Ntile(buckets) => {
            let n = m as f64;
            for (i, &orig_i) in sorted.iter().enumerate() {
                result[orig_i] = ((i as f64 / n) * (*buckets as f64)).floor() + 1.0;
            }
        }
        WindowFn::Lag { col, offset } => {
            let vals = extract_f64(block, col, sorted);
            for (i, &orig_i) in sorted.iter().enumerate() {
                result[orig_i] = if i >= *offset { vals[i - offset] } else { f64::NAN };
            }
        }
        WindowFn::Lead { col, offset } => {
            let vals = extract_f64(block, col, sorted);
            for (i, &orig_i) in sorted.iter().enumerate() {
                result[orig_i] = if i + offset < m { vals[i + offset] } else { f64::NAN };
            }
        }
        WindowFn::Sum(col) => {
            let vals = extract_f64(block, col, sorted);
            let total: f64 = vals.iter().filter(|v| !v.is_nan()).sum();
            for &orig_i in sorted { result[orig_i] = total; }
        }
        WindowFn::Avg(col) => {
            let vals = extract_f64(block, col, sorted);
            let good: Vec<f64> = vals.iter().copied().filter(|v| !v.is_nan()).collect();
            let avg = if good.is_empty() { f64::NAN } else { good.iter().sum::<f64>() / good.len() as f64 };
            for &orig_i in sorted { result[orig_i] = avg; }
        }
        WindowFn::Count(col) => {
            let vals = extract_f64(block, col, sorted);
            let cnt = vals.iter().filter(|v| !v.is_nan()).count() as f64;
            for &orig_i in sorted { result[orig_i] = cnt; }
        }
        WindowFn::Min(col) => {
            let vals = extract_f64(block, col, sorted);
            let min = vals.iter().copied().filter(|v| !v.is_nan())
                .fold(f64::INFINITY, f64::min);
            for &orig_i in sorted { result[orig_i] = min; }
        }
        WindowFn::Max(col) => {
            let vals = extract_f64(block, col, sorted);
            let max = vals.iter().copied().filter(|v| !v.is_nan())
                .fold(f64::NEG_INFINITY, f64::max);
            for &orig_i in sorted { result[orig_i] = max; }
        }
        WindowFn::CumSum(col) => {
            let vals = extract_f64(block, col, sorted);
            let mut running = 0.0f64;
            for (i, &orig_i) in sorted.iter().enumerate() {
                if !vals[i].is_nan() { running += vals[i]; }
                result[orig_i] = running;
            }
        }
        WindowFn::CumAvg(col) => {
            let vals = extract_f64(block, col, sorted);
            let mut running = 0.0f64; let mut cnt = 0usize;
            for (i, &orig_i) in sorted.iter().enumerate() {
                if !vals[i].is_nan() { running += vals[i]; cnt += 1; }
                result[orig_i] = if cnt > 0 { running / cnt as f64 } else { f64::NAN };
            }
        }
        WindowFn::FirstValue(col) => {
            let vals = extract_f64(block, col, sorted);
            let first = vals.first().copied().unwrap_or(f64::NAN);
            for &orig_i in sorted { result[orig_i] = first; }
        }
        WindowFn::LastValue(col) => {
            let vals = extract_f64(block, col, sorted);
            let last = vals.last().copied().unwrap_or(f64::NAN);
            for &orig_i in sorted { result[orig_i] = last; }
        }
    }
    Ok(())
}

/// Resolve frame bounds to inclusive (lo, hi) indices within a partition of size `m`.
fn frame_bounds(frame: &WindowFrame, current: usize, m: usize) -> (usize, usize) {
    let lo = match frame.start {
        FrameBound::UnboundedPreceding => 0,
        FrameBound::Preceding(n) => current.saturating_sub(n),
        FrameBound::CurrentRow => current,
        FrameBound::Following(n) => (current + n).min(m - 1),
        FrameBound::UnboundedFollowing => m - 1,
    };
    let hi = match frame.end {
        FrameBound::UnboundedPreceding => 0,
        FrameBound::Preceding(n) => current.saturating_sub(n),
        FrameBound::CurrentRow => current,
        FrameBound::Following(n) => (current + n).min(m - 1),
        FrameBound::UnboundedFollowing => m - 1,
    };
    (lo, hi)
}

/// Compare two rows on ORDER BY columns; returns true if they have equal values.
fn rows_equal_on_order(block: &DataBlock, row_a: usize, row_b: usize, order_by: &[WinOrder]) -> bool {
    for ord in order_by {
        if let Some(col) = col_lookup(block, &ord.col) {
            let equal = match &col.data {
                ColumnData::Int64(v) => {
                    v.get(row_a).and_then(|x| *x) == v.get(row_b).and_then(|x| *x)
                }
                ColumnData::Float64(v) => {
                    let a = v.get(row_a).and_then(|x| *x);
                    let b = v.get(row_b).and_then(|x| *x);
                    match (a, b) {
                        (Some(a), Some(b)) => a.to_bits() == b.to_bits(),
                        (None, None) => true,
                        _ => false,
                    }
                }
                ColumnData::Str(v) => {
                    v.get(row_a).and_then(|x| x.as_deref()) == v.get(row_b).and_then(|x| x.as_deref())
                }
                ColumnData::StrDict { codes, .. } => {
                    codes.get(row_a) == codes.get(row_b)
                }
                ColumnData::Bool(v) => {
                    v.get(row_a).and_then(|x| *x) == v.get(row_b).and_then(|x| *x)
                }
            };
            if !equal { return false; }
        }
    }
    true
}

fn extract_f64(block: &DataBlock, col: &str, sorted_indices: &[usize]) -> Vec<f64> {
    match col_lookup(block, col).map(|c| &c.data) {
        Some(ColumnData::Float64(v)) =>
            sorted_indices.iter().map(|&i| v.get(i).and_then(|x| *x).unwrap_or(f64::NAN)).collect(),
        Some(ColumnData::Int64(v)) =>
            sorted_indices.iter().map(|&i| v.get(i).and_then(|x| *x).unwrap_or(0) as f64).collect(),
        _ => vec![f64::NAN; sorted_indices.len()],
    }
}

// ── Convenience: process a full analytics query pattern ─────────────────────

/// Helper: compute percent rank (0.0 … 1.0) within a partition.
pub fn percent_rank(
    block:        &DataBlock,
    partition_by: &[String],
    order_by:     &[WinOrder],
    output_col:   &str,
) -> Result<DataBlock, KoreError> {
    let n = block.num_rows;
    let partitions = build_partitions(block, partition_by, order_by, n)?;
    let mut result = vec![0.0f64; n];
    for (_, sorted) in &partitions {
        let m = sorted.len();
        for (i, &orig) in sorted.iter().enumerate() {
            result[orig] = if m <= 1 { 0.0 } else { i as f64 / (m - 1) as f64 };
        }
    }
    let new_col = Column {
        name: output_col.to_string(),
        data: ColumnData::Float64(result.into_iter().map(Some).collect()),
    };
    let mut cols = block.columns.clone();
    cols.push(new_col);
    Ok(DataBlock { columns: cols, num_rows: n })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::Column;

    fn get_f64_col(block: &DataBlock, name: &str) -> Vec<f64> {
        match &block.column(name).unwrap().data {
            ColumnData::Float64(v) => v.iter().map(|x| x.unwrap_or(f64::NAN)).collect(),
            ColumnData::Int64(v)   => v.iter().map(|x| x.unwrap_or(0) as f64).collect(),
            _ => vec![],
        }
    }

    fn sales_block() -> DataBlock {
        DataBlock::new(vec![
            Column::str_col("region",  vec![Some("East".into()), Some("East".into()), Some("West".into()), Some("West".into()), Some("East".into())]),
            Column::float64("revenue", vec![Some(100.0), Some(200.0), Some(150.0), Some(50.0), Some(300.0)]),
        ]).unwrap()
    }

    #[test]
    fn test_row_number_partitioned() {
        let b = sales_block();
        let out = apply_window(&b,
            &["region".into()],
            &[WinOrder { col: "revenue".into(), desc: true }],
            &WindowFn::RowNumber, "rn",
        ).unwrap();
        let rn = get_f64_col(&out, "rn");
        assert_eq!(rn[0], 3.0); // East 100 → rank 3 (descending)
        assert_eq!(rn[1], 2.0); // East 200 → rank 2
        assert_eq!(rn[4], 1.0); // East 300 → rank 1
        assert_eq!(rn[2], 1.0); // West 150 → rank 1
        assert_eq!(rn[3], 2.0); // West  50 → rank 2
    }

    #[test]
    fn test_cum_sum() {
        let b = sales_block();
        let out = apply_window(&b,
            &["region".into()],
            &[WinOrder { col: "revenue".into(), desc: false }],
            &WindowFn::CumSum("revenue".into()), "cum_rev",
        ).unwrap();
        let cum = get_f64_col(&out, "cum_rev");
        // East sorted asc: 100,200,300 → cumsum at those positions: 100,300,600
        assert_eq!(cum[0], 100.0);
        assert_eq!(cum[1], 300.0);
        assert_eq!(cum[4], 600.0);
    }

    #[test]
    fn test_lag() {
        let b = sales_block();
        let out = apply_window(&b,
            &[], &[WinOrder { col: "revenue".into(), desc: false }],
            &WindowFn::Lag { col: "revenue".into(), offset: 1 }, "prev",
        ).unwrap();
        let lag = get_f64_col(&out, "prev");
        assert!(lag.iter().any(|v| v.is_nan())); // first row NaN
    }

    #[test]
    fn test_partition_sum() {
        let b = sales_block();
        let out = apply_window(&b,
            &["region".into()], &[],
            &WindowFn::Sum("revenue".into()), "region_total",
        ).unwrap();
        let tot = get_f64_col(&out, "region_total");
        assert_eq!(tot[0], 600.0); // East 100+200+300
        assert_eq!(tot[4], 600.0);
        assert_eq!(tot[2], 200.0); // West 150+50
        assert_eq!(tot[3], 200.0);
    }

    // ── Task 1E: Window Frame Tests ──────────────────────────────────────────

    fn sequential_block() -> DataBlock {
        DataBlock::new(vec![
            Column::float64("val", vec![Some(10.0), Some(20.0), Some(30.0), Some(40.0), Some(50.0)]),
        ]).unwrap()
    }

    #[test]
    fn test_rolling_sum_1_preceding_1_following() {
        let b = sequential_block();
        let frame = WindowFrame {
            start: FrameBound::Preceding(1),
            end: FrameBound::Following(1),
        };
        let out = apply_window_framed(&b,
            &[],
            &[WinOrder { col: "val".into(), desc: false }],
            &WindowFn::Sum("val".into()),
            Some(&frame),
            "rolling_sum",
        ).unwrap();
        let rs = get_f64_col(&out, "rolling_sum");
        // row 0: vals[0..=1] = 10+20 = 30
        // row 1: vals[0..=2] = 10+20+30 = 60
        // row 2: vals[1..=3] = 20+30+40 = 90
        // row 3: vals[2..=4] = 30+40+50 = 120
        // row 4: vals[3..=4] = 40+50 = 90
        assert_eq!(rs[0], 30.0);
        assert_eq!(rs[1], 60.0);
        assert_eq!(rs[2], 90.0);
        assert_eq!(rs[3], 120.0);
        assert_eq!(rs[4], 90.0);
    }

    #[test]
    fn test_rolling_avg_2_preceding_current_row() {
        let b = sequential_block();
        let frame = WindowFrame {
            start: FrameBound::Preceding(2),
            end: FrameBound::CurrentRow,
        };
        let out = apply_window_framed(&b,
            &[],
            &[WinOrder { col: "val".into(), desc: false }],
            &WindowFn::Avg("val".into()),
            Some(&frame),
            "rolling_avg",
        ).unwrap();
        let ra = get_f64_col(&out, "rolling_avg");
        // row 0: avg(10) = 10
        // row 1: avg(10,20) = 15
        // row 2: avg(10,20,30) = 20
        // row 3: avg(20,30,40) = 30
        // row 4: avg(30,40,50) = 40
        assert_eq!(ra[0], 10.0);
        assert_eq!(ra[1], 15.0);
        assert_eq!(ra[2], 20.0);
        assert_eq!(ra[3], 30.0);
        assert_eq!(ra[4], 40.0);
    }

    #[test]
    fn test_framed_full_partition_default() {
        let b = sequential_block();
        let out = apply_window_framed(&b,
            &[],
            &[WinOrder { col: "val".into(), desc: false }],
            &WindowFn::Sum("val".into()),
            None,
            "full_sum",
        ).unwrap();
        let fs = get_f64_col(&out, "full_sum");
        // No frame → full partition sum = 10+20+30+40+50 = 150
        for v in &fs { assert_eq!(*v, 150.0); }
    }

    // ── Task 1I: RANK / DENSE_RANK Tie Tests ─────────────────────────────────

    fn ties_block() -> DataBlock {
        // values: [10, 20, 20, 30] — second and third rows are ties
        DataBlock::new(vec![
            Column::float64("score", vec![Some(10.0), Some(20.0), Some(20.0), Some(30.0)]),
        ]).unwrap()
    }

    #[test]
    fn test_rank_with_ties() {
        let b = ties_block();
        let out = apply_window(&b,
            &[],
            &[WinOrder { col: "score".into(), desc: false }],
            &WindowFn::Rank, "rnk",
        ).unwrap();
        let rnk = get_f64_col(&out, "rnk");
        // sorted asc: [10, 20, 20, 30] → ranks [1, 2, 2, 4]
        assert_eq!(rnk[0], 1.0);
        assert_eq!(rnk[1], 2.0);
        assert_eq!(rnk[2], 2.0);
        assert_eq!(rnk[3], 4.0);
    }

    #[test]
    fn test_dense_rank_with_ties() {
        let b = ties_block();
        let out = apply_window(&b,
            &[],
            &[WinOrder { col: "score".into(), desc: false }],
            &WindowFn::DenseRank, "drnk",
        ).unwrap();
        let drnk = get_f64_col(&out, "drnk");
        // sorted asc: [10, 20, 20, 30] → dense_ranks [1, 2, 2, 3]
        assert_eq!(drnk[0], 1.0);
        assert_eq!(drnk[1], 2.0);
        assert_eq!(drnk[2], 2.0);
        assert_eq!(drnk[3], 3.0);
    }

    #[test]
    fn test_row_number_no_ties() {
        let b = ties_block();
        let out = apply_window(&b,
            &[],
            &[WinOrder { col: "score".into(), desc: false }],
            &WindowFn::RowNumber, "rn",
        ).unwrap();
        let rn = get_f64_col(&out, "rn");
        // ROW_NUMBER always sequential: [1, 2, 3, 4]
        assert_eq!(rn[0], 1.0);
        assert_eq!(rn[1], 2.0);
        assert_eq!(rn[2], 3.0);
        assert_eq!(rn[3], 4.0);
    }
}
