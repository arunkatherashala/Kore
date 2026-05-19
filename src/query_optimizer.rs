use crate::query_execution::{ExecutionPlan, ExecutionStrategy, QueryCost};
use crate::statistics::ColumnStatistics;
use crate::indexing::{IndexType, IndexSelector};
use std::collections::HashMap;

/// Cost estimator for different query execution strategies
#[derive(Debug, Clone)]
pub struct CostEstimator {
    row_count: u64,
    column_count: usize,
    total_size_estimate: f64,
    column_stats: HashMap<String, ColumnStatistics>,
}

impl CostEstimator {
    /// Create new cost estimator
    pub fn new(
        row_count: u64,
        column_count: usize,
        total_size_estimate: f64,
        column_stats: HashMap<String, ColumnStatistics>,
    ) -> Self {
        Self {
            row_count,
            column_count,
            total_size_estimate,
            column_stats,
        }
    }

    /// Estimate cost for full table scan
    pub fn estimate_full_scan(&self) -> QueryCost {
        let io_cost = self.total_size_estimate / 1_000_000.0;
        let cpu_cost = (self.row_count as f64) / 10_000.0;
        let memory_cost = (self.column_count as f64) * 1000.0;
        
        QueryCost::new(io_cost, cpu_cost, memory_cost, self.row_count)
    }

    /// Estimate cost for predicate pushdown
    pub fn estimate_predicate_pushdown(&self, selectivity: f64) -> QueryCost {
        let io_cost = self.total_size_estimate / 1_000_000.0;
        let filtered_rows = (self.row_count as f64 * selectivity).max(1.0) as u64;
        let cpu_cost = (self.row_count as f64) / 5_000.0; // Higher CPU for filtering
        let memory_cost = (filtered_rows as f64) / 100.0;
        
        QueryCost::new(io_cost, cpu_cost, memory_cost, filtered_rows)
    }

    /// Estimate cost for column pruning
    pub fn estimate_column_pruning(&self, num_columns: usize) -> QueryCost {
        let column_fraction = (num_columns as f64) / (self.column_count as f64);
        let io_cost = self.total_size_estimate * column_fraction / 1_000_000.0;
        let cpu_cost = (self.row_count as f64) / 15_000.0;
        let memory_cost = (num_columns as f64) * 500.0;
        
        QueryCost::new(io_cost, cpu_cost, memory_cost, self.row_count)
    }

    /// Estimate cost for index-based lookup
    pub fn estimate_index_lookup(&self, cardinality: usize, index_type: IndexType) -> QueryCost {
        let cardinality_ratio = (cardinality as f64) / (self.row_count as f64);
        let speedup = IndexSelector::estimated_speedup(index_type, cardinality_ratio);
        
        let io_cost = ((self.total_size_estimate / 1_000_000.0) / speedup).max(0.1);
        let cpu_cost = (50.0) / speedup; // Very low CPU for index lookup
        let memory_cost = 100.0; // Index memory is minimal
        
        QueryCost::new(io_cost, cpu_cost, memory_cost, cardinality as u64)
    }

    /// Estimate cost for cached query
    pub fn estimate_cached_query(&self) -> QueryCost {
        QueryCost::new(0.1, 0.01, 10.0, self.row_count)
            .with_cache_probability(0.95)
    }
}

/// Candidate execution plan with ranking
#[derive(Debug, Clone)]
pub struct CandidatePlan {
    pub plan: ExecutionPlan,
    pub cost: f64,
    pub speedup_estimate: f64,
    pub rank: usize,
}

impl CandidatePlan {
    /// Create new candidate
    pub fn new(plan: ExecutionPlan, cost: f64) -> Self {
        Self {
            plan,
            cost,
            speedup_estimate: 1.0,
            rank: 0,
        }
    }

    /// Set speedup estimate relative to baseline
    pub fn with_speedup(mut self, baseline_cost: f64) -> Self {
        self.speedup_estimate = baseline_cost / self.cost.max(0.01);
        self
    }

    /// Set plan ranking
    pub fn with_rank(mut self, rank: usize) -> Self {
        self.rank = rank;
        self
    }
}

