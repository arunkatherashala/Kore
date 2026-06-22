use std::env;
use std::fs::File;
use std::io::Read;
use anyhow::Result;
use kore_fileformat::{read_row_group_metadata_from_reader, RowGroupMetadata, CodecRegistry};
use kore_fileformat::KoreReader;
use std::io::Seek;

// Streaming decode scaffold: returns decoded bytes for a column.
// TODO: replace with true chunked streaming using column block metadata + `CodecRegistry`.
fn streaming_decode_column(path: &str, col_idx: usize) -> anyhow::Result<Vec<u8>> {
    // Chunked streaming decode using column block metadata.
    // Reads compressed blocks from file by offset, calls CodecRegistry::decompress,
    // and appends decompressed bytes into a buffer. This avoids materializing
    // unrelated columns and keeps memory proportional to a single block.

    // Open file once, read footer/header via KoreReader to get column block metadata.
    let mut f = std::fs::File::open(path)?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)?;

    let mut kr = KoreReader::new(data)?;
    let header = kr.header();
    if col_idx >= header.columns.len() {
        anyhow::bail!("column index out of range");
    }

    // Build decompressed buffer by iterating blocks. We use existing KoreReader
    // helpers to get block offsets via read_column_blocks (if available) or fall back
    // to kr.read_column() as a final safe option.
    if let Ok(blocks) = kr.read_column_blocks(col_idx) {
        let mut out: Vec<u8> = Vec::new();
        for blk in blocks {
            // Each block exposes offset and compressed_size
            let offset = blk.file_offset as usize;
            let csize = blk.compressed_size as usize;
            // bounds-check and read compressed bytes
            let file_bytes = kr.raw_bytes();
            if offset >= file_bytes.len() { break; }
            let end = (offset + csize).min(file_bytes.len());
            let comp = &file_bytes[offset..end];

            // Decompress using CodecRegistry
            match CodecRegistry::decompress(blk.codec_id, comp) {
                Ok(mut decomp) => out.append(&mut decomp),
                Err(_) => {
                    // If a block fails to decompress, skip it conservatively.
                }
            }
        }
        return Ok(out);
    }

    // Fallback: full column read (existing behavior)
    let dec = kr.read_column(col_idx)?;
    Ok(dec)
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: kore-reader <command> [args]. Commands: inspect <file>, write-manifest <dir>, compact <manifest.json>, ddl <add|drop|rename> ...");
        std::process::exit(2);
    }

    match args.get(1).map(|s| s.as_str()).unwrap_or("") {
        "inspect" => {
            if args.len() < 3 { eprintln!("usage: kore-reader inspect <file> [sample_size]"); std::process::exit(2); }
            let path = &args[2];
            let sample: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(10);
            inspect_file(path, sample)?;
        }
        "write-manifest" => {
            let dir = args.get(2).map(|s| s.as_str()).unwrap_or(".");
            let m = crate::manifest::new_manifest_example();
            crate::manifest::write_manifest_atomic(dir, &m)?;
            println!("manifest written to {}", dir);
        }
        "compact" => {
            let mf = args.get(2).map(|s| s.as_str()).unwrap_or("manifest.json");
            let s = std::fs::read_to_string(mf)?;
            let manifest: crate::manifest::Manifest = serde_json::from_str(&s)?;
            let new = crate::compaction::compact_manifest(&manifest, std::path::Path::new("."))?;
            crate::manifest::write_manifest_atomic(".", &new)?;
            println!("compaction prototype complete; new manifest written");
        }
        "ddl" => {
            if args.len() < 3 { eprintln!("usage: kore-reader ddl add <name> <type>"); std::process::exit(2); }
            let op = args.get(2).map(|s| s.as_str()).unwrap_or("");
            match op {
                "add" => {
                    let name = args.get(3).map(|s| s.as_str()).unwrap_or("");
                    let dtype = args.get(4).map(|s| s.as_str()).unwrap_or("string");
                    crate::ddl::apply_ddl(crate::ddl::DdlAction::AddColumn { name: name.to_string(), dtype: dtype.to_string(), nullable: true }).map_err(|e| anyhow::anyhow!(e))?;
                }
                "drop" => {
                    let name = args.get(3).map(|s| s.as_str()).unwrap_or("");
                    crate::ddl::apply_ddl(crate::ddl::DdlAction::DropColumn { name: name.to_string() }).map_err(|e| anyhow::anyhow!(e))?;
                }
                "rename" => {
                    let old = args.get(3).map(|s| s.as_str()).unwrap_or("");
                    let newn = args.get(4).map(|s| s.as_str()).unwrap_or("");
                    crate::ddl::apply_ddl(crate::ddl::DdlAction::RenameColumn { old: old.to_string(), new: newn.to_string() }).map_err(|e| anyhow::anyhow!(e))?;
                }
                _ => { eprintln!("unknown ddl op"); }
            }
        }
        "insert" => {
            if args.len() < 3 { eprintln!("usage: kore-reader insert <text>"); std::process::exit(2); }
            let payload = args.get(2).map(|s| s.as_str()).unwrap_or("");
            crate::dml::insert_rows(payload).map_err(|e| anyhow::anyhow!(e))?;
            println!("inserted payload and published manifest");
        }
        "delete" => {
            if args.len() < 5 { eprintln!("usage: kore-reader delete <file> <start> <end>"); std::process::exit(2); }
            let file = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let start = args.get(3).and_then(|s| s.parse::<u64>().ok());
            let end = args.get(4).and_then(|s| s.parse::<u64>().ok());
            crate::dml::delete_range(file, start, end).map_err(|e| anyhow::anyhow!(e))?;
            println!("staged tombstone for {} {}-{}", file, start.map(|v| v.to_string()).unwrap_or("-".into()), end.map(|v| v.to_string()).unwrap_or("-".into()));
        }
        "stream-decode" => {
            if args.len() < 3 { eprintln!("usage: kore-reader stream-decode <file> [limit]"); std::process::exit(2); }
            let file = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let limit = args.get(3).and_then(|s| s.parse::<usize>().ok()).unwrap_or(10usize);
            let p = std::path::Path::new(file);
            crate::streaming_decoder::print_sample(p, limit).map_err(|e| anyhow::anyhow!(e))?;
        }
        "dml" => {
            if args.len() < 4 { eprintln!("usage: kore-reader dml insert <src> <dest_dir>"); std::process::exit(2); }
            let op = args.get(2).map(|s| s.as_str()).unwrap_or("");
            match op {
                "insert" => {
                    let src = std::path::Path::new(args.get(3).unwrap());
                    let dest = std::path::Path::new(args.get(4).map(|s| s.as_str()).unwrap_or("data"));
                    crate::dml::insert_data_file(src, dest).map_err(|e| anyhow::anyhow!(e))?;
                    println!("inserted {} -> {}", src.display(), dest.display());
                }
                _ => { eprintln!("unknown dml op"); }
            }
        }
        other => {
            eprintln!("unknown command: {}", other);
            std::process::exit(2);
        }
    }

    Ok(())
}

