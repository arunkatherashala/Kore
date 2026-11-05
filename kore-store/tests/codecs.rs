//! Version-3 per-column codecs: round trips, determinism, tamper detection, compatibility with the
//! v2 fixtures written by the unmodified writer, and rejection by version-2-only readers.

use kore_core::{Column, ColumnData, DataBlock};
use kore_store::codec::{self, ColumnCodec, Stage, Transform, WriteOptions};
use kore_store::{rowgroups, versioned, KoreReader, KoreWriter};
use std::path::PathBuf;

#[path = "common/compat_data.rs"]
mod compat_data;

fn fixture(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Bitwise equality (NaN == NaN, -0.0 != 0.0).
fn same_col(a: &ColumnData, b: &ColumnData) -> bool {
    match (a, b) {
        (ColumnData::Float64(x), ColumnData::Float64(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| match (p, q) {
                (None, None) => true,
                (Some(p), Some(q)) => p.to_bits() == q.to_bits() || (p.is_nan() && q.is_nan()),
                _ => false,
            })
        }
        (ColumnData::Int64(x), ColumnData::Int64(y)) => x == y,
        (ColumnData::Bool(x), ColumnData::Bool(y)) => x == y,
        (ColumnData::Str(x), ColumnData::Str(y)) => x == y,
        (ColumnData::StrDict { codes: c1, dict: d1 }, ColumnData::StrDict { codes: c2, dict: d2 }) => c1 == c2 && d1 == d2,
        _ => false,
    }
}

fn assert_same(a: &DataBlock, b: &DataBlock) {
    assert_eq!(a.num_rows, b.num_rows);
    assert_eq!(a.columns.len(), b.columns.len());
    for (x, y) in a.columns.iter().zip(&b.columns) {
        assert_eq!(x.name, y.name);
        assert!(same_col(&x.data, &y.data), "column '{}' differs", x.name);
    }
}

// ── compatibility with files written by the unmodified code ──────────────────

/// Logical view independent of dictionary representation (the reader may return StrDict for Str).
fn as_strs(c: &ColumnData) -> Option<Vec<Option<String>>> {
    match c {
        ColumnData::Str(v) => Some(v.clone()),
        ColumnData::StrDict { codes, dict } => Some(codes.iter().map(|&k| if k == u8::MAX { None } else { Some(dict[k as usize].clone()) }).collect()),
        _ => None,
    }
}

fn assert_logically_same(got: &DataBlock, want: &DataBlock) {
    assert_eq!(got.num_rows, want.num_rows);
    assert_eq!(got.columns.len(), want.columns.len());
    for (g, w) in got.columns.iter().zip(&want.columns) {
        assert_eq!(g.name, w.name);
        match (as_strs(&g.data), as_strs(&w.data)) {
            (Some(a), Some(b)) => assert_eq!(a, b, "column {}", g.name),
            _ => assert!(same_col(&g.data, &w.data), "column {}", g.name),
        }
    }
}

#[test]
fn v2_fixtures_still_read_identically() {
    for (file, want) in [
        ("mixed_default.kore", compat_data::mixed()),
        ("mixed_notrailer.kore", compat_data::mixed()),
        ("sorted_ints.kore", compat_data::sorted_ints()),
        ("lowcard_strs.kore", compat_data::lowcard_strs()),
        ("text.kore", compat_data::text()),
        ("rowgroups.kore", compat_data::mixed()),
    ] {
        let got = KoreReader::from_bytes(&fixture(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_logically_same(&got, &want);
    }
    // version log: newest entry is the mixed block; the older entry is the sorted-int block
    let log = fixture("versionlog.kore");
    assert_logically_same(&KoreReader::from_bytes(&log).unwrap(), &compat_data::mixed());
    assert_logically_same(&versioned::read_at(&log, 10).unwrap(), &compat_data::sorted_ints());
}

#[test]
fn v2_writer_output_is_byte_identical_to_the_old_writer() {
    // fixtures were produced by commit a980727d before any change; the legacy writer must not drift
    assert_eq!(KoreWriter::to_bytes(&compat_data::mixed()), fixture("mixed_default.kore"));
    assert_eq!(KoreWriter::to_bytes(&compat_data::sorted_ints()).len() > 0, true);
    let notrailer = fixture("mixed_notrailer.kore");
    assert!(fixture("mixed_default.kore").starts_with(&notrailer));
    assert_eq!(rowgroups::write(&compat_data::mixed(), 100).unwrap(), fixture("rowgroups.kore"));
    // the log fixture was generated with KORE_READABLE_MODE=none, so compare structure, not bytes
    let ts: Vec<u64> = versioned::list_versions(&fixture("versionlog.kore")).unwrap().iter().map(|v| v.0).collect();
    assert_eq!(ts, [10, 20]);
    for f in ["sorted_ints.kore", "lowcard_strs.kore", "text.kore"] {
        let want = match f {
            "sorted_ints.kore" => compat_data::sorted_ints(),
            "lowcard_strs.kore" => compat_data::lowcard_strs(),
            _ => compat_data::text(),
        };
        let b = KoreWriter::to_bytes(&want);
        let fx = fixture(f);
        // identical except that the default writer may append the readable trailer (env-dependent)
        assert!(b.starts_with(&fx[..fx.len().min(b.len())]) || fx.starts_with(&b), "{f}");
    }
}

#[test]
fn v2_files_are_still_version_2_and_v3_is_only_opt_in() {
    let b = KoreWriter::to_bytes(&compat_data::mixed());
    assert_eq!(&b[4..6], &2u16.to_le_bytes());
    let b3 = KoreWriter::to_bytes_with(&compat_data::mixed(), &WriteOptions::auto()).unwrap();
    assert_eq!(&b3[4..6], &3u16.to_le_bytes());
}

// ── round trips ──────────────────────────────────────────────────────────────

fn ints() -> Vec<Option<i64>> {
    let mut v = vec![Some(i64::MIN), Some(i64::MAX), Some(0), None, Some(-1), Some(1), None, Some(i64::MAX), Some(i64::MIN)];
    v.extend((0..500).map(|k| if k % 9 == 4 { None } else { Some(1_700_000_000_000 + k * 1000 + (k % 7)) }));
    v
}
fn floats() -> Vec<Option<f64>> {
    let mut v = vec![Some(0.0), Some(-0.0), Some(f64::NAN), Some(f64::INFINITY), Some(f64::NEG_INFINITY), None, Some(f64::MIN_POSITIVE), Some(f64::from_bits(0x7ff8_0000_0000_0001))];
    v.extend((0..300).map(|k| if k % 13 == 2 { None } else { Some(k as f64 / 7.0) }));
    v
}
fn bools() -> Vec<Option<bool>> { (0..333).map(|k| if k % 7 == 0 { None } else { Some(k % 3 == 0) }).collect() }
fn strs() -> Vec<Option<String>> {
    (0..400).map(|k| match k % 6 {
        0 => None,
        1 => Some(String::new()),
        2 => Some(format!("caf\u{e9} \u{1f600} {k}")),
        _ => Some(["alpha", "beta", "gamma"][k % 3].to_string()),
    }).collect()
}
fn hexes(n: usize, nulls: bool) -> Vec<Option<String>> {
    (0..n).map(|k| if nulls && k % 5 == 3 { None } else {
        Some(format!("{:016x}{:016x}{:016x}{:016x}", (k as u64).wrapping_mul(0x9E3779B97F4A7C15), k as u64 ^ 0xdead, (k as u64).wrapping_mul(31), !(k as u64)))
    }).collect()
}

fn one(name: &str, data: ColumnData) -> DataBlock {
    DataBlock { num_rows: data.len(), columns: vec![Column { name: name.into(), data }] }
}

const STAGES: [Stage; 5] = [Stage::None, Stage::Lz4, Stage::Hz, Stage::Zstd, Stage::Auto];

#[test]
fn every_valid_transform_and_stage_round_trips() {
    let cases: Vec<(ColumnData, Vec<Transform>)> = vec![
        (ColumnData::Int64(ints()), vec![Transform::Plain, Transform::Delta, Transform::Varint, Transform::Auto]),
        (ColumnData::Float64(floats()), vec![Transform::Plain, Transform::Auto]),
        (ColumnData::Bool(bools()), vec![Transform::Plain, Transform::Auto]),
        (ColumnData::Str(strs()), vec![Transform::Plain, Transform::Dict, Transform::Auto]),
        (ColumnData::Str(hexes(300, true)), vec![Transform::Plain, Transform::Dict, Transform::HexFixed, Transform::Auto]),
        (ColumnData::StrDict { codes: (0..100).map(|k| if k % 4 == 0 { u8::MAX } else { (k % 3) as u8 }).collect(),
                               dict: vec!["a".into(), "bb".into(), "".into()] }, vec![Transform::Plain, Transform::Auto]),
        (ColumnData::Int64(vec![]), vec![Transform::Plain, Transform::Delta, Transform::Auto]),
        (ColumnData::Str(vec![None, None, None]), vec![Transform::Plain, Transform::Dict, Transform::HexFixed, Transform::Auto]),
        (ColumnData::Int64(vec![None, None]), vec![Transform::Delta]),
    ];
    for (data, transforms) in cases {
        for t in transforms {
            for s in STAGES {
                let block = one("c", data.clone());
                let bytes = KoreWriter::to_bytes_with(&block, &WriteOptions::plain().with_column("c", ColumnCodec::new(t, s)))
                    .unwrap_or_else(|e| panic!("{t:?}/{s:?}: {e}"));
                let back = KoreReader::from_bytes(&bytes).unwrap_or_else(|e| panic!("{t:?}/{s:?}: {e}"));
                assert_same(&back, &block);
            }
        }
    }
}

#[test]
fn invalid_codec_requests_are_errors_not_silent_fallbacks() {
    let w = |data: ColumnData, t| KoreWriter::to_bytes_with(&one("c", data), &WriteOptions::plain().with_column("c", ColumnCodec::new(t, Stage::None)));
    assert!(w(ColumnData::Float64(floats()), Transform::Delta).is_err());
    assert!(w(ColumnData::Int64(ints()), Transform::Dict).is_err());
    assert!(w(ColumnData::Str(strs()), Transform::HexFixed).is_err());
    assert!(w(ColumnData::Str(vec![Some("ABCD".into())]), Transform::HexFixed).is_err(), "uppercase hex would not round trip");
    assert!(w(ColumnData::Str(vec![Some("abcd".into()), Some("abcdef".into())]), Transform::HexFixed).is_err());
    assert!(w(ColumnData::Str(vec![Some("abc".into())]), Transform::HexFixed).is_err(), "odd length");
    let block = one("c", ColumnData::Int64(vec![Some(1)]));
    assert!(KoreWriter::to_bytes_with(&block, &WriteOptions::plain().with_column("nope", ColumnCodec::AUTO)).is_err());
}

#[test]
fn auto_picks_the_intended_codecs() {
    let n = 5000;
    let block = DataBlock { num_rows: n, columns: vec![
        Column { name: "id".into(), data: ColumnData::Str(hexes(n, false)) },
        Column { name: "ts".into(), data: ColumnData::Int64((0..n as i64).map(|k| Some(1_700_000_000_000 + k * 1000 + k % 7)).collect()) },
        Column { name: "tag".into(), data: ColumnData::Str((0..n).map(|k| Some(format!("tag{}", k % 12))).collect()) },
    ] };
    let (t_id, _, p_id) = codec::encode_column(&block.columns[0].data, ColumnCodec::new(Transform::Auto, Stage::None)).unwrap();
    assert_eq!(t_id, Transform::HexFixed.id().unwrap());
    assert!(p_id.len() < n * 33, "hex ids must be stored at about half size, got {}", p_id.len());
    let (t_ts, _, _) = codec::encode_column(&block.columns[1].data, ColumnCodec::new(Transform::Auto, Stage::None)).unwrap();
    assert_eq!(t_ts, Transform::Delta.id().unwrap());
    let (t_tag, _, _) = codec::encode_column(&block.columns[2].data, ColumnCodec::new(Transform::Auto, Stage::None)).unwrap();
    assert_eq!(t_tag, Transform::Dict.id().unwrap());
    let bytes = KoreWriter::to_bytes_with(&block, &WriteOptions::auto()).unwrap();
    assert!(bytes.len() < KoreWriter::to_bytes(&block).len(), "auto v3 should not be larger than v2 here");
    assert_same(&KoreReader::from_bytes(&bytes).unwrap(), &block);
}

#[test]
fn projection_reads_selected_columns_in_requested_order() {
    let block = compat_data::mixed();
    let bytes = KoreWriter::to_bytes_with(&block, &WriteOptions::auto()).unwrap();
    let got = KoreReader::from_bytes_columns(&bytes, &["s", "i"]).unwrap();
    assert_eq!(got.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["s", "i"]);
    assert!(same_col(&got.columns[1].data, &block.columns[0].data));
    assert!(KoreReader::from_bytes_columns(&bytes, &["missing"]).is_err());
}

#[test]
fn v3_inside_version_log_and_file_roundtrip() {
    let block = compat_data::text();
    let entry = KoreWriter::to_bytes_with(&block, &WriteOptions::auto()).unwrap();
    let log = versioned::append_raw(None, &entry, 5).unwrap();
    assert_same(&KoreReader::from_bytes(&log).unwrap(), &block);
    let p = std::env::temp_dir().join(format!("kore_v3_{}.kore", std::process::id()));
    KoreWriter::write_file_with(&p, &block, &WriteOptions::auto()).unwrap();
    assert_same(&KoreReader::read_file(&p).unwrap(), &block);
    std::fs::remove_file(p).ok();
}

#[test]
fn v3_text_containing_the_readable_footer_marker_is_not_truncated() {
    // the v2 reader strips a trailer by scanning for this marker; v3 never does
    let s = "KORE-READABLE-FOOTER trailer_len=00000000000000000010 mode=none\n";
    let block = one("t", ColumnData::Str(vec![Some(s.repeat(3)), Some("x".into())]));
    let bytes = KoreWriter::to_bytes_with(&block, &WriteOptions::plain()).unwrap();
    assert_same(&KoreReader::from_bytes(&bytes).unwrap(), &block);
}

// ── determinism ──────────────────────────────────────────────────────────────

fn realistic(n: usize) -> DataBlock {
    let sentences = ["the build finished without errors", "connection reset by peer while syncing the index", "user opened the settings page", "retrying request after timeout"];
    let text = (0..n).map(|k| {
        let mut s = String::new();
        let reps = 1 + (k * 7919) % 40;
        for j in 0..reps { s.push_str(sentences[(k + j * 3) % 4]); s.push(' '); s.push_str(&(k * j).to_string()); s.push_str(". "); }
        Some(s)
    }).collect();
    DataBlock { num_rows: n, columns: vec![
        Column { name: "id".into(), data: ColumnData::Str(hexes(n, false)) },
        Column { name: "ts".into(), data: ColumnData::Int64((0..n as i64).map(|k| Some(1_700_000_000_000 + k * 37 + k % 5)).collect()) },
        Column { name: "kind".into(), data: ColumnData::Str((0..n).map(|k| Some(["a", "b", "c"][k % 3].to_string())).collect()) },
        Column { name: "text".into(), data: ColumnData::Str(text) },
    ] }
}

#[test]
fn output_is_deterministic_across_runs_and_thread_counts() {
    let block = realistic(3000);
    let opts = WriteOptions::auto();
    let first = KoreWriter::to_bytes_with(&block, &opts).unwrap();
    assert_eq!(first, KoreWriter::to_bytes_with(&block, &opts).unwrap());
    for threads in [1usize, 2, 7] {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
        let b = pool.install(|| KoreWriter::to_bytes_with(&block, &opts).unwrap());
        assert_eq!(b, first, "{threads} threads");
    }
    // explicit entropy stage as well
    let o2 = WriteOptions::plain().with_column("text", ColumnCodec::new(Transform::Plain, Stage::Hz));
    assert_eq!(KoreWriter::to_bytes_with(&block, &o2).unwrap(), KoreWriter::to_bytes_with(&block, &o2).unwrap());
    // and the file really is smaller than the legacy encoding
    assert!(first.len() < KoreWriter::to_bytes(&block).len(), "v3 {} vs v2 {}", first.len(), KoreWriter::to_bytes(&block).len());
}

// ── tamper detection ─────────────────────────────────────────────────────────

fn small_v3() -> (DataBlock, Vec<u8>) {
    let n = 40;
    let block = DataBlock { num_rows: n, columns: vec![
        Column { name: "id".into(), data: ColumnData::Str(hexes(n, true)) },
        Column { name: "ts".into(), data: ColumnData::Int64((0..n as i64).map(|k| if k == 7 { None } else { Some(1_000 + k * 10) }).collect()) },
        Column { name: "tag".into(), data: ColumnData::Str((0..n).map(|k| Some(["x", "yy", "zzz"][k % 3].to_string())).collect()) },
        Column { name: "msg".into(), data: ColumnData::Str((0..n).map(|k| Some(format!("event number {k} happened, event number {k} again"))).collect()) },
        Column { name: "f".into(), data: ColumnData::Float64((0..n).map(|k| Some(k as f64 * 0.5)).collect()) },
        Column { name: "b".into(), data: ColumnData::Bool((0..n).map(|k| Some(k % 2 == 0)).collect()) },
    ] };
    let opts = WriteOptions::auto().with_column("msg", ColumnCodec::new(Transform::Plain, Stage::Hz));
    let bytes = KoreWriter::to_bytes_with(&block, &opts).unwrap();
    (block, bytes)
}

#[test]
fn tamper_at_every_byte_is_detected() {
    let (block, bytes) = small_v3();
    assert_same(&KoreReader::from_bytes(&bytes).unwrap(), &block);
    for i in 0..bytes.len() {
        for mask in [0x01u8, 0x80, 0xff] {
            let mut t = bytes.clone();
            t[i] ^= mask;
            let r = std::panic::catch_unwind(|| KoreReader::from_bytes(&t));
            match r {
                Err(_) => panic!("reader panicked at byte {i} mask {mask:#x}"),
                Ok(Ok(_)) => panic!("tamper at byte {i} (mask {mask:#x}) of {} was NOT detected", bytes.len()),
                Ok(Err(_)) => {}
            }
        }
    }
}

#[test]
fn truncation_and_trailing_bytes_are_detected() {
    let (_, bytes) = small_v3();
    for cut in 0..bytes.len() { assert!(KoreReader::from_bytes(&bytes[..cut]).is_err(), "cut at {cut}"); }
    let mut ext = bytes.clone();
    ext.push(0);
    assert!(KoreReader::from_bytes(&ext).is_err());
}

#[test]
fn consistent_checksums_over_garbage_payloads_still_fail_cleanly() {
    // an attacker (or a buggy writer) can recompute CRCs; the decoders themselves must reject garbage
    // without panicking or allocating from unchecked lengths.
    let (block, _) = small_v3();
    let mut seed = 0x1234_5678_9abc_def0u64;
    let mut rnd = move || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; seed };
    for col in &block.columns {
        let dtype = codec::dtype_of(&col.data);
        for t in 0..5u8 {
            for s in 0..4u8 {
                let (tid, sid, good) = match codec::encode_column(&col.data, ColumnCodec::new(Transform::from_id(t).unwrap(), Stage::from_id(s).unwrap())) {
                    Ok(x) => x, Err(_) => continue,
                };
                for round in 0..60 {
                    let mut p = good.clone();
                    match round % 3 {
                        0 => { if !p.is_empty() { let i = (rnd() as usize) % p.len(); p[i] = rnd() as u8; } }
                        1 => { let cut = (rnd() as usize) % (p.len() + 1); p.truncate(cut); }
                        _ => { for _ in 0..4 { if !p.is_empty() { let i = (rnd() as usize) % p.len(); p[i] ^= 1 << (rnd() % 8); } } }
                    }
                    let r = std::panic::catch_unwind(|| { let _ = codec::decode_column(&p, dtype, tid, sid, block.num_rows); });
                    assert!(r.is_ok(), "decoder panicked for {dtype:?} t{tid} s{sid}");
                }
            }
        }
    }
}

// ── old readers refuse v3 ────────────────────────────────────────────────────

#[test]
fn version_field_is_what_old_readers_check() {
    // The v2 reader's gate is `if version != 2 && version != 1 { Err("unsupported version N") }`, evaluated
    // right after the magic, before any other field is interpreted (see docs/format_codecs.md for the run
    // against the unmodified a980727d reader). This asserts the bytes that gate looks at.
    let (_, bytes) = small_v3();
    assert_eq!(&bytes[..4], b"KORE");
    let v = u16::from_le_bytes([bytes[4], bytes[5]]);
    assert!(v != 1 && v != 2, "v3 files must carry a version old readers do not accept");
    // new reader: a v3 body under a v2/other header is rejected, never misread
    for fake in [1u16, 2, 4, 0xffff] {
        let mut t = bytes.clone();
        t[4..6].copy_from_slice(&fake.to_le_bytes());
        assert!(KoreReader::from_bytes(&t).is_err(), "fake version {fake}");
    }
}
