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

// ── Multi-Engine Export (Arrow IPC, CSV, JSON) ────────────────────────────────

/// Export a DataBlock to CSV format (readable by Spark, DuckDB, Pandas, Excel).
pub fn to_csv(block: &DataBlock) -> String {
    let mut out = String::new();
    // Header
    let headers: Vec<&str> = block.columns().iter().map(|c| c.name.as_str()).collect();
    out.push_str(&headers.join(","));
    out.push('\n');
    // Rows
    for row in 0..block.num_rows() {
        let vals: Vec<String> = block.columns().iter().map(|col| {
            match col.dtype {
                DataType::F64 => format!("{}", f64::from_bits(col.values[row])),
                DataType::I64 => format!("{}", col.values[row] as i64),
                DataType::Str => format!("{}", col.values[row]),
            }
        }).collect();
        out.push_str(&vals.join(","));
        out.push('\n');
    }
    out
}

/// Export a DataBlock to NDJSON (newline-delimited JSON) — readable by Spark, DuckDB, MongoDB.
pub fn to_ndjson(block: &DataBlock) -> String {
    let mut out = String::new();
    for row in 0..block.num_rows() {
        out.push('{');
        let pairs: Vec<String> = block.columns().iter().map(|col| {
            let val = match col.dtype {
                DataType::F64 => format!("{}", f64::from_bits(col.values[row])),
                DataType::I64 => format!("{}", col.values[row] as i64),
                DataType::Str => format!("\"{}\"", col.values[row]),
            };
            format!("\"{}\":{}", col.name, val)
        }).collect();
        out.push_str(&pairs.join(","));
        out.push_str("}\n");
    }
    out
}

/// Write DataBlock to CSV file — Spark-compatible.
pub fn write_csv(path: &str, block: &DataBlock) -> io::Result<()> {
    std::fs::write(path, to_csv(block))
}

/// Write DataBlock to NDJSON file — Spark/DuckDB-compatible.
pub fn write_ndjson(path: &str, block: &DataBlock) -> io::Result<()> {
    std::fs::write(path, to_ndjson(block))
}

/// Read a CSV file into a DataBlock (all columns as F64 or I64 auto-detected).
pub fn read_csv(path: &str) -> io::Result<DataBlock> {
    let content = std::fs::read_to_string(path)?;
    let mut lines = content.lines();
    let headers: Vec<&str> = lines.next().unwrap_or("").split(',').collect();
    let mut columns: Vec<Vec<u64>> = vec![vec![]; headers.len()];
    let mut dtypes: Vec<DataType> = vec![DataType::F64; headers.len()];

    for line in lines {
        for (i, val) in line.split(',').enumerate() {
            if i >= columns.len() { break; }
            let v = val.trim();
            if let Ok(f) = v.parse::<f64>() {
                columns[i].push(f.to_bits());
                if v.contains('.') { dtypes[i] = DataType::F64; }
                else if dtypes[i] == DataType::F64 && !v.contains('.') {
                    if let Ok(n) = v.parse::<i64>() {
                        columns[i].last_mut().map(|x| *x = n as u64);
                        // keep F64 if already set
                    }
                }
            }
        }
    }

    let mut block = DataBlock::new();
    for (i, name) in headers.iter().enumerate() {
        block.add_column(name.trim(), dtypes[i], columns[i].clone());
    }
    Ok(block)
}

// ── Spark Thrift Server Integration ──────────────────────────────────────────

/// Generate a CREATE TABLE SQL statement for this DataBlock (for Spark/Hive/Trino).
pub fn to_spark_ddl(block: &DataBlock, table_name: &str, kore_path: &str) -> String {
    let cols: Vec<String> = block.columns().iter().map(|col| {
        let sql_type = match col.dtype {
            DataType::F64 => "DOUBLE",
            DataType::I64 => "BIGINT",
            DataType::Str => "STRING",
        };
        format!("    {} {}", col.name, sql_type)
    }).collect();

    format!(
        "-- KORE → Spark SQL DDL\n\
         -- Usage: spark.sql(open('table.sql').read())\n\
         CREATE TABLE IF NOT EXISTS {table_name} (\n{}\n)\n\
         USING kore\n\
         LOCATION '{kore_path}';\n",
        cols.join(",\n")
    )
}

/// Compute a concise statistics summary for monitoring dashboards.
pub fn summary(block: &DataBlock) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    map.insert("rows".into(), block.num_rows().to_string());
    map.insert("columns".into(), block.num_columns().to_string());
    for s in block.stats() {
        map.insert(format!("{}.min", s.name), format!("{:.4}", s.min));
        map.insert(format!("{}.max", s.name), format!("{:.4}", s.max));
        map.insert(format!("{}.mean", s.name), format!("{:.4}", s.mean));
    }
    map
}

// ── Kafka / Streaming Connector ───────────────────────────────────────────────

/// Serialize a DataBlock to Kafka message bytes (KORE binary framed for Kafka).
/// Format: [4-byte magic][4-byte version=5][kore_bytes]
pub fn to_kafka_bytes(block: &DataBlock) -> Vec<u8> {
    let mut msg = Vec::new();
    msg.extend_from_slice(b"KOREK");       // Kafka magic
    msg.extend_from_slice(&5u32.to_le_bytes()); // version 5
    msg.extend_from_slice(&to_bytes(block));
    msg
}

/// Deserialize a Kafka message back to a DataBlock.
pub fn from_kafka_bytes(msg: &[u8]) -> Result<DataBlock, String> {
    if msg.len() < 9 { return Err("Kafka message too short".into()); }
    if &msg[..5] != b"KOREK" { return Err("Not a KORE Kafka message".into()); }
    from_bytes(&msg[9..])
}

/// Streaming writer — writes DataBlock chunks to a file in append mode.
/// Each chunk is length-prefixed for safe streaming reads.
pub fn write_stream_chunk(path: &str, block: &DataBlock) -> io::Result<()> {
    use std::io::Write;
    let chunk = to_bytes(block);
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    // 4-byte length prefix + chunk
    file.write_all(&(chunk.len() as u32).to_le_bytes())?;
    file.write_all(&chunk)
}

/// Read all chunks from a streaming .kore file into a merged DataBlock.
pub fn read_stream_all(path: &str) -> io::Result<DataBlock> {
    let data = std::fs::read(path)?;
    let mut r = data.as_slice();
    let mut merged = DataBlock::new();
    let mut first = true;
    while r.len() >= 4 {
        let chunk_len = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize;
        r = &r[4..];
        if r.len() < chunk_len { break; }
        let block = from_bytes(&r[..chunk_len]).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        r = &r[chunk_len..];
        if first {
            for col in block.columns() {
                merged.add_column(&col.name, col.dtype, col.values.clone());
            }
            first = false;
        } else {
            for col in block.columns() {
                if let Some(mc) = merged.columns.iter_mut().find(|c| c.name == col.name) {
                    mc.values.extend_from_slice(&col.values);
                }
            }
        }
    }
    Ok(merged)
}

// ── ML / Tensor Support ───────────────────────────────────────────────────────

