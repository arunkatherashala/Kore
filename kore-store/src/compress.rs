//! RLE and delta compression for columnar data.

/// Run-length encode a slice of i64 values (None = null sentinel = i64::MIN).
pub fn rle_encode_i64(vals: &[Option<i64>]) -> Vec<u8> {
    if vals.is_empty() { return vec![]; }
    let mut out = Vec::new();
    let mut cur = vals[0];
    let mut run: u32 = 1;
    for &v in &vals[1..] {
        if v == cur && run < u32::MAX { run += 1; }
        else {
            push_rle_entry(&mut out, cur, run);
            cur = v; run = 1;
        }
    }
    push_rle_entry(&mut out, cur, run);
    out
}

pub fn rle_decode_i64(data: &[u8], n: usize) -> Vec<Option<i64>> {
    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    while i + 12 <= data.len() && out.len() < n {
        let is_null = data[i]; i += 1;
        let val = i64::from_le_bytes(data[i..i+8].try_into().unwrap()); i += 8;
        let run = u32::from_le_bytes(data[i..i+4].try_into().unwrap()) as usize; i += 4;
        let v = if is_null == 1 { None } else { Some(val) };
        let count = run.min(n - out.len());
        out.extend(std::iter::repeat(v).take(count));
    }
    out
}

fn push_rle_entry(out: &mut Vec<u8>, v: Option<i64>, run: u32) {
    out.push(if v.is_none() { 1 } else { 0 });
    out.extend_from_slice(&v.unwrap_or(0).to_le_bytes());
    out.extend_from_slice(&run.to_le_bytes());
}

/// Delta encode sorted i64 values: store first value + differences.
pub fn delta_encode_i64(vals: &[Option<i64>]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut prev = 0i64;
    for &v in vals {
        let (is_null, val) = if let Some(x) = v { (0u8, x) } else { (1u8, 0) };
        out.push(is_null);
        let delta = val.wrapping_sub(prev);
        out.extend_from_slice(&delta.to_le_bytes());
        if is_null == 0 { prev = val; }
    }
    out
}

pub fn delta_decode_i64(data: &[u8], n: usize) -> Vec<Option<i64>> {
    let mut out = Vec::with_capacity(n);
    let mut prev = 0i64;
    let mut i = 0;
    while i + 9 <= data.len() && out.len() < n {
        let is_null = data[i]; i += 1;
        let delta = i64::from_le_bytes(data[i..i+8].try_into().unwrap()); i += 8;
        if is_null == 1 {
            out.push(None);
        } else {
            prev = prev.wrapping_add(delta);
            out.push(Some(prev));
        }
    }
    out
}

/// Raw encode f64 values (no compression — just bytes).
pub fn raw_encode_f64(vals: &[Option<f64>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vals.len() * 9);
    for &v in vals {
        out.push(if v.is_none() { 1 } else { 0 });
        out.extend_from_slice(&v.unwrap_or(0.0).to_le_bytes());
    }
    out
}

pub fn raw_decode_f64(data: &[u8], n: usize) -> Vec<Option<f64>> {
    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    while i + 9 <= data.len() && out.len() < n {
        let is_null = data[i]; i += 1;
        let val = f64::from_le_bytes(data[i..i+8].try_into().unwrap()); i += 8;
        out.push(if is_null == 1 { None } else { Some(val) });
    }
    out
}

/// Encode booleans as a packed byte vector (8 per byte) + null bitmask.
pub fn raw_encode_bool(vals: &[Option<bool>]) -> Vec<u8> {
    let mut out = Vec::new();
    for &v in vals {
        out.push(match v { None => 2, Some(false) => 0, Some(true) => 1 });
    }
    out
}

pub fn raw_decode_bool(data: &[u8], n: usize) -> Vec<Option<bool>> {
    data.iter().take(n).map(|&b| match b {
        0 => Some(false), 1 => Some(true), _ => None,
    }).collect()
}

