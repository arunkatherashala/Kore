/// Adaptive Executor for KORE v1.6.0
/// 
/// Executes queries with adaptive strategy selection at runtime.
/// Tracks actual vs. estimated cardinalities and adjusts execution strategy dynamically.

use std::sync::{Arc, RwLock};

/// Statistics collected during execution
#[derive(Debug, Clone)]
pub struct ExecutionStats {
    /// Estimated rows for this operation
    pub estimated_rows: u64,
    /// Actual rows produced by this operation
    pub actual_rows: u64,
    /// Estimated cost (from optimizer)
    pub estimated_cost: f64,
    /// Actual cost incurred so far
    pub actual_cost: f64,
}

impl ExecutionStats {
    pub fn new(estimated_rows: u64, estimated_cost: f64) -> Self {
        ExecutionStats {
            estimated_rows,
            actual_rows: 0,
            estimated_cost,
            actual_cost: 0.0,
        }
    }

    /// Estimate error: ratio of actual to estimated rows
    pub fn estimation_error(&self) -> f64 {
        if self.estimated_rows == 0 {
            return 0.0;
        }
        self.actual_rows as f64 / self.estimated_rows as f64
    }

    /// Cost overrun: actual vs estimated cost
    pub fn cost_overrun(&self) -> f64 {
        if self.estimated_cost == 0.0 {
            return 0.0;
        }
        self.actual_cost / self.estimated_cost
    }

    /// Check if estimates are significantly wrong
    pub fn estimates_wrong(&self) -> bool {
        let error = self.estimation_error();
        // Consider estimates wrong if actual is > 2x or < 0.5x estimated
        error > 2.0 || error < 0.5
    }
}

/// Represents a hint for adaptive execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionHint {
    /// Continue with current strategy
    Continue,
    /// Switch to a different strategy
    Adapt,
    /// Abort current strategy and replan
    Replan,
}

/// Join execution strategies
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinExecutionStrategy {
    /// Nested loop join
    NestedLoop,
    /// Hash join with in-memory hash table
    Hash,
    /// Sort-merge join
    SortMerge,
    /// Grace hash join (spill to disk)
    GraceHash,
}

/// Execution context for adaptive execution
pub struct AdaptiveExecutionContext {
    /// Operation statistics
    pub stats: Arc<RwLock<ExecutionStats>>,
    /// Current join strategy
    pub current_strategy: JoinExecutionStrategy,
    /// Rows processed so far
    pub rows_processed: u64,
    /// Threshold for adaptation decision
    pub adaptation_threshold: f64,
}

impl AdaptiveExecutionContext {
    pub fn new(estimated_rows: u64, estimated_cost: f64) -> Self {
        AdaptiveExecutionContext {
            stats: Arc::new(RwLock::new(ExecutionStats::new(estimated_rows, estimated_cost))),
            current_strategy: JoinExecutionStrategy::Hash,
            rows_processed: 0,
            adaptation_threshold: 0.5, // Adapt if estimate error > 50%
        }
    }

    /// Record actual row produced
    pub fn record_row(&mut self) -> Result<(), String> {
        self.rows_processed += 1;

        let mut stats = self
            .stats
            .write()
            .map_err(|e| format!("Failed to acquire write lock: {}", e))?;
        stats.actual_rows += 1;

        Ok(())
    }

    /// Record cost of operation
    pub fn record_cost(&mut self, cost: f64) -> Result<(), String> {
        let mut stats = self
            .stats
            .write()
            .map_err(|e| format!("Failed to acquire write lock: {}", e))?;
        stats.actual_cost += cost;

        Ok(())
    }

    /// Check if should adapt strategy
    pub fn should_adapt(&self) -> Result<ExecutionHint, String> {
        let stats = self
            .stats
            .read()
            .map_err(|e| format!("Failed to acquire read lock: {}", e))?;

        if stats.estimates_wrong() {
            let error = stats.estimation_error();
            if error > 10.0 || error < 0.1 {
                // Very wrong: consider replanning
                Ok(ExecutionHint::Replan)
            } else {
                // Moderately wrong: consider adapting
                Ok(ExecutionHint::Adapt)
            }
        } else {
            Ok(ExecutionHint::Continue)
        }
    }

