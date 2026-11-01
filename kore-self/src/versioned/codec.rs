//! v2 segment encoding (log format 2). Implemented entirely in kore-self; the
//! `.kore` container layout (magic, header, schema, per-column blocks) is the
//! same as v1 but the header carries version 3 and every column block uses a
//! column codec chosen per column and per segment:
//!
//! * hex ids (all 64 lowercase hex chars) -> 32 raw bytes,
//! * canonical timestamps `YYYY-MM-DDTHH:MM:SSZ` -> zigzag varint deltas of epoch seconds,
//! * low-cardinality strings -> dictionary + bit-packed codes,
//! * `tags` (comma lists) -> token dictionary + per-row code lists,
//! * anything else -> length-prefixed bytes,
//! * ints -> zigzag varint deltas; floats -> raw 8 bytes; bools -> bits,
//! * optionally the `hz` entropy stage (LZ77 + Huffman) over the column body,
//!   used only when it is smaller.
//!
//! Every codec is lossless for arbitrary input (a column that does not fit a
//! specialised codec falls back to RAW), and encoding is deterministic.
//! v1 readers reject these files (header version 3), they never misread them.

use super::hz;
use super::model::{hex, Cell, Table, Ty};
use std::collections::HashMap;

pub const SEG_VERSION: u16 = 3;
const MAGIC: &[u8; 4] = b"KORE";

const M_RAW: u8 = 1;
const M_DICT: u8 = 2;
const M_HEX32: u8 = 3;
const M_TS: u8 = 4;
const M_TAGS: u8 = 5;
const M_INT: u8 = 6;
const M_F64: u8 = 7;
const M_BOOL: u8 = 8;

const HZ_FLAG: u8 = 0x80;
const V2_FLAG: u8 = 0x10;

