//! Format sync (docs/format_sync.md): files written by the kore-store of branch dev (d9edbcfc, before the
//! sync) in `tests/fixtures_dev` read with the current reader to exactly the values that were written; the
//! current writer round-trips the same data and writes the same bytes as dev's writer did.
//!
//! `store/` was written by `examples/gen_dev_fixtures.rs`, `kore-self/` by the ignored kore-self test
//! `tests_format_sync::gen_dev_format_fixtures`, both with dev's code; `SHA256SUMS` pins every file.

use kore_core::DataBlock;
use kore_store::{rowgroups, versioned, CompactOptions, DictSet, KoreReader, KoreWriter, SharedDict};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[path = "common/dev_data.rs"]
mod dev_data;
use dev_data::*;

fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures_dev") }

fn read(rel: &str) -> Vec<u8> { std::fs::read(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}")) }

fn walk(base: &Path, d: &Path, out: &mut Vec<String>) {
    let mut v: Vec<_> = std::fs::read_dir(d).unwrap().flatten().map(|e| e.path()).collect();
    v.sort();
    for p in v {
        if p.is_dir() { walk(base, &p, out); } else { out.push(p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/")); }
    }
}

fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }

/// The fixture files are the ones dev wrote: every file listed with its SHA-256, nothing extra.
#[test]
fn fixtures_are_unchanged() {
    let sums = String::from_utf8(read("SHA256SUMS")).unwrap();
    let mut listed = vec![];
    for line in sums.lines().filter(|l| !l.is_empty()) {
        let (h, name) = line.split_once("  ").expect("'<sha256>  <path>'");
        assert_eq!(hex(&Sha256::digest(read(name))), h, "{name} changed since dev wrote it");
        listed.push(name.to_string());
    }
    let mut on_disk = vec![];
    walk(&root(), &root(), &mut on_disk);
    on_disk.retain(|n| n != "SHA256SUMS");
    listed.sort();
    assert_eq!(listed, on_disk, "SHA256SUMS lists exactly the fixture files");
    assert!(listed.len() > 150, "{}", listed.len());
}

fn store_dicts() -> (SharedDict, DictSet) {
    let d = SharedDict::from_content(&dict_content()).unwrap();
    let mut s = DictSet::new();
    s.add(d.clone());
    (d, s)
}

/// (file, written values, needs the dictionary)
fn store_cases() -> Vec<(&'static str, DataBlock, bool)> {
    vec![
        ("all_types.kore", all_types(), false), ("json_cols.kore", json_cols(), false), ("empty_rows.kore", empty_rows(), false),
        ("no_columns.kore", no_columns(), false), ("all_null.kore", all_null(), false), ("large.kore", large(), false),
        ("all_types_wf.kore", all_types(), false), ("json_cols_wf.kore", json_cols(), false),
        ("large_rg2048.kore", large(), false), ("all_types_rg128.kore", all_types(), false),
        ("versionlog.kore", json_cols(), false),
        ("v4_all_types.kore", all_types(), false), ("v4_json_cols.kore", json_cols(), false), ("v4_empty_rows.kore", empty_rows(), false),
        ("v4_all_null.kore", all_null(), false), ("v4_large.kore", large(), false), ("v4_dict.kore", dict_texts(), true),
    ]
}

#[test]
fn every_dev_store_file_reads_to_the_written_values() {
    let (d, dicts) = store_dicts();
    assert_eq!(read(&format!("store/{}.kdic", d.id_hex())), d.to_bytes(), "the dictionary object is the one dev wrote");
    let loaded = SharedDict::read_file(&root().join("store").join(format!("{}.kdic", d.id_hex()))).unwrap();
    assert_eq!(loaded.id(), d.id());
    for (name, want, needs_dict) in store_cases() {
        let bytes = read(&format!("store/{name}"));
        let got = KoreReader::from_bytes_with_dicts(&bytes, &dicts).unwrap_or_else(|e| panic!("{name}: {e}"));
        same_values(&want, &got).unwrap_or_else(|e| panic!("{name}: {e}"));
        match KoreReader::from_bytes(&bytes) {
            Ok(b) => { assert!(!needs_dict, "{name}"); same_values(&want, &b).unwrap_or_else(|e| panic!("{name} (no dicts): {e}")); }
            Err(e) => assert!(needs_dict && e.to_string().contains("dictionary"), "{name}: {e}"),
        }
        // the file-path reader too
        if !needs_dict {
            let f = KoreReader::read_file(&root().join("store").join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
            same_values(&want, &f).unwrap_or_else(|e| panic!("{name} (read_file): {e}"));
        }
        if rowgroups::is_row_group_file(&bytes) {
            same_values(&want, &rowgroups::read(&bytes, None, None).unwrap()).unwrap_or_else(|e| panic!("{name} (rowgroups::read): {e}"));
        }
    }
    // every version of the version log
    let log = read("store/versionlog.kore");
    let vs = versioned::list_versions(&log).unwrap();
    assert_eq!(vs.iter().map(|v| v.0).collect::<Vec<_>>(), vec![10, 20, 30]);
    for (ts, want) in [(10, all_types()), (20, empty_rows()), (30, json_cols())] {
        same_values(&want, &versioned::read_at(&log, ts).unwrap()).unwrap_or_else(|e| panic!("version {ts}: {e}"));
    }
}

/// Files written by kore-self with dev's kore-store (log segments, index run, checkpoints, brain files)
/// decode to what dev decoded (`kore-self/kore-self.expect`, "== file" blocks).
#[test]
fn every_dev_kore_self_file_reads_as_dev_read_it() {
    let expect = String::from_utf8(read("kore-self/kore-self.expect")).unwrap().replace("\r\n", "\n");
    let mut blocks: Vec<(String, String)> = vec![];
    for part in expect.split("== file ").skip(1) {
        let part = part.split("\n== ").next().unwrap();
        let (head, rest) = part.split_once('\n').unwrap();
        let (name, kind) = head.rsplit_once(' ').unwrap();
        blocks.push((name.to_string(), format!("{kind}\n{rest}{}", if rest.is_empty() || rest.ends_with('\n') { "" } else { "\n" })));
    }
    assert_eq!(blocks.len(), 179);
    let mut kinds = std::collections::BTreeMap::new();
    for (name, want) in &blocks {
        let p = root().join("kore-self").join(name);
        let log = name.split('/').next().unwrap();
        let mut dicts = DictSet::new();
        let ddir = root().join("kore-self").join(log).join("tester").join("log").join("dicts");
        if ddir.exists() { dicts = DictSet::load_dir(&ddir).unwrap(); }
        let b = std::fs::read(&p).unwrap();
        let got = if rowgroups::is_row_group_file(&b) {
            match rowgroups::read(&b, None, None) { Ok(x) => format!("rowgroups\n{}", digest(&x)), Err(_) => "rowgroups-unreadable\n".into() }
        } else {
            match KoreReader::from_bytes_with_dicts(&b, &dicts) { Ok(x) => format!("kore\n{}", digest(&x)), Err(_) => "not-a-kore-store-file\n".into() }
        };
        assert_eq!(&got, want, "{name}");
        *kinds.entry(want.lines().next().unwrap().to_string()).or_insert(0) += 1;
    }
    // 73 kore-store files (v4 segments, checkpoints, brain files), 1 index run, 105 kore-self codec segments
    assert_eq!(kinds.get("kore"), Some(&73));
    assert_eq!(kinds.get("rowgroups"), Some(&1));
    assert_eq!(kinds.get("not-a-kore-store-file"), Some(&105));
}

/// The current writer: every dataset round-trips (default and version-4), and the bytes are those dev wrote.
#[test]
fn writer_round_trip_and_same_bytes_as_dev() {
    let (d, dicts) = store_dicts();
    for (name, b, _) in store_cases() {
        for (how, bytes) in [("to_bytes", KoreWriter::to_bytes(&b)),
                             ("compact", KoreWriter::to_bytes_compact(&b, &CompactOptions::new()).unwrap())] {
            let got = KoreReader::from_bytes(&bytes).unwrap_or_else(|e| panic!("{name} {how}: {e}"));
            same_values(&b, &got).unwrap_or_else(|e| panic!("{name} {how}: {e}"));
        }
        let _ = name;
    }
    let bytes = KoreWriter::to_bytes_compact(&dict_texts(), &CompactOptions::new().with_dict("text", d.clone())).unwrap();
    same_values(&dict_texts(), &KoreReader::from_bytes_with_dicts(&bytes, &dicts).unwrap()).unwrap();

    // same bytes as dev's writer, per API the brain uses
    let tmp = std::env::temp_dir().join(format!("kore_fsync_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let mut differ = vec![];
    let mut cmp = |name: &str, now: Vec<u8>| if now != read(&format!("store/{name}")) { differ.push(name.to_string()); };
    for (n, b) in [("all_types", all_types()), ("json_cols", json_cols()), ("empty_rows", empty_rows()), ("no_columns", no_columns()), ("all_null", all_null()), ("large", large())] {
        cmp(&format!("{n}.kore"), KoreWriter::to_bytes(&b));
    }
    for (n, b) in [("all_types_wf", all_types()), ("json_cols_wf", json_cols())] {
        let p = tmp.join(format!("{n}.kore"));
        KoreWriter::write_file(&p, &b).unwrap();
        cmp(&format!("{n}.kore"), std::fs::read(&p).unwrap());
    }
    let _ = std::fs::remove_dir_all(&tmp);
    cmp("large_rg2048.kore", rowgroups::write(&large(), 2048).unwrap());
    cmp("all_types_rg128.kore", rowgroups::write(&all_types(), 128).unwrap());
    let v1 = versioned::append_version(None, &all_types(), 10).unwrap();
    let v2 = versioned::append_version(Some(&v1), &empty_rows(), 20).unwrap();
    cmp("versionlog.kore", versioned::append_version(Some(&v2), &json_cols(), 30).unwrap());
    for (n, b) in [("v4_all_types", all_types()), ("v4_json_cols", json_cols()), ("v4_empty_rows", empty_rows()), ("v4_all_null", all_null()), ("v4_large", large())] {
        cmp(&format!("{n}.kore"), KoreWriter::to_bytes_compact(&b, &CompactOptions::new()).unwrap());
    }
    cmp("v4_dict.kore", bytes);
    assert!(differ.is_empty(), "the current writer writes other bytes than dev's for: {differ:?}");
}
