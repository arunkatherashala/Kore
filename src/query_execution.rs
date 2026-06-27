use crate::predicates::{QueryFilter, PredicateExpression, ColumnSelection};
use crate::statistics::{ColumnStatistics, TableStatistics, BlockMetadata};
use crate::caching::CachingLayer;

/// Query execution strategy
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionStrategy {
    /// Full table scan
    FullTableScan,
    /// Predicate pushdown - filter before decompression
    PredicatePushdown,
    /// Column pruning - skip unnecessary columns
    ColumnPruning,
    /// Combined optimization
    Combined,
    /// Block skipping - skip blocks outside range
    BlockSkipping,
    /// Cache hit - return from cache
    CacheHit,
}

/// Cost model for query execution
#[derive(Debug, Clone)]
pub struct QueryCost {
    pub io_cost: f64,           // I/O operations (bytes read)
    pub cpu_cost: f64,          // CPU operations (decompression, filtering)
    pub memory_cost: f64,       // Memory usage (temporary buffers)
    pub estimated_rows: u64,    // Estimated output rows
    pub cache_hit_probability: f64, // Likelihood of cache hit (0.0-1.0)
}

impl QueryCost {
    /// Create new cost estimate
    pub fn new(io_cost: f64, cpu_cost: f64, memory_cost: f64, estimated_rows: u64) -> Self {
        Self {
            io_cost,
            cpu_cost,
            memory_cost,
            estimated_rows,
            cache_hit_probability: 0.0,
        }
    }

    /// Set cache hit probability (0.0-1.0)
    pub fn with_cache_probability(mut self, prob: f64) -> Self {
        self.cache_hit_probability = prob.clamp(0.0, 1.0);
        self
    }

    /// Calculate total cost (weighted sum)
    pub fn total_cost(&self) -> f64 {
        // Weights: I/O is most expensive, then CPU, then memory
        (self.io_cost * 20.0) + (self.cpu_cost * 1.0) + (self.memory_cost * 0.05)
    }

    /// Apply cache hit reduction
    pub fn with_cache_reduction(&self) -> Self {
        let reduction_factor = 1.0 - self.cache_hit_probability;
        Self {
            io_cost: self.io_cost * reduction_factor,
            cpu_cost: self.cpu_cost * reduction_factor,
            memory_cost: self.memory_cost * reduction_factor,
            estimated_rows: self.estimated_rows,
            cache_hit_probability: self.cache_hit_probability,
        }
    }
}

/// Execution plan for a query
#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    pub strategy: ExecutionStrategy,
    pub columns_to_read: Vec<String>,
    pub predicates_to_push: Vec<String>,
    pub blocks_to_skip: Vec<u32>,
    pub estimated_cost: QueryCost,
    pub optimization_description: String,
}

impl ExecutionPlan {
    /// Create new execution plan
    pub fn new(strategy: ExecutionStrategy, estimated_cost: QueryCost) -> Self {
        Self {
            strategy: strategy.clone(),
            columns_to_read: Vec::new(),
            predicates_to_push: Vec::new(),
            blocks_to_skip: Vec::new(),
            estimated_cost,
            optimization_description: format!("Strategy: {:?}", strategy),
        }
    }

    /// Add columns to read
    pub fn with_columns(mut self, columns: Vec<String>) -> Self {
        self.columns_to_read = columns;
        self
    }

    /// Add predicates to push
    pub fn with_predicates(mut self, predicates: Vec<String>) -> Self {
        self.predicates_to_push = predicates;
        self
    }

    /// Add blocks to skip
    pub fn with_skipped_blocks(mut self, blocks: Vec<u32>) -> Self {
        self.blocks_to_skip = blocks;
        self
    }

    /// Set optimization description
    pub fn with_description(mut self, desc: String) -> Self {
        self.optimization_description = desc;
        self
    }

    /// Estimated speedup from optimizations
    pub fn speedup_factor(&self) -> f64 {
        // More optimization = higher speedup
        let mut factor = 1.0;

        if !self.columns_to_read.is_empty() {
            factor *= 2.0; // Column pruning typically gives 2-5x
        }

        if !self.predicates_to_push.is_empty() {
            factor *= 2.0; // Predicate pushdown typically gives 2-4x
        }

        if !self.blocks_to_skip.is_empty() {
            factor *= 1.5; // Block skipping gives 1.5-3x
        }

        factor
    }
}

/// Query planner
pub struct QueryPlanner {
    pub table_stats: Option<TableStatistics>,
    pub column_stats: Vec<ColumnStatistics>,
    pub cache_layer: Option<CachingLayer>,
}

impl QueryPlanner {
    /// Create new query planner
    pub fn new() -> Self {
        Self {
            table_stats: None,
            column_stats: Vec::new(),
            cache_layer: None,
        }
    }

