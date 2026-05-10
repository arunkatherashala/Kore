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

    // Filter to only existing files
    let existing_files: Vec<&str> = sample_files
        .into_iter()
        .filter(|f| std::path::Path::new(f).exists())
        .collect();

    if existing_files.is_empty() {
        println!("⚠️  No sample KORE files found. Create sample files in tools/ directory:");
        println!("   - tools/sample_10mb.kore");
        println!("   - tools/sample_small.kore\n");
    } else {
        // Run compression benchmarks
        println!("📊 Compression Benchmarks");
        println!("───────────────────────────────────────────────────────────────");
        
        let results = BenchmarkEngine::benchmark_files(existing_files.clone());
        BenchmarkEngine::print_report(&results);

        // Export to CSV
        if let Err(e) = BenchmarkEngine::export_csv(&results, "benchmark_results.csv") {
            eprintln!("Warning: Could not export CSV: {}", e);
        } else {
            println!("✓ Benchmark results exported to benchmark_results.csv");
        }

        // Detailed reports
        println!("\n📋 Detailed Benchmark Reports");
        println!("───────────────────────────────────────────────────────────────\n");
        
        for file in existing_files {
            match BenchmarkEngine::benchmark_kore_detailed(file) {
                Ok(bench) => {
                    println!("File: {}", bench.filename);
                    println!("  Original Size:  {:.2} MB", bench.original_size_mb);
                    println!("  Compressed:     {:.2} MB ({:.1}% saved)", 
                        bench.compressed_size_mb, bench.compression_percentage);
                    println!("  Throughput:     {:.2} MB/s", bench.throughput_mbps);
                    println!("  Est. Rows:      {}\n", bench.estimated_rows);
                }
                Err(e) => eprintln!("Error benchmarking {}: {}", file, e),
            }
        }
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
