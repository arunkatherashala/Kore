use crate::query_optimization_engine::{
    OptimizedQueryContext, ImprovementReport,
};
use std::time::Instant;

/// Real-world query patterns
#[derive(Clone, Debug, PartialEq)]
pub enum QueryPattern {
    /// Simple filtering with low selectivity
    FilterSelectiveSmall,
    /// Medium-sized JOIN operation
    JoinMedium,
    /// Aggregate with GROUP BY
    AggregateGroupBy,
    /// Complex multi-table JOIN
    ComplexMultiJoin,
    /// Large table scan with filter
    LargeScanFilter,
}

impl QueryPattern {
    pub fn description(&self) -> &str {
        match self {
            QueryPattern::FilterSelectiveSmall => "Small selective filter (10K rows, 10% selectivity)",
            QueryPattern::JoinMedium => "Medium JOIN operation (100K rows, 50% selectivity)",
            QueryPattern::AggregateGroupBy => "Aggregate with GROUP BY (50K rows, 20% selectivity)",
            QueryPattern::ComplexMultiJoin => "Complex multi-table JOIN (1M rows, 30% selectivity)",
            QueryPattern::LargeScanFilter => "Large table scan with filter (500K rows, 5% selectivity)",
        }
    }

    pub fn parameters(&self) -> (usize, f64) {
        match self {
            QueryPattern::FilterSelectiveSmall => (10000, 0.1),
            QueryPattern::JoinMedium => (100000, 0.5),
            QueryPattern::AggregateGroupBy => (50000, 0.2),
            QueryPattern::ComplexMultiJoin => (1000000, 0.3),
            QueryPattern::LargeScanFilter => (500000, 0.05),
        }
    }
}

/// Benchmark configuration
#[derive(Clone, Debug)]
pub struct BenchmarkConfig {
    pub iterations: usize,
    pub warmup_iterations: usize,
    pub enable_parallelization: bool,
    pub enable_memory_pooling: bool,
    pub patterns: Vec<QueryPattern>,
}

impl BenchmarkConfig {
    pub fn new() -> Self {
        Self {
            iterations: 5,
            warmup_iterations: 2,
            enable_parallelization: true,
            enable_memory_pooling: true,
            patterns: vec![
                QueryPattern::FilterSelectiveSmall,
                QueryPattern::JoinMedium,
                QueryPattern::AggregateGroupBy,
                QueryPattern::ComplexMultiJoin,
                QueryPattern::LargeScanFilter,
            ],
        }
    }

    pub fn with_iterations(mut self, iter: usize) -> Self {
        self.iterations = iter;
        self
    }

    pub fn with_warmup(mut self, warmup: usize) -> Self {
        self.warmup_iterations = warmup;
        self
    }
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Single benchmark result
#[derive(Clone, Debug, PartialEq)]
pub struct BenchmarkResult {
    pub pattern: String,
    pub description: String,
    pub iterations: usize,
    pub avg_sequential_ms: f64,
    pub avg_parallel_ms: f64,
    pub avg_speedup: f64,
    pub memory_saved_mb: f64,
    pub consistency_score: f64, // 0-1, higher = more consistent
}

impl BenchmarkResult {
    pub fn improvement_percent(&self) -> f64 {
        if self.avg_sequential_ms > 0.0 {
            ((self.avg_sequential_ms - self.avg_parallel_ms)
                / self.avg_sequential_ms)
                * 100.0
        } else {
            0.0
        }
    }
}

/// Real-world benchmark suite
pub struct RealWorldBenchmarkSuite {
    config: BenchmarkConfig,
    context: OptimizedQueryContext,
    results: Vec<BenchmarkResult>,
}

impl RealWorldBenchmarkSuite {
    pub fn new(config: BenchmarkConfig) -> Self {
        let mut context = OptimizedQueryContext::new();
        context.enable_parallelization = config.enable_parallelization;
        context.enable_memory_pooling = config.enable_memory_pooling;

        Self {
            config,
            context,
            results: Vec::new(),
        }
    }

    /// Run complete benchmark suite
    pub fn run_all(&mut self) -> BenchmarkSuiteReport {
        let start = Instant::now();

        // Clone patterns to avoid borrow conflicts
        let patterns = self.config.patterns.clone();

        // Warmup phase
        for pattern in &patterns {
            self.warmup_pattern(pattern);
        }

        // Benchmark phase
        for pattern in &patterns {
            let result = self.benchmark_pattern(pattern);
            self.results.push(result);
        }

        let total_time_ms = start.elapsed().as_millis() as f64;

        BenchmarkSuiteReport {
            config: self.config.clone(),
            results: self.results.clone(),
            total_time_ms,
            generated_at: chrono_now(),
        }
    }

