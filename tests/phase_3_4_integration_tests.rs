use kore_fileformat::query_execution::*;
use kore_fileformat::predicates::{QueryFilter, PredicateExpression, ColumnSelection};
use kore_fileformat::statistics::ColumnStatistics;

/// Test execution strategy determination
#[test]
fn test_execution_strategy_full_table_scan_creation() {
    let strategy = ExecutionStrategy::FullTableScan;
    assert_eq!(strategy, ExecutionStrategy::FullTableScan);
}

/// Test predicate pushdown strategy
#[test]
fn test_execution_strategy_predicate_pushdown_selection() {
    let planner = QueryPlanner::new();
    let filter = QueryFilter::default();
    let plan = planner.plan_query(&filter, 10000);
    assert_eq!(plan.strategy, ExecutionStrategy::FullTableScan);
}

/// Test column pruning impact on I/O
#[test]
fn test_column_pruning_reduces_io_cost() {
    let cost_full = QueryCost::new(1000.0, 500.0, 200.0, 100);
    let cost_pruned = QueryCost::new(500.0, 500.0, 200.0, 100);
    
    assert!(cost_pruned.total_cost() < cost_full.total_cost());
}

/// Test predicate pushdown reduces CPU cost
#[test]
fn test_predicate_pushdown_reduces_cpu_cost() {
    let cost_no_push = QueryCost::new(1000.0, 500.0, 200.0, 10000);
    let cost_pushed = QueryCost::new(1000.0, 150.0, 200.0, 5000);
    
    assert!(cost_pushed.cpu_cost < cost_no_push.cpu_cost);
}

/// Test combined optimization (column + predicate)
#[test]
fn test_combined_optimization_strategy() {
    let cost = QueryCost::new(500.0, 150.0, 50.0, 5000);
    let plan = ExecutionPlan::new(ExecutionStrategy::Combined, cost)
        .with_columns(vec!["col1".to_string(), "col2".to_string()])
        .with_predicates(vec!["pred1".to_string()]);
    
    assert_eq!(plan.strategy, ExecutionStrategy::Combined);
    assert!(!plan.columns_to_read.is_empty());
    assert!(!plan.predicates_to_push.is_empty());
}

/// Test query cost total calculation with weights
#[test]
fn test_query_cost_weighted_total() {
    // Total = IO * 1.0 + CPU * 0.5 + Memory * 0.1
    let cost = QueryCost::new(100.0, 200.0, 300.0, 100);
    let total = cost.total_cost();
    let expected = 100.0 * 20.0 + 200.0 * 1.0 + 300.0 * 0.05;
    assert!((total - expected).abs() < 0.01);
}

/// Test speedup factor calculation
#[test]
fn test_execution_plan_speedup_multiplication() {
    let cost = QueryCost::new(1000.0, 500.0, 200.0, 100);
    let plan = ExecutionPlan::new(ExecutionStrategy::Combined, cost)
        .with_columns(vec!["col1".to_string()])
        .with_predicates(vec!["pred1".to_string()])
        .with_skipped_blocks(vec![1, 2, 3]);
    
    // 2.0 * 2.0 * 1.5 = 6.0
    assert!((plan.speedup_factor() - 6.0).abs() < 0.01);
}

/// Test block skipping for range queries
#[test]
fn test_block_skipping_strategy() {
    let cost = QueryCost::new(1000.0, 500.0, 200.0, 10000);
    let plan = ExecutionPlan::new(ExecutionStrategy::BlockSkipping, cost)
        .with_skipped_blocks(vec![0, 1, 2, 3, 4]);
    
    assert!(!plan.blocks_to_skip.is_empty());
    assert_eq!(plan.blocks_to_skip.len(), 5);
}

/// Test cache hit detection
#[test]
fn test_cache_hit_strategy() {
    let cost = QueryCost::new(0.0, 0.0, 0.0, 100).with_cache_probability(1.0);
    let plan = ExecutionPlan::new(ExecutionStrategy::CacheHit, cost)
        .with_description("Query found in cache".to_string());
    
    assert_eq!(plan.strategy, ExecutionStrategy::CacheHit);
    assert!(plan.optimization_description.contains("cache"));
}