    /// Register table statistics
    pub fn with_table_stats(mut self, stats: TableStatistics) -> Self {
        self.table_stats = Some(stats);
        self
    }

    /// Register column statistics
    pub fn with_column_stats(mut self, stats: Vec<ColumnStatistics>) -> Self {
        self.column_stats = stats;
        self
    }

    /// Register caching layer
    pub fn with_cache_layer(mut self, cache: CachingLayer) -> Self {
        self.cache_layer = Some(cache);
        self
    }

    /// Plan a query execution
    pub fn plan_query(&self, filter: &QueryFilter, total_rows: u64) -> ExecutionPlan {
        // Check cache first
        let cache_key = format!("{:?}", filter);
        if let Some(cache) = &self.cache_layer {
            if cache.get_query(&cache_key).is_some() {
                let cost = QueryCost::new(0.0, 0.0, 0.0, 0).with_cache_probability(1.0);
                return ExecutionPlan::new(ExecutionStrategy::CacheHit, cost)
                    .with_description("Query result found in cache".to_string());
            }
        }

        // Determine best strategy
        let mut plan = ExecutionPlan::new(ExecutionStrategy::FullTableScan, QueryCost::new(
            total_rows as f64 * 8.0, // 8 bytes per row estimate
            total_rows as f64 * 0.1, // CPU cost
            (total_rows as f64 * 8.0) * 0.1, // Memory cost
            total_rows,
        ));

        // Column pruning opportunity?
        let selected_cols = filter.column_selection.columns();
        if !selected_cols.is_empty() && selected_cols.len() < self.column_stats.len() {
            let col_count = self.column_stats.len();
            let selected_count = selected_cols.len();
            let io_reduction = 1.0 - (selected_count as f64 / col_count as f64);
            
            plan.strategy = ExecutionStrategy::ColumnPruning;
            plan.columns_to_read = selected_cols.iter().map(|s| s.to_string()).collect();
            plan.estimated_cost.io_cost *= (1.0 - io_reduction);
            plan.optimization_description = format!(
                "Column pruning: read {} of {} columns ({:.1}% I/O reduction)",
                selected_count, col_count, io_reduction * 100.0
            );
        }

        // Predicate pushdown opportunity?
        if !filter.predicates.is_empty() {
            if plan.strategy == ExecutionStrategy::ColumnPruning {
                plan.strategy = ExecutionStrategy::Combined;
                plan.optimization_description = format!(
                    "{}, with predicates pushed down",
                    plan.optimization_description
                );
            } else {
                plan.strategy = ExecutionStrategy::PredicatePushdown;
                plan.optimization_description = "Predicate pushdown: filter conditions applied".to_string();
            }

            // Estimate selectivity reduction
            let selectivity = self.estimate_selectivity(filter);
            let filtered_rows = (total_rows as f64 * selectivity) as u64;
            plan.estimated_cost.cpu_cost *= selectivity;
            plan.estimated_cost.estimated_rows = filtered_rows;
            plan.predicates_to_push = vec!["predicates".to_string()];
        }

        // Block skipping opportunity?
        if let Some(table_stats) = &self.table_stats {
            let skippable = table_stats.get_skippable_blocks("id", "0", "999999");
            if !skippable.is_empty() {
                plan.blocks_to_skip = (0..skippable.len() as u32).collect();
                plan.estimated_cost.io_cost *= 0.5; // Estimate 50% I/O reduction
            }
        }

        plan
    }

    /// Estimate query selectivity
    pub fn estimate_selectivity(&self, filter: &QueryFilter) -> f64 {
        if filter.predicates.is_empty() {
            return 1.0;
        }

        // Simple selectivity model: each predicate reduces by ~50%
        let reduction_per_predicate = 0.5_f64;
        let mut selectivity = 1.0_f64;

        for _ in 0..1 {
            selectivity *= reduction_per_predicate;
        }

        selectivity.max(0.01_f64) // At least 1% selectivity
    }

    /// Compare multiple execution strategies
    pub fn compare_strategies(&self, filter: &QueryFilter, total_rows: u64) -> Vec<(ExecutionStrategy, QueryCost)> {
        vec![
            (ExecutionStrategy::FullTableScan, QueryCost::new(
                total_rows as f64 * 8.0,
                total_rows as f64 * 0.1,
                (total_rows as f64 * 8.0) * 0.1,
                total_rows,
            )),
            (ExecutionStrategy::PredicatePushdown, QueryCost::new(
                total_rows as f64 * 8.0 * 0.5,
                total_rows as f64 * 0.1 * 0.3,
                (total_rows as f64 * 8.0) * 0.05,
                (total_rows as f64 * 0.5) as u64,
            )),
            (ExecutionStrategy::ColumnPruning, QueryCost::new(
                total_rows as f64 * 8.0 * 0.7,
                total_rows as f64 * 0.1 * 0.8,
                (total_rows as f64 * 8.0) * 0.1,
                total_rows,
            )),
            (ExecutionStrategy::Combined, QueryCost::new(
                total_rows as f64 * 8.0 * 0.35,
                total_rows as f64 * 0.1 * 0.15,
                (total_rows as f64 * 8.0) * 0.03,
                (total_rows as f64 * 0.5) as u64,
            )),
        ]
    }