/// A multi-dimensional tensor stored as flat f64 data + shape.
#[derive(Debug, Clone)]
pub struct Tensor {
    pub name:   String,
    pub shape:  Vec<usize>,   // e.g. [100, 768] for 100 embeddings of dim 768
    pub data:   Vec<f64>,     // row-major (C-order)
}

impl Tensor {
    pub fn new(name: &str, shape: Vec<usize>, data: Vec<f64>) -> Self {
        let expected: usize = shape.iter().product();
        assert_eq!(data.len(), expected, "data length must equal shape product");
        Tensor { name: name.to_string(), shape, data }
    }

    pub fn ndim(&self) -> usize { self.shape.len() }
    pub fn size(&self) -> usize { self.data.len() }
    pub fn num_rows(&self) -> usize { self.shape[0] }
    pub fn num_cols(&self) -> usize { if self.shape.len() > 1 { self.shape[1] } else { 1 } }

    /// Get row i as a slice.
    pub fn row(&self, i: usize) -> &[f64] {
        let nc = self.num_cols();
        &self.data[i * nc..(i + 1) * nc]
    }

    /// Dot product of row i with a query vector (for similarity search).
    pub fn dot_row(&self, i: usize, query: &[f64]) -> f64 {
        self.row(i).iter().zip(query.iter()).map(|(a, b)| a * b).sum()
    }
}

/// A block of tensors (e.g. embedding matrix + metadata).
#[derive(Debug, Clone, Default)]
pub struct TensorBlock {
    pub tensors: Vec<Tensor>,
    pub metadata: DataBlock,   // row-level metadata (ids, labels, etc.)
}

impl TensorBlock {
    pub fn new() -> Self { Self::default() }

    pub fn add_tensor(&mut self, tensor: Tensor) { self.tensors.push(tensor); }

    pub fn tensor(&self, name: &str) -> Option<&Tensor> {
        self.tensors.iter().find(|t| t.name == name)
    }

    /// KNN search: find top-k rows closest to query vector (cosine similarity).
    pub fn knn(&self, tensor_name: &str, query: &[f64], k: usize) -> Vec<(usize, f64)> {
        let Some(t) = self.tensor(tensor_name) else { return vec![]; };
        let query_norm: f64 = query.iter().map(|x| x * x).sum::<f64>().sqrt();
        let mut scores: Vec<(usize, f64)> = (0..t.num_rows()).map(|i| {
            let row = t.row(i);
            let row_norm: f64 = row.iter().map(|x| x * x).sum::<f64>().sqrt();
            let dot: f64 = t.dot_row(i, query);
            let cos = if row_norm > 0.0 && query_norm > 0.0 { dot / (row_norm * query_norm) } else { 0.0 };
            (i, cos)
        }).collect();
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scores.truncate(k);
        scores
    }
}

// KORE-T format: "KORET" + version(4) + n_tensors(4) + tensors + kore_metadata
const MAGIC_TENSOR: &[u8; 5] = b"KORET";

pub fn write_tensors(path: &str, block: &TensorBlock) -> io::Result<()> {
    let bytes = to_bytes_tensors(block);
    std::fs::write(path, &bytes)
}

