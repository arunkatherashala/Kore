#![allow(dead_code, unused_imports, unused_variables)]

// ============================================================================
// KORE — Killer Optimized Record Exchange
// ============================================================================
// Binary columnar file format designed to beat Parquet+Snappy, Arrow, CSV.
//
// (File copied from original project)


use std::collections::HashMap;
use std::io::{Write, Read};

// -- Magic bytes --------------------------------------------------------------
pub const KORE_MAGIC: &[u8; 4] = b"KORE";
pub const KORE_VERSION: u8 = 1;

// -- Column data types --------------------------------------------------------
#[derive(Debug, Clone, PartialEq)]
pub enum KoreType {
    Int,
    Float,
    Bool,
    Str,
    Bytes,
    Embedding(u32), // embedding dimension
}

impl KoreType {
    fn to_u8(&self) -> u8 {
        match self {
            KoreType::Int      => 1,
            KoreType::Float    => 2,
            KoreType::Bool     => 3,
            KoreType::Str      => 4,
            KoreType::Bytes    => 5,
            KoreType::Embedding(_) => 6,
        }
    }
    fn from_u8(v: u8, dim: u32) -> Self {
        match v {
            1 => KoreType::Int,
            2 => KoreType::Float,
            3 => KoreType::Bool,
            4 => KoreType::Str,
            5 => KoreType::Bytes,
            6 => KoreType::Embedding(dim),
            _ => KoreType::Str,
        }
    }
}

// -- Per-column compression algorithm -----------------------------------------
#[derive(Debug, Clone, PartialEq)]
pub enum KoreAlgo {
    None,
    RLE,
    Delta,
    DictRLE,  // dictionary index + RLE
    LZ77,
    DeltaBitpack,
}

impl KoreAlgo {
    fn to_u8(&self) -> u8 {
        match self {
            KoreAlgo::None        => 0,
            KoreAlgo::RLE         => 1,
            KoreAlgo::Delta       => 2,
            KoreAlgo::DictRLE     => 3,
            KoreAlgo::LZ77        => 4,
            KoreAlgo::DeltaBitpack=> 5,
        }
    }
    fn from_u8(v: u8) -> Self {
        match v {
            1 => KoreAlgo::RLE,
            2 => KoreAlgo::Delta,
            3 => KoreAlgo::DictRLE,
            4 => KoreAlgo::LZ77,
            5 => KoreAlgo::DeltaBitpack,
            _ => KoreAlgo::None,
        }
    }
}

// -- Schema -------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct KoreColumn {
    pub name: String,
    pub col_type: KoreType,
    pub algo: KoreAlgo,
    pub encrypted: bool,
    pub enc_key: [u8; 32], // XOR stream key (zero = no encryption)
}

// -- A single value -----------------------------------------------------------
#[derive(Debug, Clone)]
pub enum KoreValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Bytes(Vec<u8>),
    Null,
}

impl KoreValue {
    fn as_i64(&self) -> i64 {
        match self { KoreValue::Int(x) => *x, KoreValue::Float(f) => *f as i64,
                     KoreValue::Bool(b) => if *b { 1 } else { 0 }, _ => 0 }
    }
    fn as_f64(&self) -> f64 {
        match self { KoreValue::Float(x) => *x, KoreValue::Int(i) => *i as f64, _ => 0.0 }
    }
    fn as_str(&self) -> &str {
        match self { KoreValue::Str(s) => s.as_str(), _ => "" }
    }
    fn to_display(&self) -> String {
        match self {
            KoreValue::Int(x)   => x.to_string(),
            KoreValue::Float(f) => format!("{:.6}", f),
            KoreValue::Bool(b)  => b.to_string(),
            KoreValue::Str(s)   => s.clone(),
            KoreValue::Bytes(b) => format!("<{} bytes>", b.len()),
            KoreValue::Null     => "null".to_string(),
        }
    }
}

// -- Bloom filter (pure stdlib, no external deps) ------------------------------
// Simple 512-byte (4096-bit) bloom filter per column per chunk
#[allow(dead_code)]
struct BloomFilter {
    bits: [u64; 64], // 64 × 64 = 4096 bits
}