/// Encode strings: [count:4] [offset_0:4] ... [offset_n:4] [bytes...]
pub fn encode_strs(vals: &[Option<String>]) -> Vec<u8> {
    let mut strings: Vec<&[u8]> = Vec::new();
    let mut null_flags: Vec<u8> = Vec::new();
    for v in vals {
        match v {
            None    => { null_flags.push(1); strings.push(b""); }
            Some(s) => { null_flags.push(0); strings.push(s.as_bytes()); }
        }
    }
    let n = vals.len() as u32;
    let mut out = Vec::new();
    out.extend_from_slice(&n.to_le_bytes());
    // null flags
    out.extend_from_slice(&null_flags);
    // offsets
    let mut offset = 0u32;
    for s in &strings {
        out.extend_from_slice(&offset.to_le_bytes());
        offset += s.len() as u32;
    }
    out.extend_from_slice(&offset.to_le_bytes()); // sentinel
    // data
    for s in &strings { out.extend_from_slice(s); }
    out
}

pub fn decode_strs(data: &[u8]) -> Vec<Option<String>> {
    if data.len() < 4 { return vec![]; }
    let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    if n == 0 { return vec![]; }
    let null_end = 4 + n;
    let off_end = null_end + (n + 1) * 4;
    if data.len() < off_end { return vec![None; n]; }
    let null_flags = &data[4..null_end];
    let off_start  = null_end;
    let data_start = off_end;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let is_null = null_flags[i] == 1;
        let o1 = u32::from_le_bytes(data[off_start + i*4..off_start + i*4 + 4].try_into().unwrap()) as usize;
        let o2 = u32::from_le_bytes(data[off_start + (i+1)*4..off_start + (i+1)*4 + 4].try_into().unwrap()) as usize;
        if is_null || data_start + o2 > data.len() {
            out.push(None);
        } else {
            let bytes = &data[data_start + o1..data_start + o2];
            out.push(Some(String::from_utf8_lossy(bytes).into_owned()));
        }
    }
    out
}

// ── NaN-sentinel Float64 (8 bytes/element, fastest read path) ─────────────────
// NaN = null; all other f64 values are real data.
pub fn nan_encode_f64(vals: &[Option<f64>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vals.len() * 8);
    for &v in vals { out.extend_from_slice(&v.unwrap_or(f64::NAN).to_le_bytes()); }
    out
}

pub fn nan_decode_f64(data: &[u8], n: usize) -> Vec<Option<f64>> {
    let mut out = Vec::with_capacity(n);
    for chunk in data.chunks_exact(8).take(n) {
        let f = f64::from_le_bytes(chunk.try_into().unwrap());
        out.push(if f.is_nan() { None } else { Some(f) });
    }
    out
}

// ── Dictionary Float64 (1 byte/element for low-cardinality columns) ───────────
// Format: [dict_count:u8] [dict_val_0:f64(8)] ... [code_0:u8] [code_1:u8] ...
// NaN sentinel used for null dict entry.
// Returns None if cardinality > 255 (caller falls back to NanRaw).
pub fn dict_encode_f64(vals: &[Option<f64>]) -> Option<Vec<u8>> {
    let mut dict: Vec<f64>                     = Vec::new();
    let mut dict_map: std::collections::HashMap<u64, u8> = std::collections::HashMap::new();
    let mut codes: Vec<u8>                     = Vec::with_capacity(vals.len());
    for &v in vals {
        let bits = v.unwrap_or(f64::NAN).to_bits();
        if let Some(&code) = dict_map.get(&bits) {
            codes.push(code);
        } else {
            if dict.len() >= 255 { return None; }
            let code = dict.len() as u8;
            dict_map.insert(bits, code);
            dict.push(v.unwrap_or(f64::NAN));
            codes.push(code);
        }
    }
    let mut out = Vec::with_capacity(1 + dict.len() * 8 + codes.len());
    out.push(dict.len() as u8);
    for &f in &dict { out.extend_from_slice(&f.to_le_bytes()); }
    out.extend_from_slice(&codes);
    Some(out)
}

pub fn dict_decode_f64(data: &[u8], n: usize) -> Vec<Option<f64>> {
    if data.is_empty() { return vec![None; n]; }
    let dict_len = data[0] as usize;
    let mut dict: Vec<Option<f64>> = Vec::with_capacity(dict_len);
    let mut i = 1;
    for _ in 0..dict_len {
        if i + 8 > data.len() { break; }
        let f = f64::from_le_bytes(data[i..i+8].try_into().unwrap()); i += 8;
        dict.push(if f.is_nan() { None } else { Some(f) });
    }
    let mut out = Vec::with_capacity(n);
    while out.len() < n && i < data.len() {
        let code = data[i] as usize; i += 1;
        out.push(dict.get(code).copied().flatten());
    }
    out
}

