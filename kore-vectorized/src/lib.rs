//! kore-vectorized — Layer 63: Vectorized batch SQL execution engine
//!
//! Processes SQL predicates and aggregations in SIMD-friendly batches of
//! BATCH_SIZE (1024) rows. LLVM auto-vectorizes the tight inner loops to
//! AVX2 (8× f64 per cycle) or AVX-512 (16× f64 per cycle).
//!
//! vs kore-sql (row-at-a-time interpreter):
//!   kore-sql:          1 row/dispatch × N rows = N interpreter calls
//!   kore-vectorized:   1024 rows/dispatch × N/1024 = 1000× fewer calls
//!
//! Architecture:
//!   1. Decompose SQL WHERE clause into vectorizable primitives
//!   2. Process BATCH_SIZE rows per call using fixed-size arrays
//!   3. Collect matching row indices using prefix-sum compaction
//!   4. Aggregate with SIMD sum/min/max/count per batch
//!   5. Merge partial aggregates at end

use std::collections::HashMap;
use rayon::prelude::*;
use kore_core::types::{Column, ColumnData, DataBlock};

pub const BATCH_SIZE: usize = 1024;

// ─── Vectorized filter primitives ────────────────────────────────────────────

/// A single comparison operation on a numeric column.
#[derive(Debug, Clone, Copy)]
pub enum CmpOp { Eq, Ne, Lt, Le, Gt, Ge }

/// A compound filter: multiple AND conditions on different columns.
/// Each condition is col OP literal (the dominant pattern in TPC-H).
#[derive(Debug, Clone)]
pub struct VecFilter {
    pub conditions: Vec<ColCondition>,
}

#[derive(Debug, Clone)]
pub struct ColCondition {
    pub col_name:   String,
    pub op:         CmpOp,
    pub threshold:  f64,
    pub str_value:  Option<String>,  // if set: string equality comparison (op must be Eq or Ne)
}

// ─── Vectorized aggregation ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum VecAgg {
    Sum, Count, Min, Max, Avg,
}

#[derive(Debug, Clone)]
pub struct AggSpec {
    pub input_col:  String,
    pub agg:        VecAgg,
    pub output_col: String,
}

#[derive(Debug, Clone)]
pub struct GroupBySpec {
    pub group_cols: Vec<String>,
    pub aggs:       Vec<AggSpec>,
}

// ─── Hot inner loops (LLVM vectorizes these) ─────────────────────────────────

/// Filter a batch of f64 values against a threshold.
/// Returns a bitmask (u64 with 1 bit per element, up to 64 elements).
#[inline(always)]
pub fn batch_cmp_f64(vals: &[f64], threshold: f64, op: CmpOp) -> u64 {
    let n = vals.len().min(64);
    let mut mask = 0u64;
    match op {
        CmpOp::Lt => { for i in 0..n { if vals[i] <  threshold { mask |= 1 << i; } } }
        CmpOp::Le => { for i in 0..n { if vals[i] <= threshold { mask |= 1 << i; } } }
        CmpOp::Gt => { for i in 0..n { if vals[i] >  threshold { mask |= 1 << i; } } }
        CmpOp::Ge => { for i in 0..n { if vals[i] >= threshold { mask |= 1 << i; } } }
        CmpOp::Eq => { for i in 0..n { if (vals[i] - threshold).abs() < 1e-10 { mask |= 1 << i; } } }
        CmpOp::Ne => { for i in 0..n { if (vals[i] - threshold).abs() >= 1e-10 { mask |= 1 << i; } } }
    }
    mask
}

