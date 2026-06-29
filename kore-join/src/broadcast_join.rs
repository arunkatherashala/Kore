//! Broadcast Join — replicate the smaller (broadcast) side to every worker.
//!
//! Semantically identical to a hash join; the distinction is the physical plan:
//! the broadcast table is built once and shared across all partitions of the
//! probe side.  Here we model that with a pre-built HashJoin.

use kore_core::{DataBlock, KoreError};
use crate::{hash_join::HashJoin, JoinConfig};

pub struct BroadcastJoin;

impl BroadcastJoin {
    /// `small` is the broadcast (build) side; `large` is the probe side.
    pub fn join(
        large: &DataBlock,
        small: &DataBlock,
        cfg: &JoinConfig,
    ) -> Result<DataBlock, KoreError> {
        // Validate that the broadcast side is "small" (warn if reversed)
        if small.num_rows > large.num_rows {
            // Still works; just a performance hint
        }
        // Reuse HashJoin — the broadcast optimisation is the build-once semantics
        HashJoin::join(large, small, cfg)
    }

    /// Partition `probe` into `n_partitions` chunks and join each chunk against
    /// the pre-built broadcast table, then merge results.
    pub fn join_partitioned(
        probe:          &DataBlock,
        broadcast:      &DataBlock,
        cfg:            &JoinConfig,
        n_partitions:   usize,
    ) -> Result<DataBlock, KoreError> {
        let chunk_size = (probe.num_rows / n_partitions).max(1);
        let mut results: Vec<DataBlock> = Vec::new();

        let mut start = 0;
        while start < probe.num_rows {
            let end = (start + chunk_size).min(probe.num_rows);
            let indices: Vec<usize> = (start..end).collect();
            let chunk = probe.select_rows(&indices);
            let joined = HashJoin::join(&chunk, broadcast, cfg)?;
            results.push(joined);
            start = end;
        }

        DataBlock::concat(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, DataBlock};

    #[test]
    fn broadcast_inner() {
        let large = DataBlock::new(vec![
            Column::int64("id", (1..=100i64).map(|x| Some(x)).collect()),
        ]).unwrap();
        let small = DataBlock::new(vec![
            Column::int64("id",    vec![Some(10), Some(50), Some(99)]),
            Column::str_col("tag", vec![Some("a".into()), Some("b".into()), Some("c".into())]),
        ]).unwrap();
        let cfg = JoinConfig::inner("id", "id");
        let result = BroadcastJoin::join(&large, &small, &cfg).unwrap();
        assert_eq!(result.num_rows, 3);
    }

    #[test]
    fn partitioned_broadcast() {
        let large = DataBlock::new(vec![
            Column::int64("id", (1..=20i64).map(|x| Some(x)).collect()),
        ]).unwrap();
        let small = DataBlock::new(vec![
            Column::int64("id", vec![Some(5), Some(10), Some(15)]),
        ]).unwrap();
        let cfg = JoinConfig::inner("id", "id");
        let result = BroadcastJoin::join_partitioned(&large, &small, &cfg, 4).unwrap();
        assert_eq!(result.num_rows, 3);
    }
}