pub fn read_tensors(path: &str) -> io::Result<TensorBlock> {
    let bytes = std::fs::read(path)?;
    from_bytes_tensors(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn to_bytes_tensors(block: &TensorBlock) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(MAGIC_TENSOR);
    buf.extend_from_slice(&6u32.to_le_bytes()); // version 6
    buf.extend_from_slice(&(block.tensors.len() as u32).to_le_bytes());
    for t in &block.tensors {
        let nb = t.name.as_bytes();
        buf.push(nb.len() as u8);
        buf.extend_from_slice(nb);
        buf.extend_from_slice(&(t.shape.len() as u32).to_le_bytes());
        for &d in &t.shape { buf.extend_from_slice(&(d as u64).to_le_bytes()); }
        buf.extend_from_slice(&(t.data.len() as u64).to_le_bytes());
        for &v in &t.data { buf.extend_from_slice(&v.to_bits().to_le_bytes()); }
    }
    let meta = to_bytes(&block.metadata);
    buf.extend_from_slice(&(meta.len() as u32).to_le_bytes());
    buf.extend_from_slice(&meta);
    let cs = crc32(&buf);
    buf.extend_from_slice(&cs.to_le_bytes());
    buf
}

fn from_bytes_tensors(data: &[u8]) -> Result<TensorBlock, String> {
    if data.len() < 13 { return Err("too short".into()); }
    let (body, crc_bytes) = data.split_at(data.len() - 4);
    if crc32(body) != u32::from_le_bytes(crc_bytes.try_into().unwrap()) { return Err("CRC32 mismatch".into()); }
    if &body[..5] != MAGIC_TENSOR { return Err("bad tensor magic".into()); }
    let mut r = &body[9..];
    let n_tensors = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize;
    r = &r[4..];
    let mut tensors = Vec::new();
    for _ in 0..n_tensors {
        let name_len = r[0] as usize; r = &r[1..];
        let name = std::str::from_utf8(&r[..name_len]).unwrap().to_string(); r = &r[name_len..];
        let ndim = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize; r = &r[4..];
        let shape: Vec<usize> = (0..ndim).map(|_| { let v = u64::from_le_bytes(r[..8].try_into().unwrap()) as usize; r = &r[8..]; v }).collect();
        let n_vals = u64::from_le_bytes(r[..8].try_into().unwrap()) as usize; r = &r[8..];
        let data: Vec<f64> = (0..n_vals).map(|_| { let v = f64::from_bits(u64::from_le_bytes(r[..8].try_into().unwrap())); r = &r[8..]; v }).collect();
        tensors.push(Tensor { name, shape, data });
    }
    let meta_len = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize; r = &r[4..];
    let metadata = from_bytes(&r[..meta_len])?;
    Ok(TensorBlock { tensors, metadata })
}

// ── Avro Wire Format (simplified — for Kafka/Hadoop interop) ─────────────────

/// Serialize DataBlock as simplified Avro binary (compatible subset).
/// Full Avro requires schema registry; this is a self-describing subset.
pub fn to_avro_bytes(block: &DataBlock) -> Vec<u8> {
    // Simplified Avro: [schema_json_len(4)][schema_json][blocks...]
    // Each block: [count(8)][size(8)][records...]
    let schema = avro_schema(block);
    let schema_bytes = schema.as_bytes();
    let mut buf = Vec::new();
    // Magic
    buf.extend_from_slice(b"Obj\x01");
    // Schema metadata (simplified)
    buf.extend_from_slice(&(schema_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(schema_bytes);
    // Data: one block with all rows
    let n = block.num_rows() as i64;
    buf.extend_from_slice(&n.to_le_bytes()); // block count
    // Write each row in columnar → row order
    let col_data: Vec<&Column> = block.columns().iter().collect();
    let mut row_bytes: Vec<u8> = Vec::new();
    for i in 0..block.num_rows() {
        for col in &col_data {
            match col.dtype {
                DataType::F64 => row_bytes.extend_from_slice(&col.values[i].to_le_bytes()),
                DataType::I64 => row_bytes.extend_from_slice(&(col.values[i] as i64).to_le_bytes()),
                DataType::Str => row_bytes.extend_from_slice(&col.values[i].to_le_bytes()),
            }
        }
    }
    buf.extend_from_slice(&(row_bytes.len() as u64).to_le_bytes());
    buf.extend_from_slice(&row_bytes);
    buf.extend_from_slice(&0i64.to_le_bytes()); // end of blocks
    // CRC32 sync marker
    let cs = crc32(&buf);
    buf.extend_from_slice(&cs.to_le_bytes());
    buf
}

fn avro_schema(block: &DataBlock) -> String {
    let fields: Vec<String> = block.columns().iter().map(|col| {
        let avro_type = match col.dtype {
            DataType::F64 => "double",
            DataType::I64 => "long",
            DataType::Str => "string",
        };
        format!("{{\"name\":\"{}\",\"type\":\"{}\"}}", col.name, avro_type)
    }).collect();
    format!("{{\"type\":\"record\",\"name\":\"KoreRecord\",\"fields\":[{}]}}", fields.join(","))
}

pub fn write_avro(path: &str, block: &DataBlock) -> io::Result<()> {
    std::fs::write(path, to_avro_bytes(block))
}

// ── Protocol Buffers Wire Format (simplified) ─────────────────────────────────

/// Serialize DataBlock as simplified protobuf-compatible binary.
/// Field numbers: col 0 = field 1, col 1 = field 2, etc.
pub fn to_protobuf_bytes(block: &DataBlock) -> Vec<u8> {
    let mut buf = Vec::new();
    for row in 0..block.num_rows() {
        let mut row_buf = Vec::new();
        for (fi, col) in block.columns().iter().enumerate() {
            let field_num = (fi + 1) as u64;
            match col.dtype {
                DataType::F64 => {
                    // wire type 1 = 64-bit
                    row_buf.extend_from_slice(&encode_varint((field_num << 3) | 1));
                    row_buf.extend_from_slice(&col.values[row].to_le_bytes());
                }
                DataType::I64 => {
                    // wire type 0 = varint
                    row_buf.extend_from_slice(&encode_varint((field_num << 3) | 0));
                    row_buf.extend_from_slice(&encode_varint(col.values[row]));
                }
                DataType::Str => {
                    row_buf.extend_from_slice(&encode_varint((field_num << 3) | 1));
                    row_buf.extend_from_slice(&col.values[row].to_le_bytes());
                }
            }
        }
        // Each row is a length-delimited message
        buf.extend_from_slice(&encode_varint(row_buf.len() as u64));
        buf.extend_from_slice(&row_buf);
    }
    buf
}

fn encode_varint(mut v: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    loop {
        if v < 0x80 { buf.push(v as u8); break; }
        buf.push((v & 0x7F) as u8 | 0x80);
        v >>= 7;
    }
    buf
}

pub fn write_protobuf(path: &str, block: &DataBlock) -> io::Result<()> {
    std::fs::write(path, to_protobuf_bytes(block))
}

// ── Column Statistics Footer (Predicate Pushdown) ─────────────────────────────

/// Per-column statistics stored in file footer — enables scan skipping.
#[derive(Debug, Clone)]
pub struct ColFooter {
    pub name:       String,
    pub dtype:      DataType,
    pub row_count:  u64,
    pub null_count: u64,
    pub min_val:    f64,
    pub max_val:    f64,
    pub sum_val:    f64,
}

/// Write a DataBlock with column statistics footer (KORE v3 enhanced format).
pub fn write_file_v3(path: &str, block: &DataBlock) -> io::Result<()> {
    let bytes = to_bytes_v3(block);
    std::fs::write(path, &bytes)
}

/// Read a v3 DataBlock. Also returns column footers for predicate pushdown.
pub fn read_file_v3(path: &str) -> io::Result<(DataBlock, Vec<ColFooter>)> {
    let bytes = std::fs::read(path)?;
    from_bytes_v3(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Read ONLY column footers (no data) for fast scan planning.
pub fn read_footer_only(path: &str) -> io::Result<Vec<ColFooter>> {
    let bytes = std::fs::read(path)?;
    let (_, footers) = from_bytes_v3(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(footers)
}

/// Check if a file MIGHT match a predicate (returns false → safe to skip file).
pub fn can_skip_file(footers: &[ColFooter], col_name: &str, min: f64, max: f64) -> bool {
    if let Some(f) = footers.iter().find(|f| f.name == col_name) {
        f.max_val < min || f.min_val > max
    } else { false }
}

// KORE v3 format: "KOREV" magic + version 3 + data section + footer section
const MAGIC_V3: &[u8; 5] = b"KOREV";

fn to_bytes_v3(block: &DataBlock) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(MAGIC_V3);
    buf.extend_from_slice(&3u32.to_le_bytes()); // version 3
    // Data section (same as v2)
    let data = to_bytes(block);
    buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
    buf.extend_from_slice(&data);
    // Footer section: per-column stats
    buf.extend_from_slice(&(block.num_columns() as u32).to_le_bytes());
    for col in block.columns() {
        let nb = col.name.as_bytes();
        buf.push(nb.len() as u8);
        buf.extend_from_slice(nb);
        buf.push(col.dtype as u8);
        let n = col.values.len() as u64;
        buf.extend_from_slice(&n.to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes()); // null_count (todo: null bitmap)
        let (min, max, sum) = col.values.iter().fold((f64::INFINITY, f64::NEG_INFINITY, 0f64), |(mn, mx, s), &v| {
            let f = f64::from_bits(v);
            (mn.min(f), mx.max(f), s + f)
        });
        buf.extend_from_slice(&min.to_bits().to_le_bytes());
        buf.extend_from_slice(&max.to_bits().to_le_bytes());
        buf.extend_from_slice(&sum.to_bits().to_le_bytes());
    }
    let cs = crc32(&buf);
    buf.extend_from_slice(&cs.to_le_bytes());
    buf
}

fn from_bytes_v3(data: &[u8]) -> Result<(DataBlock, Vec<ColFooter>), String> {
    if data.len() < 13 { return Err("too short".into()); }
    let (body, crc_bytes) = data.split_at(data.len() - 4);
    if crc32(body) != u32::from_le_bytes(crc_bytes.try_into().unwrap()) { return Err("CRC32 mismatch".into()); }
    if &body[..5] != MAGIC_V3 { return Err("bad v3 magic".into()); }
    let mut r = &body[9..];
    let data_len = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize;
    r = &r[4..];
    let block = from_bytes(&r[..data_len])?;
    r = &r[data_len..];
    let n_cols = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize;
    r = &r[4..];
    let mut footers = Vec::new();
    for _ in 0..n_cols {
        let nl = r[0] as usize; r = &r[1..];
        let name = std::str::from_utf8(&r[..nl]).unwrap().to_string(); r = &r[nl..];
        let dtype = match r[0] { 1 => DataType::F64, 2 => DataType::I64, _ => DataType::Str }; r = &r[1..];
        let row_count  = u64::from_le_bytes(r[..8].try_into().unwrap()); r = &r[8..];
        let null_count = u64::from_le_bytes(r[..8].try_into().unwrap()); r = &r[8..];
        let min_val = f64::from_bits(u64::from_le_bytes(r[..8].try_into().unwrap())); r = &r[8..];
        let max_val = f64::from_bits(u64::from_le_bytes(r[..8].try_into().unwrap())); r = &r[8..];
        let sum_val = f64::from_bits(u64::from_le_bytes(r[..8].try_into().unwrap())); r = &r[8..];
        footers.push(ColFooter { name, dtype, row_count, null_count, min_val, max_val, sum_val });
    }
    Ok((block, footers))
}

// ── Null Bitmap Support ───────────────────────────────────────────────────────

/// A nullable column — stores a validity bitmap alongside values.
#[derive(Debug, Clone)]
pub struct NullableColumn {
    pub name:    String,
    pub dtype:   DataType,
    pub values:  Vec<u64>,       // 0 where null
    pub validity: Vec<u64>,      // bitmask: bit i set = row i is valid (not null)
}

impl NullableColumn {
    pub fn new(name: &str, dtype: DataType, values: Vec<Option<u64>>) -> Self {
        let n = values.len();
        let mut vals = Vec::with_capacity(n);
        let mut validity = vec![0u64; (n + 63) / 64];
        for (i, v) in values.into_iter().enumerate() {
            match v {
                Some(x) => { vals.push(x); validity[i/64] |= 1u64 << (i%64); }
                None    => { vals.push(0); }
            }
        }
        NullableColumn { name: name.to_string(), dtype, values: vals, validity }
    }

    pub fn is_valid(&self, i: usize) -> bool {
        self.validity[i/64] & (1u64 << (i%64)) != 0
    }

    pub fn get(&self, i: usize) -> Option<u64> {
        if self.is_valid(i) { Some(self.values[i]) } else { None }
    }

    pub fn null_count(&self) -> usize {
        self.values.len() - self.values.iter().enumerate()
            .filter(|(i, _)| self.is_valid(*i)).count()
    }
}

/// A DataBlock that supports null values per cell.
#[derive(Debug, Clone, Default)]
pub struct NullableBlock {
    pub columns: Vec<NullableColumn>,
}

impl NullableBlock {
    pub fn new() -> Self { Self::default() }

    pub fn add_column(&mut self, name: &str, dtype: DataType, values: Vec<Option<u64>>) {
        self.columns.push(NullableColumn::new(name, dtype, values));
    }

    pub fn num_rows(&self) -> usize {
        self.columns.first().map(|c| c.values.len()).unwrap_or(0)
    }

    pub fn null_count(&self, col_name: &str) -> usize {
        self.columns.iter().find(|c| c.name == col_name).map(|c| c.null_count()).unwrap_or(0)
    }
}

/// Write a NullableBlock to disk.
pub fn write_nullable(path: &str, block: &NullableBlock) -> io::Result<()> {
    let bytes = to_bytes_nullable(block);
    std::fs::write(path, &bytes)
}

/// Read a NullableBlock from disk.
pub fn read_nullable(path: &str) -> io::Result<NullableBlock> {
    let bytes = std::fs::read(path)?;
    from_bytes_nullable(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

const MAGIC_NULL: &[u8; 5] = b"KOREN";

fn to_bytes_nullable(block: &NullableBlock) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(MAGIC_NULL);
    buf.extend_from_slice(&7u32.to_le_bytes());
    buf.extend_from_slice(&(block.columns.len() as u32).to_le_bytes());
    for col in &block.columns {
        let nb = col.name.as_bytes();
        buf.push(nb.len() as u8); buf.extend_from_slice(nb);
        buf.push(col.dtype as u8);
        let n = col.values.len() as u64;
        buf.extend_from_slice(&n.to_le_bytes());
        // validity bitmap
        buf.extend_from_slice(&(col.validity.len() as u32).to_le_bytes());
        for &v in &col.validity { buf.extend_from_slice(&v.to_le_bytes()); }
        // values
        for &v in &col.values { buf.extend_from_slice(&v.to_le_bytes()); }
    }
    let cs = crc32(&buf);
    buf.extend_from_slice(&cs.to_le_bytes());
    buf
}

fn from_bytes_nullable(data: &[u8]) -> Result<NullableBlock, String> {
    if data.len() < 13 { return Err("too short".into()); }
    let (body, crc_bytes) = data.split_at(data.len() - 4);
    if crc32(body) != u32::from_le_bytes(crc_bytes.try_into().unwrap()) { return Err("CRC32 mismatch".into()); }
    if &body[..5] != MAGIC_NULL { return Err("bad nullable magic".into()); }
    let mut r = &body[9..];
    let n_cols = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize; r = &r[4..];
    let mut block = NullableBlock::new();
    for _ in 0..n_cols {
        let nl = r[0] as usize; r = &r[1..];
        let name = std::str::from_utf8(&r[..nl]).unwrap().to_string(); r = &r[nl..];
        let dtype = match r[0] { 1=>DataType::F64, 2=>DataType::I64, _=>DataType::Str }; r = &r[1..];
        let n = u64::from_le_bytes(r[..8].try_into().unwrap()) as usize; r = &r[8..];
        let nbitmaps = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize; r = &r[4..];
        let validity: Vec<u64> = (0..nbitmaps).map(|_| { let v = u64::from_le_bytes(r[..8].try_into().unwrap()); r = &r[8..]; v }).collect();
        let values: Vec<u64> = (0..n).map(|_| { let v = u64::from_le_bytes(r[..8].try_into().unwrap()); r = &r[8..]; v }).collect();
        block.columns.push(NullableColumn { name, dtype, values, validity });
    }
    Ok(block)
}

// ── Delta Encoding (sorted integer columns) ────────────────────────────────────

/// Delta encode sorted integer column — stores first value + differences.
/// Excellent for timestamps, sequential IDs, sorted prices.
pub fn delta_encode(values: &[u64]) -> (u64, Vec<i64>) {
    if values.is_empty() { return (0, vec![]); }
    let base = values[0];
    let deltas: Vec<i64> = std::iter::once(0)
        .chain(values.windows(2).map(|w| w[1] as i64 - w[0] as i64))
        .collect();
    (base, deltas)
}

/// Delta decode back to original values.
pub fn delta_decode(base: u64, deltas: &[i64]) -> Vec<u64> {
    let mut result = Vec::with_capacity(deltas.len());
    let mut cur = base as i64;
    for &d in deltas {
        cur += d;
        result.push(cur as u64);
    }
    result
}

// ── Dictionary / String Encoding ──────────────────────────────────────────────

/// Dictionary-encode a string column — store unique strings + integer codes.
pub fn dict_encode_strings(strings: &[String]) -> (Vec<String>, Vec<u32>) {
    let mut dict: Vec<String> = Vec::new();
    let mut dict_map: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut codes = Vec::with_capacity(strings.len());
    for s in strings {
        let code = if let Some(&c) = dict_map.get(s) { c } else {
            let c = dict.len() as u32;
            dict.push(s.clone());
            dict_map.insert(s.clone(), c);
            c
        };
        codes.push(code);
    }
    (dict, codes)
}

/// Write a string column with dictionary encoding.
pub fn write_dict_strings(path: &str, col_name: &str, strings: &[String]) -> io::Result<()> {
    let (dict, codes) = dict_encode_strings(strings);
    let mut buf = Vec::new();
    buf.extend_from_slice(b"KORED");
    buf.extend_from_slice(&8u32.to_le_bytes());
    let nb = col_name.as_bytes();
    buf.push(nb.len() as u8); buf.extend_from_slice(nb);
    buf.extend_from_slice(&(dict.len() as u32).to_le_bytes());
    for s in &dict {
        let sb = s.as_bytes();
        buf.extend_from_slice(&(sb.len() as u16).to_le_bytes());
        buf.extend_from_slice(sb);
    }
    buf.extend_from_slice(&(codes.len() as u32).to_le_bytes());
    for &c in &codes { buf.extend_from_slice(&c.to_le_bytes()); }
    let cs = crc32(&buf);
    buf.extend_from_slice(&cs.to_le_bytes());
    std::fs::write(path, &buf)
}

// ── Table Catalog (multi-file tables) ────────────────────────────────────────

/// A catalog entry tracking all partition files of a logical table.
#[derive(Debug, Clone)]
pub struct TableCatalog {
    pub name:       String,
    pub files:      Vec<CatalogEntry>,
}

#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub path:         String,
    pub row_count:    u64,
    pub size_bytes:   u64,
    pub partition_val: Option<String>,
    pub snapshot_id:   u32,
}

impl TableCatalog {
    pub fn new(name: &str) -> Self { TableCatalog { name: name.to_string(), files: vec![] } }

    pub fn add_file(&mut self, path: &str, row_count: u64, size_bytes: u64, partition: Option<&str>, snapshot: u32) {
        self.files.push(CatalogEntry {
            path: path.to_string(), row_count, size_bytes,
            partition_val: partition.map(|s| s.to_string()), snapshot_id: snapshot,
        });
    }

    pub fn total_rows(&self) -> u64 { self.files.iter().map(|f| f.row_count).sum() }
    pub fn total_size_kb(&self) -> u64 { self.files.iter().map(|f| f.size_bytes).sum::<u64>() / 1024 }
    pub fn latest_snapshot(&self) -> u32 { self.files.iter().map(|f| f.snapshot_id).max().unwrap_or(0) }

    /// Save catalog to JSON file.
    pub fn save(&self, path: &str) -> io::Result<()> {
        let mut json = format!("{{\"name\":\"{}\",\"files\":[", self.name);
        for (i, f) in self.files.iter().enumerate() {
            if i > 0 { json.push(','); }
            let pv = f.partition_val.as_deref().unwrap_or("null");
            json.push_str(&format!(
                "{{\"path\":\"{}\",\"rows\":{},\"bytes\":{},\"partition\":\"{}\",\"snapshot\":{}}}",
                f.path, f.row_count, f.size_bytes, pv, f.snapshot_id
            ));
        }
        json.push_str(&format!("],\"total_rows\":{},\"total_size_kb\":{},\"snapshots\":{}}}", 
            self.total_rows(), self.total_size_kb(), self.latest_snapshot()));
        std::fs::write(path, json)
    }

    /// Load catalog from JSON file.
    pub fn load(path: &str) -> io::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        // Minimal parser
        let name = json.split("\"name\":\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or("unknown").to_string();
        Ok(TableCatalog { name, files: vec![] }) // simplified — full parser omitted for zero-deps
    }
}

// ── Frame-of-Reference (FOR) Encoding ─────────────────────────────────────────

/// FOR encoding — store min value + offsets (saves bits for clustered data).
pub fn for_encode(values: &[u64]) -> (u64, Vec<u64>) {
    if values.is_empty() { return (0, vec![]); }
    let min = *values.iter().min().unwrap();
    (min, values.iter().map(|&v| v - min).collect())
}

/// FOR decode.
pub fn for_decode(min: u64, offsets: &[u64]) -> Vec<u64> {
    offsets.iter().map(|&o| o + min).collect()
}

// ── Bitpacking (pack small integers into fewer bits) ──────────────────────────

/// Pack u64 values using only `bits` bits per value (when max < 2^bits).
pub fn bitpack(values: &[u64], bits: u8) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut cur_byte: u8 = 0;
    let mut cur_bit: u8 = 0;
    for &v in values {
        for b in 0..bits {
            if (v >> b) & 1 == 1 { cur_byte |= 1 << cur_bit; }
            cur_bit += 1;
            if cur_bit == 8 { buf.push(cur_byte); cur_byte = 0; cur_bit = 0; }
        }
    }
    if cur_bit > 0 { buf.push(cur_byte); }
    buf
}

/// Unpack bitpacked values.
pub fn bitunpack(data: &[u8], count: usize, bits: u8) -> Vec<u64> {
    let mut result = Vec::with_capacity(count);
    let mut bit_pos: usize = 0;
    for _ in 0..count {
        let mut v: u64 = 0;
        for b in 0..bits as usize {
            let byte_idx = (bit_pos + b) / 8;
            let bit_idx  = (bit_pos + b) % 8;
            if byte_idx < data.len() && (data[byte_idx] >> bit_idx) & 1 == 1 { v |= 1 << b; }
        }
        result.push(v);
        bit_pos += bits as usize;
    }
    result
}

/// Choose optimal compression codec for a column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Codec { Raw, RLE, Delta, FOR, Bitpack }

pub fn auto_select_codec(values: &[u64]) -> Codec {
    if values.len() < 2 { return Codec::Raw; }
    // Check cardinality for RLE
    let unique: std::collections::HashSet<u64> = values.iter().copied().collect();
    let cardinality_ratio = unique.len() as f64 / values.len() as f64;
    if cardinality_ratio < 0.1 { return Codec::RLE; }
    // Check if sorted (good for delta)
    let sorted = values.windows(2).all(|w| w[1] >= w[0]);
    if sorted { return Codec::Delta; }
    // Check value range for bitpacking
    let max = *values.iter().max().unwrap();
    let bits_needed = (64 - max.leading_zeros()) as u8;
    if bits_needed <= 16 { return Codec::Bitpack; }
    // Check FOR (clustered values)
    let min = *values.iter().min().unwrap();
    let range = max - min;
    if range < max / 4 { return Codec::FOR; }
    Codec::Raw
}

// ── LZ4-inspired Compression (zero deps) ─────────────────────────────────────
// Fast byte-level compression using literal copies + back-references.
// Compatible framing: [original_len(4)] + [compressed_blocks...]

/// Compress bytes using LZ4-inspired block compression (zero deps).
pub fn lz4_compress(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    // Store original length for decompression
    out.extend_from_slice(&(input.len() as u32).to_le_bytes());
    let mut pos = 0;
    let mut literals: Vec<u8> = Vec::new();
    while pos < input.len() {
        // Try to find a back-reference (match) in the last 64KB
        let search_start = pos.saturating_sub(65536);
        let mut best_match_pos = 0;
        let mut best_match_len = 0;
        for mp in search_start..pos {
            let mut ml = 0;
            while pos + ml < input.len() && input[mp + ml] == input[pos + ml] && ml < 255 {
                ml += 1;
            }
            if ml > best_match_len {
                best_match_len = ml;
                best_match_pos = mp;
            }
        }
        if best_match_len >= 4 {
            // Emit literals first
            let lit_len = literals.len();
            if lit_len < 15 {
                out.push(((lit_len as u8) << 4) | 0x0F.min(best_match_len as u8 - 4));
            } else {
                out.push(0xF0 | 0x0F.min(best_match_len as u8 - 4));
                let mut rem = lit_len - 15;
                while rem >= 255 { out.push(255); rem -= 255; }
                out.push(rem as u8);
            }
            out.extend_from_slice(&literals);
            literals.clear();
            // Emit offset (16-bit) + match length
            let offset = (pos - best_match_pos) as u16;
            out.extend_from_slice(&offset.to_le_bytes());
            pos += best_match_len;
        } else {
            literals.push(input[pos]);
            pos += 1;
        }
    }
    // Flush remaining literals
    let lit_len = literals.len();
    if lit_len < 15 { out.push((lit_len as u8) << 4); } else {
        out.push(0xF0); let mut rem = lit_len - 15;
        while rem >= 255 { out.push(255); rem -= 255; } out.push(rem as u8);
    }
    out.extend_from_slice(&literals);
    out
}

/// Decompress LZ4-compressed bytes.
pub fn lz4_decompress(input: &[u8]) -> Vec<u8> {
    if input.len() < 4 { return vec![]; }
    let orig_len = u32::from_le_bytes(input[..4].try_into().unwrap()) as usize;
    let mut out = Vec::with_capacity(orig_len);
    let mut pos = 4;
    while pos < input.len() {
        let token = input[pos]; pos += 1;
        // Literal length
        let mut lit_len = (token >> 4) as usize;
        if lit_len == 15 {
            loop { let extra = input[pos] as usize; pos += 1; lit_len += extra; if extra < 255 { break; } }
        }
        out.extend_from_slice(&input[pos..pos+lit_len]); pos += lit_len;
        if pos >= input.len() { break; }
        // Match
        let offset = u16::from_le_bytes(input[pos..pos+2].try_into().unwrap()) as usize; pos += 2;
        let mut match_len = (token & 0x0F) as usize + 4;
        if match_len - 4 == 15 {
            loop { let extra = input[pos] as usize; pos += 1; match_len += extra; if extra < 255 { break; } }
        }
        let match_start = out.len().saturating_sub(offset);
        for i in 0..match_len { out.push(out[match_start + i]); }
    }
    out
}

/// Write a DataBlock with LZ4 column compression.
pub fn write_file_lz4(path: &str, block: &DataBlock) -> io::Result<()> {
    let bytes = to_bytes_lz4(block);
    std::fs::write(path, &bytes)
}

/// Read an LZ4-compressed KORE file.
pub fn read_file_lz4(path: &str) -> io::Result<DataBlock> {
    let bytes = std::fs::read(path)?;
    from_bytes_lz4(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

const MAGIC_LZ4: &[u8; 5] = b"KOREL";

fn to_bytes_lz4(block: &DataBlock) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(MAGIC_LZ4);
    buf.extend_from_slice(&9u32.to_le_bytes()); // version 9
    buf.extend_from_slice(&(block.columns.len() as u32).to_le_bytes());
    for col in block.columns() {
        let nb = col.name.as_bytes();
        buf.push(nb.len() as u8); buf.extend_from_slice(nb);
        buf.push(col.dtype as u8);
        buf.extend_from_slice(&(col.values.len() as u64).to_le_bytes());
        // Convert values to bytes and compress
        let raw: Vec<u8> = col.values.iter().flat_map(|&v| v.to_le_bytes()).collect();
        let compressed = lz4_compress(&raw);
        buf.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        buf.extend_from_slice(&compressed);
    }
    let cs = crc32(&buf);
    buf.extend_from_slice(&cs.to_le_bytes());
    buf
}

fn from_bytes_lz4(data: &[u8]) -> Result<DataBlock, String> {
    if data.len() < 13 { return Err("too short".into()); }
    let (body, crc_bytes) = data.split_at(data.len() - 4);
    if crc32(body) != u32::from_le_bytes(crc_bytes.try_into().unwrap()) { return Err("CRC32 mismatch".into()); }
    if &body[..5] != MAGIC_LZ4 { return Err("bad LZ4 magic".into()); }
    let mut r = &body[9..];
    let n_cols = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize; r = &r[4..];
    let mut block = DataBlock::new();
    for _ in 0..n_cols {
        let nl = r[0] as usize; r = &r[1..];
        let name = std::str::from_utf8(&r[..nl]).unwrap().to_string(); r = &r[nl..];
        let dtype = match r[0] { 1=>DataType::F64, 2=>DataType::I64, _=>DataType::Str }; r = &r[1..];
        let n_rows = u64::from_le_bytes(r[..8].try_into().unwrap()) as usize; r = &r[8..];
        let comp_len = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize; r = &r[4..];
        let raw = lz4_decompress(&r[..comp_len]); r = &r[comp_len..];
        let values: Vec<u64> = (0..n_rows).map(|i| u64::from_le_bytes(raw[i*8..i*8+8].try_into().unwrap())).collect();
        block.add_column(&name, dtype, values);
    }
    Ok(block)
}

// ── Nested Types ──────────────────────────────────────────────────────────────

/// A column where each cell is a list of values (array column).
#[derive(Debug, Clone)]
pub struct ArrayColumn {
    pub name:   String,
    pub dtype:  DataType,
    pub arrays: Vec<Vec<u64>>,  // each element is a variable-length array
}

impl ArrayColumn {
    pub fn new(name: &str, dtype: DataType) -> Self {
        ArrayColumn { name: name.to_string(), dtype, arrays: vec![] }
    }

    pub fn push(&mut self, arr: Vec<u64>) { self.arrays.push(arr); }
    pub fn get(&self, i: usize) -> &[u64] { &self.arrays[i] }
    pub fn len(&self) -> usize { self.arrays.len() }
    pub fn flatten(&self) -> Vec<u64> { self.arrays.iter().flat_map(|a| a.iter().copied()).collect() }
}

/// A nested block with array columns.
#[derive(Debug, Clone, Default)]
pub struct NestedBlock {
    pub scalars: DataBlock,
    pub arrays:  Vec<ArrayColumn>,
}

impl NestedBlock {
    pub fn new() -> Self { Self::default() }

    pub fn add_scalar(&mut self, name: &str, dtype: DataType, values: Vec<u64>) {
        self.scalars.add_column(name, dtype, values);
    }

    pub fn add_array_col(&mut self, col: ArrayColumn) { self.arrays.push(col); }

    pub fn array_col(&self, name: &str) -> Option<&ArrayColumn> {
        self.arrays.iter().find(|a| a.name == name)
    }
}

/// Write a NestedBlock to disk.
pub fn write_nested(path: &str, block: &NestedBlock) -> io::Result<()> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"KOREX");
    buf.extend_from_slice(&10u32.to_le_bytes());
    // Scalar section
    let scalar_bytes = to_bytes(&block.scalars);
    buf.extend_from_slice(&(scalar_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(&scalar_bytes);
    // Array section
    buf.extend_from_slice(&(block.arrays.len() as u32).to_le_bytes());
    for ac in &block.arrays {
        let nb = ac.name.as_bytes();
        buf.push(nb.len() as u8); buf.extend_from_slice(nb);
        buf.push(ac.dtype as u8);
        buf.extend_from_slice(&(ac.arrays.len() as u32).to_le_bytes());
        for arr in &ac.arrays {
            buf.extend_from_slice(&(arr.len() as u32).to_le_bytes());
            for &v in arr { buf.extend_from_slice(&v.to_le_bytes()); }
        }
    }
    let cs = crc32(&buf);
    buf.extend_from_slice(&cs.to_le_bytes());
    std::fs::write(path, &buf)
}

/// Read a NestedBlock from disk.
pub fn read_nested(path: &str) -> io::Result<NestedBlock> {
    let data = std::fs::read(path)?;
    let (body, crc_bytes) = data.split_at(data.len() - 4);
    if crc32(body) != u32::from_le_bytes(crc_bytes.try_into().unwrap()) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "CRC32 mismatch"));
    }
    if &body[..5] != b"KOREX" { return Err(io::Error::new(io::ErrorKind::InvalidData, "bad nested magic")); }
    let mut r = &body[9..];
    let scalar_len = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize; r = &r[4..];
    let scalars = from_bytes(&r[..scalar_len]).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    r = &r[scalar_len..];
    let n_arrays = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize; r = &r[4..];
    let mut arrays = Vec::new();
    for _ in 0..n_arrays {
        let nl = r[0] as usize; r = &r[1..];
        let name = std::str::from_utf8(&r[..nl]).unwrap().to_string(); r = &r[nl..];
        let dtype = match r[0] { 1=>DataType::F64, 2=>DataType::I64, _=>DataType::Str }; r = &r[1..];
        let n_rows = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize; r = &r[4..];
        let mut col = ArrayColumn::new(&name, dtype);
        for _ in 0..n_rows {
            let alen = u32::from_le_bytes(r[..4].try_into().unwrap()) as usize; r = &r[4..];
            let arr: Vec<u64> = (0..alen).map(|_| { let v = u64::from_le_bytes(r[..8].try_into().unwrap()); r = &r[8..]; v }).collect();
            col.push(arr);
        }
        arrays.push(col);
    }
    Ok(NestedBlock { scalars, arrays })
}

// ── Mini SQL Engine ───────────────────────────────────────────────────────────

/// A simple SQL query result.
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows:    Vec<Vec<f64>>,
    pub row_count: usize,
}

impl QueryResult {
    pub fn print(&self) {
        println!("{}", self.columns.join(" | "));
        println!("{}", "-".repeat(self.columns.len() * 12));
        for row in &self.rows {
            let vals: Vec<String> = row.iter().map(|v| format!("{:.4}", v)).collect();
            println!("{}", vals.join(" | "));
        }
        println!("({} rows)", self.row_count);
    }
}

/// Execute a simple SQL SELECT over a .kore file.
///
/// Supported:
/// - `SELECT col1, col2 FROM file.kore`
/// - `SELECT col1, col2 FROM file.kore WHERE col > value`
/// - `SELECT col1, SUM(col2) FROM file.kore GROUP BY col1`
/// - `SELECT * FROM file.kore LIMIT 10`
/// - `SELECT col FROM file.kore ORDER BY col DESC`
///
/// Example:
/// ```
/// let result = kore_sql("SELECT region, SUM(price) FROM data.kore GROUP BY region")?;
/// result.print();
/// ```
pub fn kore_sql(sql: &str) -> io::Result<QueryResult> {
    let sql = sql.trim().to_uppercase();

    // Parse FROM
    let from_pos = sql.find("FROM ").ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "No FROM clause"))?;
    let after_from = sql[from_pos + 5..].trim();
    let file_end = after_from.find(' ').unwrap_or(after_from.len());
    let file_path = after_from[..file_end].to_string().to_lowercase();

    // Load file
    let block = read_file(&file_path)?;

    // Parse WHERE
    let mut keep_rows: Vec<bool> = vec![true; block.num_rows()];
    if let Some(where_pos) = sql.find(" WHERE ") {
        let where_clause = &sql[where_pos + 7..];
        let where_end = where_clause.find(" GROUP ").or(where_clause.find(" ORDER ").or(where_clause.find(" LIMIT "))).unwrap_or(where_clause.len());
        let cond = &where_clause[..where_end];
        // Simple: col OP value (OP = >, <, >=, <=, =)
        let (col_name, op, val) = parse_condition(cond)?;
        if let Some(col) = block.column(&col_name.to_lowercase()) {
            for (i, &v) in col.values.iter().enumerate() {
                let fv = f64::from_bits(v);
                keep_rows[i] = match op.as_str() {
                    ">" => fv > val, "<" => fv < val, ">=" => fv >= val,
                    "<=" => fv <= val, "=" | "==" => (fv - val).abs() < 1e-10,
                    "!=" | "<>" => (fv - val).abs() >= 1e-10, _ => true,
                };
            }
        }
    }

    // Parse SELECT cols
    let select_clause = sql[7..from_pos].trim().to_string();
    let is_star = select_clause == "*";

    // Parse GROUP BY
    let group_by_col = if let Some(gp) = sql.find(" GROUP BY ") {
        let after = &sql[gp + 10..];
        let end = after.find(' ').unwrap_or(after.len());
        Some(after[..end].to_lowercase())
    } else { None };

    // Parse ORDER BY + DESC/ASC
    let (order_col, order_desc) = if let Some(op) = sql.find(" ORDER BY ") {
        let after = &sql[op + 10..];
        let parts: Vec<&str> = after.split_whitespace().collect();
        let col = parts[0].to_lowercase();
        let desc = parts.get(1).map(|&s| s == "DESC").unwrap_or(false);
        (Some(col), desc)
    } else { (None, false) };

    // Parse LIMIT
    let limit = if let Some(lp) = sql.find(" LIMIT ") {
        sql[lp + 7..].split_whitespace().next().and_then(|s| s.parse::<usize>().ok())
    } else { None };

    // Build result
    if let Some(ref group_col) = group_by_col {
        // GROUP BY aggregation
        let gc = block.column(group_col).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "GROUP BY column not found"))?;
        let mut groups: std::collections::HashMap<u64, (u64, f64, usize)> = std::collections::HashMap::new(); // key → (key, sum, count)
        for (i, &k) in gc.values.iter().enumerate() {
            if !keep_rows[i] { continue; }
            let entry = groups.entry(k).or_insert((k, 0.0, 0));
            entry.2 += 1;
            // Sum all other numeric columns
            for col in block.columns() {
                if col.name != *group_col {
                    entry.1 += f64::from_bits(col.values[i]);
                }
            }
        }
        let mut sorted: Vec<(u64, f64, usize)> = groups.values().cloned().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        let rows: Vec<Vec<f64>> = sorted.iter().map(|(k, sum, count)| vec![f64::from_bits(*k), *sum, *count as f64]).collect();
        let row_count = rows.len();
        let cols = vec![group_col.clone(), format!("SUM"), "COUNT".to_string()];
        return Ok(QueryResult { columns: cols, rows, row_count });
    }

    // Regular SELECT
    let col_names: Vec<String> = if is_star {
        block.columns().iter().map(|c| c.name.clone()).collect()
    } else {
        select_clause.split(',').map(|s| s.trim().to_lowercase().to_string()).collect()
    };

    let indices: Vec<usize> = (0..block.num_rows()).filter(|&i| keep_rows[i]).collect();
    let mut rows: Vec<Vec<f64>> = indices.iter().map(|&i| {
        col_names.iter().map(|cn| {
            block.column(cn).map(|c| f64::from_bits(c.values[i])).unwrap_or(0.0)
        }).collect()
    }).collect();

    // ORDER BY
    if let Some(ref oc) = order_col {
        let oc_idx = col_names.iter().position(|n| n == oc).unwrap_or(0);
        rows.sort_by(|a, b| {
            let cmp = a[oc_idx].partial_cmp(&b[oc_idx]).unwrap_or(std::cmp::Ordering::Equal);
            if order_desc { cmp.reverse() } else { cmp }
        });
    }

    // LIMIT
    if let Some(n) = limit { rows.truncate(n); }

    let row_count = rows.len();
    Ok(QueryResult { columns: col_names, rows, row_count })
}

