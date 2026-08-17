//! KoreWriter — serialize a DataBlock to bytes / a file.

use std::env;
use std::io::{self, Write};
use kore_core::{ColumnData, DataBlock};
use crate::{Compression, DType, MAGIC, VERSION, compress};

/// AES-256-GCM encryption marker in kore binary footer.
const KORE_ENC_MARKER: &[u8] = b"KENC";

/// Encrypt column data with AES-256-GCM using a password-derived key.
pub fn encrypt_column(data: &[u8], password: &[u8]) -> Result<(Vec<u8>, crate::EncryptionMetadata), String> {
    use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};
    use aes_gcm::Nonce;
    use sha2::Sha256;
    use pbkdf2::pbkdf2_hmac;

    let mut salt = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut salt);
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password, &salt, 100_000, &mut key);

    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let nonce_bytes = generate_nonce();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, data).map_err(|e| e.to_string())?;

    let meta = crate::EncryptionMetadata {
        encrypted_cols: vec![],
        algorithm: "AES-256-GCM".into(),
        kdf: "PBKDF2".into(),
        salt: salt.to_vec(),
        nonce: nonce_bytes.to_vec(),
    };
    Ok((ciphertext, meta))
}

/// Decrypt column data with AES-256-GCM.
pub fn decrypt_column(ciphertext: &[u8], password: &[u8], meta: &crate::EncryptionMetadata) -> Result<Vec<u8>, String> {
    use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};
    use aes_gcm::Nonce;
    use sha2::Sha256;
    use pbkdf2::pbkdf2_hmac;

    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password, &meta.salt, 100_000, &mut key);

    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(&meta.nonce);
    cipher.decrypt(nonce, ciphertext).map_err(|e| e.to_string())
}

const READABLE_TRAILER_BEGIN: &str = "\nKORE-READABLE-BEGIN\n";
const READABLE_TRAILER_END: &str = "KORE-READABLE-END\n";
const READABLE_FOOTER_PREFIX: &str = "KORE-READABLE-FOOTER trailer_len=";
const READABLE_PREVIEW_ROWS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadableMode {
    None,
    Preview,
    Full,
}

/// Try both LZ4 and Zstd, pick the best.
/// Returns (compression_codec, compressed) with best ratio.
fn try_best_compression(comp: Compression, data: Vec<u8>) -> (Compression, Vec<u8>) {
    if data.len() < 64 { return (comp, data); }  // not worth it for tiny cols
    
    let lz4 = lz4_flex::compress_prepend_size(&data);
    let zstd = compress::zstd_encode(&data);
    
    // Pick best compression ratio
    let (final_codec, final_data) = if lz4.len() < zstd.len() {
        (Compression::Lz4, lz4)
    } else {
        (Compression::Zstd, zstd)
    };
    
    if final_data.len() < data.len() {
        // Encode the original comp type in first byte so reader can round-trip
        let mut out = vec![comp as u8];
        out.extend_from_slice(&final_data);
        (final_codec, out)
    } else {
        (comp, data)
    }
}

/// Compute statistics for a column (min, max, null count).
fn compute_col_stats(data: &ColumnData) -> compress::ColStats {
    match data {
        ColumnData::Int64(vals) => compress::ColStats::for_int64(vals),
        ColumnData::Float64(vals) => compress::ColStats::for_f64(vals),
        ColumnData::Bool(_) | ColumnData::Str(_) | ColumnData::StrDict { .. } => {
            compress::ColStats {
                null_count: 0,
                min_i64: None,
                max_i64: None,
                min_f64: None,
                max_f64: None,
            }
        }
    }
}

/// Generate bloom filter for string columns (for fast cardinality checks).
fn generate_bloom_filter(data: &ColumnData, fpp: f64) -> Option<compress::BloomFilter> {
    match data {
        ColumnData::Str(vals) => {
            let non_null: Vec<&String> = vals.iter().filter_map(|v| v.as_ref()).collect();
            if non_null.is_empty() { return None; }
            let mut bf = compress::BloomFilter::new(non_null.len(), fpp);
            for v in non_null {
                bf.insert(v.as_bytes());
            }
            Some(bf)
        }
        ColumnData::StrDict { dict, codes: _ } => {
            let mut bf = compress::BloomFilter::new(dict.len(), fpp);
            for v in dict {
                bf.insert(v.as_bytes());
            }
            Some(bf)
        }
        _ => None,
    }
}

