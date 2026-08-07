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
    if data.len() < 4 + n + (n + 1) * 4 { return vec![]; }
    let null_flags = &data[4..4 + n];
    let off_start  = 4 + n;
    let data_start = off_start + (n + 1) * 4;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let is_null = null_flags[i] == 1;
        let o1 = u32::from_le_bytes(data[off_start + i*4..off_start + i*4 + 4].try_into().unwrap()) as usize;
        let o2 = u32::from_le_bytes(data[off_start + (i+1)*4..off_start + (i+1)*4 + 4].try_into().unwrap()) as usize;
        if is_null {
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
