//! KORE Layer 44 — Table Catalog with Column Statistics & Histograms
//!
//! The catalog stores per-table, per-column statistics used by the query
//! optimizer for cardinality estimation:
//!
//! - **Equi-depth histograms** — N buckets of equal row counts; each bucket
//!   stores [lo, hi] range.  Used for range-predicate selectivity.
//! - **NDV (number of distinct values)** — for join cardinality.
//! - **Null fraction** — for IS NULL predicate selectivity.
//! - **Column correlation** — detect when sorting one col predicts another.
//!
//! The optimizer uses these to:
//!   - Choose the smaller side for broadcast joins
//!   - Estimate join output rows: |L| × |R| / NDV
//!   - Detect data skew for repartitioning decisions

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use kore_core::{ColumnData, DataBlock};

// ─── Histogram ────────────────────────────────────────────────────────────────

/// One bucket of an equi-depth histogram.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistBucket {
    pub lo:    f64,
    pub hi:    f64,
    pub count: usize,
}

/// Equi-depth histogram over numeric values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Histogram {
    pub buckets: Vec<HistBucket>,
    pub total:   usize,
}

impl Histogram {
    /// Build an equi-depth histogram from a sorted slice of `f64` values.
    pub fn build(data: &mut Vec<f64>, n_buckets: usize) -> Self {
        if data.is_empty() || n_buckets == 0 {
            return Self { buckets: vec![], total: 0 };
        }
        data.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let total  = data.len();
        let target = (total + n_buckets - 1) / n_buckets;
        let mut buckets = Vec::new();
        let mut i = 0;
        while i < total {
            let end  = (i + target).min(total);
            let lo   = data[i];
            let hi   = data[end - 1];
            buckets.push(HistBucket { lo, hi, count: end - i });
            i = end;
        }
        Self { buckets, total }
    }

    /// Estimate the fraction of rows satisfying `lo ≤ x ≤ hi`.
    /// Returns a value in [0.0, 1.0].
    pub fn selectivity(&self, lo: Option<f64>, hi: Option<f64>) -> f64 {
        if self.total == 0 { return 0.0; }
        let lo = lo.unwrap_or(f64::NEG_INFINITY);
        let hi = hi.unwrap_or(f64::INFINITY);
        let matching: usize = self.buckets.iter().map(|b| {
            if b.hi < lo || b.lo > hi { return 0; }
            if b.lo >= lo && b.hi <= hi { return b.count; }
            // Partial overlap: linear interpolation within the bucket
            let bucket_range = (b.hi - b.lo).max(1e-10);
            let overlap_lo   = b.lo.max(lo);
            let overlap_hi   = b.hi.min(hi);
            let frac = (overlap_hi - overlap_lo) / bucket_range;
            (b.count as f64 * frac.clamp(0.0, 1.0)) as usize
        }).sum();
        matching as f64 / self.total as f64
    }
}

// ─── Column statistics ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColStats {
    pub name:       String,
    pub dtype:      String,
    pub row_count:  usize,
    pub null_count: usize,
    pub ndv:        usize,        // number of distinct values
    pub min_f64:    Option<f64>,
    pub max_f64:    Option<f64>,
    pub histogram:  Option<Histogram>,
}