/// Filter a batch of i64 values.
#[inline(always)]
pub fn batch_cmp_i64(vals: &[i64], threshold: i64, op: CmpOp) -> u64 {
    let n = vals.len().min(64);
    let mut mask = 0u64;
    match op {
        CmpOp::Lt => { for i in 0..n { if vals[i] <  threshold { mask |= 1 << i; } } }
        CmpOp::Le => { for i in 0..n { if vals[i] <= threshold { mask |= 1 << i; } } }
        CmpOp::Gt => { for i in 0..n { if vals[i] >  threshold { mask |= 1 << i; } } }
        CmpOp::Ge => { for i in 0..n { if vals[i] >= threshold { mask |= 1 << i; } } }
        CmpOp::Eq => { for i in 0..n { if vals[i] == threshold { mask |= 1 << i; } } }
        CmpOp::Ne => { for i in 0..n { if vals[i] != threshold { mask |= 1 << i; } } }
    }
    mask
}

/// Vectorized SUM over a selected set of values (bitmap-selected rows).
#[inline(always)]
pub fn batch_sum_masked(vals: &[f64], mask: u64, n: usize) -> f64 {
    let mut sum = 0.0f64;
    for i in 0..n.min(64) {
        if (mask >> i) & 1 == 1 { sum += vals[i]; }
    }
    sum
}

/// Tight inner sum loop — no masking, processes full 64-element stripe.
/// LLVM AVX2 vectorizes to: 4 × vaddpd ymm (32 f64/cycle throughput).
#[inline(always)]
pub fn batch_sum_full(vals: &[f64]) -> f64 {
    let mut s0 = 0.0f64; let mut s1 = 0.0f64;
    let mut s2 = 0.0f64; let mut s3 = 0.0f64;
    let chunks = vals.len() / 4 * 4;
    let mut i = 0;
    while i < chunks {
        s0 += vals[i]; s1 += vals[i+1]; s2 += vals[i+2]; s3 += vals[i+3];
        i += 4;
    }
    while i < vals.len() { s0 += vals[i]; i += 1; }
    s0 + s1 + s2 + s3
}

// ─── Full vectorized filter ───────────────────────────────────────────────────

/// Filter a DataBlock using a VecFilter, returning matching row indices.
/// Parallel for large blocks (splits rows across Rayon threads).
pub fn vectorized_filter(block: &DataBlock, filter: &VecFilter) -> Vec<usize> {
    if filter.conditions.is_empty() {
        return (0..block.num_rows).collect();
    }
    let n = block.num_rows;
    // Parallel for large blocks — each thread handles its row range independently
    if n >= 100_000 {
        let nthreads = rayon::current_num_threads();
        let chunk_sz = ((n + nthreads - 1) / nthreads).max(64);
        // Pre-locate columns once (shared across threads)
        let col_refs: Vec<Option<&Column>> = filter.conditions.iter()
            .map(|c| block.columns.iter().find(|col| col.name == c.col_name || col.name.ends_with(&format!(".{}", c.col_name))))
            .collect();
        let local: Vec<Vec<usize>> = (0..nthreads)
            .into_par_iter()
            .map(|t| {
                let row_start = t * chunk_sz;
                let row_end   = (row_start + chunk_sz).min(n);
                if row_start >= row_end { return vec![]; }
                filter_range(block, filter, &col_refs, row_start, row_end)
            })
            .collect();
        let mut result = Vec::with_capacity(n / 8);
        for v in local { result.extend(v); }
        return result;
    }
    // Sequential for small blocks
    let col_refs: Vec<Option<&Column>> = filter.conditions.iter()
        .map(|c| block.columns.iter().find(|col| col.name == c.col_name || col.name.ends_with(&format!(".{}", c.col_name))))
        .collect();
    filter_range(block, filter, &col_refs, 0, n)
}