// ── Native StrDict encoding ────────────────────────────────────────────────────
// Format: [dict_count:u16] [entry_len:u16 + entry_bytes]... [codes: n * u8]
// 0xFF = null code (same as in-memory representation).
pub fn encode_strdict(codes: &[u8], dict: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(dict.len() as u16).to_le_bytes());
    for entry in dict {
        let b = entry.as_bytes();
        out.extend_from_slice(&(b.len() as u16).to_le_bytes());
        out.extend_from_slice(b);
    }
    out.extend_from_slice(codes);
    out
}

pub fn decode_strdict(data: &[u8], n: usize) -> (Vec<u8>, Vec<String>) {
    if data.len() < 2 { return (vec![u8::MAX; n], vec![]); }
    let dict_len = u16::from_le_bytes(data[0..2].try_into().unwrap()) as usize;
    let mut dict = Vec::with_capacity(dict_len);
    let mut i = 2;
    for _ in 0..dict_len {
        if i + 2 > data.len() { break; }
        let elen = u16::from_le_bytes(data[i..i+2].try_into().unwrap()) as usize; i += 2;
        if i + elen > data.len() { break; }
        dict.push(String::from_utf8_lossy(&data[i..i+elen]).into_owned()); i += elen;
    }
    let codes = data[i..].iter().take(n).copied().collect();
    (codes, dict)
}

// ── ZSTD compression (better ratio than LZ4) ──────────────────────────────────
pub fn zstd_encode(data: &[u8]) -> Vec<u8> {
    zstd::encode_all(data, 3).unwrap_or_else(|_| data.to_vec())
}

pub fn zstd_decode(data: &[u8], n: usize) -> Vec<u8> {
    zstd::decode_all(data).unwrap_or_else(|_| vec![0; n])
}

// ── CRC32 checksums for data integrity ─────────────────────────────────────────
pub fn crc32(data: &[u8]) -> u32 {
    use crc::{Crc, CRC_32_ISO_HDLC};
    const CRC: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);
    CRC.checksum(data)
}

// ── Column statistics for predicate pushdown ──────────────────────────────────
#[derive(Clone, Debug)]
pub struct ColStats {
    pub null_count: usize,
    pub min_i64: Option<i64>,
    pub max_i64: Option<i64>,
    pub min_f64: Option<f64>,
    pub max_f64: Option<f64>,
}

impl ColStats {
    pub fn for_int64(vals: &[Option<i64>]) -> Self {
        let mut null_count = 0;
        let mut min = i64::MAX;
        let mut max = i64::MIN;
        for &v in vals {
            match v {
                None => null_count += 1,
                Some(i) => {
                    if i < min { min = i; }
                    if i > max { max = i; }
                }
            }
        }
        Self {
            null_count,
            min_i64: if min <= max { Some(min) } else { None },
            max_i64: if min <= max { Some(max) } else { None },
            min_f64: None,
            max_f64: None,
        }
    }

    pub fn for_f64(vals: &[Option<f64>]) -> Self {
        let mut null_count = 0;
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        for &v in vals {
            match v {
                None => null_count += 1,
                Some(f) => {
                    if f < min { min = f; }
                    if f > max { max = f; }
                }
            }
        }
        Self {
            null_count,
            min_i64: None,
            max_i64: None,
            min_f64: if min.is_finite() { Some(min) } else { None },
            max_f64: if max.is_finite() { Some(max) } else { None },
        }
    }
}

// ── Bloom Filters for fast cardinality checks ─────────────────────────────────
#[derive(Clone, Debug)]
pub struct BloomFilter {
    bits: Vec<u8>,
    num_hash_fns: u8,
}

impl BloomFilter {
    pub fn new(expected_items: usize, fpp: f64) -> Self {
        let m = ((expected_items as f64 * fpp.ln()) / (2f64 * (2f64.ln()).powi(2))).ceil() as usize;
        let k = ((m as f64 / expected_items as f64) * 2f64.ln()).ceil() as u8;
        BloomFilter {
            bits: vec![0; (m + 7) / 8],
            num_hash_fns: k,
        }
    }

    pub fn insert(&mut self, data: &[u8]) {
        for i in 0..self.num_hash_fns {
            let hash = hash_fn(data, i) as usize % (self.bits.len() * 8);
            let byte_idx = hash / 8;
            let bit_idx = hash % 8;
            self.bits[byte_idx] |= 1 << bit_idx;
        }
    }

