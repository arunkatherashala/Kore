//! KORE Binary Format — pure Rust, zero dependencies.
//!
//! Reads and writes `.kore` columnar files with CRC32 integrity.
//!
//! # Quick Start
//! ```
//! use kore_fileformat::{DataBlock, DataType, write_file, read_file};
//!
//! let mut block = DataBlock::new();
//! block.add_column("price", DataType::F64, vec![10.5f64.to_bits(), 20.0f64.to_bits(), 30.75f64.to_bits()]);
//! block.add_column("qty",   DataType::I64, vec![100, 200, 300]);
//! write_file("data.kore", &block).unwrap();
//!
//! let result = read_file("data.kore").unwrap();
//! assert_eq!(result.num_rows(), 3);
//! ```

use std::io;

/// Column data type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataType {
    F64 = 1,
    I64 = 2,
    Str = 3,
}

/// A single named column of raw 64-bit values.
#[derive(Debug, Clone)]
pub struct Column {
    pub name:    String,
    pub dtype:   DataType,
    pub values:  Vec<u64>,   // F64/I64 stored as raw bits; Str not yet supported
}

/// A collection of columns forming a table.
#[derive(Debug, Clone, Default)]
pub struct DataBlock {
    columns: Vec<Column>,
}

impl DataBlock {
    pub fn new() -> Self { Self::default() }

    pub fn add_column(&mut self, name: &str, dtype: DataType, values: Vec<u64>) {
        self.columns.push(Column { name: name.to_string(), dtype, values });
    }

    pub fn num_rows(&self) -> usize {
        self.columns.first().map(|c| c.values.len()).unwrap_or(0)
    }

    pub fn num_columns(&self) -> usize { self.columns.len() }

    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }

    pub fn columns(&self) -> &[Column] { &self.columns }
}

// ── KORE v2 wire format ──────────────────────────────────────────────────────
//  [4]  magic   "KORE"
//  [4]  version u32le = 2
//  [4]  n_cols  u32le
//  per column:
//    [1]  dtype   u8
//    [1]  name_len u8
//    [name_len] name bytes (UTF-8)
//    [8]  n_rows  u64le
//    [n_rows*8] values (little-endian u64)
//  [4]  crc32   u32le (over all preceding bytes)

const MAGIC: &[u8; 4] = b"KORE";
const VERSION: u32 = 2;

/// Write a `DataBlock` to a file path.
pub fn write_file(path: &str, block: &DataBlock) -> io::Result<()> {
    let bytes = to_bytes(block);
    std::fs::write(path, &bytes)
}

/// Read a `DataBlock` from a file path.
pub fn read_file(path: &str) -> io::Result<DataBlock> {
    let bytes = std::fs::read(path)?;
    from_bytes(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Serialize a `DataBlock` to bytes (KORE v2 format).
pub fn to_bytes(block: &DataBlock) -> Vec<u8> {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&VERSION.to_le_bytes());
    buf.extend_from_slice(&(block.columns.len() as u32).to_le_bytes());

    for col in &block.columns {
        buf.push(col.dtype as u8);
        let name_bytes = col.name.as_bytes();
        buf.push(name_bytes.len() as u8);
        buf.extend_from_slice(name_bytes);
        buf.extend_from_slice(&(col.values.len() as u64).to_le_bytes());
        for &v in &col.values {
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }

    let checksum = crc32(&buf);
    buf.extend_from_slice(&checksum.to_le_bytes());
    buf
}

/// Deserialize bytes into a `DataBlock`.
pub fn from_bytes(data: &[u8]) -> Result<DataBlock, String> {
    if data.len() < 12 { return Err("too short".into()); }

    // Verify CRC32
    let (body, crc_bytes) = data.split_at(data.len() - 4);
    let stored = u32::from_le_bytes(crc_bytes.try_into().unwrap());
    let computed = crc32(body);
    if stored != computed {
        return Err(format!("CRC32 mismatch: stored={stored:#010x} computed={computed:#010x}"));
    }

    let mut r = body;

    // Magic
    if &r[..4] != MAGIC { return Err("bad magic".into()); }
    r = &r[4..];

    // Version
    let _ver = u32::from_le_bytes(r[..4].try_into().unwrap());
    r = &r[4..];

    // Column count
    let n_cols = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize;
    r = &r[4..];

    let mut block = DataBlock::new();
    for _ in 0..n_cols {
        let dtype_byte = r[0];
        let name_len   = r[1] as usize;
        r = &r[2..];
        let name = std::str::from_utf8(&r[..name_len]).map_err(|e| e.to_string())?.to_string();
        r = &r[name_len..];
        let n_rows = u64::from_le_bytes(r[..8].try_into().unwrap()) as usize;
        r = &r[8..];
        let mut values = Vec::with_capacity(n_rows);
        for _ in 0..n_rows {
            values.push(u64::from_le_bytes(r[..8].try_into().unwrap()));
            r = &r[8..];
        }
        let dtype = match dtype_byte {
            1 => DataType::F64,
            2 => DataType::I64,
            _ => DataType::Str,
        };
        block.add_column(&name, dtype, values);
    }
    Ok(block)
}

/// CRC32 (IEEE polynomial) — pure Rust, zero deps.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 { crc = (crc >> 1) ^ 0xEDB8_8320; }
            else             { crc >>= 1; }
        }
    }
    !crc
}