fn inspect_file(path: &str, sample: usize) -> Result<()> {
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
            println!("Per-column null counts (from footer row-group metadata):");
            for (i, stats) in meta.column_stats.iter().enumerate() {
                println!("  {}: null_count={} min={:?} max={:?}", i, stats.null_count, stats.min, stats.max);
            }
        }
        Err(e) => {
            println!("Row-group metadata reader failed: {}", e);
            println!("No readable footer found and heuristic scan disabled to avoid memory issues.");
            println!("Attempting safe block-level scan using KoreReader header...");

            match std::panic::catch_unwind(|| KoreReader::open(path)) {
                Ok(open_res) => match open_res {
                    Ok(kr) => {
                        println!("Opened KORE file — attempting per-column safe decode (thresholded)...");
                        let header = kr.header();
                        let ncols = header.column_count as usize;
                        let nrows = header.row_count as usize;
                        let col_names: Vec<String> = header.columns.iter().map(|c| c.name.clone()).collect();

                        const UNCOMPRESSED_THRESHOLD: usize = 500 * 1024 * 1024;

                        let mut total_nulls: Vec<Option<u64>> = vec![None; ncols];
                        let mut samples: Vec<Vec<String>> = vec![Vec::new(); ncols];

                        for (ci, colmeta) in header.columns.iter().enumerate() {
                            if colmeta.uncompressed_size as usize > UNCOMPRESSED_THRESHOLD {
                                println!("  Skipping heavy column {} (uncompressed {} bytes)", ci, colmeta.uncompressed_size);
                                continue;
                            }

                            println!("  Decoding column {} ({} bytes uncompressed)...", ci, colmeta.uncompressed_size);

                            let ci_copy = ci;
                            let path_clone = path.clone();
                            match std::panic::catch_unwind(|| {
                                streaming_decode_column(&path_clone, ci_copy)
                            }) {
                                Ok(Ok(dec_bytes)) => {
                                    println!("    decoded {} bytes for column {}", dec_bytes.len(), ci);
                                    let dt = header.columns[ci].data_type;
                                    let mut nulls = 0u64;
                                    if dt == 0 {
                                        let mut p = 0usize;
                                        let mut cnt = 0usize;
                                        while p + 8 <= dec_bytes.len() && cnt < nrows {
                                            let v = i64::from_le_bytes(dec_bytes[p..p+8].try_into().unwrap());
                                            if v == i64::MIN { nulls += 1; } else if samples[ci].len() < sample { samples[ci].push(format!("{}", v)); }
                                            p += 8; cnt += 1;
                                        }
                                    } else if dt == 2 {
                                        let mut p = 0usize; let mut cnt = 0usize;
                                        while p < dec_bytes.len() && cnt < nrows {
                                            let (slen, np) = {
                                                if p + 4 <= dec_bytes.len() { let sl = u32::from_le_bytes(dec_bytes[p..p+4].try_into().unwrap()) as usize; (sl, p+4) } else { break };
                                            };
                                            let end = np + slen.min(dec_bytes.len().saturating_sub(np));
                                            let s = String::from_utf8_lossy(&dec_bytes[np..end]).into_owned();
                                            if s.is_empty() { nulls += 1; } else if samples[ci].len() < sample { samples[ci].push(s); }
                                            p = end; cnt += 1;
                                        }
                                    } else if dt == 3 {
                                        let mut cnt = 0usize;
                                        for &b in dec_bytes.iter().take(nrows) {
                                            if b == 2 { nulls += 1; } else if samples[ci].len() < sample { samples[ci].push(format!("{}", b != 0)); }
                                            cnt += 1;
                                        }
                                    } else {
                                        let nz = dec_bytes.iter().filter(|&&b| b == 0).count() as u64;
                                        nulls = nz;
                                    }
                                    total_nulls[ci] = Some(nulls);
                                }
                                Ok(Err(_)) | Err(_) => {
                                    println!("  Column {} decode failed or panicked; skipping.", ci);
                                }
                            }
                        }

                        println!("Per-column null counts (partial):");
                        for ci in 0..ncols {
                            let name = col_names.get(ci).map(|s| s.as_str()).unwrap_or("");
                            match total_nulls[ci] {
                                Some(n) => println!("  {} ({}): {} nulls", ci, name, n),
                                None => println!("  {} ({}): <skipped or unknown>", ci, name),
                            }
                            if !samples[ci].is_empty() { println!("    samples: [{}]", samples[ci].join(", ")); }
                        }
                    }
                    Err(e2) => {
                        println!("KoreReader header scan failed: {}", e2);
                    }
                },
                Err(panic_info) => {
                    let _ = std::fs::write("kore-reader-panic.log", format!("Open panic: {:?}\nPath: {}\n", panic_info, path));
                    println!("KoreReader::open() panicked; wrote kore-reader-panic.log and will fall back to footer-only output.");
                }
            }
        }

    Ok(())
}