/// Test query planner with multiple column stats
#[test]
fn test_query_planner_with_statistics() {
    let mut planner = QueryPlanner::new();
    
    let col1 = ColumnStatistics {
        name: "id".to_string(),
        data_type: "Int64".to_string(),
        row_count: 10000,
        null_count: 0,
        distinct_count: 10000,
        min_value: Some("1".to_string()),
        max_value: Some("10000".to_string()),
        avg_length: 8.0,
        compression_ratio: 1.0,
    };
    
    planner = planner.with_column_stats(vec![col1]);
    assert!(!planner.column_stats.is_empty());
}

/// Test strategy comparison returns multiple options
#[test]
fn test_query_planner_strategy_comparison() {
    let planner = QueryPlanner::new();
    let filter = QueryFilter::default();
    let strategies = planner.compare_strategies(&filter, 5000);
    
    assert!(strategies.len() >= 4); // At least full scan, pushdown, pruning, combined
}

/// Test best strategy selection
#[test]
fn test_query_planner_best_strategy_selection() {
    let planner = QueryPlanner::new();
    let filter = QueryFilter::default();
    let best = planner.best_strategy(&filter, 5000);
    
    // Should select a valid strategy
    assert_ne!(best, ExecutionStrategy::CacheHit);
}

/// Test selectivity estimation with predicates
#[test]
fn test_query_planner_selectivity_with_predicates() {
    let planner = QueryPlanner::new();
    let filter = QueryFilter::default();
    let selectivity = planner.estimate_selectivity(&filter);
    
    // No predicates = full selectivity
    assert_eq!(selectivity, 1.0);
}

/// Test cost reduction with cache probability
#[test]
fn test_query_cost_cache_reduction() {
    let cost = QueryCost::new(1000.0, 500.0, 200.0, 100)
        .with_cache_probability(0.5);
    let reduced = cost.with_cache_reduction();
    
    // 50% reduction for 50% cache probability
    assert!((reduced.io_cost - 500.0).abs() < 0.01);
}

/// Test execution plan with all optimizations
#[test]
fn test_execution_plan_full_optimization() {
    let cost = QueryCost::new(1000.0, 500.0, 200.0, 10000);
    let plan = ExecutionPlan::new(ExecutionStrategy::Combined, cost)
        .with_columns(vec!["col1".to_string(), "col2".to_string()])
        .with_predicates(vec!["pred1".to_string(), "pred2".to_string()])
        .with_skipped_blocks(vec![1, 2, 3, 4, 5])
        .with_description("Full optimization applied".to_string());
    
    assert_eq!(plan.columns_to_read.len(), 2);
    assert_eq!(plan.predicates_to_push.len(), 2);
    assert_eq!(plan.blocks_to_skip.len(), 5);
}

/// Test query planner cache layer integration
#[test]
fn test_query_planner_with_cache_integration() {
    let cache = kore_fileformat::caching::CachingLayer::new();
    let planner = QueryPlanner::new()
        .with_cache_layer(cache);
    
    assert!(planner.cache_layer.is_some());
}

/// Test cost model I/O dominance
#[test]
fn test_cost_model_io_dominance_over_cpu() {
    let io_cost = QueryCost::new(100.0, 100.0, 0.0, 100);
    let cpu_cost = QueryCost::new(0.0, 1000.0, 0.0, 100);
    
    // I/O cost should dominate even with much lower value
    assert!(io_cost.total_cost() > cpu_cost.total_cost());
}

/// Test cost model CPU vs memory
#[test]
fn test_cost_model_cpu_dominance_over_memory() {
    let cpu_cost = QueryCost::new(0.0, 100.0, 0.0, 100);
    let mem_cost = QueryCost::new(0.0, 0.0, 1000.0, 100);
    
    // CPU should dominate memory
    assert!(cpu_cost.total_cost() > mem_cost.total_cost());
}

/// Test estimated rows calculation
#[test]
fn test_execution_plan_estimated_rows() {
    let cost = QueryCost::new(1000.0, 500.0, 200.0, 5000);
    let plan = ExecutionPlan::new(ExecutionStrategy::FullTableScan, cost);
    
    assert_eq!(plan.estimated_cost.estimated_rows, 5000);
}

