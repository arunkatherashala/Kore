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
    let mut out = Vec::with_capacity(n.min(1 << 16));
    let mut i = 0;
    while i + 13 <= data.len() && out.len() < n {
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
    let mut out = Vec::with_capacity(n.min(data.len() / 9 + 1));
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
    let mut out = Vec::with_capacity(n.min(data.len() / 9 + 1));
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
    let off_end = match (n + 1).checked_mul(4).and_then(|o| null_end.checked_add(o)) {
        Some(e) if e <= data.len() => e,
        _ => return vec![],
    };
    let null_flags = &data[4..null_end];
    let offsets = &data[null_end..off_end];
    let body = &data[off_end..];
    let offset = |i: usize| u32::from_le_bytes(offsets[i * 4..i * 4 + 4].try_into().unwrap()) as usize;
    // Validate the string bytes once; a slice between two char boundaries of valid UTF-8 is valid
    // UTF-8, so per-string validation is only needed when the whole body is not valid (lossy path).
    let text = std::str::from_utf8(body).ok();
    let one = |i: usize| -> Option<String> {
        let (a, b) = (offset(i), offset(i + 1));
        if null_flags[i] == 1 || a > b || b > body.len() { return None; }
        Some(utf8_owned(text, body, a, b))
    };
    build_strs(n, one)
}

/// Rows from which a string column's values are built on several threads (allocation-bound).
const PAR_STR_ROWS: usize = 1 << 15;

/// `(0..n).map(one).collect()`, in parallel for large columns (same values, same order).
fn build_strs(n: usize, one: impl Fn(usize) -> Option<String> + Sync + Send) -> Vec<Option<String>> {
    if n >= PAR_STR_ROWS {
        use rayon::prelude::*;
        (0..n).into_par_iter().with_min_len(4096).map(one).collect()
    } else {
        (0..n).map(one).collect()
    }
}

/// `String::from_utf8_lossy(&body[a..b]).into_owned()`, skipping validation when `text` (the whole of
/// `body`, already validated) has char boundaries at both ends.
#[inline]
fn utf8_owned(text: Option<&str>, body: &[u8], a: usize, b: usize) -> String {
    match text {
        Some(t) if t.is_char_boundary(a) && t.is_char_boundary(b) => t[a..b].to_owned(),
        _ => String::from_utf8_lossy(&body[a..b]).into_owned(),
    }
}

/// Strings as `[count:4] [null flags: count] [byte length per string: count*4] [bytes...]`.
/// Lengths repeat far more than the absolute offsets of `encode_strs`, so they compress much better.
pub fn encode_strs_len(vals: &[Option<String>]) -> Vec<u8> {
    let total: usize = vals.iter().map(|v| v.as_ref().map_or(0, |s| s.len())).sum();
    let mut out = Vec::with_capacity(4 + vals.len() * 5 + total);
    out.extend_from_slice(&(vals.len() as u32).to_le_bytes());
    out.extend(vals.iter().map(|v| v.is_none() as u8));
    for v in vals {
        out.extend_from_slice(&(v.as_ref().map_or(0, |s| s.len()) as u32).to_le_bytes());
    }
    for s in vals.iter().flatten() { out.extend_from_slice(s.as_bytes()); }
    out
}

/// Inverse of `encode_strs_len`; malformed input yields an empty vector (length mismatch is
/// reported by the caller).
pub fn decode_strs_len(data: &[u8]) -> Vec<Option<String>> {
    if data.len() < 4 { return vec![]; }
    let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let Some(len_end) = n.checked_mul(5).and_then(|x| x.checked_add(4)) else { return vec![] };
    if n == 0 || len_end > data.len() { return vec![]; }
    let flags = &data[4..4 + n];
    let lens = &data[4 + n..len_end];
    let body = &data[len_end..];
    // start of every value (nulls take no bytes); any value running past the end voids the column
    let mut starts = Vec::with_capacity(n + 1);
    let mut pos = 0usize;
    for i in 0..n {
        starts.push(pos);
        if flags[i] == 1 { continue; }
        let l = u32::from_le_bytes(lens[i * 4..i * 4 + 4].try_into().unwrap()) as usize;
        match pos.checked_add(l).filter(|&e| e <= body.len()) {
            Some(e) => pos = e,
            None => return vec![],
        }
    }
    starts.push(pos);
    let text = std::str::from_utf8(body).ok();
    build_strs(n, |i| if flags[i] == 1 { None } else { Some(utf8_owned(text, body, starts[i], starts[i + 1])) })
}

