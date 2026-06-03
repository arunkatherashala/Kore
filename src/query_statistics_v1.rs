/// Query Statistics Engine for KORE v1.6.0
/// 
/// Provides cost estimation, cardinality estimation, and selectivity analysis
/// for the query optimizer. Uses histogram-based statistics for accuracy.
/// 
/// Statistics Types:
/// - Column: min, max, distinct count, null count, histogram
/// - Table: row count, total size, column stats
/// - Cost: I/O, CPU, memory estimates

use crate::kore_v2::{KVal, KType};
use std::collections::{HashMap, BTreeMap};
use std::cmp::Ordering;

/// Unique identifier for a column in statistics context
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ColumnId(pub u32);

/// Represents selectivity range for a predicate
#[derive(Debug, Clone)]
pub struct Selectivity {
    pub lower_bound: f64,  // Conservative estimate (lower selectivity)
    pub upper_bound: f64,  // Optimistic estimate (higher selectivity)
    pub best_estimate: f64, // Expected value
}

impl Selectivity {
    pub fn new(lower: f64, upper: f64, best: f64) -> Self {
        Selectivity {
            lower_bound: lower.max(0.0).min(1.0),
            upper_bound: upper.max(0.0).min(1.0),
            best_estimate: best.max(0.0).min(1.0),
        }
    }

    pub fn certain() -> Self {
        Selectivity { lower_bound: 1.0, upper_bound: 1.0, best_estimate: 1.0 }
    }

    pub fn impossible() -> Self {
        Selectivity { lower_bound: 0.0, upper_bound: 0.0, best_estimate: 0.0 }
    }
}

/// Histogram bucket for value distribution
#[derive(Debug, Clone)]
pub struct HistogramBucket {
    pub min_value: KVal,
    pub max_value: KVal,
    pub count: u64,
    pub distinct_count: u64,
}

impl HistogramBucket {
    pub fn new(min: KVal, max: KVal, count: u64, distinct: u64) -> Self {
        HistogramBucket {
            min_value: min,
            max_value: max,
            count,
            distinct_count: distinct,
        }
    }
}

/// Statistics for a single column
#[derive(Debug, Clone)]
pub struct ColumnStats {
    pub column_id: ColumnId,
    pub column_name: String,
    pub column_type: KType,
    
    // Basic stats
    pub min_value: Option<KVal>,
    pub max_value: Option<KVal>,
    pub distinct_count: u64,
    pub null_count: u64,
    pub total_count: u64,
    
    // Histograms for value distribution
    pub histogram: Vec<HistogramBucket>,
    
    // Selectivity cache (populated by optimizer)
    pub selectivity_cache: HashMap<String, Selectivity>,
}

impl ColumnStats {
    pub fn new(
        col_id: ColumnId,
        col_name: String,
        col_type: KType,
        total: u64,
    ) -> Self {
        ColumnStats {
            column_id: col_id,
            column_name: col_name,
            column_type: col_type,
            min_value: None,
            max_value: None,
            distinct_count: 0,
            null_count: 0,
            total_count: total,
            histogram: Vec::new(),
            selectivity_cache: HashMap::new(),
        }
    }

    /// Null percentage of this column
    pub fn null_percentage(&self) -> f64 {
        if self.total_count == 0 { 0.0 } else {
            (self.null_count as f64) / (self.total_count as f64)
        }
    }

    /// Non-null cardinality
    pub fn non_null_count(&self) -> u64 {
        self.total_count.saturating_sub(self.null_count)
    }

    /// Ratio of distinct values to non-null values (uniqueness)
    pub fn uniqueness(&self) -> f64 {
        let non_null = self.non_null_count() as f64;
        if non_null == 0.0 { 0.0 } else {
            (self.distinct_count as f64) / non_null
        }
    }

    /// Average frequency per distinct value
    pub fn average_frequency(&self) -> f64 {
        if self.distinct_count == 0 { 0.0 } else {
            (self.non_null_count() as f64) / (self.distinct_count as f64)
        }
    }

    /// Estimate selectivity for an equality predicate
    pub fn estimate_equality_selectivity(&self, _value: &KVal) -> f64 {
        if self.distinct_count == 0 { 0.0 } else {
            1.0 / (self.distinct_count as f64)
        }
    }