    /// Get best execution strategy
    pub fn best_strategy(&self, filter: &QueryFilter, total_rows: u64) -> ExecutionStrategy {
        // If filter is empty, no optimization is possible
        if filter.is_empty() {
            return ExecutionStrategy::FullTableScan;
        }

        let strategies = self.compare_strategies(filter, total_rows);
        strategies
            .into_iter()
            .min_by(|a, b| {
                a.1.total_cost()
                    .partial_cmp(&b.1.total_cost())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(strategy, _)| strategy)
            .unwrap_or(ExecutionStrategy::FullTableScan)
    }
}

impl Default for QueryPlanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_cost_creation() {
        let cost = QueryCost::new(1000.0, 500.0, 200.0, 100);
        assert_eq!(cost.io_cost, 1000.0);
        assert_eq!(cost.cpu_cost, 500.0);
        assert_eq!(cost.memory_cost, 200.0);
        assert_eq!(cost.estimated_rows, 100);
    }

    #[test]
    fn test_query_cost_total() {
        let cost = QueryCost::new(1000.0, 500.0, 200.0, 100);
        let total = cost.total_cost();
        assert_eq!(total, 1000.0 * 20.0 + 500.0 * 1.0 + 200.0 * 0.05);
    }

    #[test]
    fn test_query_cost_cache_reduction() {
        let cost = QueryCost::new(1000.0, 500.0, 200.0, 100)
            .with_cache_probability(0.8);
        let reduced = cost.with_cache_reduction();
        assert!((reduced.io_cost - 200.0).abs() < 0.01); // 1000 * (1-0.8)
    }

    #[test]
    fn test_execution_plan_creation() {
        let cost = QueryCost::new(1000.0, 500.0, 200.0, 100);
        let plan = ExecutionPlan::new(ExecutionStrategy::FullTableScan, cost);
        assert_eq!(plan.strategy, ExecutionStrategy::FullTableScan);
        assert!(plan.columns_to_read.is_empty());
    }

    #[test]
    fn test_execution_plan_with_columns() {
        let cost = QueryCost::new(1000.0, 500.0, 200.0, 100);
        let plan = ExecutionPlan::new(ExecutionStrategy::ColumnPruning, cost)
            .with_columns(vec!["col1".to_string(), "col2".to_string()]);
        assert_eq!(plan.columns_to_read.len(), 2);
    }

    #[test]
    fn test_execution_plan_speedup_factor() {
        let cost = QueryCost::new(1000.0, 500.0, 200.0, 100);
        let plan = ExecutionPlan::new(ExecutionStrategy::Combined, cost)
            .with_columns(vec!["col1".to_string()])
            .with_predicates(vec!["pred1".to_string()])
            .with_skipped_blocks(vec![1, 2]);
        
        // 2.0 (columns) * 2.0 (predicates) * 1.5 (blocks) = 6.0
        assert!((plan.speedup_factor() - 6.0).abs() < 0.01);
    }

    #[test]
    fn test_query_planner_creation() {
        let planner = QueryPlanner::new();
        assert!(planner.table_stats.is_none());
        assert!(planner.column_stats.is_empty());
    }

    #[test]
    fn test_execution_strategy_comparison() {
        let full_scan = ExecutionStrategy::FullTableScan;
        let pushdown = ExecutionStrategy::PredicatePushdown;
        assert_ne!(full_scan, pushdown);
    }

