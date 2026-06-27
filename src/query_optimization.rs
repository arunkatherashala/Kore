//! Query Optimization Module for Phase 3.1
//!
//! Implements predicate pushdown and column pruning optimization strategies.
//! Calculates performance improvements and optimal query plans.

use std::collections::HashMap;

/// Statistics about query optimization results
#[derive(Debug, Clone)]
pub struct OptimizationStats {
    /// Original data to be read (bytes)
    pub original_bytes: u64,
    /// Data that will actually be read after optimization (bytes)
    pub optimized_bytes: u64,
    /// Number of columns in original query
    pub original_columns: usize,
    /// Number of columns after pruning
    pub optimized_columns: usize,
    /// Estimated rows before filtering
    pub estimated_input_rows: u64,
    /// Estimated rows after filtering
    pub estimated_output_rows: u64,
}

impl OptimizationStats {
    /// Create new optimization stats
    pub fn new(
        original_bytes: u64,
        optimized_bytes: u64,
        original_columns: usize,
        optimized_columns: usize,
        estimated_input_rows: u64,
        estimated_output_rows: u64,
    ) -> Self {
        OptimizationStats {
            original_bytes,
            optimized_bytes,
            original_columns,
            optimized_columns,
            estimated_input_rows,
            estimated_output_rows,
        }
    }

    /// Calculate bytes saved
    pub fn bytes_saved(&self) -> u64 {
        self.original_bytes - self.optimized_bytes
    }

    /// Calculate percentage reduction in data to read
    pub fn data_reduction_percent(&self) -> f64 {
        if self.original_bytes == 0 {
            return 0.0;
        }
        (self.bytes_saved() as f64 / self.original_bytes as f64) * 100.0
    }

    /// Calculate estimated speedup factor from optimization
    pub fn estimated_speedup(&self) -> f64 {
        if self.original_bytes == 0 || self.optimized_bytes == 0 {
            return 1.0;
        }
        self.original_bytes as f64 / self.optimized_bytes as f64
    }

    /// Calculate rows eliminated by filtering
    pub fn rows_eliminated(&self) -> u64 {
        self.estimated_input_rows.saturating_sub(self.estimated_output_rows)
    }

    /// Calculate filtering selectivity (0.0 = filters out all, 1.0 = no filtering)
    pub fn selectivity(&self) -> f64 {
        if self.estimated_input_rows == 0 {
            return 1.0;
        }
        self.estimated_output_rows as f64 / self.estimated_input_rows as f64
    }
}

/// Query optimization plan
#[derive(Debug, Clone)]
pub struct OptimizationPlan {
    pub stats: OptimizationStats,
    /// Which columns to read
    pub columns_to_read: Vec<String>,
    /// Predicates to push down
    pub predicates: Vec<String>,
    /// Optimization techniques applied
    pub techniques_applied: Vec<String>,
}

impl OptimizationPlan {
    /// Create a new optimization plan
    pub fn new(
        stats: OptimizationStats,
        columns_to_read: Vec<String>,
        predicates: Vec<String>,
    ) -> Self {
        let mut techniques = Vec::new();

        if stats.original_columns > stats.optimized_columns {
            techniques.push("Column Pruning".to_string());
        }

        if stats.estimated_output_rows < stats.estimated_input_rows {
            techniques.push("Predicate Pushdown".to_string());
        }

        if stats.original_bytes > stats.optimized_bytes {
            techniques.push("I/O Reduction".to_string());
        }

        OptimizationPlan {
            stats,
            columns_to_read,
            predicates,
            techniques_applied: techniques,
        }
    }

    /// Generate a summary of the optimization plan
    pub fn summary(&self) -> String {
        format!(
            "Optimization Plan:\n  Columns: {} -> {} ({}% reduction)\n  Data: {} -> {} bytes ({}% reduction)\n  Speedup: {:.1}x\n  Techniques: {:?}",
            self.stats.original_columns,
            self.stats.optimized_columns,
            (1.0 - (self.stats.optimized_columns as f64 / self.stats.original_columns as f64)) * 100.0,
            self.stats.original_bytes,
            self.stats.optimized_bytes,
            self.stats.data_reduction_percent() as u32,
            self.stats.estimated_speedup(),
            self.techniques_applied
        )
    }
}

/// Query optimizer - main entry point for optimization
pub struct QueryOptimizer {
    /// Estimated bytes per column
    column_sizes: HashMap<String, u64>,
    /// Estimated selectivity for different predicates
    predicate_selectivity: HashMap<String, f64>,
}