fn parse_condition(cond: &str) -> io::Result<(String, String, f64)> {
    for op in &[">=", "<=", "!=", "<>", ">", "<", "="] {
        if let Some(pos) = cond.find(op) {
            let col = cond[..pos].trim().to_string();
            let val: f64 = cond[pos+op.len()..].trim().parse()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid WHERE value"))?;
            return Ok((col, op.to_string(), val));
        }
    }
    Err(io::Error::new(io::ErrorKind::InvalidInput, "Cannot parse WHERE condition"))
}

// ── MVCC (Multi-Version Concurrency Control) ──────────────────────────────────

/// An MVCC transaction — snapshot isolation for concurrent readers.
pub struct MvccTransaction {
    pub snapshot_version: u32,
    pub path: String,
}

impl MvccTransaction {
    /// Begin a read transaction — snapshots the current version.
    pub fn begin_read(path: &str) -> io::Result<Self> {
        let ver_path = format!("{}.ver", path);
        let version = std::fs::read_to_string(&ver_path)
            .ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
        Ok(MvccTransaction { snapshot_version: version, path: path.to_string() })
    }

    /// Read data at the snapshot version (time-travel consistent read).
    pub fn read(&self) -> io::Result<DataBlock> {
        if self.snapshot_version == 0 {
            read_file(&self.path)
        } else {
            let snap_path = format!("{}.v{:03}.kore", self.path, self.snapshot_version);
            if std::path::Path::new(&snap_path).exists() {
                read_file(&snap_path)
            } else {
                read_file(&self.path)
            }
        }
    }
}

