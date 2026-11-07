//! Version-4 ("compact") container and shared dictionaries: round trips, projection, checksums and tamper
//! detection for every new object, compatibility (old files still read, v2/v3 writers unchanged, readers
//! of earlier versions refuse v4), time travel with dictionaries, determinism.

use kore_core::{Column, ColumnData, DataBlock};
use kore_store::codec::{ColumnCodec, Stage, Transform, WriteOptions};
use kore_store::{v4, versioned, CompactOptions, DictSet, KoreReader, KoreWriter, SharedDict};
use std::path::PathBuf;

fn fixture(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn strs(c: &ColumnData) -> Vec<Option<String>> {
    match c {
        ColumnData::Str(v) => v.clone(),
        ColumnData::StrDict { codes, dict } => codes.iter().map(|&k| if k == u8::MAX { None } else { Some(dict[k as usize].clone()) }).collect(),
        _ => panic!("not a string column"),
    }
}

fn same_col(a: &ColumnData, b: &ColumnData) -> bool {
    match (a, b) {
        (ColumnData::Float64(x), ColumnData::Float64(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.map(f64::to_bits) == q.map(f64::to_bits)),
        (ColumnData::Int64(x), ColumnData::Int64(y)) => x == y,
        (ColumnData::Bool(x), ColumnData::Bool(y)) => x == y,
        (ColumnData::Str(x), ColumnData::Str(y)) => x == y,
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

fn s(v: &str) -> Option<String> { Some(v.to_string()) }

/// 64 pseudo-random lowercase hex chars (like a sha256 id), splitmix64 of k.
fn hex(k: usize) -> String {
    let mut x = k as u64;
    let mut out = String::new();
    for _ in 0..4 {
        x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        out.push_str(&format!("{:016x}", z ^ (z >> 31)));
    }
    out
}

/// A segment shaped like a Digital Life events segment (ids, text timestamps, low-cardinality strings,
/// comma lists, text), with nulls in a few columns.
fn events(n: usize, seed: usize) -> DataBlock {
    let pool = ["Standup done, nothing blocked.", "Deployed the release to staging and ran the smoke tests.",
        "Talked to Mara about the move; she prefers the office on Tuesdays.", "Battery replaced, the sensor reports again.",
        "Read two chapters of the book about distributed systems."];
    let text: Vec<Option<String>> = (0..n).map(|k| {
        let m = 1 + (k * 7 + seed) % 6;
        let mut t = String::new();
        for j in 0..m { t.push_str(pool[(k * 3 + j + seed) % pool.len()]); t.push(' '); }
        t.push_str(&format!("#{}", k + seed * n));
        Some(t)
    }).collect();
    DataBlock { num_rows: n, columns: vec![
        Column { name: "event_id".into(), data: ColumnData::Str((0..n).map(|k| Some(hex(k + seed * n))).collect()) },
        Column { name: "occurred_at".into(), data: ColumnData::Str((0..n).map(|k| {
            let t = 1_735_689_600 + 30 * (k + seed * n) as i64 + (k as i64 * 7) % 30;
            Some(fmt(t))
        }).collect()) },
        Column { name: "recorded_at".into(), data: ColumnData::Str((0..n).map(|_| s("2026-01-01T00:00:00Z")).collect()) },
        Column { name: "source".into(), data: ColumnData::Str((0..n).map(|k| s(["journal", "chat", "email", "voice"][k % 4])).collect()) },
        Column { name: "kind".into(), data: ColumnData::Str((0..n).map(|k| if k % 17 == 3 { None } else { s(["note", "action", "decision"][(k / 3) % 3]) }).collect()) },
        Column { name: "text".into(), data: ColumnData::Str(text) },
        Column { name: "tags".into(), data: ColumnData::Str((0..n).map(|k| match k % 5 { 0 => s(""), 1 => s("tag01"), 2 => s("tag02,tag07"), 3 => None, _ => s("tag03,tag01,tag09") }).collect()) },
        Column { name: "redacted".into(), data: ColumnData::Bool((0..n).map(|k| Some(k % 11 == 0)).collect()) },
        Column { name: "conf".into(), data: ColumnData::Float64((0..n).map(|k| if k % 9 == 0 { None } else { Some(k as f64 / 100.0) }).collect()) },
        Column { name: "seq".into(), data: ColumnData::Int64((0..n as i64).map(|k| Some(k * 3 - 7)).collect()) },
    ] }
}

// reference formatter for test data (independent of the crate's)
fn fmt(t: i64) -> String {
    let days = t.div_euclid(86_400);
    let rem = t.rem_euclid(86_400);
    // civil from days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn dict_for(block: &DataBlock) -> SharedDict {
    let mut c = Vec::new();
    for t in strs(&block.columns[5].data).into_iter().flatten() { c.extend_from_slice(t.as_bytes()); }
    SharedDict::from_content(&c).unwrap()
}

// ── round trips ──────────────────────────────────────────────────────────────

#[test]
fn compact_auto_round_trips_and_uses_the_new_transforms() {
    let b = events(1000, 0);
    let bytes = KoreWriter::to_bytes_compact(&b, &CompactOptions::new()).unwrap();
    assert!(v4::is_v4(&bytes));
    assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), 4);
    assert_same(&KoreReader::from_bytes(&bytes).unwrap(), &b);
    let info = v4::info(&bytes).unwrap();
    let t = |name: &str| info.columns.iter().find(|c| c.0 == name).unwrap().2;
    assert_eq!(t("event_id"), Transform::HexFixed.id().unwrap());
    assert_eq!(t("occurred_at"), Transform::TsText.id().unwrap());
    assert!(t("tags") == Transform::Tokens.id().unwrap() || t("tags") == Transform::Dict.id().unwrap());
    assert!(info.dict_ids.is_empty());
    // smaller than the default writer and than version 3 auto on the same block
    let v2 = KoreWriter::to_bytes(&b).len();
    let v3 = KoreWriter::to_bytes_with(&b, &WriteOptions::auto()).unwrap().len();
    assert!(bytes.len() < v3 && v3 < v2, "v4 {} v3 {v3} v2 {v2}", bytes.len());
}

#[test]
fn every_explicit_codec_round_trips_including_edge_values() {
    let n = 77;
    let ts: Vec<Option<String>> = (0..n).map(|k| match k % 6 {
        0 => None, 1 => s("0000-01-01T00:00:00Z"), 2 => s("9999-12-31T23:59:59Z"), 3 => s("1970-01-01T00:00:00Z"),
        4 => s("2024-02-29T12:00:00Z"), _ => s("1969-12-31T23:59:59Z"),
    }).collect();
    let toks: Vec<Option<String>> = (0..n).map(|k| match k % 8 {
        0 => None, 1 => s(""), 2 => s(","), 3 => s("a,,b"), 4 => s(",x"), 5 => s("y,"), 6 => s("ü,日本"), _ => s("a"),
    }).collect();
    let block = DataBlock { num_rows: n, columns: vec![
        Column { name: "ts".into(), data: ColumnData::Str(ts.clone()) },
        Column { name: "tok".into(), data: ColumnData::Str(toks.clone()) },
    ] };
    for stage in [Stage::None, Stage::Lz4, Stage::Hz, Stage::Zstd, Stage::Auto] {
        for (col, t) in [("ts", Transform::TsText), ("ts", Transform::Plain), ("ts", Transform::Dict), ("tok", Transform::Tokens), ("tok", Transform::Plain), ("tok", Transform::Dict)] {
            let o = CompactOptions::new().with_column(col, ColumnCodec::new(t, stage));
            let bytes = KoreWriter::to_bytes_compact(&block, &o).unwrap();
            assert_same(&KoreReader::from_bytes(&bytes).unwrap(), &block);
        }
    }
    // the generic types and an all-null / empty block
    let b = events(64, 3);
    for stage in [Stage::None, Stage::Lz4, Stage::Hz, Stage::Zstd] {
        let o = CompactOptions { codecs: WriteOptions { default: ColumnCodec::new(Transform::Auto, stage), columns: vec![] }, ..CompactOptions::new() };
        assert_same(&KoreReader::from_bytes(&KoreWriter::to_bytes_compact(&b, &o).unwrap()).unwrap(), &b);
    }
    let empty = DataBlock { num_rows: 0, columns: vec![Column { name: "x".into(), data: ColumnData::Str(vec![]) }] };
    assert_same(&KoreReader::from_bytes(&KoreWriter::to_bytes_compact(&empty, &CompactOptions::new()).unwrap()).unwrap(), &empty);
    let nulls = DataBlock { num_rows: 5, columns: vec![Column { name: "x".into(), data: ColumnData::Str(vec![None; 5]) }] };
    assert_same(&KoreReader::from_bytes(&KoreWriter::to_bytes_compact(&nulls, &CompactOptions::new()).unwrap()).unwrap(), &nulls);
}

#[test]
fn non_canonical_timestamps_are_refused_explicitly_and_kept_verbatim_under_auto() {
    for bad in ["2026-01-01 00:00:00Z", "2026-02-30T00:00:00Z", "2026-01-01T00:00:60Z", "2026-1-01T00:00:00Z", "+026-01-01T00:00:00Z", "2026-01-01T00:00:00z"] {
        let block = DataBlock { num_rows: 2, columns: vec![Column { name: "t".into(), data: ColumnData::Str(vec![s("2026-01-01T00:00:00Z"), s(bad)]) }] };
        let o = CompactOptions::new().with_column("t", ColumnCodec::new(Transform::TsText, Stage::None));
        assert!(KoreWriter::to_bytes_compact(&block, &o).is_err(), "{bad}");
        let bytes = KoreWriter::to_bytes_compact(&block, &CompactOptions::new()).unwrap();
        assert_same(&KoreReader::from_bytes(&bytes).unwrap(), &block);
    }
}

#[test]
fn version_3_writer_refuses_the_v4_only_transforms() {
    let b = events(10, 0);
    for t in [Transform::TsText, Transform::Tokens] {
        let o = WriteOptions::auto().with_column("occurred_at", ColumnCodec::new(t, Stage::None));
        let e = KoreWriter::to_bytes_with(&b, &o).unwrap_err().to_string();
        assert!(e.contains("version-4"), "{e}");
    }
    // and version 3 never stores the new ids under Auto
    let v3 = KoreWriter::to_bytes_with(&b, &WriteOptions::auto()).unwrap();
    assert_eq!(u16::from_le_bytes([v3[4], v3[5]]), 3);
    assert!(Transform::from_id(5).is_err() && Transform::from_id(6).is_err());
}

// ── shared dictionaries ──────────────────────────────────────────────────────

#[test]
fn shared_dictionary_round_trip_smaller_and_required() {
    let first = events(1000, 0);
    let d = dict_for(&first);
    let later = events(1000, 1);
    let o = CompactOptions::new().with_dict("text", d.clone());
    let with = KoreWriter::to_bytes_compact(&later, &o).unwrap();
    let without = KoreWriter::to_bytes_compact(&later, &CompactOptions::new()).unwrap();
    assert!(with.len() < without.len(), "dict {} vs none {}", with.len(), without.len());
    let info = v4::info(&with).unwrap();
    assert_eq!(info.dict_ids, vec![d.id()]);
    let ds = DictSet::new().with(d.clone());
    assert_same(&KoreReader::from_bytes_with_dicts(&with, &ds).unwrap(), &later);
    // without the dictionary: a clear error naming the dictionary
    let e = KoreReader::from_bytes(&with).unwrap_err().to_string();
    assert!(e.contains("needs shared dictionary") && e.contains(&d.id_hex()), "{e}");
    // serialized object round trip
    let back = SharedDict::from_bytes(d.to_bytes()).unwrap();
    assert_eq!(back.id(), d.id());
    assert_same(&KoreReader::from_bytes_with_dicts(&with, &DictSet::new().with(back)).unwrap(), &later);
    // a different, valid dictionary is not accepted in its place
    let other = SharedDict::from_content(b"some other content entirely").unwrap();
    assert!(KoreReader::from_bytes_with_dicts(&with, &DictSet::new().with(other)).is_err());
    // trained dictionaries work the same way
    let samples: Vec<Vec<u8>> = strs(&first.columns[5].data).into_iter().flatten().map(|t| t.into_bytes()).collect();
    let tr = SharedDict::train(&samples, 16 << 10).unwrap();
    let wt = KoreWriter::to_bytes_compact(&later, &CompactOptions::new().with_dict("text", tr.clone())).unwrap();
    assert_same(&KoreReader::from_bytes_with_dicts(&wt, &DictSet::new().with(tr)).unwrap(), &later);
}

#[test]
fn projection_reads_only_needed_columns_and_needs_only_their_dictionaries() {
    let b = events(500, 2);
    let d = dict_for(&events(500, 0));
    let bytes = KoreWriter::to_bytes_compact(&b, &CompactOptions::new().with_dict("text", d.clone())).unwrap();
    assert!(!v4::info(&bytes).unwrap().dict_ids.is_empty());
    // columns that do not use the dictionary read with the plain API
    let p = KoreReader::from_bytes_columns(&bytes, &["kind", "event_id"]).unwrap();
    assert_eq!(p.columns.len(), 2);
    assert_eq!(p.columns[0].name, "kind");
    assert!(same_col(&p.columns[0].data, &b.columns[4].data));
    assert!(same_col(&p.columns[1].data, &b.columns[0].data));
    assert!(KoreReader::from_bytes_columns(&bytes, &["text"]).is_err());
    let ds = DictSet::new().with(d);
    let t = KoreReader::from_bytes_columns_with_dicts(&bytes, &["text"], &ds).unwrap();
    assert!(same_col(&t.columns[0].data, &b.columns[5].data));
    assert!(KoreReader::from_bytes_columns_with_dicts(&bytes, &["nope"], &ds).is_err());
    // a corrupted payload of an unrequested column does not affect a projection (it is not read),
    // but a full read reports it
    let info = v4::info(&bytes).unwrap();
    let payload_start = bytes.len() - info.columns.iter().map(|c| c.5).sum::<usize>();
    let mut bad = bytes.clone();
    bad[payload_start + 1] ^= 0x40; // inside event_id's payload (first column)
    assert!(KoreReader::from_bytes_columns(&bad, &["kind"]).is_ok());
    assert!(KoreReader::from_bytes_with_dicts(&bad, &ds).is_err());
}

// ── checksums / tamper ───────────────────────────────────────────────────────

#[test]
fn tamper_at_every_byte_of_a_v4_file_is_detected() {
    let b = events(40, 0);
    let d = SharedDict::from_content(b"Standup done, nothing blocked. Deployed the release to staging").unwrap();
    let o = CompactOptions::new().with_dict("text", d.clone()).with_column("text", ColumnCodec::new(Transform::Plain, Stage::Zstd));
    let bytes = KoreWriter::to_bytes_compact(&b, &o).unwrap();
    let ds = DictSet::new().with(d);
    assert_same(&KoreReader::from_bytes_with_dicts(&bytes, &ds).unwrap(), &b);
    for i in 0..bytes.len() {
        for mask in [0x01u8, 0x80, 0xff] {
            let mut t = bytes.clone();
            t[i] ^= mask;
            assert!(KoreReader::from_bytes_with_dicts(&t, &ds).is_err(), "flip {mask:#x} at byte {i} of {} not detected", bytes.len());
        }
    }
    for cut in 0..bytes.len() { assert!(KoreReader::from_bytes_with_dicts(&bytes[..cut], &ds).is_err(), "truncated at {cut}"); }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(KoreReader::from_bytes_with_dicts(&longer, &ds).is_err());
}

#[test]
fn tamper_at_every_byte_of_a_dictionary_object_is_detected() {
    let d = SharedDict::from_content(b"recurring sentences of the first segment, stored once").unwrap();
    let obj = d.to_bytes().to_vec();
    assert_eq!(&obj[..4], b"KDIC");
    for i in 0..obj.len() {
        for mask in [0x01u8, 0x80, 0xff] {
            let mut t = obj.clone();
            t[i] ^= mask;
            assert!(SharedDict::from_bytes(&t).is_err(), "flip {mask:#x} at byte {i}");
        }
    }
    for cut in 0..obj.len() { assert!(SharedDict::from_bytes(&obj[..cut]).is_err()); }
    // a dictionary file whose name is not its id is refused by load_dir
    let dir = std::env::temp_dir().join(format!("kore_kdic_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(d.file_name()), &obj).unwrap();
    assert_eq!(DictSet::load_dir(&dir).unwrap().len(), 1);
    std::fs::write(dir.join(format!("{}.kdic", "0".repeat(64))), &obj).unwrap();
    assert!(DictSet::load_dir(&dir).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn consistent_checksums_over_garbage_payloads_fail_cleanly() {
    // payload bytes replaced, CRCs and directory CRC recomputed: decoders must error, not panic
    let b = events(30, 1);
    let bytes = KoreWriter::to_bytes_compact(&b, &CompactOptions::new()).unwrap();
    let info = v4::info(&bytes).unwrap();
    let total: usize = info.columns.iter().map(|c| c.5).sum();
    let header_len = bytes.len() - total - 4;
    let mut x = 12345u64;
    for round in 0..200 {
        let mut t = bytes.clone();
        for k in header_len + 4..t.len() {
            x ^= x << 13; x ^= x >> 7; x ^= x << 17;
            if (x % 5) as usize == round % 5 { t[k] = x as u8; }
        }
        // recompute every payload CRC and the directory CRC
        let mut pos = 4 + 2 + 4 + 8 + 2; // no dictionaries in this file
        let mut off = header_len + 4;
        for c in &info.columns {
            pos += 2 + c.0.len() + 4 + 8;
            let crc = kore_store::compress::crc32(&t[off..off + c.5]);
            t[pos..pos + 4].copy_from_slice(&crc.to_le_bytes());
            pos += 4;
            off += c.5;
        }
        let dc = kore_store::compress::crc32(&t[..header_len]);
        t[header_len..header_len + 4].copy_from_slice(&dc.to_le_bytes());
        let _ = KoreReader::from_bytes(&t); // must return, Ok or Err
    }
}

// ── compatibility ────────────────────────────────────────────────────────────

#[test]
fn readers_of_earlier_versions_stop_at_the_version_field() {
    let bytes = KoreWriter::to_bytes_compact(&events(20, 0), &CompactOptions::new()).unwrap();
    // the version-3 reader (as shipped in 6e2e6a7b) refuses it
    let e = kore_store::v3::read(&bytes, None).unwrap_err().to_string();
    assert!(e.contains("unsupported version 4"), "{e}");
    // a v4 body presented as version 3 is not misread
    let mut fake = bytes.clone();
    fake[4..6].copy_from_slice(&3u16.to_le_bytes());
    assert!(KoreReader::from_bytes(&fake).is_err());
    // the v4 reader refuses other versions
    let v3 = KoreWriter::to_bytes_with(&events(20, 0), &WriteOptions::auto()).unwrap();
    assert!(v4::read(&v3, None, &DictSet::new()).is_err());
}

#[test]
fn files_of_earlier_writers_still_read_and_writers_are_unchanged() {
    // written by the unmodified 6e2e6a7b code (examples/write_v3_sample.rs there)
    let v2 = fixture("sample_v2_6e2e6a7b.kore");
    let v3 = fixture("sample_v3_6e2e6a7b.kore");
    let a = KoreReader::from_bytes(&v2).unwrap();
    let b = KoreReader::from_bytes(&v3).unwrap();
    assert_eq!(a.num_rows, 200);
    for (x, y) in a.columns.iter().zip(&b.columns) {
        assert_eq!(x.name, y.name);
        match (&x.data, &y.data) {
            (ColumnData::Int64(p), ColumnData::Int64(q)) => assert_eq!(p, q),
            _ => assert_eq!(strs(&x.data), strs(&y.data)),
        }
    }
    // both writers reproduce the old bytes exactly
    let block = DataBlock { num_rows: 200, columns: b.columns.clone() };
    assert_eq!(KoreWriter::to_bytes(&block), v2, "default writer output changed");
    assert_eq!(KoreWriter::to_bytes_with(&block, &WriteOptions::auto()).unwrap(), v3, "version-3 writer output changed");
}

// ── versioning / time travel ─────────────────────────────────────────────────

#[test]
fn version_log_with_v4_entries_time_travels_with_dictionaries() {
    let d1 = dict_for(&events(300, 0));
    let d2 = dict_for(&events(300, 5)); // a later, different dictionary: old versions keep the old one
    let v1 = events(300, 1);
    let v2 = events(310, 2);
    let v3 = events(320, 3);
    let log = versioned::append_version(None, &v1, 100).unwrap(); // a default v2 entry
    let e2 = KoreWriter::to_bytes_compact(&v2, &CompactOptions::new().with_dict("text", d1.clone())).unwrap();
    let log = versioned::append_raw(Some(&log), &e2, 200).unwrap();
    let e3 = KoreWriter::to_bytes_compact(&v3, &CompactOptions::new().with_dict("text", d2.clone())).unwrap();
    let log = versioned::append_raw(Some(&log), &e3, 300).unwrap();
    assert_eq!(versioned::list_versions(&log).unwrap(), vec![(100, 300), (200, 310), (300, 320)]);
    let ds = DictSet::new().with(d1.clone()).with(d2.clone());
    assert_same(&materialize(versioned::read_at_with_dicts(&log, 150, &ds).unwrap()), &v1);
    assert_same(&versioned::read_at_with_dicts(&log, 250, &ds).unwrap(), &v2);
    assert_same(&versioned::read_latest_with_dicts(&log, &ds).unwrap(), &v3);
    // reading version 2 needs d1 only; version 3 needs d2
    assert!(versioned::read_at_with_dicts(&log, 250, &DictSet::new().with(d1.clone())).is_ok());
    assert!(versioned::read_at_with_dicts(&log, 250, &DictSet::new().with(d2.clone())).is_err());
    // projections of dictionary-free columns at any point in time need no dictionaries
    let k = versioned::read_at_columns_with_dicts(&log, 250, &["kind"], &DictSet::new()).unwrap();
    assert!(same_col(&k.columns[0].data, &v2.columns[4].data));
    // plain read of the latest version (uses d2) is refused clearly
    assert!(KoreReader::from_bytes(&log).unwrap_err().to_string().contains("needs shared dictionary"));
}

fn materialize(b: DataBlock) -> DataBlock {
    let cols = b.columns.into_iter().map(|c| match &c.data {
        ColumnData::StrDict { .. } => Column { name: c.name.clone(), data: ColumnData::Str(strs(&c.data)) },
        _ => c,
    }).collect();
    DataBlock { columns: cols, num_rows: b.num_rows }
}

// ── determinism ──────────────────────────────────────────────────────────────

#[test]
fn output_is_deterministic_across_runs_and_thread_counts() {
    let b = events(2000, 4);
    let d = dict_for(&events(1000, 0));
    let o = CompactOptions::new().with_dict("text", d);
    let first = KoreWriter::to_bytes_compact(&b, &o).unwrap();
    assert_eq!(first, KoreWriter::to_bytes_compact(&b, &o).unwrap());
    for threads in [1usize, 2, 7] {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
        assert_eq!(pool.install(|| KoreWriter::to_bytes_compact(&b, &o).unwrap()), first, "{threads} threads");
    }
}
