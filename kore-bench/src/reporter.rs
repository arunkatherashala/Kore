//! Reporting — pretty-print table + JSON/CSV export.

use std::fs::File;
use std::io::{BufWriter, Write};
use crate::suite::BenchResult;

pub fn print_table(results: &[BenchResult]) {
    println!();
    println!("  {:<14} {:<42} {:>8} {:>12} {:>12} {:>9}",
        "Layer", "Operation", "Rows", "KORE ms", "JVM est ms", "Speedup");
    println!("  {}", "─".repeat(105));
    for r in results {
        println!("  {:<14} {:<42} {:>8} {:>12.2} {:>12.2} {:>8.1}×",
            r.layer, r.operation, r.rows, r.kore_ms, r.jvm_est_ms, r.speedup);
    }
    println!("  {}", "─".repeat(105));

    let avg_speedup = results.iter().map(|r| r.speedup).sum::<f64>() / results.len() as f64;
    println!("  Average estimated speedup over JVM/Spark: {:.1}×", avg_speedup);
    println!();
}

pub fn save_json(results: &[BenchResult], path: &str) -> std::io::Result<()> {
    let f = File::create(path)?;
    let mut w = BufWriter::new(f);
    let json = serde_json::to_string_pretty(results).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    w.write_all(json.as_bytes())?;
    Ok(())
}

pub fn save_csv(results: &[BenchResult], path: &str) -> std::io::Result<()> {
    let f = File::create(path)?;
    let mut w = BufWriter::new(f);
    writeln!(w, "layer,operation,rows,kore_ms,jvm_est_ms,speedup,ops_per_sec")?;
    for r in results {
        writeln!(w, "{},{},{},{:.4},{:.4},{:.2},{:.0}",
            r.layer, r.operation.replace(',', ";"),
            r.rows, r.kore_ms, r.jvm_est_ms, r.speedup, r.ops_per_sec)?;
    }
    Ok(())
}