    /// Estimate selectivity for a range predicate (val1 <= col <= val2)
    pub fn estimate_range_selectivity(&self, val1: &KVal, val2: &KVal) -> f64 {
        match (self.min_value.as_ref(), self.max_value.as_ref()) {
            (Some(min), Some(max)) => {
                // Simple linear interpolation
                let col_range = match (min, max) {
                    (KVal::Int(min_v), KVal::Int(max_v)) => {
                        (*max_v as f64 - *min_v as f64).max(1.0)
                    }
                    (KVal::Float(min_v), KVal::Float(max_v)) => {
                        (max_v - min_v).abs().max(1.0)
                    }
                    _ => 1.0,
                };

                let query_range = match (val1, val2) {
                    (KVal::Int(v1), KVal::Int(v2)) => {
                        (*v2 as f64 - *v1 as f64).max(0.0)
                    }
                    (KVal::Float(v1), KVal::Float(v2)) => {
                        (v2 - v1).abs().max(0.0)
                    }
                    _ => col_range,
                };

                (query_range / col_range).max(0.0).min(1.0)
            }
            _ => 0.5, // Unknown distribution
        }
    }

    /// Estimate selectivity for a NOT_NULL predicate
    pub fn estimate_not_null_selectivity(&self) -> f64 {
        if self.total_count == 0 { 1.0 } else {
            (self.non_null_count() as f64) / (self.total_count as f64)
        }
    }

    /// Estimate selectivity for IS_NULL predicate
    pub fn estimate_is_null_selectivity(&self) -> f64 {
        if self.total_count == 0 { 0.0 } else {
            (self.null_count as f64) / (self.total_count as f64)
        }
    }
}

/// Statistics for an entire table
#[derive(Debug, Clone)]
pub struct TableStats {
    pub table_name: String,
    pub row_count: u64,
    pub total_size_bytes: u64,
    pub columns: HashMap<ColumnId, ColumnStats>,
    pub column_name_to_id: HashMap<String, ColumnId>,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

impl TableStats {
    pub fn new(table_name: String, row_count: u64) -> Self {
        TableStats {
            table_name,
            row_count,
            total_size_bytes: 0,
            columns: HashMap::new(),
            column_name_to_id: HashMap::new(),
            last_updated: chrono::Utc::now(),
        }
    }

    /// Add column statistics
    pub fn add_column(&mut self, stats: ColumnStats) {
        self.column_name_to_id.insert(stats.column_name.clone(), stats.column_id);
        self.columns.insert(stats.column_id, stats);
    }

    /// Get statistics for a column by name
    pub fn get_column_stats(&self, col_name: &str) -> Option<&ColumnStats> {
        self.column_name_to_id
            .get(col_name)
            .and_then(|id| self.columns.get(id))
    }

    /// Get mutable statistics for a column by name
    pub fn get_column_stats_mut(&mut self, col_name: &str) -> Option<&mut ColumnStats> {
        if let Some(id) = self.column_name_to_id.get(col_name).copied() {
            self.columns.get_mut(&id)
        } else {
            None
        }
    }

    /// Average row size
    pub fn average_row_size_bytes(&self) -> u64 {
        if self.row_count == 0 { 0 } else {
            (self.total_size_bytes / self.row_count).max(1)
        }
    }

    /// Number of columns in table
    pub fn num_columns(&self) -> usize {
        self.columns.len()
    }

    /// List of all column names
    pub fn column_names(&self) -> Vec<&str> {
        self.column_name_to_id.keys().map(|s| s.as_str()).collect()
    }
}

/// Cost model for query execution
#[derive(Debug, Clone)]
pub struct CostModel {
    // I/O cost constants (cost units per unit of data)
    pub sequential_io_cost: f64,      // Per MB scanned sequentially
    pub random_io_cost: f64,           // Per MB scanned randomly
    pub network_io_cost: f64,          // Per MB sent over network
    
    // CPU cost constants (cost units per row)
    pub cpu_cost_per_row: f64,         // Per row processed
    pub comparison_cost: f64,          // Per comparison operation
    pub hash_cost: f64,                // Per hash computation
    
    // Memory cost constants
    pub memory_cost_per_mb: f64,       // Per MB of memory used
    