// ── NaN-sentinel Float64 (8 bytes/element, fastest read path) ─────────────────
// NaN = null; all other f64 values are real data.
//
// NULL is the canonical quiet NaN (0x7FF8_0000_0000_0000). A genuine NaN value is stored with a
// different payload so it survives as NaN; files written before this distinction decode their
// canonical NaNs as NULL exactly as before.
const NULL_BITS: u64 = 0x7FF8_0000_0000_0000;
const REAL_NAN_BITS: u64 = 0x7FF8_0000_0000_0001;

#[inline]
fn f64_to_stored(v: Option<f64>) -> u64 {
    match v {
        None => NULL_BITS,
        Some(x) if x.is_nan() => REAL_NAN_BITS,
        Some(x) => x.to_bits(),
    }
}

#[inline]
fn f64_from_stored(bits: u64) -> Option<f64> {
    if bits == NULL_BITS { None } else { Some(f64::from_bits(bits)) }
}

pub fn nan_encode_f64(vals: &[Option<f64>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vals.len() * 8);
    for &v in vals { out.extend_from_slice(&f64_to_stored(v).to_le_bytes()); }
    out
}

pub fn nan_decode_f64(data: &[u8], n: usize) -> Vec<Option<f64>> {
    let mut out = Vec::with_capacity(n.min(data.len() / 8 + 1));
    for chunk in data.chunks_exact(8).take(n) {
        out.push(f64_from_stored(u64::from_le_bytes(chunk.try_into().unwrap())));
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
        let bits = f64_to_stored(v);
        if let Some(&code) = dict_map.get(&bits) {
            codes.push(code);
        } else {
            if dict.len() >= 255 { return None; }
            let code = dict.len() as u8;
            dict_map.insert(bits, code);
            dict.push(f64::from_bits(bits));
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
    if data.is_empty() { return vec![]; }
    let dict_len = data[0] as usize;
    let mut dict: Vec<Option<f64>> = Vec::with_capacity(dict_len);
    let mut i = 1;
    for _ in 0..dict_len {
        if i + 8 > data.len() { break; }
        let bits = u64::from_le_bytes(data[i..i+8].try_into().unwrap()); i += 8;
        dict.push(f64_from_stored(bits));
    }
    let mut out = Vec::with_capacity(n.min(data.len()));
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
    if data.len() < 2 { return (vec![], vec![]); }
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
    zstd_encode_reused(data).unwrap_or_else(|| zstd::encode_all(data, 3).unwrap_or_else(|_| data.to_vec()))
}

/// Byte-for-byte the same as `zstd::encode_all(data, 3)` (same streaming encoder, same calls), but on
/// a per-thread compression context created once instead of once per column. None when the context is
/// busy or any step fails; the caller then uses `encode_all` itself.
fn zstd_encode_reused(data: &[u8]) -> Option<Vec<u8>> {
    use std::cell::RefCell;
    use zstd::zstd_safe::{CCtx, CParameter, ResetDirective};
    thread_local! { static CCTX: RefCell<Option<CCtx<'static>>> = const { RefCell::new(None) }; }
    CCTX.with(|cell| {
        let mut slot = cell.try_borrow_mut().ok()?;
        if slot.is_none() {
            // what `zstd::stream::raw::Encoder::new(3)` sets up
            let mut c = CCtx::create();
            c.set_parameter(CParameter::CompressionLevel(3)).ok()?;
            c.load_dictionary(&[]).ok()?;
            *slot = Some(c);
        }
        let cctx = slot.as_mut()?;
        // a new frame from a clean session; parameters (level 3) are kept
        cctx.reset(ResetDirective::SessionOnly).ok()?;
        let mut enc = zstd::stream::write::Encoder::with_context(Vec::<u8>::new(), cctx);
        let mut src = data;
        let r = std::io::copy(&mut src, &mut enc).and_then(|_| enc.finish());
        match r {
            Ok(v) => Some(v),
            Err(_) => { *slot = None; None }
        }
    })
}

/// Transpose fixed-width records into byte planes (all byte 0s, then all byte 1s, ...).
/// `data.len()` must be a multiple of `width`.
pub fn byte_shuffle(data: &[u8], width: usize) -> Vec<u8> {
    let n = data.len() / width;
    let mut out = vec![0u8; data.len()];
    for i in 0..n {
        for j in 0..width { out[j * n + i] = data[i * width + j]; }
    }
    out
}

/// Inverse of `byte_shuffle`.
pub fn byte_unshuffle(data: &[u8], width: usize) -> Vec<u8> {
    let n = data.len() / width;
    let mut out = vec![0u8; data.len()];
    for i in 0..n {
        for j in 0..width { out[i * width + j] = data[j * n + i]; }
    }
    out
}

/// Decompress a ZSTD stream (one or more frames), returning exactly what `zstd::decode_all` returns.
///
/// Fast path: one reused decompression context per thread, output written straight into a single
/// buffer pre-sized from `size_hint` (or the frame header when it records the content size), with no
/// intermediate read buffer. Anything the fast path does not finish cleanly (error, truncated frame,
/// trailing bytes, empty input) is handed to `zstd::decode_all`, so results and errors are unchanged.
pub fn zstd_decode_all(src: &[u8], size_hint: usize) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    if zstd_decode_into(src, size_hint, &mut out) { Ok(out) } else { zstd::decode_all(src) }
}

/// The fast path of `zstd_decode_all`, decoding into `out` (cleared first; its capacity is reused).
/// Returns false, with `out` in an unspecified state, when the input needs `zstd::decode_all` to get
/// the result or the error that function gives.
pub fn zstd_decode_into(src: &[u8], size_hint: usize, out: &mut Vec<u8>) -> bool {
    use std::cell::RefCell;
    use zstd::zstd_safe::{DCtx, InBuffer, OutBuffer, ResetDirective};
    thread_local! { static DCTX: RefCell<Option<DCtx<'static>>> = const { RefCell::new(None) }; }
    if src.is_empty() { return false; }
    // A recorded content size is only a hint here (a stream can hold several frames); cap it so a
    // corrupt header cannot force a huge allocation up front.
    let declared = zstd::zstd_safe::get_frame_content_size(src).ok().flatten().unwrap_or(0) as usize;
    let cap_limit = src.len().saturating_mul(64).max(1 << 16);
    let first = size_hint.max(declared).max(src.len().saturating_mul(2)).clamp(256, cap_limit);
    out.clear();
    if out.capacity() < first { out.reserve(first); }
    DCTX.with(|cell| {
        let Ok(mut slot) = cell.try_borrow_mut() else { return false };
        let dctx = slot.get_or_insert_with(DCtx::create);
        if dctx.reset(ResetDirective::SessionOnly).is_err() { return false; }
        let mut input = InBuffer::around(src);
        loop {
            if out.capacity() - out.len() < 4096 { out.reserve(out.len().max(1 << 16)); }
            let (in_before, out_before) = (input.pos(), out.len());
            // the output buffer sets `out.len()` to the bytes written when it goes out of scope
            let r = dctx.decompress_stream(&mut OutBuffer::around_pos(&mut *out, out_before), &mut input);
            let hint = match r { Ok(h) => h, Err(_) => { let _ = dctx.reset(ResetDirective::SessionOnly); return false; } };
            let consumed = input.pos() == src.len();
            if hint == 0 && consumed { return true; }
            let full = out.capacity() == out.len();
            if consumed && !full && input.pos() == in_before && out.len() == out_before {
                // input exhausted mid-frame: truncated; let decode_all produce its error
                let _ = dctx.reset(ResetDirective::SessionOnly);
                return false;
            }
        }
    })
}

pub fn zstd_decode(data: &[u8], n: usize) -> Vec<u8> {
    zstd::decode_all(data).unwrap_or_else(|_| vec![0; n])
}

// ── CRC32 checksums for data integrity ─────────────────────────────────────────
/// CRC-32 (ISO-HDLC, the zlib/PNG polynomial). Slice-by-16 tables: the same checksum as the
/// byte-at-a-time default of the `crc` crate, several times faster on large columns.
pub fn crc32(data: &[u8]) -> u32 {
    use crc::{Crc, Table, CRC_32_ISO_HDLC};
    const CRC: Crc<u32, Table<16>> = Crc::<u32, Table<16>>::new(&CRC_32_ISO_HDLC);
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

#[cfg(test)]
mod speed_equivalence_tests {
    //! The faster implementations must return exactly what the ones they replace returned.
    use super::*;

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 { let mut x = self.0; x ^= x << 13; x ^= x >> 7; x ^= x << 17; self.0 = x; x }
        fn bytes(&mut self, n: usize, alphabet: u64) -> Vec<u8> { (0..n).map(|_| (self.next() % alphabet) as u8).collect() }
    }

    fn crc32_bytewise(data: &[u8]) -> u32 {
        use crc::{Crc, CRC_32_ISO_HDLC};
        const CRC: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);
        CRC.checksum(data)
    }

    // the decoders exactly as they were before the speed work
    fn decode_strs_reference(data: &[u8]) -> Vec<Option<String>> {
        if data.len() < 4 { return vec![]; }
        let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        if n == 0 { return vec![]; }
        let null_end = 4 + n;
        let off_end = match (n + 1).checked_mul(4).and_then(|o| null_end.checked_add(o)) {
            Some(e) if e <= data.len() => e,
            _ => return vec![],
        };
        let null_flags = &data[4..null_end];
        let off_start = null_end;
        let data_start = off_end;
        let offset = |i: usize| u32::from_le_bytes(data[off_start + i*4..off_start + i*4 + 4].try_into().unwrap()) as usize;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let (o1, o2) = (offset(i), offset(i + 1));
            let bytes = if null_flags[i] == 1 { None } else {
                data_start.checked_add(o1).zip(data_start.checked_add(o2)).and_then(|(a, b)| data.get(a..b))
            };
            out.push(bytes.map(|b| String::from_utf8_lossy(b).into_owned()));
        }
        out
    }

    fn decode_strs_len_reference(data: &[u8]) -> Vec<Option<String>> {
        if data.len() < 4 { return vec![]; }
        let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        let Some(len_end) = n.checked_mul(5).and_then(|x| x.checked_add(4)) else { return vec![] };
        if n == 0 || len_end > data.len() { return vec![]; }
        let flags = &data[4..4 + n];
        let lens = &data[4 + n..len_end];
        let mut pos = len_end;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let l = u32::from_le_bytes(lens[i * 4..i * 4 + 4].try_into().unwrap()) as usize;
            if flags[i] == 1 { out.push(None); continue; }
            match pos.checked_add(l).and_then(|e| data.get(pos..e)) {
                Some(b) => { out.push(Some(String::from_utf8_lossy(b).into_owned())); pos += l; }
                None => return vec![],
            }
        }
        out
    }

    fn sample_strs(r: &mut Rng, n: usize) -> Vec<Option<String>> {
        let pool = ["", "a", "caf\u{e9}", "\u{1f600}x", "plain ascii text", "\u{4e2d}\u{6587}"];
        (0..n).map(|i| if r.next() % 9 == 0 { None } else { Some(format!("{}{}", pool[(r.next() % 6) as usize], i % 13)) }).collect()
    }

    #[test]
    fn crc32_matches_the_bytewise_table_and_the_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        let mut r = Rng(0x1234_5678_9abc_def1);
        for n in [0usize, 1, 3, 15, 16, 17, 31, 64, 1000, 65_537] {
            let d = r.bytes(n, 256);
            assert_eq!(crc32(&d), crc32_bytewise(&d), "len {n}");
        }
    }

    #[test]
    fn zstd_encode_is_byte_identical_to_encode_all() {
        let mut r = Rng(0x0bad_cafe_f00d_1234);
        let mut inputs: Vec<Vec<u8>> = vec![vec![], vec![7], vec![0; 300_000]];
        for n in [10usize, 64, 1000, 4096, 131_071, 131_072, 131_073, 400_000] {
            inputs.push(r.bytes(n, 256));
            inputs.push(r.bytes(n, 4));
        }
        // the same thread twice (context reuse) and other threads (fresh contexts)
        for _ in 0..2 {
            for d in &inputs { assert_eq!(zstd_encode(d), zstd::encode_all(&d[..], 3).unwrap(), "len {}", d.len()); }
        }
        std::thread::scope(|s| for d in &inputs {
            s.spawn(move || assert_eq!(zstd_encode(d), zstd::encode_all(&d[..], 3).unwrap()));
        });
    }

    #[test]
    fn zstd_decode_all_matches_decode_all_on_good_and_bad_input() {
        let mut r = Rng(0x5eed_5eed_5eed_5eed);
        let mut cases: Vec<Vec<u8>> = vec![vec![], vec![0x28, 0xb5, 0x2f, 0xfd], r.bytes(100, 256)];
        for n in [1usize, 100, 70_000, 300_000] {
            let raw = r.bytes(n, 5);
            let z = zstd::encode_all(&raw[..], 3).unwrap();
            cases.push(z.clone());
            cases.push(z[..z.len() / 2].to_vec()); // truncated
            let mut two = z.clone(); two.extend_from_slice(&z); cases.push(two); // two frames
            let mut junk = z.clone(); junk.extend_from_slice(b"junk"); cases.push(junk); // trailing bytes
            let mut flip = z.clone(); let k = flip.len() / 3; flip[k] ^= 0x40; cases.push(flip);
            cases.push(zstd::bulk::compress(&raw, 3).unwrap()); // content size in the header
        }
        for (i, c) in cases.iter().enumerate() {
            for hint in [0usize, 10, 1 << 20] {
                let want = zstd::decode_all(&c[..]);
                let got = zstd_decode_all(c, hint);
                match (&want, &got) {
                    (Ok(a), Ok(b)) => assert_eq!(a, b, "case {i}"),
                    (Err(_), Err(_)) => {}
                    _ => panic!("case {i}: decode_all {:?} vs fast {:?}", want.as_ref().map(|v| v.len()), got.as_ref().map(|v| v.len())),
                }
            }
        }
    }

    #[test]
    fn string_decoders_match_the_previous_ones_on_valid_and_damaged_input() {
        let mut r = Rng(0xfeed_beef_dead_c0de);
        for n in [0usize, 1, 2, 50, 1000] {
            let v = sample_strs(&mut r, n);
            for enc in [encode_strs(&v), encode_strs_len(&v)] {
                assert_eq!(decode_strs(&encode_strs(&v)), decode_strs_reference(&encode_strs(&v)));
                assert_eq!(decode_strs_len(&encode_strs_len(&v)), decode_strs_len_reference(&encode_strs_len(&v)));
                // damage: every single byte flipped in a few ways, and every truncation
                for k in 0..enc.len().min(600) {
                    for x in [0x01u8, 0x80, 0xff] {
                        let mut m = enc.clone(); m[k] ^= x;
                        assert_eq!(decode_strs(&m), decode_strs_reference(&m));
                        assert_eq!(decode_strs_len(&m), decode_strs_len_reference(&m));
                    }
                    let t = &enc[..k];
                    assert_eq!(decode_strs(t), decode_strs_reference(t));
                    assert_eq!(decode_strs_len(t), decode_strs_len_reference(t));
                }
            }
        }
        // large columns take the multi-threaded path
        let v = sample_strs(&mut r, 3 * PAR_STR_ROWS);
        for enc in [encode_strs(&v), encode_strs_len(&v)] {
            for k in [0usize, 3, 5, enc.len() / 2, enc.len() - 1] {
                let mut m = enc.clone(); m[k] ^= 0x80;
                assert_eq!(decode_strs(&m), decode_strs_reference(&m));
                assert_eq!(decode_strs_len(&m), decode_strs_len_reference(&m));
            }
        }
        assert_eq!(decode_strs(&encode_strs(&v)), v);
        assert_eq!(decode_strs_len(&encode_strs_len(&v)), v);
        // invalid UTF-8 inside one string and a char split across two strings
        let mut raw = encode_strs(&[Some("ab".into()), Some("\u{e9}".into())]);
        let l = raw.len(); raw[l - 1] = 0xff;
        assert_eq!(decode_strs(&raw), decode_strs_reference(&raw));
        let mut split = encode_strs(&[Some("x\u{e9}".into()), Some("y".into())]);
        let off = 4 + 2 + 4; // second offset
        split[off..off + 4].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(decode_strs(&split), decode_strs_reference(&split));
    }
}