// ── Schema Evolution ─────────────────────────────────────────────────────────

/// Add a new column to an existing `.kore` file (schema evolution).
/// New column is filled with `default_val` for existing rows.
///
/// ```
/// kore_fileformat::add_column("data.kore", "region", kore_fileformat::DataType::I64, 0);
/// ```
pub fn add_column(path: &str, name: &str, dtype: DataType, default_val: u64) -> io::Result<()> {
    let mut block = read_file(path)?;
    let n = block.num_rows();
    block.add_column(name, dtype, vec![default_val; n]);
    write_file(path, &block)
}

/// Remove a column from an existing `.kore` file by name.
pub fn drop_column(path: &str, name: &str) -> io::Result<()> {
    let mut block = read_file(path)?;
    block.columns.retain(|c| c.name != name);
    write_file(path, &block)
}

/// Rename a column in an existing `.kore` file.
pub fn rename_column(path: &str, old_name: &str, new_name: &str) -> io::Result<()> {
    let mut block = read_file(path)?;
    if let Some(col) = block.columns.iter_mut().find(|c| c.name == old_name) {
        col.name = new_name.to_string();
    }
    write_file(path, &block)
}

// ── Append Mode ───────────────────────────────────────────────────────────────

/// Append rows from `new_block` to an existing `.kore` file.
/// Both blocks must have the same column names and types.
///
/// ```
/// let mut extra = kore_fileformat::DataBlock::new();
/// extra.add_column("price", kore_fileformat::DataType::F64, vec![50.0f64.to_bits()]);
/// kore_fileformat::append_file("data.kore", &extra).unwrap();
/// ```
pub fn append_file(path: &str, new_block: &DataBlock) -> io::Result<()> {
    let mut base = read_file(path)?;
    for new_col in &new_block.columns {
        if let Some(base_col) = base.columns.iter_mut().find(|c| c.name == new_col.name) {
            base_col.values.extend_from_slice(&new_col.values);
        }
    }
    write_file(path, &base)
}

// ── Column Stats ──────────────────────────────────────────────────────────────

/// Basic statistics for a numeric column.
#[derive(Debug, Clone)]
pub struct ColStats {
    pub name:  String,
    pub dtype: DataType,
    pub count: usize,
    pub min:   f64,
    pub max:   f64,
    pub mean:  f64,
    pub nulls: usize,
}

impl DataBlock {
    /// Compute stats for all numeric columns.
    pub fn stats(&self) -> Vec<ColStats> {
        self.columns.iter().map(|col| {
            let nums: Vec<f64> = match col.dtype {
                DataType::F64 => col.values.iter().map(|&v| f64::from_bits(v)).collect(),
                DataType::I64 => col.values.iter().map(|&v| v as i64 as f64).collect(),
                DataType::Str => return ColStats {
                    name: col.name.clone(), dtype: col.dtype,
                    count: col.values.len(), min: 0.0, max: 0.0, mean: 0.0, nulls: 0,
                },
            };
            let count = nums.len();
            let min = nums.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let mean = if count > 0 { nums.iter().sum::<f64>() / count as f64 } else { 0.0 };
            ColStats { name: col.name.clone(), dtype: col.dtype, count, min, max, mean, nulls: 0 }
        }).collect()
    }

    /// Filter rows where column `name` equals `value` (exact match for I64/F64 bits).
    pub fn filter_eq(&self, col_name: &str, value: u64) -> DataBlock {
        let Some(filter_col) = self.column(col_name) else { return DataBlock::new() };
        let keep: Vec<usize> = filter_col.values.iter().enumerate()
            .filter_map(|(i, &v)| if v == value { Some(i) } else { None })
            .collect();
        let mut result = DataBlock::new();
        for col in &self.columns {
            let filtered: Vec<u64> = keep.iter().map(|&i| col.values[i]).collect();
            result.add_column(&col.name, col.dtype, filtered);
        }
        result
    }

