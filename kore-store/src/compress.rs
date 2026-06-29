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
        for _ in 0..run.min(n - out.len()) { out.push(v); }
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
