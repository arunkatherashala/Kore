//! Hash Join — build a hash table on the smaller (build) side, probe with the larger side.
//!
//! Supports INNER, LEFT, RIGHT and FULL OUTER joins.
//! Probe phase is parallelized via Rayon (O(n/T) per thread).

use std::collections::HashMap;
use std::sync::Arc;
use rayon::prelude::*;
use kore_core::{Column, DataBlock, JoinKey, JoinType, KoreError};

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
            let key = join_key(right, i, &cfg.right_key)?;
            // a NULL key equals nothing, not even another NULL
            if key == JoinKey::Null { continue; }
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
                    if let Ok(key) = join_key(left, l, &cfg.left_key) {
                        let hit = if key == JoinKey::Null { None } else { table.get(&key) };
                        if let Some(right_rows) = hit {
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

    /// Int64-key hash join: flat (array based) hash table on the build side, chunked parallel probe, and
    /// a parallel column gather. For inner joins the smaller input is the build side, so a handful of
    /// filtered dimension rows never forces a hash table over a multi-million-row fact table.
    fn join_int64(
        left: &DataBlock,
        right: &DataBlock,
        lv: &[Option<i64>],
        rv: &[Option<i64>],
        cfg: &JoinConfig,
    ) -> Result<DataBlock, KoreError> {
        const NONE: u32 = u32::MAX;
        let inner = matches!(cfg.join_type, JoinType::Inner);
        if lv.len() >= NONE as usize || rv.len() >= NONE as usize {
            return Self::join_int64_wide(left, right, lv, rv, cfg);
        }
        // build on the right unless this is an inner join and the left side is smaller
        let swap = inner && lv.len() < rv.len();
        let (bk, pk) = if swap { (lv, rv) } else { (rv, lv) };
        let table = FlatTable::build(bk);

        let n_probe = pk.len();
        const PCHUNK: usize = 32_768;
        let keep_unmatched = matches!(cfg.join_type, JoinType::Left | JoinType::Full);
        let parts: Vec<(Vec<u32>, Vec<u32>)> = (0..n_probe.div_ceil(PCHUNK)).into_par_iter().map(|c| {
            let lo = c * PCHUNK;
            let hi = (lo + PCHUNK).min(n_probe);
            let mut pi: Vec<u32> = Vec::with_capacity(hi - lo);
            let mut bi: Vec<u32> = Vec::with_capacity(hi - lo);
            for p in lo..hi {
                if let Some(k) = pk[p] {
                    let mut e = table.first(k);
                    if e == NONE {
                        if keep_unmatched { pi.push(p as u32); bi.push(NONE); }
                    } else {
                        while e != NONE {
                            pi.push(p as u32);
                            bi.push(e);
                            e = table.next[e as usize];
                        }
                    }
                } else if keep_unmatched {
                    // a NULL key matches nothing, but an outer join still keeps the row
                    pi.push(p as u32); bi.push(NONE);
                }
            }
            (pi, bi)
        }).collect();
        let total: usize = parts.iter().map(|p| p.0.len()).sum();
        let mut pidx: Vec<u32> = Vec::with_capacity(total);
        let mut bidx: Vec<u32> = Vec::with_capacity(total);
        for (a, b) in &parts { pidx.extend_from_slice(a); bidx.extend_from_slice(b); }
        drop(parts);
        // probe side = left unless we swapped
        let (mut lidx, mut ridx) = if swap { (bidx, pidx) } else { (pidx, bidx) };

        if matches!(cfg.join_type, JoinType::Right | JoinType::Full) {
            let mut matched = vec![false; rv.len()];
            for &r in &ridx { if r != NONE { matched[r as usize] = true; } }
            for (r, m) in matched.iter().enumerate() {
                if !m { lidx.push(NONE); ridx.push(r as u32); }
            }
        }
        build_result_idx(left, right, &lidx, &ridx)
    }

    /// Fallback for inputs too large for 32-bit row ids (never hit in practice).
    fn join_int64_wide(
        left: &DataBlock, right: &DataBlock, lv: &[Option<i64>], rv: &[Option<i64>], cfg: &JoinConfig,
    ) -> Result<DataBlock, KoreError> {
        let mut table: HashMap<i64, Vec<usize>> = HashMap::with_capacity(rv.len() / 4 + 16);
        for (i, k) in rv.iter().enumerate() { if let Some(k) = k { table.entry(*k).or_default().push(i); } }
        let mut pairs: Vec<(Option<usize>, Option<usize>)> = Vec::new();
        for (l, k) in lv.iter().enumerate() {
            if let Some(k) = k {
                if let Some(rows) = table.get(k) { for &r in rows { pairs.push((Some(l), Some(r))); } }
                else if matches!(cfg.join_type, JoinType::Left | JoinType::Full) { pairs.push((Some(l), None)); }
            }
        }
        if matches!(cfg.join_type, JoinType::Right | JoinType::Full) {
            let mut matched = vec![false; rv.len()];
            for &(_, r) in &pairs { if let Some(ri) = r { matched[ri] = true; } }
            for (r, m) in matched.iter().enumerate() { if !m { pairs.push((None, Some(r))); } }
        }
        build_result(left, right, &pairs)
    }
}

/// Chained hash table over i64 keys: `next[row]` links rows with the same key (ascending row order).
/// Dense key ranges (surrogate keys) use a direct-address table, everything else open addressing.
struct FlatTable {
    min: i64,
    direct: bool,
    heads: Vec<u32>,      // direct: indexed by key-min; hashed: head row per slot
    slot_keys: Vec<i64>,  // hashed only
    mask: usize,
    next: Vec<u32>,
}

impl FlatTable {
    const NONE: u32 = u32::MAX;

    #[inline(always)]
    fn hash(k: i64) -> usize {
        let h = (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        (h ^ (h >> 32)) as usize
    }

    fn build(keys: &[Option<i64>]) -> Self {
        let n = keys.len();
        let (mut lo, mut hi, mut any) = (i64::MAX, i64::MIN, false);
        for k in keys.iter().flatten() { any = true; if *k < lo { lo = *k; } if *k > hi { hi = *k; } }
        let mut next = vec![Self::NONE; n];
        if !any {
            return FlatTable { min: 0, direct: true, heads: Vec::new(), slot_keys: Vec::new(), mask: 0, next };
        }
        let range = (hi as i128 - lo as i128) as u128 + 1;
        if range <= (4 * n as u128 + 1024).max(1 << 16).min(1 << 27) {
            let mut heads = vec![Self::NONE; range as usize];
            for i in (0..n).rev() {
                if let Some(k) = keys[i] {
                    let s = (k as i128 - lo as i128) as usize;
                    next[i] = heads[s];
                    heads[s] = i as u32;
                }
            }
            FlatTable { min: lo, direct: true, heads, slot_keys: Vec::new(), mask: 0, next }
        } else {
            let cap = (n * 2).next_power_of_two().max(16);
            let mask = cap - 1;
            let mut heads = vec![Self::NONE; cap];
            let mut slot_keys = vec![0i64; cap];
            for i in (0..n).rev() {
                if let Some(k) = keys[i] {
                    let mut s = Self::hash(k) & mask;
                    loop {
                        if heads[s] == Self::NONE { slot_keys[s] = k; break; }
                        if slot_keys[s] == k { break; }
                        s = (s + 1) & mask;
                    }
                    next[i] = heads[s];
                    heads[s] = i as u32;
                }
            }
            FlatTable { min: 0, direct: false, heads, slot_keys, mask, next }
        }
    }

    /// First build row with key `k`, or NONE.
    #[inline(always)]
    fn first(&self, k: i64) -> u32 {
        if self.direct {
            let d = k as i128 - self.min as i128;
            if d < 0 || d >= self.heads.len() as i128 { return Self::NONE; }
            self.heads[d as usize]
        } else {
            let mut s = Self::hash(k) & self.mask;
            loop {
                let h = self.heads[s];
                if h == Self::NONE { return Self::NONE; }
                if self.slot_keys[s] == k { return h; }
                s = (s + 1) & self.mask;
            }
        }
    }
}

/// Gather both sides' columns for row-id pairs (u32::MAX = no row, i.e. NULL), one column per task.
pub(crate) fn build_result_idx(left: &DataBlock, right: &DataBlock, lidx: &[u32], ridx: &[u32]) -> Result<DataBlock, KoreError> {
    let left_names: std::collections::HashSet<&str> = left.columns.iter().map(|c| c.name.as_str()).collect();
    let mut jobs: Vec<(String, &kore_core::ColumnData, &[u32])> = Vec::with_capacity(left.columns.len() + right.columns.len());
    for c in &left.columns { jobs.push((c.name.clone(), &c.data, lidx)); }
    for c in &right.columns {
        let name = if left_names.contains(c.name.as_str()) { format!("{}_r", c.name) } else { c.name.clone() };
        jobs.push((name, &c.data, ridx));
    }
    let columns: Vec<Column> = jobs.into_par_iter().map(|(name, data, idx)| Column { name, data: gather(data, idx) }).collect();
    Ok(DataBlock { columns, num_rows: lidx.len() })
}

fn gather(src: &kore_core::ColumnData, idx: &[u32]) -> kore_core::ColumnData {
    use kore_core::ColumnData;
    const NONE: u32 = u32::MAX;
    match src {
        ColumnData::Int64(v) => ColumnData::Int64(idx.iter().map(|&i| if i == NONE { None } else { v[i as usize] }).collect()),
        ColumnData::Float64(v) => ColumnData::Float64(idx.iter().map(|&i| if i == NONE { None } else { v[i as usize] }).collect()),
        ColumnData::Bool(v) => ColumnData::Bool(idx.iter().map(|&i| if i == NONE { None } else { v[i as usize] }).collect()),
        ColumnData::Str(v) => ColumnData::Str(idx.iter().map(|&i| if i == NONE { None } else { v[i as usize].clone() }).collect()),
        ColumnData::StrDict { codes, dict } => ColumnData::StrDict {
            codes: idx.iter().map(|&i| if i == NONE { u8::MAX } else { codes[i as usize] }).collect(),
            dict: dict.clone(),
        },
    }
}

/// Join key of a cell. Floats are comparable (an integral float equals the same integer, so a DOUBLE key can
/// join a BIGINT key); NULL stays `JoinKey::Null`, which never matches.
fn join_key(block: &DataBlock, row: usize, col: &str) -> Result<JoinKey, KoreError> {
    let c = block.column(col).ok_or_else(|| KoreError::ColumnNotFound(col.into()))?;
    Ok(match c.data.get_value(row) {
        kore_core::Value::Float(f) => {
            if f.is_nan() { JoinKey::Null }
            else if f.fract() == 0.0 && f.abs() < 9.0e15 { JoinKey::Int(f as i64) }
            else { JoinKey::Str(format!("float#{:?}", f)) }
        }
        v => JoinKey::from(&v),
    })
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

    #[test]
    fn null_keys_never_match_but_outer_joins_keep_the_row() {
        // integer keys (fast path)
        let l = DataBlock::new(vec![Column::int64("k", vec![Some(1), None, Some(3)])]).unwrap();
        let r = DataBlock::new(vec![Column::int64("k", vec![None, Some(3)]), Column::int64("v", vec![Some(10), Some(30)])]).unwrap();
        assert_eq!(HashJoin::join(&l, &r, &JoinConfig::inner("k", "k")).unwrap().num_rows, 1);
        // LEFT keeps the NULL-key row and the unmatched row: 1 -> none, NULL -> none, 3 -> 30
        assert_eq!(HashJoin::join(&l, &r, &JoinConfig::left("k", "k")).unwrap().num_rows, 3);
        // string keys (generic path): NULL = NULL is not a match
        let l = DataBlock::new(vec![Column::str_col("k", vec![Some("a".into()), None])]).unwrap();
        let r = DataBlock::new(vec![Column::str_col("k", vec![None, Some("a".into())])]).unwrap();
        assert_eq!(HashJoin::join(&l, &r, &JoinConfig::inner("k", "k")).unwrap().num_rows, 1);
        assert_eq!(HashJoin::join(&l, &r, &JoinConfig::new("k", "k", JoinType::Full)).unwrap().num_rows, 3);
    }

    #[test]
    fn float_keys_join_on_value_and_against_integers() {
        let l = DataBlock::new(vec![Column::float64("k", vec![Some(1.0), Some(2.5), Some(3.0)])]).unwrap();
        let r = DataBlock::new(vec![Column::float64("k", vec![Some(2.5), Some(9.0)])]).unwrap();
        // previously every float key hashed to NULL, which made this a cross join
        assert_eq!(HashJoin::join(&l, &r, &JoinConfig::inner("k", "k")).unwrap().num_rows, 1);
        let ri = DataBlock::new(vec![Column::int64("k", vec![Some(1), Some(3)])]).unwrap();
        assert_eq!(HashJoin::join(&l, &ri, &JoinConfig::inner("k", "k")).unwrap().num_rows, 2);
    }
}
