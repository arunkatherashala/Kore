//! KORE Layer 41 — Adaptive Query Execution (AQE)
//!
//! AQE mirrors Apache Spark's runtime re-optimization:
//! - After each stage completes, collect statistics (row counts, data sizes,
//!   distinct value counts, column min/max).
//! - Use those statistics to re-optimize the *remaining* plan:
//!   1. **Broadcast-join promotion** — if a stage is small enough to broadcast,
//!      switch from sort-merge join to broadcast hash join.
//!   2. **Skew detection** — identify data-skewed partitions and split them.
//!   3. **Partition coalescing** — merge tiny shuffle partitions into fewer ones.
//!   4. **Dynamic predicate pushdown** — push runtime-known filters into later
//!      stages.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use kore_core::{ColumnData, DataBlock};

// ─── Runtime statistics ───────────────────────────────────────────────────────

/// Per-stage statistics collected after execution.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StageStats {
    pub stage_id:    String,
    pub num_rows:    usize,
    pub size_bytes:  usize,
    pub partitions:  usize,
    pub columns:     HashMap<String, ColStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ColStats {
    pub null_count: usize,
    pub ndv:        usize,   // number of distinct values (estimated)
    pub min_f64:    Option<f64>,
    pub max_f64:    Option<f64>,
}

impl StageStats {
    /// Estimate the bytes for a Float64 / Int64 block (rough: 8 bytes / cell).
    fn estimate_bytes(block: &DataBlock) -> usize {
        block.columns.iter().map(|c| c.data.len() * 8).sum()
    }

    /// Collect statistics from a materialised DataBlock.
    pub fn collect(stage_id: &str, block: &DataBlock, partitions: usize) -> Self {
        let size_bytes = Self::estimate_bytes(block);
        let mut columns = HashMap::new();

        for col in &block.columns {
            let mut cs = ColStats::default();
            let mut seen = std::collections::HashSet::new();

            match &col.data {
                ColumnData::Int64(v) => {
                    let mut min = i64::MAX;
                    let mut max = i64::MIN;
                    for x in v {
                        match x {
                            None    => cs.null_count += 1,
                            Some(i) => {
                                if *i < min { min = *i; }
                                if *i > max { max = *i; }
                                seen.insert(i.to_string());
                            }
                        }
                    }
                    if min <= max {
                        cs.min_f64 = Some(min as f64);
                        cs.max_f64 = Some(max as f64);
                    }
                }
                ColumnData::Float64(v) => {
                    let mut min = f64::INFINITY;
                    let mut max = f64::NEG_INFINITY;
                    for x in v {
                        match x {
                            None    => cs.null_count += 1,
                            Some(f) => {
                                if *f < min { min = *f; }
                                if *f > max { max = *f; }
                                seen.insert(format!("{:.6}", f));
                            }
                        }
                    }
                    if min.is_finite() { cs.min_f64 = Some(min); cs.max_f64 = Some(max); }
                }
                ColumnData::Str(v) => {
                    for x in v {
                        match x {
                            None    => cs.null_count += 1,
                            Some(s) => { seen.insert(s.clone()); }
                        }
                    }
                }
                ColumnData::Bool(v) => {
                    for x in v { if x.is_none() { cs.null_count += 1; } }
                    seen.insert("true".into()); seen.insert("false".into());
                }
                ColumnData::StrDict { codes, dict } => {
                    for &c in codes {
                        if c == u8::MAX { cs.null_count += 1; } else if let Some(s) = dict.get(c as usize) { seen.insert(s.clone()); }
                    }
                }
            }

            cs.ndv = seen.len();
            columns.insert(col.name.clone(), cs);
        }

        StageStats { stage_id: stage_id.to_string(), num_rows: block.num_rows, size_bytes, partitions, columns }
    }
}

// ─── AQE Optimizer ───────────────────────────────────────────────────────────

/// Broadcast threshold: if a stage produces fewer bytes, broadcast it.
pub const BROADCAST_THRESHOLD_BYTES: usize = 10 * 1024 * 1024; // 10 MB

/// Skew threshold: a partition is skewed if it has > `SKEW_FACTOR` × median rows.
pub const SKEW_FACTOR: f64 = 3.0;

/// Minimum partition size (rows) before coalescing is triggered.
pub const COALESCE_THRESHOLD_ROWS: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub enum AqeDecision {
    /// Use broadcast hash join for this stage (stage is small enough).
    BroadcastJoin { stage_id: String },
    /// Split these skewed partitions.
    SkewSplit      { stage_id: String, partition_indices: Vec<usize> },
    /// Merge these tiny partitions together.
    Coalesce       { stage_id: String, target_partitions: usize },
    /// Push this predicate into the stage's scan.
    PredicatePush  { stage_id: String, filter: String },
    /// No change needed.
    NoOp,
}

pub struct AqeOptimizer {
    stats: HashMap<String, StageStats>,
}

impl AqeOptimizer {
    pub fn new() -> Self { Self { stats: HashMap::new() } }

    /// Register statistics for a completed stage.
    pub fn record(&mut self, stats: StageStats) {
        self.stats.insert(stats.stage_id.clone(), stats);
    }

    /// Should this stage's output be broadcast to all workers?
    pub fn should_broadcast(&self, stage_id: &str) -> bool {
        self.stats.get(stage_id)
            .map(|s| s.size_bytes < BROADCAST_THRESHOLD_BYTES)
            .unwrap_or(false)
    }