    #[test]
    fn test_query_cost_weighted_calculation() {
        // Test weight distribution: I/O (20.0), CPU (1.0), Memory (0.05)
        let cost1 = QueryCost::new(100.0, 0.0, 0.0, 0);
        let cost2 = QueryCost::new(0.0, 100.0, 0.0, 0);
        let cost3 = QueryCost::new(0.0, 0.0, 100.0, 0);

        assert!((cost1.total_cost() - 2000.0).abs() < 0.01);
        assert!((cost2.total_cost() - 100.0).abs() < 0.01);
        assert!((cost3.total_cost() - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_query_cost_cache_probability_clamp() {
        let cost1 = QueryCost::new(1000.0, 500.0, 200.0, 100)
            .with_cache_probability(1.5); // Should clamp to 1.0
        assert_eq!(cost1.cache_hit_probability, 1.0);

        let cost2 = QueryCost::new(1000.0, 500.0, 200.0, 100)
            .with_cache_probability(-0.5); // Should clamp to 0.0
        assert_eq!(cost2.cache_hit_probability, 0.0);
    }

    #[test]
    fn test_execution_plan_description() {
        let cost = QueryCost::new(1000.0, 500.0, 200.0, 100);
        let plan = ExecutionPlan::new(ExecutionStrategy::FullTableScan, cost)
            .with_description("Custom description".to_string());
        assert_eq!(plan.optimization_description, "Custom description");
    }

    #[test]
    fn test_query_planner_empty_filter() {
        let planner = QueryPlanner::new();
        let filter = QueryFilter::default();
        let plan = planner.plan_query(&filter, 1000);
        assert_eq!(plan.strategy, ExecutionStrategy::FullTableScan);
    }

    #[test]
    fn test_query_planner_selectivity_estimation() {
        let planner = QueryPlanner::new();
        let filter = QueryFilter::default();
        
        let selectivity = planner.estimate_selectivity(&filter);
        assert_eq!(selectivity, 1.0); // No predicates = full selectivity
    }

    #[test]
    fn test_query_cost_estimated_rows() {
        let cost = QueryCost::new(1000.0, 500.0, 200.0, 5000);
        assert_eq!(cost.estimated_rows, 5000);
    }

    #[test]
    fn test_execution_strategy_full_table_scan() {
        let strategy = ExecutionStrategy::FullTableScan;
        assert_eq!(strategy, ExecutionStrategy::FullTableScan);
    }

    #[test]
    fn test_execution_strategy_predicate_pushdown() {
        let strategy = ExecutionStrategy::PredicatePushdown;
        assert_eq!(strategy, ExecutionStrategy::PredicatePushdown);
    }

    #[test]
    fn test_execution_strategy_column_pruning() {
        let strategy = ExecutionStrategy::ColumnPruning;
        assert_eq!(strategy, ExecutionStrategy::ColumnPruning);
    }

    #[test]
    fn test_execution_strategy_combined() {
        let strategy = ExecutionStrategy::Combined;
        assert_eq!(strategy, ExecutionStrategy::Combined);
    }

    #[test]
    fn test_execution_strategy_block_skipping() {
        let strategy = ExecutionStrategy::BlockSkipping;
        assert_eq!(strategy, ExecutionStrategy::BlockSkipping);
    }

    #[test]
    fn test_execution_strategy_cache_hit() {
        let strategy = ExecutionStrategy::CacheHit;
        assert_eq!(strategy, ExecutionStrategy::CacheHit);
    }

    #[test]
    fn test_query_planner_best_strategy() {
        let planner = QueryPlanner::new();
        let filter = QueryFilter::default();
        let best = planner.best_strategy(&filter, 1000);
        // Empty filter should choose FullTableScan
        assert_eq!(best, ExecutionStrategy::FullTableScan);
    }

    #[test]
    fn test_query_planner_compare_strategies() {
        let planner = QueryPlanner::new();
        let filter = QueryFilter::default();
        let strategies = planner.compare_strategies(&filter, 1000);
        assert!(strategies.len() >= 4); // At least 4 strategies
    }

    #[test]
    fn test_query_cost_cache_reduction_zero_probability() {
        let cost = QueryCost::new(1000.0, 500.0, 200.0, 100)
            .with_cache_probability(0.0);
        let reduced = cost.with_cache_reduction();
        assert_eq!(reduced.io_cost, 1000.0); // No reduction
    }

    #[test]
    fn test_execution_plan_with_predicates() {
        let cost = QueryCost::new(1000.0, 500.0, 200.0, 100);
        let plan = ExecutionPlan::new(ExecutionStrategy::PredicatePushdown, cost)
            .with_predicates(vec!["age > 25".to_string()]);
        assert_eq!(plan.predicates_to_push.len(), 1);
    }

    #[test]
    fn test_execution_plan_with_skipped_blocks() {
        let cost = QueryCost::new(1000.0, 500.0, 200.0, 100);
        let plan = ExecutionPlan::new(ExecutionStrategy::BlockSkipping, cost)
            .with_skipped_blocks(vec![1, 2, 3]);
        assert_eq!(plan.blocks_to_skip.len(), 3);
    }

    #[test]
    fn test_query_planner_with_cache_layer() {
        let cache = CachingLayer::new();
        let planner = QueryPlanner::new().with_cache_layer(cache);
        assert!(planner.cache_layer.is_some());
    }
}
