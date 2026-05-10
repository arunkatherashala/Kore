/// Baseline performance benchmarking and measurement
///
/// Provides infrastructure for measuring query performance,
/// comparing optimization techniques, and tracking improvements.

use std::time::Instant;
use std::collections::HashMap;

/// Performance metric for a single operation
#[derive(Clone, Debug, PartialEq)]
pub struct PerformanceMetric {
    pub operation_name: String,
    pub duration_ms: u64,
    pub throughput_ops_per_sec: f64,
    pub memory_peak_mb: f64,
}

impl PerformanceMetric {
    pub fn new(
        name: &str,
        duration_ms: u64,
        ops: usize,
        peak_mem_mb: f64,
    ) -> Self {
        let throughput = if duration_ms > 0 {
            (ops as f64) / (duration_ms as f64) * 1000.0
        } else {
            0.0
        };

        Self {
            operation_name: name.to_string(),
            duration_ms,
            throughput_ops_per_sec: throughput,
            memory_peak_mb: peak_mem_mb,
        }
    }
}

/// Benchmark session for measuring operation performance
pub struct BenchmarkSession {
    name: String,
    start_time: Instant,
    operations_count: usize,
    memory_peak: f64,
}

impl BenchmarkSession {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            start_time: Instant::now(),
            operations_count: 0,
            memory_peak: 0.0,
        }
    }

    pub fn record_operation(&mut self) {
        self.operations_count += 1;
    }

    pub fn record_operations(&mut self, count: usize) {
        self.operations_count += count;
    }

    pub fn set_peak_memory(&mut self, mb: f64) {
        self.memory_peak = self.memory_peak.max(mb);
    }

    pub fn finish(&self) -> PerformanceMetric {
        let duration = self.start_time.elapsed().as_millis() as u64;

        PerformanceMetric::new(
            &self.name,
            duration,
            self.operations_count,
            self.memory_peak,
        )
    }
}

/// Baseline performance metrics
#[derive(Clone, Debug, PartialEq)]
pub struct BaselineMetrics {
    pub query_name: String,
    pub sequential_time_ms: u64,
    pub rows_processed: usize,
    pub selectivity: f64,
    pub memory_mb: f64,
}

impl BaselineMetrics {
    pub fn new(
        query: &str,
        time_ms: u64,
        rows: usize,
        sel: f64,
        mem: f64,
    ) -> Self {
        Self {
            query_name: query.to_string(),
            sequential_time_ms: time_ms,
            rows_processed: rows,
            selectivity: sel,
            memory_mb: mem,
        }
    }

    pub fn throughput_rows_per_sec(&self) -> f64 {
        if self.sequential_time_ms > 0 {
            (self.rows_processed as f64)
                / (self.sequential_time_ms as f64)
                * 1000.0
        } else {
            0.0
        }
    }
}

/// Optimization comparison result
#[derive(Clone, Debug, PartialEq)]
pub struct OptimizationComparison {
    pub operation: String,
    pub baseline_ms: u64,
    pub optimized_ms: u64,
    pub speedup_factor: f64,
    pub improvement_percent: f64,
    pub memory_savings_mb: f64,
}

impl OptimizationComparison {
    pub fn new(
        op: &str,
        baseline: u64,
        optimized: u64,
        mem_saved: f64,
    ) -> Self {
        let speedup = if optimized > 0 {
            (baseline as f64) / (optimized as f64)
        } else {
            0.0
        };

        let improvement =
            ((baseline as f64 - optimized as f64)
                / (baseline as f64))
                * 100.0;

        Self {
            operation: op.to_string(),
            baseline_ms: baseline,
            optimized_ms: optimized,
            speedup_factor: speedup,
            improvement_percent: improvement,
            memory_savings_mb: mem_saved,
        }
    }

    pub fn is_improvement(&self) -> bool {
        self.speedup_factor > 1.0
    }
}

/// Query performance baseline
#[derive(Clone, Debug)]
pub struct QueryBaseline {
    pub query_id: usize,
    pub query_text: String,
    pub metrics: BaselineMetrics,
}

/// Baseline performance tracker
pub struct BaselineTracker {
    baselines: HashMap<String, BaselineMetrics>,
    comparisons: Vec<OptimizationComparison>,
}

impl BaselineTracker {
    pub fn new() -> Self {
        Self {
            baselines: HashMap::new(),
            comparisons: Vec::new(),
        }
    }

