//! Shared pieces of rule mode: a bounds-checked byte reader, zigzag varints, *streams* (a byte buffer
//! coded with the best of the version-4 stages), embedded version-4 column payloads, the digest of a
//! decoded column and the byte-plane shuffle used for 64-bit residuals.

use kore_core::{ColumnData, KoreError};
use crate::codec::{self, ColumnCodec, Stage};

pub(crate) fn bad(m: impl Into<String>) -> KoreError { KoreError::InvalidArgument(m.into()) }

/// hz is only tried below this raw size (same rule as the version-4 writer).
const HZ_MAX: usize = 128 << 10;
/// Below this raw size a stream is zstd-coded at `Levels::small` (same rule as the version-4 writer).
const SMALL: usize = 256 << 10;
/// Hard cap on one decoded stream.
const MAX_STREAM: usize = 1 << 31;

/// zstd levels of the surrounding version-4 options (large payloads, payloads below 256 KiB).
#[derive(Clone, Copy, Debug)]
pub struct Levels { pub large: i32, pub small: i32 }

pub(crate) fn zz(v: i64) -> u64 { ((v << 1) ^ (v >> 63)) as u64 }
pub(crate) fn unzz(v: u64) -> i64 { ((v >> 1) as i64) ^ -((v & 1) as i64) }
pub(crate) fn put_varint(out: &mut Vec<u8>, v: u64) { codec::put_varint(out, v) }
pub(crate) fn put_zz(out: &mut Vec<u8>, v: i64) { codec::put_varint(out, zz(v)) }
pub(crate) fn varint_len(v: u64) -> usize { if v == 0 { 1 } else { (64 - v.leading_zeros() as usize + 6) / 7 } }

/// Bounds-checked reader over one payload. Every method fails with an error on truncation.
pub struct Rd<'a> { pub d: &'a [u8], pub pos: usize }

impl<'a> Rd<'a> {
    pub fn new(d: &'a [u8]) -> Self { Rd { d, pos: 0 } }
    pub fn u8(&mut self) -> Result<u8, KoreError> {
        let b = *self.d.get(self.pos).ok_or_else(|| bad("truncated rule payload"))?;
        self.pos += 1;
        Ok(b)
    }
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], KoreError> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.d.len()).ok_or_else(|| bad("truncated rule payload"))?;
        let s = &self.d[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    pub fn u64le(&mut self) -> Result<u64, KoreError> { Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap())) }
    pub fn varint(&mut self) -> Result<u64, KoreError> { codec::get_varint(self.d, &mut self.pos) }
    pub fn zz(&mut self) -> Result<i64, KoreError> { Ok(unzz(self.varint()?)) }
    /// A varint that must be at most `max` (counts, lengths, indices).
    pub fn count(&mut self, max: usize) -> Result<usize, KoreError> {
        let v = self.varint()?;
        if v > max as u64 { return Err(bad("implausible count in rule payload")); }
        Ok(v as usize)
    }
    pub fn done(&self) -> Result<(), KoreError> {
        if self.pos == self.d.len() { Ok(()) } else { Err(bad("trailing bytes in rule payload")) }
    }
}

/// Code `raw` with the smallest of: no stage, lz4, zstd (level by size, as the version-4 writer), hz
/// (below 128 KiB). Layout: `stage:u8 | varint(body_len) | body` (body as the version-4 stage payload).
/// Ties keep the earlier candidate in the order none, lz4, zstd, hz.
pub(crate) fn put_stream(out: &mut Vec<u8>, raw: &[u8], lv: Levels) {
    let (stage, body) = best_stage(raw, lv);
    out.push(stage);
    put_varint(out, body.as_ref().map_or(raw.len(), |b| b.len()) as u64);
    match body { Some(b) => out.extend_from_slice(&b), None => out.extend_from_slice(raw) }
}


fn best_stage(raw: &[u8], lv: Levels) -> (u8, Option<Vec<u8>>) {
    if raw.len() < 16 { return (0, None); }
    let mut best: (u8, Option<Vec<u8>>) = (0, None);
    let mut best_len = raw.len();
    let consider = |s: u8, b: Vec<u8>, best: &mut (u8, Option<Vec<u8>>), best_len: &mut usize| {
        if b.len() < *best_len { *best_len = b.len(); *best = (s, Some(b)); }
    };
    consider(1, codec::encode_stage(raw, Stage::Lz4), &mut best, &mut best_len);
    let lvl = if raw.len() < SMALL { lv.small } else { lv.large };
    let mut z = Vec::new();
    put_varint(&mut z, raw.len() as u64);
    z.extend_from_slice(&zstd::bulk::compress(raw, lvl).expect("zstd in-memory compress"));
    consider(3, z, &mut best, &mut best_len);
    if raw.len() < HZ_MAX { consider(2, codec::encode_stage(raw, Stage::Hz), &mut best, &mut best_len); }
    best
}