// ── primitives ───────────────────────────────────────────────────────────────

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}
fn zz(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}
fn unzz(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}
impl<'a> Rd<'a> {
    fn new(b: &'a [u8]) -> Self {
        Rd { b, p: 0 }
    }
    fn left(&self) -> usize {
        self.b.len() - self.p
    }
    fn u8(&mut self) -> Result<u8, String> {
        let v = *self.b.get(self.p).ok_or("codec: truncated")?;
        self.p += 1;
        Ok(v)
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if n > self.left() {
            return Err("codec: truncated".into());
        }
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn varint(&mut self) -> Result<u64, String> {
        let (mut v, mut sh) = (0u64, 0u32);
        loop {
            let b = self.u8()?;
            if sh > 63 {
                return Err("codec: bad varint".into());
            }
            v |= ((b & 0x7f) as u64) << sh;
            if b & 0x80 == 0 {
                return Ok(v);
            }
            sh += 7;
        }
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
}

fn pack_bits(out: &mut Vec<u8>, vals: &[u32], bits: u32) {
    if bits == 0 {
        return;
    }
    let (mut acc, mut n) = (0u64, 0u32);
    for &v in vals {
        acc |= (v as u64) << n;
        n += bits;
        while n >= 8 {
            out.push(acc as u8);
            acc >>= 8;
            n -= 8;
        }
    }
    if n > 0 {
        out.push(acc as u8);
    }
}

fn unpack_bits(r: &mut Rd, count: usize, bits: u32) -> Result<Vec<u32>, String> {
    if bits > 32 {
        return Err("codec: bad bit width".into());
    }
    if bits == 0 {
        return Ok(vec![0; count]);
    }
    let nbytes = (count * bits as usize).div_ceil(8);
    let data = r.take(nbytes)?;
    let mut out = Vec::with_capacity(count);
    let (mut acc, mut n, mut pos) = (0u64, 0u32, 0usize);
    let mask = if bits == 32 { u32::MAX as u64 } else { (1u64 << bits) - 1 };
    for _ in 0..count {
        while n < bits {
            acc |= (data[pos] as u64) << n;
            pos += 1;
            n += 8;
        }
        out.push((acc & mask) as u32);
        acc >>= bits;
        n -= bits;
    }
    Ok(out)
}

fn bits_for(n: usize) -> u32 {
    if n <= 1 {
        0
    } else {
        usize::BITS - (n - 1).leading_zeros()
    }
}

// ── timestamps ───────────────────────────────────────────────────────────────

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn fmt_ts(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    if (0..=9999).contains(&y) {
        // canonical form without the formatting machinery (this runs once per timestamp cell on decode)
        let (h, mi, se) = (rem / 3600, rem % 3600 / 60, rem % 60);
        let two = |v: i64, o: &mut Vec<u8>| {
            o.push(b'0' + (v / 10) as u8);
            o.push(b'0' + (v % 10) as u8);
        };
        let mut o = Vec::with_capacity(20);
        o.push(b'0' + (y / 1000) as u8);
        o.push(b'0' + (y / 100 % 10) as u8);
        o.push(b'0' + (y / 10 % 10) as u8);
        o.push(b'0' + (y % 10) as u8);
        o.push(b'-');
        two(m, &mut o);
        o.push(b'-');
        two(d, &mut o);
        o.push(b'T');
        two(h, &mut o);
        o.push(b':');
        two(mi, &mut o);
        o.push(b':');
        two(se, &mut o);
        o.push(b'Z');
        return String::from_utf8(o).expect("ascii");
    }
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Epoch seconds if `s` is exactly the canonical form (verified by reformatting).
fn parse_ts(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || b[19] != b'Z' {
        return None;
    }
    let num = |a: usize, z: usize| -> Option<i64> {
        let mut v = 0i64;
        for &c in &b[a..z] {
            if !c.is_ascii_digit() {
                return None;
            }
            v = v * 10 + (c - b'0') as i64;
        }
        Some(v)
    };
    let (y, mo, d, h, mi, se) = (num(0, 4)?, num(5, 7)?, num(8, 10)?, num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 59 {
        return None;
    }
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se;
    if fmt_ts(secs) == s {
        Some(secs)
    } else {
        None
    }
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn hex_val(c: u8) -> u8 {
    if c <= b'9' {
        c - b'0'
    } else {
        c - b'a' + 10
    }
}

pub fn hex64_to_bytes(s: &str) -> Option<[u8; 32]> {
    if !is_hex64(s) {
        return None;
    }
    let b = s.as_bytes();
    let mut o = [0u8; 32];
    for i in 0..32 {
        o[i] = (hex_val(b[2 * i]) << 4) | hex_val(b[2 * i + 1]);
    }
    Some(o)
}

// ── string column codecs ─────────────────────────────────────────────────────

fn enc_raw(vals: &[&str]) -> Vec<u8> {
    let mut o = vec![M_RAW];
    for v in vals {
        put_varint(&mut o, v.len() as u64);
    }
    for v in vals {
        o.extend_from_slice(v.as_bytes());
    }
    o
}

fn write_dict(o: &mut Vec<u8>, dict: &[&str]) {
    put_varint(o, dict.len() as u64);
    for d in dict {
        put_varint(o, d.len() as u64);
    }
    for d in dict {
        o.extend_from_slice(d.as_bytes());
    }
}

fn read_dict(r: &mut Rd) -> Result<Vec<String>, String> {
    let nd = r.varint()? as usize;
    if nd > r.left() {
        return Err("codec: dict too large".into());
    }
    let mut lens = Vec::with_capacity(nd);
    for _ in 0..nd {
        lens.push(r.varint()? as usize);
    }
    let mut d = Vec::with_capacity(nd);
    for l in lens {
        d.push(String::from_utf8(r.take(l)?.to_vec()).map_err(|_| "codec: dict not utf-8")?);
    }
    Ok(d)
}

/// Distinct values in first-seen order, or None when more than `cap` distinct.
fn distinct<'a>(vals: &[&'a str], cap: usize) -> Option<(Vec<&'a str>, HashMap<&'a str, u32>)> {
    let mut map: HashMap<&str, u32> = HashMap::new();
    let mut dict: Vec<&str> = Vec::new();
    for v in vals {
        if !map.contains_key(v) {
            if dict.len() >= cap {
                return None;
            }
            map.insert(v, dict.len() as u32);
            dict.push(v);
        }
    }
    Some((dict, map))
}

fn enc_dict(vals: &[&str], dict: &[&str], map: &HashMap<&str, u32>) -> Vec<u8> {
    let mut o = vec![M_DICT];
    write_dict(&mut o, dict);
    let bits = bits_for(dict.len());
    o.push(bits as u8);
    let codes: Vec<u32> = vals.iter().map(|v| map[v]).collect();
    pack_bits(&mut o, &codes, bits);
    o
}

fn enc_tags(vals: &[&str]) -> Option<Vec<u8>> {
    let mut map: HashMap<&str, u32> = HashMap::new();
    let mut dict: Vec<&str> = Vec::new();
    for v in vals {
        if v.is_empty() {
            continue;
        }
        for t in v.split(',') {
            if !map.contains_key(t) {
                if dict.len() >= 4096 {
                    return None;
                }
                map.insert(t, dict.len() as u32);
                dict.push(t);
            }
        }
    }
    let mut o = vec![M_TAGS];
    write_dict(&mut o, &dict);
    for v in vals {
        if v.is_empty() {
            put_varint(&mut o, 0);
        } else {
            let toks: Vec<&str> = v.split(',').collect();
            put_varint(&mut o, toks.len() as u64);
            for t in toks {
                put_varint(&mut o, map[t] as u64);
            }
        }
    }
    Some(o)
}

fn enc_ts(secs: &[i64]) -> Vec<u8> {
    let mut o = vec![M_TS];
    let mut prev = 0i64;
    for &s in secs {
        put_varint(&mut o, zz(s.wrapping_sub(prev)));
        prev = s;
    }
    o
}

fn encode_strs_body(vals: &[&str], name: &str) -> Vec<u8> {
    if vals.is_empty() {
        return enc_raw(vals);
    }
    if vals.iter().all(|v| is_hex64(v)) {
        let mut o = vec![M_HEX32];
        for v in vals {
            o.extend_from_slice(&hex64_to_bytes(v).expect("checked"));
        }
        return o;
    }
    let mut best = enc_raw(vals);
    let mut consider = |c: Vec<u8>| {
        if c.len() < best.len() {
            best = c;
        }
    };
    if let Some((dict, map)) = distinct(vals, vals.len() / 2 + 16) {
        consider(enc_dict(vals, &dict, &map));
    }
    if let Some(ts) = vals.iter().map(|v| parse_ts(v)).collect::<Option<Vec<i64>>>() {
        consider(enc_ts(&ts));
    }
    if name == "tags" {
        if let Some(t) = enc_tags(vals) {
            consider(t);
        }
    }
    best
}

enum StrCol {
    Hex(Vec<[u8; 32]>),
    Strs(Vec<String>),
}

fn decode_strs_body(r: &mut Rd, m: usize) -> Result<StrCol, String> {
    let mode = r.u8()?;
    match mode {
        M_RAW => {
            if m > r.left() {
                return Err("codec: bad count".into());
            }
            let mut lens = Vec::with_capacity(m);
            for _ in 0..m {
                lens.push(r.varint()? as usize);
            }
            let mut out = Vec::with_capacity(m);
            for l in lens {
                out.push(String::from_utf8(r.take(l)?.to_vec()).map_err(|_| "codec: not utf-8")?);
            }
            Ok(StrCol::Strs(out))
        }
        M_DICT => {
            let dict = read_dict(r)?;
            let bits = r.u8()? as u32;
            let codes = unpack_bits(r, m, bits)?;
            let mut out = Vec::with_capacity(m);
            for c in codes {
                out.push(dict.get(c as usize).ok_or("codec: dict code out of range")?.clone());
            }
            Ok(StrCol::Strs(out))
        }
        M_HEX32 => {
            let raw = r.take(m.checked_mul(32).ok_or("codec: overflow")?)?;
            Ok(StrCol::Hex(raw.chunks_exact(32).map(|c| <[u8; 32]>::try_from(c).unwrap()).collect()))
        }
        M_TS => {
            let mut out = Vec::with_capacity(m.min(r.left()));
            let mut prev = 0i64;
            for _ in 0..m {
                prev = prev.wrapping_add(unzz(r.varint()?));
                out.push(fmt_ts(prev));
            }
            Ok(StrCol::Strs(out))
        }
        M_TAGS => {
            let dict = read_dict(r)?;
            let mut out = Vec::with_capacity(m.min(r.left()));
            for _ in 0..m {
                let c = r.varint()? as usize;
                if c > r.left() {
                    return Err("codec: bad tag count".into());
                }
                let mut s = String::new();
                for k in 0..c {
                    if k > 0 {
                        s.push(',');
                    }
                    s.push_str(dict.get(r.varint()? as usize).ok_or("codec: tag code out of range")?);
                }
                out.push(s);
            }
            Ok(StrCol::Strs(out))
        }
        x => Err(format!("codec: unknown string mode {x}")),
    }
}

// ── columns ──────────────────────────────────────────────────────────────────

fn nulls_section(o: &mut Vec<u8>, isnull: &[bool]) {
    if isnull.iter().any(|&b| b) {
        o.push(1);
        let mut bm = vec![0u8; isnull.len().div_ceil(8)];
        for (i, &b) in isnull.iter().enumerate() {
            if b {
                bm[i / 8] |= 1 << (i % 8);
            }
        }
        o.extend(bm);
    } else {
        o.push(0);
    }
}

fn read_nulls(r: &mut Rd, n: usize) -> Result<Vec<bool>, String> {
    match r.u8()? {
        0 => Ok(vec![false; n]),
        1 => {
            let bm = r.take(n.div_ceil(8))?;
            Ok((0..n).map(|i| bm[i / 8] >> (i % 8) & 1 == 1).collect())
        }
        _ => Err("codec: bad null flag".into()),
    }
}

fn wrap(body: Vec<u8>, try_hz: bool) -> (u8, Vec<u8>) {
    if try_hz && body.len() >= 64 {
        let z = hz::compress(&body);
        if z.len() < body.len() {
            return (V2_FLAG | HZ_FLAG, z);
        }
    }
    (V2_FLAG, body)
}

fn encode_col(name: &str, ty: Ty, col: usize, rows: &[Vec<Cell>]) -> (u8, Vec<u8>) {
    let n = rows.len();
    let mut body = Vec::new();
    let mut try_hz = true;
    match ty {
        Ty::S => {
            let cells: Vec<Option<&str>> = rows.iter().map(|r| if let Cell::S(s) = &r[col] { Some(s.as_str()) } else { None }).collect();
            let isnull: Vec<bool> = cells.iter().map(|c| c.is_none()).collect();
            nulls_section(&mut body, &isnull);
            let vals: Vec<&str> = cells.into_iter().flatten().collect();
            let strs = encode_strs_body(&vals, name);
            // Distinct 32-byte ids (sha256) are incompressible: skip the entropy stage (it never pays, and costs time).
            if strs.first() == Some(&M_HEX32) {
                let mut seen = std::collections::HashSet::with_capacity(vals.len());
                if vals.iter().all(|v| seen.insert(*v)) {
                    try_hz = false;
                }
            }
            body.extend(strs);
        }
        Ty::I => {
            let cells: Vec<Option<i64>> = rows.iter().map(|r| if let Cell::I(x) = r[col] { Some(x) } else { None }).collect();
            nulls_section(&mut body, &cells.iter().map(|c| c.is_none()).collect::<Vec<_>>());
            body.push(M_INT);
            let mut prev = 0i64;
            for v in cells.into_iter().flatten() {
                put_varint(&mut body, zz(v.wrapping_sub(prev)));
                prev = v;
            }
        }
        Ty::F => {
            let cells: Vec<Option<f64>> = rows.iter().map(|r| if let Cell::F(x) = r[col] { Some(x) } else { None }).collect();
            nulls_section(&mut body, &cells.iter().map(|c| c.is_none()).collect::<Vec<_>>());
            body.push(M_F64);
            for v in cells.into_iter().flatten() {
                body.extend(v.to_bits().to_le_bytes());
            }
        }
        Ty::B => {
            let cells: Vec<Option<bool>> = rows.iter().map(|r| if let Cell::B(x) = r[col] { Some(x) } else { None }).collect();
            nulls_section(&mut body, &cells.iter().map(|c| c.is_none()).collect::<Vec<_>>());
            body.push(M_BOOL);
            let bits: Vec<u32> = cells.into_iter().flatten().map(|b| b as u32).collect();
            pack_bits(&mut body, &bits, 1);
        }
    }
    let _ = n;
    wrap(body, try_hz)
}

fn dtype_byte(t: Ty) -> u8 {
    match t {
        Ty::I => 1,
        Ty::F => 2,
        Ty::B => 3,
        Ty::S => 4,
    }
}

pub fn encode_segment(table: Table, rows: &[Vec<Cell>]) -> Vec<u8> {
    let spec = table.columns();
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend(SEG_VERSION.to_le_bytes());
    out.extend((spec.len() as u32).to_le_bytes());
    out.extend((rows.len() as u64).to_le_bytes());
    for (name, ty) in spec {
        out.extend((name.len() as u16).to_le_bytes());
        out.extend(name.as_bytes());
        out.push(dtype_byte(*ty));
    }
    // (Encoding the columns in parallel gave the same bytes but was slower under load: measured, not used.)
    for (i, (name, ty)) in spec.iter().enumerate() {
        let (comp, data) = encode_col(name, *ty, i, rows);
        out.push(comp);
        out.extend((data.len() as u64).to_le_bytes());
        out.extend(data);
    }
    out
}

/// True if these bytes look like a v2 segment (header version 3).
pub fn is_v2(bytes: &[u8]) -> bool {
    bytes.len() >= 6 && &bytes[0..4] == MAGIC && u16::from_le_bytes([bytes[4], bytes[5]]) == SEG_VERSION
}

struct Header<'a> {
    nrows: usize,
    body: Rd<'a>,
}

fn read_header<'a>(table: Table, bytes: &'a [u8]) -> Result<Header<'a>, String> {
    let mut r = Rd::new(bytes);
    if r.take(4)? != MAGIC {
        return Err("invalid KORE magic bytes".into());
    }
    if r.u16()? != SEG_VERSION {
        return Err("not a v2 segment".into());
    }
    let ncols = r.u32()? as usize;
    let nrows = r.u64()? as usize;
    let spec = table.columns();
    if ncols != spec.len() {
        return Err(format!("{}: expected {} columns, found {ncols}", table.name(), spec.len()));
    }
    for (name, ty) in spec {
        let l = r.u16()? as usize;
        let nm = r.take(l)?;
        if nm != name.as_bytes() {
            return Err(format!("{}: expected column '{name}'", table.name()));
        }
        if r.u8()? != dtype_byte(*ty) {
            return Err(format!("{}: column '{name}' has the wrong type", table.name()));
        }
    }
    if nrows > bytes.len().saturating_mul(64) + 64 {
        return Err("codec: implausible row count".into());
    }
    Ok(Header { nrows, body: r })
}

