//! Release compatibility gate, direction "old files open with the new code".
//!
//! Every committed `.kore` file in `tests/fixtures`, `tests/fixtures/dl_log4` and `../conformance`
//! was written by an older version of the writer (v2 samples, row groups, version logs, the events
//! fixtures of commit a980727d, the Digital Life format-4 segments, rule mode v6, the conformance
//! set). This test opens each one with the current reader and compares the decoded data with the
//! digest pinned in `tests/fixtures/old_files.digest` (per column: type, null count, FNV-1a 64 of the
//! canonical row encoding, the same encoding as `conformance.rs`). Version logs also pin every
//! version. A new fixture without a pinned digest fails the test, so it cannot be skipped silently.
//!
//! The digest file is also read by `kore-python/compat_check.py`, so the Python module must decode
//! the same files to the same data.
//!
//! Re-pin (only when a fixture is added on purpose, never to hide a decode change):
//!   KORE_REGEN_OLD_FILES=1 cargo test -p kore-store --test old_files

use kore_core::{ColumnData, DataBlock};
use kore_store::{versioned, DictSet, KoreReader};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const PASSWORD: &[u8] = b"kore-conformance";

fn base() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")) }

fn oracle_path() -> PathBuf { base().join("tests").join("fixtures").join("old_files.digest") }

/// (repo-relative name with forward slashes, path) of every committed fixture, sorted.
fn fixture_files() -> Vec<(String, PathBuf)> {
    let b = base();
    let mut out = vec![];
    for (prefix, dir) in [
        ("kore-store/tests/fixtures", b.join("tests").join("fixtures")),
        ("kore-store/tests/fixtures/dl_log4", b.join("tests").join("fixtures").join("dl_log4")),
        ("conformance", b.join("..").join("conformance")),
    ] {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .flatten().map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().map_or(false, |e| e == "kore"))
            .collect();
        files.sort();
        for f in files {
            let name = format!("{prefix}/{}", f.file_name().unwrap().to_string_lossy());
            out.push((name, f));
        }
    }
    out
}

fn fnv(h: &mut u64, bytes: &[u8]) {
    for &b in bytes { *h ^= b as u64; *h = h.wrapping_mul(0x100000001b3); }
}

fn digest(b: &DataBlock) -> String {
    let mut out = format!("rows {}\n", b.num_rows);
    for c in &b.columns {
        let mut h = 0xcbf29ce484222325u64;
        let mut nulls = 0usize;
        let mut put = |h: &mut u64, v: Option<Vec<u8>>| match v {
            None => { nulls += 1; fnv(h, &[0]); }
            Some(bytes) => { fnv(h, &[1]); fnv(h, &bytes); }
        };
        let s = |x: &str| [&(x.len() as u32).to_le_bytes()[..], x.as_bytes()].concat();
        let ty = match &c.data {
            ColumnData::Int64(v) => { for x in v { put(&mut h, x.map(|x| x.to_le_bytes().to_vec())); } "i64" }
            ColumnData::Float64(v) => {
                for x in v { put(&mut h, x.map(|x| (if x.is_nan() { 0x7FF8_0000_0000_0001u64 } else { x.to_bits() }).to_le_bytes().to_vec())); }
                "f64"
            }
            ColumnData::Bool(v) => { for x in v { put(&mut h, x.map(|x| vec![x as u8])); } "bool" }
            ColumnData::Str(v) => { for x in v { put(&mut h, x.as_deref().map(s)); } "str" }
            ColumnData::StrDict { codes, dict } => {
                for &code in codes {
                    put(&mut h, if code == u8::MAX { None } else { Some(s(&dict[code as usize])) });
                }
                "str"
            }
        };
        out += &format!("{} {ty} {nulls} {h:016x}\n", c.name);
    }
    out
}

