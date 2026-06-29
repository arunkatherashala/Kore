//! Sort-Merge Join — sort both sides on the join key, then merge in a single pass.
//!
//! Complexity: O(n log n + m log m)  — scales to tables that don't fit in memory.
//! Handles many-to-many by buffering equal-key groups.

use std::cmp::Ordering;
use kore_core::{compare_join_keys, DataBlock, JoinType, KoreError};
use crate::{hash_join::build_result, JoinConfig};

pub struct SortMergeJoin;

impl SortMergeJoin {
    pub fn join(
        left:  &DataBlock,
        right: &DataBlock,
        cfg:   &JoinConfig,
    ) -> Result<DataBlock, KoreError> {
        let left_s  = left.sort_by(&cfg.left_key,  true)?;
        let right_s = right.sort_by(&cfg.right_key, true)?;

        let mut pairs: Vec<(Option<usize>, Option<usize>)> = Vec::new();
        let mut right_matched = vec![false; right_s.num_rows];

        let mut l = 0usize;
        let mut r = 0usize;

        while l < left_s.num_rows && r < right_s.num_rows {
            let lk = left_s.join_key(l, &cfg.left_key)?;
            let rk = right_s.join_key(r, &cfg.right_key)?;

            match compare_join_keys(&lk, &rk) {
                Ordering::Equal => {
                    // Buffer all left rows with this key
                    let l_start = l;
                    let lk_ref  = lk.clone();
                    while l < left_s.num_rows && left_s.join_key(l, &cfg.left_key)? == lk_ref {
                        l += 1;
                    }
                    // Buffer all right rows with this key
                    let r_start = r;
                    let rk_ref  = rk.clone();
                    while r < right_s.num_rows && right_s.join_key(r, &cfg.right_key)? == rk_ref {
                        right_matched[r] = true;
                        r += 1;
                    }
                    // Cross-product of matching groups
                    for li in l_start..l {
                        for ri in r_start..r {
                            pairs.push((Some(li), Some(ri)));
                        }
                    }
                }
                Ordering::Less => {
                    if matches!(cfg.join_type, JoinType::Left | JoinType::Full) {
                        pairs.push((Some(l), None));
                    }
                    l += 1;
                }
                Ordering::Greater => {
                    if matches!(cfg.join_type, JoinType::Right | JoinType::Full) {
                        pairs.push((None, Some(r)));
                    }
                    right_matched[r] = true;
                    r += 1;
                }
            }
        }

        // Remaining left rows (LEFT / FULL)
        while l < left_s.num_rows {
            if matches!(cfg.join_type, JoinType::Left | JoinType::Full) {
                pairs.push((Some(l), None));
            }
            l += 1;
        }
        // Remaining right rows (RIGHT / FULL)
        while r < right_s.num_rows {
            if matches!(cfg.join_type, JoinType::Right | JoinType::Full) {
                pairs.push((None, Some(r)));
            }
            r += 1;
        }

        build_result(&left_s, &right_s, &pairs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, DataBlock, JoinType};

    #[test]
    fn smj_inner() {
        let left = DataBlock::new(vec![
            Column::int64("id", vec![Some(3), Some(1), Some(2)]),
        ]).unwrap();
        let right = DataBlock::new(vec![
            Column::int64("id",  vec![Some(2), Some(4), Some(1)]),
            Column::float64("v", vec![Some(2.0), Some(4.0), Some(1.0)]),
        ]).unwrap();
        let cfg = JoinConfig::inner("id", "id");
        let result = SortMergeJoin::join(&left, &right, &cfg).unwrap();
        assert_eq!(result.num_rows, 2);
    }

    #[test]
    fn smj_many_to_many() {
        let left  = DataBlock::new(vec![Column::int64("k", vec![Some(1), Some(1), Some(2)])]).unwrap();
        let right = DataBlock::new(vec![Column::int64("k", vec![Some(1), Some(1), Some(3)])]).unwrap();
        let cfg   = JoinConfig::inner("k", "k");
        let result = SortMergeJoin::join(&left, &right, &cfg).unwrap();
        assert_eq!(result.num_rows, 4); // 2×2 cross product for key=1
    }

    #[test]
    fn smj_full_outer() {
        let left  = DataBlock::new(vec![Column::int64("k", vec![Some(1), Some(2)])]).unwrap();
        let right = DataBlock::new(vec![Column::int64("k", vec![Some(2), Some(3)])]).unwrap();
        let cfg   = JoinConfig::new("k", "k", JoinType::Full);
        let result = SortMergeJoin::join(&left, &right, &cfg).unwrap();
        assert_eq!(result.num_rows, 3); // (1,null), (2,2), (null,3)
    }
}