impl QueryOptimizer {
    /// Create a new query optimizer
    pub fn new() -> Self {
        QueryOptimizer {
            column_sizes: HashMap::new(),
            predicate_selectivity: HashMap::new(),
        }
    }

    /// Register estimated size for a column
    pub fn register_column_size(&mut self, column_name: String, bytes: u64) {
        self.column_sizes.insert(column_name, bytes);
    }

    /// Register estimated selectivity for a predicate
    pub fn register_predicate_selectivity(&mut self, predicate: String, selectivity: f64) {
        self.predicate_selectivity.insert(predicate, selectivity);
    }

    /// Calculate bytes for a set of columns
    pub fn calculate_column_bytes(&self, columns: &[String]) -> u64 {
        columns.iter().map(|c| self.column_sizes.get(c).copied().unwrap_or(0)).sum()
    }

    /// Calculate estimated output rows after filtering
    pub fn calculate_filtered_rows(
        &self,
        input_rows: u64,
        predicates: &[String],
    ) -> u64 {
        let mut selectivity = 1.0;
        for pred in predicates {
            if let Some(pred_selectivity) = self.predicate_selectivity.get(pred) {
                selectivity *= pred_selectivity;
            }
        }
        (input_rows as f64 * selectivity) as u64
    }

    /// Generate optimization plan
    pub fn optimize(
        &self,
        all_columns: &[String],
        selected_columns: &[String],
        predicates: &[String],
        total_rows: u64,
    ) -> OptimizationPlan {
        let original_bytes = self.calculate_column_bytes(all_columns);
        let optimized_bytes = self.calculate_column_bytes(selected_columns);
        let filtered_rows = self.calculate_filtered_rows(total_rows, predicates);

        let stats = OptimizationStats::new(
            original_bytes,
            optimized_bytes,
            all_columns.len(),
            selected_columns.len(),
            total_rows,
            filtered_rows,
        );

        OptimizationPlan::new(
            stats,
            selected_columns.to_vec(),
            predicates.to_vec(),
        )
    }
}