    /// Filter rows where column value is in range [lo, hi] (inclusive, raw u64 bits).
    pub fn filter_range(&self, col_name: &str, lo: u64, hi: u64) -> DataBlock {
        let Some(fc) = self.column(col_name) else { return DataBlock::new() };
        let keep: Vec<usize> = fc.values.iter().enumerate()
            .filter_map(|(i, &v)| if v >= lo && v <= hi { Some(i) } else { None })
            .collect();
        let mut result = DataBlock::new();
        for col in &self.columns {
            result.add_column(&col.name, col.dtype, keep.iter().map(|&i| col.values[i]).collect());
        }
        result
    }

    /// Select only specified columns (projection pushdown).
    pub fn select(&self, names: &[&str]) -> DataBlock {
        let mut result = DataBlock::new();
        for col in &self.columns {
            if names.contains(&col.name.as_str()) {
                result.add_column(&col.name, col.dtype, col.values.clone());
            }
        }
        result
    }
}

// ── RLE Compression (zero deps) ───────────────────────────────────────────────

/// RLE-compress a column's values. Returns (values, run_lengths).
pub fn rle_encode(values: &[u64]) -> (Vec<u64>, Vec<u32>) {
    if values.is_empty() { return (vec![], vec![]); }
    let mut vals = Vec::new();
    let mut runs = Vec::new();
    let mut cur = values[0];
    let mut count: u32 = 1;
    for &v in &values[1..] {
        if v == cur {
            count += 1;
        } else {
            vals.push(cur);
            runs.push(count);
            cur = v;
            count = 1;
        }
    }
    vals.push(cur);
    runs.push(count);
    (vals, runs)
}

/// RLE-decompress back to original values.
pub fn rle_decode(vals: &[u64], runs: &[u32]) -> Vec<u64> {
    let mut result = Vec::new();
    for (&v, &r) in vals.iter().zip(runs.iter()) {
        for _ in 0..r { result.push(v); }
    }
    result
}

/// Write with RLE compression for all columns. Saves space for repetitive data.
pub fn write_file_rle(path: &str, block: &DataBlock) -> io::Result<()> {
    let bytes = to_bytes_rle(block);
    std::fs::write(path, &bytes)
}