    /// Estimate the join output size (selectivity model).
    ///
    /// Uses the formula: `|L| * |R| / max(NDV_L(key), NDV_R(key))`
    pub fn estimate_join_rows(&self, left_id: &str, right_id: &str, key: &str) -> Option<usize> {
        let ls = self.stats.get(left_id)?;
        let rs = self.stats.get(right_id)?;
        let lk = ls.columns.get(key)?;
        let rk = rs.columns.get(key)?;
        let ndv = lk.ndv.max(rk.ndv).max(1);
        Some((ls.num_rows * rs.num_rows) / ndv)
    }

    /// Detect skewed partitions (returns indices of skewed ones).
    pub fn skewed_partitions(&self, stage_id: &str, partition_rows: &[usize]) -> Vec<usize> {
        if partition_rows.is_empty() { return vec![]; }
        let mut sorted = partition_rows.to_vec();
        sorted.sort_unstable();
        let median = sorted[sorted.len() / 2] as f64;
        partition_rows.iter().enumerate()
            .filter(|(_, &r)| r as f64 > median * SKEW_FACTOR)
            .map(|(i, _)| i)
            .collect()
    }

    /// Recommended number of output partitions for a shuffle stage.
    pub fn recommend_partitions(&self, stage_id: &str) -> usize {
        let Some(s) = self.stats.get(stage_id) else { return 200 }; // Spark default
        // Aim for ~64 MB per partition
        let target_bytes = 64 * 1024 * 1024_usize;
        let n = (s.size_bytes + target_bytes - 1) / target_bytes;
        n.max(1).min(2000)
    }

    /// Produce an optimisation decision for the given stage.
    pub fn decide(&self, stage_id: &str, partition_rows: Option<&[usize]>) -> AqeDecision {
        if self.should_broadcast(stage_id) {
            return AqeDecision::BroadcastJoin { stage_id: stage_id.to_string() };
        }
        if let Some(rows) = partition_rows {
            let skewed = self.skewed_partitions(stage_id, rows);
            if !skewed.is_empty() {
                return AqeDecision::SkewSplit { stage_id: stage_id.to_string(), partition_indices: skewed };
            }
            let tiny = rows.iter().filter(|&&r| r < COALESCE_THRESHOLD_ROWS).count();
            if tiny > rows.len() / 2 {
                let target = self.recommend_partitions(stage_id);
                return AqeDecision::Coalesce { stage_id: stage_id.to_string(), target_partitions: target };
            }
        }
        AqeDecision::NoOp
    }

    /// List all recorded stage IDs sorted by size (largest first).
    pub fn stages_by_size(&self) -> Vec<(&str, usize)> {
        let mut v: Vec<_> = self.stats.iter().map(|(id, s)| (id.as_str(), s.size_bytes)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v
    }
}

impl Default for AqeOptimizer { fn default() -> Self { Self::new() } }

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn make_block(n: usize) -> DataBlock {
        DataBlock {
            num_rows: n,
            columns: vec![
                Column { name: "id".into(),
                    data: ColumnData::Int64((0..n as i64).map(Some).collect()) },
                Column { name: "region".into(),
                    data: ColumnData::Str((0..n).map(|i| Some(format!("r{}", i % 5))).collect()) },
            ],
        }
    }

    #[test]
    fn test_stats_collection() {
        let block = make_block(100);
        let stats = StageStats::collect("s0", &block, 4);
        assert_eq!(stats.num_rows, 100);
        assert!(stats.size_bytes > 0);
        // id has 100 distinct values
        let id_stats = stats.columns.get("id").unwrap();
        assert_eq!(id_stats.ndv, 100);
        // region has 5 distinct values
        let reg_stats = stats.columns.get("region").unwrap();
        assert_eq!(reg_stats.ndv, 5);
    }

    #[test]
    fn test_broadcast_decision() {
        let mut opt = AqeOptimizer::new();
        // Tiny block (8 rows × 8 bytes × 2 cols = 128 bytes)
        let tiny = make_block(8);
        let stats = StageStats::collect("small", &tiny, 1);
        opt.record(stats);
        assert!(opt.should_broadcast("small"));

        // Large block — set size_bytes manually to exceed threshold
        let large = make_block(10);
        let mut stats2 = StageStats::collect("large", &large, 10);
        stats2.size_bytes = 20 * 1024 * 1024;   // 20 MB
        opt.record(stats2);
        assert!(!opt.should_broadcast("large"));
    }

    #[test]
    fn test_skew_detection() {
        let opt = AqeOptimizer::new();
        let rows = vec![100, 105, 95, 1000, 98]; // last bucket is skewed
        let skewed = opt.skewed_partitions("s", &rows);
        assert_eq!(skewed, vec![3]); // index 3 has 1000 rows
    }

    #[test]
    fn test_join_cardinality_estimate() {
        let mut opt = AqeOptimizer::new();
        let l = make_block(1000);
        let r = make_block(500);
        opt.record(StageStats::collect("L", &l, 4));
        opt.record(StageStats::collect("R", &r, 4));
        // region: 5 NDV on both sides; estimate = 1000*500/5 = 100_000
        let est = opt.estimate_join_rows("L", "R", "region").unwrap();
        assert!(est > 0);
    }

    #[test]
    fn test_recommend_partitions() {
        let mut opt = AqeOptimizer::new();
        // 200 MB of data → 200/64 ≈ 4 partitions
        let mut stats = StageStats::collect("s", &make_block(10), 1);
        stats.size_bytes = 200 * 1024 * 1024;
        opt.record(stats);
        let n = opt.recommend_partitions("s");
        assert_eq!(n, 4); // ceil(200/64) = 4
    }
}