/// Assign unique column IDs for schema evolution tracking.
fn generate_column_ids(num_cols: usize) -> Vec<u32> {
    (0..num_cols as u32).collect()
}

/// Generate nonce for AES-256-GCM (12 bytes).
fn generate_nonce() -> [u8; 12] {
    use rand::RngCore;
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    nonce
}

/// Create initial version snapshot for MVCC/time travel.
fn create_version_snapshot(version_id: u32, row_count: u64, block_offset: u64) -> crate::VersionSnapshot {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    
    crate::VersionSnapshot {
        version_id,
        timestamp,
        block_offset,
        row_count,
        prev_version: None,
    }
}

/// Create default partition spec (no partitioning).
fn create_default_partition_spec() -> crate::PartitionSpec {
    crate::PartitionSpec {
        spec_id: 0,
        columns: vec![],
        transforms: vec![],
        parent_spec_id: None,
    }
}

/// Check if any rows are deleted (placeholder for future delete tracking).
fn get_delete_vector(_num_rows: usize) -> Option<crate::DeleteVector> {
    // In future: track which rows have been soft-deleted
    // For now: None (no deletes)
    None
}

pub struct KoreWriter;

impl KoreWriter {
    /// Serialize a DataBlock to a byte buffer.
    pub fn to_bytes(block: &DataBlock) -> Vec<u8> {
        let mut buf = Vec::new();
        KoreWriter::write_to(&mut buf, block).expect("in-memory write never fails");
        buf
    }

