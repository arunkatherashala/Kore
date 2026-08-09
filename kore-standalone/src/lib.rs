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

// ── String Column Support ─────────────────────────────────────────────────────

/// A column that holds variable-length UTF-8 strings.
#[derive(Debug, Clone, Default)]
pub struct StringColumn {
    pub name:   String,
    pub values: Vec<String>,
}

/// A DataBlock that supports mixed numeric + string columns.
#[derive(Debug, Clone, Default)]
pub struct MixedBlock {
    pub numeric:  DataBlock,
    pub strings:  Vec<StringColumn>,
}

impl MixedBlock {
    pub fn new() -> Self { Self::default() }

    pub fn add_f64(&mut self, name: &str, values: Vec<f64>) {
        self.numeric.add_column(name, DataType::F64, values.iter().map(|&v| v.to_bits()).collect());
    }

    pub fn add_i64(&mut self, name: &str, values: Vec<i64>) {
        self.numeric.add_column(name, DataType::I64, values.iter().map(|&v| v as u64).collect());
    }

    pub fn add_str(&mut self, name: &str, values: Vec<String>) {
        self.strings.push(StringColumn { name: name.to_string(), values });
    }

    pub fn num_rows(&self) -> usize {
        self.numeric.num_rows().max(self.strings.first().map(|s| s.values.len()).unwrap_or(0))
    }

    pub fn get_str(&self, name: &str) -> Option<&StringColumn> {
        self.strings.iter().find(|s| s.name == name)
    }
}

/// Write a MixedBlock (numeric + string columns) to a .kore file.
/// Format extension: string columns stored as length-prefixed UTF-8 after numeric section.
pub fn write_mixed(path: &str, block: &MixedBlock) -> io::Result<()> {
    let bytes = to_bytes_mixed(block);
    std::fs::write(path, &bytes)
}

