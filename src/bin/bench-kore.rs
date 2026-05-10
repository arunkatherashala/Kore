/// KORE Benchmark Runner
/// Comprehensive performance testing binary

use kore_fileformat::benchmarks::{BenchmarkEngine, BenchmarkResult, QueryBenchmark};
use std::env;

fn main() {
    println!("\n🔬 KORE Performance Benchmark Suite");
    println!("═══════════════════════════════════════════════════════════════\n");

    // Get sample files
    let sample_files = vec![
        "tools/sample_10mb.kore",
        "tools/sample_small.kore",
    ];

    // Run compression benchmarks
    println!("📊 Compression Benchmarks");
    println!("───────────────────────────────────────────────────────────────");
    
    let results = BenchmarkEngine::benchmark_files(sample_files);
    BenchmarkEngine::print_report(&results);

    // Export to CSV
    if let Err(e) = BenchmarkEngine::export_csv(&results, "benchmark_results.csv") {
        eprintln!("Warning: Could not export CSV: {}", e);
    } else {
        println!("✓ Benchmark results exported to benchmark_results.csv");
    }

    // Compare compression formats
    println!("\n📈 Format Comparison");
    println!("───────────────────────────────────────────────────────────────");
    let comparison = BenchmarkEngine::compare_formats();
    comparison.print_comparison();

    // Query benchmarks
    println!("⚡ Query Performance Benchmarks");
    println!("───────────────────────────────────────────────────────────────\n");

    let queries = vec![
        ("SELECT * FROM data LIMIT 1000", 1_000),
        ("SELECT col1, col2 FROM data WHERE col3 > 100", 500_000),
        ("SELECT * FROM data ORDER BY col1 LIMIT 100", 10_000_000),
        ("SELECT col1, SUM(col2) FROM data GROUP BY col1", 5_000),
    ];

    for (query, estimated_rows) in queries {
        let benchmark = QueryBenchmark::benchmark_query(query, estimated_rows);
        println!("Query: {}", query);
        println!("  Rows Processed:  {}", benchmark.rows_processed);
        println!("  Execution Time:  {:.2} ms", benchmark.execution_time_ms);
        println!("  Throughput:      {:.0} rows/sec\n", benchmark.throughput_rows_per_sec);
    }

    println!("═══════════════════════════════════════════════════════════════");
    println!("✅ Benchmarks Complete!");
    println!("═══════════════════════════════════════════════════════════════\n");
}
