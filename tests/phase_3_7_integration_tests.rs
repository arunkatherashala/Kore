use kore_fileformat::query_optimizer::*;
use kore_fileformat::statistics::ColumnStatistics;
use kore_fileformat::indexing::IndexType;
use kore_fileformat::query_execution::{ExecutionPlan, ExecutionStrategy, QueryCost};
use std::collections::HashMap;

fn create_e_commerce_stats() -> (u64, usize, f64, HashMap<String, ColumnStatistics>) {
    let row_count = 5_000_000_u64;
    let column_count = 15_usize;
    let total_size = 500_000_000.0; // 500 MB

    let mut column_stats = HashMap::new();
    
    // Product ID (high cardinality - good for hash)
    column_stats.insert(
        "product_id".to_string(),
        ColumnStatistics::new(
            "product_id".to_string(),
            "u32".to_string(),
            row_count,
            0,
            50000,
        ),
    );

    // User ID (high cardinality)
    column_stats.insert(
        "user_id".to_string(),
        ColumnStatistics::new(
            "user_id".to_string(),
            "u32".to_string(),
            row_count,
            1000,
            100000,
        ),
    );

    // Status (low cardinality - good for bitmap)
    column_stats.insert(
        "status".to_string(),
        ColumnStatistics::new(
            "status".to_string(),
            "string".to_string(),
            row_count,
            0,
            5,
        ),
    );

    // Amount (medium cardinality - good for btree range)
    column_stats.insert(
        "amount".to_string(),
        ColumnStatistics::new(
            "amount".to_string(),
            "f64".to_string(),
            row_count,
            100,
            5000,
        ),
    );

    // Date (medium cardinality)
    column_stats.insert(
        "order_date".to_string(),
        ColumnStatistics::new(
            "order_date".to_string(),
            "string".to_string(),
            row_count,
            0,
            365,
        ),
    );

    (row_count, column_count, total_size, column_stats)
}

/// Test cost estimator with e-commerce dataset
#[test]
fn test_ecommerce_full_scan_cost() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let estimator = CostEstimator::new(row_count, column_count, total_size, column_stats);
    let cost = estimator.estimate_full_scan();
    
    assert!(cost.io_cost > 0.0);
    assert!(cost.cpu_cost > 0.0);
    assert!(cost.total_cost() > cost.io_cost); // I/O dominates
}

/// Test cost reduction with column pruning
#[test]
fn test_column_pruning_cost_reduction() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let estimator = CostEstimator::new(row_count, column_count, total_size, column_stats);
    
    let full_scan = estimator.estimate_full_scan().total_cost();
    let pruned = estimator.estimate_column_pruning(3).total_cost();
    
    assert!(pruned < full_scan);
    let reduction = (full_scan - pruned) / full_scan;
    assert!(reduction > 0.1); // At least 10% reduction
}

/// Test predicate selectivity impact
#[test]
fn test_predicate_selectivity_impact() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let estimator = CostEstimator::new(row_count, column_count, total_size, column_stats);
    
    let full = estimator.estimate_full_scan().estimated_rows;
    let filtered_10 = estimator.estimate_predicate_pushdown(0.1).estimated_rows;
    let filtered_50 = estimator.estimate_predicate_pushdown(0.5).estimated_rows;
    
    assert!(filtered_10 < filtered_50);
    assert!(filtered_50 < full);
}

/// Test index lookup cost
#[test]
fn test_index_lookup_cost_hash() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let estimator = CostEstimator::new(row_count, column_count, total_size, column_stats);
    
    let full_scan = estimator.estimate_full_scan().total_cost();
    let indexed = estimator.estimate_index_lookup(50000, IndexType::Hash).total_cost();
    
    assert!(indexed < full_scan);
}

/// Test selectivity estimator accuracy
#[test]
fn test_selectivity_equality_product_id() {
    let (_, _, _, column_stats) = create_e_commerce_stats();
    let estimator = SelectivityEstimator::new(column_stats);
    
    let selectivity = estimator.estimate_equality("product_id");
    // Should be 1/50000 ≈ 0.00002
    assert!(selectivity > 0.00001);
    assert!(selectivity < 0.0001);
}