fn filter_range(
    block: &DataBlock,
    filter: &VecFilter,
    col_refs: &[Option<&Column>],
    row_start: usize,
    row_end: usize,
) -> Vec<usize> {
    let n = row_end - row_start;
    let mut indices = Vec::with_capacity(n / 4);

    // Process 64 rows at a time (one u64 bitmask per pass)
    let mut row = row_start;
    while row < row_end {
        let batch_end = (row + 64).min(row_end);
        let batch_len = batch_end - row;

        let mut combined_mask: u64 = if batch_len >= 64 { u64::MAX } else { (1u64 << batch_len) - 1 };

        for (cond_idx, cond) in filter.conditions.iter().enumerate() {
            if combined_mask == 0 { break; }  // short-circuit
            let col = match col_refs[cond_idx] { Some(c) => c, None => continue };
            let mask = match &col.data {
                ColumnData::Float64(v) => {
                    let slice: Vec<f64> = v[row..batch_end].iter()
                        .map(|x| x.unwrap_or(f64::NAN)).collect();
                    batch_cmp_f64(&slice, cond.threshold, cond.op)
                }
                ColumnData::Int64(v) => {
                    let slice: Vec<i64> = v[row..batch_end].iter()
                        .map(|x| x.unwrap_or(i64::MIN)).collect();
                    batch_cmp_i64(&slice, cond.threshold as i64, cond.op)
                }
                ColumnData::Str(v) => {
                    // String equality/inequality (vectorized over batch)
                    if let Some(ref sv) = cond.str_value {
                        let n2 = batch_end - row;
                        let mut mask = 0u64;
                        match cond.op {
                            CmpOp::Eq => {
                                for i in 0..n2.min(64) {
                                    if v[row + i].as_deref() == Some(sv.as_str()) { mask |= 1 << i; }
                                }
                            }
                            CmpOp::Ne => {
                                for i in 0..n2.min(64) {
                                    if v[row + i].as_deref() != Some(sv.as_str()) { mask |= 1 << i; }
                                }
                            }
                            _ => mask = if batch_len >= 64 { u64::MAX } else { (1u64 << batch_len) - 1 },
                        }
                        mask
                    } else {
                        if batch_len >= 64 { u64::MAX } else { (1u64 << batch_len) - 1 }
                    }
                }
                _ => if batch_len >= 64 { u64::MAX } else { (1u64 << batch_len) - 1 },  // pass through
            };
            combined_mask &= mask;
        }

        // Expand bitmask to indices
        let mut m = combined_mask;
        while m != 0 {
            let bit = m.trailing_zeros() as usize;
            indices.push(row + bit);
            m &= m - 1;  // clear lowest set bit
        }
        row += 64;
    }

    indices
}

// ─── Vectorized global aggregation (no GROUP BY) ─────────────────────────────

#[derive(Debug, Clone)]
pub struct AggResult {
    pub col_name: String,
    pub agg:      VecAgg,
    pub value:    f64,
}

/// Aggregate a DataBlock using vectorized SIMD inner loops.
pub fn vectorized_agg(block: &DataBlock, row_indices: &[usize], specs: &[AggSpec]) -> Vec<AggResult> {
    specs.iter().map(|spec| {
        let col = block.columns.iter().find(|c|
            c.name == spec.input_col || c.name.ends_with(&format!(".{}", spec.input_col))
        );

        let value = match col {
            None => 0.0,
            Some(c) => match &c.data {
                ColumnData::Float64(v) => {
                    // Extract values for selected rows
                    let vals: Vec<f64> = row_indices.iter()
                        .filter_map(|&r| v.get(r).and_then(|x| *x)).collect();
                    compute_agg(&vals, &spec.agg)
                }
                ColumnData::Int64(v) => {
                    let vals: Vec<f64> = row_indices.iter()
                        .filter_map(|&r| v.get(r).and_then(|x| *x).map(|i| i as f64)).collect();
                    compute_agg(&vals, &spec.agg)
                }
                _ => 0.0,
            }
        };

        AggResult { col_name: spec.output_col.clone(), agg: spec.agg.clone(), value }
    }).collect()
}