    /// Record baseline metric
    pub fn record_baseline(&mut self, metrics: BaselineMetrics) {
        self.baselines
            .insert(metrics.query_name.clone(), metrics);
    }

    /// Get baseline for query
    pub fn get_baseline(&self, query: &str) -> Option<&BaselineMetrics> {
        self.baselines.get(query)
    }

    /// Record optimization comparison
    pub fn record_comparison(&mut self, comparison: OptimizationComparison) {
        self.comparisons.push(comparison);
    }

    /// Get average speedup across all comparisons
    pub fn average_speedup(&self) -> f64 {
        if self.comparisons.is_empty() {
            return 1.0;
        }

        let total: f64 =
            self.comparisons.iter().map(|c| c.speedup_factor).sum();
        total / (self.comparisons.len() as f64)
    }

    /// Get total memory savings
    pub fn total_memory_savings(&self) -> f64 {
        self.comparisons
            .iter()
            .map(|c| c.memory_savings_mb)
            .sum()
    }

    /// Get improvement summary
    pub fn improvement_summary(&self) -> ImprovementSummary {
        let total_ops = self.comparisons.len();
        let improved = self
            .comparisons
            .iter()
            .filter(|c| c.is_improvement())
            .count();

        let avg_improvement = if !self.comparisons.is_empty() {
            self.comparisons
                .iter()
                .map(|c| c.improvement_percent)
                .sum::<f64>()
                / (self.comparisons.len() as f64)
        } else {
            0.0
        };

        ImprovementSummary {
            total_optimizations: total_ops,
            improvements_found: improved,
            improvement_rate: if total_ops > 0 {
                (improved as f64) / (total_ops as f64) * 100.0
            } else {
                0.0
            },
            average_improvement_percent: avg_improvement,
            average_speedup: self.average_speedup(),
            total_memory_savings_mb: self.total_memory_savings(),
        }
    }
}

impl Default for BaselineTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Performance improvement summary
#[derive(Clone, Debug, PartialEq)]
pub struct ImprovementSummary {
    pub total_optimizations: usize,
    pub improvements_found: usize,
    pub improvement_rate: f64,
    pub average_improvement_percent: f64,
    pub average_speedup: f64,
    pub total_memory_savings_mb: f64,
}

/// Sequential vs parallel performance comparison
#[derive(Clone, Debug, PartialEq)]
pub struct ParallelizationComparison {
    pub query_name: String,
    pub sequential_ms: u64,
    pub parallel_ms: u64,
    pub num_workers: usize,
    pub speedup: f64,
    pub efficiency: f64,
}

impl ParallelizationComparison {
    pub fn new(
        query: &str,
        seq_ms: u64,
        par_ms: u64,
        workers: usize,
    ) -> Self {
        let speedup = if par_ms > 0 {
            (seq_ms as f64) / (par_ms as f64)
        } else {
            0.0
        };

        let efficiency = if workers > 0 {
            speedup / (workers as f64)
        } else {
            0.0
        };

        Self {
            query_name: query.to_string(),
            sequential_ms: seq_ms,
            parallel_ms: par_ms,
            num_workers: workers,
            speedup,
            efficiency,
        }
    }
}

/// Memory pooling impact measurement
#[derive(Clone, Debug, PartialEq)]
pub struct MemoryPoolingImpact {
    pub operation: String,
    pub without_pooling_mb: f64,
    pub with_pooling_mb: f64,
    pub memory_savings_mb: f64,
    pub savings_percent: f64,
    pub allocation_count_without: usize,
    pub allocation_count_with: usize,
    pub allocations_avoided: usize,
}

