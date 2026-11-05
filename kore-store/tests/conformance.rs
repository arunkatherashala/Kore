//! Conformance fixtures: files covering every on-disk variant plus language-neutral expected
//! checksums (conformance/expected.txt). Any reader — Rust, Python, or a native Java/C#/Go parser —
//! must decode every file to the same logical data.
//!
//! Regenerate with: KORE_REGEN_FIXTURES=1 cargo test -p kore-store --test conformance
//!
//! Canonical row encoding hashed with FNV-1a 64 per column (see conformance/README.md):
//!   null            -> 0x00
//!   i64 / f64 / bool/ str -> 0x01 followed by: i64 8 bytes LE | f64 8 bytes LE bits (any NaN is
//!                      0x7FF8000000000001) | bool 1 byte | str u32 LE length + UTF-8 bytes

use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{reader::KoreReader, versioned, KoreWriter};
use std::path::PathBuf;

const ROWS: usize = 3000;
const PASSWORD: &[u8] = b"kore-conformance";

fn dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("conformance") }

fn dataset() -> DataBlock {
    let ints = (0..ROWS).map(|k| match k {
        0 => Some(i64::MIN),
        1 => Some(i64::MAX),
        _ if k % 10 == 3 => None,
        _ => Some(k as i64 * 7919 - 10_000_000),
    }).collect();
    let floats = (0..ROWS).map(|k| {
        if k == 7 { Some(-0.0) }
        else if k % 13 == 2 { None }
        else if k % 97 == 5 { Some(f64::NAN) }
        else { Some(((k * 7919) % 100_003) as f64 / 8.0) }
    }).collect();
    let strs = (0..ROWS).map(|k| {
        if k % 11 == 0 { None }
        else if k % 17 == 1 { Some(String::new()) }
        else if k % 19 == 4 { Some(format!("caf\u{e9}\u{1f600}-{k}")) }
        else { Some(format!("{}-{:02}-{:02}", 1990 + k % 10, (k / 10) % 12 + 1, (k / 120) % 28 + 1)) }
    }).collect();
    let bools = (0..ROWS).map(|k| if k % 7 == 0 { None } else { Some(k % 3 == 0) }).collect();
    let qty = (0..ROWS).map(|k| Some(((k as u64 * 2_654_435_761) % 10_007) as i64)).collect();
    DataBlock::new(vec![
        Column::int64("i", ints),
        Column::float64("f", floats),
        Column::str_col("s", strs),
        Column { name: "b".into(), data: ColumnData::Bool(bools) },
        Column::int64("q", qty),
    ]).unwrap()
}

fn fnv(h: &mut u64, bytes: &[u8]) {
    for &b in bytes { *h ^= b as u64; *h = h.wrapping_mul(0x100000001b3); }
}

/// (name, type, null count, fnv64) per column
fn digest(b: &DataBlock) -> Vec<(String, &'static str, usize, u64)> {
    b.columns.iter().map(|c| {
        let mut h = 0xcbf29ce484222325u64;
        let mut nulls = 0;
        let mut put = |h: &mut u64, v: Option<Vec<u8>>| match v {
            None => { nulls += 1; fnv(h, &[0]); }
            Some(bytes) => { fnv(h, &[1]); fnv(h, &bytes); }
        };
        let ty = match &c.data {
            ColumnData::Int64(v) => { for x in v { put(&mut h, x.map(|x| x.to_le_bytes().to_vec())); } "i64" }
            ColumnData::Float64(v) => {
                for x in v { put(&mut h, x.map(|x| (if x.is_nan() { 0x7FF8_0000_0000_0001u64 } else { x.to_bits() }).to_le_bytes().to_vec())); }
                "f64"
            }
            ColumnData::Bool(v) => { for x in v { put(&mut h, x.map(|x| vec![x as u8])); } "bool" }
            ColumnData::Str(v) => {
                for x in v { put(&mut h, x.as_ref().map(|s| [&(s.len() as u32).to_le_bytes()[..], s.as_bytes()].concat())); }
                "str"
            }
            ColumnData::StrDict { codes, dict } => {
                for &code in codes {
                    let s = if code == u8::MAX { None } else { Some(&dict[code as usize]) };
                    put(&mut h, s.map(|s| [&(s.len() as u32).to_le_bytes()[..], s.as_bytes()].concat()));
                }
                "str"
            }
        };
        (c.name.clone(), ty, nulls, h)
    }).collect()
}