/// Test multiple execution strategies comparison
#[test]
fn test_strategy_cost_comparison() {
    let planner = QueryPlanner::new();
    let filter = QueryFilter::default();
    let strategies = planner.compare_strategies(&filter, 10000);
    
    // Combined should be cheapest
    let mut min_cost = f64::MAX;
    let mut best_strategy = ExecutionStrategy::FullTableScan;
    
    for (strategy, cost) in strategies {
        if cost.total_cost() < min_cost {
            min_cost = cost.total_cost();
            best_strategy = strategy;
        }
    }
    
    assert_eq!(best_strategy, ExecutionStrategy::Combined);
}

/// Test execution strategy equality
#[test]
fn test_execution_strategy_equality() {
    let s1 = ExecutionStrategy::FullTableScan;
    let s2 = ExecutionStrategy::FullTableScan;
    let s3 = ExecutionStrategy::PredicatePushdown;
    
    assert_eq!(s1, s2);
    assert_ne!(s1, s3);
}

/// Test column pruning without predicates
#[test]
fn test_column_pruning_only_optimization() {
    let cost = QueryCost::new(700.0, 500.0, 200.0, 10000);
    let plan = ExecutionPlan::new(ExecutionStrategy::ColumnPruning, cost)
        .with_columns(vec!["id".to_string(), "name".to_string()]);
    
    assert_eq!(plan.strategy, ExecutionStrategy::ColumnPruning);
    assert!(!plan.predicates_to_push.is_empty() == false || plan.predicates_to_push.is_empty());
}

/// Test predicate pushdown only optimization
#[test]
fn test_predicate_pushdown_only_optimization() {
    let cost = QueryCost::new(1000.0, 250.0, 200.0, 5000);
    let plan = ExecutionPlan::new(ExecutionStrategy::PredicatePushdown, cost)
        .with_predicates(vec!["age > 18".to_string()]);
    
    assert_eq!(plan.strategy, ExecutionStrategy::PredicatePushdown);
    assert_eq!(plan.predicates_to_push.len(), 1);
}

/// Test query planner default creation
#[test]
fn test_query_planner_default() {
    let planner = QueryPlanner::default();
    assert!(planner.table_stats.is_none());
    assert!(planner.column_stats.is_empty());
    assert!(planner.cache_layer.is_none());
}

/// Test execution cost with very large table
#[test]
fn test_execution_cost_large_table() {
    let large_rows = 1_000_000_000u64;
    let cost = QueryCost::new(
        large_rows as f64 * 8.0,
        large_rows as f64 * 0.1,
        (large_rows as f64 * 8.0) * 0.1,
        large_rows,
    );
    
    assert_eq!(cost.estimated_rows, large_rows);
    assert!(cost.total_cost() > 1_000_000.0);
}

/// Test plan speedup with no optimizations
#[test]
fn test_execution_plan_speedup_no_optimizations() {
    let cost = QueryCost::new(1000.0, 500.0, 200.0, 100);
    let plan = ExecutionPlan::new(ExecutionStrategy::FullTableScan, cost);
    
    assert_eq!(plan.speedup_factor(), 1.0);
}

/// Test plan speedup with single optimization
#[test]
fn test_execution_plan_speedup_single_optimization() {
    let cost = QueryCost::new(1000.0, 500.0, 200.0, 100);
    let plan = ExecutionPlan::new(ExecutionStrategy::ColumnPruning, cost)
        .with_columns(vec!["col1".to_string()]);
    
    assert_eq!(plan.speedup_factor(), 2.0);
}

/// Test plan speedup with double optimization
#[test]
fn test_execution_plan_speedup_double_optimization() {
    let cost = QueryCost::new(1000.0, 500.0, 200.0, 100);
    let plan = ExecutionPlan::new(ExecutionStrategy::Combined, cost)
        .with_columns(vec!["col1".to_string()])
        .with_predicates(vec!["pred1".to_string()]);
    
    assert_eq!(plan.speedup_factor(), 4.0);
}
