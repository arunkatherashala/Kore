use kore_benchmark::{
    BenchmarkConfig, BenchmarkComparison, Engine, Dataset,
    tpch,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("╔════════════════════════════════════════════════════════════════════╗");
    eprintln!("║      KORE vs Spark vs DuckDB: TPC-H Benchmark Comparison            ║");
    eprintln!("╚════════════════════════════════════════════════════════════════════╝\n");
    
    let mut comparison = BenchmarkComparison::new();
    
    // Run TPC-H on KORE
    eprintln!("[1/3] Running TPC-H SF=1 on KORE...");
    let kore_config = BenchmarkConfig::new(Engine::Kore, Dataset::TpcH)
        .with_scale(1.0);
    let kore_results = tpch::run_tpch_benchmark(&kore_config).await?;
    for result in kore_results {
        comparison.add_result(result);
    }
    
    // Run TPC-H on Spark
    eprintln!("[2/3] Running TPC-H SF=1 on Spark...");
    let spark_config = BenchmarkConfig::new(Engine::Spark, Dataset::TpcH)
        .with_scale(1.0);
    let spark_results = tpch::run_tpch_benchmark(&spark_config).await?;
    for result in spark_results {
        comparison.add_result(result);
    }
    
    // Run TPC-H on DuckDB
    eprintln!("[3/3] Running TPC-H SF=1 on DuckDB...");
    let duckdb_config = BenchmarkConfig::new(Engine::DuckDB, Dataset::TpcH)
        .with_scale(1.0);
    let duckdb_results = tpch::run_tpch_benchmark(&duckdb_config).await?;
    for result in duckdb_results {
        comparison.add_result(result);
    }
    
    // Generate and print report
    let report = comparison.generate_report();
    println!("{}", report);
    
    // Export results
    let output_dir = "benchmark_results";
    std::fs::create_dir_all(output_dir)?;
    
    comparison.to_csv(&format!("{}/results.csv", output_dir))?;
    comparison.to_json(&format!("{}/results.json", output_dir))?;
    comparison.to_html(&format!("{}/report.html", output_dir))?;
    
    eprintln!("\n✅ Results exported to: {}/", output_dir);
    eprintln!("   - results.csv (tabular data)");
    eprintln!("   - results.json (JSON format)");
    eprintln!("   - report.html (interactive dashboard)");
    
    Ok(())
}