    /// Serialize with AES-256-GCM encryption on all column data.
    pub fn to_bytes_encrypted(block: &DataBlock, password: &[u8]) -> Result<Vec<u8>, String> {
        let plain = Self::to_bytes(block);
        let (ciphertext, meta) = encrypt_column(&plain, password)?;
        let mut out = Vec::new();
        out.extend_from_slice(KORE_ENC_MARKER);
        let salt_len = meta.salt.len() as u16;
        let nonce_len = meta.nonce.len() as u16;
        out.extend_from_slice(&salt_len.to_le_bytes());
        out.extend_from_slice(&meta.salt);
        out.extend_from_slice(&nonce_len.to_le_bytes());
        out.extend_from_slice(&meta.nonce);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// Serialize with MVCC version snapshot appended to footer.
    pub fn to_bytes_versioned(block: &DataBlock, version_id: u32) -> Vec<u8> {
        let mut buf = Self::to_bytes(block);
        let snapshot = create_version_snapshot(version_id, block.num_rows as u64, 0);
        // Append version snapshot marker + data
        buf.extend_from_slice(b"KVER");
        buf.extend_from_slice(&snapshot.version_id.to_le_bytes());
        buf.extend_from_slice(&snapshot.timestamp.to_le_bytes());
        buf.extend_from_slice(&snapshot.row_count.to_le_bytes());
        buf.extend_from_slice(&snapshot.block_offset.to_le_bytes());
        buf
    }

    /// Serialize to any `Write` target.
    pub fn write_to<W: Write>(w: &mut W, block: &DataBlock) -> io::Result<()> {
        let num_cols = block.columns.len() as u32;
        let num_rows = block.num_rows as u64;
        let readable_mode = readable_mode_from_env();

        // ── Header ────────────────────────────────────────────────────────
        w.write_all(MAGIC)?;
        w.write_all(&VERSION.to_le_bytes())?;
        w.write_all(&num_cols.to_le_bytes())?;
        w.write_all(&num_rows.to_le_bytes())?;

        // ── Schema ────────────────────────────────────────────────────────
        for col in &block.columns {
            let name_bytes = col.name.as_bytes();
            w.write_all(&(name_bytes.len() as u16).to_le_bytes())?;
            w.write_all(name_bytes)?;
            let dtype: u8 = match &col.data {
                ColumnData::Int64(_)       => DType::I64     as u8,
                ColumnData::Float64(_)     => DType::F64     as u8,
                ColumnData::Bool(_)        => DType::Bool    as u8,
                ColumnData::Str(_)         => DType::Str     as u8,
                ColumnData::StrDict { .. } => DType::StrDict as u8,
            };
            w.write_all(&[dtype])?;
        }

        // ── Column data ───────────────────────────────────────────────────
        // Use parallel encoding for massive write speedup on multi-core systems.
        use rayon::prelude::*;
        let encoded_cols: Vec<(Compression, Vec<u8>, compress::ColStats)> = block.columns.par_iter()
            .map(|col| {
                let (comp, data) = encode_column(&col.data);
                let stats = compute_col_stats(&col.data);
                let (final_comp, final_data) = try_best_compression(comp, data);
                (final_comp, final_data, stats)
            })
            .collect();

        // ── Checksums and stats section ───────────────────────────────────
        let mut stats_section = Vec::new();
        for (_final_comp, final_data, col_stats) in &encoded_cols {
            // CRC32 checksum for this column
            let checksum = compress::crc32(final_data);
            stats_section.extend_from_slice(&checksum.to_le_bytes());
            
            // Column statistics
            stats_section.push(col_stats.null_count as u8);
            if let Some(m) = col_stats.min_i64 {
                stats_section.push(1); // has_i64_stats
                stats_section.extend_from_slice(&m.to_le_bytes());
                if let Some(mx) = col_stats.max_i64 {
                    stats_section.extend_from_slice(&mx.to_le_bytes());
                }
            } else {
                stats_section.push(0); // no i64 stats
            }
        }
        
        // ── Column data with inline checksums ──────────────────────────────
        for (final_comp, final_data, _) in encoded_cols {
            eprintln!("[WRITER] col comp={:?} data_len={}", final_comp, final_data.len());
            w.write_all(&[final_comp as u8])?;
            w.write_all(&(final_data.len() as u64).to_le_bytes())?;
            w.write_all(&final_data)?;
        }
        
        // Write stats section
        w.write_all(&(stats_section.len() as u32).to_le_bytes())?;
        w.write_all(&stats_section)?;

        if readable_mode != ReadableMode::None {
            // Append a plain-text trailer so the same .kore file stays self-describing
            // when opened in an editor, without changing the fast binary read path.
            let trailer = render_readable_trailer(block, readable_mode, readable_rows_from_env());
            let footer = format!(
                "{}{:020} mode={}\n",
                READABLE_FOOTER_PREFIX,
                trailer.len(),
                readable_mode_name(readable_mode),
            );
            w.write_all(trailer.as_bytes())?;
            w.write_all(footer.as_bytes())?;
        }
        Ok(())
    }

    /// Convenience: write to a file path.
    pub fn write_file(path: &std::path::Path, block: &DataBlock) -> io::Result<()> {
        let f = std::fs::File::create(path)?;
        let mut w = std::io::BufWriter::with_capacity(1024 * 1024, f); // 1MB buffer
        Self::write_to(&mut w, block)?;
        w.flush()
    }
}

fn readable_mode_from_env() -> ReadableMode {
    match env::var("KORE_READABLE_MODE") {
        Ok(v) if v.eq_ignore_ascii_case("none") => ReadableMode::None,
        Ok(v) if v.eq_ignore_ascii_case("full") => ReadableMode::Full,
        _ => ReadableMode::Preview,
    }
}

fn readable_rows_from_env() -> usize {
    env::var("KORE_READABLE_ROWS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(READABLE_PREVIEW_ROWS)
}

fn readable_mode_name(mode: ReadableMode) -> &'static str {
    match mode {
        ReadableMode::None => "none",
        ReadableMode::Preview => "preview",
        ReadableMode::Full => "full",
    }
}

fn render_readable_trailer(block: &DataBlock, mode: ReadableMode, preview_cap: usize) -> String {
    let preview_rows = match mode {
        ReadableMode::None => 0,
        ReadableMode::Preview => block.num_rows.min(preview_cap),
        ReadableMode::Full => block.num_rows,
    };
    let mut out = String::new();
    out.push_str(READABLE_TRAILER_BEGIN);
    out.push_str(&format!(
        "version={VERSION}\nrows={}\ncolumns={}\nmode={}\n",
        block.num_rows,
        block.columns.len(),
        readable_mode_name(mode),
    ));
    out.push_str("schema:\n");
    for col in &block.columns {
        out.push_str("  - ");
        out.push_str(&col.name);
        out.push_str(": ");
        out.push_str(dtype_name(&col.data));
        out.push('\n');
    }

    out.push_str("preview_csv:\n");
    for (idx, col) in block.columns.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&escape_csv_cell(&col.name));
    }
    out.push('\n');

    for row_idx in 0..preview_rows {
        for (col_idx, col) in block.columns.iter().enumerate() {
            if col_idx > 0 {
                out.push(',');
            }
            out.push_str(&escape_csv_cell(&format_cell(&col.data, row_idx)));
        }
        out.push('\n');
    }

    if mode == ReadableMode::Preview && block.num_rows > preview_rows {
        out.push_str(&format!("preview_rows={} of {}\n", preview_rows, block.num_rows));
    } else {
        out.push_str(&format!("preview_rows={} of {}\n", preview_rows, block.num_rows));
    }
    out.push_str(READABLE_TRAILER_END);
    out
}

