//! The version-4 writer still produces exactly the bytes of the bbbefd2c writer (digests in
//! tests/v4_writer_bytes.txt, written by that commit's code) on many options, the batch API is
//! byte-identical per file, and the explicit `CompactOptions::fast()` / `small_zstd_level` settings
//! round-trip.

#[path = "common/v4_cases.rs"]
mod v4_cases;

use kore_core::{ColumnData, DataBlock};
use kore_store::{v4, CompactOptions, DictSet, KoreReader, KoreWriter, SharedDict};
use sha2::{Digest, Sha256};

fn sha(b: &[u8]) -> String { Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect() }

fn expected() -> Vec<(String, usize, String)> {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("v4_writer_bytes.txt");
    std::fs::read_to_string(p).unwrap().lines().filter(|l| !l.trim().is_empty()).map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        (f[0].to_string(), f[1].parse().unwrap(), f[2].to_string())
    }).collect()
}

fn dicts_of(o: &CompactOptions) -> DictSet {
    let mut s = DictSet::new();
    for (_, d) in &o.dicts { s.add(d.clone()); }
    s
}

fn same_values(a: &DataBlock, b: &DataBlock) {
    assert_eq!(a.num_rows, b.num_rows);
    for (x, y) in a.columns.iter().zip(&b.columns) {
        assert_eq!(x.name, y.name);
        match (&x.data, &y.data) {
            (ColumnData::Float64(p), ColumnData::Float64(q)) => assert!(p.iter().zip(q).all(|(u, v)| u.map(f64::to_bits) == v.map(f64::to_bits))),
            _ => assert!((0..a.num_rows).all(|r| x.data.get_value(r) == y.data.get_value(r)), "column {}", x.name),
        }
    }
}

#[test]
fn writer_bytes_match_bbbefd2c() {
    let want = expected();
    let cases = v4_cases::cases();
    assert_eq!(want.len(), cases.len());
    for ((name, b, o), (wname, wlen, wsha)) in cases.iter().zip(&want) {
        assert_eq!(name, wname);
        let fast = KoreWriter::to_bytes_compact(b, o).unwrap();
        assert_eq!((fast.len(), sha(&fast)), (*wlen, wsha.clone()), "case {name}: write differs from bbbefd2c");
        // the default small level given explicitly = the same bytes
        assert_eq!(KoreWriter::to_bytes_compact(b, &o.clone().with_small_zstd_level(19)).unwrap(), fast);
    }
    // batch API: same bytes per file
    let items: Vec<(&DataBlock, &CompactOptions)> = cases.iter().map(|(_, b, o)| (b, o)).collect();
    let many = KoreWriter::to_bytes_compact_many(&items).unwrap();
    for (out, (_, _, wsha)) in many.iter().zip(&want) { assert_eq!(&sha(out), wsha); }
}

#[test]
fn explicit_small_levels_round_trip_and_are_deterministic() {
    for (name, b, o) in v4_cases::cases() {
        for small in [12, 9, 3, 1] {
            let o2 = o.clone().with_small_zstd_level(small);
            let f = v4::write(&b, &o2).unwrap();
            assert_eq!(f, v4::write(&b, &o2).unwrap(), "case {name} small level {small}: not deterministic");
            same_values(&KoreReader::from_bytes_with_dicts(&f, &dicts_of(&o2)).unwrap(), &b);
        }
    }
}

#[test]
fn fast_preset_round_trips_and_is_explicit() {
    let b = v4_cases::events(2000, 7, 14);
    let d = SharedDict::from_content(b"the meeting moved to tuesday and Mara agreed").unwrap();
    let def = CompactOptions::new().with_dict("text", d.clone());
    let fast = CompactOptions::fast().with_dict("text", d.clone());
    assert_eq!(CompactOptions::new().small_zstd_level, 19);
    assert_eq!(fast.small_zstd_level, 9);
    let a = KoreWriter::to_bytes_compact(&b, &def).unwrap();
    let f = KoreWriter::to_bytes_compact(&b, &fast).unwrap();
    assert_ne!(a, f, "the fast setting changes bytes (that is why it is opt-in)");
    let ds = DictSet::new().with(d);
    same_values(&KoreReader::from_bytes_with_dicts(&f, &ds).unwrap(), &b);
    assert!(v4::is_v4(&f));
}