impl MemoryPoolingImpact {
    pub fn new(
        op: &str,
        without_mb: f64,
        with_mb: f64,
        allocs_without: usize,
        allocs_with: usize,
    ) -> Self {
        let savings = without_mb - with_mb;
        let savings_percent = (savings / without_mb) * 100.0;

        Self {
            operation: op.to_string(),
            without_pooling_mb: without_mb,
            with_pooling_mb: with_mb,
            memory_savings_mb: savings,
            savings_percent,
            allocation_count_without: allocs_without,
            allocation_count_with: allocs_with,
            allocations_avoided: allocs_without - allocs_with,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_metric() {
        let metric =
            PerformanceMetric::new("test_query", 1000, 10000, 50.0);

        assert_eq!(metric.operation_name, "test_query");
        assert_eq!(metric.duration_ms, 1000);
        assert!(metric.throughput_ops_per_sec > 0.0);
    }

    #[test]
    fn test_benchmark_session() {
        let mut session = BenchmarkSession::new("query_test");
        session.record_operations(1000);
        session.set_peak_memory(100.0);

        let metric = session.finish();
        assert_eq!(metric.operation_name, "query_test");
        assert_eq!(metric.memory_peak_mb, 100.0);
    }

    #[test]
    fn test_baseline_metrics() {
        let baseline = BaselineMetrics::new("q1", 500, 10000, 0.5, 75.0);

        assert_eq!(baseline.query_name, "q1");
        assert_eq!(baseline.sequential_time_ms, 500);
        assert!(baseline.throughput_rows_per_sec() > 0.0);
    }

    #[test]
    fn test_optimization_comparison() {
        let comp =
            OptimizationComparison::new("join_opt", 1000, 400, 20.0);

        assert!(comp.is_improvement());
        assert!(comp.speedup_factor > 1.0);
        assert!(comp.improvement_percent > 0.0);
    }

    #[test]
    fn test_baseline_tracker() {
        let mut tracker = BaselineTracker::new();
        let baseline1 =
            BaselineMetrics::new("q1", 100, 1000, 0.5, 50.0);
        let baseline2 =
            BaselineMetrics::new("q2", 200, 2000, 0.3, 75.0);

        tracker.record_baseline(baseline1);
        tracker.record_baseline(baseline2);

        assert!(tracker.get_baseline("q1").is_some());
        assert!(tracker.get_baseline("q2").is_some());
    }

    #[test]
    fn test_baseline_tracker_comparisons() {
        let mut tracker = BaselineTracker::new();
        tracker.record_comparison(OptimizationComparison::new(
            "op1", 1000, 500, 10.0,
        ));
        tracker.record_comparison(OptimizationComparison::new(
            "op2", 800, 200, 20.0,
        ));

        let summary = tracker.improvement_summary();
        assert_eq!(summary.total_optimizations, 2);
        assert_eq!(summary.improvements_found, 2);
        assert!(summary.average_speedup > 1.0);
    }

    #[test]
    fn test_improvement_summary() {
        let summary = ImprovementSummary {
            total_optimizations: 10,
            improvements_found: 8,
            improvement_rate: 80.0,
            average_improvement_percent: 40.0,
            average_speedup: 2.5,
            total_memory_savings_mb: 500.0,
        };

        assert_eq!(summary.improvement_rate, 80.0);
        assert!(summary.average_speedup > 1.0);
    }

    #[test]
    fn test_parallelization_comparison() {
        let comp =
            ParallelizationComparison::new("query1", 1000, 300, 4);

        assert!(comp.speedup > 1.0);
        assert!(comp.efficiency > 0.0);
        assert!(comp.efficiency <= 1.0);
    }

    #[test]
    fn test_memory_pooling_impact() {
        let impact = MemoryPoolingImpact::new(
            "batch_insert",
            100.0,
            30.0,
            1000,
            100,
        );

        assert_eq!(impact.allocations_avoided, 900);
        assert!(impact.savings_percent > 0.0);
        assert!(impact.memory_savings_mb > 0.0);
    }

    #[test]
    fn test_query_baseline() {
        let baseline = QueryBaseline {
            query_id: 1,
            query_text: "SELECT * FROM users".to_string(),
            metrics: BaselineMetrics::new("q", 100, 1000, 1.0, 50.0),
        };

        assert_eq!(baseline.query_id, 1);
        assert!(!baseline.query_text.is_empty());
    }

    #[test]
    fn test_average_speedup_empty_tracker() {
        let tracker = BaselineTracker::new();
        assert_eq!(tracker.average_speedup(), 1.0);
    }

    #[test]
    fn test_average_speedup_with_comparisons() {
        let mut tracker = BaselineTracker::new();
        tracker.record_comparison(OptimizationComparison::new(
            "op1", 1000, 500, 0.0,
        ));
        tracker.record_comparison(OptimizationComparison::new(
            "op2", 1000, 250, 0.0,
        ));

        let avg = tracker.average_speedup();
        assert!(avg > 1.5);
    }
}
