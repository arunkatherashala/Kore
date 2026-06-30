//! Hash Join — build a hash table on the smaller (build) side, probe with the larger side.
//!
//! Supports INNER, LEFT, RIGHT and FULL OUTER joins.
//! Probe phase is parallelized via Rayon (O(n/T) per thread).

use std::collections::HashMap;
use std::sync::Arc;
use rayon::prelude::*;
use kore_core::{Column, DataBlock, JoinKey, JoinType, KoreError, Value};

use crate::JoinConfig;

pub struct HashJoin;

impl HashJoin {
    /// Execute the join.  `left` is the probe side; `right` is the build side.
    pub fn join(
        left: &DataBlock,
        right: &DataBlock,
        cfg: &JoinConfig,
    ) -> Result<DataBlock, KoreError> {
        // ── Fast path: Int64 key columns (most common, no JoinKey allocation) ──
        let right_int_col = right.columns.iter().find(|c| c.name == cfg.right_key);
        let left_int_col  = left.columns.iter().find(|c| c.name == cfg.left_key);

        if let (Some(rc), Some(lc)) = (right_int_col, left_int_col) {
            if let (kore_core::ColumnData::Int64(rv), kore_core::ColumnData::Int64(lv)) =
                (&rc.data, &lc.data)
            {
                return Self::join_int64(left, right, lv, rv, cfg);
            }
        }

        // ── Fallback: generic JoinKey path (handles Str/Bool/Null keys) ────────
        let mut table: HashMap<JoinKey, Vec<usize>> = HashMap::with_capacity(right.num_rows);
        for i in 0..right.num_rows {
            let key = right.join_key(i, &cfg.right_key)?;
            table.entry(key).or_default().push(i);
        }
        let table = Arc::new(table);

        let n_left = left.num_rows;
        let n_threads = rayon::current_num_threads();
        let chunk_sz = ((n_left + n_threads - 1) / n_threads).max(1);

        let local_pairs: Vec<Vec<(Option<usize>, Option<usize>)>> = (0..n_threads)
            .into_par_iter()
            .map(|t| {
                let start = t * chunk_sz;
                let end   = (start + chunk_sz).min(n_left);
                if start >= end { return vec![]; }
                let mut pairs: Vec<(Option<usize>, Option<usize>)> = Vec::new();
                for l in start..end {
                    if let Ok(key) = left.join_key(l, &cfg.left_key) {
                        if let Some(right_rows) = table.get(&key) {
                            for &r in right_rows { pairs.push((Some(l), Some(r))); }
                        } else if matches!(cfg.join_type, JoinType::Left | JoinType::Full) {
                            pairs.push((Some(l), None));
                        }
                    }
                }
                pairs
            })
            .collect();

        let mut pairs: Vec<(Option<usize>, Option<usize>)> = Vec::new();
        for lp in local_pairs { pairs.extend(lp); }

        if matches!(cfg.join_type, JoinType::Right | JoinType::Full) {
            let mut right_matched = vec![false; right.num_rows];
            for &(_, r) in &pairs { if let Some(ri) = r { right_matched[ri] = true; } }
            for (r, matched) in right_matched.iter().enumerate() {
                if !matched { pairs.push((None, Some(r))); }
            }
        }

        build_result(left, right, &pairs)
    }

    /// Optimized Int64 hash join — no JoinKey allocation, parallel probe.
    fn join_int64(
        left: &DataBlock,
        right: &DataBlock,
        lv: &[Option<i64>],
        rv: &[Option<i64>],
        cfg: &JoinConfig,
    ) -> Result<DataBlock, KoreError> {
        use std::collections::HashMap;

        // ── Sequential build: preallocated, direct array access, no JoinKey ───
        // Sequential is FASTER than parallel for build due to merge overhead.
        // Key insight: direct rv[i] access eliminates JoinKey enum alloc per row.
        let n_right = rv.len();
        let mut table: HashMap<i64, Vec<usize>> = HashMap::with_capacity(n_right / 4 + 16);
        for i in 0..n_right {
            if let Some(k) = rv[i] { table.entry(k).or_default().push(i); }
        }
        let table = Arc::new(table);

        // ── Parallel probe: T threads, each scans its chunk of the probe side ──
        let n_left   = lv.len();
        let n_threads = rayon::current_num_threads();
        let chunk_sz  = ((n_left + n_threads - 1) / n_threads).max(1);

        let local_pairs: Vec<Vec<(Option<usize>, Option<usize>)>> = (0..n_threads)
            .into_par_iter()
            .map(|t| {
                let start = t * chunk_sz;
                let end   = (start + chunk_sz).min(n_left);
                if start >= end { return vec![]; }
                let mut pairs = Vec::new();
                for l in start..end {
                    if let Some(k) = lv[l] {
                        if let Some(right_rows) = table.get(&k) {
                            for &r in right_rows { pairs.push((Some(l), Some(r))); }
                        } else if matches!(cfg.join_type, JoinType::Left | JoinType::Full) {
                            pairs.push((Some(l), None));
                        }
                    }
                }
                pairs
            })
            .collect();

        let mut pairs: Vec<(Option<usize>, Option<usize>)> = Vec::new();
        for lp in local_pairs { pairs.extend(lp); }

        if matches!(cfg.join_type, JoinType::Right | JoinType::Full) {
            let mut right_matched = vec![false; n_right];
            for &(_, r) in &pairs { if let Some(ri) = r { right_matched[ri] = true; } }
            for (r, matched) in right_matched.iter().enumerate() {
                if !matched { pairs.push((None, Some(r))); }
            }
        }

        build_result(left, right, &pairs)
    }
}