/// Selectivity estimator using statistics
#[derive(Debug, Clone)]
pub struct SelectivityEstimator {
    column_stats: HashMap<String, ColumnStatistics>,
}

impl SelectivityEstimator {
    /// Create new selectivity estimator
    pub fn new(column_stats: HashMap<String, ColumnStatistics>) -> Self {
        Self { column_stats }
    }

    /// Estimate selectivity for equality predicate
    pub fn estimate_equality(&self, column: &str) -> f64 {
        if let Some(stats) = self.column_stats.get(column) {
            1.0 / (stats.distinct_count as f64).max(1.0)
        } else {
            0.1 // Default assumption
        }
    }

    /// Estimate selectivity for range predicate
    pub fn estimate_range(&self, column: &str, _min: &str, _max: &str) -> f64 {
        if let Some(stats) = self.column_stats.get(column) {
            (stats.null_count as f64) / (stats.row_count as f64).max(1.0)
        } else {
            0.5 // Default: 50% of rows
        }
    }

    /// Estimate selectivity for IN predicate
    pub fn estimate_in(&self, column: &str, num_values: usize) -> f64 {
        if let Some(stats) = self.column_stats.get(column) {
            let per_value = 1.0 / (stats.distinct_count as f64).max(1.0);
            (per_value * num_values as f64).min(1.0)
        } else {
            0.1 * num_values as f64
        }
    }

    /// Estimate combined selectivity for multiple predicates (AND logic)
    pub fn estimate_combined(&self, selectivities: Vec<f64>) -> f64 {
        selectivities.iter().product()
    }
}

/// Plan generator for creating candidate execution plans
#[derive(Debug, Clone)]
pub struct PlanGenerator {
    cost_estimator: CostEstimator,
    selectivity_estimator: SelectivityEstimator,
}

impl PlanGenerator {
    /// Create new plan generator
    pub fn new(
        cost_estimator: CostEstimator,
        selectivity_estimator: SelectivityEstimator,
    ) -> Self {
        Self {
            cost_estimator,
            selectivity_estimator,
        }
    }

    /// Generate candidate plans
    pub fn generate_plans(&self, num_columns: usize, has_predicates: bool) -> Vec<CandidatePlan> {
        let mut plans = Vec::new();

        // Plan 1: Full table scan
        let baseline_cost = self.cost_estimator.estimate_full_scan();
        let baseline_total = baseline_cost.total_cost();
        let full_scan_plan = ExecutionPlan::new(
            ExecutionStrategy::FullTableScan,
            baseline_cost.clone(),
        );
        plans.push(
            CandidatePlan::new(full_scan_plan, baseline_total)
                .with_rank(1)
        );

        // Plan 2: Column pruning (if applicable)
        if num_columns > 0 && num_columns < 10 {
            let pruned_cost = self.cost_estimator.estimate_column_pruning(num_columns);
            let pruned_total = pruned_cost.total_cost();
            let column_plan = ExecutionPlan::new(
                ExecutionStrategy::ColumnPruning,
                pruned_cost,
            );
            plans.push(
                CandidatePlan::new(column_plan, pruned_total)
                    .with_speedup(baseline_total)
                    .with_rank(2)
            );
        }

        // Plan 3: Predicate pushdown (if applicable)
        if has_predicates {
            let selectivity = 0.3; // Assume 30% selectivity
            let pred_cost = self.cost_estimator.estimate_predicate_pushdown(selectivity);
            let pred_total = pred_cost.total_cost();
            let pred_plan = ExecutionPlan::new(
                ExecutionStrategy::PredicatePushdown,
                pred_cost,
            );
            plans.push(
                CandidatePlan::new(pred_plan, pred_total)
                    .with_speedup(baseline_total)
                    .with_rank(3)
            );
        }

        // Plan 4: Combined optimization
        if num_columns > 0 && has_predicates {
            let selectivity = 0.3;
            let combined_io = (baseline_total * 0.3 * 0.5) / 20.0; // 70% reduction
            let combined_cpu = (baseline_total * 0.3) / 20.0;
            let combined_cost = QueryCost::new(
                combined_io * 20.0,
                combined_cpu * 20.0,
                100.0,
                (self.cost_estimator.row_count as f64 * selectivity) as u64,
            );
            let combined_total = combined_cost.total_cost();
            let combined_plan = ExecutionPlan::new(
                ExecutionStrategy::Combined,
                combined_cost,
            );
            plans.push(
                CandidatePlan::new(combined_plan, combined_total)
                    .with_speedup(baseline_total)
                    .with_rank(4)
            );
        }

        // Plan 5: Cached query
        let cached_cost = self.cost_estimator.estimate_cached_query();
        let cached_total = cached_cost.total_cost();
        let cached_plan = ExecutionPlan::new(
            ExecutionStrategy::CacheHit,
            cached_cost,
        );
        plans.push(
            CandidatePlan::new(cached_plan, cached_total)
                .with_speedup(baseline_total)
                .with_rank(5)
        );

        // Sort by cost (ascending)
        plans.sort_by(|a, b| a.cost.partial_cmp(&b.cost).unwrap_or(std::cmp::Ordering::Equal));

        plans
    }
}