    // Thresholds
    pub memory_budget_mb: u64,         // Maximum memory to use
    pub sequential_scan_threshold: u64, // Row threshold before considering index
}

impl Default for CostModel {
    fn default() -> Self {
        CostModel {
            sequential_io_cost: 1.0,    // 1 unit per MB scanned
            random_io_cost: 10.0,       // 10x more expensive than sequential
            network_io_cost: 5.0,       // 5x more expensive than sequential
            cpu_cost_per_row: 0.01,     // Very cheap CPU relative to I/O
            comparison_cost: 0.001,     // Negligible
            hash_cost: 0.002,           // Negligible
            memory_cost_per_mb: 0.1,    // Relatively cheap
            memory_budget_mb: 1024,     // 1GB default
            sequential_scan_threshold: 1_000_000, // 1M rows
        }
    }
}

impl CostModel {
    /// Estimate cost of table scan
    pub fn cost_table_scan(&self, table: &TableStats, filtered_rows: u64) -> f64 {
        // Base I/O cost
        let io_mb = (table.total_size_bytes / 1_048_576).max(1) as f64;
        let io_cost = io_mb * self.sequential_io_cost;
        
        // CPU cost for processing rows
        let cpu_cost = (filtered_rows as f64) * self.cpu_cost_per_row;
        
        io_cost + cpu_cost
    }

    /// Estimate cost of index lookup
    pub fn cost_index_lookup(&self, num_lookups: u64) -> f64 {
        (num_lookups as f64) * self.random_io_cost
    }

    /// Estimate cost of hash join
    pub fn cost_hash_join(&self, left_rows: u64, right_rows: u64, join_keys: usize) -> f64 {
        // Build hash table: hash every row on right
        let build_cost = (right_rows as f64) * self.hash_cost * (join_keys as f64);
        
        // Probe: hash every row on left
        let probe_cost = (left_rows as f64) * self.hash_cost * (join_keys as f64);
        
        // CPU for comparison/output
        let output_cost = ((left_rows * right_rows) as f64 / 1000000.0).min(left_rows as f64 + right_rows as f64)
            * self.cpu_cost_per_row;
        
        build_cost + probe_cost + output_cost
    }

    /// Estimate cost of nested loop join
    pub fn cost_nested_loop_join(&self, left_rows: u64, right_rows: u64) -> f64 {
        // Scan right for each left row
        ((left_rows * right_rows) as f64) * self.cpu_cost_per_row
    }

    /// Estimate cost of merge join (assumes sorted data)
    pub fn cost_merge_join(&self, left_rows: u64, right_rows: u64) -> f64 {
        // Single pass through both sides
        ((left_rows + right_rows) as f64) * self.cpu_cost_per_row
    }

    /// Estimate cost of sort
    pub fn cost_sort(&self, rows: u64) -> f64 {
        let log_n = (rows as f64).log2().max(1.0);
        ((rows as f64) * log_n) * self.cpu_cost_per_row
    }

    /// Estimate cost of group by / aggregation
    pub fn cost_aggregate(&self, input_rows: u64, group_count: u64) -> f64 {
        // Hash each row
        let hash_cost = (input_rows as f64) * self.hash_cost;
        
        // Output group results
        let output_cost = (group_count as f64) * self.cpu_cost_per_row;
        
        hash_cost + output_cost
    }
}

/// Statistics collector for building table statistics
#[derive(Debug, Clone)]
pub struct StatisticsCollector {
    pub cost_model: CostModel,
}

impl StatisticsCollector {
    pub fn new() -> Self {
        StatisticsCollector {
            cost_model: CostModel::default(),
        }
    }

    pub fn with_cost_model(cost_model: CostModel) -> Self {
        StatisticsCollector { cost_model }
    }

    /// Create statistics for a table (from actual data scan)
    pub fn collect_table_statistics(
        &self,
        table_name: &str,
        rows: &[Vec<KVal>],
        column_names: &[String],
        column_types: &[KType],
    ) -> Result<TableStats, String> {
        if rows.is_empty() {
            return Ok(TableStats::new(table_name.to_string(), 0));
        }

        let mut table_stats = TableStats::new(table_name.to_string(), rows.len() as u64);
        let row_count = rows.len() as u64;

        // Process each column
        for (col_idx, col_name) in column_names.iter().enumerate() {
            let mut col_stats = ColumnStats::new(
                ColumnId(col_idx as u32),
                col_name.clone(),
                column_types[col_idx],
                row_count,
            );

            // Collect value statistics
            let mut values = Vec::new();
            let mut null_count = 0;
            let mut value_counts: BTreeMap<String, u64> = BTreeMap::new();

            for row in rows {
                if col_idx < row.len() {
                    match &row[col_idx] {
                        KVal::Null => null_count += 1,
                        v => {
                            values.push(v.clone());
                            let key = format!("{:?}", v);
                            *value_counts.entry(key).or_insert(0) += 1;
                        }
                    }
                }
            }

            col_stats.null_count = null_count;
            col_stats.distinct_count = value_counts.len() as u64;

            // Set min/max
            if !values.is_empty() {
                col_stats.min_value = Some(values[0].clone());
                col_stats.max_value = Some(values[values.len() - 1].clone());
            }

            // Build histogram (simple: equal-width buckets)
            if !values.is_empty() && col_stats.distinct_count > 0 {
                let bucket_count = ((col_stats.distinct_count as f64).sqrt() as u64).min(100).max(1);
                let mut buckets = Vec::new();
                let mut bucket_values: Vec<KVal> = value_counts.iter()
                    .flat_map(|(_, count)| std::iter::repeat(values[0].clone()).take(*count as usize))
                    .collect();
                bucket_values.sort_by(|a, b| {
                    match (a, b) {
                        (KVal::Int(av), KVal::Int(bv)) => av.cmp(bv),
                        (KVal::Float(av), KVal::Float(bv)) => {
                            if av < bv { Ordering::Less } else if av > bv { Ordering::Greater } else { Ordering::Equal }
                        }
                        _ => Ordering::Equal
                    }
                });

                for _ in 0..bucket_count.min(bucket_values.len() as u64) {
                    if !bucket_values.is_empty() {
                        buckets.push(HistogramBucket::new(
                            bucket_values[0].clone(),
                            bucket_values[bucket_values.len() - 1].clone(),
                            bucket_values.len() as u64,
                            value_counts.len() as u64,
                        ));
                    }
                }
                col_stats.histogram = buckets;
            }

            table_stats.add_column(col_stats);
        }

        table_stats.last_updated = chrono::Utc::now();
        Ok(table_stats)
    }

