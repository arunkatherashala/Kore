use kore_benchmark::{
    BenchmarkConfig, BenchmarkComparison, Engine, Dataset,
    tpch,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("╔════════════════════════════════════════════════════════════════════╗");
    eprintln!("║           KORE TPC-H Benchmark (v1.0.0 Validation)                 ║");
    eprintln!("╚════════════════════════════════════════════════════════════════════╝\n");
    
    // Run TPC-H on KORE
    let config = BenchmarkConfig::new(Engine::Kore, Dataset::TpcH)
        .with_scale(1.0);
    
    eprintln!("Running TPC-H SF=1 (22 queries) on KORE...\n");
    let results = tpch::run_tpch_benchmark(&config).await?;
    
    // Build comparison for stats
    let mut comparison = BenchmarkComparison::new();
    for result in results {
        comparison.add_result(result);
    }
    
    println!("{}", comparison.generate_report());
    
    // Export results
    std::fs::create_dir_all("benchmark_results")?;
    comparison.to_csv("benchmark_results/tpch_results.csv")?;
    comparison.to_json("benchmark_results/tpch_results.json")?;
    
    eprintln!("✅ Results saved to benchmark_results/");
    
    Ok(())
}
