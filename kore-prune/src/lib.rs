//! KORE Layer 48 — Zone-Map Partition Pruning
//!
//! Zone maps (also called small file statistics, min/max indexes, or data
//! skipping indexes) store per-partition min/max values for each column.
//!
//! Before reading a partition, we check whether the query predicate could
//! possibly match any row in that partition.  If not, we skip it entirely.
//!
//! This mirrors:
//!   - Apache Spark's Dynamic Partition Pruning
//!   - Parquet row-group filtering
//!   - DeltaLake data skipping
//!   - Snowflake micro-partition pruning
//!
//! For a table with N partitions and a selective predicate, pruning can
//! reduce I/O by 90%+ on sorted or range-partitioned data.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use kore_core::{ColumnData, DataBlock};

// ─── Zone map for one column ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ColZone {
    pub col_name:   String,
    pub null_count: usize,
    pub row_count:  usize,
    // Numeric range
    pub min_i64:  Option<i64>,
    pub max_i64:  Option<i64>,
    pub min_f64:  Option<f64>,
    pub max_f64:  Option<f64>,
    // String range (lexicographic)
    pub min_str:  Option<String>,
    pub max_str:  Option<String>,
    // Boolean range
    pub has_true:  bool,
    pub has_false: bool,
}

impl ColZone {
    pub fn build(col_name: &str, data: &ColumnData) -> Self {
        let mut z = ColZone { col_name: col_name.to_string(), row_count: data.len(), ..Default::default() };
        match data {
            ColumnData::Int64(v) => {
                let mut mn = i64::MAX;
                let mut mx = i64::MIN;
                for x in v {
                    match x { None => z.null_count += 1, Some(i) => { if *i < mn { mn = *i; } if *i > mx { mx = *i; } } }
                }
                if mn <= mx { z.min_i64 = Some(mn); z.max_i64 = Some(mx); }
            }
            ColumnData::Float64(v) => {
                let mut mn = f64::INFINITY;
                let mut mx = f64::NEG_INFINITY;
                for x in v {
                    match x { None => z.null_count += 1, Some(f) => { if *f < mn { mn = *f; } if *f > mx { mx = *f; } } }
                }
                if mn.is_finite() { z.min_f64 = Some(mn); z.max_f64 = Some(mx); }
            }
            ColumnData::Str(v) => {
                let mut mn: Option<&String> = None;
                let mut mx: Option<&String> = None;
                for x in v {
                    match x {
                        None    => z.null_count += 1,
                        Some(s) => {
                            if mn.map(|m| s < m).unwrap_or(true)  { mn = Some(s); }
                            if mx.map(|m| s > m).unwrap_or(true)  { mx = Some(s); }
                        }
                    }
                }
                z.min_str = mn.cloned();
                z.max_str = mx.cloned();
            }
            ColumnData::Bool(v) => {
                for x in v {
                    match x { None => z.null_count += 1, Some(true) => z.has_true = true, Some(false) => z.has_false = true }
                }
            }
        }
        z
    }

    // ── Pruning predicates ────────────────────────────────────────────────────

    /// Can any row satisfy `col > val`?
    pub fn can_be_gt_f64(&self, val: f64) -> bool {
        self.max_f64.map(|mx| mx > val).unwrap_or(
            self.max_i64.map(|mx| (mx as f64) > val).unwrap_or(true)
        )
    }
    pub fn can_be_ge_f64(&self, val: f64) -> bool {
        self.max_f64.map(|mx| mx >= val).unwrap_or(
            self.max_i64.map(|mx| (mx as f64) >= val).unwrap_or(true)
        )
    }
    pub fn can_be_lt_f64(&self, val: f64) -> bool {
        self.min_f64.map(|mn| mn < val).unwrap_or(
            self.min_i64.map(|mn| (mn as f64) < val).unwrap_or(true)
        )
    }
    pub fn can_be_le_f64(&self, val: f64) -> bool {
        self.min_f64.map(|mn| mn <= val).unwrap_or(
            self.min_i64.map(|mn| (mn as f64) <= val).unwrap_or(true)
        )
    }
    pub fn can_be_eq_f64(&self, val: f64) -> bool {
        let mn = self.min_f64.or(self.min_i64.map(|i| i as f64));
        let mx = self.max_f64.or(self.max_i64.map(|i| i as f64));
        match (mn, mx) {
            (Some(lo), Some(hi)) => val >= lo && val <= hi,
            _ => true,
        }
    }
    pub fn can_be_in_range_f64(&self, lo: f64, hi: f64) -> bool {
        let mn = self.min_f64.or(self.min_i64.map(|i| i as f64));
        let mx = self.max_f64.or(self.max_i64.map(|i| i as f64));
        match (mn, mx) {
            (Some(min_v), Some(max_v)) => max_v >= lo && min_v <= hi,
            _ => true,
        }
    }
    pub fn can_be_eq_str(&self, val: &str) -> bool {
        match (&self.min_str, &self.max_str) {
            (Some(lo), Some(hi)) => val >= lo.as_str() && val <= hi.as_str(),
            _ => true,
        }
    }
    pub fn has_nulls(&self) -> bool { self.null_count > 0 }
    pub fn all_null(&self) -> bool  { self.null_count == self.row_count }
}