    /// Adapt join strategy based on actual cardinalities
    pub fn choose_adaptive_join_strategy(&self, left_rows: u64, right_rows: u64) -> JoinExecutionStrategy {
        let smaller = left_rows.min(right_rows);
        let larger = left_rows.max(right_rows);

        // Very small tables: nested loop is fine
        if larger < 1000 {
            return JoinExecutionStrategy::NestedLoop;
        }

        // Small inner table: hash join is good
        if smaller < 100_000 {
            return JoinExecutionStrategy::Hash;
        }

        // Medium: still try hash join but with spilling
        if smaller < 1_000_000 {
            return JoinExecutionStrategy::GraceHash;
        }

        // Large tables: sort-merge is most scalable
        JoinExecutionStrategy::SortMerge
    }

    /// Update strategy based on execution hint
    pub fn update_strategy(&mut self, hint: ExecutionHint) {
        match hint {
            ExecutionHint::Continue => {
                // Keep current strategy
            }
            ExecutionHint::Adapt => {
                // Try a different strategy
                self.current_strategy = match self.current_strategy {
                    JoinExecutionStrategy::Hash => JoinExecutionStrategy::SortMerge,
                    JoinExecutionStrategy::NestedLoop => JoinExecutionStrategy::Hash,
                    JoinExecutionStrategy::SortMerge => JoinExecutionStrategy::NestedLoop,
                    JoinExecutionStrategy::GraceHash => JoinExecutionStrategy::SortMerge,
                };
            }
            ExecutionHint::Replan => {
                // Switch to the most conservative strategy (sort-merge)
                self.current_strategy = JoinExecutionStrategy::SortMerge;
            }
        }
    }
}

/// Execute a join with potential runtime adaptation
pub fn execute_join_adaptive(
    left_rows: u64,
    right_rows: u64,
    context: &mut AdaptiveExecutionContext,
) -> Result<u64, String> {
    // Execute with current strategy
    let mut result_count = 0;

    loop {
        // Check if we should adapt
        match context.should_adapt()? {
            ExecutionHint::Continue => {
                // Continue with current strategy
                break;
            }
            ExecutionHint::Adapt => {
                // Update strategy and continue
                context.update_strategy(ExecutionHint::Adapt);
            }
            ExecutionHint::Replan => {
                // Abort and replan
                return Err("Query plan needs replanning".to_string());
            }
        }
    }

    // Simulate join execution
    match context.current_strategy {
        JoinExecutionStrategy::NestedLoop => {
            result_count = (left_rows * right_rows) / 10; // Assume 10% selectivity
        }
        JoinExecutionStrategy::Hash => {
            result_count = (left_rows * right_rows) / 10;
        }
        JoinExecutionStrategy::SortMerge => {
            result_count = (left_rows * right_rows) / 10;
        }
        JoinExecutionStrategy::GraceHash => {
            result_count = (left_rows * right_rows) / 10;
        }
    }

    // Record actual results
    for _ in 0..result_count {
        context.record_row()?;
    }

    Ok(result_count)
}

/// Pipeline execution with runtime adaptation
pub struct AdaptiveQueryPipeline {
    /// Current stage index
    pub current_stage: usize,
    /// Execution contexts for each stage
    pub stage_contexts: Vec<AdaptiveExecutionContext>,
}

impl AdaptiveQueryPipeline {
    pub fn new(num_stages: usize) -> Self {
        AdaptiveQueryPipeline {
            current_stage: 0,
            stage_contexts: Vec::with_capacity(num_stages),
        }
    }

    /// Add a stage to the pipeline
    pub fn add_stage(&mut self, estimated_rows: u64, estimated_cost: f64) {
        self.stage_contexts
            .push(AdaptiveExecutionContext::new(estimated_rows, estimated_cost));
    }

    /// Execute next stage
    pub fn execute_next_stage(&mut self) -> Result<ExecutionHint, String> {
        if self.current_stage >= self.stage_contexts.len() {
            return Err("No more stages to execute".to_string());
        }

        let context = &self.stage_contexts[self.current_stage];
        let hint = context.should_adapt()?;

        self.current_stage += 1;
        Ok(hint)
    }

    /// Get pipeline statistics
    pub fn get_stats(&self) -> Result<Vec<ExecutionStats>, String> {
        let mut all_stats = Vec::new();

        for context in &self.stage_contexts {
            let stats = context
                .stats
                .read()
                .map_err(|e| format!("Failed to read stats: {}", e))?;
            all_stats.push(stats.clone());
        }

        Ok(all_stats)
    }

