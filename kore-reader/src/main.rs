use std::env;
use std::fs::File;
use std::io::Read;
use anyhow::Result;
use kore_fileformat::{read_row_group_metadata_from_reader, RowGroupMetadata};
use kore_fileformat::KoreReader;
use std::io::Seek;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: kore-reader <file> [sample_size]");
        std::process::exit(2);
    }

        let path = &args[1];
        let sample: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);

        let mut f = File::open(path)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;

        // Try to read row group metadata as a quick way to inspect
        let mut cursor = std::io::Cursor::new(&buf);
        match read_row_group_metadata_from_reader(&mut cursor) {
            Ok(meta) => {
                println!("Row group metadata parsed: {} bytes", buf.len());
                println!("Row count: {}", meta.row_count);
                println!("Per-column stats count: {}", meta.column_stats.len());
                // Sum and print authoritative null_count per column from the footer
                println!("Per-column null counts (from footer row-group metadata):");
                for (i, stats) in meta.column_stats.iter().enumerate() {
                    println!("  {}: null_count={} min={:?} max={:?}", i, stats.null_count, stats.min, stats.max);
                }
            }
            Err(e) => {
                println!("Row-group metadata reader failed: {}", e);
                println!("No readable footer found and heuristic scan disabled to avoid memory issues.");
                println!("Attempting safe block-level scan using KoreReader header...");

                match KoreReader::open(path) {
                    Ok(kr) => {
                        // Use the reader's public fields and `read_all_columns` to get decoded values
                        println!("Opened KORE file — decoding all columns (may be memory intensive)...");
                        let cols = kr.read_all_columns();
                        let ncols = kr.ncols;
                        let nrows = kr.nrows;
                        let col_names: Vec<String> = kr.columns.iter().map(|c| c.name.clone()).collect();

                        println!("File: {} columns={} rows={}", path, ncols, nrows);
                        let mut total_nulls: Vec<u64> = vec![0u64; ncols];
                        // collect samples per column
                        for ci in 0..ncols {
                            let col = &cols[ci];
                            for v in col.iter() {
                                if v.is_null() { total_nulls[ci] += 1; }
                            }
                        }

                        println!("Per-column null counts (decoded):");
                        for ci in 0..ncols {
                            let name = col_names.get(ci).map(|s| s.as_str()).unwrap_or("");
                            println!("  {} ({}): {} nulls / {} rows", ci, name, total_nulls[ci], cols[ci].len());
                            // print first `sample` values
                            let mut out: Vec<String> = Vec::new();
                            for v in cols[ci].iter().take(sample) { out.push(v.display()); }
                            println!("    samples: [{}]", out.join(", "));
                        }
                    }
                    Err(e2) => {
                        println!("KoreReader header scan failed: {}", e2);
                    }
                }
            }
        }

    Ok(())
}