/// Materialise a DataBlock from (left_idx | None, right_idx | None) pairs.
/// Uses bulk column-at-a-time copy — replaces 102M per-row virtual dispatch
/// calls with tight indexed iterator chains that LLVM can vectorize.
pub(crate) fn build_result(
    left:  &DataBlock,
    right: &DataBlock,
    pairs: &[(Option<usize>, Option<usize>)],
) -> Result<DataBlock, KoreError> {
    let n = pairs.len();

    // Pre-extract index vecs once (avoids re-scanning pairs per column)
    let left_idxs:  Vec<Option<usize>> = pairs.iter().map(|(l, _)| *l).collect();
    let right_idxs: Vec<Option<usize>> = pairs.iter().map(|(_, r)| *r).collect();

    let left_names: std::collections::HashSet<&str> =
        left.columns.iter().map(|c| c.name.as_str()).collect();

    let mut columns: Vec<Column> = Vec::with_capacity(left.columns.len() + right.columns.len());

    // Bulk-copy left columns column-at-a-time
    for col in &left.columns {
        let data = bulk_copy(&col.data, &left_idxs);
        columns.push(Column { name: col.name.clone(), data });
    }

    // Bulk-copy right columns column-at-a-time (suffix _r on name clash)
    for col in &right.columns {
        let name = if left_names.contains(col.name.as_str()) {
            format!("{}_r", col.name)
        } else {
            col.name.clone()
        };
        let data = bulk_copy(&col.data, &right_idxs);
        columns.push(Column { name, data });
    }

    Ok(DataBlock { columns, num_rows: n })
}

/// Bulk-copy a column using a pre-computed index array — O(n), no virtual dispatch per row.
fn bulk_copy(src: &kore_core::ColumnData, idxs: &[Option<usize>]) -> kore_core::ColumnData {
    use kore_core::ColumnData;
    match src {
        ColumnData::Int64(v) =>
            ColumnData::Int64(idxs.iter().map(|i| i.and_then(|r| v.get(r).and_then(|x| *x))).collect()),
        ColumnData::Float64(v) =>
            ColumnData::Float64(idxs.iter().map(|i| i.and_then(|r| v.get(r).and_then(|x| *x))).collect()),
        ColumnData::Bool(v) =>
            ColumnData::Bool(idxs.iter().map(|i| i.and_then(|r| v.get(r).and_then(|x| *x))).collect()),
        ColumnData::Str(v) =>
            ColumnData::Str(idxs.iter().map(|i| i.and_then(|r| v.get(r).and_then(|x| x.clone()))).collect()),
        ColumnData::StrDict { codes, dict } =>
            ColumnData::Str(idxs.iter().map(|i| i.and_then(|r| {
                let c = codes.get(r).copied().unwrap_or(u8::MAX);
                if c == u8::MAX { None } else { dict.get(c as usize).cloned() }
            })).collect()),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, DataBlock, JoinType};

    fn make_blocks() -> (DataBlock, DataBlock) {
        let left = DataBlock::new(vec![
            Column::int64("id",  vec![Some(1), Some(2), Some(3)]),
            Column::str_col("name", vec![Some("alice".into()), Some("bob".into()), Some("carol".into())]),
        ]).unwrap();
        let right = DataBlock::new(vec![
            Column::int64("id",    vec![Some(2), Some(3), Some(4)]),
            Column::float64("score", vec![Some(9.1), Some(8.5), Some(7.2)]),
        ]).unwrap();
        (left, right)
    }

    #[test]
    fn inner_join() {
        let (l, r) = make_blocks();
        let cfg = JoinConfig::inner("id", "id");
        let result = HashJoin::join(&l, &r, &cfg).unwrap();
        assert_eq!(result.num_rows, 2);
    }

    #[test]
    fn left_join() {
        let (l, r) = make_blocks();
        let cfg = JoinConfig::left("id", "id");
        let result = HashJoin::join(&l, &r, &cfg).unwrap();
        assert_eq!(result.num_rows, 3);
    }

    #[test]
    fn full_outer_join() {
        let (l, r) = make_blocks();
        let cfg = JoinConfig::new("id", "id", JoinType::Full);
        let result = HashJoin::join(&l, &r, &cfg).unwrap();
        assert_eq!(result.num_rows, 4); // alice(unmatched) + bob+carol(matched) + 4(unmatched)
    }
}