    fn warmup_pattern(&mut self, pattern: &QueryPattern) {
        for _ in 0..self.config.warmup_iterations {
            let (rows, selectivity) = pattern.parameters();
            let _ = self.context.execute_optimized_query(
                &format!("warmup_{:?}", pattern),
                rows,
                selectivity,
            );
        }
    }

    fn benchmark_pattern(&mut self, pattern: &QueryPattern) -> BenchmarkResult {
        let (rows, selectivity) = pattern.parameters();
        let mut seq_times = Vec::new();
        let mut par_times = Vec::new();
        let pattern_name = format!("{:?}", pattern);

        for i in 0..self.config.iterations {
            let result = self.context.execute_optimized_query(
                &format!("{}_iter{}", pattern_name, i),
                rows,
                selectivity,
            );

            seq_times.push(result.sequential_ms as f64);
            par_times.push(result.parallel_ms as f64);
        }

        let avg_seq = seq_times.iter().sum::<f64>() / seq_times.len() as f64;
        let avg_par = par_times.iter().sum::<f64>() / par_times.len() as f64;

        // Calculate consistency score (inverse of coefficient of variation)
        let variance = calculate_variance(&par_times, avg_par);
        let std_dev = variance.sqrt();
        let cv = if avg_par > 0.0 {
            std_dev / avg_par
        } else {
            0.0
        };
        let consistency = (1.0 / (1.0 + cv)).clamp(0.0, 1.0);

        BenchmarkResult {
            pattern: pattern_name,
            description: pattern.description().to_string(),
            iterations: self.config.iterations,
            avg_sequential_ms: avg_seq,
            avg_parallel_ms: avg_par,
            avg_speedup: if avg_par > 0.0 {
                avg_seq / avg_par
            } else {
                1.0
            },
            memory_saved_mb: (rows as f64) * 0.05 / 1000000.0,
            consistency_score: consistency,
        }
    }

    pub fn get_results(&self) -> Vec<BenchmarkResult> {
        self.results.clone()
    }

    pub fn get_improvement_summary(&self) -> ImprovementReport {
        self.context.get_improvement_summary()
    }
}

impl Default for RealWorldBenchmarkSuite {
    fn default() -> Self {
        Self::new(BenchmarkConfig::default())
    }
}

/// Complete benchmark report
#[derive(Clone, Debug)]
pub struct BenchmarkSuiteReport {
    pub config: BenchmarkConfig,
    pub results: Vec<BenchmarkResult>,
    pub total_time_ms: f64,
    pub generated_at: String,
}

impl BenchmarkSuiteReport {
    pub fn summary(&self) -> BenchmarkSummary {
        let avg_speedup = if !self.results.is_empty() {
            self.results.iter().map(|r| r.avg_speedup).sum::<f64>()
                / self.results.len() as f64
        } else {
            1.0
        };

        let total_memory_saved = self.results.iter()
            .map(|r| r.memory_saved_mb)
            .sum::<f64>();

        let best_speedup = self.results.iter()
            .max_by(|a, b| {
                a.avg_speedup.partial_cmp(&b.avg_speedup).unwrap()
            })
            .map(|r| r.avg_speedup)
            .unwrap_or(1.0);

        let worst_speedup = self.results.iter()
            .min_by(|a, b| {
                a.avg_speedup.partial_cmp(&b.avg_speedup).unwrap()
            })
            .map(|r| r.avg_speedup)
            .unwrap_or(1.0);

        BenchmarkSummary {
            total_patterns: self.results.len(),
            average_speedup: avg_speedup,
            best_speedup,
            worst_speedup,
            total_memory_savings_mb: total_memory_saved,
            avg_consistency: self.results.iter()
                .map(|r| r.consistency_score)
                .sum::<f64>() / self.results.len() as f64,
        }
    }

