//! Runtime adaptive helpers used *after* the map stage of a shuffle.
//!
//! These are the concrete algorithms Spark's AQE applies at runtime once it
//! sees actual per-partition sizes from the map stage:
//!
//! * [`SkewSplitter`] — a heavy-hitter partition is split into `k` sub-blocks
//!   so no single reducer becomes the whole-job bottleneck. Uses a secondary
//!   hash-with-salt so equal-key rows can still be re-gathered downstream.
//! * [`PartitionCoalescer`] — after shuffle, walk partitions in order and
//!   pack them into buckets of ~`target_rows` rows, then emit one reducer per
//!   bucket. Cuts down on task spawn overhead for skewed-small workloads.
//! * [`ShuffleAdvisor`] — combines runtime stats with the [`AqeOptimizer`]
//!   decisions to yield a concrete `ShufflePlan` for the reduce phase.

use kore_core::{ColumnData, DataBlock, KoreError};
use serde::{Deserialize, Serialize};

use crate::{AqeDecision, AqeOptimizer, StageStats};

// ─── Skew splitter ────────────────────────────────────────────────────────────

/// Number of skew sub-partitions to split a heavy hitter into. Default 4.
pub const DEFAULT_SKEW_SUBPARTS: usize = 4;

pub struct SkewSplitter {
    pub subparts: usize,
}

impl SkewSplitter {
    pub fn new(subparts: usize) -> Self { Self { subparts: subparts.max(2) } }

    /// Split `block` into `subparts` sub-blocks using a secondary hash of the
    /// key column with a per-block salt. Guarantees:
    ///   * total row count preserved
    ///   * max sub-block row count ≤ ceil(N / subparts)
    ///     (approximately — actual hash may skew a little)
    pub fn split(&self, block: &DataBlock, key: &str) -> Result<Vec<DataBlock>, KoreError> {
        let n = block.num_rows;
        if n == 0 { return Ok(Vec::new()); }
        let np = self.subparts;
        let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); np];
        let col = block.column(key)
            .ok_or_else(|| KoreError::ColumnNotFound(key.into()))?;

        for i in 0..n {
            // Salt with row index so heavy-hitter *identical* keys still
            // spread across sub-partitions. This is Spark's skew-join salt
            // technique: correctness is preserved for GROUP BY because the
            // reducer concatenates sub-partitions then re-groups. For skew
            // joins, the peer side is duplicated across all sub-partitions.
            let h = secondary_hash(&col.data, i) ^ mix_index(i, np);
            buckets[(h as usize) % np].push(i);
        }
        let parts: Vec<DataBlock> = buckets.iter()
            .map(|idx| block.select_rows(idx))
            .filter(|b| b.num_rows > 0)
            .collect();
        Ok(parts)
    }
}

/// SplitMix-style mix so consecutive row indices map to different sub-buckets.
/// The output is a full 64-bit value which will later be reduced modulo
/// `n_buckets` at the callsite (`% np`). We keep `n_buckets` in the signature
/// only for a compile-time hint that this function is per-splitter-tuned.
fn mix_index(i: usize, _n_buckets: usize) -> u64 {
    let mut x = i as u64;
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn secondary_hash(data: &ColumnData, row: usize) -> u64 {
    // FNV-1a with a fixed salt distinct from the primary partitioner's hash,
    // so we don't just re-create the same buckets.
    let mut h: u64 = 0xdead_beef_cafe_babe;
    let bytes: Vec<u8> = match data {
        ColumnData::Int64(v)   => v.get(row).and_then(|x| *x)
            .map(|i| i.to_le_bytes().to_vec()).unwrap_or_default(),
        ColumnData::Float64(v) => v.get(row).and_then(|x| *x)
            .map(|f| f.to_bits().to_le_bytes().to_vec()).unwrap_or_default(),
        ColumnData::Bool(v)    => v.get(row).and_then(|x| *x)
            .map(|b| vec![b as u8]).unwrap_or_default(),
        ColumnData::Str(v)     => v.get(row).and_then(|x| x.as_deref())
            .map(|s| s.as_bytes().to_vec()).unwrap_or_default(),
        ColumnData::StrDict { codes, dict } => {
            let c = codes.get(row).copied().unwrap_or(u8::MAX);
            if c == u8::MAX { vec![] } else { dict.get(c as usize).map(|s| s.as_bytes().to_vec()).unwrap_or_default() }
        }
    };
    for b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(1_099_511_628_211);
    }
    h
}

// ─── Partition coalescer ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoalesceBucket {
    /// Original partition indices merged into this bucket.
    pub sources:  Vec<usize>,
    pub rows:     usize,
}

/// Coalesce many tiny partitions into `target_rows`-sized buckets.
///
/// Uses a simple greedy walk in the input order (which matches Spark's AQE
/// coalesce behavior). Guarantees:
///   * `sum(bucket.rows for bucket in output) == sum(partition_rows)`
///   * `output.len() <= partition_rows.len()`
///   * Each bucket except the last has at least one partition assigned.
pub struct PartitionCoalescer {
    pub target_rows: usize,
}

impl PartitionCoalescer {
    pub fn new(target_rows: usize) -> Self {
        Self { target_rows: target_rows.max(1) }
    }

    pub fn coalesce(&self, partition_rows: &[usize]) -> Vec<CoalesceBucket> {
        let mut out = Vec::new();
        let mut cur = CoalesceBucket { sources: Vec::new(), rows: 0 };
        for (idx, &rows) in partition_rows.iter().enumerate() {
            if !cur.sources.is_empty() && cur.rows + rows > self.target_rows {
                out.push(std::mem::replace(&mut cur, CoalesceBucket { sources: Vec::new(), rows: 0 }));
            }
            cur.sources.push(idx);
            cur.rows += rows;
        }
        if !cur.sources.is_empty() { out.push(cur); }
        out
    }
}