    pub fn contains(&self, data: &[u8]) -> bool {
        for i in 0..self.num_hash_fns {
            let hash = hash_fn(data, i) as usize % (self.bits.len() * 8);
            let byte_idx = hash / 8;
            let bit_idx = hash % 8;
            if (self.bits[byte_idx] & (1 << bit_idx)) == 0 { return false; }
        }
        true
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = vec![self.num_hash_fns];
        out.extend_from_slice(&(self.bits.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.bits);
        out
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.is_empty() { return None; }
        let num_hash_fns = data[0];
        let len = u32::from_le_bytes(data[1..5].try_into().ok()?) as usize;
        let bits = data[5..5+len].to_vec();
        Some(BloomFilter { bits, num_hash_fns })
    }
}

fn hash_fn(data: &[u8], seed: u8) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    data.hash(&mut hasher);
    hasher.finish()
}

// ── Array encoding (variable-length arrays) ────────────────────────────────────
pub fn encode_array_i64(arrays: &[Vec<i64>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(arrays.len() as u32).to_le_bytes());
    for arr in arrays {
        out.extend_from_slice(&(arr.len() as u32).to_le_bytes());
        for &v in arr { out.extend_from_slice(&v.to_le_bytes()); }
    }
    out
}

pub fn decode_array_i64(data: &[u8]) -> Vec<Vec<i64>> {
    if data.len() < 4 { return vec![]; }
    let mut out = Vec::new();
    let num_arrays = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let mut pos = 4;
    for _ in 0..num_arrays {
        if pos + 4 > data.len() { break; }
        let len = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;
        let mut arr = Vec::with_capacity(len);
        for _ in 0..len {
            if pos + 8 > data.len() { break; }
            let v = i64::from_le_bytes(data[pos..pos+8].try_into().unwrap());
            arr.push(v);
            pos += 8;
        }
        out.push(arr);
    }
    out
}

// ── Struct encoding (field offsets) ─────────────────────────────────────────────
pub fn encode_struct_fields(field_data: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(field_data.len() as u16).to_le_bytes());
    for (name, data) in field_data {
        let name_bytes = name.as_bytes();
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
    }
    out
}

pub fn decode_struct_fields(data: &[u8]) -> Vec<(String, Vec<u8>)> {
    if data.len() < 2 { return vec![]; }
    let mut out = Vec::new();
    let num_fields = u16::from_le_bytes(data[0..2].try_into().unwrap()) as usize;
    let mut pos = 2;
    for _ in 0..num_fields {
        if pos + 2 > data.len() { break; }
        let name_len = u16::from_le_bytes(data[pos..pos+2].try_into().unwrap()) as usize;
        pos += 2;
        if pos + name_len > data.len() { break; }
        let name = String::from_utf8_lossy(&data[pos..pos+name_len]).into_owned();
        pos += name_len;
        if pos + 4 > data.len() { break; }
        let field_len = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;
        if pos + field_len > data.len() { break; }
        let field_data = data[pos..pos+field_len].to_vec();
        out.push((name, field_data));
        pos += field_len;
    }
    out
}

// ── MVCC Version Snapshots (time travel support) ────────────────────────────────
pub fn encode_version_snapshots(snapshots: &[crate::VersionSnapshot]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(snapshots.len() as u32).to_le_bytes());
    for snap in snapshots {
        out.extend_from_slice(&snap.version_id.to_le_bytes());
        out.extend_from_slice(&snap.timestamp.to_le_bytes());
        out.extend_from_slice(&snap.block_offset.to_le_bytes());
        out.extend_from_slice(&snap.row_count.to_le_bytes());
        let has_prev = snap.prev_version.is_some();
        out.push(if has_prev { 1 } else { 0 });
        if has_prev {
            out.extend_from_slice(&snap.prev_version.unwrap().to_le_bytes());
        }
    }
    out
}

pub fn decode_version_snapshots(data: &[u8]) -> Vec<crate::VersionSnapshot> {
    if data.len() < 4 { return vec![]; }
    let mut out = Vec::new();
    let num_snapshots = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let mut pos = 4;
    for _ in 0..num_snapshots {
        if pos + 25 > data.len() { break; }
        let version_id = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
        let timestamp = u64::from_le_bytes(data[pos+4..pos+12].try_into().unwrap());
        let block_offset = u64::from_le_bytes(data[pos+12..pos+20].try_into().unwrap());
        let row_count = u64::from_le_bytes(data[pos+20..pos+28].try_into().unwrap());
        let has_prev = data[pos+28] == 1;
        pos += 29;
        let prev_version = if has_prev && pos + 4 <= data.len() {
            let pv = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
            pos += 4;
            Some(pv)
        } else {
            None
        };
        out.push(crate::VersionSnapshot { version_id, timestamp, block_offset, row_count, prev_version });
    }
    out
}