/// Multi-index coordinator
#[derive(Debug, Clone)]
pub struct MultiIndexCoordinator {
    available_indices: HashMap<String, IndexType>,
}

impl MultiIndexCoordinator {
    /// Create new coordinator
    pub fn new() -> Self {
        Self {
            available_indices: HashMap::new(),
        }
    }

    /// Register available index
    pub fn register_index(&mut self, column: String, index_type: IndexType) {
        self.available_indices.insert(column, index_type);
    }

    /// Find best index for column
    pub fn find_best_index(&self, column: &str) -> Option<IndexType> {
        self.available_indices.get(column).cloned()
    }

    /// Get all registered columns
    pub fn registered_columns(&self) -> Vec<String> {
        self.available_indices.keys().cloned().collect()
    }

    /// Get index count
    pub fn index_count(&self) -> usize {
        self.available_indices.len()
    }
}

impl Default for MultiIndexCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// Plan evaluator for comparing and ranking plans
#[derive(Debug, Clone)]
pub struct PlanEvaluator {
    baseline_cost: f64,
}

impl PlanEvaluator {
    /// Create new evaluator
    pub fn new(baseline_cost: f64) -> Self {
        Self { baseline_cost }
    }

    /// Evaluate and rank plans
    pub fn evaluate_plans(&self, mut plans: Vec<CandidatePlan>) -> Vec<CandidatePlan> {
        // Calculate speedup for each plan
        for plan in &mut plans {
            plan.speedup_estimate = self.baseline_cost / plan.cost.max(0.01);
        }

        // Sort by speedup (descending)
        plans.sort_by(|a, b| {
            b.speedup_estimate.partial_cmp(&a.speedup_estimate)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Assign final ranks
        for (idx, plan) in plans.iter_mut().enumerate() {
            plan.rank = idx + 1;
        }

        plans
    }

    /// Get best plan (lowest cost)
    pub fn best_plan<'a>(&self, plans: &'a [CandidatePlan]) -> Option<&'a CandidatePlan> {
        plans.iter().min_by(|a, b| {
            a.cost.partial_cmp(&b.cost).unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Calculate improvement percentage
    pub fn improvement_percentage(&self, plan: &CandidatePlan) -> f64 {
        ((self.baseline_cost - plan.cost) / self.baseline_cost) * 100.0
    }
}

/// Advanced query optimizer
#[derive(Debug, Clone)]
pub struct AdvancedQueryOptimizer {
    cost_estimator: CostEstimator,
    selectivity_estimator: SelectivityEstimator,
    plan_generator: PlanGenerator,
    plan_evaluator: PlanEvaluator,
    index_coordinator: MultiIndexCoordinator,
}

impl AdvancedQueryOptimizer {
    /// Create new optimizer
    pub fn new(
        row_count: u64,
        column_count: usize,
        total_size_estimate: f64,
        column_stats: HashMap<String, ColumnStatistics>,
    ) -> Self {
        let cost_estimator = CostEstimator::new(row_count, column_count, total_size_estimate, column_stats.clone());
        let selectivity_estimator = SelectivityEstimator::new(column_stats);
        let baseline = cost_estimator.estimate_full_scan();
        let baseline_cost = baseline.total_cost();
        let plan_generator = PlanGenerator::new(cost_estimator.clone(), selectivity_estimator.clone());
        let plan_evaluator = PlanEvaluator::new(baseline_cost);

        Self {
            cost_estimator,
            selectivity_estimator,
            plan_generator,
            plan_evaluator,
            index_coordinator: MultiIndexCoordinator::new(),
        }
    }

    /// Optimize query and return best plan
    pub fn optimize_query(&self, num_columns: usize, has_predicates: bool) -> Option<CandidatePlan> {
        let candidate_plans = self.plan_generator.generate_plans(num_columns, has_predicates);
        let evaluated_plans = self.plan_evaluator.evaluate_plans(candidate_plans);
        evaluated_plans.first().cloned()
    }

    /// Get top N plans
    pub fn get_top_plans(&self, num_columns: usize, has_predicates: bool, n: usize) -> Vec<CandidatePlan> {
        let candidate_plans = self.plan_generator.generate_plans(num_columns, has_predicates);
        let evaluated_plans = self.plan_evaluator.evaluate_plans(candidate_plans);
        evaluated_plans.into_iter().take(n).collect()
    }

    /// Register index for optimization
    pub fn register_index(&mut self, column: String, index_type: IndexType) {
        self.index_coordinator.register_index(column, index_type);
    }

    /// Get index coordinator
    pub fn index_coordinator(&self) -> &MultiIndexCoordinator {
        &self.index_coordinator
    }

    /// Get selectivity for column
    pub fn get_selectivity(&self, column: &str) -> f64 {
        self.selectivity_estimator.estimate_equality(column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_stats() -> (u64, usize, f64, HashMap<String, ColumnStatistics>) {
        let row_count = 1_000_000_u64;
        let column_count = 10_usize;
        let total_size = 100_000_000.0; // 100 MB

        let mut column_stats = HashMap::new();
        for i in 0..10 {
            column_stats.insert(
                format!("col_{}", i),
                ColumnStatistics::new(
                    format!("col_{}", i),
                    "String".to_string(),
                    row_count,
                    1000,
                    1000 + (i as u64 * 100),
                ),
            );
        }

        (row_count, column_count, total_size, column_stats)
    }

    #[test]
    fn test_cost_estimator_creation() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let estimator = CostEstimator::new(row_count, column_count, total_size, column_stats);
        assert_eq!(estimator.row_count, 1_000_000);
    }

    #[test]
    fn test_estimate_full_scan() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let estimator = CostEstimator::new(row_count, column_count, total_size, column_stats);
        let cost = estimator.estimate_full_scan();
        assert!(cost.io_cost > 0.0);
        assert!(cost.total_cost() > 0.0);
    }

    #[test]
    fn test_estimate_predicate_pushdown() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let estimator = CostEstimator::new(row_count, column_count, total_size, column_stats);
        let cost = estimator.estimate_predicate_pushdown(0.3);
        assert!(cost.estimated_rows <= 1_000_000);
    }

    #[test]
    fn test_estimate_column_pruning() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let estimator = CostEstimator::new(row_count, column_count, total_size, column_stats);
        let cost = estimator.estimate_column_pruning(5);
        assert!(cost.io_cost < 100.0);
    }

    #[test]
    fn test_estimate_index_lookup() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let estimator = CostEstimator::new(row_count, column_count, total_size, column_stats);
        let cost = estimator.estimate_index_lookup(1000, IndexType::Hash);
        assert!(cost.io_cost < 10.0);
    }