fn compute_agg(vals: &[f64], agg: &VecAgg) -> f64 {
    match agg {
        VecAgg::Sum   => batch_sum_full(vals),
        VecAgg::Count => vals.len() as f64,
        VecAgg::Min   => vals.iter().copied().fold(f64::INFINITY, f64::min),
        VecAgg::Max   => vals.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        VecAgg::Avg   => if vals.is_empty() { 0.0 } else { batch_sum_full(vals) / vals.len() as f64 },
    }
}

// ─── Vectorized GROUP BY ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GroupResult {
    pub key:   HashMap<String, String>,  // group key values
    pub aggs:  Vec<AggResult>,
}

/// Vectorized GROUP BY: uses FNV-1a hash keys (zero String allocation per row).
/// Then aggregates each group with SIMD batch_sum_full.
pub fn vectorized_group_by(
    block: &DataBlock,
    row_indices: &[usize],
    spec: &GroupBySpec,
) -> Vec<GroupResult> {
    use rayon::prelude::*;

    // Build group key → row indices map using u128 FNV hash
    // Parallel for large inputs (chunked, then merged)
    let n_rows = row_indices.len();
    let nthreads = rayon::current_num_threads();
    let use_parallel = n_rows >= 500_000;
    let nchunks = if use_parallel { (nthreads * 2).max(1) } else { 1 };
    let chunk_sz = ((n_rows + nchunks - 1) / nchunks).max(1);

    // Pre-locate group columns
    let gcols: Vec<Option<&Column>> = spec.group_cols.iter()
        .map(|name| block.columns.iter().find(|c| c.name == *name || c.name.ends_with(&format!(".{name}"))))
        .collect();

    #[inline(always)]
    fn make_key(gcols: &[Option<&Column>], row: usize) -> u128 {
        let mut k: u128 = 0xcbf29ce484222325_cbf29ce484222325u128;
        for (i, col_opt) in gcols.iter().enumerate() {
            let v: u64 = match col_opt {
                None => 0,
                Some(col) => match &col.data {
                    ColumnData::Int64(v)   => v.get(row).and_then(|x| *x).unwrap_or(i64::MIN) as u64,
                    ColumnData::Float64(v) => v.get(row).and_then(|x| *x).map(|f| f.to_bits()).unwrap_or(0),
                    ColumnData::Bool(v)    => v.get(row).and_then(|x| *x).unwrap_or(false) as u64,
                    ColumnData::Str(v)     => {
                        let s = v.get(row).and_then(|x| x.as_deref()).unwrap_or("");
                        let mut h: u64 = 14695981039346656037;
                        for b in s.bytes() { h ^= b as u64; h = h.wrapping_mul(1099511628211); }
                        h
                    }
                }
            };
            k = k.wrapping_add(v as u128)
                 .wrapping_mul(0x9e3779b97f4a7c15_f39cc0605cedc835u128)
                 .rotate_left((i as u32 * 11 + 7) % 127);
        }
        k
    }

    // Local maps per chunk
    type LocalMap = Vec<(u128, Vec<usize>)>;
    let local_maps: Vec<LocalMap> = (0..nchunks).into_par_iter().map(|c| {
        let start = c * chunk_sz;
        let end   = (start + chunk_sz).min(n_rows);
        if start >= end { return vec![]; }
        let mut local: HashMap<u128, Vec<usize>> = HashMap::new();
        let mut order: Vec<u128> = Vec::new();
        for &row in &row_indices[start..end] {
            let k = make_key(&gcols, row);
            if !local.contains_key(&k) { order.push(k); }
            local.entry(k).or_default().push(row);
        }
        order.into_iter().map(|k| { let v = local.remove(&k).unwrap(); (k, v) }).collect()
    }).collect();

    // Merge
    let mut group_map: HashMap<u128, Vec<usize>> = HashMap::new();
    let mut key_order: Vec<u128> = Vec::new();
    for local in local_maps {
        for (key, mut idxs) in local {
            if !group_map.contains_key(&key) { key_order.push(key); }
            group_map.entry(key).or_default().append(&mut idxs);
        }
    }

    // Aggregate each group
    key_order.iter().map(|&key| {
        let rows = &group_map[&key];
        let first = rows[0];

        // Reconstruct key values
        let key_vals: HashMap<String, String> = spec.group_cols.iter().zip(gcols.iter())
            .map(|(name, col_opt)| {
                let val = match col_opt {
                    None => "null".to_string(),
                    Some(col) => match &col.data {
                        ColumnData::Int64(v)   => v.get(first).and_then(|x| *x).map(|i| i.to_string()).unwrap_or_default(),
                        ColumnData::Float64(v) => v.get(first).and_then(|x| *x).map(|f| format!("{f:.4}")).unwrap_or_default(),
                        ColumnData::Bool(v)    => v.get(first).and_then(|x| *x).map(|b| b.to_string()).unwrap_or_default(),
                        ColumnData::Str(v)     => v.get(first).and_then(|x| x.clone()).unwrap_or_default(),
                    }
                };
                (name.clone(), val)
            }).collect();

        let agg_results = vectorized_agg(block, rows, &spec.aggs);
        GroupResult { key: key_vals, aggs: agg_results }
    }).collect()
}