impl ColStats {
    /// Collect statistics from a column in a DataBlock.
    pub fn collect(block: &DataBlock, col_name: &str, n_buckets: usize) -> Option<Self> {
        let col = block.columns.iter().find(|c| c.name == col_name
            || c.name.ends_with(&format!(".{}", col_name)))?;

        let mut null_count = 0usize;
        let row_count = col.data.len();

        match &col.data {
            ColumnData::Int64(v) => {
                let mut vals: Vec<f64> = Vec::new();
                let mut seen = std::collections::HashSet::new();
                let mut min = i64::MAX;
                let mut max = i64::MIN;
                for x in v {
                    match x {
                        None    => null_count += 1,
                        Some(i) => {
                            vals.push(*i as f64);
                            seen.insert(*i);
                            if *i < min { min = *i; }
                            if *i > max { max = *i; }
                        }
                    }
                }
                let hist = Histogram::build(&mut vals, n_buckets);
                Some(ColStats {
                    name: col_name.into(), dtype: "INT64".into(),
                    row_count, null_count, ndv: seen.len(),
                    min_f64: if min <= max { Some(min as f64) } else { None },
                    max_f64: if min <= max { Some(max as f64) } else { None },
                    histogram: Some(hist),
                })
            }
            ColumnData::Float64(v) => {
                let mut vals: Vec<f64> = Vec::new();
                let mut min = f64::INFINITY;
                let mut max = f64::NEG_INFINITY;
                let mut seen_keys = std::collections::HashSet::<u64>::new();
                for x in v {
                    match x {
                        None    => null_count += 1,
                        Some(f) => {
                            vals.push(*f);
                            seen_keys.insert(f.to_bits());
                            if *f < min { min = *f; }
                            if *f > max { max = *f; }
                        }
                    }
                }
                let hist = Histogram::build(&mut vals, n_buckets);
                Some(ColStats {
                    name: col_name.into(), dtype: "FLOAT64".into(),
                    row_count, null_count, ndv: seen_keys.len(),
                    min_f64: if min.is_finite() { Some(min) } else { None },
                    max_f64: if min.is_finite() { Some(max) } else { None },
                    histogram: Some(hist),
                })
            }
            ColumnData::Str(v) => {
                let mut seen = std::collections::HashSet::new();
                for x in v {
                    match x {
                        None    => null_count += 1,
                        Some(s) => { seen.insert(s.clone()); }
                    }
                }
                Some(ColStats {
                    name: col_name.into(), dtype: "STRING".into(),
                    row_count, null_count, ndv: seen.len(),
                    min_f64: None, max_f64: None, histogram: None,
                })
            }
            ColumnData::Bool(v) => {
                for x in v { if x.is_none() { null_count += 1; } }
                Some(ColStats {
                    name: col_name.into(), dtype: "BOOL".into(),
                    row_count, null_count, ndv: 2,
                    min_f64: Some(0.0), max_f64: Some(1.0), histogram: None,
                })
            }
        }
    }

    pub fn null_fraction(&self) -> f64 {
        if self.row_count == 0 { 0.0 } else { self.null_count as f64 / self.row_count as f64 }
    }

    pub fn selectivity_range(&self, lo: Option<f64>, hi: Option<f64>) -> f64 {
        let not_null = 1.0 - self.null_fraction();
        match &self.histogram {
            Some(h) => h.selectivity(lo, hi) * not_null,
            None    => {
                // Uniform distribution assumption
                let (tlo, thi) = match (self.min_f64, self.max_f64) {
                    (Some(a), Some(b)) if b > a => (a, b),
                    _ => return 1.0 * not_null,
                };
                let range    = thi - tlo;
                let ql       = lo.unwrap_or(tlo).max(tlo);
                let qh       = hi.unwrap_or(thi).min(thi);
                let frac     = ((qh - ql) / range).clamp(0.0, 1.0);
                frac * not_null
            }
        }
    }
}

// ─── Table metadata ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TableMeta {
    pub name:        String,
    pub row_count:   usize,
    pub size_bytes:  usize,
    pub col_stats:   Vec<ColStats>,
}

impl TableMeta {
    pub fn col(&self, name: &str) -> Option<&ColStats> {
        self.col_stats.iter().find(|c| c.name == name || c.name.ends_with(&format!(".{}", name)))
    }
}

// ─── Catalog ──────────────────────────────────────────────────────────────────

/// Registry of table statistics used by the optimizer.
#[derive(Default)]
pub struct Catalog {
    tables: HashMap<String, TableMeta>,
}

impl Catalog {
    pub fn new() -> Self { Self::default() }

    /// Analyze a DataBlock and store statistics for `table_name`.
    pub fn analyze(&mut self, table_name: &str, block: &DataBlock) {
        let col_names: Vec<String> = block.columns.iter().map(|c| c.name.clone()).collect();
        let col_stats = col_names.iter()
            .filter_map(|n| ColStats::collect(block, n, 50))
            .collect();
        let size_bytes = block.columns.iter().map(|c| c.data.len() * 8).sum();
        self.tables.insert(table_name.into(), TableMeta {
            name:       table_name.into(),
            row_count:  block.num_rows,
            size_bytes,
            col_stats,
        });
    }

    pub fn get(&self, table: &str) -> Option<&TableMeta> {
        self.tables.get(table)
    }