/// Inverse of `put_stream`. `max_raw` bounds the decoded size (refused before any allocation).
pub(crate) fn get_stream(rd: &mut Rd, max_raw: usize) -> Result<Vec<u8>, KoreError> {
    let stage = Stage::from_id(rd.u8()?)?;
    let len = rd.count(rd.d.len())?;
    let body = rd.bytes(len)?;
    let max_raw = max_raw.min(MAX_STREAM);
    if stage != Stage::None {
        let mut p = 0;
        let raw_len = codec::get_varint(body, &mut p)?;
        if raw_len > max_raw as u64 { return Err(bad("implausible stream size in rule payload")); }
    } else if body.len() > max_raw {
        return Err(bad("implausible stream size in rule payload"));
    }
    Ok(codec::decode_stage(body, stage)?.into_owned())
}

/// Bound on the raw bytes of a stream of `n` varints.
pub(crate) fn varints_max(n: usize) -> usize { n.saturating_mul(10).saturating_add(16) }

/// Decode exactly `n` zigzag varints from a stream.
pub(crate) fn get_zz_stream(rd: &mut Rd, n: usize) -> Result<Vec<i64>, KoreError> {
    let raw = get_stream(rd, varints_max(n))?;
    let mut out = Vec::with_capacity(n);
    let mut p = 0;
    for _ in 0..n { out.push(unzz(codec::get_varint(&raw, &mut p)?)); }
    if p != raw.len() { return Err(bad("trailing bytes in residual stream")); }
    Ok(out)
}

/// Decode exactly `n` varints (each at most `max`) from a stream.
pub(crate) fn get_uint_stream(rd: &mut Rd, n: usize, max: u64) -> Result<Vec<u64>, KoreError> {
    let raw = get_stream(rd, varints_max(n))?;
    let mut out = Vec::with_capacity(n);
    let mut p = 0;
    for _ in 0..n {
        let v = codec::get_varint(&raw, &mut p)?;
        if v > max { return Err(bad("value out of range in rule stream")); }
        out.push(v);
    }
    if p != raw.len() { return Err(bad("trailing bytes in rule stream")); }
    Ok(out)
}