fn next_block<'a>(r: &mut Rd<'a>) -> Result<(u8, &'a [u8]), String> {
    let comp = r.u8()?;
    let len = r.u64()? as usize;
    Ok((comp, r.take(len)?))
}

fn unwrap_block(comp: u8, data: &[u8]) -> Result<std::borrow::Cow<'_, [u8]>, String> {
    if comp & V2_FLAG == 0 || comp & !(V2_FLAG | HZ_FLAG) != 0 {
        return Err(format!("codec: unknown column wrapper {comp:#x}"));
    }
    if comp & HZ_FLAG != 0 {
        Ok(std::borrow::Cow::Owned(hz::decompress(data)?))
    } else {
        Ok(std::borrow::Cow::Borrowed(data))
    }
}

/// Turn decoded non-null values into one cell per row (nulls become `Cell::Null`).
fn fill_cells(isnull: &[bool], vals: impl ExactSizeIterator<Item = Cell>, any_null: bool) -> Result<Vec<Cell>, String> {
    let m = vals.len();
    if !any_null {
        if m != isnull.len() {
            return Err("codec: value count mismatch".into());
        }
        return Ok(vals.collect());
    }
    let mut it = vals;
    let mut out = Vec::with_capacity(isnull.len());
    for &nl in isnull {
        out.push(if nl { Cell::Null } else { it.next().ok_or("codec: too few values")? });
    }
    if it.next().is_some() {
        return Err("codec: too many values".into());
    }
    Ok(out)
}

