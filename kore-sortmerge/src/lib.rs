//! KORE Layer 71 — Sort-merge join + broadcast join with auto strategy selection.

use std::collections::HashMap;
use kore_core::{Column, ColumnData, DataBlock, JoinKey, Value, compare_join_keys};
use rayon::prelude::*;

// ─── Join strategy ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinStrategy {
    SortMerge,
    Broadcast,
    HashJoin,
    NestedLoop,
}

/// Automatically choose a join strategy based on estimated row counts.
///
/// | Condition | Strategy |
/// |-----------|----------|
/// | Either side < 100 k rows | `Broadcast` |
/// | Both sides > 1 M rows | `SortMerge` |
/// | Otherwise | `HashJoin` |
pub fn choose_strategy(left_rows: usize, right_rows: usize) -> JoinStrategy {
    let small = left_rows.min(right_rows);
    let large  = left_rows.max(right_rows);
    if small < 100_000 {
        JoinStrategy::Broadcast
    } else if large > 1_000_000 {
        JoinStrategy::SortMerge
    } else {
        JoinStrategy::HashJoin
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn col_key(col: &Column, row: usize) -> JoinKey {
    JoinKey::from(&col.data.get_value(row))
}

fn collect_keys(block: &DataBlock, key_col: &str) -> Vec<JoinKey> {
    let col = block.column(key_col).expect("key column not found");
    (0..block.num_rows).map(|r| col_key(col, r)).collect()
}

/// Merge two DataBlocks horizontally at specific row indices.
/// Columns from `right` that share a name with `left` are renamed with a `_r` suffix.
fn merge_rows(
    left:  &DataBlock,
    right: &DataBlock,
    left_indices:  &[usize],
    right_indices: &[usize],
) -> DataBlock {
    assert_eq!(left_indices.len(), right_indices.len());
    let left_names: std::collections::HashSet<&str> = left.columns.iter()
        .map(|c| c.name.as_str())
        .collect();

    let mut columns: Vec<Column> = left.columns.iter()
        .map(|c| Column { name: c.name.clone(), data: c.data.take_rows(left_indices) })
        .collect();

    for c in &right.columns {
        let name = if left_names.contains(c.name.as_str()) {
            format!("{}_r", c.name)
        } else {
            c.name.clone()
        };
        columns.push(Column { name, data: c.data.take_rows(right_indices) });
    }

    DataBlock { num_rows: left_indices.len(), columns }
}

// ─── Sort-merge join ──────────────────────────────────────────────────────────

/// Classic sort-merge inner join.
///
/// Both sides are sorted by key (using Rayon parallel sort), then merged
/// with a two-pointer sweep.  All columns from both sides are included.
pub fn sort_merge_join(
    left:      &DataBlock,
    right:     &DataBlock,
    left_key:  &str,
    right_key: &str,
) -> DataBlock {
    // 1. Build (key, original_index) pairs, sort in parallel
    let left_col  = left.column(left_key).expect("left key column");
    let right_col = right.column(right_key).expect("right key column");

    let mut left_pairs: Vec<(JoinKey, usize)> = (0..left.num_rows)
        .map(|i| (col_key(left_col, i), i))
        .collect();
    let mut right_pairs: Vec<(JoinKey, usize)> = (0..right.num_rows)
        .map(|i| (col_key(right_col, i), i))
        .collect();

    left_pairs.par_sort_unstable_by(|(a, _), (b, _)| compare_join_keys(a, b));
    right_pairs.par_sort_unstable_by(|(a, _), (b, _)| compare_join_keys(a, b));

    // 2. Two-pointer merge
    let mut left_idx  = Vec::new();
    let mut right_idx = Vec::new();

    let mut li = 0usize;
    let mut ri = 0usize;

    while li < left_pairs.len() && ri < right_pairs.len() {
        match compare_join_keys(&left_pairs[li].0, &right_pairs[ri].0) {
            std::cmp::Ordering::Less    => { li += 1; }
            std::cmp::Ordering::Greater => { ri += 1; }
            std::cmp::Ordering::Equal   => {
                // Collect all matching rows on both sides (handle duplicates)
                let key = &left_pairs[li].0;
                let l_start = li;
                while li < left_pairs.len() && &left_pairs[li].0 == key { li += 1; }
                let r_start = ri;
                while ri < right_pairs.len() && &right_pairs[ri].0 == key { ri += 1; }

                for &(_, lo) in &left_pairs[l_start..li] {
                    for &(_, ro) in &right_pairs[r_start..ri] {
                        left_idx.push(lo);
                        right_idx.push(ro);
                    }
                }
            }
        }
    }

    merge_rows(left, right, &left_idx, &right_idx)
}

// ─── Broadcast join ───────────────────────────────────────────────────────────

/// Broadcast (hash-lookup) inner join.
///
/// The `small` side is hashed once into a `HashMap`.  The `large` side is
/// then split into Rayon chunks and each chunk performs lookups concurrently.
pub fn broadcast_join(
    small:     &DataBlock,
    large:     &DataBlock,
    small_key: &str,
    large_key: &str,
) -> DataBlock {
    // Build hash map: key → list of small-side row indices
    let small_col = small.column(small_key).expect("small key column");
    let mut hmap: HashMap<JoinKey, Vec<usize>> = HashMap::with_capacity(small.num_rows);
    for r in 0..small.num_rows {
        hmap.entry(col_key(small_col, r)).or_default().push(r);
    }

    let large_col = large.column(large_key).expect("large key column");

    // Process large side in parallel chunks
    let chunk_size = (large.num_rows / rayon::current_num_threads()).max(1024);
    let chunks: Vec<(usize, usize)> = (0..large.num_rows)
        .step_by(chunk_size)
        .map(|start| (start, (start + chunk_size).min(large.num_rows)))
        .collect();

    let partial: Vec<(Vec<usize>, Vec<usize>)> = chunks.par_iter()
        .map(|&(start, end)| {
            let mut l_idx = Vec::new();
            let mut r_idx = Vec::new();
            for r in start..end {
                let k = col_key(large_col, r);
                if let Some(matches) = hmap.get(&k) {
                    for &s in matches {
                        l_idx.push(s);
                        r_idx.push(r);
                    }
                }
            }
            (l_idx, r_idx)
        })
        .collect();

    let mut small_idx = Vec::new();
    let mut large_idx = Vec::new();
    for (l, r) in partial {
        small_idx.extend(l);
        large_idx.extend(r);
    }

    // For broadcast join the "small" side goes left in the output
    merge_rows(small, large, &small_idx, &large_idx)
}

// ─── Hash join (generic) ──────────────────────────────────────────────────────

/// Simple single-threaded hash join (used by `HashJoin` strategy).
pub fn hash_join(
    left:      &DataBlock,
    right:     &DataBlock,
    left_key:  &str,
    right_key: &str,
) -> DataBlock {
    broadcast_join(left, right, left_key, right_key)
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// Auto-dispatch join based on `choose_strategy`.
pub fn auto_join(
    left:      &DataBlock,
    right:     &DataBlock,
    left_key:  &str,
    right_key: &str,
) -> DataBlock {
    match choose_strategy(left.num_rows, right.num_rows) {
        JoinStrategy::SortMerge  => sort_merge_join(left, right, left_key, right_key),
        JoinStrategy::Broadcast  => {
            if left.num_rows <= right.num_rows {
                broadcast_join(left, right, left_key, right_key)
            } else {
                broadcast_join(right, left, right_key, left_key)
            }
        }
        JoinStrategy::HashJoin   => hash_join(left, right, left_key, right_key),
        JoinStrategy::NestedLoop => nested_loop_join(left, right, left_key, right_key),
    }
}

/// Nested-loop join (reference implementation, never use on large data).
pub fn nested_loop_join(
    left:      &DataBlock,
    right:     &DataBlock,
    left_key:  &str,
    right_key: &str,
) -> DataBlock {
    let lc = left.column(left_key).expect("left key");
    let rc = right.column(right_key).expect("right key");
    let mut li = Vec::new();
    let mut ri = Vec::new();
    for l in 0..left.num_rows {
        let lk = col_key(lc, l);
        for r in 0..right.num_rows {
            if lk == col_key(rc, r) {
                li.push(l);
                ri.push(r);
            }
        }
    }
    merge_rows(left, right, &li, &ri)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_block(n: usize, offset: usize) -> DataBlock {
        // id column: 0..n (with 50% overlap when offset = n/2)
        let ids: Vec<Option<i64>> = (offset..offset + n)
            .map(|i| Some(i as i64))
            .collect();
        let vals: Vec<Option<f64>> = (0..n).map(|i| Some(i as f64 * 1.5)).collect();
        DataBlock::new(vec![
            Column::int64("id",    ids),
            Column::float64("val", vals),
        ]).unwrap()
    }

    #[test]
    fn strategy_selection() {
        assert_eq!(choose_strategy(50_000, 5_000_000), JoinStrategy::Broadcast);
        assert_eq!(choose_strategy(2_000_000, 3_000_000), JoinStrategy::SortMerge);
        assert_eq!(choose_strategy(200_000, 500_000), JoinStrategy::HashJoin);
    }

    #[test]
    fn sort_merge_basic() {
        // left: ids 0..10, right: ids 5..15 — overlap at 5..10
        let left  = make_block(10, 0);
        let right = make_block(10, 5);
        let result = sort_merge_join(&left, &right, "id", "id");
        assert_eq!(result.num_rows, 5);
    }

    #[test]
    fn broadcast_basic() {
        let small = make_block(5, 0);
        let large = make_block(20, 0);
        let result = broadcast_join(&small, &large, "id", "id");
        // small has 5 rows, large has 20 rows, ids 0..5 match
        assert_eq!(result.num_rows, 5);
    }

    #[test]
    fn sort_merge_large() {
        let n = 100_000;
        let left  = make_block(n, 0);
        let right = make_block(n, n / 2); // 50% overlap
        let result = sort_merge_join(&left, &right, "id", "id");
        assert_eq!(result.num_rows, n / 2);
    }

    #[test]
    fn broadcast_large() {
        let small = make_block(1_000, 0);
        let large = make_block(100_000, 0);
        let result = broadcast_join(&small, &large, "id", "id");
        assert_eq!(result.num_rows, 1_000);
    }
}