/// Read a RLE-compressed .kore file.
pub fn read_file_rle(path: &str) -> io::Result<DataBlock> {
    let bytes = std::fs::read(path)?;
    from_bytes_rle(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

// KORE-RLE format: magic "KORER" + version 3 + same column layout but:
//   each column: dtype, name_len, name, n_unique u64le, n_unique*8 values, n_unique*4 run_lengths
const MAGIC_RLE: &[u8; 5] = b"KORER";

fn to_bytes_rle(block: &DataBlock) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(MAGIC_RLE);
    buf.extend_from_slice(&3u32.to_le_bytes());
    buf.extend_from_slice(&(block.columns.len() as u32).to_le_bytes());
    for col in &block.columns {
        let (vals, runs) = rle_encode(&col.values);
        buf.push(col.dtype as u8);
        let nb = col.name.as_bytes();
        buf.push(nb.len() as u8);
        buf.extend_from_slice(nb);
        buf.extend_from_slice(&(col.values.len() as u64).to_le_bytes()); // original rows
        buf.extend_from_slice(&(vals.len() as u32).to_le_bytes());
        for &v in &vals { buf.extend_from_slice(&v.to_le_bytes()); }
        for &r in &runs { buf.extend_from_slice(&r.to_le_bytes()); }
    }
    let cs = crc32(&buf);
    buf.extend_from_slice(&cs.to_le_bytes());
    buf
}

fn from_bytes_rle(data: &[u8]) -> Result<DataBlock, String> {
    if data.len() < 13 { return Err("too short".into()); }
    let (body, crc_bytes) = data.split_at(data.len() - 4);
    let stored = u32::from_le_bytes(crc_bytes.try_into().unwrap());
    if crc32(body) != stored { return Err("CRC32 mismatch".into()); }
    let mut r = body;
    if &r[..5] != MAGIC_RLE { return Err("bad RLE magic".into()); }
    r = &r[9..]; // skip magic(5) + version(4)
    let n_cols = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize;
    r = &r[4..];
    let mut block = DataBlock::new();
    for _ in 0..n_cols {
        let dtype_byte = r[0];
        let name_len = r[1] as usize;
        r = &r[2..];
        let name = std::str::from_utf8(&r[..name_len]).unwrap().to_string();
        r = &r[name_len..];
        let _n_rows = u64::from_le_bytes(r[..8].try_into().unwrap());
        r = &r[8..];
        let n_unique = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize;
        r = &r[4..];
        let vals: Vec<u64> = (0..n_unique).map(|_| { let v = u64::from_le_bytes(r[..8].try_into().unwrap()); r = &r[8..]; v }).collect();
        let runs: Vec<u32> = (0..n_unique).map(|_| { let v = u32::from_le_bytes(r[..4].try_into().unwrap()); r = &r[4..]; v }).collect();
        let dtype = match dtype_byte { 1 => DataType::F64, 2 => DataType::I64, _ => DataType::Str };
        block.add_column(&name, dtype, rle_decode(&vals, &runs));
    }
    Ok(block)
}

// ── Time Travel / Snapshots ───────────────────────────────────────────────────

/// Write a versioned snapshot. Creates `path.v001.kore`, `path.v002.kore`, etc.
/// Returns the snapshot path created.
pub fn write_snapshot(base_path: &str, block: &DataBlock) -> io::Result<String> {
    let version = next_snapshot_version(base_path);
    let snap_path = format!("{}.v{:03}.kore", base_path, version);
    write_file(&snap_path, block)?;
    // Update latest pointer
    std::fs::write(format!("{}.latest", base_path), version.to_string())?;
    Ok(snap_path)
}

/// Read a specific snapshot version. Version 0 = latest.
pub fn read_snapshot(base_path: &str, version: u32) -> io::Result<DataBlock> {
    let v = if version == 0 { current_snapshot_version(base_path) } else { version };
    read_file(&format!("{}.v{:03}.kore", base_path, v))
}

/// List all available snapshot versions.
pub fn list_snapshots(base_path: &str) -> Vec<u32> {
    let prefix = format!("{}.v", base_path);
    (1..=999).filter(|&v| std::path::Path::new(&format!("{}{:03}.kore", prefix, v)).exists()).collect()
}

fn next_snapshot_version(base_path: &str) -> u32 { current_snapshot_version(base_path) + 1 }
fn current_snapshot_version(base_path: &str) -> u32 {
    std::fs::read_to_string(format!("{}.latest", base_path))
        .ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

// ── Partitioned Tables ────────────────────────────────────────────────────────

/// Write a partitioned table — splits data by unique values of `partition_col`.
/// Creates: `base_dir/partition_col=VALUE/data.kore`
pub fn write_partitioned(base_dir: &str, block: &DataBlock, partition_col: &str) -> io::Result<Vec<String>> {
    let Some(pc) = block.column(partition_col) else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "partition column not found"));
    };
    let mut partitions: std::collections::HashMap<u64, Vec<usize>> = std::collections::HashMap::new();
    for (i, &v) in pc.values.iter().enumerate() {
        partitions.entry(v).or_default().push(i);
    }
    let mut paths = Vec::new();
    for (part_val, indices) in &partitions {
        let dir = format!("{}/{}={}", base_dir, partition_col, part_val);
        std::fs::create_dir_all(&dir)?;
        let path = format!("{}/data.kore", dir);
        let mut part_block = DataBlock::new();
        for col in block.columns() {
            part_block.add_column(&col.name, col.dtype, indices.iter().map(|&i| col.values[i]).collect());
        }
        write_file(&path, &part_block)?;
        paths.push(path);
    }
    Ok(paths)
}

/// Read all partitions from a partitioned table directory into one merged DataBlock.
pub fn read_partitioned(base_dir: &str) -> io::Result<DataBlock> {
    let mut merged = DataBlock::new();
    let mut initialized = false;
    for entry in std::fs::read_dir(base_dir)? {
        let entry = entry?;
        let path = entry.path().join("data.kore");
        if path.exists() {
            let block = read_file(path.to_str().unwrap())?;
            if !initialized {
                for col in block.columns() {
                    merged.add_column(&col.name, col.dtype, col.values.clone());
                }
                initialized = true;
            } else {
                for col in block.columns() {
                    if let Some(mc) = merged.columns.iter_mut().find(|c| c.name == col.name) {
                        mc.values.extend_from_slice(&col.values);
                    }
                }
            }
        }
    }
    Ok(merged)
}