impl Default for QueryOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimization_stats_bytes_saved() {
        let stats = OptimizationStats::new(1000, 400, 10, 4, 10000, 10000);
        assert_eq!(stats.bytes_saved(), 600);
    }

    #[test]
    fn test_optimization_stats_data_reduction_percent() {
        let stats = OptimizationStats::new(1000, 400, 10, 4, 10000, 10000);
        assert_eq!(stats.data_reduction_percent(), 60.0);
    }

    #[test]
    fn test_optimization_stats_estimated_speedup() {
        let stats = OptimizationStats::new(1000, 400, 10, 4, 10000, 10000);
        assert!((stats.estimated_speedup() - 2.5).abs() < 0.01);
    }

    #[test]
    fn test_optimization_stats_rows_eliminated() {
        let stats = OptimizationStats::new(1000, 1000, 10, 10, 10000, 5000);
        assert_eq!(stats.rows_eliminated(), 5000);
    }

    #[test]
    fn test_optimization_stats_selectivity() {
        let stats = OptimizationStats::new(1000, 1000, 10, 10, 10000, 5000);
        assert!((stats.selectivity() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_optimization_plan_column_pruning_detection() {
        let stats = OptimizationStats::new(1000, 400, 10, 4, 10000, 10000);
        let plan = OptimizationPlan::new(stats, vec!["a".to_string()], vec![]);
        assert!(plan.techniques_applied.contains(&"Column Pruning".to_string()));
    }

    #[test]
    fn test_optimization_plan_predicate_pushdown_detection() {
        let stats = OptimizationStats::new(1000, 1000, 10, 10, 10000, 5000);
        let plan = OptimizationPlan::new(stats, vec![], vec!["age > 30".to_string()]);
        assert!(plan.techniques_applied.contains(&"Predicate Pushdown".to_string()));
    }

    #[test]
    fn test_optimization_plan_io_reduction_detection() {
        let stats = OptimizationStats::new(1000, 400, 10, 4, 10000, 10000);
        let plan = OptimizationPlan::new(stats, vec![], vec![]);
        assert!(plan.techniques_applied.contains(&"I/O Reduction".to_string()));
    }

    #[test]
    fn test_query_optimizer_column_size_registration() {
        let mut optimizer = QueryOptimizer::new();
        optimizer.register_column_size("id".to_string(), 100);
        optimizer.register_column_size("name".to_string(), 200);

        let bytes = optimizer.calculate_column_bytes(&["id".to_string(), "name".to_string()]);
        assert_eq!(bytes, 300);
    }

    #[test]
    fn test_query_optimizer_calculate_column_bytes() {
        let mut optimizer = QueryOptimizer::new();
        optimizer.register_column_size("col1".to_string(), 100);
        optimizer.register_column_size("col2".to_string(), 200);
        optimizer.register_column_size("col3".to_string(), 150);

        assert_eq!(
            optimizer.calculate_column_bytes(&["col1".to_string(), "col2".to_string()]),
            300
        );
    }

    #[test]
    fn test_query_optimizer_predicate_selectivity() {
        let mut optimizer = QueryOptimizer::new();
        optimizer.register_predicate_selectivity("age > 30".to_string(), 0.7);
        optimizer.register_predicate_selectivity("score >= 80".to_string(), 0.5);

        let filtered = optimizer.calculate_filtered_rows(
            1000,
            &["age > 30".to_string(), "score >= 80".to_string()],
        );
        assert_eq!(filtered, 350);
    }

    #[test]
    fn test_query_optimizer_generate_plan_column_pruning() {
        let mut optimizer = QueryOptimizer::new();
        optimizer.register_column_size("id".to_string(), 100);
        optimizer.register_column_size("name".to_string(), 200);
        optimizer.register_column_size("age".to_string(), 100);

        let plan = optimizer.optimize(
            &["id".to_string(), "name".to_string(), "age".to_string()],
            &["id".to_string(), "name".to_string()],
            &[],
            1000,
        );

        assert_eq!(plan.stats.original_columns, 3);
        assert_eq!(plan.stats.optimized_columns, 2);
        assert!(plan.techniques_applied.contains(&"Column Pruning".to_string()));
    }

    #[test]
    fn test_query_optimizer_generate_plan_predicate_pushdown() {
        let mut optimizer = QueryOptimizer::new();
        optimizer.register_column_size("id".to_string(), 100);
        optimizer.register_column_size("name".to_string(), 200);
        optimizer.register_predicate_selectivity("age > 30".to_string(), 0.5);

        let plan = optimizer.optimize(
            &["id".to_string(), "name".to_string()],
            &["id".to_string(), "name".to_string()],
            &["age > 30".to_string()],
            1000,
        );

        assert!(plan.techniques_applied.contains(&"Predicate Pushdown".to_string()));
        assert_eq!(plan.stats.estimated_output_rows, 500);
    }

    #[test]
    fn test_query_optimizer_combined_optimization() {
        let mut optimizer = QueryOptimizer::new();
        optimizer.register_column_size("id".to_string(), 100);
        optimizer.register_column_size("name".to_string(), 200);
        optimizer.register_column_size("age".to_string(), 100);
        optimizer.register_column_size("email".to_string(), 150);
        optimizer.register_predicate_selectivity("age > 30".to_string(), 0.7);

        let plan = optimizer.optimize(
            &[
                "id".to_string(),
                "name".to_string(),
                "age".to_string(),
                "email".to_string(),
            ],
            &["id".to_string(), "name".to_string(), "age".to_string()],
            &["age > 30".to_string()],
            1000,
        );

        assert!(plan.techniques_applied.contains(&"Column Pruning".to_string()));
        assert!(plan.techniques_applied.contains(&"Predicate Pushdown".to_string()));
        assert_eq!(plan.stats.estimated_output_rows, 700);
    }

    #[test]
    fn test_query_optimizer_plan_summary() {
        let mut optimizer = QueryOptimizer::new();
        optimizer.register_column_size("id".to_string(), 100);
        optimizer.register_column_size("name".to_string(), 200);

        let plan = optimizer.optimize(
            &["id".to_string(), "name".to_string()],
            &["id".to_string()],
            &[],
            1000,
        );

        let summary = plan.summary();
        assert!(summary.contains("Optimization Plan"));
        assert!(summary.contains("Column Pruning"));
    }

    #[test]
    fn test_optimization_stats_zero_division_protection() {
        let stats = OptimizationStats::new(0, 0, 0, 0, 0, 0);
        assert_eq!(stats.data_reduction_percent(), 0.0);
        assert_eq!(stats.estimated_speedup(), 1.0);
        assert_eq!(stats.selectivity(), 1.0);
    }
}