#[allow(dead_code)]
impl BloomFilter {
    fn new() -> Self { BloomFilter { bits: [0u64; 64] } }

    fn hash1(s: &str) -> usize {
        let mut h: u64 = 0x9e3779b97f4a7c15;
        for b in s.bytes() { h ^= b as u64; h = h.wrapping_mul(0x517cc1b727220a95); }
        (h >> 6) as usize % 4096
    }
    fn hash2(s: &str) -> usize {
        let mut h: u64 = 0x6c62272e07bb0142;
        for b in s.bytes() { h = h.wrapping_add(b as u64); h ^= h >> 16; h = h.wrapping_mul(0x45d9f3b); }
        (h >> 4) as usize % 4096
    }


impl Default for BloomFilter {
    fn default() -> Self { BloomFilter::new() }
}
    fn hash3(s: &str) -> usize {
        let mut h: u64 = 0xbf58476d1ce4e5b9;
        for b in s.bytes() { h = h.wrapping_mul(0x94d049bb133111eb) ^ b as u64; }
        h as usize % 4096
    }

    fn insert(&mut self, s: &str) {
        for pos in [Self::hash1(s), Self::hash2(s), Self::hash3(s)] {
            self.bits[pos / 64] |= 1u64 << (pos % 64);
        }
    }
    fn may_contain(&self, s: &str) -> bool {
        [Self::hash1(s), Self::hash2(s), Self::hash3(s)].iter().all(|&pos| {
            self.bits[pos / 64] & (1u64 << (pos % 64)) != 0
        })
    }
    fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(512);
        for word in &self.bits {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out
    }
    fn from_bytes(data: &[u8]) -> Self {
        let mut bf = BloomFilter::new();
        for (i, chunk) in data.chunks(8).enumerate() {
            if i < 64 && chunk.len() == 8 {
                bf.bits[i] = u64::from_le_bytes(chunk.try_into().unwrap_or([0u8;8]));
            }
        }
        bf
    }
}

// -- LZ77 compression (pure stdlib sliding window) -----------------------------
fn lz77_compress(input: &[u8]) -> Vec<u8> {
    if input.is_empty() { return Vec::new(); }
    let mut out: Vec<u8> = Vec::with_capacity(input.len());
    let window = 256usize; // search window
    let min_match = 4usize;
    let max_match = 258usize;
    let mut pos = 0;

    while pos < input.len() {
        let look_start = pos.saturating_sub(window);
        let remaining  = &input[pos..];
        let mut best_offset = 0usize;
        let mut best_len    = 0usize;

        for start in look_start..pos {
            let mut len = 0;
            while len < remaining.len().min(max_match) && input[start + len] == remaining[len] {
                len += 1;
                if start + len >= pos { break; }
            }
            if len >= min_match && len > best_len {
                best_len    = len;
                best_offset = pos - start;
            }
        }

        if best_len >= min_match {
            // Emit: 0xFF marker + offset(2 bytes) + length(1 byte)
            out.push(0xFF);
            out.extend_from_slice(&(best_offset as u16).to_le_bytes());
            out.push(best_len as u8);
            pos += best_len;
        } else {
            let byte = input[pos];
            if byte == 0xFF {
                out.push(0xFF); out.push(0); out.push(0); out.push(1); // escaped literal
            } else {
                out.push(byte);
            }
            pos += 1;
        }
    }
    out
}

