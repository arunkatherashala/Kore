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
    _block: &DataBlock,
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
                    ColumnData::StrDict { codes, dict } => {
                        let c = codes.get(row).copied().unwrap_or(u8::MAX);
                        let s = if c == u8::MAX { "" } else { dict.get(c as usize).map(|x| x.as_str()).unwrap_or("") };
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
                        ColumnData::StrDict { codes, dict } => {
                            let c = codes.get(first).copied().unwrap_or(u8::MAX);
                            if c == u8::MAX { String::new() } else { dict.get(c as usize).cloned().unwrap_or_default() }
                        }
                    }
                };
                (name.clone(), val)
            }).collect();

        let agg_results = vectorized_agg(block, rows, &spec.aggs);
        GroupResult { key: key_vals, aggs: agg_results }
    }).collect()
}

// ─── Vectorized Hash Join ─────────────────────────────────────────────────────

/// FNV-1a hash for a column value at a given row.
#[inline(always)]
fn fnv1a_column_value(col: &Column, row: usize) -> u64 {
    const FNV_OFFSET: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;
    match &col.data {
        ColumnData::Int64(v) => {
            let val = v.get(row).and_then(|x| *x).unwrap_or(i64::MIN);
            let bytes = val.to_le_bytes();
            let mut h = FNV_OFFSET;
            for b in &bytes { h ^= *b as u64; h = h.wrapping_mul(FNV_PRIME); }
            h
        }
        ColumnData::Float64(v) => {
            let val = v.get(row).and_then(|x| *x).unwrap_or(f64::NAN);
            let bytes = val.to_bits().to_le_bytes();
            let mut h = FNV_OFFSET;
            for b in &bytes { h ^= *b as u64; h = h.wrapping_mul(FNV_PRIME); }
            h
        }
        ColumnData::Str(v) => {
            let s = v.get(row).and_then(|x| x.as_deref()).unwrap_or("");
            let mut h = FNV_OFFSET;
            for b in s.bytes() { h ^= b as u64; h = h.wrapping_mul(FNV_PRIME); }
            h
        }
        ColumnData::StrDict { codes, dict } => {
            let c = codes.get(row).copied().unwrap_or(u8::MAX);
            let s = if c == u8::MAX { "" } else { dict.get(c as usize).map(|x| x.as_str()).unwrap_or("") };
            let mut h = FNV_OFFSET;
            for b in s.bytes() { h ^= b as u64; h = h.wrapping_mul(FNV_PRIME); }
            h
        }
        ColumnData::Bool(v) => {
            let val = v.get(row).and_then(|x| *x).unwrap_or(false) as u8;
            let mut h = FNV_OFFSET;
            h ^= val as u64; h = h.wrapping_mul(FNV_PRIME);
            h
        }
    }
}

/// Compare two column values for equality (used in hash join probe).
#[inline]
fn column_values_eq(left_col: &Column, left_row: usize, right_col: &Column, right_row: usize) -> bool {
    match (&left_col.data, &right_col.data) {
        (ColumnData::Int64(lv), ColumnData::Int64(rv)) => {
            lv.get(left_row).and_then(|x| *x) == rv.get(right_row).and_then(|x| *x)
        }
        (ColumnData::Float64(lv), ColumnData::Float64(rv)) => {
            match (lv.get(left_row).and_then(|x| *x), rv.get(right_row).and_then(|x| *x)) {
                (Some(a), Some(b)) => (a - b).abs() < 1e-10,
                (None, None) => true,
                _ => false,
            }
        }
        (ColumnData::Str(lv), ColumnData::Str(rv)) => {
            lv.get(left_row).and_then(|x| x.as_deref()) == rv.get(right_row).and_then(|x| x.as_deref())
        }
        (ColumnData::Int64(lv), ColumnData::Float64(rv)) => {
            match (lv.get(left_row).and_then(|x| *x), rv.get(right_row).and_then(|x| *x)) {
                (Some(a), Some(b)) => (a as f64 - b).abs() < 1e-10,
                _ => false,
            }
        }
        (ColumnData::Float64(lv), ColumnData::Int64(rv)) => {
            match (lv.get(left_row).and_then(|x| *x), rv.get(right_row).and_then(|x| *x)) {
                (Some(a), Some(b)) => (a - b as f64).abs() < 1e-10,
                _ => false,
            }
        }
        _ => false,
    }
}