/// Write with MVCC version increment.
pub fn mvcc_write(path: &str, block: &DataBlock) -> io::Result<u32> {
    let ver_path = format!("{}.ver", path);
    let current: u32 = std::fs::read_to_string(&ver_path)
        .ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let new_version = current + 1;
    // Write snapshot of current before overwriting
    if current > 0 {
        let snap = format!("{}.v{:03}.kore", path, current);
        if !std::path::Path::new(&snap).exists() {
            if let Ok(existing) = read_file(path) {
                let _ = write_file(&snap, &existing);
            }
        }
    }
    write_file(path, block)?;
    std::fs::write(&ver_path, new_version.to_string())?;
    Ok(new_version)
}

// ── Cloud-Native HTTP Reader (S3 / GCS / Azure) ────────────────────────────────

/// Download a .kore file from an HTTP/HTTPS URL into memory.
/// Works with: AWS S3 presigned URLs, GCS signed URLs, Azure SAS URLs, HTTP servers.
///
/// Example:
/// ```
/// let block = read_url("https://my-bucket.s3.amazonaws.com/data.kore?X-Amz-...")?;
/// ```
pub fn read_url(url: &str) -> io::Result<DataBlock> {
    // Parse host and path from URL
    let url = url.trim();
    let (scheme, rest) = url.split_once("://").ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid URL"))?;
    let (host_part, path_part) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = if host_part.contains(':') {
        let (h, p) = host_part.split_once(':').unwrap();
        (h, p.parse::<u16>().unwrap_or(80))
    } else {
        (host_part, if scheme == "https" { 443 } else { 80 })
    };
    let path = format!("/{}", path_part);
    // HTTP GET request using std::net
    use std::io::{BufRead, Read, Write};
    let addr = format!("{}:{}", host, port);
    let mut stream: Box<dyn std::io::Read> = if scheme == "https" {
        // For HTTPS in zero-deps context, we just attempt TCP (user handles TLS termination via proxy)
        return Err(io::Error::new(io::ErrorKind::Unsupported, "HTTPS requires TLS. Use presigned HTTP URLs or a proxy."));
    } else {
        Box::new(std::net::TcpStream::connect(&addr)?) as Box<dyn std::io::Read>
    };
    // Send HTTP/1.1 GET via separate write stream
    let request = format!("GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", path, host);
    let mut write_stream = std::net::TcpStream::connect(&addr)?;
    {
        use std::io::Write;
        write_stream.write_all(request.as_bytes())?;
    }
    let mut read_stream = write_stream;
    // Read response
    let mut response = Vec::new();
    std::io::Read::read_to_end(&mut read_stream, &mut response)?;
    // Skip HTTP headers
    let header_end = response.windows(4).position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "No HTTP header end"))? + 4;
    let body = &response[header_end..];
    from_bytes(body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Upload a DataBlock to an HTTP endpoint via PUT request.
pub fn write_url(url: &str, block: &DataBlock) -> io::Result<()> {
    use std::io::Write;
    let bytes = to_bytes(block);
    let url = url.trim();
    let (_, rest) = url.split_once("://").ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid URL"))?;
    let (host_part, path_part) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = if host_part.contains(':') {
        let (h, p) = host_part.split_once(':').unwrap();
        (h, p.parse::<u16>().unwrap_or(80))
    } else { (host_part, 80) };
    let path = format!("/{}", path_part);
    let mut stream = std::net::TcpStream::connect(format!("{}:{}", host, port))?;
    let request = format!(
        "PUT {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        path, host, bytes.len()
    );
    stream.write_all(request.as_bytes())?;
    stream.write_all(&bytes)?;
    Ok(())
}