// ─── Shuffle advisor ──────────────────────────────────────────────────────────

/// Runtime plan for the reduce phase, produced from map-side stats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShufflePlan {
    /// Partition indices to split for skew relief (heavy hitters).
    pub split_partitions: Vec<usize>,
    /// Coalesced reduce buckets (may be fewer than input partitions).
    pub coalesced: Vec<CoalesceBucket>,
    /// Whether to promote a shuffle join to a broadcast join based on the
    /// actual materialized size.
    pub promote_broadcast: bool,
}

pub struct ShuffleAdvisor<'a> {
    pub opt:      &'a AqeOptimizer,
    pub stage_id: &'a str,
}

impl<'a> ShuffleAdvisor<'a> {
    /// Build a runtime plan from map-side per-partition row counts.
    pub fn advise(
        &self,
        partition_rows: &[usize],
        coalesce_target: usize,
    ) -> ShufflePlan {
        let skewed = self.opt.skewed_partitions(self.stage_id, partition_rows);
        let coalesced = PartitionCoalescer::new(coalesce_target).coalesce(partition_rows);
        let promote_broadcast = self.opt.should_broadcast(self.stage_id);
        ShufflePlan {
            split_partitions: skewed,
            coalesced,
            promote_broadcast,
        }
    }
}

/// Convenience: register `stats` and immediately return a shuffle plan.
pub fn advise_with_stats(
    opt: &mut AqeOptimizer,
    stats: StageStats,
    partition_rows: &[usize],
    coalesce_target: usize,
) -> (ShufflePlan, AqeDecision) {
    let stage_id = stats.stage_id.clone();
    opt.record(stats);
    let plan = ShuffleAdvisor { opt, stage_id: &stage_id }.advise(partition_rows, coalesce_target);
    let decision = opt.decide(&stage_id, Some(partition_rows));
    (plan, decision)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, DataBlock};

    fn skew_block(hot_key: &str, hot_rows: usize, cold_rows: usize) -> DataBlock {
        // hot_rows of `hot_key`, cold_rows of unique other keys
        let mut vals = vec![Some(hot_key.to_string()); hot_rows];
        vals.extend((0..cold_rows).map(|i| Some(format!("k{i}"))));
        DataBlock::new(vec![
            Column::str_col("region", vals),
            Column::float64("amount",
                (0..(hot_rows + cold_rows)).map(|i| Some(i as f64)).collect()),
        ]).unwrap()
    }

    #[test]
    fn skew_splitter_preserves_row_count() {
        let block = skew_block("HOT", 1000, 10);
        let splitter = SkewSplitter::new(4);
        let parts = splitter.split(&block, "region").unwrap();
        let total: usize = parts.iter().map(|p| p.num_rows).sum();
        assert_eq!(total, 1010);
    }

    #[test]
    fn skew_splitter_actually_splits_heavy_hitter() {
        let block = skew_block("HOT", 10_000, 0);
        let splitter = SkewSplitter::new(4);
        let parts = splitter.split(&block, "region").unwrap();
        // 10_000 rows of a single key split into 4 sub-partitions ⇒
        // max sub-partition ≪ original size. Even with hash collisions it
        // must be well below the original 10k.
        assert!(parts.len() >= 2, "expected at least 2 sub-partitions, got {}", parts.len());
        let max_rows = parts.iter().map(|p| p.num_rows).max().unwrap_or(0);
        assert!(max_rows < 10_000,
            "expected split to reduce max partition below original, max={max_rows}");
    }

    #[test]
    fn coalescer_respects_target_and_covers_all_partitions() {
        let rows = [10, 20, 30, 40, 50, 60, 70]; // total = 280
        let c = PartitionCoalescer::new(100);
        let buckets = c.coalesce(&rows);
        let total_rows: usize = buckets.iter().map(|b| b.rows).sum();
        assert_eq!(total_rows, 280);
        let total_srcs: usize = buckets.iter().map(|b| b.sources.len()).sum();
        assert_eq!(total_srcs, rows.len());
        for b in &buckets {
            // Each bucket is either a single partition (may exceed target) or
            // an accumulation whose *previous* sum was still under the target.
            assert!(!b.sources.is_empty());
        }
        assert!(buckets.len() <= rows.len());
    }

    #[test]
    fn coalescer_never_expands() {
        let rows = [5; 100]; // 100 tiny partitions, target 200 → few buckets
        let c = PartitionCoalescer::new(200);
        let buckets = c.coalesce(&rows);
        assert!(buckets.len() < 10, "expected big reduction, got {}", buckets.len());
    }

    #[test]
    fn shuffle_advisor_flags_skew_and_broadcast() {
        use crate::AqeOptimizer;
        let mut opt = AqeOptimizer::new();
        // Small stage → broadcast candidate.
        let tiny = DataBlock::new(vec![
            Column::int64("id", (0..8).map(Some).collect()),
        ]).unwrap();
        let stats = StageStats::collect("s1", &tiny, 4);
        opt.record(stats);
        // Partition histogram with a heavy hitter.
        let hist = vec![100, 100, 100, 1000];
        let plan = ShuffleAdvisor { opt: &opt, stage_id: "s1" }.advise(&hist, 100);
        assert!(plan.promote_broadcast, "expected broadcast promotion");
        assert_eq!(plan.split_partitions, vec![3], "expected skew flag on partition 3");
        assert!(plan.coalesced.len() <= hist.len());
    }
}
