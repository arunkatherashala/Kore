//! Hash Join — build a hash table on the smaller (build) side, probe with the larger side.
//!
//! Supports INNER, LEFT, RIGHT and FULL OUTER joins.

use std::collections::HashMap;
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
        // ── Build phase: key → list of right-side row indices ──────────────────
        let mut table: HashMap<JoinKey, Vec<usize>> = HashMap::new();
        for i in 0..right.num_rows {
            let key = right.join_key(i, &cfg.right_key)?;
            table.entry(key).or_default().push(i);
        }

        // ── Probe phase ────────────────────────────────────────────────────────
        // (left_row_idx | None, right_row_idx | None)
        let mut pairs: Vec<(Option<usize>, Option<usize>)> = Vec::new();
        let mut right_matched: Vec<bool> = vec![false; right.num_rows];

        for l in 0..left.num_rows {
            let key = left.join_key(l, &cfg.left_key)?;
            if let Some(right_rows) = table.get(&key) {
                for &r in right_rows {
                    pairs.push((Some(l), Some(r)));
                    right_matched[r] = true;
                }
            } else if matches!(cfg.join_type, JoinType::Left | JoinType::Full) {
                pairs.push((Some(l), None));
            }
        }

        // ── Unmatched right rows for RIGHT / FULL OUTER ────────────────────────
        if matches!(cfg.join_type, JoinType::Right | JoinType::Full) {
            for (r, matched) in right_matched.iter().enumerate() {
                if !matched {
                    pairs.push((None, Some(r)));
                }
            }
        }

        build_result(left, right, &pairs)
    }
}

/// Materialise a DataBlock from (left_idx | None, right_idx | None) pairs.
pub(crate) fn build_result(
    left:  &DataBlock,
    right: &DataBlock,
    pairs: &[(Option<usize>, Option<usize>)],
) -> Result<DataBlock, KoreError> {
    let n = pairs.len();

    // Deduplicate right column names that clash with left column names
    let left_names: std::collections::HashSet<&str> =
        left.columns.iter().map(|c| c.name.as_str()).collect();

    let mut columns: Vec<Column> = Vec::with_capacity(left.columns.len() + right.columns.len());

    // Left columns
    for col in &left.columns {
        let mut data = col.data.empty_like();
        for &(l_idx, _) in pairs {
            let val = l_idx.map(|i| col.data.get_value(i)).unwrap_or(Value::Null);
            data.append_value(&val)?;
        }
        columns.push(Column { name: col.name.clone(), data });
    }

    // Right columns (suffix _r on name clash)
    for col in &right.columns {
        let name = if left_names.contains(col.name.as_str()) {
            format!("{}_r", col.name)
        } else {
            col.name.clone()
        };
        let mut data = col.data.empty_like();
        for &(_, r_idx) in pairs {
            let val = r_idx.map(|i| col.data.get_value(i)).unwrap_or(Value::Null);
            data.append_value(&val)?;
        }
        columns.push(Column { name, data });
    }

    Ok(DataBlock { columns, num_rows: n })
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