    /// Estimate output rows for `table WHERE col BETWEEN lo AND hi`.
    pub fn estimate_filter_rows(&self, table: &str, col: &str, lo: Option<f64>, hi: Option<f64>) -> Option<usize> {
        let meta = self.tables.get(table)?;
        let cs   = meta.col(col)?;
        let sel  = cs.selectivity_range(lo, hi);
        Some((meta.row_count as f64 * sel).ceil() as usize)
    }

    /// Estimate inner-join output rows: |L| × |R| / NDV(key).
    pub fn estimate_join_rows(&self, lt: &str, rt: &str, key: &str) -> Option<usize> {
        let lm  = self.tables.get(lt)?;
        let rm  = self.tables.get(rt)?;
        let lk  = lm.col(key)?;
        let rk  = rm.col(key)?;
        let ndv = lk.ndv.max(rk.ndv).max(1);
        Some((lm.row_count * rm.row_count) / ndv)
    }

    /// Should the table be broadcast? (below a size threshold)
    pub fn should_broadcast(&self, table: &str, threshold_bytes: usize) -> bool {
        self.tables.get(table)
            .map(|m| m.size_bytes < threshold_bytes)
            .unwrap_or(false)
    }

    /// Return table names sorted by estimated row count (smallest first).
    /// Used to pick the build side of a broadcast join.
    pub fn tables_by_size(&self) -> Vec<(&str, usize)> {
        let mut v: Vec<_> = self.tables.iter().map(|(n, m)| (n.as_str(), m.row_count)).collect();
        v.sort_by_key(|&(_, r)| r);
        v
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn sales_block() -> DataBlock {
        DataBlock {
            num_rows: 100,
            columns: vec![
                Column { name: "price".into(), data: ColumnData::Float64(
                    (0..100).map(|i| Some(i as f64 * 10.0)).collect()
                )},
                Column { name: "region".into(), data: ColumnData::Str(
                    (0..100).map(|i| Some(format!("R{}", i % 5))).collect()
                )},
                Column { name: "qty".into(), data: ColumnData::Int64(
                    (0..100).map(|i| Some(i as i64 % 20)).collect()
                )},
            ],
        }
    }

    #[test]
    fn test_histogram_build_selectivity() {
        let mut vals: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let h = Histogram::build(&mut vals, 10);
        assert_eq!(h.buckets.len(), 10);
        // Range [25, 75] should cover ~50% of [0,100]
        let sel = h.selectivity(Some(25.0), Some(75.0));
        assert!(sel > 0.40 && sel < 0.65, "sel={sel}");
    }

    #[test]
    fn test_catalog_analyze() {
        let mut cat = Catalog::new();
        cat.analyze("sales", &sales_block());
        let meta = cat.get("sales").unwrap();
        assert_eq!(meta.row_count, 100);

        let price_stats = meta.col("price").unwrap();
        assert_eq!(price_stats.min_f64, Some(0.0));
        assert_eq!(price_stats.max_f64, Some(990.0));
        assert_eq!(price_stats.ndv, 100);

        let region_stats = meta.col("region").unwrap();
        assert_eq!(region_stats.ndv, 5);
    }

    #[test]
    fn test_filter_estimation() {
        let mut cat = Catalog::new();
        cat.analyze("sales", &sales_block());
        // price IN [200, 500] = 30 out of 100 rows (index 20..50)
        let est = cat.estimate_filter_rows("sales", "price", Some(200.0), Some(500.0));
        assert!(est.is_some());
        let e = est.unwrap();
        assert!(e > 10 && e < 60, "est={e}");
    }

    #[test]
    fn test_join_cardinality() {
        let mut cat = Catalog::new();
        cat.analyze("orders",  &sales_block());
        cat.analyze("products", &sales_block());
        let est = cat.estimate_join_rows("orders", "products", "region").unwrap();
        // 100 * 100 / 5 = 2000
        assert_eq!(est, 2000);
    }

    #[test]
    fn test_broadcast_hint() {
        let mut cat = Catalog::new();
        let small = DataBlock {
            num_rows: 5,
            columns: vec![Column { name: "id".into(), data: ColumnData::Int64(
                (0..5).map(|i| Some(i)).collect()
            )}],
        };
        cat.analyze("dim", &small);
        assert!(cat.should_broadcast("dim", 10 * 1024 * 1024));
    }
}
