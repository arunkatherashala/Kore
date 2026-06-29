//! kore-bench — Layer 20: KORE vs Spark performance benchmark suite.
//!
//! Outputs results to stdout as a formatted table, and writes
//! `kore_bench.json` and `kore_bench.csv` to the current directory.

mod suite;
mod reporter;

fn main() {
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║    KORE Benchmark Suite — Layers 15-25 + World Comparison   ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let results = rt.block_on(suite::run_all());

    reporter::print_table(&results);

    match reporter::save_json(&results, "kore_bench.json") {
        Ok(_)  => println!("\n  Results saved → kore_bench.json"),
        Err(e) => eprintln!("  Warning: could not save JSON: {}", e),
    }
    match reporter::save_csv(&results, "kore_bench.csv") {
        Ok(_)  => println!("  Results saved → kore_bench.csv"),
        Err(e) => eprintln!("  Warning: could not save CSV: {}", e),
    }
    println!();
}