// ─── Partition metadata ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionMeta {
    pub id:        usize,
    pub path:      Option<String>,   // file path (None = in-memory)
    pub row_count: usize,
    pub zones:     Vec<ColZone>,
}

impl PartitionMeta {
    /// Build zone maps from a materialised DataBlock.
    pub fn from_block(id: usize, block: &DataBlock) -> Self {
        let zones = block.columns.iter()
            .map(|c| ColZone::build(&c.name, &c.data))
            .collect();
        Self { id, path: None, row_count: block.num_rows, zones }
    }

    pub fn zone(&self, col: &str) -> Option<&ColZone> {
        self.zones.iter().find(|z| z.col_name == col || z.col_name.ends_with(&format!(".{}", col)))
    }
}

// ─── Pruning predicates ───────────────────────────────────────────────────────

/// A predicate that can be evaluated against zone maps.
#[derive(Debug, Clone)]
pub enum PrunePred {
    ColGtF64 { col: String, val: f64 },
    ColGeF64 { col: String, val: f64 },
    ColLtF64 { col: String, val: f64 },
    ColLeF64 { col: String, val: f64 },
    ColEqF64 { col: String, val: f64 },
    ColBetweenF64 { col: String, lo: f64, hi: f64 },
    ColEqStr { col: String, val: String },
    IsNull    { col: String },
    IsNotNull { col: String },
    And(Box<PrunePred>, Box<PrunePred>),
    Or (Box<PrunePred>, Box<PrunePred>),
    Not(Box<PrunePred>),
}

impl PrunePred {
    /// Returns `false` if we can PROVE no row in `meta` satisfies this predicate.
    pub fn can_match(&self, meta: &PartitionMeta) -> bool {
        match self {
            Self::ColGtF64 { col, val } =>
                meta.zone(col).map(|z| z.can_be_gt_f64(*val)).unwrap_or(true),
            Self::ColGeF64 { col, val } =>
                meta.zone(col).map(|z| z.can_be_ge_f64(*val)).unwrap_or(true),
            Self::ColLtF64 { col, val } =>
                meta.zone(col).map(|z| z.can_be_lt_f64(*val)).unwrap_or(true),
            Self::ColLeF64 { col, val } =>
                meta.zone(col).map(|z| z.can_be_le_f64(*val)).unwrap_or(true),
            Self::ColEqF64 { col, val } =>
                meta.zone(col).map(|z| z.can_be_eq_f64(*val)).unwrap_or(true),
            Self::ColBetweenF64 { col, lo, hi } =>
                meta.zone(col).map(|z| z.can_be_in_range_f64(*lo, *hi)).unwrap_or(true),
            Self::ColEqStr { col, val } =>
                meta.zone(col).map(|z| z.can_be_eq_str(val)).unwrap_or(true),
            Self::IsNull    { col } => meta.zone(col).map(|z| z.has_nulls()).unwrap_or(true),
            Self::IsNotNull { col } => meta.zone(col).map(|z| !z.all_null()).unwrap_or(true),
            Self::And(l, r) => l.can_match(meta) && r.can_match(meta),
            Self::Or (l, r) => l.can_match(meta) || r.can_match(meta),
            Self::Not(inner) => {
                // Conservative: can't prune unless inner is a simple comparison
                let _ = inner;
                true
            }
        }
    }
}

// ─── Pruning engine ───────────────────────────────────────────────────────────

/// Manages a set of partitions and prunes them against predicates.
#[derive(Default)]
pub struct PruningEngine {
    partitions: Vec<PartitionMeta>,
}

impl PruningEngine {
    pub fn new() -> Self { Self::default() }

    pub fn add_partition_from_block(&mut self, id: usize, block: &DataBlock) {
        self.partitions.push(PartitionMeta::from_block(id, block));
    }

    pub fn add_partition_meta(&mut self, meta: PartitionMeta) {
        self.partitions.push(meta);
    }

    /// Return partition IDs that survive the predicate (cannot be pruned).
    pub fn surviving_ids(&self, pred: &PrunePred) -> Vec<usize> {
        self.partitions.iter()
            .filter(|p| pred.can_match(p))
            .map(|p| p.id)
            .collect()
    }

    /// Return partition IDs that are definitely pruned.
    pub fn pruned_ids(&self, pred: &PrunePred) -> Vec<usize> {
        self.partitions.iter()
            .filter(|p| !pred.can_match(p))
            .map(|p| p.id)
            .collect()
    }

