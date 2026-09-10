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
            ColumnData::StrDict { codes, dict } => {
                let mut seen = std::collections::HashSet::new();
                for &c in codes {
                    if c == u8::MAX { null_count += 1; } else if let Some(s) = dict.get(c as usize) { seen.insert(s.clone()); }
                }
                Some(ColStats {
                    name: col_name.into(), dtype: "STRING".into(),
                    row_count, null_count, ndv: seen.len(),
                    min_f64: None, max_f64: None, histogram: None,
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

    /// Re-analyze a table only if it has never been analyzed or if the given
    /// block has significantly more rows than the stored metadata (>20% growth).
    /// Returns `true` if stats were refreshed.
    pub fn analyze_if_stale(&mut self, table_name: &str, block: &DataBlock) -> bool {
        match self.tables.get(table_name) {
            None => {
                self.analyze(table_name, block);
                true
            }
            Some(meta) => {
                let growth = block.num_rows as f64 / meta.row_count.max(1) as f64;
                if growth > 1.2 || growth < 0.8 {
                    self.analyze(table_name, block);
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Check whether a table has been analyzed.
    pub fn has_stats(&self, table: &str) -> bool {
        self.tables.contains_key(table)
    }

    /// List all table names that have been analyzed.
    pub fn analyzed_tables(&self) -> Vec<&str> {
        self.tables.keys().map(|s| s.as_str()).collect()
    }
}

// ─── CatalogProvider trait & supporting types ────────────────────────────────

use kore_core::KoreError;

pub trait CatalogProvider: Send + Sync {
    fn name(&self) -> &str;
    fn list_tables(&self) -> Vec<String>;
    fn get_table(&self, name: &str) -> Option<TableInfo>;
    fn create_table(&mut self, name: &str, schema: Vec<ColumnInfo>) -> Result<(), KoreError>;
    fn drop_table(&mut self, name: &str) -> Result<(), KoreError>;
    fn table_exists(&self, name: &str) -> bool;
}

#[derive(Debug, Clone)]
pub struct TableInfo {
    pub name: String,
    pub columns: Vec<ColumnInfo>,
    pub row_count: usize,
    pub size_bytes: usize,
    pub partitions: Vec<PartitionInfo>,
    pub format: TableFormat,
    pub location: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: ColumnType,
    pub nullable: bool,
    pub stats: Option<ColStats>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ColumnType {
    Int64,
    Float64,
    Utf8,
    Boolean,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TableFormat {
    Kore,
    Delta,
    Iceberg,
    Parquet,
    Csv,
}

#[derive(Debug, Clone)]
pub struct PartitionInfo {
    pub id: usize,
    pub values: HashMap<String, String>,
    pub row_count: usize,
    pub size_bytes: usize,
    pub path: String,
}

// ─── InMemoryCatalog ─────────────────────────────────────────────────────────

pub struct InMemoryCatalog {
    catalog_name: String,
    inner: Catalog,
    schemas: HashMap<String, Vec<ColumnInfo>>,
    partitions: HashMap<String, Vec<PartitionInfo>>,
}

impl InMemoryCatalog {
    pub fn new(name: &str) -> Self {
        Self {
            catalog_name: name.to_string(),
            inner: Catalog::new(),
            schemas: HashMap::new(),
            partitions: HashMap::new(),
        }
    }

    pub fn inner(&self) -> &Catalog {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut Catalog {
        &mut self.inner
    }

    pub fn set_partitions(&mut self, table: &str, partitions: Vec<PartitionInfo>) {
        self.partitions.insert(table.to_string(), partitions);
    }
}

impl CatalogProvider for InMemoryCatalog {
    fn name(&self) -> &str {
        &self.catalog_name
    }

    fn list_tables(&self) -> Vec<String> {
        self.schemas.keys().cloned().collect()
    }

    fn get_table(&self, name: &str) -> Option<TableInfo> {
        let columns = self.schemas.get(name)?;
        let meta = self.inner.get(name);
        let (row_count, size_bytes) = meta
            .map(|m| (m.row_count, m.size_bytes))
            .unwrap_or((0, 0));
        let partitions = self.partitions.get(name).cloned().unwrap_or_default();
        Some(TableInfo {
            name: name.to_string(),
            columns: columns.clone(),
            row_count,
            size_bytes,
            partitions,
            format: TableFormat::Kore,
            location: None,
        })
    }

    fn create_table(&mut self, name: &str, schema: Vec<ColumnInfo>) -> Result<(), KoreError> {
        if self.schemas.contains_key(name) {
            return Err(KoreError::InvalidArgument(format!(
                "table '{}' already exists",
                name
            )));
        }
        self.schemas.insert(name.to_string(), schema);
        Ok(())
    }

    fn drop_table(&mut self, name: &str) -> Result<(), KoreError> {
        if self.schemas.remove(name).is_none() {
            return Err(KoreError::InvalidArgument(format!(
                "table '{}' not found",
                name
            )));
        }
        self.partitions.remove(name);
        Ok(())
    }

    fn table_exists(&self, name: &str) -> bool {
        self.schemas.contains_key(name)
    }
}

// ─── HiveMetastoreClient (stub) ─────────────────────────────────────────────

pub struct HiveMetastoreClient {
    metastore_uri: String,
}

impl HiveMetastoreClient {
    pub fn new(metastore_uri: &str) -> Self {
        Self {
            metastore_uri: metastore_uri.to_string(),
        }
    }

    pub fn uri(&self) -> &str {
        &self.metastore_uri
    }
}

impl CatalogProvider for HiveMetastoreClient {
    fn name(&self) -> &str {
        "hive"
    }

    fn list_tables(&self) -> Vec<String> {
        vec![]
    }

    fn get_table(&self, _name: &str) -> Option<TableInfo> {
        None
    }

    fn create_table(&mut self, _name: &str, _schema: Vec<ColumnInfo>) -> Result<(), KoreError> {
        Err(KoreError::InvalidArgument(
            "Hive metastore not connected".into(),
        ))
    }

    fn drop_table(&mut self, _name: &str) -> Result<(), KoreError> {
        Err(KoreError::InvalidArgument(
            "Hive metastore not connected".into(),
        ))
    }

    fn table_exists(&self, _name: &str) -> bool {
        false
    }
}

// ─── Partition pruning ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PartitionFilter {
    pub column: String,
    pub op: FilterOp,
    pub value: String,
}

#[derive(Debug, Clone)]
pub enum FilterOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In(Vec<String>),
}

fn partition_matches(partition: &PartitionInfo, filter: &PartitionFilter) -> bool {
    let Some(pval) = partition.values.get(&filter.column) else {
        return true;
    };
    match &filter.op {
        FilterOp::Eq => pval == &filter.value,
        FilterOp::Ne => pval != &filter.value,
        FilterOp::Lt => pval.as_str() < filter.value.as_str(),
        FilterOp::Le => pval.as_str() <= filter.value.as_str(),
        FilterOp::Gt => pval.as_str() > filter.value.as_str(),
        FilterOp::Ge => pval.as_str() >= filter.value.as_str(),
        FilterOp::In(set) => set.iter().any(|v| v == pval),
    }
}

pub fn prune_partitions<'a>(
    partitions: &'a [PartitionInfo],
    filters: &[PartitionFilter],
) -> Vec<&'a PartitionInfo> {
    partitions
        .iter()
        .filter(|p| filters.iter().all(|f| partition_matches(p, f)))
        .collect()
}

// ─── UnifiedCatalog ─────────────────────────────────────────────────────────

pub struct UnifiedCatalog {
    providers: HashMap<String, Box<dyn CatalogProvider>>,
}

impl UnifiedCatalog {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register_provider(&mut self, name: &str, provider: Box<dyn CatalogProvider>) {
        self.providers.insert(name.to_string(), provider);
    }

    pub fn list_all_tables(&self) -> Vec<(String, String)> {
        self.providers
            .iter()
            .flat_map(|(pname, prov)| {
                prov.list_tables()
                    .into_iter()
                    .map(move |t| (pname.clone(), t))
            })
            .collect()
    }

    pub fn get_table(&self, provider: &str, table: &str) -> Option<TableInfo> {
        self.providers.get(provider)?.get_table(table)
    }
}

impl Default for UnifiedCatalog {
    fn default() -> Self {
        Self::new()
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

    // ─── InMemoryCatalog tests ───────────────────────────────────────────

    fn sample_schema() -> Vec<ColumnInfo> {
        vec![
            ColumnInfo { name: "id".into(), data_type: ColumnType::Int64, nullable: false, stats: None },
            ColumnInfo { name: "name".into(), data_type: ColumnType::Utf8, nullable: true, stats: None },
            ColumnInfo { name: "score".into(), data_type: ColumnType::Float64, nullable: true, stats: None },
        ]
    }

    #[test]
    fn test_in_memory_catalog_create_and_list() {
        let mut cat = InMemoryCatalog::new("test_catalog");
        assert_eq!(cat.name(), "test_catalog");
        assert!(cat.list_tables().is_empty());

        cat.create_table("users", sample_schema()).unwrap();
        assert!(cat.table_exists("users"));
        assert!(!cat.table_exists("orders"));

        let tables = cat.list_tables();
        assert_eq!(tables.len(), 1);
        assert!(tables.contains(&"users".to_string()));
    }

    #[test]
    fn test_in_memory_catalog_get_table() {
        let mut cat = InMemoryCatalog::new("mem");
        cat.create_table("events", sample_schema()).unwrap();

        let info = cat.get_table("events").unwrap();
        assert_eq!(info.name, "events");
        assert_eq!(info.columns.len(), 3);
        assert_eq!(info.format, TableFormat::Kore);
        assert!(info.location.is_none());
        assert!(info.partitions.is_empty());
    }

    #[test]
    fn test_in_memory_catalog_get_nonexistent() {
        let cat = InMemoryCatalog::new("mem");
        assert!(cat.get_table("missing").is_none());
    }

    #[test]
    fn test_in_memory_catalog_duplicate_create() {
        let mut cat = InMemoryCatalog::new("mem");
        cat.create_table("t1", sample_schema()).unwrap();
        let err = cat.create_table("t1", sample_schema());
        assert!(err.is_err());
    }

    #[test]
    fn test_in_memory_catalog_drop() {
        let mut cat = InMemoryCatalog::new("mem");
        cat.create_table("t1", sample_schema()).unwrap();
        cat.create_table("t2", sample_schema()).unwrap();
        assert_eq!(cat.list_tables().len(), 2);

        cat.drop_table("t1").unwrap();
        assert!(!cat.table_exists("t1"));
        assert!(cat.table_exists("t2"));
    }

    #[test]
    fn test_in_memory_catalog_drop_nonexistent() {
        let mut cat = InMemoryCatalog::new("mem");
        let err = cat.drop_table("nope");
        assert!(err.is_err());
    }

    #[test]
    fn test_in_memory_catalog_with_partitions() {
        let mut cat = InMemoryCatalog::new("mem");
        cat.create_table("logs", sample_schema()).unwrap();
        let parts = vec![
            PartitionInfo {
                id: 0,
                values: HashMap::from([("date".into(), "2025-01-01".into())]),
                row_count: 1000,
                size_bytes: 8000,
                path: "/data/logs/date=2025-01-01".into(),
            },
            PartitionInfo {
                id: 1,
                values: HashMap::from([("date".into(), "2025-01-02".into())]),
                row_count: 1500,
                size_bytes: 12000,
                path: "/data/logs/date=2025-01-02".into(),
            },
        ];
        cat.set_partitions("logs", parts);

        let info = cat.get_table("logs").unwrap();
        assert_eq!(info.partitions.len(), 2);
        assert_eq!(info.partitions[0].path, "/data/logs/date=2025-01-01");
    }

    // ─── HiveMetastoreClient tests ───────────────────────────────────────

    #[test]
    fn test_hive_client_stub() {
        let mut hive = HiveMetastoreClient::new("thrift://metastore:9083");
        assert_eq!(hive.name(), "hive");
        assert_eq!(hive.uri(), "thrift://metastore:9083");
        assert!(hive.list_tables().is_empty());
        assert!(!hive.table_exists("anything"));
        assert!(hive.get_table("anything").is_none());
        assert!(hive.create_table("t", vec![]).is_err());
        assert!(hive.drop_table("t").is_err());
    }

    // ─── Partition pruning tests ─────────────────────────────────────────

    fn sample_partitions() -> Vec<PartitionInfo> {
        vec![
            PartitionInfo {
                id: 0,
                values: HashMap::from([("region".into(), "US".into()), ("year".into(), "2024".into())]),
                row_count: 1000, size_bytes: 8000,
                path: "/data/region=US/year=2024".into(),
            },
            PartitionInfo {
                id: 1,
                values: HashMap::from([("region".into(), "EU".into()), ("year".into(), "2024".into())]),
                row_count: 800, size_bytes: 6400,
                path: "/data/region=EU/year=2024".into(),
            },
            PartitionInfo {
                id: 2,
                values: HashMap::from([("region".into(), "US".into()), ("year".into(), "2025".into())]),
                row_count: 1200, size_bytes: 9600,
                path: "/data/region=US/year=2025".into(),
            },
            PartitionInfo {
                id: 3,
                values: HashMap::from([("region".into(), "EU".into()), ("year".into(), "2025".into())]),
                row_count: 900, size_bytes: 7200,
                path: "/data/region=EU/year=2025".into(),
            },
        ]
    }

    #[test]
    fn test_prune_eq() {
        let parts = sample_partitions();
        let filters = vec![PartitionFilter {
            column: "region".into(), op: FilterOp::Eq, value: "US".into(),
        }];
        let result = prune_partitions(&parts, &filters);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|p| p.values["region"] == "US"));
    }

    #[test]
    fn test_prune_ne() {
        let parts = sample_partitions();
        let filters = vec![PartitionFilter {
            column: "region".into(), op: FilterOp::Ne, value: "US".into(),
        }];
        let result = prune_partitions(&parts, &filters);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|p| p.values["region"] == "EU"));
    }

    #[test]
    fn test_prune_gt() {
        let parts = sample_partitions();
        let filters = vec![PartitionFilter {
            column: "year".into(), op: FilterOp::Gt, value: "2024".into(),
        }];
        let result = prune_partitions(&parts, &filters);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|p| p.values["year"] == "2025"));
    }

    #[test]
    fn test_prune_le() {
        let parts = sample_partitions();
        let filters = vec![PartitionFilter {
            column: "year".into(), op: FilterOp::Le, value: "2024".into(),
        }];
        let result = prune_partitions(&parts, &filters);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|p| p.values["year"] == "2024"));
    }

    #[test]
    fn test_prune_lt() {
        let parts = sample_partitions();
        let filters = vec![PartitionFilter {
            column: "year".into(), op: FilterOp::Lt, value: "2025".into(),
        }];
        let result = prune_partitions(&parts, &filters);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|p| p.values["year"] == "2024"));
    }

    #[test]
    fn test_prune_ge() {
        let parts = sample_partitions();
        let filters = vec![PartitionFilter {
            column: "year".into(), op: FilterOp::Ge, value: "2025".into(),
        }];
        let result = prune_partitions(&parts, &filters);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|p| p.values["year"] == "2025"));
    }

    #[test]
    fn test_prune_in() {
        let parts = sample_partitions();
        let filters = vec![PartitionFilter {
            column: "region".into(),
            op: FilterOp::In(vec!["US".into(), "APAC".into()]),
            value: String::new(),
        }];
        let result = prune_partitions(&parts, &filters);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|p| p.values["region"] == "US"));
    }

    #[test]
    fn test_prune_multiple_filters() {
        let parts = sample_partitions();
        let filters = vec![
            PartitionFilter { column: "region".into(), op: FilterOp::Eq, value: "US".into() },
            PartitionFilter { column: "year".into(), op: FilterOp::Eq, value: "2025".into() },
        ];
        let result = prune_partitions(&parts, &filters);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, 2);
    }

    #[test]
    fn test_prune_no_filters_returns_all() {
        let parts = sample_partitions();
        let result = prune_partitions(&parts, &[]);
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn test_prune_unknown_column_passes_all() {
        let parts = sample_partitions();
        let filters = vec![PartitionFilter {
            column: "nonexistent".into(), op: FilterOp::Eq, value: "x".into(),
        }];
        let result = prune_partitions(&parts, &filters);
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn test_prune_no_match() {
        let parts = sample_partitions();
        let filters = vec![PartitionFilter {
            column: "region".into(), op: FilterOp::Eq, value: "APAC".into(),
        }];
        let result = prune_partitions(&parts, &filters);
        assert!(result.is_empty());
    }

    // ─── UnifiedCatalog tests ────────────────────────────────────────────

    #[test]
    fn test_unified_catalog_register_and_list() {
        let mut unified = UnifiedCatalog::new();

        let mut mem1 = InMemoryCatalog::new("warehouse");
        mem1.create_table("orders", sample_schema()).unwrap();
        mem1.create_table("customers", sample_schema()).unwrap();

        let mut mem2 = InMemoryCatalog::new("staging");
        mem2.create_table("raw_events", sample_schema()).unwrap();

        unified.register_provider("warehouse", Box::new(mem1));
        unified.register_provider("staging", Box::new(mem2));

        let all = unified.list_all_tables();
        assert_eq!(all.len(), 3);

        let warehouse_tables: Vec<_> = all.iter()
            .filter(|(p, _)| p == "warehouse")
            .map(|(_, t)| t.clone())
            .collect();
        assert_eq!(warehouse_tables.len(), 2);
        assert!(warehouse_tables.contains(&"orders".to_string()));
        assert!(warehouse_tables.contains(&"customers".to_string()));

        let staging_tables: Vec<_> = all.iter()
            .filter(|(p, _)| p == "staging")
            .map(|(_, t)| t.clone())
            .collect();
        assert_eq!(staging_tables.len(), 1);
        assert!(staging_tables.contains(&"raw_events".to_string()));
    }

    #[test]
    fn test_unified_catalog_get_table() {
        let mut unified = UnifiedCatalog::new();
        let mut mem = InMemoryCatalog::new("main");
        mem.create_table("users", sample_schema()).unwrap();
        unified.register_provider("main", Box::new(mem));

        let info = unified.get_table("main", "users").unwrap();
        assert_eq!(info.name, "users");
        assert_eq!(info.columns.len(), 3);

        assert!(unified.get_table("main", "nonexistent").is_none());
        assert!(unified.get_table("other_provider", "users").is_none());
    }

    #[test]
    fn test_unified_catalog_with_hive_stub() {
        let mut unified = UnifiedCatalog::new();
        let hive = HiveMetastoreClient::new("thrift://host:9083");
        unified.register_provider("hive", Box::new(hive));

        let all = unified.list_all_tables();
        assert!(all.is_empty());
        assert!(unified.get_table("hive", "any_table").is_none());
    }

    #[test]
    fn test_unified_catalog_empty() {
        let unified = UnifiedCatalog::new();
        assert!(unified.list_all_tables().is_empty());
        assert!(unified.get_table("any", "table").is_none());
    }
}