fn decode_col(ty: Ty, n: usize, comp: u8, data: &[u8]) -> Result<Vec<Cell>, String> {
    let raw = unwrap_block(comp, data)?;
    let mut r = Rd::new(&raw);
    let isnull = read_nulls(&mut r, n)?;
    let any_null = isnull.iter().any(|&b| b);
    let m = if any_null { isnull.iter().filter(|&&b| !b).count() } else { n };
    match ty {
        Ty::S => match decode_strs_body(&mut r, m)? {
            StrCol::Hex(h) => fill_cells(&isnull, h.iter().map(|b| Cell::S(hex(b))), any_null),
            StrCol::Strs(s) => fill_cells(&isnull, s.into_iter().map(Cell::S), any_null),
        },
        Ty::I => {
            if r.u8()? != M_INT {
                return Err("codec: bad int mode".into());
            }
            let mut v = Vec::with_capacity(m.min(r.left()));
            let mut prev = 0i64;
            for _ in 0..m {
                prev = prev.wrapping_add(unzz(r.varint()?));
                v.push(Cell::I(prev));
            }
            fill_cells(&isnull, v.into_iter(), any_null)
        }
        Ty::F => {
            if r.u8()? != M_F64 {
                return Err("codec: bad float mode".into());
            }
            let raw = r.take(m.checked_mul(8).ok_or("codec: overflow")?)?;
            let v: Vec<Cell> = raw.chunks_exact(8).map(|c| Cell::F(f64::from_bits(u64::from_le_bytes(c.try_into().unwrap())))).collect();
            fill_cells(&isnull, v.into_iter(), any_null)
        }
        Ty::B => {
            if r.u8()? != M_BOOL {
                return Err("codec: bad bool mode".into());
            }
            let v: Vec<Cell> = unpack_bits(&mut r, m, 1)?.into_iter().map(|b| Cell::B(b == 1)).collect();
            fill_cells(&isnull, v.into_iter(), any_null)
        }
    }
}