/// Read a MixedBlock from a .kore file.
pub fn read_mixed(path: &str) -> io::Result<MixedBlock> {
    let bytes = std::fs::read(path)?;
    from_bytes_mixed(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

const MAGIC_MIX: &[u8; 5] = b"KOREM";

fn to_bytes_mixed(block: &MixedBlock) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(MAGIC_MIX);
    buf.extend_from_slice(&4u32.to_le_bytes()); // version 4
    // numeric section
    let num_bytes = to_bytes(&block.numeric);
    buf.extend_from_slice(&(num_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(&num_bytes);
    // string section
    buf.extend_from_slice(&(block.strings.len() as u32).to_le_bytes());
    for sc in &block.strings {
        let nb = sc.name.as_bytes();
        buf.push(nb.len() as u8);
        buf.extend_from_slice(nb);
        buf.extend_from_slice(&(sc.values.len() as u32).to_le_bytes());
        for s in &sc.values {
            let sb = s.as_bytes();
            buf.extend_from_slice(&(sb.len() as u32).to_le_bytes());
            buf.extend_from_slice(sb);
        }
    }
    let cs = crc32(&buf);
    buf.extend_from_slice(&cs.to_le_bytes());
    buf
}

fn from_bytes_mixed(data: &[u8]) -> Result<MixedBlock, String> {
    if data.len() < 13 { return Err("too short".into()); }
    let (body, crc_bytes) = data.split_at(data.len() - 4);
    if crc32(body) != u32::from_le_bytes(crc_bytes.try_into().unwrap()) {
        return Err("CRC32 mismatch".into());
    }
    if &body[..5] != MAGIC_MIX { return Err("bad MIX magic".into()); }
    let mut r = &body[9..]; // skip magic(5)+version(4)
    let num_len = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize;
    r = &r[4..];
    let numeric = from_bytes(&r[..num_len])?;
    r = &r[num_len..];
    let n_str_cols = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize;
    r = &r[4..];
    let mut strings = Vec::new();
    for _ in 0..n_str_cols {
        let name_len = r[0] as usize;
        r = &r[1..];
        let name = std::str::from_utf8(&r[..name_len]).unwrap().to_string();
        r = &r[name_len..];
        let n_vals = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize;
        r = &r[4..];
        let mut values = Vec::new();
        for _ in 0..n_vals {
            let slen = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize;
            r = &r[4..];
            values.push(std::str::from_utf8(&r[..slen]).unwrap().to_string());
            r = &r[slen..];
        }
        strings.push(StringColumn { name, values });
    }
    Ok(MixedBlock { numeric, strings })
}

// ── ACID File Locking ─────────────────────────────────────────────────────────

/// Acquire an exclusive lock on a .kore file before writing.
/// Returns a `FileLock` that releases on drop.
pub struct FileLock {
    lock_path: String,
}

impl FileLock {
    /// Try to acquire lock. Retries up to `timeout_ms` milliseconds.
    pub fn acquire(path: &str, timeout_ms: u64) -> io::Result<Self> {
        let lock_path = format!("{}.lock", path);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            match std::fs::OpenOptions::new().create_new(true).write(true).open(&lock_path) {
                Ok(_) => return Ok(FileLock { lock_path }),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    if std::time::Instant::now() > deadline {
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "lock timeout"));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => return Err(e),
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.lock_path); }
}

/// Write a DataBlock with ACID locking (safe for concurrent writers).
pub fn write_file_locked(path: &str, block: &DataBlock, timeout_ms: u64) -> io::Result<()> {
    let _lock = FileLock::acquire(path, timeout_ms)?;
    write_file(path, block)
}

/// Atomic append with ACID locking.
pub fn append_file_locked(path: &str, new_block: &DataBlock, timeout_ms: u64) -> io::Result<()> {
    let _lock = FileLock::acquire(path, timeout_ms)?;
    append_file(path, new_block)
}

// ── Bloom Filter ──────────────────────────────────────────────────────────────

/// A basic bloom filter for fast membership testing (zero false negatives, low false positives).
#[derive(Debug, Clone)]
pub struct BloomFilter {
    bits:     Vec<u64>,
    n_hashes: u32,
    n_bits:   u32,
}

impl BloomFilter {
    /// Create a bloom filter for `capacity` items with ~1% false positive rate.
    pub fn new(capacity: usize) -> Self {
        let n_bits = ((capacity as f64 * 9.585) as u32).max(64);
        let words = ((n_bits + 63) / 64) as usize;
        BloomFilter { bits: vec![0u64; words], n_hashes: 7, n_bits }
    }

    /// Insert a value into the filter.
    pub fn insert(&mut self, value: u64) {
        for i in 0..self.n_hashes {
            let h = self.hash(value, i);
            let bit = (h % self.n_bits as u64) as usize;
            self.bits[bit / 64] |= 1u64 << (bit % 64);
        }
    }

    /// Test if a value MIGHT be in the filter (no false negatives).
    pub fn contains(&self, value: u64) -> bool {
        (0..self.n_hashes).all(|i| {
            let h = self.hash(value, i);
            let bit = (h % self.n_bits as u64) as usize;
            self.bits[bit / 64] & (1u64 << (bit % 64)) != 0
        })
    }

    fn hash(&self, value: u64, seed: u32) -> u64 {
        // FNV-1a inspired mixing
        let mut h = value ^ (seed as u64 * 0x517CC1B727220A95);
        h ^= h >> 33; h = h.wrapping_mul(0xff51afd7ed558ccd);
        h ^= h >> 33; h = h.wrapping_mul(0xc4ceb9fe1a85ec53);
        h ^ (h >> 33)
    }

    /// Build a bloom filter from a column's values.
    pub fn from_column(col: &Column) -> Self {
        let mut bf = BloomFilter::new(col.values.len());
        for &v in &col.values { bf.insert(v); }
        bf
    }
}

/// Build bloom filters for all columns in a block (for fast scan skipping).
pub fn build_bloom_filters(block: &DataBlock) -> std::collections::HashMap<String, BloomFilter> {
    block.columns().iter().map(|c| (c.name.clone(), BloomFilter::from_column(c))).collect()
}

// ── Delta / Merge (Upsert) ────────────────────────────────────────────────────

/// Merge `delta` into an existing file using `key_col` as the join key.
/// - Matching rows are UPDATED with delta values.
/// - Non-matching delta rows are INSERTED.
/// - Rows in base not in delta are kept unchanged (no delete by default).
pub fn merge_into(path: &str, delta: &DataBlock, key_col: &str) -> io::Result<()> {
    let mut base = read_file(path)?;
    let Some(base_keys) = base.column(key_col).map(|c| c.values.clone()) else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "key column not found in base"));
    };
    let Some(delta_keys) = delta.column(key_col) else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "key column not found in delta"));
    };

    // Build index: key → base row index
    let mut key_to_idx: std::collections::HashMap<u64, usize> = base_keys.iter()
        .enumerate().map(|(i, &k)| (k, i)).collect();

    // UPDATE matching rows, collect INSERT indices
    let mut inserts: Vec<usize> = Vec::new();
    for (di, &dk) in delta_keys.values.iter().enumerate() {
        if let Some(&bi) = key_to_idx.get(&dk) {
            // UPDATE: overwrite base[bi] with delta[di]
            for col in base.columns.iter_mut() {
                if let Some(dc) = delta.column(&col.name) {
                    col.values[bi] = dc.values[di];
                }
            }
        } else {
            inserts.push(di);
            key_to_idx.insert(dk, base.num_rows() + inserts.len() - 1);
        }
    }

    // INSERT new rows
    for di in inserts {
        for col in base.columns.iter_mut() {
            if let Some(dc) = delta.column(&col.name) {
                col.values.push(dc.values[di]);
            } else {
                col.values.push(0); // default for missing columns
            }
        }
    }

    write_file(path, &base)
}

/// Delete rows from a .kore file where `key_col` value is in `delete_keys`.
pub fn delete_rows(path: &str, key_col: &str, delete_keys: &[u64]) -> io::Result<()> {
    let base = read_file(path)?;
    let Some(kc) = base.column(key_col) else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "key column not found"));
    };
    let del_set: std::collections::HashSet<u64> = delete_keys.iter().copied().collect();
    let keep: Vec<usize> = kc.values.iter().enumerate()
        .filter_map(|(i, &v)| if !del_set.contains(&v) { Some(i) } else { None })
        .collect();
    let mut result = DataBlock::new();
    for col in base.columns() {
        result.add_column(&col.name, col.dtype, keep.iter().map(|&i| col.values[i]).collect());
    }
    write_file(path, &result)
}