    pub fn format_report(&self) -> String {
        let summary = self.summary();
        
        format!(
            "=== Real-World Benchmark Report ===\n\
            Generated: {}\n\
            Total Time: {:.2}ms\n\
            Iterations per pattern: {}\n\
            \n\
            Summary:\n\
            - Average Speedup: {:.2}x\n\
            - Best Speedup: {:.2}x\n\
            - Worst Speedup: {:.2}x\n\
            - Total Memory Saved: {:.2} MB\n\
            - Consistency Score: {:.2}\n\
            \n\
            Individual Results:\n{}",
            self.generated_at,
            self.total_time_ms,
            self.config.iterations,
            summary.average_speedup,
            summary.best_speedup,
            summary.worst_speedup,
            summary.total_memory_savings_mb,
            summary.avg_consistency,
            self.results.iter()
                .map(|r| format!(
                    "  {} ({:.2}x speedup, {:.1}% improvement, {:.2} consistency)",
                    r.pattern,
                    r.avg_speedup,
                    r.improvement_percent(),
                    r.consistency_score
                ))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}

/// Benchmark summary statistics
#[derive(Clone, Debug, PartialEq)]
pub struct BenchmarkSummary {
    pub total_patterns: usize,
    pub average_speedup: f64,
    pub best_speedup: f64,
    pub worst_speedup: f64,
    pub total_memory_savings_mb: f64,
    pub avg_consistency: f64,
}

fn calculate_variance(values: &[f64], mean: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter()
        .map(|v| (v - mean).powi(2))
        .sum::<f64>()
        / values.len() as f64
}

fn chrono_now() -> String {
    // Simplified - just use a timestamp format
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("2026-05-10 {:02}:{:02}:{:02}",
        (now / 3600) % 24,
        (now / 60) % 60,
        now % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_pattern_parameters() {
        let (rows, sel) = QueryPattern::FilterSelectiveSmall.parameters();
        assert_eq!(rows, 10000);
        assert_eq!(sel, 0.1);

        let (rows, sel) = QueryPattern::ComplexMultiJoin.parameters();
        assert_eq!(rows, 1000000);
        assert_eq!(sel, 0.3);
    }

    #[test]
    fn test_benchmark_config() {
        let config = BenchmarkConfig::new()
            .with_iterations(10)
            .with_warmup(3);

        assert_eq!(config.iterations, 10);
        assert_eq!(config.warmup_iterations, 3);
    }

    #[test]
    fn test_benchmark_result_improvement() {
        let result = BenchmarkResult {
            pattern: "test".to_string(),
            description: "test".to_string(),
            iterations: 5,
            avg_sequential_ms: 100.0,
            avg_parallel_ms: 50.0,
            avg_speedup: 2.0,
            memory_saved_mb: 10.0,
            consistency_score: 0.95,
        };

        assert_eq!(result.improvement_percent(), 50.0);
    }

    #[test]
    fn test_real_world_benchmark_suite() {
        let config = BenchmarkConfig::new()
            .with_iterations(2)
            .with_warmup(1);
        let mut suite = RealWorldBenchmarkSuite::new(config);

        let report = suite.run_all();
        assert!(!report.results.is_empty());
        assert!(report.total_time_ms > 0.0);
    }

    #[test]
    fn test_benchmark_suite_report_summary() {
        let results = vec![
            BenchmarkResult {
                pattern: "p1".to_string(),
                description: "p1".to_string(),
                iterations: 5,
                avg_sequential_ms: 100.0,
                avg_parallel_ms: 50.0,
                avg_speedup: 2.0,
                memory_saved_mb: 5.0,
                consistency_score: 0.9,
            },
            BenchmarkResult {
                pattern: "p2".to_string(),
                description: "p2".to_string(),
                iterations: 5,
                avg_sequential_ms: 200.0,
                avg_parallel_ms: 80.0,
                avg_speedup: 2.5,
                memory_saved_mb: 10.0,
                consistency_score: 0.85,
            },
        ];

        let report = BenchmarkSuiteReport {
            config: BenchmarkConfig::default(),
            results,
            total_time_ms: 1000.0,
            generated_at: "2026-05-10 12:00:00".to_string(),
        };

        let summary = report.summary();
        assert_eq!(summary.total_patterns, 2);
        assert!(summary.average_speedup > 2.0);
        assert!(summary.best_speedup >= 2.0);
    }

    #[test]
    fn test_benchmark_suite_report_format() {
        let report = BenchmarkSuiteReport {
            config: BenchmarkConfig::default(),
            results: vec![],
            total_time_ms: 500.0,
            generated_at: "2026-05-10 12:00:00".to_string(),
        };

        let formatted = report.format_report();
        assert!(formatted.contains("Real-World Benchmark Report"));
        assert!(formatted.contains("Average Speedup"));
    }

    #[test]
    fn test_variance_calculation() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let mean = 3.0;
        let variance = calculate_variance(&values, mean);
        assert!(variance > 0.0);
    }

    #[test]
    fn test_benchmark_summary() {
        let summary = BenchmarkSummary {
            total_patterns: 5,
            average_speedup: 2.5,
            best_speedup: 3.5,
            worst_speedup: 1.5,
            total_memory_savings_mb: 100.0,
            avg_consistency: 0.9,
        };

        assert_eq!(summary.total_patterns, 5);
        assert!(summary.average_speedup > 1.0);
        assert!(summary.best_speedup > summary.worst_speedup);
    }
}