pub fn decode_segment(table: Table, bytes: &[u8]) -> Result<Vec<Vec<Cell>>, String> {
    let mut h = read_header(table, bytes)?;
    let spec = table.columns();
    let mut blocks = Vec::with_capacity(spec.len());
    for _ in spec {
        blocks.push(next_block(&mut h.body)?);
    }
    let nrows = h.nrows;
    // (Decoding columns in parallel inside a segment measured slower than sequential: segments are already decoded in parallel.)
    let cols: Vec<Vec<Cell>> = spec.iter().zip(&blocks).map(|((_, ty), (comp, data))| decode_col(*ty, nrows, *comp, data)).collect::<Result<_, _>>()?;
    let mut rows: Vec<Vec<Cell>> = (0..nrows).map(|_| Vec::with_capacity(spec.len())).collect();
    for c in cols {
        rows.iter_mut().zip(c).for_each(|(r, x)| r.push(x));
    }
    Ok(rows)
}

/// Decode only row `k` of a v2 segment: every column block is unwrapped (the entropy stage
/// is per column), but only one value per column is materialised.
pub fn decode_row(table: Table, bytes: &[u8], k: usize) -> Result<Vec<Cell>, String> {
    let mut h = read_header(table, bytes)?;
    if k >= h.nrows {
        return Err("codec: row out of range".into());
    }
    let spec = table.columns();
    let mut out = Vec::with_capacity(spec.len());
    for (_, ty) in spec {
        let (comp, data) = next_block(&mut h.body)?;
        let raw = unwrap_block(comp, data)?;
        let mut r = Rd::new(&raw);
        let n = h.nrows;
        let isnull = read_nulls(&mut r, n)?;
        if isnull[k] {
            out.push(Cell::Null);
            continue;
        }
        let any_null = isnull.iter().any(|&b| b);
        let m = if any_null { isnull.iter().filter(|&&b| !b).count() } else { n };
        let idx = if any_null { isnull[..k].iter().filter(|&&b| !b).count() } else { k };
        out.push(match ty {
            Ty::S => Cell::S(value_str(&mut r, m, idx)?),
            Ty::I => {
                if r.u8()? != M_INT {
                    return Err("codec: bad int mode".into());
                }
                let mut prev = 0i64;
                for _ in 0..=idx {
                    prev = prev.wrapping_add(unzz(r.varint()?));
                }
                Cell::I(prev)
            }
            Ty::F => {
                if r.u8()? != M_F64 {
                    return Err("codec: bad float mode".into());
                }
                let all = r.take(m.checked_mul(8).ok_or("codec: overflow")?)?;
                Cell::F(f64::from_bits(u64::from_le_bytes(all[idx * 8..idx * 8 + 8].try_into().unwrap())))
            }
            Ty::B => {
                if r.u8()? != M_BOOL {
                    return Err("codec: bad bool mode".into());
                }
                let all = r.take(m.div_ceil(8))?;
                Cell::B(all[idx / 8] >> (idx % 8) & 1 == 1)
            }
        });
    }
    Ok(out)
}

