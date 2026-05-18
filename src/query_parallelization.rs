use std::thread;

/// Configuration for parallel query execution
#[derive(Clone)]
pub struct ParallelConfig {
    pub worker_threads: usize,
    pub chunk_size: usize,
    pub enable_parallel_joins: bool,
}

impl ParallelConfig {
    pub fn new() -> Self {
        Self {
            worker_threads: Self::default_worker_count(),
            chunk_size: 10000,
            enable_parallel_joins: true,
        }
    }

    fn default_worker_count() -> usize {
        // Default to 4 workers; can be overridden
        4
    }

    pub fn with_threads(mut self, threads: usize) -> Self {
        self.worker_threads = threads;
        self
    }

    pub fn with_chunk_size(mut self, size: usize) -> Self {
        self.chunk_size = size;
        self
    }
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents a parallel query execution task
#[derive(Clone, Debug)]
pub struct ParallelTask {
    pub task_id: usize,
    pub start_offset: usize,
    pub end_offset: usize,
    pub filter_selectivity: f64,
}

/// Result from parallel query execution
#[derive(Clone, Debug, PartialEq)]
pub struct ParallelResult {
    pub task_id: usize,
    pub rows_processed: usize,
    pub rows_passed_filter: usize,
    pub execution_time_ms: u64,
}

/// Parallel query executor for large result sets
pub struct ParallelQueryExecutor {
    config: ParallelConfig,
}

impl ParallelQueryExecutor {
    pub fn new(config: ParallelConfig) -> Self {
        Self { config }
    }

    /// Execute query in parallel across multiple tasks
    /// Returns Vec<ParallelResult> with per-task metrics
    pub fn execute_parallel(
        &self,
        total_rows: usize,
        selectivity: f64,
    ) -> Vec<ParallelResult> {
        let chunk_size = self.config.chunk_size;
        let num_chunks =
            (total_rows + chunk_size - 1) / chunk_size;

        let mut handles = vec![];

        for task_id in 0..num_chunks {
            let start = task_id * chunk_size;
            let end = (start + chunk_size).min(total_rows);
            let selectivity_copy = selectivity;

            let handle = thread::spawn(move || {
                Self::execute_chunk(
                    task_id,
                    start,
                    end,
                    selectivity_copy,
                )
            });

            handles.push(handle);
        }

        let mut results = vec![];
        for handle in handles {
            if let Ok(result) = handle.join() {
                results.push(result);
            }
        }

        results
    }

    fn execute_chunk(
        task_id: usize,
        start: usize,
        end: usize,
        selectivity: f64,
    ) -> ParallelResult {
        let rows_processed = end - start;
        let rows_passed_filter =
            ((rows_processed as f64) * selectivity).ceil() as usize;

        ParallelResult {
            task_id,
            rows_processed,
            rows_passed_filter,
            execution_time_ms: 1,
        }
    }

    /// Estimate parallel speedup factor
    pub fn estimate_speedup(&self) -> f64 {
        (self.config.worker_threads as f64) * 0.85
    }
}

/// Parallel JOIN executor
pub struct ParallelJoinExecutor {
    config: ParallelConfig,
}

impl ParallelJoinExecutor {
    pub fn new(config: ParallelConfig) -> Self {
        Self { config }
    }

    /// Execute JOIN in parallel
    /// left_rows: size of left table
    /// right_rows: size of right table
    /// selectivity: estimated selectivity of join condition
    pub fn execute_parallel_join(
        &self,
        left_rows: usize,
        right_rows: usize,
        selectivity: f64,
    ) -> JoinResult {
        let start_time = std::time::Instant::now();

        // Partition left table
        let left_chunk_size = (left_rows + self.config.worker_threads - 1)
            / self.config.worker_threads;
        let mut handles = vec![];

        for task_id in 0..self.config.worker_threads {
            let start = task_id * left_chunk_size;
            if start >= left_rows {
                break;
            }
            let end = (start + left_chunk_size).min(left_rows);
            let sel = selectivity;
            let right = right_rows;

            let handle = thread::spawn(move || {
                Self::hash_join_partition(
                    task_id,
                    start,
                    end,
                    right,
                    sel,
                )
            });

            handles.push(handle);
        }

        let mut result_rows = 0;
        for handle in handles {
            if let Ok(rows) = handle.join() {
                result_rows += rows;
            }
        }

        let execution_time_ms =
            start_time.elapsed().as_millis() as u64;

        JoinResult {
            result_rows,
            execution_time_ms,
            tasks_executed: self.config.worker_threads,
            hash_join_used: true,
        }
    }

    fn hash_join_partition(
        _task_id: usize,
        start: usize,
        end: usize,
        right_rows: usize,
        selectivity: f64,
    ) -> usize {
        let left_chunk = end - start;
        let estimated_matches =
            ((left_chunk as f64) * (right_rows as f64) * selectivity)
                as usize;
        estimated_matches
    }