fn lz77_decompress(input: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < input.len() {
        if input[i] == 0xFF && i + 3 < input.len() {
            let offset = u16::from_le_bytes([input[i+1], input[i+2]]) as usize;
            let length = input[i+3] as usize;
            i += 4;
            if offset == 0 && length == 1 {
                out.push(0xFF); // escaped literal
            } else {
                let base = out.len().saturating_sub(offset);
                for j in 0..length {
                    let src = base + j;
                    if src < out.len() { let b = out[src]; out.push(b); }
                    else { out.push(0); }
                }
            }
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
    out
}

// -- Delta encoding for integers -----------------------------------------------
fn delta_encode_i64(values: &[i64]) -> Vec<u8> {
    if values.is_empty() { return Vec::new(); }
    let mut out = Vec::with_capacity(values.len() * 4);
    out.extend_from_slice(&values[0].to_le_bytes());
    for i in 1..values.len() {
        let delta = values[i].wrapping_sub(values[i-1]);
        // Variable-length zigzag: positive deltas common
        let zigzag = ((delta << 1) ^ (delta >> 63)) as u64;
        encode_varint(zigzag, &mut out);
    }
    out
}

fn delta_decode_i64(data: &[u8]) -> Vec<i64> {
    if data.len() < 8 { return Vec::new(); }
    let mut out = Vec::new();
    let base = i64::from_le_bytes(data[..8].try_into().unwrap_or([0u8;8]));
    out.push(base);
    let mut pos = 8;
    while pos < data.len() {
        let (zigzag, consumed) = decode_varint(&data[pos..]);
        let delta = ((zigzag >> 1) as i64) ^ (-((zigzag & 1) as i64));
        out.push(out.last().unwrap_or(&0).wrapping_add(delta));
        pos += consumed;
    }
    out
}

fn encode_varint(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 { out.push(byte); break; }
        else       { out.push(byte | 0x80); }
    }
}

fn decode_varint(data: &[u8]) -> (u64, usize) {
    let mut result = 0u64;
    let mut shift  = 0;
    let mut i      = 0;
    while i < data.len() {
        let byte = data[i] as u64;
        result |= (byte & 0x7F) << shift;
        i += 1;
        if byte & 0x80 == 0 { break; }
        shift += 7;
        if shift >= 64 { break; }
    }
    (result, i)
}

fn delta_encode_f64(values: &[f64]) -> Vec<u8> {
    let as_i64: Vec<i64> = values.iter().map(|f| f64::to_bits(*f) as i64).collect();
    delta_encode_i64(&as_i64)
}
fn delta_decode_f64(data: &[u8]) -> Vec<f64> {
    delta_decode_i64(data).iter().map(|&i| f64::from_bits(i as u64)).collect()
}

// -- XOR stream cipher for column encryption -----------------------------------
fn xor_encrypt(data: &[u8], key: &[u8; 32]) -> Vec<u8> {
    // Key schedule: expand 32-byte key to stream using simple mixing
    let mut stream_key = Vec::with_capacity(data.len());
    let mut state = u64::from_le_bytes(key[..8].try_into().unwrap_or([0u8;8]));
    let mut i = 0;
    while stream_key.len() < data.len() {
        state ^= u64::from_le_bytes(key[i % 32..(i % 32)+8].try_into().unwrap_or_else(|_| {
            let mut b = [0u8;8]; b[..key[i%32..].len().min(8)].copy_from_slice(&key[i%32..i%32+key[i%32..].len().min(8)]); b
        }));
        state = state.wrapping_mul(0x9e3779b97f4a7c15).rotate_left(17);
        for b in state.to_le_bytes() { stream_key.push(b); }
        i += 8;
    }
    data.iter().zip(stream_key.iter()).map(|(d, k)| d ^ k).collect()
}
// Decryption is the same operation (XOR is symmetric)
fn xor_decrypt(data: &[u8], key: &[u8; 32]) -> Vec<u8> { xor_encrypt(data, key) }

// -- RLE for byte slices -------------------------------------------------------
fn rle_encode_bytes(data: &[u8]) -> Vec<u8> {
    if data.is_empty() { return Vec::new(); }
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        let mut run: usize = 1;
        while (i + run) < data.len() && data[i + run] == b && run < 255 {
            run += 1;
        }
        out.push(run as u8);
        out.push(b);
        i += run;
    }
    out
}

fn rle_decode_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < data.len() {
        let run = data[i] as usize;
        let b   = data[i+1];
        out.extend(std::iter::repeat_n(b, run));
        i += 2;
    }
    out
}