// ─── High-level API ───────────────────────────────────────────────────────────

/// Full pipeline: filter → group by → aggregate, all vectorized.
pub fn execute_vectorized(
    block: &DataBlock,
    filter: Option<&VecFilter>,
    group_by: Option<&GroupBySpec>,
) -> Vec<GroupResult> {
    let filtered_rows = match filter {
        Some(f) => vectorized_filter(block, f),
        None    => (0..block.num_rows).collect(),
    };

    match group_by {
        Some(g) => vectorized_group_by(block, &filtered_rows, g),
        None    => {
            // Global aggregation — single group
            if !filtered_rows.is_empty() {
                let aggs: Vec<AggResult> = vec![];
                vec![GroupResult { key: HashMap::new(), aggs }]
            } else {
                vec![]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::types::{Column, ColumnData, DataBlock};

    fn make_block(n: usize) -> DataBlock {
        DataBlock {
            num_rows: n,
            columns: vec![
                Column { name: "amount".into(), data: ColumnData::Float64(
                    (0..n).map(|i| Some(i as f64)).collect()
                )},
                Column { name: "cat".into(), data: ColumnData::Str(
                    (0..n).map(|i| Some(["A","B","C"][i%3].to_string())).collect()
                )},
            ],
        }
    }

    #[test]
    fn test_vectorized_filter() {
        let block = make_block(1000);
        let filter = VecFilter { conditions: vec![
            ColCondition { col_name: "amount".into(), op: CmpOp::Lt, threshold: 100.0, str_value: None },
        ]};
        let rows = vectorized_filter(&block, &filter);
        assert_eq!(rows.len(), 100);
    }

    #[test]
    fn test_vectorized_sum() {
        let block = make_block(100);
        let all_rows: Vec<usize> = (0..100).collect();
        let specs = vec![AggSpec { input_col: "amount".into(), agg: VecAgg::Sum, output_col: "total".into() }];
        let results = vectorized_agg(&block, &all_rows, &specs);
        assert!((results[0].value - 4950.0).abs() < 0.01); // sum(0..100)
    }

    #[test]
    fn test_vectorized_group_by() {
        let block = make_block(300);
        let all_rows: Vec<usize> = (0..300).collect();
        let spec = GroupBySpec {
            group_cols: vec!["cat".into()],
            aggs: vec![AggSpec { input_col: "amount".into(), agg: VecAgg::Count, output_col: "cnt".into() }],
        };
        let results = vectorized_group_by(&block, &all_rows, &spec);
        assert_eq!(results.len(), 3);  // 3 distinct categories
        for r in &results { assert_eq!(r.aggs[0].value, 100.0); }  // 100 rows each
    }
}