    /// Choose JOIN strategy based on table sizes
    pub fn choose_join_strategy(
        left_rows: usize,
        right_rows: usize,
    ) -> JoinStrategy {
        let total_rows = left_rows + right_rows;

        if total_rows < 10000 {
            JoinStrategy::NestedLoop
        } else if left_rows > right_rows * 100 {
            JoinStrategy::HashJoin
        } else if left_rows < right_rows {
            JoinStrategy::HashJoin
        } else {
            JoinStrategy::SortMerge
        }
    }
}

/// JOIN execution strategies
#[derive(Clone, Debug, PartialEq)]
pub enum JoinStrategy {
    NestedLoop,
    HashJoin,
    SortMerge,
}

/// Result from parallel JOIN execution
#[derive(Clone, Debug, PartialEq)]
pub struct JoinResult {
    pub result_rows: usize,
    pub execution_time_ms: u64,
    pub tasks_executed: usize,
    pub hash_join_used: bool,
}

/// Performance metrics for parallelization
#[derive(Clone, Debug, PartialEq)]
pub struct ParallelMetrics {
    pub sequential_time_ms: u64,
    pub parallel_time_ms: u64,
    pub speedup_factor: f64,
    pub efficiency: f64,
}

impl ParallelMetrics {
    pub fn new(
        seq_time: u64,
        par_time: u64,
        num_workers: usize,
    ) -> Self {
        let speedup =
            (seq_time as f64) / (par_time.max(1) as f64);
        let efficiency = speedup / (num_workers as f64);

        Self {
            sequential_time_ms: seq_time,
            parallel_time_ms: par_time,
            speedup_factor: speedup,
            efficiency,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parallel_config_default() {
        let config = ParallelConfig::default();
        assert!(config.worker_threads > 0);
        assert_eq!(config.chunk_size, 10000);
        assert!(config.enable_parallel_joins);
    }

    #[test]
    fn test_parallel_config_custom() {
        let config = ParallelConfig::new()
            .with_threads(4)
            .with_chunk_size(5000);

        assert_eq!(config.worker_threads, 4);
        assert_eq!(config.chunk_size, 5000);
    }

    #[test]
    fn test_parallel_query_execution() {
        let config = ParallelConfig::new()
            .with_threads(2)
            .with_chunk_size(5000);
        let executor = ParallelQueryExecutor::new(config);

        let results =
            executor.execute_parallel(10000, 0.5);

        assert!(!results.is_empty());
        let total_processed: usize =
            results.iter().map(|r| r.rows_processed).sum();
        assert_eq!(total_processed, 10000);
    }

    #[test]
    fn test_parallel_query_selectivity() {
        let config = ParallelConfig::new()
            .with_threads(2)
            .with_chunk_size(1000);
        let executor = ParallelQueryExecutor::new(config);

        let results =
            executor.execute_parallel(1000, 0.3);

        let total_passed: usize =
            results.iter().map(|r| r.rows_passed_filter).sum();
        assert!(total_passed < 1000);
        assert!(total_passed > 0);
    }

    #[test]
    fn test_parallel_speedup_estimation() {
        let config = ParallelConfig::new().with_threads(4);
        let executor = ParallelQueryExecutor::new(config);

        let speedup = executor.estimate_speedup();
        assert!(speedup > 1.0);
        assert!(speedup <= 4.0);
    }

    #[test]
    fn test_parallel_join_execution() {
        let config = ParallelConfig::new().with_threads(2);
        let executor = ParallelJoinExecutor::new(config);

        let result =
            executor.execute_parallel_join(1000, 100, 0.5);

        assert!(result.result_rows > 0);
        assert_eq!(result.hash_join_used, true);
        assert_eq!(result.tasks_executed, 2);
    }

    #[test]
    fn test_join_strategy_small_tables() {
        let strategy =
            ParallelJoinExecutor::choose_join_strategy(100, 100);
        assert_eq!(strategy, JoinStrategy::NestedLoop);
    }

    #[test]
    fn test_join_strategy_large_imbalanced() {
        let strategy =
            ParallelJoinExecutor::choose_join_strategy(100000, 100);
        assert_eq!(strategy, JoinStrategy::HashJoin);
    }

    #[test]
    fn test_join_strategy_medium_tables() {
        let strategy =
            ParallelJoinExecutor::choose_join_strategy(10000, 5000);
        assert_eq!(strategy, JoinStrategy::SortMerge);
    }

    #[test]
    fn test_parallel_metrics() {
        let metrics =
            ParallelMetrics::new(1000, 300, 4);

        assert_eq!(metrics.sequential_time_ms, 1000);
        assert_eq!(metrics.parallel_time_ms, 300);
        assert!(metrics.speedup_factor > 3.0);
        assert!(metrics.speedup_factor < 4.0);
        assert!(metrics.efficiency > 0.7);
    }

    #[test]
    fn test_parallel_result_structure() {
        let result = ParallelResult {
            task_id: 0,
            rows_processed: 1000,
            rows_passed_filter: 500,
            execution_time_ms: 10,
        };

        assert_eq!(result.task_id, 0);
        assert_eq!(result.rows_processed, 1000);
        assert_eq!(result.rows_passed_filter, 500);
    }

    #[test]
    fn test_join_result_structure() {
        let result = JoinResult {
            result_rows: 5000,
            execution_time_ms: 50,
            tasks_executed: 4,
            hash_join_used: true,
        };

        assert_eq!(result.result_rows, 5000);
        assert!(result.hash_join_used);
    }
}
