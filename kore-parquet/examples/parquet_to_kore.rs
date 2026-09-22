//! Direct Parquet → KORE converter.
//!
//! Usage: cargo run -p kore-parquet --release --example parquet_to_kore -- <input.parquet> <output.kore>

use std::path::Path;
use std::time::Instant;

use kore_parquet::ParquetReader;
use kore_store::KoreWriter;

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args.next().unwrap_or_else(|| {
        eprintln!("usage: parquet_to_kore <input.parquet> <output.kore>");
        std::process::exit(2);
    });
    let output = args.next().unwrap_or_else(|| {
        eprintln!("usage: parquet_to_kore <input.parquet> <output.kore>");
        std::process::exit(2);
    });

    let t0 = Instant::now();
    let block = ParquetReader::new(&input).read().unwrap_or_else(|e| {
        eprintln!("read parquet failed: {e}");
        std::process::exit(1);
    });
    let read_ms = t0.elapsed().as_millis();
    println!(
        "read {} rows x {} cols in {} ms",
        block.num_rows,
        block.columns.len(),
        read_ms
    );

    let t1 = Instant::now();
    KoreWriter::write_file(Path::new(&output), &block).unwrap_or_else(|e| {
        eprintln!("write kore failed: {e}");
        std::process::exit(1);
    });
    println!("wrote {output} in {} ms", t1.elapsed().as_millis());
}