// ── Partition Evolution (dynamic partition spec changes) ────────────────────────
pub fn encode_partition_specs(specs: &[crate::PartitionSpec]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(specs.len() as u16).to_le_bytes());
    for spec in specs {
        out.extend_from_slice(&spec.spec_id.to_le_bytes());
        out.extend_from_slice(&(spec.columns.len() as u16).to_le_bytes());
        for &col_idx in &spec.columns {
            out.extend_from_slice(&col_idx.to_le_bytes());
        }
        out.extend_from_slice(&(spec.transforms.len() as u16).to_le_bytes());
        for transform in &spec.transforms {
            let b = transform.as_bytes();
            out.extend_from_slice(&(b.len() as u16).to_le_bytes());
            out.extend_from_slice(b);
        }
        let has_parent = spec.parent_spec_id.is_some();
        out.push(if has_parent { 1 } else { 0 });
        if has_parent {
            out.extend_from_slice(&spec.parent_spec_id.unwrap().to_le_bytes());
        }
    }
    out
}

pub fn decode_partition_specs(data: &[u8]) -> Vec<crate::PartitionSpec> {
    if data.len() < 2 { return vec![]; }
    let mut out = Vec::new();
    let num_specs = u16::from_le_bytes(data[0..2].try_into().unwrap()) as usize;
    let mut pos = 2;
    for _ in 0..num_specs {
        if pos + 2 > data.len() { break; }
        let spec_id = u16::from_le_bytes(data[pos..pos+2].try_into().unwrap());
        pos += 2;
        if pos + 2 > data.len() { break; }
        let num_cols = u16::from_le_bytes(data[pos..pos+2].try_into().unwrap()) as usize;
        pos += 2;
        let mut columns = Vec::new();
        for _ in 0..num_cols {
            if pos + 2 > data.len() { break; }
            columns.push(u16::from_le_bytes(data[pos..pos+2].try_into().unwrap()));
            pos += 2;
        }
        if pos + 2 > data.len() { break; }
        let num_transforms = u16::from_le_bytes(data[pos..pos+2].try_into().unwrap()) as usize;
        pos += 2;
        let mut transforms = Vec::new();
        for _ in 0..num_transforms {
            if pos + 2 > data.len() { break; }
            let t_len = u16::from_le_bytes(data[pos..pos+2].try_into().unwrap()) as usize;
            pos += 2;
            if pos + t_len > data.len() { break; }
            transforms.push(String::from_utf8_lossy(&data[pos..pos+t_len]).into_owned());
            pos += t_len;
        }
        if pos >= data.len() { break; }
        let has_parent = data[pos] == 1;
        pos += 1;
        let parent_spec_id = if has_parent && pos + 2 <= data.len() {
            let parent = u16::from_le_bytes(data[pos..pos+2].try_into().unwrap());
            pos += 2;
            Some(parent)
        } else {
            None
        };
        out.push(crate::PartitionSpec { spec_id, columns, transforms, parent_spec_id });
    }
    out
}

// ── Row-Level Delete Vectors (soft deletes without full rewrite) ────────────────
pub fn encode_delete_vector(dv: &crate::DeleteVector) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&dv.cardinality.to_le_bytes());
    out.extend_from_slice(&dv.timestamp.to_le_bytes());
    out.extend_from_slice(&(dv.bitmap.len() as u32).to_le_bytes());
    out.extend_from_slice(&dv.bitmap);
    out
}

pub fn decode_delete_vector(data: &[u8]) -> Option<crate::DeleteVector> {
    if data.len() < 16 { return None; }
    let cardinality = u32::from_le_bytes(data[0..4].try_into().ok()?);
    let timestamp = u64::from_le_bytes(data[4..12].try_into().ok()?);
    let bitmap_len = u32::from_le_bytes(data[12..16].try_into().ok()?) as usize;
    if 16 + bitmap_len > data.len() { return None; }
    let bitmap = data[16..16+bitmap_len].to_vec();
    Some(crate::DeleteVector { bitmap, cardinality, timestamp })
}
