//! Per-column codecs for the version-3 container (opt-in, see `v3`).
//!
//! A column payload is produced in two steps:
//!
//! 1. a **transform** lays the typed values out as bytes (`Plain`, `Delta`, `Varint`, `Dict`,
//!    `HexFixed`),
//! 2. an optional **stage** compresses those bytes (`None`, `Lz4`, `Hz` entropy coder, `Zstd`).
//!
//! Both ids are stored per column in the file directory. `Auto` is a writer-side request only: it tries
//! the applicable candidates and stores the concrete winner (smallest bytes, ties resolved by the fixed
//! candidate order, so the output is a pure function of the input).
//!
//! Null bitmap used by every transform except `Dict` (which reserves code 0 for null):
//! `has_nulls:u8` then, if 1, `ceil(n/8)` bytes, LSB first, bit = 1 means NULL. Only non-null values are
//! stored afterwards.

use kore_core::{ColumnData, KoreError};
use crate::hz;

fn bad(m: impl Into<String>) -> KoreError { KoreError::InvalidArgument(m.into()) }

/// Hard cap on the size of one decoded column payload.
const MAX_RAW: usize = 1 << 31;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transform {
    /// writer-side: choose among the transforms valid for the column type
    Auto,
    /// i64: 8 bytes LE per value | f64: 8 bytes of IEEE bits | bool: bit-packed | str: varint lengths then bytes
    /// | str_dict: legacy dictionary layout
    Plain,
    /// i64: zigzag varint of the wrapping difference to the previous non-null value (first against 0)
    Delta,
    /// i64: zigzag varint of the value
    Varint,
    /// str: dictionary of any size (first-appearance order), codes 1..=len, 0 = NULL, width 1/2/4 bytes
    Dict,
    /// str: every non-null value is the same even-length lowercase hex string; stored as raw bytes (half the size)
    HexFixed,
    /// str (version-4 files only): every non-null value is a canonical UTC timestamp
    /// `YYYY-MM-DDTHH:MM:SSZ` (years 0000-9999); stored as zigzag varint deltas of epoch seconds
    TsText,
    /// str (version-4 files only): values are lists split on ','; token dictionary (first appearance) and
    /// per value `varint(count)` then `varint(code)` per token; "" is a list of zero tokens
    Tokens,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// writer-side: choose among None, Lz4, Hz and Zstd
    Auto,
    None,
    Lz4,
    Hz,
    Zstd,
}

impl Transform {
    pub fn id(self) -> Option<u8> {
        match self {
            Transform::Auto => None, Transform::Plain => Some(0), Transform::Delta => Some(1),
            Transform::Varint => Some(2), Transform::Dict => Some(3), Transform::HexFixed => Some(4),
            Transform::TsText => Some(5), Transform::Tokens => Some(6),
        }
    }
    /// True for transforms that only version-4 files may use (version-3 readers do not know them).
    pub fn v4_only(self) -> bool { matches!(self, Transform::TsText | Transform::Tokens) }
    /// Transform ids accepted in version-4 files (a superset of `from_id`).
    pub fn from_id_v4(v: u8) -> Result<Self, KoreError> {
        match v {
            5 => Ok(Transform::TsText),
            6 => Ok(Transform::Tokens),
            _ => Self::from_id(v),
        }
    }
    pub fn from_id(v: u8) -> Result<Self, KoreError> {
        Ok(match v {
            0 => Transform::Plain, 1 => Transform::Delta, 2 => Transform::Varint,
            3 => Transform::Dict, 4 => Transform::HexFixed,
            _ => return Err(bad(format!("unknown transform {v}"))),
        })
    }
}