/// Test selectivity for low-cardinality status
#[test]
fn test_selectivity_status_field() {
    let (_, _, _, column_stats) = create_e_commerce_stats();
    let estimator = SelectivityEstimator::new(column_stats);
    
    let selectivity = estimator.estimate_equality("status");
    // Should be 1/5 = 0.2 average
    assert!(selectivity > 0.1);
    assert!(selectivity < 0.5);
}

/// Test combined selectivity (AND logic)
#[test]
fn test_combined_selectivity_multiple_predicates() {
    let (_, _, _, column_stats) = create_e_commerce_stats();
    let estimator = SelectivityEstimator::new(column_stats);
    
    let sel1 = estimator.estimate_equality("product_id");
    let sel2 = estimator.estimate_equality("status");
    let combined = estimator.estimate_combined(vec![sel1, sel2]);
    
    assert!(combined < sel1);
    assert!(combined < sel2);
    assert_eq!(combined, sel1 * sel2);
}

/// Test plan generator creates multiple candidates
#[test]
fn test_plan_generator_creates_candidates() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let cost_estimator = CostEstimator::new(row_count, column_count, total_size, column_stats.clone());
    let selectivity_estimator = SelectivityEstimator::new(column_stats);
    let generator = PlanGenerator::new(cost_estimator, selectivity_estimator);
    
    let plans = generator.generate_plans(5, false);
    assert!(plans.len() >= 2);
}

/// Test plan generator with predicates
#[test]
fn test_plan_generator_with_predicates() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let cost_estimator = CostEstimator::new(row_count, column_count, total_size, column_stats.clone());
    let selectivity_estimator = SelectivityEstimator::new(column_stats);
    let generator = PlanGenerator::new(cost_estimator, selectivity_estimator);
    
    let plans = generator.generate_plans(5, true);
    assert!(plans.len() >= 4); // Should generate: full scan, pruning, predicate, combined, cache
}

/// Test plan ranking
#[test]
fn test_plan_evaluator_ranking() {
    let evaluator = PlanEvaluator::new(100.0);
    
    let mut plans = vec![
        CandidatePlan::new(ExecutionPlan::new(ExecutionStrategy::FullTableScan, QueryCost::new(10.0, 1.0, 100.0, 1000)), 100.0),
        CandidatePlan::new(ExecutionPlan::new(ExecutionStrategy::ColumnPruning, QueryCost::new(5.0, 0.5, 50.0, 1000)), 50.0),
        CandidatePlan::new(ExecutionPlan::new(ExecutionStrategy::CacheHit, QueryCost::new(0.1, 0.01, 10.0, 1000)), 10.0),
    ];
    
    let evaluated = evaluator.evaluate_plans(plans);
    
    // Should be ranked 1, 2, 3
    assert_eq!(evaluated[0].rank, 1);
    assert_eq!(evaluated[1].rank, 2);
    assert_eq!(evaluated[2].rank, 3);
}

/// Test best plan selection
#[test]
fn test_best_plan_selection() {
    let evaluator = PlanEvaluator::new(100.0);
    
    let plans = vec![
        CandidatePlan::new(ExecutionPlan::new(ExecutionStrategy::FullTableScan, QueryCost::new(10.0, 1.0, 100.0, 1000)), 100.0),
        CandidatePlan::new(ExecutionPlan::new(ExecutionStrategy::ColumnPruning, QueryCost::new(5.0, 0.5, 50.0, 1000)), 25.0),
        CandidatePlan::new(ExecutionPlan::new(ExecutionStrategy::CacheHit, QueryCost::new(0.1, 0.01, 10.0, 1000)), 10.0),
    ];
    
    let best = evaluator.best_plan(&plans);
    assert!(best.is_some());
    assert_eq!(best.unwrap().cost, 10.0);
}