    pub fn partition_count(&self) -> usize { self.partitions.len() }

    pub fn total_rows(&self) -> usize { self.partitions.iter().map(|p| p.row_count).sum() }

    /// Pruning ratio: fraction of partitions skipped.
    pub fn prune_ratio(&self, pred: &PrunePred) -> f64 {
        if self.partitions.is_empty() { return 0.0; }
        let pruned = self.pruned_ids(pred).len();
        pruned as f64 / self.partitions.len() as f64
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn range_block(start: i64, end: i64) -> DataBlock {
        DataBlock {
            num_rows: (end - start) as usize,
            columns: vec![
                Column { name: "id".into(), data: ColumnData::Int64(
                    (start..end).map(|i| Some(i)).collect()
                )},
                Column { name: "score".into(), data: ColumnData::Float64(
                    (start..end).map(|i| Some(i as f64 * 10.0)).collect()
                )},
                Column { name: "tag".into(), data: ColumnData::Str(
                    (start..end).map(|i| Some(format!("tag{}", i % 5))).collect()
                )},
            ],
        }
    }

    fn build_engine() -> PruningEngine {
        let mut eng = PruningEngine::new();
        eng.add_partition_from_block(0, &range_block(0,   100));  // id 0..99,   score 0..990
        eng.add_partition_from_block(1, &range_block(100, 200));  // id 100..199
        eng.add_partition_from_block(2, &range_block(200, 300));  // id 200..299
        eng.add_partition_from_block(3, &range_block(300, 400));  // id 300..399
        eng
    }

    #[test]
    fn test_prune_by_range() {
        let eng = build_engine();
        let pred = PrunePred::ColBetweenF64 { col: "score".into(), lo: 1000.0, hi: 1990.0 };
        let surviving = eng.surviving_ids(&pred);
        // score 1000..1990 is in partition 1 (100*10=1000) and 2 (200*10=2000 → 1990)
        assert!(surviving.contains(&1), "{:?}", surviving);
        assert!(!surviving.contains(&0)); // partition 0 maxes at 990
        assert!(!surviving.contains(&3)); // partition 3 starts at 3000
    }

    #[test]
    fn test_prune_gt() {
        let eng = build_engine();
        let pred = PrunePred::ColGtF64 { col: "score".into(), val: 3500.0 };
        let pruned = eng.pruned_ids(&pred);
        // Only partition 3 has score up to 3990; 0,1,2 have max < 3500
        // Wait, partition 3 has 300..400, score 3000..3990, max=3990 > 3500 → not pruned
        // partitions 0,1,2 have max 990, 1990, 2990 all < 3500 → pruned
        assert_eq!(pruned.len(), 3);
        assert!(!pruned.contains(&3));
    }

    #[test]
    fn test_prune_ratio() {
        let eng = build_engine();
        let pred = PrunePred::ColBetweenF64 { col: "id".into(), lo: 50.0, hi: 150.0 };
        let ratio = eng.prune_ratio(&pred);
        // Partitions 2 (200..300) and 3 (300..400) pruned → ratio = 2/4 = 0.5
        assert!((ratio - 0.5).abs() < 0.01, "ratio={ratio}");
    }

    #[test]
    fn test_prune_string_eq() {
        let mut eng = PruningEngine::new();
        let b1 = DataBlock {
            num_rows: 3,
            columns: vec![Column { name: "cat".into(), data: ColumnData::Str(vec![
                Some("A".into()), Some("A".into()), Some("B".into())
            ])}],
        };
        let b2 = DataBlock {
            num_rows: 3,
            columns: vec![Column { name: "cat".into(), data: ColumnData::Str(vec![
                Some("C".into()), Some("D".into()), Some("E".into())
            ])}],
        };
        eng.add_partition_from_block(0, &b1);
        eng.add_partition_from_block(1, &b2);
        let pred = PrunePred::ColEqStr { col: "cat".into(), val: "A".into() };
        let surviving = eng.surviving_ids(&pred);
        // b2 min="C" max="E", "A" < "C" → pruned
        assert_eq!(surviving, vec![0]);
    }

    #[test]
    fn test_and_predicate() {
        let eng = build_engine();
        let pred = PrunePred::And(
            Box::new(PrunePred::ColGeF64 { col: "score".into(), val: 0.0 }),
            Box::new(PrunePred::ColLeF64 { col: "score".into(), val: 500.0 }),
        );
        let surviving = eng.surviving_ids(&pred);
        // score [0,500] only overlaps with partition 0 (score 0..990)
        assert!(surviving.contains(&0));
        // partitions 1,2,3 start at 1000,2000,3000 → pruned
        assert!(!surviving.contains(&1));
    }
}