    /// Update statistics with sample data
    pub fn update_statistics_sample(
        &self,
        table_stats: &mut TableStats,
        sample_rows: &[Vec<KVal>],
        column_names: &[String],
    ) -> Result<(), String> {
        if sample_rows.is_empty() {
            return Ok(());
        }

        let total_row_count = table_stats.row_count; // Capture before mutable borrows

        for (col_idx, col_name) in column_names.iter().enumerate() {
            if let Some(col_stats) = table_stats.get_column_stats_mut(col_name) {
                // Simple update: count nulls in sample
                let mut new_null_count = 0;
                for row in sample_rows {
                    if col_idx < row.len() {
                        if matches!(row[col_idx], KVal::Null) {
                            new_null_count += 1;
                        }
                    }
                }

                // Extrapolate to full table
                let null_percentage = (new_null_count as f64) / (sample_rows.len() as f64);
                col_stats.null_count = ((total_row_count as f64) * null_percentage) as u64;
            }
        }

        table_stats.last_updated = chrono::Utc::now();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selectivity_creation() {
        let sel = Selectivity::new(0.1, 0.5, 0.3);
        assert_eq!(sel.lower_bound, 0.1);
        assert_eq!(sel.upper_bound, 0.5);
        assert_eq!(sel.best_estimate, 0.3);
    }

    #[test]
    fn test_column_stats_null_percentage() {
        let mut col_stats = ColumnStats::new(ColumnId(0), "test".to_string(), KType::Int, 100);
        col_stats.null_count = 10;
        assert_eq!(col_stats.null_percentage(), 0.1);
    }

    #[test]
    fn test_column_stats_uniqueness() {
        let mut col_stats = ColumnStats::new(ColumnId(0), "test".to_string(), KType::Int, 100);
        col_stats.distinct_count = 50;
        col_stats.null_count = 0;
        assert_eq!(col_stats.uniqueness(), 0.5);
    }

    #[test]
    fn test_table_stats_lookup() {
        let mut table = TableStats::new("test_table".to_string(), 1000);
        let mut col = ColumnStats::new(ColumnId(0), "col1".to_string(), KType::Int, 1000);
        col.distinct_count = 100;
        table.add_column(col);

        let retrieved = table.get_column_stats("col1");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().distinct_count, 100);
    }

    #[test]
    fn test_cost_model_defaults() {
        let model = CostModel::default();
        assert!(model.sequential_io_cost > 0.0);
        assert!(model.random_io_cost > model.sequential_io_cost);
    }

    #[test]
    fn test_cost_table_scan() {
        let model = CostModel::default();
        let mut table = TableStats::new("test".to_string(), 1000);
        table.total_size_bytes = 1024 * 1024; // 1 MB

        let cost = model.cost_table_scan(&table, 500);
        assert!(cost > 0.0);
    }

    #[test]
    fn test_equality_selectivity() {
        let mut col = ColumnStats::new(ColumnId(0), "test".to_string(), KType::Int, 100);
        col.distinct_count = 50;
        let sel = col.estimate_equality_selectivity(&KVal::Int(5));
        assert_eq!(sel, 1.0 / 50.0);
    }
}