fn dtype_name(data: &ColumnData) -> &'static str {
    match data {
        ColumnData::Int64(_) => "i64",
        ColumnData::Float64(_) => "f64",
        ColumnData::Bool(_) => "bool",
        ColumnData::Str(_) => "str",
        ColumnData::StrDict { .. } => "str_dict",
    }
}

fn format_cell(data: &ColumnData, row: usize) -> String {
    match data {
        ColumnData::Int64(values) => values.get(row).and_then(|v| *v).map(|v| v.to_string()).unwrap_or_else(|| "NULL".to_string()),
        ColumnData::Float64(values) => values.get(row).and_then(|v| *v).map(|v| v.to_string()).unwrap_or_else(|| "NULL".to_string()),
        ColumnData::Bool(values) => values.get(row).and_then(|v| *v).map(|v| v.to_string()).unwrap_or_else(|| "NULL".to_string()),
        ColumnData::Str(values) => values.get(row).and_then(|v| v.as_ref()).cloned().unwrap_or_else(|| "NULL".to_string()),
        ColumnData::StrDict { codes, dict } => match codes.get(row).copied() {
            Some(code) if code != u8::MAX => dict.get(code as usize).cloned().unwrap_or_else(|| "NULL".to_string()),
            _ => "NULL".to_string(),
        },
    }
}

fn escape_csv_cell(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        let escaped = value.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        value.to_string()
    }
}

fn encode_column(data: &ColumnData) -> (Compression, Vec<u8>) {
    match data {
        ColumnData::Int64(v) => {
            let is_sorted = v.windows(2).all(|w| match (w[0], w[1]) {
                (Some(a), Some(b)) => a <= b,
                _ => true,
            });
            if is_sorted {
                (Compression::Delta, compress::delta_encode_i64(v))
            } else {
                (Compression::Rle, compress::rle_encode_i64(v))
            }
        }
        // Float64: try dictionary first (huge win for low-cardinality, e.g. l_discount).
        // Fall back to NaN-sentinel (8 bytes/el) which is still better than raw (9 bytes/el).
        ColumnData::Float64(v) => {
            if let Some(dict_bytes) = compress::dict_encode_f64(v) {
                (Compression::Dict, dict_bytes)
            } else {
                (Compression::NanRaw, compress::nan_encode_f64(v))
            }
        }
        ColumnData::Bool(v)    => (Compression::Raw, compress::raw_encode_bool(v)),
        ColumnData::Str(v)     => {
            // Auto-detect dictionary opportunity for strings.
            // If < 255 unique strings and n > 1000, dictionary is 10x faster/smaller.
            if v.len() > 1000 {
                let mut dict = Vec::new();
                let mut codes = Vec::with_capacity(v.len());
                let mut map = std::collections::HashMap::new();
                let mut ok = true;
                for opt_s in v {
                    if let Some(s) = opt_s {
                        if let Some(&code) = map.get(s.as_str()) {
                            codes.push(code);
                        } else {
                            if dict.len() >= 254 { ok = false; break; }
                            let code = dict.len() as u8;
                            map.insert(s.as_str(), code);
                            dict.push(s.clone());
                            codes.push(code);
                        }
                    } else {
                        codes.push(u8::MAX);
                    }
                }
                if ok {
                    return (Compression::Dict, compress::encode_strdict(&codes, &dict));
                }
            }
            (Compression::Raw, compress::encode_strs(v))
        },
        // StrDict: store codes directly (1 byte/row) + tiny dict — no string explosion.
        ColumnData::StrDict { codes, dict } => {
            (Compression::Raw, compress::encode_strdict(codes, dict))
        }
    }
}