/// Vectorized hash join (inner join): build on right, probe from left in batches.
///
/// Uses FNV-1a hashing for the build phase and processes the probe side
/// in batches of BATCH_SIZE for cache-friendly access patterns.
pub fn vectorized_hash_join(
    left: &DataBlock,
    right: &DataBlock,
    left_key: &str,
    right_key: &str,
) -> DataBlock {
    let left_col = left.columns.iter().find(|c| c.name == left_key);
    let right_col = right.columns.iter().find(|c| c.name == right_key);

    let (left_col, right_col) = match (left_col, right_col) {
        (Some(l), Some(r)) => (l, r),
        _ => return DataBlock::empty(),
    };

    // Build phase: hash right side into HashMap<hash, Vec<row_idx>>
    let mut build_table: HashMap<u64, Vec<usize>> = HashMap::with_capacity(right.num_rows);
    for row in 0..right.num_rows {
        let h = fnv1a_column_value(right_col, row);
        build_table.entry(h).or_default().push(row);
    }

    // Probe phase: scan left in BATCH_SIZE chunks, collect (left_idx, right_idx) pairs
    let mut left_indices: Vec<usize> = Vec::new();
    let mut right_indices: Vec<usize> = Vec::new();

    let n_left = left.num_rows;
    let mut batch_start = 0;
    while batch_start < n_left {
        let batch_end = (batch_start + BATCH_SIZE).min(n_left);

        for left_row in batch_start..batch_end {
            let h = fnv1a_column_value(left_col, left_row);
            if let Some(candidates) = build_table.get(&h) {
                for &right_row in candidates {
                    if column_values_eq(left_col, left_row, right_col, right_row) {
                        left_indices.push(left_row);
                        right_indices.push(right_row);
                    }
                }
            }
        }

        batch_start = batch_end;
    }

    // Build output: all left columns + all right columns (except right join key)
    let mut out_columns: Vec<Column> = Vec::new();

    for col in &left.columns {
        out_columns.push(Column {
            name: col.name.clone(),
            data: col.data.take_rows(&left_indices),
        });
    }
    for col in &right.columns {
        if col.name == right_key { continue; }
        let out_name = if left.columns.iter().any(|c| c.name == col.name) {
            format!("{}_right", col.name)
        } else {
            col.name.clone()
        };
        out_columns.push(Column {
            name: out_name,
            data: col.data.take_rows(&right_indices),
        });
    }

    DataBlock { columns: out_columns, num_rows: left_indices.len() }
}

// ─── Vectorized Sort-Merge Join ──────────────────────────────────────────────

/// Vectorized sort-merge join (inner join): sort both sides, then two-pointer merge.
///
/// Handles duplicate keys by tracking run boundaries and producing the
/// cross product for matching key groups, processed in batches.
pub fn vectorized_sort_merge_join(
    left: &DataBlock,
    right: &DataBlock,
    left_key: &str,
    right_key: &str,
) -> DataBlock {
    // Sort both sides by key column
    let left_sorted = match left.sort_by(left_key, true) {
        Ok(b) => b,
        Err(_) => return DataBlock::empty(),
    };
    let right_sorted = match right.sort_by(right_key, true) {
        Ok(b) => b,
        Err(_) => return DataBlock::empty(),
    };

    let left_col = match left_sorted.columns.iter().find(|c| c.name == left_key) {
        Some(c) => c,
        None => return DataBlock::empty(),
    };
    let right_col = match right_sorted.columns.iter().find(|c| c.name == right_key) {
        Some(c) => c,
        None => return DataBlock::empty(),
    };

    let n_left = left_sorted.num_rows;
    let n_right = right_sorted.num_rows;

    let mut left_indices: Vec<usize> = Vec::new();
    let mut right_indices: Vec<usize> = Vec::new();

    let mut li = 0usize;
    let mut ri = 0usize;

    while li < n_left && ri < n_right {
        let cmp = compare_column_values(left_col, li, right_col, ri);
        match cmp {
            std::cmp::Ordering::Less => { li += 1; }
            std::cmp::Ordering::Greater => { ri += 1; }
            std::cmp::Ordering::Equal => {
                // Find the extent of duplicates on both sides
                let li_start = li;
                while li < n_left && compare_column_values(left_col, li, left_col, li_start) == std::cmp::Ordering::Equal {
                    li += 1;
                }
                let ri_start = ri;
                while ri < n_right && compare_column_values(right_col, ri, right_col, ri_start) == std::cmp::Ordering::Equal {
                    ri += 1;
                }
                // Cross-product of matching groups, in batches
                let mut pair_count = 0;
                for l in li_start..li {
                    for r in ri_start..ri {
                        left_indices.push(l);
                        right_indices.push(r);
                        pair_count += 1;
                        if pair_count % BATCH_SIZE == 0 {
                            // Batch boundary — continue (allows future async yield points)
                        }
                    }
                }
            }
        }
    }

    // Build output from sorted blocks
    let mut out_columns: Vec<Column> = Vec::new();

    for col in &left_sorted.columns {
        out_columns.push(Column {
            name: col.name.clone(),
            data: col.data.take_rows(&left_indices),
        });
    }
    for col in &right_sorted.columns {
        if col.name == right_key { continue; }
        let out_name = if left_sorted.columns.iter().any(|c| c.name == col.name) {
            format!("{}_right", col.name)
        } else {
            col.name.clone()
        };
        out_columns.push(Column {
            name: out_name,
            data: col.data.take_rows(&right_indices),
        });
    }

    DataBlock { columns: out_columns, num_rows: left_indices.len() }
}