    /// Check if pipeline needs replanning
    pub fn needs_replanning(&self) -> Result<bool, String> {
        for context in &self.stage_contexts {
            match context.should_adapt()? {
                ExecutionHint::Replan => return Ok(true),
                _ => {}
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_stats_creation() {
        let stats = ExecutionStats::new(1000, 100.0);
        assert_eq!(stats.estimated_rows, 1000);
        assert_eq!(stats.estimated_cost, 100.0);
        assert_eq!(stats.actual_rows, 0);
    }

    #[test]
    fn test_estimation_error() {
        let mut stats = ExecutionStats::new(1000, 100.0);
        stats.actual_rows = 2000; // 2x estimates

        assert_eq!(stats.estimation_error(), 2.0);
    }

    #[test]
    fn test_estimates_wrong_too_high() {
        let mut stats = ExecutionStats::new(1000, 100.0);
        stats.actual_rows = 100; // 0.1x estimates

        assert!(stats.estimates_wrong());
    }

    #[test]
    fn test_estimates_wrong_too_low() {
        let mut stats = ExecutionStats::new(1000, 100.0);
        stats.actual_rows = 3000; // 3x estimates

        assert!(stats.estimates_wrong());
    }

    #[test]
    fn test_adaptive_context_creation() {
        let ctx = AdaptiveExecutionContext::new(1000, 100.0);
        assert_eq!(ctx.rows_processed, 0);
        assert_eq!(ctx.current_strategy, JoinExecutionStrategy::Hash);
    }

    #[test]
    fn test_record_row() {
        let mut ctx = AdaptiveExecutionContext::new(1000, 100.0);
        let result = ctx.record_row();
        assert!(result.is_ok());
        assert_eq!(ctx.rows_processed, 1);
    }

    #[test]
    fn test_choose_adaptive_join_strategy_small() {
        let ctx = AdaptiveExecutionContext::new(1000, 100.0);
        let strategy = ctx.choose_adaptive_join_strategy(100, 200);
        assert_eq!(strategy, JoinExecutionStrategy::NestedLoop);
    }

    #[test]
    fn test_choose_adaptive_join_strategy_medium() {
        let ctx = AdaptiveExecutionContext::new(1000, 100.0);
        let strategy = ctx.choose_adaptive_join_strategy(10000, 50000);
        assert_eq!(strategy, JoinExecutionStrategy::Hash);
    }

    #[test]
    fn test_choose_adaptive_join_strategy_large() {
        let ctx = AdaptiveExecutionContext::new(1000, 100.0);
        let strategy = ctx.choose_adaptive_join_strategy(10_000_000, 10_000_000);
        assert_eq!(strategy, JoinExecutionStrategy::SortMerge);
    }

    #[test]
    fn test_should_adapt_no_error() {
        let ctx = AdaptiveExecutionContext::new(1000, 100.0);
        let hint = ctx.should_adapt().unwrap();
        assert_eq!(hint, ExecutionHint::Continue);
    }

    #[test]
    fn test_update_strategy() {
        let mut ctx = AdaptiveExecutionContext::new(1000, 100.0);
        let initial = ctx.current_strategy;
        ctx.update_strategy(ExecutionHint::Adapt);
        let after_adapt = ctx.current_strategy;

        assert_ne!(initial, after_adapt);
    }

    #[test]
    fn test_adaptive_pipeline_creation() {
        let pipeline = AdaptiveQueryPipeline::new(3);
        assert_eq!(pipeline.current_stage, 0);
        assert_eq!(pipeline.stage_contexts.len(), 0);
    }

    #[test]
    fn test_adaptive_pipeline_add_stages() {
        let mut pipeline = AdaptiveQueryPipeline::new(3);
        pipeline.add_stage(1000, 100.0);
        pipeline.add_stage(500, 50.0);
        pipeline.add_stage(100, 10.0);

        assert_eq!(pipeline.stage_contexts.len(), 3);
    }

    #[test]
    fn test_execute_join_adaptive() {
        let mut ctx = AdaptiveExecutionContext::new(100, 10.0);
        let result = execute_join_adaptive(100, 100, &mut ctx);
        assert!(result.is_ok());
        assert!(result.unwrap() > 0);
    }
}