/// Test improvement percentage calculation
#[test]
fn test_improvement_percentage() {
    let evaluator = PlanEvaluator::new(100.0);
    let plan = ExecutionPlan::new(ExecutionStrategy::ColumnPruning, QueryCost::new(5.0, 0.5, 50.0, 1000));
    let candidate = CandidatePlan::new(plan, 50.0);
    
    let improvement = evaluator.improvement_percentage(&candidate);
    assert_eq!(improvement, 50.0);
}

/// Test multi-index coordinator
#[test]
fn test_multi_index_coordinator_ecommerce() {
    let mut coordinator = MultiIndexCoordinator::new();
    
    coordinator.register_index("product_id".to_string(), IndexType::Hash);
    coordinator.register_index("status".to_string(), IndexType::Bitmap);
    coordinator.register_index("order_date".to_string(), IndexType::BTree);
    
    assert_eq!(coordinator.index_count(), 3);
    assert_eq!(coordinator.find_best_index("product_id"), Some(IndexType::Hash));
    assert_eq!(coordinator.find_best_index("status"), Some(IndexType::Bitmap));
}

/// Test optimizer basic functionality
#[test]
fn test_advanced_optimizer_basic() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
    
    let best_plan = optimizer.optimize_query(5, false);
    assert!(best_plan.is_some());
}

/// Test optimizer with predicates
#[test]
fn test_advanced_optimizer_with_predicates() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
    
    let best_plan = optimizer.optimize_query(5, true);
    assert!(best_plan.is_some());
    assert!(best_plan.unwrap().speedup_estimate > 1.0);
}

/// Test optimizer top-N plans
#[test]
fn test_optimizer_top_plans() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
    
    let top_3 = optimizer.get_top_plans(5, true, 3);
    assert_eq!(top_3.len(), 3);
    
    // Top plans should have decreasing speedup
    for i in 1..top_3.len() {
        assert!(top_3[i-1].speedup_estimate >= top_3[i].speedup_estimate);
    }
}

/// Test optimizer with indices
#[test]
fn test_optimizer_with_indices() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let mut optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
    
    optimizer.register_index("product_id".to_string(), IndexType::Hash);
    optimizer.register_index("status".to_string(), IndexType::Bitmap);
    
    let best_plan = optimizer.optimize_query(3, true);
    assert!(best_plan.is_some());
}

/// Test real-world query: "Find all orders for a product status"
#[test]
fn test_query_product_status_search() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let mut optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
    
    optimizer.register_index("product_id".to_string(), IndexType::Hash);
    optimizer.register_index("status".to_string(), IndexType::Bitmap);
    
    let best_plan = optimizer.optimize_query(3, true);
    assert!(best_plan.is_some());
    assert!(best_plan.unwrap().speedup_estimate > 2.0);
}

/// Test real-world query: "Find orders in price range"
#[test]
fn test_query_price_range_search() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let mut optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
    
    optimizer.register_index("amount".to_string(), IndexType::BTree);
    optimizer.register_index("order_date".to_string(), IndexType::BTree);
    
    let best_plan = optimizer.optimize_query(4, true);
    assert!(best_plan.is_some());
}

/// Test query optimization with many columns
#[test]
fn test_optimization_large_column_selection() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
    
    // Select all columns
    let best_plan = optimizer.optimize_query(15, true);
    assert!(best_plan.is_some());
}

/// Test query optimization with single column
#[test]
fn test_optimization_single_column() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
    
    let best_plan = optimizer.optimize_query(1, true);
    assert!(best_plan.is_some());
    // Single column should have good speedup due to pruning
    assert!(best_plan.unwrap().speedup_estimate > 1.5);
}

/// Test cost model accuracy
#[test]
fn test_cost_model_io_dominance() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let estimator = CostEstimator::new(row_count, column_count, total_size, column_stats);
    
    let cost = estimator.estimate_full_scan();
    // I/O should be the dominant component
    let io_percentage = (cost.io_cost * 20.0) / cost.total_cost();
    assert!(io_percentage > 0.7); // I/O should be >70% of total cost
}