/// Compare column values at given rows for ordering (used in sort-merge join).
fn compare_column_values(col_a: &Column, row_a: usize, col_b: &Column, row_b: usize) -> std::cmp::Ordering {
    match (&col_a.data, &col_b.data) {
        (ColumnData::Int64(va), ColumnData::Int64(vb)) => {
            let a = va.get(row_a).and_then(|x| *x);
            let b = vb.get(row_b).and_then(|x| *x);
            match (a, b) {
                (Some(x), Some(y)) => x.cmp(&y),
                (None, None) => std::cmp::Ordering::Equal,
                (None, _) => std::cmp::Ordering::Less,
                (_, None) => std::cmp::Ordering::Greater,
            }
        }
        (ColumnData::Float64(va), ColumnData::Float64(vb)) => {
            let a = va.get(row_a).and_then(|x| *x);
            let b = vb.get(row_b).and_then(|x| *x);
            match (a, b) {
                (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                (None, None) => std::cmp::Ordering::Equal,
                (None, _) => std::cmp::Ordering::Less,
                (_, None) => std::cmp::Ordering::Greater,
            }
        }
        (ColumnData::Str(va), ColumnData::Str(vb)) => {
            let a = va.get(row_a).and_then(|x| x.as_deref());
            let b = vb.get(row_b).and_then(|x| x.as_deref());
            match (a, b) {
                (Some(x), Some(y)) => x.cmp(y),
                (None, None) => std::cmp::Ordering::Equal,
                (None, _) => std::cmp::Ordering::Less,
                (_, None) => std::cmp::Ordering::Greater,
            }
        }
        (ColumnData::StrDict { codes: ca, dict: da }, ColumnData::StrDict { codes: cb, dict: db }) => {
            let a_code = ca.get(row_a).copied().unwrap_or(u8::MAX);
            let b_code = cb.get(row_b).copied().unwrap_or(u8::MAX);
            let a_s = if a_code == u8::MAX { None } else { da.get(a_code as usize).map(|s| s.as_str()) };
            let b_s = if b_code == u8::MAX { None } else { db.get(b_code as usize).map(|s| s.as_str()) };
            match (a_s, b_s) {
                (Some(x), Some(y)) => x.cmp(y),
                (None, None) => std::cmp::Ordering::Equal,
                (None, _) => std::cmp::Ordering::Less,
                (_, None) => std::cmp::Ordering::Greater,
            }
        }
        _ => std::cmp::Ordering::Equal,
    }
}

// ─── Vectorized Window Functions ─────────────────────────────────────────────

/// Window function variants for vectorized computation.
#[derive(Debug, Clone)]
pub enum VecWindowFn {
    RowNumber,
    Rank,
    DenseRank,
    Lag(usize),
    Lead(usize),
    RunningSum,
    RunningAvg,
}

/// Vectorized window function: partition, sort within partition, compute window values.
///
/// Returns the original block with an additional column `_window` containing
/// the computed window function result as Float64.
pub fn vectorized_window(
    block: &DataBlock,
    partition_col: Option<&str>,
    order_col: &str,
    func: VecWindowFn,
) -> DataBlock {
    if block.num_rows == 0 {
        let mut cols = block.columns.clone();
        cols.push(Column::float64("_window", vec![]));
        return DataBlock { columns: cols, num_rows: 0 };
    }

    // Build partition groups: Vec<Vec<usize>> where each inner vec is sorted row indices
    let partitions = build_partitions(block, partition_col, order_col);

    // Compute window function values, placing results at original row positions
    let mut window_values: Vec<Option<f64>> = vec![None; block.num_rows];

    let order_col_ref = block.columns.iter().find(|c| c.name == order_col);

    for partition in &partitions {
        compute_window_for_partition(block, partition, order_col_ref, &func, &mut window_values);
    }

    let mut out_columns = block.columns.clone();
    out_columns.push(Column::float64("_window", window_values));
    DataBlock { columns: out_columns, num_rows: block.num_rows }
}

/// Group row indices by partition column value, sorted by order column within each partition.
fn build_partitions(block: &DataBlock, partition_col: Option<&str>, order_col: &str) -> Vec<Vec<usize>> {
    let all_rows: Vec<usize> = (0..block.num_rows).collect();

    let groups: Vec<Vec<usize>> = match partition_col {
        None => vec![all_rows],
        Some(pcol) => {
            let col = match block.columns.iter().find(|c| c.name == pcol) {
                Some(c) => c,
                None => return vec![all_rows],
            };
            // Group by partition value using FNV-1a hash
            let mut map: HashMap<u64, Vec<usize>> = HashMap::new();
            let mut order: Vec<u64> = Vec::new();
            for row in 0..block.num_rows {
                let h = fnv1a_column_value(col, row);
                if !map.contains_key(&h) { order.push(h); }
                map.entry(h).or_default().push(row);
            }
            order.into_iter().map(|k| map.remove(&k).unwrap()).collect()
        }
    };

    // Sort each group by order_col
    let order_col_ref = block.columns.iter().find(|c| c.name == order_col);
    groups.into_iter().map(|mut group| {
        if let Some(ocol) = order_col_ref {
            group.sort_by(|&a, &b| {
                compare_column_values_single(ocol, a, ocol, b)
            });
        }
        group
    }).collect()
}

/// Compare values within a single column at two row positions.
fn compare_column_values_single(col: &Column, row_a: usize, _col_b: &Column, row_b: usize) -> std::cmp::Ordering {
    compare_column_values(col, row_a, col, row_b)
}

/// Compute window function values for a single partition (already sorted).
fn compute_window_for_partition(
    block: &DataBlock,
    partition: &[usize],
    order_col: Option<&Column>,
    func: &VecWindowFn,
    output: &mut [Option<f64>],
) {
    match func {
        VecWindowFn::RowNumber => {
            for (i, &row) in partition.iter().enumerate() {
                output[row] = Some((i + 1) as f64);
            }
        }

        VecWindowFn::Rank => {
            if partition.is_empty() { return; }
            output[partition[0]] = Some(1.0);
            for i in 1..partition.len() {
                let prev = partition[i - 1];
                let curr = partition[i];
                let same = match order_col {
                    Some(c) => compare_column_values(c, prev, c, curr) == std::cmp::Ordering::Equal,
                    None => true,
                };
                if same {
                    output[curr] = output[prev];
                } else {
                    output[curr] = Some((i + 1) as f64);
                }
            }
        }

        VecWindowFn::DenseRank => {
            if partition.is_empty() { return; }
            let mut dense = 1.0f64;
            output[partition[0]] = Some(dense);
            for i in 1..partition.len() {
                let prev = partition[i - 1];
                let curr = partition[i];
                let same = match order_col {
                    Some(c) => compare_column_values(c, prev, c, curr) == std::cmp::Ordering::Equal,
                    None => true,
                };
                if !same { dense += 1.0; }
                output[curr] = Some(dense);
            }
        }

        VecWindowFn::Lag(offset) => {
            for (i, &row) in partition.iter().enumerate() {
                if i >= *offset {
                    let source_row = partition[i - offset];
                    output[row] = get_numeric_value(block, order_col, source_row);
                } else {
                    output[row] = None;
                }
            }
        }

        VecWindowFn::Lead(offset) => {
            for (i, &row) in partition.iter().enumerate() {
                if i + offset < partition.len() {
                    let source_row = partition[i + offset];
                    output[row] = get_numeric_value(block, order_col, source_row);
                } else {
                    output[row] = None;
                }
            }
        }

        VecWindowFn::RunningSum => {
            let mut sum = 0.0f64;
            for &row in partition {
                if let Some(val) = get_numeric_value(block, order_col, row) {
                    sum += val;
                }
                output[row] = Some(sum);
            }
        }

        VecWindowFn::RunningAvg => {
            let mut sum = 0.0f64;
            let mut count = 0u64;
            for &row in partition {
                if let Some(val) = get_numeric_value(block, order_col, row) {
                    sum += val;
                    count += 1;
                }
                output[row] = if count > 0 { Some(sum / count as f64) } else { None };
            }
        }
    }
}

/// Extract numeric value from the order column at a given row.
#[inline]
fn get_numeric_value(_block: &DataBlock, order_col: Option<&Column>, row: usize) -> Option<f64> {
    match order_col {
        None => None,
        Some(col) => match &col.data {
            ColumnData::Int64(v) => v.get(row).and_then(|x| *x).map(|i| i as f64),
            ColumnData::Float64(v) => v.get(row).and_then(|x| *x),
            _ => None,
        }
    }
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

    // ─── Hash Join Tests ─────────────────────────────────────────────────────

    fn make_join_blocks() -> (DataBlock, DataBlock) {
        let left = DataBlock {
            num_rows: 5,
            columns: vec![
                Column::int64("id", vec![Some(1), Some(2), Some(3), Some(4), Some(5)]),
                Column::str_col("name", vec![
                    Some("Alice".into()), Some("Bob".into()), Some("Charlie".into()),
                    Some("Diana".into()), Some("Eve".into()),
                ]),
            ],
        };
        let right = DataBlock {
            num_rows: 4,
            columns: vec![
                Column::int64("id", vec![Some(2), Some(3), Some(5), Some(7)]),
                Column::float64("score", vec![Some(85.0), Some(92.0), Some(78.0), Some(99.0)]),
            ],
        };
        (left, right)
    }

    #[test]
    fn test_hash_join_basic() {
        let (left, right) = make_join_blocks();
        let result = vectorized_hash_join(&left, &right, "id", "id");
        assert_eq!(result.num_rows, 3); // ids 2, 3, 5 match
        let id_col = result.column("id").unwrap();
        if let ColumnData::Int64(vals) = &id_col.data {
            let mut ids: Vec<i64> = vals.iter().filter_map(|x| *x).collect();
            ids.sort();
            assert_eq!(ids, vec![2, 3, 5]);
        } else { panic!("expected Int64 column"); }
    }

    #[test]
    fn test_hash_join_with_duplicates() {
        let left = DataBlock {
            num_rows: 4,
            columns: vec![
                Column::int64("key", vec![Some(1), Some(1), Some(2), Some(3)]),
                Column::str_col("val", vec![
                    Some("a".into()), Some("b".into()), Some("c".into()), Some("d".into()),
                ]),
            ],
        };
        let right = DataBlock {
            num_rows: 3,
            columns: vec![
                Column::int64("key", vec![Some(1), Some(1), Some(2)]),
                Column::float64("score", vec![Some(10.0), Some(20.0), Some(30.0)]),
            ],
        };
        let result = vectorized_hash_join(&left, &right, "key", "key");
        // left key=1 (2 rows) × right key=1 (2 rows) = 4 matches, plus left key=2 × right key=2 = 1
        assert_eq!(result.num_rows, 5);
    }

    #[test]
    fn test_hash_join_no_matches() {
        let left = DataBlock {
            num_rows: 2,
            columns: vec![
                Column::int64("id", vec![Some(10), Some(20)]),
            ],
        };
        let right = DataBlock {
            num_rows: 2,
            columns: vec![
                Column::int64("id", vec![Some(30), Some(40)]),
            ],
        };
        let result = vectorized_hash_join(&left, &right, "id", "id");
        assert_eq!(result.num_rows, 0);
    }

    #[test]
    fn test_hash_join_string_keys() {
        let left = DataBlock {
            num_rows: 3,
            columns: vec![
                Column::str_col("dept", vec![
                    Some("eng".into()), Some("sales".into()), Some("eng".into()),
                ]),
                Column::int64("emp_id", vec![Some(1), Some(2), Some(3)]),
            ],
        };
        let right = DataBlock {
            num_rows: 2,
            columns: vec![
                Column::str_col("dept", vec![Some("eng".into()), Some("hr".into())]),
                Column::float64("budget", vec![Some(1_000_000.0), Some(500_000.0)]),
            ],
        };
        let result = vectorized_hash_join(&left, &right, "dept", "dept");
        assert_eq!(result.num_rows, 2); // two "eng" rows from left match
    }

    #[test]
    fn test_hash_join_missing_column() {
        let left = DataBlock {
            num_rows: 2,
            columns: vec![Column::int64("a", vec![Some(1), Some(2)])],
        };
        let right = DataBlock {
            num_rows: 2,
            columns: vec![Column::int64("b", vec![Some(1), Some(2)])],
        };
        let result = vectorized_hash_join(&left, &right, "x", "y");
        assert_eq!(result.num_rows, 0);
    }

    #[test]
    fn test_hash_join_large_batch() {
        let n = 2048; // > BATCH_SIZE to exercise batching
        let left = DataBlock {
            num_rows: n,
            columns: vec![
                Column::int64("id", (0..n as i64).map(|i| Some(i)).collect()),
                Column::float64("val", (0..n).map(|i| Some(i as f64 * 1.5)).collect()),
            ],
        };
        let right = DataBlock {
            num_rows: n,
            columns: vec![
                Column::int64("id", (0..n as i64).map(|i| Some(i)).collect()),
                Column::float64("score", (0..n).map(|i| Some(i as f64 * 2.0)).collect()),
            ],
        };
        let result = vectorized_hash_join(&left, &right, "id", "id");
        assert_eq!(result.num_rows, n);
    }

    // ─── Sort-Merge Join Tests ───────────────────────────────────────────────

    #[test]
    fn test_sort_merge_join_basic() {
        let (left, right) = make_join_blocks();
        let result = vectorized_sort_merge_join(&left, &right, "id", "id");
        assert_eq!(result.num_rows, 3); // ids 2, 3, 5 match
    }

    #[test]
    fn test_sort_merge_join_with_duplicates() {
        let left = DataBlock {
            num_rows: 4,
            columns: vec![
                Column::int64("key", vec![Some(1), Some(1), Some(2), Some(3)]),
                Column::str_col("val", vec![
                    Some("a".into()), Some("b".into()), Some("c".into()), Some("d".into()),
                ]),
            ],
        };
        let right = DataBlock {
            num_rows: 3,
            columns: vec![
                Column::int64("key", vec![Some(1), Some(1), Some(2)]),
                Column::float64("score", vec![Some(10.0), Some(20.0), Some(30.0)]),
            ],
        };
        let result = vectorized_sort_merge_join(&left, &right, "key", "key");
        assert_eq!(result.num_rows, 5); // 2×2 + 1×1
    }

    #[test]
    fn test_sort_merge_join_unsorted_input() {
        let left = DataBlock {
            num_rows: 4,
            columns: vec![
                Column::int64("id", vec![Some(5), Some(1), Some(3), Some(2)]),
                Column::str_col("data", vec![
                    Some("e".into()), Some("a".into()), Some("c".into()), Some("b".into()),
                ]),
            ],
        };
        let right = DataBlock {
            num_rows: 3,
            columns: vec![
                Column::int64("id", vec![Some(3), Some(1), Some(5)]),
                Column::float64("v", vec![Some(30.0), Some(10.0), Some(50.0)]),
            ],
        };
        let result = vectorized_sort_merge_join(&left, &right, "id", "id");
        assert_eq!(result.num_rows, 3); // ids 1, 3, 5 match
    }

    #[test]
    fn test_sort_merge_join_no_matches() {
        let left = DataBlock {
            num_rows: 2,
            columns: vec![Column::int64("id", vec![Some(1), Some(2)])],
        };
        let right = DataBlock {
            num_rows: 2,
            columns: vec![Column::int64("id", vec![Some(3), Some(4)])],
        };
        let result = vectorized_sort_merge_join(&left, &right, "id", "id");
        assert_eq!(result.num_rows, 0);
    }

    #[test]
    fn test_sort_merge_join_string_keys() {
        let left = DataBlock {
            num_rows: 3,
            columns: vec![
                Column::str_col("name", vec![
                    Some("bob".into()), Some("alice".into()), Some("charlie".into()),
                ]),
                Column::int64("age", vec![Some(30), Some(25), Some(35)]),
            ],
        };
        let right = DataBlock {
            num_rows: 2,
            columns: vec![
                Column::str_col("name", vec![Some("alice".into()), Some("charlie".into())]),
                Column::float64("salary", vec![Some(75000.0), Some(90000.0)]),
            ],
        };
        let result = vectorized_sort_merge_join(&left, &right, "name", "name");
        assert_eq!(result.num_rows, 2);
    }

    #[test]
    fn test_join_results_match() {
        let (left, right) = make_join_blocks();
        let hash_result = vectorized_hash_join(&left, &right, "id", "id");
        let merge_result = vectorized_sort_merge_join(&left, &right, "id", "id");
        assert_eq!(hash_result.num_rows, merge_result.num_rows);
    }

    // ─── Window Function Tests ───────────────────────────────────────────────

    fn make_window_block() -> DataBlock {
        DataBlock {
            num_rows: 8,
            columns: vec![
                Column::str_col("dept", vec![
                    Some("eng".into()), Some("eng".into()), Some("eng".into()),
                    Some("sales".into()), Some("sales".into()),
                    Some("hr".into()), Some("hr".into()), Some("hr".into()),
                ]),
                Column::int64("salary", vec![
                    Some(100), Some(120), Some(110),
                    Some(90), Some(95),
                    Some(80), Some(85), Some(82),
                ]),
            ],
        }
    }

    #[test]
    fn test_window_row_number_no_partition() {
        let block = DataBlock {
            num_rows: 5,
            columns: vec![
                Column::int64("val", vec![Some(50), Some(30), Some(40), Some(10), Some(20)]),
            ],
        };
        let result = vectorized_window(&block, None, "val", VecWindowFn::RowNumber);
        assert_eq!(result.num_rows, 5);
        let win_col = result.column("_window").unwrap();
        if let ColumnData::Float64(vals) = &win_col.data {
            // Sorted order: 10(row3), 20(row4), 30(row1), 40(row2), 50(row0)
            assert_eq!(vals[3], Some(1.0)); // val=10 is rank 1
            assert_eq!(vals[4], Some(2.0)); // val=20 is rank 2
            assert_eq!(vals[1], Some(3.0)); // val=30 is rank 3
            assert_eq!(vals[2], Some(4.0)); // val=40 is rank 4
            assert_eq!(vals[0], Some(5.0)); // val=50 is rank 5
        } else { panic!("expected Float64"); }
    }

    #[test]
    fn test_window_row_number_with_partition() {
        let block = make_window_block();
        let result = vectorized_window(&block, Some("dept"), "salary", VecWindowFn::RowNumber);
        let win_col = result.column("_window").unwrap();
        if let ColumnData::Float64(vals) = &win_col.data {
            // eng partition (rows 0,1,2) sorted by salary: 100,110,120 → row_numbers 1,2,3
            // Original row 0 (salary=100) → row_number 1
            assert_eq!(vals[0], Some(1.0));
            // Original row 2 (salary=110) → row_number 2
            assert_eq!(vals[2], Some(2.0));
            // Original row 1 (salary=120) → row_number 3
            assert_eq!(vals[1], Some(3.0));
        } else { panic!("expected Float64"); }
    }

    #[test]
    fn test_window_rank() {
        let block = DataBlock {
            num_rows: 5,
            columns: vec![
                Column::int64("score", vec![Some(100), Some(90), Some(100), Some(80), Some(90)]),
            ],
        };
        let result = vectorized_window(&block, None, "score", VecWindowFn::Rank);
        let win_col = result.column("_window").unwrap();
        if let ColumnData::Float64(vals) = &win_col.data {
            // Sorted: 80(row3), 90(row1), 90(row4), 100(row0), 100(row2)
            assert_eq!(vals[3], Some(1.0)); // 80 → rank 1
            // 90s share rank 2
            assert_eq!(vals[1], Some(2.0));
            assert_eq!(vals[4], Some(2.0));
            // 100s share rank 4
            assert_eq!(vals[0], Some(4.0));
            assert_eq!(vals[2], Some(4.0));
        } else { panic!("expected Float64"); }
    }

    #[test]
    fn test_window_dense_rank() {
        let block = DataBlock {
            num_rows: 5,
            columns: vec![
                Column::int64("score", vec![Some(100), Some(90), Some(100), Some(80), Some(90)]),
            ],
        };
        let result = vectorized_window(&block, None, "score", VecWindowFn::DenseRank);
        let win_col = result.column("_window").unwrap();
        if let ColumnData::Float64(vals) = &win_col.data {
            // Sorted: 80(row3), 90(row1), 90(row4), 100(row0), 100(row2)
            assert_eq!(vals[3], Some(1.0)); // 80 → dense_rank 1
            assert_eq!(vals[1], Some(2.0)); // 90 → dense_rank 2
            assert_eq!(vals[4], Some(2.0)); // 90 → dense_rank 2
            assert_eq!(vals[0], Some(3.0)); // 100 → dense_rank 3
            assert_eq!(vals[2], Some(3.0)); // 100 → dense_rank 3
        } else { panic!("expected Float64"); }
    }

    #[test]
    fn test_window_lag() {
        let block = DataBlock {
            num_rows: 5,
            columns: vec![
                Column::int64("val", vec![Some(10), Some(20), Some(30), Some(40), Some(50)]),
            ],
        };
        let result = vectorized_window(&block, None, "val", VecWindowFn::Lag(1));
        let win_col = result.column("_window").unwrap();
        if let ColumnData::Float64(vals) = &win_col.data {
            // Already sorted (10,20,30,40,50). Lag(1) gives previous value in sorted order.
            assert_eq!(vals[0], None);       // first in sort → no lag
            assert_eq!(vals[1], Some(10.0)); // lag of 20 = 10
            assert_eq!(vals[2], Some(20.0)); // lag of 30 = 20
            assert_eq!(vals[3], Some(30.0)); // lag of 40 = 30
            assert_eq!(vals[4], Some(40.0)); // lag of 50 = 40
        } else { panic!("expected Float64"); }
    }

    #[test]
    fn test_window_lead() {
        let block = DataBlock {
            num_rows: 5,
            columns: vec![
                Column::int64("val", vec![Some(10), Some(20), Some(30), Some(40), Some(50)]),
            ],
        };
        let result = vectorized_window(&block, None, "val", VecWindowFn::Lead(1));
        let win_col = result.column("_window").unwrap();
        if let ColumnData::Float64(vals) = &win_col.data {
            assert_eq!(vals[0], Some(20.0)); // lead of 10 = 20
            assert_eq!(vals[1], Some(30.0)); // lead of 20 = 30
            assert_eq!(vals[2], Some(40.0)); // lead of 30 = 40
            assert_eq!(vals[3], Some(50.0)); // lead of 40 = 50
            assert_eq!(vals[4], None);       // last in sort → no lead
        } else { panic!("expected Float64"); }
    }

    #[test]
    fn test_window_lag_offset_2() {
        let block = DataBlock {
            num_rows: 5,
            columns: vec![
                Column::int64("val", vec![Some(10), Some(20), Some(30), Some(40), Some(50)]),
            ],
        };
        let result = vectorized_window(&block, None, "val", VecWindowFn::Lag(2));
        let win_col = result.column("_window").unwrap();
        if let ColumnData::Float64(vals) = &win_col.data {
            assert_eq!(vals[0], None);
            assert_eq!(vals[1], None);
            assert_eq!(vals[2], Some(10.0));
            assert_eq!(vals[3], Some(20.0));
            assert_eq!(vals[4], Some(30.0));
        } else { panic!("expected Float64"); }
    }

    #[test]
    fn test_window_running_sum() {
        let block = DataBlock {
            num_rows: 4,
            columns: vec![
                Column::int64("val", vec![Some(10), Some(20), Some(30), Some(40)]),
            ],
        };
        let result = vectorized_window(&block, None, "val", VecWindowFn::RunningSum);
        let win_col = result.column("_window").unwrap();
        if let ColumnData::Float64(vals) = &win_col.data {
            assert_eq!(vals[0], Some(10.0));  // 10
            assert_eq!(vals[1], Some(30.0));  // 10+20
            assert_eq!(vals[2], Some(60.0));  // 10+20+30
            assert_eq!(vals[3], Some(100.0)); // 10+20+30+40
        } else { panic!("expected Float64"); }
    }

    #[test]
    fn test_window_running_avg() {
        let block = DataBlock {
            num_rows: 4,
            columns: vec![
                Column::int64("val", vec![Some(10), Some(20), Some(30), Some(40)]),
            ],
        };
        let result = vectorized_window(&block, None, "val", VecWindowFn::RunningAvg);
        let win_col = result.column("_window").unwrap();
        if let ColumnData::Float64(vals) = &win_col.data {
            assert!((vals[0].unwrap() - 10.0).abs() < 0.01);  // 10/1
            assert!((vals[1].unwrap() - 15.0).abs() < 0.01);  // 30/2
            assert!((vals[2].unwrap() - 20.0).abs() < 0.01);  // 60/3
            assert!((vals[3].unwrap() - 25.0).abs() < 0.01);  // 100/4
        } else { panic!("expected Float64"); }
    }

    #[test]
    fn test_window_running_sum_partitioned() {
        let block = DataBlock {
            num_rows: 6,
            columns: vec![
                Column::str_col("grp", vec![
                    Some("A".into()), Some("A".into()), Some("A".into()),
                    Some("B".into()), Some("B".into()), Some("B".into()),
                ]),
                Column::int64("val", vec![Some(10), Some(20), Some(30), Some(5), Some(15), Some(25)]),
            ],
        };
        let result = vectorized_window(&block, Some("grp"), "val", VecWindowFn::RunningSum);
        let win_col = result.column("_window").unwrap();
        if let ColumnData::Float64(vals) = &win_col.data {
            // Group A sorted: 10,20,30 → running sums: 10,30,60
            assert_eq!(vals[0], Some(10.0));
            assert_eq!(vals[1], Some(30.0));
            assert_eq!(vals[2], Some(60.0));
            // Group B sorted: 5,15,25 → running sums: 5,20,45
            assert_eq!(vals[3], Some(5.0));
            assert_eq!(vals[4], Some(20.0));
            assert_eq!(vals[5], Some(45.0));
        } else { panic!("expected Float64"); }
    }

    #[test]
    fn test_window_empty_block() {
        let block = DataBlock { num_rows: 0, columns: vec![
            Column::int64("val", vec![]),
        ]};
        let result = vectorized_window(&block, None, "val", VecWindowFn::RowNumber);
        assert_eq!(result.num_rows, 0);
        assert!(result.column("_window").is_some());
    }

    #[test]
    fn test_window_single_row() {
        let block = DataBlock {
            num_rows: 1,
            columns: vec![Column::int64("val", vec![Some(42)])],
        };
        let result = vectorized_window(&block, None, "val", VecWindowFn::RowNumber);
        let win_col = result.column("_window").unwrap();
        if let ColumnData::Float64(vals) = &win_col.data {
            assert_eq!(vals[0], Some(1.0));
        } else { panic!("expected Float64"); }
    }

    #[test]
    fn test_window_preserves_original_columns() {
        let block = make_window_block();
        let result = vectorized_window(&block, Some("dept"), "salary", VecWindowFn::RunningSum);
        assert!(result.column("dept").is_some());
        assert!(result.column("salary").is_some());
        assert!(result.column("_window").is_some());
        assert_eq!(result.columns.len(), 3);
    }
}