/// Value number `idx` of a string column body holding `m` values.
fn value_str(r: &mut Rd, m: usize, idx: usize) -> Result<String, String> {
    let mode = r.u8()?;
    match mode {
        M_RAW => {
            if m > r.left() {
                return Err("codec: bad count".into());
            }
            let (mut start, mut len) = (0usize, 0usize);
            for i in 0..m {
                let l = r.varint()? as usize;
                if i < idx {
                    start += l;
                } else if i == idx {
                    len = l;
                }
            }
            let rest = r.take(r.left())?;
            let s = rest.get(start..start.checked_add(len).ok_or("codec: overflow")?).ok_or("codec: truncated")?;
            String::from_utf8(s.to_vec()).map_err(|_| "codec: not utf-8".to_string())
        }
        M_DICT => {
            let dict = read_dict(r)?;
            let bits = r.u8()? as u32;
            if bits > 32 {
                return Err("codec: bad bit width".into());
            }
            let nbytes = (m * bits as usize).div_ceil(8);
            let data = r.take(nbytes)?;
            let code = if bits == 0 {
                0
            } else {
                let mut v = 0u64;
                let bit0 = idx * bits as usize;
                for b in 0..bits as usize {
                    let p = bit0 + b;
                    v |= ((data[p / 8] >> (p % 8) & 1) as u64) << b;
                }
                v as usize
            };
            dict.get(code).cloned().ok_or_else(|| "codec: dict code out of range".to_string())
        }
        M_HEX32 => {
            let raw = r.take(m.checked_mul(32).ok_or("codec: overflow")?)?;
            Ok(hex(&raw[idx * 32..idx * 32 + 32]))
        }
        M_TS => {
            let mut prev = 0i64;
            for _ in 0..=idx {
                prev = prev.wrapping_add(unzz(r.varint()?));
            }
            Ok(fmt_ts(prev))
        }
        M_TAGS => {
            let dict = read_dict(r)?;
            let mut out = String::new();
            for i in 0..m {
                let c = r.varint()? as usize;
                if c > r.left() {
                    return Err("codec: bad tag count".into());
                }
                for t in 0..c {
                    let code = r.varint()? as usize;
                    if i == idx {
                        if t > 0 {
                            out.push(',');
                        }
                        out.push_str(dict.get(code).ok_or("codec: tag code out of range")?);
                    }
                }
                if i == idx {
                    break;
                }
            }
            Ok(out)
        }
        x => Err(format!("codec: unknown string mode {x}")),
    }
}