    #[test]
    fn test_estimate_cached_query() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let estimator = CostEstimator::new(row_count, column_count, total_size, column_stats);
        let cost = estimator.estimate_cached_query();
        assert!(cost.cache_hit_probability > 0.9);
    }

    #[test]
    fn test_candidate_plan_creation() {
        let plan = ExecutionPlan::new(ExecutionStrategy::FullTableScan, QueryCost::new(10.0, 1.0, 100.0, 1000));
        let candidate = CandidatePlan::new(plan, 50.0);
        assert_eq!(candidate.cost, 50.0);
    }

    #[test]
    fn test_candidate_plan_with_speedup() {
        let plan = ExecutionPlan::new(ExecutionStrategy::FullTableScan, QueryCost::new(10.0, 1.0, 100.0, 1000));
        let candidate = CandidatePlan::new(plan, 25.0).with_speedup(100.0);
        assert_eq!(candidate.speedup_estimate, 4.0);
    }

    #[test]
    fn test_selectivity_estimator_equality() {
        let (_, _, _, column_stats) = create_test_stats();
        let estimator = SelectivityEstimator::new(column_stats);
        let selectivity = estimator.estimate_equality("col_0");
        assert!(selectivity > 0.0 && selectivity < 1.0);
    }

    #[test]
    fn test_selectivity_estimator_range() {
        let (_, _, _, column_stats) = create_test_stats();
        let estimator = SelectivityEstimator::new(column_stats);
        let selectivity = estimator.estimate_range("col_0", "0", "100");
        assert!(selectivity >= 0.0 && selectivity <= 1.0);
    }

    #[test]
    fn test_selectivity_estimator_in() {
        let (_, _, _, column_stats) = create_test_stats();
        let estimator = SelectivityEstimator::new(column_stats);
        let selectivity = estimator.estimate_in("col_0", 10);
        assert!(selectivity >= 0.0 && selectivity <= 1.0);
    }

    #[test]
    fn test_selectivity_estimator_combined() {
        let (_, _, _, column_stats) = create_test_stats();
        let estimator = SelectivityEstimator::new(column_stats);
        let combined = estimator.estimate_combined(vec![0.5, 0.3, 0.2]);
        assert_eq!(combined, 0.03);
    }

    #[test]
    fn test_plan_generator_creation() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let cost_estimator = CostEstimator::new(row_count, column_count, total_size, column_stats.clone());
        let selectivity_estimator = SelectivityEstimator::new(column_stats);
        let generator = PlanGenerator::new(cost_estimator, selectivity_estimator);
        assert!(generator.generate_plans(5, false).len() > 0);
    }

    #[test]
    fn test_plan_generator_multiple_plans() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let cost_estimator = CostEstimator::new(row_count, column_count, total_size, column_stats.clone());
        let selectivity_estimator = SelectivityEstimator::new(column_stats);
        let generator = PlanGenerator::new(cost_estimator, selectivity_estimator);
        let plans = generator.generate_plans(5, true);
        assert!(plans.len() >= 3);
    }

    #[test]
    fn test_multi_index_coordinator_registration() {
        let mut coordinator = MultiIndexCoordinator::new();
        coordinator.register_index("col_1".to_string(), IndexType::Hash);
        coordinator.register_index("col_2".to_string(), IndexType::BTree);
        assert_eq!(coordinator.index_count(), 2);
    }

    #[test]
    fn test_multi_index_coordinator_find_index() {
        let mut coordinator = MultiIndexCoordinator::new();
        coordinator.register_index("col_1".to_string(), IndexType::Hash);
        let found = coordinator.find_best_index("col_1");
        assert_eq!(found, Some(IndexType::Hash));
    }

    #[test]
    fn test_multi_index_coordinator_registered_columns() {
        let mut coordinator = MultiIndexCoordinator::new();
        coordinator.register_index("col_1".to_string(), IndexType::Hash);
        coordinator.register_index("col_2".to_string(), IndexType::BTree);
        let columns = coordinator.registered_columns();
        assert_eq!(columns.len(), 2);
    }

    #[test]
    fn test_plan_evaluator_best_plan() {
        let evaluator = PlanEvaluator::new(100.0);
        let plan1 = ExecutionPlan::new(ExecutionStrategy::FullTableScan, QueryCost::new(10.0, 1.0, 100.0, 1000));
        let plan2 = ExecutionPlan::new(ExecutionStrategy::ColumnPruning, QueryCost::new(5.0, 0.5, 50.0, 1000));
        
        let candidates = vec![
            CandidatePlan::new(plan1, 50.0),
            CandidatePlan::new(plan2, 25.0),
        ];
        
        let best = evaluator.best_plan(&candidates);
        assert!(best.is_some());
        assert_eq!(best.unwrap().cost, 25.0);
    }

    #[test]
    fn test_plan_evaluator_improvement_percentage() {
        let evaluator = PlanEvaluator::new(100.0);
        let plan = ExecutionPlan::new(ExecutionStrategy::ColumnPruning, QueryCost::new(5.0, 0.5, 50.0, 1000));
        let candidate = CandidatePlan::new(plan, 50.0);
        
        let improvement = evaluator.improvement_percentage(&candidate);
        assert_eq!(improvement, 50.0);
    }

    #[test]
    fn test_advanced_optimizer_creation() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
        assert!(optimizer.index_coordinator.index_count() == 0);
    }

    #[test]
    fn test_advanced_optimizer_optimize_query() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
        let best_plan = optimizer.optimize_query(5, true);
        assert!(best_plan.is_some());
    }

    #[test]
    fn test_advanced_optimizer_get_top_plans() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
        let top_plans = optimizer.get_top_plans(5, true, 3);
        assert!(top_plans.len() > 0);
        assert!(top_plans.len() <= 3);
    }

    #[test]
    fn test_advanced_optimizer_register_index() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let mut optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
        optimizer.register_index("col_1".to_string(), IndexType::Hash);
        assert_eq!(optimizer.index_coordinator().index_count(), 1);
    }

    #[test]
    fn test_advanced_optimizer_selectivity() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
        let selectivity = optimizer.get_selectivity("col_0");
        assert!(selectivity > 0.0);
    }

    #[test]
    fn test_cost_comparison_different_strategies() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let estimator = CostEstimator::new(row_count, column_count, total_size, column_stats);
        
        let full_scan = estimator.estimate_full_scan().total_cost();
        let pruned = estimator.estimate_column_pruning(5).total_cost();
        let predicate = estimator.estimate_predicate_pushdown(0.3).total_cost();
        
        assert!(pruned < full_scan);
        assert!(predicate < full_scan);
    }

    #[test]
    fn test_optimizer_cost_reduction_with_indices() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let estimator = CostEstimator::new(row_count, column_count, total_size, column_stats);
        
        let baseline = estimator.estimate_full_scan().total_cost();
        let indexed = estimator.estimate_index_lookup(1000, IndexType::Hash).total_cost();
        
        assert!(indexed < baseline);
    }

    #[test]
    fn test_multi_strategy_optimization() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let cost_estimator = CostEstimator::new(row_count, column_count, total_size, column_stats.clone());
        let selectivity_estimator = SelectivityEstimator::new(column_stats);
        let generator = PlanGenerator::new(cost_estimator, selectivity_estimator);
        
        let plans = generator.generate_plans(8, true);
        
        // Should generate multiple strategies
        let strategies: Vec<_> = plans.iter().map(|p| p.plan.strategy.clone()).collect();
        assert!(strategies.contains(&ExecutionStrategy::FullTableScan));
        assert!(strategies.contains(&ExecutionStrategy::CacheHit));
    }

    #[test]
    fn test_plan_ranking() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
        let top_plans = optimizer.get_top_plans(5, true, 5);
        
        // Plans should be ranked from best to worst
        for i in 1..top_plans.len() {
            assert!(top_plans[i - 1].rank <= top_plans[i].rank);
        }
    }

    #[test]
    fn test_selectivity_product_calculation() {
        let (_, _, _, column_stats) = create_test_stats();
        let estimator = SelectivityEstimator::new(column_stats);
        
        let selectivity1 = estimator.estimate_equality("col_0");
        let selectivity2 = estimator.estimate_equality("col_1");
        let combined = estimator.estimate_combined(vec![selectivity1, selectivity2]);
        
        assert!(combined <= selectivity1);
        assert!(combined <= selectivity2);
    }

    #[test]
    fn test_index_coordinator_defaults() {
        let coordinator = MultiIndexCoordinator::default();
        assert_eq!(coordinator.index_count(), 0);
    }

    #[test]
    fn test_optimizer_complex_scenario() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let mut optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
        
        // Register multiple indices
        optimizer.register_index("col_0".to_string(), IndexType::Hash);
        optimizer.register_index("col_1".to_string(), IndexType::BTree);
        optimizer.register_index("col_2".to_string(), IndexType::Bitmap);
        
        // Get optimization recommendation
        let best_plan = optimizer.optimize_query(8, true);
        
        assert!(best_plan.is_some());
        assert!(best_plan.unwrap().speedup_estimate > 1.0);
    }

    #[test]
    fn test_cost_reduction_percentage() {
        let (row_count, column_count, total_size, column_stats) = create_test_stats();
        let estimator = CostEstimator::new(row_count, column_count, total_size, column_stats);
        
        let baseline = estimator.estimate_full_scan();
        let optimized = estimator.estimate_column_pruning(3);
        
        let reduction_percent = ((baseline.total_cost() - optimized.total_cost()) / baseline.total_cost()) * 100.0;
        assert!(reduction_percent > 0.0);
    }

    #[test]
    fn test_plan_evaluation_ranking() {
        let evaluator = PlanEvaluator::new(100.0);
        let mut plans = vec![
            CandidatePlan::new(ExecutionPlan::new(ExecutionStrategy::FullTableScan, QueryCost::new(10.0, 1.0, 100.0, 1000)), 100.0),
            CandidatePlan::new(ExecutionPlan::new(ExecutionStrategy::ColumnPruning, QueryCost::new(5.0, 0.5, 50.0, 1000)), 50.0),
            CandidatePlan::new(ExecutionPlan::new(ExecutionStrategy::CacheHit, QueryCost::new(0.1, 0.01, 10.0, 1000)), 10.0),
        ];
        
        let evaluated = evaluator.evaluate_plans(plans);
        assert_eq!(evaluated[0].rank, 1);
        assert!(evaluated[0].speedup_estimate > evaluated[1].speedup_estimate);
    }

    #[test]
    fn test_large_scale_optimization() {
        let row_count = 10_000_000u64;
        let column_count = 100usize;
        let total_size = 1_000_000_000.0f64;

        let mut column_stats = HashMap::new();
        for i in 0..100 {
            column_stats.insert(
                format!("col_{}", i),
                ColumnStatistics {
                    name: format!("col_{}", i),
                    data_type: "Int64".to_string(),
                    row_count,
                    distinct_count: 10000 + (i as u64 * 100),
                    null_count: 10000,
                    min_value: Some("0".to_string()),
                    max_value: Some("9999".to_string()),
                    avg_length: 8.0,
                    compression_ratio: 0.5,
                },
            );
        }

        let optimizer = AdvancedQueryOptimizer::new(row_count, column_count, total_size, column_stats);
        let best_plan = optimizer.optimize_query(50, true);
        
        assert!(best_plan.is_some());
        assert!(best_plan.unwrap().speedup_estimate > 1.0);
    }

    #[test]
    fn test_query_cost_cache_reduction() {
        let cost = QueryCost::new(100.0, 10.0, 1000.0, 1_000_000)
            .with_cache_probability(0.8);
        let reduced = cost.with_cache_reduction();
        
        assert!(reduced.io_cost < cost.io_cost);
        assert!(reduced.cpu_cost < cost.cpu_cost);
    }

    #[test]
    fn test_zero_cost_edge_case() {
        let evaluator = PlanEvaluator::new(0.1); // Very small baseline
        let plan = ExecutionPlan::new(ExecutionStrategy::CacheHit, QueryCost::new(0.01, 0.001, 1.0, 1000));
        let candidate = CandidatePlan::new(plan, 0.01);
        
        let speedup = evaluator.improvement_percentage(&candidate);
        assert!(speedup >= 0.0 && speedup <= 100.0);
    }
}