impl Stage {
    pub fn id(self) -> Option<u8> {
        match self {
            Stage::Auto => None, Stage::None => Some(0), Stage::Lz4 => Some(1), Stage::Hz => Some(2), Stage::Zstd => Some(3),
        }
    }
    pub fn from_id(v: u8) -> Result<Self, KoreError> {
        Ok(match v {
            0 => Stage::None, 1 => Stage::Lz4, 2 => Stage::Hz, 3 => Stage::Zstd,
            _ => return Err(bad(format!("unknown stage {v}"))),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnCodec {
    pub transform: Transform,
    pub stage: Stage,
}

impl ColumnCodec {
    pub const AUTO: ColumnCodec = ColumnCodec { transform: Transform::Auto, stage: Stage::Auto };
    pub const PLAIN: ColumnCodec = ColumnCodec { transform: Transform::Plain, stage: Stage::None };
    pub fn new(transform: Transform, stage: Stage) -> Self { ColumnCodec { transform, stage } }
}

/// Writer options for the version-3 container. Columns not listed use `default`.
#[derive(Debug, Clone)]
pub struct WriteOptions {
    pub default: ColumnCodec,
    pub columns: Vec<(String, ColumnCodec)>,
}

impl Default for WriteOptions {
    fn default() -> Self { WriteOptions::auto() }
}

impl WriteOptions {
    /// Every column chooses its own transform and stage.
    pub fn auto() -> Self { WriteOptions { default: ColumnCodec::AUTO, columns: vec![] } }
    /// Every column: `Plain` transform, no stage (pure layout, no compression).
    pub fn plain() -> Self { WriteOptions { default: ColumnCodec::PLAIN, columns: vec![] } }
    /// Override the codec of one column (later calls for the same name win).
    pub fn with_column(mut self, name: &str, codec: ColumnCodec) -> Self {
        self.columns.retain(|(n, _)| n != name);
        self.columns.push((name.to_string(), codec));
        self
    }
    pub fn codec_for(&self, name: &str) -> ColumnCodec {
        self.columns.iter().find(|(n, _)| n == name).map(|(_, c)| *c).unwrap_or(self.default)
    }
}

// ── varint / zigzag ──────────────────────────────────────────────────────────

pub(crate) fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 { out.push((v as u8) | 0x80); v >>= 7; }
    out.push(v as u8);
}

pub(crate) fn get_varint(d: &[u8], pos: &mut usize) -> Result<u64, KoreError> {
    let (mut v, mut shift) = (0u64, 0u32);
    loop {
        let b = *d.get(*pos).ok_or_else(|| bad("truncated varint"))?;
        *pos += 1;
        if shift == 63 && b > 1 { return Err(bad("varint overflow")); }
        v |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 { return Ok(v); }
        shift += 7;
        if shift > 63 { return Err(bad("varint overflow")); }
    }
}

fn zz(v: i64) -> u64 { ((v << 1) ^ (v >> 63)) as u64 }
fn unzz(v: u64) -> i64 { ((v >> 1) as i64) ^ -((v & 1) as i64) }

// ── null bitmap ──────────────────────────────────────────────────────────────

fn put_nulls(out: &mut Vec<u8>, n: usize, is_null: impl Fn(usize) -> bool) {
    if !(0..n).any(&is_null) { out.push(0); return; }
    out.push(1);
    let start = out.len();
    out.resize(start + n.div_ceil(8), 0);
    for i in 0..n { if is_null(i) { out[start + i / 8] |= 1 << (i % 8); } }
}

/// Returns the bitmap (None = no nulls) and the number of non-null rows.
fn get_nulls<'a>(d: &'a [u8], pos: &mut usize, n: usize) -> Result<(Option<&'a [u8]>, usize), KoreError> {
    let flag = *d.get(*pos).ok_or_else(|| bad("truncated null flag"))?;
    *pos += 1;
    match flag {
        0 => Ok((None, n)),
        1 => {
            let len = n.div_ceil(8);
            let end = pos.checked_add(len).filter(|&e| e <= d.len()).ok_or_else(|| bad("truncated null bitmap"))?;
            let bm = &d[*pos..end];
            *pos = end;
            if n % 8 != 0 && bm.last().map_or(false, |&b| b >> (n % 8) != 0) { return Err(bad("stray bits in null bitmap")); }
            let nulls: usize = bm.iter().map(|b| b.count_ones() as usize).sum();
            Ok((Some(bm), n - nulls))
        }
        _ => Err(bad("bad null flag")),
    }
}

fn is_null(bm: Option<&[u8]>, i: usize) -> bool { bm.map_or(false, |b| b[i / 8] >> (i % 8) & 1 == 1) }

// ── transforms: encode ───────────────────────────────────────────────────────

pub fn dtype_of(data: &ColumnData) -> crate::DType {
    match data {
        ColumnData::Int64(_) => crate::DType::I64, ColumnData::Float64(_) => crate::DType::F64,
        ColumnData::Bool(_) => crate::DType::Bool, ColumnData::Str(_) => crate::DType::Str,
        ColumnData::StrDict { .. } => crate::DType::StrDict,
    }
}

fn valid_for(data: &ColumnData, t: Transform) -> bool {
    use Transform::*;
    match data {
        ColumnData::Int64(_) => matches!(t, Plain | Delta | Varint),
        ColumnData::Float64(_) | ColumnData::Bool(_) | ColumnData::StrDict { .. } => matches!(t, Plain),
        ColumnData::Str(_) => matches!(t, Plain | Dict | HexFixed | TsText | Tokens),
    }
}

fn is_lower_hex(b: u8) -> bool { b.is_ascii_digit() || (b'a'..=b'f').contains(&b) }

/// `Some(width_in_chars)` if all non-null values are equal-length, even, non-empty lowercase hex.
pub(crate) fn hex_width(v: &[Option<String>]) -> Option<usize> {
    let mut w: Option<usize> = None;
    for s in v.iter().flatten() {
        let b = s.as_bytes();
        match w { None => w = Some(b.len()), Some(x) if x != b.len() => return None, _ => {} }
        if b.is_empty() || b.len() % 2 != 0 || !b.iter().all(|&c| is_lower_hex(c)) { return None; }
    }
    Some(w.unwrap_or(0))
}

fn hexval(c: u8) -> u8 { if c <= b'9' { c - b'0' } else { c - b'a' + 10 } }

fn dict_build(v: &[Option<String>], max_distinct: usize) -> Option<(Vec<&str>, Vec<u32>)> {
    let mut map: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    let mut dict: Vec<&str> = Vec::new();
    let mut codes = Vec::with_capacity(v.len());
    for o in v {
        match o {
            None => codes.push(0),
            Some(s) => {
                let c = match map.get(s.as_str()) {
                    Some(&c) => c,
                    None => {
                        if dict.len() >= max_distinct { return None; }
                        dict.push(s);
                        let c = dict.len() as u32;
                        map.insert(s, c);
                        c
                    }
                };
                codes.push(c);
            }
        }
    }
    Some((dict, codes))
}

/// Encode `data` with a concrete (non-Auto) transform.
pub fn encode_transform(data: &ColumnData, t: Transform) -> Result<Vec<u8>, KoreError> {
    if !valid_for(data, t) { return Err(bad(format!("transform {t:?} is not valid for {:?} columns", dtype_of(data)))); }
    let mut out = Vec::new();
    match (data, t) {
        (ColumnData::Int64(v), _) => {
            put_nulls(&mut out, v.len(), |i| v[i].is_none());
            let mut prev = 0i64;
            for x in v.iter().flatten() {
                match t {
                    Transform::Plain => out.extend_from_slice(&x.to_le_bytes()),
                    Transform::Varint => put_varint(&mut out, zz(*x)),
                    _ => { put_varint(&mut out, zz(x.wrapping_sub(prev))); prev = *x; }
                }
            }
        }
        (ColumnData::Float64(v), _) => {
            put_nulls(&mut out, v.len(), |i| v[i].is_none());
            for x in v.iter().flatten() { out.extend_from_slice(&x.to_bits().to_le_bytes()); }
        }
        (ColumnData::Bool(v), _) => {
            put_nulls(&mut out, v.len(), |i| v[i].is_none());
            let start = out.len();
            out.resize(start + v.len().div_ceil(8), 0);
            for (i, x) in v.iter().enumerate() { if *x == Some(true) { out[start + i / 8] |= 1 << (i % 8); } }
        }
        (ColumnData::StrDict { codes, dict }, _) => {
            if dict.len() > u16::MAX as usize || dict.iter().any(|s| s.len() > u16::MAX as usize) {
                return Err(bad("str_dict entry or dictionary too large"));
            }
            out = crate::compress::encode_strdict(codes, dict);
        }
        (ColumnData::Str(v), Transform::Plain) => {
            put_nulls(&mut out, v.len(), |i| v[i].is_none());
            for s in v.iter().flatten() { put_varint(&mut out, s.len() as u64); }
            for s in v.iter().flatten() { out.extend_from_slice(s.as_bytes()); }
        }
        (ColumnData::Str(v), Transform::Dict) => {
            let (dict, codes) = dict_build(v, u32::MAX as usize - 1).ok_or_else(|| bad("dictionary too large"))?;
            put_varint(&mut out, dict.len() as u64);
            for s in &dict { put_varint(&mut out, s.len() as u64); }
            for s in &dict { out.extend_from_slice(s.as_bytes()); }
            let width = if dict.len() < 256 { 1u8 } else if dict.len() < 65_536 { 2 } else { 4 };
            out.push(width);
            for c in codes { out.extend_from_slice(&c.to_le_bytes()[..width as usize]); }
        }
        (ColumnData::Str(v), Transform::HexFixed) => {
            let w = hex_width(v).ok_or_else(|| bad("HexFixed needs equal-length even lowercase hex strings"))?;
            if w > u16::MAX as usize { return Err(bad("HexFixed width too large")); }
            put_nulls(&mut out, v.len(), |i| v[i].is_none());
            out.extend_from_slice(&(w as u16).to_le_bytes());
            for s in v.iter().flatten() {
                for pair in s.as_bytes().chunks(2) { out.push(hexval(pair[0]) << 4 | hexval(pair[1])); }
            }
        }
        (ColumnData::Str(v), Transform::TsText) => {
            put_nulls(&mut out, v.len(), |i| v[i].is_none());
            let mut prev = 0i64;
            for s in v.iter().flatten() {
                let x = parse_ts(s).ok_or_else(|| bad("TsText needs canonical YYYY-MM-DDTHH:MM:SSZ timestamps"))?;
                put_varint(&mut out, zz(x.wrapping_sub(prev)));
                prev = x;
            }
        }
        (ColumnData::Str(v), Transform::Tokens) => {
            let mut map: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
            let mut dict: Vec<&str> = Vec::new();
            for s in v.iter().flatten() {
                if s.is_empty() { continue; }
                for t in s.split(',') {
                    if !map.contains_key(t) {
                        map.insert(t, dict.len() as u32);
                        dict.push(t);
                    }
                }
            }
            put_nulls(&mut out, v.len(), |i| v[i].is_none());
            put_varint(&mut out, dict.len() as u64);
            for t in &dict { put_varint(&mut out, t.len() as u64); }
            for t in &dict { out.extend_from_slice(t.as_bytes()); }
            for s in v.iter().flatten() {
                if s.is_empty() { put_varint(&mut out, 0); continue; }
                put_varint(&mut out, s.split(',').count() as u64);
                for t in s.split(',') { put_varint(&mut out, map[t] as u64); }
            }
        }
        _ => unreachable!("validated above"),
    }
    Ok(out)
}

// -- canonical timestamps (TsText) --------------------------------------------

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYY-MM-DDTHH:MM:SSZ` of epoch seconds; None outside years 0000-9999.
pub(crate) fn fmt_ts(secs: i64) -> Option<String> {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days.checked_add(719_468)?;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 { y += 1; }
    if !(0..=9999).contains(&y) { return None; }
    let mut o = Vec::with_capacity(20);
    let two = |v: i64, o: &mut Vec<u8>| { o.push(b'0' + (v / 10) as u8); o.push(b'0' + (v % 10) as u8); };
    two(y / 100, &mut o); two(y % 100, &mut o); o.push(b'-');
    two(m, &mut o); o.push(b'-'); two(d, &mut o); o.push(b'T');
    two(rem / 3600, &mut o); o.push(b':'); two(rem % 3600 / 60, &mut o); o.push(b':'); two(rem % 60, &mut o); o.push(b'Z');
    Some(String::from_utf8(o).expect("ascii"))
}

/// Epoch seconds if `s` is exactly the canonical form (checked by formatting it back).
pub(crate) fn parse_ts(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || b[19] != b'Z' { return None; }
    let num = |a: usize, z: usize| -> Option<i64> {
        let mut v = 0i64;
        for &c in &b[a..z] { if !c.is_ascii_digit() { return None; } v = v * 10 + (c - b'0') as i64; }
        Some(v)
    };
    let (y, mo, d, h, mi, se) = (num(0, 4)?, num(5, 7)?, num(8, 10)?, num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 59 { return None; }
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se;
    if fmt_ts(secs).as_deref() == Some(s) { Some(secs) } else { None }
}

// ── transforms: decode ───────────────────────────────────────────────────────

fn expect_end(d: &[u8], pos: usize) -> Result<(), KoreError> {
    if pos == d.len() { Ok(()) } else { Err(bad("trailing bytes in column payload")) }
}

/// Decode a payload produced by `encode_transform`. Strict: malformed input is an error, never a panic.
pub fn decode_transform(d: &[u8], dtype: crate::DType, t: Transform, n: usize) -> Result<ColumnData, KoreError> {
    use crate::DType;
    let mut pos = 0usize;
    match (dtype, t) {
        (DType::I64, Transform::Plain | Transform::Delta | Transform::Varint) => {
            let (bm, nn) = get_nulls(d, &mut pos, n)?;
            let mut out = Vec::with_capacity(n);
            let mut prev = 0i64;
            if t == Transform::Plain {
                let need = nn.checked_mul(8).ok_or_else(|| bad("overflow"))?;
                if d.len() - pos != need { return Err(bad("i64 payload length mismatch")); }
            }
            for i in 0..n {
                if is_null(bm, i) { out.push(None); continue; }
                let x = match t {
                    Transform::Plain => { let v = i64::from_le_bytes(d[pos..pos + 8].try_into().unwrap()); pos += 8; v }
                    Transform::Varint => unzz(get_varint(d, &mut pos)?),
                    _ => { prev = prev.wrapping_add(unzz(get_varint(d, &mut pos)?)); prev }
                };
                out.push(Some(x));
            }
            expect_end(d, pos)?;
            Ok(ColumnData::Int64(out))
        }
        (DType::F64, Transform::Plain) => {
            let (bm, nn) = get_nulls(d, &mut pos, n)?;
            if d.len() - pos != nn.checked_mul(8).ok_or_else(|| bad("overflow"))? { return Err(bad("f64 payload length mismatch")); }
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                if is_null(bm, i) { out.push(None); continue; }
                out.push(Some(f64::from_bits(u64::from_le_bytes(d[pos..pos + 8].try_into().unwrap()))));
                pos += 8;
            }
            Ok(ColumnData::Float64(out))
        }
        (DType::Bool, Transform::Plain) => {
            let (bm, _) = get_nulls(d, &mut pos, n)?;
            if d.len() - pos != n.div_ceil(8) { return Err(bad("bool payload length mismatch")); }
            let bits = &d[pos..];
            let out = (0..n).map(|i| if is_null(bm, i) { None } else { Some(bits[i / 8] >> (i % 8) & 1 == 1) }).collect();
            Ok(ColumnData::Bool(out))
        }
        (DType::StrDict, Transform::Plain) => {
            let (codes, dict) = crate::compress::decode_strdict(d, n);
            if codes.len() != n { return Err(bad("str_dict payload malformed")); }
            Ok(ColumnData::StrDict { codes, dict })
        }
        (DType::Str, Transform::Plain) => {
            let (bm, nn) = get_nulls(d, &mut pos, n)?;
            let mut lens = Vec::with_capacity(nn);
            let mut total = 0usize;
            for _ in 0..nn {
                let l = usize::try_from(get_varint(d, &mut pos)?).map_err(|_| bad("length overflow"))?;
                total = total.checked_add(l).ok_or_else(|| bad("length overflow"))?;
                lens.push(l);
            }
            if d.len() - pos != total { return Err(bad("string bytes length mismatch")); }
            let mut out = Vec::with_capacity(n);
            let mut li = 0;
            for i in 0..n {
                if is_null(bm, i) { out.push(None); continue; }
                let s = std::str::from_utf8(&d[pos..pos + lens[li]]).map_err(|_| bad("invalid UTF-8 in string column"))?;
                out.push(Some(s.to_string()));
                pos += lens[li];
                li += 1;
            }
            Ok(ColumnData::Str(out))
        }
        (DType::Str, Transform::Dict) => {
            let count = usize::try_from(get_varint(d, &mut pos)?).map_err(|_| bad("overflow"))?;
            if count > d.len() { return Err(bad("implausible dictionary size")); }
            let mut lens = Vec::with_capacity(count);
            let mut total = 0usize;
            for _ in 0..count {
                let l = usize::try_from(get_varint(d, &mut pos)?).map_err(|_| bad("overflow"))?;
                total = total.checked_add(l).ok_or_else(|| bad("overflow"))?;
                lens.push(l);
            }
            let end = pos.checked_add(total).filter(|&e| e <= d.len()).ok_or_else(|| bad("truncated dictionary"))?;
            let mut dict: Vec<String> = Vec::with_capacity(count);
            for l in lens {
                dict.push(std::str::from_utf8(&d[pos..pos + l]).map_err(|_| bad("invalid UTF-8 in dictionary"))?.to_string());
                pos += l;
            }
            debug_assert_eq!(pos, end);
            let width = *d.get(pos).ok_or_else(|| bad("truncated dictionary"))? as usize;
            pos += 1;
            if !matches!(width, 1 | 2 | 4) { return Err(bad("bad code width")); }
            if d.len() - pos != n.checked_mul(width).ok_or_else(|| bad("overflow"))? { return Err(bad("dictionary codes length mismatch")); }
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let mut b = [0u8; 4];
                b[..width].copy_from_slice(&d[pos + i * width..pos + (i + 1) * width]);
                let c = u32::from_le_bytes(b) as usize;
                if c == 0 { out.push(None); } else { out.push(Some(dict.get(c - 1).ok_or_else(|| bad("dictionary code out of range"))?.clone())); }
            }
            Ok(ColumnData::Str(out))
        }
        (DType::Str, Transform::HexFixed) => {
            let (bm, nn) = get_nulls(d, &mut pos, n)?;
            let w = u16::from_le_bytes(d.get(pos..pos + 2).ok_or_else(|| bad("truncated"))?.try_into().unwrap()) as usize;
            pos += 2;
            if w % 2 != 0 || (w == 0 && nn > 0) { return Err(bad("bad hex width")); }
            if d.len() - pos != nn.checked_mul(w / 2).ok_or_else(|| bad("overflow"))? { return Err(bad("hex payload length mismatch")); }
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                if is_null(bm, i) { out.push(None); continue; }
                let mut s = String::with_capacity(w);
                for &b in &d[pos..pos + w / 2] { s.push(HEX[(b >> 4) as usize] as char); s.push(HEX[(b & 15) as usize] as char); }
                pos += w / 2;
                out.push(Some(s));
            }
            Ok(ColumnData::Str(out))
        }
        (DType::Str, Transform::TsText) => {
            let (bm, _) = get_nulls(d, &mut pos, n)?;
            let mut out = Vec::with_capacity(n);
            let mut prev = 0i64;
            for i in 0..n {
                if is_null(bm, i) { out.push(None); continue; }
                prev = prev.wrapping_add(unzz(get_varint(d, &mut pos)?));
                out.push(Some(fmt_ts(prev).ok_or_else(|| bad("timestamp out of range"))?));
            }
            expect_end(d, pos)?;
            Ok(ColumnData::Str(out))
        }
        (DType::Str, Transform::Tokens) => {
            let (bm, _) = get_nulls(d, &mut pos, n)?;
            let count = usize::try_from(get_varint(d, &mut pos)?).map_err(|_| bad("overflow"))?;
            if count > d.len() { return Err(bad("implausible token dictionary size")); }
            let mut lens = Vec::with_capacity(count);
            let mut total = 0usize;
            for _ in 0..count {
                let l = usize::try_from(get_varint(d, &mut pos)?).map_err(|_| bad("overflow"))?;
                total = total.checked_add(l).ok_or_else(|| bad("overflow"))?;
                lens.push(l);
            }
            if pos.checked_add(total).map_or(true, |e| e > d.len()) { return Err(bad("truncated token dictionary")); }
            let mut dict: Vec<&str> = Vec::with_capacity(count);
            for l in lens {
                dict.push(std::str::from_utf8(&d[pos..pos + l]).map_err(|_| bad("invalid UTF-8 in token dictionary"))?);
                pos += l;
            }
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                if is_null(bm, i) { out.push(None); continue; }
                let k = usize::try_from(get_varint(d, &mut pos)?).map_err(|_| bad("overflow"))?;
                if k > d.len() - pos { return Err(bad("implausible token count")); }
                let mut s = String::new();
                for j in 0..k {
                    if j > 0 { s.push(','); }
                    let c = usize::try_from(get_varint(d, &mut pos)?).map_err(|_| bad("overflow"))?;
                    s.push_str(dict.get(c).ok_or_else(|| bad("token code out of range"))?);
                }
                // one empty token would decode to "", which is the zero-token encoding
                if k == 1 && s.is_empty() { return Err(bad("non-canonical token list")); }
                out.push(Some(s));
            }
            expect_end(d, pos)?;
            Ok(ColumnData::Str(out))
        }
        _ => Err(bad(format!("transform {t:?} is not valid for {dtype:?} columns"))),
    }
}

// ── transforms: decode a row range ───────────────────────────────────────────

/// First and last epoch second that `fmt_ts` can format (0000-01-01T00:00:00Z, 9999-12-31T23:59:59Z).
pub(crate) fn ts_min() -> i64 { days_from_civil(0, 1, 1) * 86_400 }
pub(crate) fn ts_max() -> i64 { days_from_civil(9999, 12, 31) * 86_400 + 86_399 }

/// Number of non-null rows before row `i`.
fn nonnull_before(bm: Option<&[u8]>, i: usize) -> usize {
    match bm {
        None => i,
        Some(b) => {
            let full: usize = b[..i / 8].iter().map(|x| x.count_ones() as usize).sum();
            let part = if i % 8 == 0 { 0 } else { (b[i / 8] & ((1u8 << (i % 8)) - 1)).count_ones() as usize };
            i - full - part
        }
    }
}

/// Rows `lo..hi` of a payload produced by `encode_transform`, without materialising the other rows.
///
/// Contract (tested against `decode_transform` on valid and on mutated payloads): the whole payload is
/// validated exactly as `decode_transform` validates it, so this returns `Ok` if and only if
/// `decode_transform(d, dtype, t, n)` returns `Ok`, and then the values are rows `lo..hi` of that result.
/// Only the work that allocates (the strings of the other rows) is skipped. Needs `lo <= hi <= n`.
pub fn decode_transform_rows(d: &[u8], dtype: crate::DType, t: Transform, n: usize, lo: usize, hi: usize) -> Result<ColumnData, KoreError> {
    use crate::DType;
    if lo > hi || hi > n { return Err(bad(format!("rows {lo}..{hi} out of range for {n} rows"))); }
    let mut pos = 0usize;
    let inside = |i: usize| i >= lo && i < hi;
    match (dtype, t) {
        (DType::I64, Transform::Plain) => {
            let (bm, nn) = get_nulls(d, &mut pos, n)?;
            let need = nn.checked_mul(8).ok_or_else(|| bad("overflow"))?;
            if d.len() - pos != need { return Err(bad("i64 payload length mismatch")); }
            let mut k = nonnull_before(bm, lo);
            let mut out = Vec::with_capacity(hi - lo);
            for i in lo..hi {
                if is_null(bm, i) { out.push(None); continue; }
                out.push(Some(i64::from_le_bytes(d[pos + 8 * k..pos + 8 * k + 8].try_into().unwrap())));
                k += 1;
            }
            Ok(ColumnData::Int64(out))
        }
        (DType::I64, Transform::Delta | Transform::Varint) => {
            let (bm, _) = get_nulls(d, &mut pos, n)?;
            let mut out = Vec::with_capacity(hi - lo);
            let mut prev = 0i64;
            for i in 0..n {
                if is_null(bm, i) { if inside(i) { out.push(None); } continue; }
                let x = if t == Transform::Varint { unzz(get_varint(d, &mut pos)?) } else { prev = prev.wrapping_add(unzz(get_varint(d, &mut pos)?)); prev };
                if inside(i) { out.push(Some(x)); }
            }
            expect_end(d, pos)?;
            Ok(ColumnData::Int64(out))
        }
        (DType::F64, Transform::Plain) => {
            let (bm, nn) = get_nulls(d, &mut pos, n)?;
            if d.len() - pos != nn.checked_mul(8).ok_or_else(|| bad("overflow"))? { return Err(bad("f64 payload length mismatch")); }
            let mut k = nonnull_before(bm, lo);
            let mut out = Vec::with_capacity(hi - lo);
            for i in lo..hi {
                if is_null(bm, i) { out.push(None); continue; }
                out.push(Some(f64::from_bits(u64::from_le_bytes(d[pos + 8 * k..pos + 8 * k + 8].try_into().unwrap()))));
                k += 1;
            }
            Ok(ColumnData::Float64(out))
        }
        (DType::Bool, Transform::Plain) => {
            let (bm, _) = get_nulls(d, &mut pos, n)?;
            if d.len() - pos != n.div_ceil(8) { return Err(bad("bool payload length mismatch")); }
            let bits = &d[pos..];
            Ok(ColumnData::Bool((lo..hi).map(|i| if is_null(bm, i) { None } else { Some(bits[i / 8] >> (i % 8) & 1 == 1) }).collect()))
        }
        (DType::StrDict, Transform::Plain) => {
            // legacy layout (not chosen by the version-3/4 writers for Str columns): decoded whole, then cut
            let (codes, dict) = crate::compress::decode_strdict(d, n);
            if codes.len() != n { return Err(bad("str_dict payload malformed")); }
            Ok(ColumnData::StrDict { codes: codes[lo..hi].to_vec(), dict })
        }
        (DType::Str, Transform::Plain) => {
            let (bm, nn) = get_nulls(d, &mut pos, n)?;
            let mut lens = Vec::with_capacity(nn);
            let mut total = 0usize;
            for _ in 0..nn {
                let l = usize::try_from(get_varint(d, &mut pos)?).map_err(|_| bad("length overflow"))?;
                total = total.checked_add(l).ok_or_else(|| bad("length overflow"))?;
                lens.push(l);
            }
            if d.len() - pos != total { return Err(bad("string bytes length mismatch")); }
            // every value is valid UTF-8 <=> the concatenation is valid UTF-8 and every value starts on a
            // character boundary of it
            let body = std::str::from_utf8(&d[pos..]).map_err(|_| bad("invalid UTF-8 in string column"))?;
            let first = nonnull_before(bm, lo);
            let (mut off, mut start) = (0usize, 0usize);
            for (k, l) in lens.iter().enumerate() {
                if k == first { start = off; }
                if !body.is_char_boundary(off) { return Err(bad("invalid UTF-8 in string column")); }
                off += l;
            }
            if first >= lens.len() { start = off; }
            let mut out = Vec::with_capacity(hi - lo);
            let (mut k, mut off) = (first, start);
            for i in lo..hi {
                if is_null(bm, i) { out.push(None); continue; }
                out.push(Some(body[off..off + lens[k]].to_string()));
                off += lens[k];
                k += 1;
            }
            Ok(ColumnData::Str(out))
        }
        (DType::Str, Transform::Dict) => {
            let count = usize::try_from(get_varint(d, &mut pos)?).map_err(|_| bad("overflow"))?;
            if count > d.len() { return Err(bad("implausible dictionary size")); }
            let mut lens = Vec::with_capacity(count);
            let mut total = 0usize;
            for _ in 0..count {
                let l = usize::try_from(get_varint(d, &mut pos)?).map_err(|_| bad("overflow"))?;
                total = total.checked_add(l).ok_or_else(|| bad("overflow"))?;
                lens.push(l);
            }
            pos.checked_add(total).filter(|&e| e <= d.len()).ok_or_else(|| bad("truncated dictionary"))?;
            let mut dict: Vec<&str> = Vec::with_capacity(count);
            for l in lens {
                dict.push(std::str::from_utf8(&d[pos..pos + l]).map_err(|_| bad("invalid UTF-8 in dictionary"))?);
                pos += l;
            }
            let width = *d.get(pos).ok_or_else(|| bad("truncated dictionary"))? as usize;
            pos += 1;
            if !matches!(width, 1 | 2 | 4) { return Err(bad("bad code width")); }
            if d.len() - pos != n.checked_mul(width).ok_or_else(|| bad("overflow"))? { return Err(bad("dictionary codes length mismatch")); }
            let codes = &d[pos..];
            let code = |i: usize| { let mut b = [0u8; 4]; b[..width].copy_from_slice(&codes[i * width..(i + 1) * width]); u32::from_le_bytes(b) as usize };
            // every code is range-checked (as by the full decoder); only the range is materialised
            if (0..n).any(|i| code(i) > dict.len()) { return Err(bad("dictionary code out of range")); }
            Ok(ColumnData::Str((lo..hi).map(|i| match code(i) { 0 => None, c => Some(dict[c - 1].to_string()) }).collect()))
        }
        (DType::Str, Transform::HexFixed) => {
            let (bm, nn) = get_nulls(d, &mut pos, n)?;
            let w = u16::from_le_bytes(d.get(pos..pos + 2).ok_or_else(|| bad("truncated"))?.try_into().unwrap()) as usize;
            pos += 2;
            if w % 2 != 0 || (w == 0 && nn > 0) { return Err(bad("bad hex width")); }
            if d.len() - pos != nn.checked_mul(w / 2).ok_or_else(|| bad("overflow"))? { return Err(bad("hex payload length mismatch")); }
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut k = nonnull_before(bm, lo);
            let mut out = Vec::with_capacity(hi - lo);
            for i in lo..hi {
                if is_null(bm, i) { out.push(None); continue; }
                let at = pos + k * (w / 2);
                let mut s = String::with_capacity(w);
                for &b in &d[at..at + w / 2] { s.push(HEX[(b >> 4) as usize] as char); s.push(HEX[(b & 15) as usize] as char); }
                out.push(Some(s));
                k += 1;
            }
            Ok(ColumnData::Str(out))
        }
        (DType::Str, Transform::TsText) => {
            let (bm, _) = get_nulls(d, &mut pos, n)?;
            let (min, max) = (ts_min(), ts_max());
            let mut out = Vec::with_capacity(hi - lo);
            let mut prev = 0i64;
            for i in 0..n {
                if is_null(bm, i) { if inside(i) { out.push(None); } continue; }
                prev = prev.wrapping_add(unzz(get_varint(d, &mut pos)?));
                // exactly the values `fmt_ts` formats (tested at both ends)
                if prev < min || prev > max { return Err(bad("timestamp out of range")); }
                if inside(i) { out.push(Some(fmt_ts(prev).ok_or_else(|| bad("timestamp out of range"))?)); }
            }
            expect_end(d, pos)?;
            Ok(ColumnData::Str(out))
        }
        (DType::Str, Transform::Tokens) => {
            let (bm, _) = get_nulls(d, &mut pos, n)?;
            let count = usize::try_from(get_varint(d, &mut pos)?).map_err(|_| bad("overflow"))?;
            if count > d.len() { return Err(bad("implausible token dictionary size")); }
            let mut lens = Vec::with_capacity(count);
            let mut total = 0usize;
            for _ in 0..count {
                let l = usize::try_from(get_varint(d, &mut pos)?).map_err(|_| bad("overflow"))?;
                total = total.checked_add(l).ok_or_else(|| bad("overflow"))?;
                lens.push(l);
            }
            if pos.checked_add(total).map_or(true, |e| e > d.len()) { return Err(bad("truncated token dictionary")); }
            let mut dict: Vec<&str> = Vec::with_capacity(count);
            for l in lens {
                dict.push(std::str::from_utf8(&d[pos..pos + l]).map_err(|_| bad("invalid UTF-8 in token dictionary"))?);
                pos += l;
            }
            let mut out = Vec::with_capacity(hi - lo);
            for i in 0..n {
                if is_null(bm, i) { if inside(i) { out.push(None); } continue; }
                let keep = inside(i);
                let k = usize::try_from(get_varint(d, &mut pos)?).map_err(|_| bad("overflow"))?;
                if k > d.len() - pos { return Err(bad("implausible token count")); }
                let mut s = String::new();
                let mut one_empty = false;
                for j in 0..k {
                    let c = usize::try_from(get_varint(d, &mut pos)?).map_err(|_| bad("overflow"))?;
                    let tok = dict.get(c).ok_or_else(|| bad("token code out of range"))?;
                    one_empty = tok.is_empty();
                    if keep { if j > 0 { s.push(','); } s.push_str(tok); }
                }
                // one empty token would decode to "", which is the zero-token encoding
                if k == 1 && one_empty { return Err(bad("non-canonical token list")); }
                if keep { out.push(Some(s)); }
            }
            expect_end(d, pos)?;
            Ok(ColumnData::Str(out))
        }
        _ => Err(bad(format!("transform {t:?} is not valid for {dtype:?} columns"))),
    }
}

// ── stages ───────────────────────────────────────────────────────────────────

/// Apply a concrete stage. Stage payload: `varint(raw_len) | body`; `None` stores the bytes unframed.
pub fn encode_stage(raw: &[u8], s: Stage) -> Vec<u8> {
    let mut out = Vec::new();
    match s {
        Stage::None | Stage::Auto => return raw.to_vec(),
        Stage::Lz4 => { put_varint(&mut out, raw.len() as u64); out.extend_from_slice(&lz4_flex::block::compress(raw)); }
        Stage::Hz => { put_varint(&mut out, raw.len() as u64); out.extend_from_slice(&hz::compress_segmented(raw)); }
        Stage::Zstd => { put_varint(&mut out, raw.len() as u64); out.extend_from_slice(&zstd::bulk::compress(raw, 3).expect("zstd in-memory compress")); }
    }
    out
}

pub fn decode_stage(payload: &[u8], s: Stage) -> Result<std::borrow::Cow<'_, [u8]>, KoreError> {
    use std::borrow::Cow;
    if matches!(s, Stage::None | Stage::Auto) { return Ok(Cow::Borrowed(payload)); }
    let mut pos = 0;
    let raw_len = usize::try_from(get_varint(payload, &mut pos)?).map_err(|_| bad("overflow"))?;
    if raw_len > MAX_RAW { return Err(bad("implausible decoded size")); }
    let body = &payload[pos..];
    let out = match s {
        Stage::Lz4 => {
            if raw_len > body.len().saturating_mul(256).saturating_add(1024) { return Err(bad("LZ4 claims implausible decoded size")); }
            lz4_flex::block::decompress(body, raw_len).map_err(|e| bad(format!("LZ4: {e}")))?
        }
        Stage::Hz => hz::decompress_segmented(body, raw_len).map_err(bad)?,
        Stage::Zstd => zstd::bulk::decompress(body, raw_len).map_err(|e| bad(format!("ZSTD: {e}")))?,
        _ => unreachable!(),
    };
    if out.len() != raw_len { return Err(bad("decoded stage length mismatch")); }
    Ok(Cow::Owned(out))
}

// ── column choice ────────────────────────────────────────────────────────────

fn candidate_transforms(data: &ColumnData) -> Vec<Transform> {
    match data {
        ColumnData::Int64(_) => vec![Transform::Plain, Transform::Delta, Transform::Varint],
        ColumnData::Str(v) => {
            let mut c = vec![Transform::Plain];
            // dictionary only pays off when values repeat; probe with a cap so text columns bail out early
            if v.len() >= 2 && dict_build(v, v.len() / 2).is_some() { c.push(Transform::Dict); }
            if hex_width(v).map_or(false, |w| w > 0) { c.push(Transform::HexFixed); }
            c
        }
        _ => vec![Transform::Plain],
    }
}

/// Encode one column: returns `(transform id, stage id, payload)`.
pub fn encode_column(data: &ColumnData, codec: ColumnCodec) -> Result<(u8, u8, Vec<u8>), KoreError> {
    use rayon::prelude::*;
    if codec.transform.v4_only() {
        return Err(bad(format!("transform {:?} is only available in version-4 files (KoreWriter::to_bytes_compact)", codec.transform)));
    }
    let transforms = match codec.transform {
        Transform::Auto => candidate_transforms(data),
        t => vec![t],
    };
    let stages: Vec<Stage> = match codec.stage {
        Stage::Auto => vec![Stage::None, Stage::Lz4, Stage::Hz, Stage::Zstd],
        s => vec![s],
    };
    let mut best: Option<(u8, u8, Vec<u8>)> = None;
    for t in transforms {
        let raw = encode_transform(data, t)?;
        // the unstaged candidate is compared by length only (no copy of a possibly huge payload)
        let staged: Vec<(Stage, Option<Vec<u8>>)> = stages.par_iter()
            .map(|&s| (s, if s == Stage::None { None } else { Some(encode_stage(&raw, s)) })).collect();
        let size_of = |c: &(Stage, Option<Vec<u8>>)| c.1.as_ref().map_or(raw.len(), |p| p.len());
        let mut win = 0;
        for i in 1..staged.len() { if size_of(&staged[i]) < size_of(&staged[win]) { win = i; } }
        if best.as_ref().map_or(true, |b| size_of(&staged[win]) < b.2.len()) {
            let (s, p) = staged.into_iter().nth(win).unwrap();
            best = Some((t.id().unwrap(), s.id().unwrap(), p.unwrap_or(raw)));
        }
    }
    best.ok_or_else(|| bad("no codec candidate"))
}

/// Decode one column payload.
pub fn decode_column(payload: &[u8], dtype: crate::DType, transform: u8, stage: u8, n: usize) -> Result<ColumnData, KoreError> {
    let t = Transform::from_id(transform)?;
    let s = Stage::from_id(stage)?;
    let raw = decode_stage(payload, s)?;
    decode_transform(&raw, dtype, t, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip_and_overflow() {
        for v in [0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
            let mut b = vec![]; put_varint(&mut b, v);
            let mut p = 0; assert_eq!(get_varint(&b, &mut p).unwrap(), v); assert_eq!(p, b.len());
        }
        let mut p = 0; assert!(get_varint(&[0xff; 11], &mut p).is_err());
        for v in [i64::MIN, -1, 0, 1, i64::MAX] { assert_eq!(unzz(zz(v)), v); }
    }

    #[test]
    fn ts_range_is_exactly_what_fmt_ts_formats() {
        let (lo, hi) = (ts_min(), ts_max());
        assert_eq!(fmt_ts(lo).as_deref(), Some("0000-01-01T00:00:00Z"));
        assert_eq!(fmt_ts(hi).as_deref(), Some("9999-12-31T23:59:59Z"));
        assert_eq!(fmt_ts(lo - 1), None);
        assert_eq!(fmt_ts(hi + 1), None);
        for x in [i64::MIN, i64::MIN + 1, i64::MIN / 2, -1 << 40, 1 << 40, i64::MAX / 2, i64::MAX - 1, i64::MAX] {
            assert_eq!(fmt_ts(x).is_some(), (lo..=hi).contains(&x), "{x}");
        }
        // monotonic year: every value inside the range formats
        let mut s = lo;
        while s < hi { assert!(fmt_ts(s).is_some()); s += 86_400 * 37 + 1234; }
    }

    #[test]
    fn empty_columns() {
        for d in [ColumnData::Int64(vec![]), ColumnData::Str(vec![]), ColumnData::Float64(vec![]), ColumnData::Bool(vec![])] {
            let (t, s, p) = encode_column(&d, ColumnCodec::AUTO).unwrap();
            let back = decode_column(&p, dtype_of(&d), t, s, 0).unwrap();
            assert_eq!(back.len(), 0);
        }
    }
}