// -- Auto-select compression algorithm for a column ---------------------------
fn auto_select_algo(col: &KoreColumn, values: &[KoreValue]) -> KoreAlgo {
    match col.col_type {
        KoreType::Int   => KoreAlgo::Delta,
        KoreType::Float => KoreAlgo::Delta,
        KoreType::Bool  => KoreAlgo::RLE,
        KoreType::Embedding(_) => KoreAlgo::None, // raw floats — compression hurts
        KoreType::Str => {
            // Count unique values
            let mut seen = std::collections::HashSet::new();
            for v in values { seen.insert(v.as_str().to_string()); }
            let avg_len: usize = values.iter().map(|v| v.as_str().len()).sum::<usize>()
                                  / values.len().max(1);
            if seen.len() <= 256 {
                KoreAlgo::DictRLE // low cardinality → dict + RLE
            } else if avg_len > 32 {
                KoreAlgo::LZ77    // long strings → LZ77
            } else {
                KoreAlgo::None    // short diverse strings → raw
            }
        }
        KoreType::Bytes => KoreAlgo::LZ77,
    }
}

// -- Write a single column block (returns encoded bytes) ----------------------
fn encode_column_block(
    values: &[KoreValue],
    _col: &KoreColumn,
    _dict: &HashMap<String, u32>,
    _algo: &KoreAlgo,
) -> Vec<u8> {
    // Standalone crate fallback: keep API non-panicking until v1 encoder is imported.
    let mut out = Vec::new();
    for v in values {
        let s = v.to_display();
        out.extend_from_slice(&(s.len() as u32).to_le_bytes());
        out.extend_from_slice(s.as_bytes());
    }
    out
}

// -- Decode a column block -----------------------------------------------------
fn decode_column_block(
    _data: &[u8],
    _col: &KoreColumn,
    _algo: &KoreAlgo,
    _count: usize,
    _dict_rev: &Vec<String>,
) -> Vec<KoreValue> {
    // Standalone crate fallback: return empty to avoid panic.
    Vec::new()
}

// -- KORE Writer ---------------------------------------------------------------
pub struct KoreWriter {
    pub columns: Vec<KoreColumn>,
    pub chunk_size: usize, // rows per chunk (default 65536)
}

impl KoreWriter {
    pub fn new(columns: Vec<KoreColumn>) -> Self {
        KoreWriter { columns, chunk_size: 65536 }
    }

    /// Write rows to a KORE file. rows[i][j] = value at row i, column j.
    pub fn write(&self, _path: &str, _rows: &[Vec<KoreValue>]) -> Result<String, String> {
        Err("KORE v1 writer is not implemented in this standalone crate; use kore_v2::KoreWriter".to_string())
    }
}

// -- KORE Reader ---------------------------------------------------------------
#[allow(dead_code)]
pub struct KoreFile {
    pub row_count:   u64,
    pub col_count:   u32,
    pub chunk_count: u32,
    pub chunk_size:  u32,
    pub created:     u64,
    pub columns:     Vec<KoreColumn>,
    pub algos:       Vec<KoreAlgo>,
    pub dict:        Vec<String>,
    pub chunk_offsets: Vec<u64>,
    data: Vec<u8>,
    chunks_start: usize,
}

impl KoreFile {
    pub fn open(_path: &str) -> Result<Self, String> {
        Err("KORE v1 reader is not implemented in this standalone crate; use kore_v2::KoreReader".to_string())
    }

    pub fn read_all(&self) -> Vec<Vec<KoreValue>> { Vec::new() }
    pub fn read_column(&self, _col_name: &str) -> Vec<KoreValue> { Vec::new() }
    pub fn info(&self) -> String { "kore-fileformat (stub)".to_string() }
}

// -- Public API (called from Killer builtins) ----------------------------------

/// kore_write(path, schema_json, data_rows) → "ok: ..." or "error: ..."
pub fn kore_write_simple(path: &str, schema_json: &str, data_json: &str) -> String {
    // Thin wrapper kept for compatibility; original implementation lives in upstream kore.rs
    match KoreWriter::new(Vec::new()).write(path, &[]) {
        Ok(m) => format!("ok: {}", m),
        Err(e) => format!("error: {}", e),
    }
}

/// kore_read(path) → JSON string of all rows
pub fn kore_read_simple(_path: &str) -> String { "[]".to_string() }

/// kore_read_col(path, col_name) → JSON array of values for that column
pub fn kore_read_col_simple(_path: &str, _col_name: &str) -> String { "[]".to_string() }

/// kore_info(path) → metadata string
pub fn kore_info_simple(_path: &str) -> String { "kore-fileformat (stub)".to_string() }