pub(crate) fn zz_bytes(r: &[i64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(r.len() + 16);
    for &x in r { put_zz(&mut out, x); }
    out
}

// ── null bitmap ──────────────────────────────────────────────────────────────

/// `has_nulls:u8` then, if 1, a stream of the bitmap (LSB first, bit = 1 means NULL).
pub(crate) fn put_nulls(out: &mut Vec<u8>, is_null: &[bool], lv: Levels) {
    if !is_null.iter().any(|&b| b) { out.push(0); return; }
    out.push(1);
    let mut bm = vec![0u8; is_null.len().div_ceil(8)];
    for (i, &b) in is_null.iter().enumerate() { if b { bm[i / 8] |= 1 << (i % 8); } }
    put_stream(out, &bm, lv);
}

pub(crate) fn get_nulls(rd: &mut Rd, n: usize) -> Result<Vec<bool>, KoreError> {
    match rd.u8()? {
        0 => Ok(vec![false; n]),
        1 => {
            let bm = get_stream(rd, n.div_ceil(8))?;
            if bm.len() != n.div_ceil(8) { return Err(bad("null bitmap length mismatch")); }
            if n % 8 != 0 && bm.last().map_or(false, |&b| b >> (n % 8) != 0) { return Err(bad("stray bits in null bitmap")); }
            Ok((0..n).map(|i| bm[i / 8] >> (i % 8) & 1 == 1).collect())
        }
        _ => Err(bad("bad null flag in rule payload")),
    }
}

// ── embedded version-4 column payloads ───────────────────────────────────────

/// A whole column coded by the version-4 writer (`Auto`, no shared dictionary):
/// `transform:u8 | stage:u8 | varint(len) | payload`.
pub(crate) fn put_v4col(out: &mut Vec<u8>, data: &ColumnData, lv: Levels) -> Result<(), KoreError> {
    let (t, s, _, p) = crate::v4::encode_column(data, ColumnCodec::AUTO, None, lv.large, lv.small)?;
    out.push(t);
    out.push(s);
    put_varint(out, p.len() as u64);
    out.extend_from_slice(&p);
    Ok(())
}

pub(crate) fn get_v4col(rd: &mut Rd, dtype: crate::DType, n: usize) -> Result<ColumnData, KoreError> {
    let t = rd.u8()?;
    let s = rd.u8()?;
    if s == crate::v4::STAGE_ZSTD_DICT { return Err(bad("shared dictionary inside a rule payload")); }
    let len = rd.count(rd.d.len())?;
    let p = rd.bytes(len)?;
    let d = crate::v4::decode_payload(p, dtype, t, s, None, n)?;
    if d.len() != n { return Err(bad("embedded column length mismatch")); }
    Ok(d)
}

// ── digest of a decoded column ───────────────────────────────────────────────

/// CRC-64/XZ over a canonical layout of the values: dtype, row count, then per row a null flag and the
/// value (i64 LE, f64 bits LE, bool byte, string length varint + bytes). Bit-exact: -0.0, NaN payloads
/// and null positions all change it.
pub fn digest(d: &ColumnData) -> u64 {
    use crc::{Crc, Table, CRC_64_XZ};
    const C: Crc<u64, Table<16>> = Crc::<u64, Table<16>>::new(&CRC_64_XZ);
    let mut h = C.digest();
    let mut buf: Vec<u8> = Vec::with_capacity(1 << 16);
    buf.push(codec::dtype_of(d) as u8);
    buf.extend_from_slice(&(d.len() as u64).to_le_bytes());
    let flush = |buf: &mut Vec<u8>, h: &mut crc::Digest<u64, Table<16>>| { if buf.len() >= (1 << 16) - 64 { h.update(buf); buf.clear(); } };
    match d {
        ColumnData::Int64(v) => for x in v {
            match x { None => buf.push(0), Some(x) => { buf.push(1); buf.extend_from_slice(&x.to_le_bytes()); } }
            flush(&mut buf, &mut h);
        },
        ColumnData::Float64(v) => for x in v {
            match x { None => buf.push(0), Some(x) => { buf.push(1); buf.extend_from_slice(&x.to_bits().to_le_bytes()); } }
            flush(&mut buf, &mut h);
        },
        ColumnData::Bool(v) => for x in v {
            buf.push(match x { None => 0, Some(false) => 1, Some(true) => 2 });
            flush(&mut buf, &mut h);
        },
        ColumnData::Str(v) => for x in v {
            match x {
                None => buf.push(0),
                Some(s) => {
                    buf.push(1);
                    put_varint(&mut buf, s.len() as u64);
                    if s.len() > 4096 { h.update(&buf); buf.clear(); h.update(s.as_bytes()); } else { buf.extend_from_slice(s.as_bytes()); }
                }
            }
            flush(&mut buf, &mut h);
        },
        ColumnData::StrDict { codes, dict } => for &c in codes {
            match dict.get(c as usize) {
                Some(s) if c != u8::MAX => { buf.push(1); put_varint(&mut buf, s.len() as u64); buf.extend_from_slice(s.as_bytes()); }
                _ => buf.push(0),
            }
            flush(&mut buf, &mut h);
        },
    }
    h.update(&buf);
    h.finalize()
}

// ── byte planes ──────────────────────────────────────────────────────────────

/// Byte-plane layout of 64-bit words: all byte 0s, then all byte 1s, ... (LE byte order).
pub(crate) fn shuffle8(v: &[u64]) -> Vec<u8> {
    let n = v.len();
    let mut out = vec![0u8; n * 8];
    for (i, x) in v.iter().enumerate() {
        let b = x.to_le_bytes();
        for j in 0..8 { out[j * n + i] = b[j]; }
    }
    out
}

pub(crate) fn unshuffle8(b: &[u8], n: usize) -> Result<Vec<u64>, KoreError> {
    if b.len() != n.checked_mul(8).ok_or_else(|| bad("overflow"))? { return Err(bad("byte-plane stream length mismatch")); }
    Ok((0..n).map(|i| {
        let mut w = [0u8; 8];
        for j in 0..8 { w[j] = b[j * n + i]; }
        u64::from_le_bytes(w)
    }).collect())
}

pub(crate) fn put_u64_stream(out: &mut Vec<u8>, v: &[u64], lv: Levels) { put_stream(out, &shuffle8(v), lv) }
pub(crate) fn get_u64_stream(rd: &mut Rd, n: usize) -> Result<Vec<u64>, KoreError> {
    let raw = get_stream(rd, n.saturating_mul(8))?;
    unshuffle8(&raw, n)
}

/// Sample windows over `0..m` used for scoring: the whole range if short, else three windows of `w`
/// (start, middle, end).
pub(crate) fn windows(m: usize, w: usize) -> Vec<std::ops::Range<usize>> {
    if m <= 3 * w { return vec![0..m]; }
    let mid = m / 2 - w / 2;
    vec![0..w, mid..mid + w, m - w..m]
}

/// Cheap size score of a byte buffer: min(raw, zstd level 1).
pub(crate) fn score(raw: &[u8]) -> usize {
    if raw.len() < 64 { return raw.len(); }
    raw.len().min(zstd::bulk::compress(raw, 1).expect("zstd").len() + 4)
}