fn expected_text(b: &DataBlock) -> String {
    let mut out = format!("rows {}\n", b.num_rows);
    for (name, ty, nulls, h) in digest(b) { out += &format!("{name} {ty} {nulls} {h:016x}\n"); }
    out
}

fn write_all() {
    std::env::set_var("KORE_READABLE_MODE", "none");
    let d = dir();
    std::fs::create_dir_all(&d).unwrap();
    let b = dataset();
    let put = |name: &str, bytes: Vec<u8>| std::fs::write(d.join(name), bytes).unwrap();
    for v in ["KORE_SHUFFLE", "KORE_STR_LENGTHS"] { std::env::remove_var(v); }
    put("default.kore", KoreWriter::to_bytes(&b));
    std::env::set_var("KORE_SHUFFLE", "1");
    put("shuffle.kore", KoreWriter::to_bytes(&b));
    std::env::remove_var("KORE_SHUFFLE");
    std::env::set_var("KORE_STR_LENGTHS", "1");
    put("strlen.kore", KoreWriter::to_bytes(&b));
    std::env::set_var("KORE_SHUFFLE", "1");
    put("shuffle_strlen.kore", KoreWriter::to_bytes(&b));
    for v in ["KORE_SHUFFLE", "KORE_STR_LENGTHS"] { std::env::remove_var(v); }
    put("encrypted.kore", KoreWriter::to_bytes_encrypted(&b, PASSWORD).unwrap());
    put("row_groups.kore", kore_store::rowgroups::write(&b, 700).unwrap());
    // a log whose first version is a different, tiny block (timestamp 100) and whose latest is the dataset (200)
    let small = DataBlock::new(vec![Column::int64("only", vec![Some(1), Some(2)])]).unwrap();
    let log = versioned::append_version(None, &small, 100).unwrap();
    put("versions.kore", versioned::append_version(Some(&log), &b, 200).unwrap());
    std::fs::write(d.join("expected.txt"), expected_text(&b)).unwrap();
}

#[test]
fn every_fixture_decodes_to_the_expected_data() {
    if std::env::var("KORE_REGEN_FIXTURES").map(|v| v == "1").unwrap_or(false) { write_all(); }
    let d = dir();
    // git on Windows may check text files out with CRLF
    let expected = std::fs::read_to_string(d.join("expected.txt")).expect("run with KORE_REGEN_FIXTURES=1 first")
        .replace("\r\n", "\n");
    assert_eq!(expected_text(&dataset()), expected, "generator and expected.txt disagree");

    for name in ["default", "shuffle", "strlen", "shuffle_strlen", "versions", "row_groups"] {
        let bytes = std::fs::read(d.join(format!("{name}.kore"))).unwrap();
        let got = KoreReader::from_bytes(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(expected_text(&got), expected, "{name}.kore decodes differently");
    }
    let enc = std::fs::read(d.join("encrypted.kore")).unwrap();
    assert_eq!(expected_text(&KoreReader::from_bytes_decrypt(&enc, PASSWORD).unwrap()), expected);
    assert!(KoreReader::from_bytes_decrypt(&enc, b"wrong").is_err());

    // time travel: before the second version only the tiny block exists
    let log = std::fs::read(d.join("versions.kore")).unwrap();
    assert_eq!(KoreReader::read_at_version(&log, 150).unwrap().num_rows, 2);
    assert_eq!(KoreReader::read_at_version(&log, 200).unwrap().num_rows, ROWS);
    assert!(KoreReader::read_at_version(&log, 50).is_err());

    // row groups: 3000 rows / 700 = 5 groups; only group 0 holds i64::MAX, so a range above every
    // other value keeps one group
    let rg = std::fs::read(d.join("row_groups.kore")).unwrap();
    let idx = kore_store::rowgroups::index(&rg).unwrap();
    assert_eq!(idx.groups.len(), 5);
    let above = kore_store::rowgroups::Range { column: "i".into(), min: Some(15_000_000.0), max: None };
    assert_eq!(kore_store::rowgroups::matching_groups(&idx, &above).unwrap(), vec![0]);

    // projection agrees with the full read
    let bytes = std::fs::read(d.join("shuffle_strlen.kore")).unwrap();
    let one = KoreReader::from_bytes_columns(&bytes, &["s"]).unwrap();
    let full = digest(&KoreReader::from_bytes(&bytes).unwrap());
    assert_eq!(digest(&one)[0], full.iter().find(|c| c.0 == "s").unwrap().clone());
}