/// Encoded bytes per column (block header included) of a v2 segment, for size reports.
pub fn column_block_sizes(bytes: &[u8]) -> Result<Vec<(String, usize)>, String> {
    let mut r = Rd::new(bytes);
    if r.take(4)? != MAGIC || r.u16()? != SEG_VERSION {
        return Err("not a v2 segment".into());
    }
    let ncols = r.u32()? as usize;
    let _ = r.u64()?;
    let mut names = vec![];
    for _ in 0..ncols {
        let l = r.u16()? as usize;
        names.push(String::from_utf8_lossy(r.take(l)?).to_string());
        r.u8()?;
    }
    let mut out = vec![];
    for n in names {
        let (_, d) = next_block(&mut r)?;
        out.push((n, d.len() + 9));
    }
    Ok(out)
}

/// An id as stored: 32 raw bytes when it is canonical hex, else the string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IdKey {
    Hex([u8; 32]),
    Other(String),
}

impl IdKey {
    pub fn from_str(s: &str) -> IdKey {
        match hex64_to_bytes(s) {
            Some(b) => IdKey::Hex(b),
            None => IdKey::Other(s.to_string()),
        }
    }
}

/// Decode only column 0 (the id column) of a v2 segment; nulls are skipped.
pub fn decode_first_col_ids(table: Table, bytes: &[u8]) -> Result<Vec<IdKey>, String> {
    let mut h = read_header(table, bytes)?;
    let (comp, data) = next_block(&mut h.body)?;
    let raw = unwrap_block(comp, data)?;
    let mut r = Rd::new(&raw);
    let isnull = read_nulls(&mut r, h.nrows)?;
    let m = isnull.iter().filter(|&&b| !b).count();
    Ok(match decode_strs_body(&mut r, m)? {
        StrCol::Hex(v) => v.into_iter().map(IdKey::Hex).collect(),
        StrCol::Strs(v) => v.iter().map(|s| IdKey::from_str(s)).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> Cell {
        Cell::S(x.to_string())
    }

    #[test]
    fn ts_roundtrip_and_rejects_noncanonical() {
        for t in ["2025-01-01T00:00:00Z", "2026-03-01T12:34:56Z", "1999-12-31T23:59:59Z", "2024-02-29T01:02:03Z"] {
            assert_eq!(fmt_ts(parse_ts(t).unwrap()), t);
        }
        for t in ["2025-02-30T00:00:00Z", "2025-01-01", "2025-13-01T00:00:00Z", "2025-01-01T00:00:00z", "2025-01-01T24:00:00Z"] {
            assert!(parse_ts(t).is_none(), "{t}");
        }
    }

    #[test]
    fn events_roundtrip_all_codecs() {
        let id = |i: u32| hex(&[(i % 251) as u8; 32]);
        let mut rows = vec![];
        for i in 0..300u32 {
            rows.push(vec![
                s(&id(i)), s(&format!("2025-06-{:02}T10:{:02}:00Z", 1 + i % 28, i % 60)), s("2026-03-01T00:00:00Z"), s(["a", "b", "c"][(i % 3) as usize]),
                s("note"), s(&format!("text number {i} with, commas \u{4e16}")), s(["", "x", "x,y", "z,,w", ","][(i % 5) as usize]), Cell::B(i % 7 == 0),
            ]);
        }
        let bytes = encode_segment(Table::Events, &rows);
        assert!(is_v2(&bytes));
        assert_eq!(decode_segment(Table::Events, &bytes).unwrap(), rows);
        assert_eq!(encode_segment(Table::Events, &rows), bytes, "deterministic");
        let ids = decode_first_col_ids(Table::Events, &bytes).unwrap();
        assert_eq!(ids.len(), 300);
        assert_eq!(ids[5], IdKey::from_str(&id(5)));
    }

    #[test]
    fn decode_row_equals_decode_segment_for_every_row() {
        let id = |i: u32| hex(&[(i % 251) as u8; 32]);
        let mut rows = vec![];
        for i in 0..300u32 {
            rows.push(vec![
                s(&id(i)), s(&format!("2025-06-{:02}T10:{:02}:00Z", 1 + i % 28, i % 60)), s("2026-03-01T00:00:00Z"), s(["a", "b", "c"][(i % 3) as usize]),
                s("note"), s(&format!("text number {i} with, commas \u{4e16} {}", "x".repeat((i % 17) as usize))), s(["", "x", "x,y", "z,,w", ","][(i % 5) as usize]), Cell::B(i % 7 == 0),
            ]);
        }
        let bytes = encode_segment(Table::Events, &rows);
        assert_eq!(decode_segment(Table::Events, &bytes).unwrap(), rows);
        for k in 0..rows.len() {
            assert_eq!(decode_row(Table::Events, &bytes, k).unwrap(), rows[k], "row {k}");
        }
        assert!(decode_row(Table::Events, &bytes, 300).is_err());
        // nulls, odd ids, floats
        let rv = |i: usize| {
            vec![
                if i % 3 == 0 { s("not hex") } else { s(&hex(&[i as u8; 32])) }, s("belief"), s(&format!("k{}", i % 4)), s("st"), Cell::F(i as f64 / 7.0), s("rat"),
                if i % 2 == 0 { Cell::Null } else { s(&hex(&[9u8; 32])) }, s("2025-01-01"), s("user"), s("2025-01-01T00:00:00Z"),
            ]
        };
        let rev_rows: Vec<Vec<Cell>> = (0..40).map(rv).collect();
        let b = encode_segment(Table::Revisions, &rev_rows);
        assert_eq!(decode_segment(Table::Revisions, &b).unwrap(), rev_rows);
        for k in 0..rev_rows.len() {
            assert_eq!(decode_row(Table::Revisions, &b, k).unwrap(), rev_rows[k], "rev row {k}");
        }
        let meta = vec![vec![Cell::I(2), s("2026-01-01T00:00:00Z")], vec![Cell::I(5), s("2026-01-02T00:00:00Z")]];
        let b = encode_segment(Table::SchemaMeta, &meta);
        for k in 0..2 {
            assert_eq!(decode_row(Table::SchemaMeta, &b, k).unwrap(), meta[k]);
        }
    }

    #[test]
    fn nulls_odd_ids_and_wrong_types_roundtrip() {
        let rows = vec![
            vec![s("not hex"), s("x"), Cell::F(0.5), s("2025-01-01T00:00:00Z"), Cell::Null, s("r"), s("2025-01-01T00:00:00Z"), s("user"), s("2025-01-01T00:00:00Z"), Cell::Null],
            vec![s(&"A".repeat(64)), s("y"), Cell::F(f64::NAN), Cell::Null, s(&hex(&[7u8; 32])), s(""), s("bad ts"), s("user"), s("2025-01-01T00:00:00Z"), s("q")],
        ];
        // revisions schema: rev_id, entity_type, key, stance, confidence, ... ; build matching shapes
        let mut rev_rows = vec![];
        for (i, r) in rows.iter().enumerate() {
            rev_rows.push(vec![
                r[0].clone(), s("belief"), s("k"), s("st"), Cell::F(if i == 0 { 0.5 } else { -0.0 }), s("rat"),
                if i == 0 { Cell::Null } else { s(&hex(&[9u8; 32])) }, s("2025-01-01"), s("user"), s("2025-01-01T00:00:00Z"),
            ]);
        }
        let b = encode_segment(Table::Revisions, &rev_rows);
        let back = decode_segment(Table::Revisions, &b).unwrap();
        assert_eq!(back, rev_rows);
        let empty = encode_segment(Table::Outcomes, &[]);
        assert_eq!(decode_segment(Table::Outcomes, &empty).unwrap(), Vec::<Vec<Cell>>::new());
        let _ = rows;
    }

    #[test]
    fn schema_meta_and_garbage() {
        let rows = vec![vec![Cell::I(2), s("2026-01-01T00:00:00Z")]];
        let b = encode_segment(Table::SchemaMeta, &rows);
        assert_eq!(decode_segment(Table::SchemaMeta, &b).unwrap(), rows);
        assert!(decode_segment(Table::Events, &b).is_err());
        for cut in 0..b.len() {
            let _ = decode_segment(Table::SchemaMeta, &b[..cut]);
        }
    }
}