/// The pinned text for one file: "== <name>[ needs-dicts][ encrypted]" then the latest-version
/// digest, then "-- version <ts>" + digest for every entry of a version log.
fn describe(name: &str, data: &[u8], dicts: &DictSet) -> String {
    let encrypted = name.ends_with("/encrypted.kore");
    let latest = if encrypted {
        KoreReader::from_bytes_decrypt(data, PASSWORD)
    } else {
        KoreReader::from_bytes_with_dicts(data, dicts)
    }.unwrap_or_else(|e| panic!("{name}: the current reader cannot open this older file: {e}"));

    // Without the shared dictionaries a file either reads to the same data or is refused with a
    // clear message; it never decodes to something else.
    let mut flags = String::new();
    if encrypted {
        flags += " encrypted";
        assert!(KoreReader::from_bytes(data).is_err(), "{name}: encrypted file read without a password");
    } else {
        match KoreReader::from_bytes(data) {
            Ok(b) => assert_eq!(digest(&b), digest(&latest), "{name}: decodes differently without dictionaries"),
            Err(e) => {
                let msg = e.to_string();
                assert!(msg.contains("dictionary"), "{name}: unclear error without dictionaries: {msg}");
                flags += " needs-dicts";
            }
        }
    }

    let mut out = format!("== {name}{flags}\n{}", digest(&latest));
    if !encrypted && versioned::is_version_log(data) {
        let versions = versioned::list_versions(data).unwrap_or_else(|e| panic!("{name}: version list: {e}"));
        assert!(!versions.is_empty(), "{name}: empty version log");
        for (ts, _) in versions {
            let b = versioned::read_at_with_dicts(data, ts, dicts).unwrap_or_else(|e| panic!("{name} version {ts}: {e}"));
            out += &format!("-- version {ts}\n{}", digest(&b));
        }
    }
    out
}

fn parse_oracle(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let mut cur: Option<(String, String)> = None;
    for line in text.replace("\r\n", "\n").lines() {
        if line.starts_with('#') || line.is_empty() { continue; }
        if let Some(rest) = line.strip_prefix("== ") {
            if let Some((k, v)) = cur.take() { map.insert(k, v); }
            let name = rest.split(' ').next().unwrap().to_string();
            cur = Some((name, format!("{line}\n")));
        } else if let Some((_, v)) = cur.as_mut() {
            *v += line;
            *v += "\n";
        }
    }
    if let Some((k, v)) = cur.take() { map.insert(k, v); }
    map
}

const HEADER: &str = "\
# Pinned decode of every committed .kore fixture (written by older versions of the writer).
# Checked by kore-store/tests/old_files.rs (Rust) and kore-python/compat_check.py (Python).
# Per file: '== <path>[ needs-dicts][ encrypted]', the latest-version digest
# ('rows N' + '<column> <type> <nulls> <fnv64>'), and '-- version <ts>' blocks for version logs.
# Re-pin only when adding a fixture on purpose: KORE_REGEN_OLD_FILES=1 cargo test -p kore-store --test old_files
";

#[test]
fn every_committed_old_file_opens_and_decodes_to_the_pinned_data() {
    let dicts = DictSet::load_dir(&base().join("tests").join("fixtures").join("dl_log4")).unwrap();
    let files = fixture_files();
    assert!(files.len() >= 30, "fixtures found: {}", files.len());

    let mut got = BTreeMap::new();
    for (name, path) in &files {
        let data = std::fs::read(path).unwrap();
        got.insert(name.clone(), describe(name, &data, &dicts));
    }

    if std::env::var("KORE_REGEN_OLD_FILES").map(|v| v == "1").unwrap_or(false) {
        let mut text = HEADER.to_string();
        for v in got.values() { text += v; }
        std::fs::write(oracle_path(), text).unwrap();
    }

    let want = parse_oracle(&std::fs::read_to_string(oracle_path())
        .expect("tests/fixtures/old_files.digest missing (KORE_REGEN_OLD_FILES=1 to create it)"));
    for (name, text) in &got {
        let pinned = want.get(name).unwrap_or_else(|| panic!(
            "{name} has no pinned digest in tests/fixtures/old_files.digest; pin it with KORE_REGEN_OLD_FILES=1"));
        assert_eq!(text, pinned, "{name}: the current reader decodes this older file differently than before");
    }
    for name in want.keys() {
        assert!(got.contains_key(name), "{name} is pinned but the fixture file is gone");
    }
}

#[test]
fn oracle_is_parseable_and_nonempty() {
    let text = std::fs::read_to_string(oracle_path()).unwrap_or_default();
    if text.is_empty() { return; } // created by the test above on first run with KORE_REGEN_OLD_FILES=1
    let map = parse_oracle(&text);
    assert!(map.len() >= 30, "pinned files: {}", map.len());
    assert!(map.keys().all(|k| Path::new(k).extension().map_or(false, |e| e == "kore")));
}