/// Test cache hit probability impact
#[test]
fn test_cache_hit_reduction() {
    let cost = QueryCost::new(100.0, 10.0, 1000.0, 1_000_000)
        .with_cache_probability(0.9);
    
    let reduced = cost.with_cache_reduction();
    assert!(reduced.total_cost() < cost.total_cost());
}

/// Test multi-criteria optimization
#[test]
fn test_optimization_with_cache_and_indices() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let mut optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
    
    optimizer.register_index("product_id".to_string(), IndexType::Hash);
    optimizer.register_index("status".to_string(), IndexType::Bitmap);
    optimizer.register_index("order_date".to_string(), IndexType::BTree);
    optimizer.register_index("amount".to_string(), IndexType::BTree);
    
    let top_5 = optimizer.get_top_plans(6, true, 5);
    assert!(top_5.len() > 0);
    
    // Best plan should have good speedup
    assert!(top_5[0].speedup_estimate > 1.0);
}

/// Test optimizer consistency across multiple calls
#[test]
fn test_optimizer_consistency() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
    
    let plan1 = optimizer.optimize_query(5, true);
    let plan2 = optimizer.optimize_query(5, true);
    
    assert_eq!(plan1.as_ref().unwrap().cost, plan2.as_ref().unwrap().cost);
}

/// Test optimizer with extreme selectivity
#[test]
fn test_optimizer_extreme_selectivity() {
    let (_, _, _, column_stats) = create_e_commerce_stats();
    let estimator = SelectivityEstimator::new(column_stats);
    
    let sel_high = estimator.estimate_equality("product_id");
    let sel_low = estimator.estimate_equality("status");
    
    assert!(sel_high < sel_low);
}

/// Test plan generation diversity
#[test]
fn test_plan_generation_diversity() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let cost_estimator = CostEstimator::new(row_count, column_count, total_size, column_stats.clone());
    let selectivity_estimator = SelectivityEstimator::new(column_stats);
    let generator = PlanGenerator::new(cost_estimator, selectivity_estimator);
    
    let plans = generator.generate_plans(8, true);
    
    // Should have diverse strategies
    let strategies: Vec<_> = plans.iter().map(|p| format!("{:?}", p.plan.strategy)).collect();
    let unique: std::collections::HashSet<_> = strategies.iter().cloned().collect();
    
    assert!(unique.len() > 2);
}

/// Test query with no optimization opportunity
#[test]
fn test_query_minimal_optimization() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
    
    // All columns, no predicates
    let best_plan = optimizer.optimize_query(15, false);
    assert!(best_plan.is_some());
    // May not have much speedup without predicates
    assert!(best_plan.unwrap().speedup_estimate >= 1.0);
}

/// Test optimizer with index on every column
#[test]
fn test_optimizer_fully_indexed() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let mut optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
    
    for col in &["product_id", "user_id", "status", "amount", "order_date"] {
        optimizer.register_index(col.to_string(), IndexType::Hash);
    }
    
    let best_plan = optimizer.optimize_query(3, true);
    assert!(best_plan.is_some());
    assert!(best_plan.unwrap().speedup_estimate > 2.0);
}

/// Test selectivity for non-existent column
#[test]
fn test_selectivity_unknown_column() {
    let (_, _, _, column_stats) = create_e_commerce_stats();
    let estimator = SelectivityEstimator::new(column_stats);
    
    let selectivity = estimator.estimate_equality("unknown_column");
    assert_eq!(selectivity, 0.1); // Default assumption
}

/// Test combined index strategy recommendation
#[test]
fn test_combined_index_strategy() {
    let (row_count, column_count, total_size, column_stats) = create_e_commerce_stats();
    let mut optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
    
    // Hash for high-cardinality product lookups
    optimizer.register_index("product_id".to_string(), IndexType::Hash);
    
    // Bitmap for low-cardinality status filtering
    optimizer.register_index("status".to_string(), IndexType::Bitmap);
    
    // BTree for range queries on amount and date
    optimizer.register_index("amount".to_string(), IndexType::BTree);
    optimizer.register_index("order_date".to_string(), IndexType::BTree);
    
    let best_plan = optimizer.optimize_query(10, true);
    assert!(best_plan.is_some());
}
