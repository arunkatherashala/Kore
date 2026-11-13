//! Deterministic datasets of the `tests/fixtures_dev` set (docs/format_sync.md), shared by the generator
//! (`examples/gen_dev_fixtures.rs`, run with dev's kore-store before the format sync) and the test
//! `tests/dev_fixtures.rs` (run with the synced kore-store). Also the canonical value digest (the same
//! encoding as `tests/old_files.rs` of fileformat), used for files written by kore-self.
#![allow(dead_code)]
use kore_core::{Column, ColumnData, DataBlock};

fn col(name: &str, data: ColumnData) -> Column { Column { name: name.into(), data } }

/// Every column type with NULLs, extreme ints, special floats (NaN, +-0.0, +-inf, subnormal) and
/// unicode / empty strings.
pub fn all_types() -> DataBlock {
    let n = 300usize;
    let ext = [i64::MIN, i64::MAX, 0, -1, 1];
    let ints = (0..n).map(|k| if k % 10 == 3 { None } else if k < 5 { Some(ext[k]) } else { Some((k as i64 * 7919) % 1000 - 500) }).collect();
    let sf = [f64::NAN, -0.0, 0.0, f64::INFINITY, f64::NEG_INFINITY, f64::MIN_POSITIVE / 4.0, f64::MAX, f64::MIN];
    let floats = (0..n).map(|k| if k % 13 == 2 { None } else if k < sf.len() { Some(sf[k]) } else { Some(k as f64 / 8.0 - 3.3) }).collect();
    let bools = (0..n).map(|k| if k % 7 == 0 { None } else { Some(k % 3 == 0) }).collect();
    let strs = (0..n).map(|k| if k % 11 == 0 { None } else if k % 17 == 1 { Some(String::new()) }
        else { Some(format!("row-{k} caf\u{e9} \u{1f600} \"q\" \\ \t {}", "x".repeat(k % 40))) }).collect();
    let lc = ["alpha", "beta", "gamma"];
    let codes = (0..n).map(|k| if k % 9 == 0 { u8::MAX } else { (k % 3) as u8 }).collect();
    DataBlock { num_rows: n, columns: vec![
        col("i", ColumnData::Int64(ints)),
        col("f", ColumnData::Float64(floats)),
        col("b", ColumnData::Bool(bools)),
        col("s", ColumnData::Str(strs)),
        col("sd", ColumnData::StrDict { codes, dict: lc.iter().map(|s| s.to_string()).collect() }),
    ] }
}

/// The shape `kore_rows` writes: typed columns plus `<key>:json` text columns holding nested JSON.
pub fn json_cols() -> DataBlock {
    let n = 120usize;
    let id = (0..n).map(|k| Some(k as i64)).collect();
    let name = (0..n).map(|k| if k % 8 == 5 { None } else { Some(format!("being-{}", k % 6)) }).collect();
    let meta = (0..n).map(|k| match k % 5 {
        0 => None,
        1 => Some(format!("{{\"a\":{k},\"b\":[1,2,{{\"c\":\"d\\u00e9\"}}],\"e\":null,\"f\":{{\"g\":{{\"h\":[true,false]}}}}}}")),
        2 => Some(format!("[{k},\"x\",[[],{{}}],-1.5e-7]")),
        3 => Some("\"just a string with \\\"quotes\\\" and \\n\"".to_string()),
        _ => Some("{}".to_string()),
    }).collect();
    let score = (0..n).map(|k| if k % 4 == 0 { None } else { Some(k as f64 * 0.1) }).collect();
    let ok = (0..n).map(|k| if k % 6 == 0 { None } else { Some(k % 2 == 0) }).collect();
    DataBlock { num_rows: n, columns: vec![
        col("id", ColumnData::Int64(id)),
        col("name", ColumnData::Str(name)),
        col("meta:json", ColumnData::Str(meta)),
        col("score", ColumnData::Float64(score)),
        col("ok", ColumnData::Bool(ok)),
    ] }
}

/// Columns of every type, zero rows.
pub fn empty_rows() -> DataBlock {
    DataBlock { num_rows: 0, columns: vec![
        col("i", ColumnData::Int64(vec![])),
        col("f", ColumnData::Float64(vec![])),
        col("b", ColumnData::Bool(vec![])),
        col("s", ColumnData::Str(vec![])),
    ] }
}

/// No columns at all.
pub fn no_columns() -> DataBlock { DataBlock { num_rows: 0, columns: vec![] } }

/// Every cell NULL.
pub fn all_null() -> DataBlock {
    let n = 50usize;
    DataBlock { num_rows: n, columns: vec![
        col("i", ColumnData::Int64(vec![None; n])),
        col("f", ColumnData::Float64(vec![None; n])),
        col("b", ColumnData::Bool(vec![None; n])),
        col("s", ColumnData::Str(vec![None; n])),
    ] }
}

/// Larger table (event-log shape) for row groups and big payloads; compressible to keep the fixtures small.
pub fn large() -> DataBlock {
    let n = 6_000usize;
    let ts = (0..n).map(|k| Some(1_760_000_000 + (k as i64) * 3 + (k as i64 % 5))).collect();
    let kind = (0..n).map(|k| Some(["note", "chat", "sense", "plan"][k % 4].to_string())).collect();
    let text = (0..n).map(|k| if k % 101 == 0 { None } else { Some(format!("event {} about topic {} said {}", k, k % 37, k % 3)) }).collect();
    let v = (0..n).map(|k| if k % 53 == 0 { None } else { Some((k % 1000) as f64 / 4.0) }).collect();
    DataBlock { num_rows: n, columns: vec![
        col("ts", ColumnData::Int64(ts)),
        col("kind", ColumnData::Str(kind)),
        col("text", ColumnData::Str(text)),
        col("v", ColumnData::Float64(v)),
    ] }
}

/// Pseudo-random text `k` (high entropy, so only a dictionary that holds it makes it small).
fn rnd_text(k: u64) -> String {
    let mut x = k.wrapping_mul(0x9E3779B97F4A7C15) ^ 0xD1B54A32D192ED03;
    let mut s = format!("note {k}: ");
    for _ in 0..12 {
        x ^= x << 13; x ^= x >> 7; x ^= x << 17;
        s += &format!("{:x} ", x & 0xFFFF_FFFF);
    }
    s
}

/// Event texts for a shared dictionary (v4 compact with a `.kdic`): each text is in the dictionary.
pub fn dict_texts() -> DataBlock {
    let n = 80usize;
    let text = (0..n).map(|k| if k % 29 == 0 { None } else { Some(rnd_text(k as u64 % 60)) }).collect();
    let id = (0..n).map(|k| Some(format!("{:016x}{:016x}{:016x}{:016x}", (k as u64).wrapping_mul(0x9E3779B97F4A7C15), k * 31, k * 7, k))).collect();
    DataBlock { num_rows: n, columns: vec![
        col("id", ColumnData::Str(id)),
        col("text", ColumnData::Str(text)),
    ] }
}

/// Content of the shared dictionary used with `dict_texts`.
pub fn dict_content() -> Vec<u8> {
    let mut c = vec![];
    for k in 0..60 {
        if k > 0 { c.push(0xFF); }
        c.extend_from_slice(rnd_text(k).as_bytes());
    }
    c
}

/// Every NaN compares as this one bit pattern: dev's default writer stores a NaN as 0x7FF8_0000_0000_0001
/// (a written 0x7FF8_0000_0000_0000 reads back with payload 1), so NaN payloads are not part of the values.
pub const NAN_BITS: u64 = 0x7FF8_0000_0000_0001;

/// Values of one column as canonical cells (StrDict expanded to strings; floats as bits, NaN folded).
#[derive(Debug, PartialEq, Eq)]
pub enum Cells { I(Vec<Option<i64>>), F(Vec<Option<u64>>), B(Vec<Option<bool>>), S(Vec<Option<String>>) }

pub fn cells(d: &ColumnData) -> Cells {
    match d {
        ColumnData::Int64(v) => Cells::I(v.clone()),
        ColumnData::Float64(v) => Cells::F(v.iter().map(|x| x.map(|x| if x.is_nan() { NAN_BITS } else { x.to_bits() })).collect()),
        ColumnData::Bool(v) => Cells::B(v.clone()),
        ColumnData::Str(v) => Cells::S(v.clone()),
        ColumnData::StrDict { codes, dict } => Cells::S(codes.iter().map(|&c| if c == u8::MAX { None } else { Some(dict[c as usize].clone()) }).collect()),
    }
}

/// `a` and `b` hold exactly the same values (names, order, rows, bit-exact floats, every NaN as `NAN_BITS`).
pub fn same_values(a: &DataBlock, b: &DataBlock) -> Result<(), String> {
    if a.num_rows != b.num_rows { return Err(format!("rows {} vs {}", a.num_rows, b.num_rows)); }
    if a.columns.len() != b.columns.len() { return Err(format!("columns {} vs {}", a.columns.len(), b.columns.len())); }
    for (x, y) in a.columns.iter().zip(&b.columns) {
        if x.name != y.name { return Err(format!("column name {} vs {}", x.name, y.name)); }
        if cells(&x.data) != cells(&y.data) { return Err(format!("column {} differs", x.name)); }
    }
    Ok(())
}

fn fnv(h: &mut u64, bytes: &[u8]) {
    for &b in bytes { *h ^= b as u64; *h = h.wrapping_mul(0x100000001b3); }
}

/// Canonical value digest: "rows N" then per column "<name> <type> <nulls> <fnv64>" (floats bit-exact
/// with NaN folded to `NAN_BITS`, as `old_files.rs` does; StrDict counts as str).
pub fn digest(b: &DataBlock) -> String {
    let mut out = format!("rows {}\n", b.num_rows);
    for c in &b.columns {
        let mut h = 0xcbf29ce484222325u64;
        let mut nulls = 0usize;
        let mut put = |h: &mut u64, v: Option<Vec<u8>>| match v {
            None => { nulls += 1; fnv(h, &[0]); }
            Some(bytes) => { fnv(h, &[1]); fnv(h, &bytes); }
        };
        let ty = match cells(&c.data) {
            Cells::I(v) => { for x in v { put(&mut h, x.map(|x| x.to_le_bytes().to_vec())); } "i64" }
            Cells::F(v) => { for x in v { put(&mut h, x.map(|x| x.to_le_bytes().to_vec())); } "f64" }
            Cells::B(v) => { for x in v { put(&mut h, x.map(|x| vec![x as u8])); } "bool" }
            Cells::S(v) => {
                for x in v { put(&mut h, x.map(|x| [&(x.len() as u32).to_le_bytes()[..], x.as_bytes()].concat())); }
                "str"
            }
        };
        out += &format!("{} {ty} {nulls} {h:016x}\n", c.name);
    }
    out
}
