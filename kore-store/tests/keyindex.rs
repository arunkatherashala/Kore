//! Key index objects (`keyindex`): round trips over version 2/3/4 files, lookups including missing keys
//! and duplicates, tamper and truncation at every byte of the index, pinned data files, refusal by
//! readers that do not know the object, old files still indexed and read, time travel.

use kore_core::{Column, ColumnData, DataBlock};
use kore_store::keyindex::{self, IndexedFile, KeyIndex, KeyIndexBuilder, Verify};
use kore_store::{versioned, CompactOptions, DictSet, KoreReader, KoreWriter, SharedDict, WriteOptions};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;

fn fixture(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// 64 pseudo-random lowercase hex chars (like a sha256 id).
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

fn events(n: usize, seed: usize) -> DataBlock {
    DataBlock { num_rows: n, columns: vec![
        Column { name: "event_id".into(), data: ColumnData::Str((0..n).map(|k| Some(hex(k + seed * n))).collect()) },
        Column { name: "kind".into(), data: ColumnData::Str((0..n).map(|k| Some(["note", "action", "decision"][k % 3].to_string())).collect()) },
        Column { name: "text".into(), data: ColumnData::Str((0..n).map(|k| Some(format!("event {} of segment {seed}: some words about the day", k + seed * n))).collect()) },
        Column { name: "seq".into(), data: ColumnData::Int64((0..n as i64).map(|k| Some(k + (seed * n) as i64)).collect()) },
    ] }
}

fn text_of(h: &keyindex::Hit) -> String { h.values.columns[0].data.get_str(0).unwrap().to_string() }

struct Store { files: HashMap<String, Vec<u8>>, dicts: DictSet, blocks: Vec<(String, DataBlock)> }

/// Six files: default (v2), v3, v4, v4 with a shared dictionary, and two more v4.
fn store() -> Store {
    let d = SharedDict::from_content(b"some words about the day event of segment").unwrap();
    let mut files = HashMap::new();
    let mut blocks = vec![];
    for seed in 0..6 {
        let b = events(150 + seed * 7, seed);
        let bytes = match seed {
            0 => KoreWriter::to_bytes(&b),
            1 => KoreWriter::to_bytes_with(&b, &WriteOptions::auto()).unwrap(),
            3 => KoreWriter::to_bytes_compact(&b, &CompactOptions::new().with_dict("text", d.clone())).unwrap(),
            _ => KoreWriter::to_bytes_compact(&b, &CompactOptions::new()).unwrap(),
        };
        let name = format!("events/{seed:06}.kore");
        files.insert(name.clone(), bytes);
        blocks.push((name, b));
    }
    Store { files, dicts: DictSet::new().with(d), blocks }
}

fn build(s: &Store) -> KeyIndex {
    let mut b = KeyIndexBuilder::new("event_id");
    for (name, _) in &s.blocks { b.add_file_bytes(name, &s.files[name], &s.dicts).unwrap(); }
    b.build().unwrap()
}

fn rd<'a>(s: &'a Store) -> impl FnMut(&IndexedFile) -> Result<Vec<u8>, kore_core::KoreError> + 'a {
    move |f| Ok(s.files[&f.name].clone())
}

#[test]
fn round_trip_every_key_and_missing_keys() {
    let s = store();
    let idx = build(&s);
    // bytes survive a serialization round trip with the same id
    let again = KeyIndex::from_bytes(idx.to_bytes()).unwrap();
    assert_eq!(again.id(), idx.id());
    assert_eq!(again.id(), <[u8; 32]>::from(Sha256::digest(&idx.to_bytes()[..idx.to_bytes().len() - 32])));
    assert_eq!(idx.file_name(), format!("{}.kidx", idx.id_hex()));
    let total: usize = s.blocks.iter().map(|b| b.1.num_rows).sum();
    assert_eq!(idx.num_keys() as usize, total);
    // deterministic
    assert_eq!(build(&s).to_bytes(), idx.to_bytes());
    // the keys supplied directly give the same object as decoding the files
    let mut b2 = KeyIndexBuilder::new("event_id");
    for (name, blk) in &s.blocks {
        let ColumnData::Str(v) = &blk.columns[0].data else { panic!() };
        b2.add_file_keys(name, &s.files[name], v.iter().map(|x| x.as_deref())).unwrap();
    }
    assert_eq!(b2.build().unwrap().to_bytes(), idx.to_bytes());

    for (fno, (_, blk)) in s.blocks.iter().enumerate() {
        for r in 0..blk.num_rows {
            let key = blk.columns[0].data.get_str(r).unwrap();
            assert!(idx.candidates(key).contains(&fno));
            let hits = idx.lookup(key, &["text", "seq"], &s.dicts, Verify::Sha256, rd(&s)).unwrap();
            assert_eq!(hits.len(), 1, "key {key}");
            assert_eq!((hits[0].file, hits[0].row), (fno, r));
            assert_eq!(text_of(&hits[0]), blk.columns[2].data.get_str(r).unwrap());
            assert_eq!(hits[0].values.columns[1].data, ColumnData::Int64(vec![blk.columns[3].data.get_value(r).as_i64()]));
        }
    }
    // missing keys: never a hit, and almost never a file read
    let mut reads = 0;
    for k in 0..2000 {
        let key = hex(1_000_000 + k);
        let hits = idx.lookup(&key, &["text"], &s.dicts, Verify::Sha256, |f| { reads += 1; Ok(s.files[&f.name].clone()) }).unwrap();
        assert!(hits.is_empty());
    }
    assert!(reads < 40, "{reads} file reads for 2000 missing keys");
    for key in ["", "x", "not a key at all, much longer than one word"] {
        assert!(idx.lookup(key, &["text"], &s.dicts, Verify::Sha256, rd(&s)).unwrap().is_empty());
    }
}

trait AsI64 { fn as_i64(&self) -> Option<i64>; }
impl AsI64 for kore_core::Value { fn as_i64(&self) -> Option<i64> { if let kore_core::Value::Int(i) = self { Some(*i) } else { None } } }

#[test]
fn duplicates_nulls_empty_and_single() {
    // the same key in two files, a null key, and a value that only differs in length
    let a = DataBlock { num_rows: 3, columns: vec![
        Column { name: "id".into(), data: ColumnData::Str(vec![Some("k1".into()), None, Some("k2".into())]) },
        Column { name: "v".into(), data: ColumnData::Int64(vec![Some(1), Some(2), Some(3)]) }] };
    let b = DataBlock { num_rows: 2, columns: vec![
        Column { name: "id".into(), data: ColumnData::Str(vec![Some("k1".into()), Some("k1\0".into())]) },
        Column { name: "v".into(), data: ColumnData::Int64(vec![Some(10), Some(20)]) }] };
    let (ab, bb) = (KoreWriter::to_bytes(&a), KoreWriter::to_bytes_compact(&b, &CompactOptions::new()).unwrap());
    let mut bl = KeyIndexBuilder::new("id");
    bl.add_file_bytes("a", &ab, &DictSet::new()).unwrap();
    bl.add_file_bytes("b", &bb, &DictSet::new()).unwrap();
    assert!(bl.add_file_bytes("a", &ab, &DictSet::new()).is_err(), "same name twice");
    let idx = bl.build().unwrap();
    assert_eq!(idx.num_keys(), 4);
    let read = |f: &IndexedFile| Ok(if f.name == "a" { ab.clone() } else { bb.clone() });
    let hits = idx.lookup("k1", &["v"], &DictSet::new(), Verify::Sha256, read).unwrap();
    assert_eq!(hits.iter().map(|h| (h.file, h.row)).collect::<Vec<_>>(), vec![(0, 0), (1, 0)]);
    let hits = idx.lookup("k1\0", &["v"], &DictSet::new(), Verify::Sha256, read).unwrap();
    assert_eq!(hits.iter().map(|h| (h.file, h.row)).collect::<Vec<_>>(), vec![(1, 1)]);
    assert!(idx.lookup("k3", &["v"], &DictSet::new(), Verify::Sha256, read).unwrap().iter().all(|_| false));
    // empty index and single key
    let e = KeyIndexBuilder::new("id").build().unwrap();
    assert_eq!(e.num_keys(), 0);
    assert!(e.candidates("k1").is_empty());
    assert_eq!(KeyIndex::from_bytes(e.to_bytes()).unwrap().id(), e.id());
    let mut one = KeyIndexBuilder::new("id");
    one.add_file_keys("x", b"bytes", [Some("only")]).unwrap();
    let one = one.build().unwrap();
    assert_eq!(one.candidates("only"), vec![0]);
    // a non-string key column is refused clearly
    let mut bad = KeyIndexBuilder::new("v");
    assert!(bad.add_file_bytes("a", &ab, &DictSet::new()).unwrap_err().to_string().contains("not a string column"));
}

#[test]
fn many_keys_many_files() {
    // 1200 files x 50 keys: file numbers need 11 bits, many blocks
    let mut bl = KeyIndexBuilder::new("event_id");
    let mut where_ = vec![];
    for f in 0..1200 {
        let keys: Vec<String> = (0..50).map(|k| hex(f * 50 + k)).collect();
        bl.add_file_keys(&format!("f{f}"), format!("file {f}").as_bytes(), keys.iter().map(|k| Some(k.as_str()))).unwrap();
        for k in keys { where_.push((k, f)); }
    }
    let idx = bl.build().unwrap();
    assert_eq!(idx.num_keys(), 60_000);
    for (k, f) in where_.iter().step_by(7) { assert!(idx.candidates(k).contains(f)); }
    let per_key = idx.to_bytes().len() as f64 / 60_000.0;
    // 11-bit file numbers + about 10.5 bits of hash per key, plus 51 bytes per file entry (1200 entries)
    assert!(per_key < 4.0, "{per_key} bytes per key");
    let mut fp = 0;
    for k in 0..20_000 { if !idx.candidates(&hex(10_000_000 + k)).is_empty() { fp += 1; } }
    assert!(fp < 200, "{fp} false candidates out of 20000");
}

#[test]
fn tamper_and_truncation_at_every_byte_of_the_index() {
    let s = store();
    let idx = build(&s);
    let bytes = idx.to_bytes().to_vec();
    for i in 0..bytes.len() {
        for m in [0x01u8, 0x80, 0xFF] {
            let mut t = bytes.clone();
            t[i] ^= m;
            assert!(KeyIndex::from_bytes(&t).is_err(), "flip {m:#x} at byte {i} accepted");
        }
    }
    for cut in 0..bytes.len() { assert!(KeyIndex::from_bytes(&bytes[..cut]).is_err(), "truncated at {cut} accepted"); }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(KeyIndex::from_bytes(&longer).is_err());
}

/// Recompute the trailing sha256 (a forger who knows the format): structure checks must still refuse
/// or the object must stay safe to use (no panic, no wrong rows).
fn reseal(mut b: Vec<u8>) -> Vec<u8> {
    let n = b.len() - 32;
    let h: [u8; 32] = Sha256::digest(&b[..n]).into();
    b[n..].copy_from_slice(&h);
    b
}

#[test]
fn resealed_garbage_never_panics_or_returns_wrong_rows() {
    let s = store();
    let idx = build(&s);
    let bytes = idx.to_bytes().to_vec();
    for i in 0..bytes.len() - 32 {
        for m in [0x01u8, 0x80] {
            let mut t = bytes.clone();
            t[i] ^= m;
            let t = reseal(t);
            let r = std::panic::catch_unwind(|| {
                if let Ok(x) = KeyIndex::from_bytes(&t) {
                    for (fno, (_, blk)) in s.blocks.iter().enumerate().step_by(2) {
                        let key = blk.columns[0].data.get_str(3).unwrap();
                        // a forged index may point elsewhere or at a file it does not pin; it can never
                        // produce a row whose key differs
                        if let Ok(h) = x.lookup(key, &["event_id"], &s.dicts, Verify::Sha256, |f| s.files.get(&f.name).cloned().ok_or_else(|| kore_core::KoreError::InvalidArgument("no such file".into()))) {
                            for hit in h { assert_eq!(hit.values.columns[0].data.get_str(0), Some(key)); }
                        }
                        let _ = fno;
                    }
                }
            });
            assert!(r.is_ok(), "panic with flip {m:#x} at byte {i}");
        }
    }
}

#[test]
fn changed_data_files_are_refused() {
    let s = store();
    let idx = build(&s);
    let key = s.blocks[2].1.columns[0].data.get_str(5).unwrap().to_string();
    // same length, one byte changed: sha256 pin refuses it
    let mut changed = s.files[&s.blocks[2].0].clone();
    let last = changed.len() - 1;
    changed[last] ^= 1;
    let e = idx.lookup(&key, &["text"], &s.dicts, Verify::Sha256, |_| Ok(changed.clone())).unwrap_err();
    assert!(e.to_string().contains("changed since the index was built (sha256)"), "{e}");
    // with Verify::Length the file's own checksums catch a change in what is decoded (here the header);
    // a change in a column that is not decoded is only caught by Verify::Sha256
    let mut hdr = s.files[&s.blocks[2].0].clone();
    hdr[20] ^= 1;
    assert!(idx.lookup(&key, &["text"], &s.dicts, Verify::Length, |_| Ok(hdr.clone())).is_err());
    assert!(idx.lookup(&key, &["text"], &s.dicts, Verify::Length, |_| Ok(changed.clone())).is_ok());
    // truncated file
    let short = s.files[&s.blocks[2].0][..100].to_vec();
    for v in [Verify::Sha256, Verify::Length] {
        let e = idx.lookup(&key, &["text"], &s.dicts, v, |_| Ok(short.clone())).unwrap_err();
        assert!(e.to_string().contains("(length)"), "{e}");
    }
    // a different valid file under the same name (e.g. a rewritten segment)
    let other = KoreWriter::to_bytes_compact(&events(157, 2), &CompactOptions::new().with_zstd_level(3)).unwrap();
    assert!(idx.lookup(&key, &["text"], &s.dicts, Verify::Sha256, |_| Ok(other.clone())).is_err());
    // the dictionary of a v4 file is still required for its text
    let key3 = s.blocks[3].1.columns[0].data.get_str(1).unwrap();
    let e = idx.lookup(key3, &["text"], &DictSet::new(), Verify::Sha256, rd(&s)).unwrap_err();
    assert!(e.to_string().contains("needs shared dictionary"), "{e}");
    // ...but not for columns that do not use it
    assert_eq!(idx.lookup(key3, &["seq"], &DictSet::new(), Verify::Sha256, rd(&s)).unwrap().len(), 1);
}

#[test]
fn readers_refuse_the_object_clearly_and_unknown_versions() {
    let s = store();
    let idx = build(&s);
    let b = idx.to_bytes();
    // data readers (also the ones that predate the index: they check the KORE magic first)
    let e = KoreReader::from_bytes(b).unwrap_err().to_string();
    assert!(e.contains("key index"), "{e}");
    assert!(KoreReader::from_bytes_with_dicts(b, &s.dicts).is_err());
    assert!(KoreReader::from_bytes_columns(b, &["event_id"]).is_err());
    assert!(kore_store::v4::info(b).is_err());
    assert!(SharedDict::from_bytes(b).is_err());
    // a future index version is refused at the version field, even with a valid checksum
    let mut v2 = b.to_vec();
    v2[4] = 2;
    let e = KoreReader::from_bytes(&reseal(v2.clone())).unwrap_err().to_string();
    assert!(e.contains("key index"), "{e}");
    let e = KeyIndex::from_bytes(&reseal(v2)).unwrap_err().to_string();
    assert!(e.contains("unsupported key index version 2"), "{e}");
    let mut h = b.to_vec();
    h[6] = 2;
    assert!(KeyIndex::from_bytes(&reseal(h)).unwrap_err().to_string().contains("unknown key index hash"));
    let mut f = b.to_vec();
    f[7] = 1;
    assert!(KeyIndex::from_bytes(&reseal(f)).unwrap_err().to_string().contains("flags"));
    // a data file is not an index
    assert!(KeyIndex::from_bytes(&s.files[&s.blocks[0].0]).unwrap_err().to_string().contains("not a KORE key index"));
}

#[test]
fn old_files_are_indexed_and_still_read() {
    let v2 = fixture("sample_v2_6e2e6a7b.kore");
    let v3 = fixture("sample_v3_6e2e6a7b.kore");
    let b2 = KoreReader::from_bytes(&v2).unwrap();
    let b3 = KoreReader::from_bytes(&v3).unwrap();
    let key_col = b2.columns.iter().find(|c| matches!(c.data, ColumnData::Str(_) | ColumnData::StrDict { .. })).unwrap().name.clone();
    let mut bl = KeyIndexBuilder::new(&key_col);
    bl.add_file_bytes("v2", &v2, &DictSet::new()).unwrap();
    bl.add_file_bytes("v3", &v3, &DictSet::new()).unwrap();
    let idx = bl.build().unwrap();
    let key = b2.columns.iter().find(|c| c.name == key_col).unwrap().data.get_str(7).unwrap().to_string();
    let hits = idx.lookup(&key, &[key_col.as_str()], &DictSet::new(), Verify::Sha256, |f| Ok(if f.name == "v2" { v2.clone() } else { v3.clone() })).unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h.values.columns[0].data.get_str(0) == Some(key.as_str())));
    // the files themselves are unchanged and read as before
    assert_eq!(KoreReader::from_bytes(&v2).unwrap().num_rows, b2.num_rows);
    assert_eq!(KoreReader::from_bytes(&v3).unwrap().num_rows, b3.num_rows);
}

#[test]
fn time_travel_one_index_per_version() {
    // a version log; version t has its own index naming the log entry "@t"
    let d = SharedDict::from_content(b"some words about the day").unwrap();
    let ds = DictSet::new().with(d.clone());
    let mut log: Option<Vec<u8>> = None;
    let mut idxs = vec![];
    let mut blocks = vec![];
    for (v, ts) in [100u64, 200, 300].into_iter().enumerate() {
        // each version rewrites the block: same ids, different text
        let mut b = events(120, 0);
        if let ColumnData::Str(t) = &mut b.columns[2].data { for x in t.iter_mut().flatten() { x.push_str(&format!(" (version {v})")); } }
        let bytes = KoreWriter::to_bytes_compact(&b, &CompactOptions::new().with_dict("text", d.clone())).unwrap();
        log = Some(versioned::append_raw(log.as_deref(), &bytes, ts).unwrap());
        let mut bl = KeyIndexBuilder::new("event_id");
        bl.add_file_bytes(&format!("@{ts}"), &bytes, &ds).unwrap();
        idxs.push(bl.build().unwrap());
        blocks.push(b);
    }
    let log = log.unwrap();
    let key = blocks[0].columns[0].data.get_str(42).unwrap().to_string();
    let entry = |f: &IndexedFile| -> Result<Vec<u8>, kore_core::KoreError> {
        let ts: u64 = f.name[1..].parse().unwrap();
        Ok(versioned::select_raw(&log, ts)?.to_vec())
    };
    for (v, idx) in idxs.iter().enumerate() {
        let hits = idx.lookup(&key, &["text"], &ds, Verify::Sha256, entry).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(text_of(&hits[0]).ends_with(&format!("(version {v})")));
    }
    // ids differ per version; an old index used against the newest entry is refused (pinned bytes)
    assert_ne!(idxs[0].id(), idxs[2].id());
    let latest = versioned::select_raw(&log, u64::MAX).unwrap().to_vec();
    assert!(idxs[0].lookup(&key, &["text"], &ds, Verify::Sha256, |_| Ok(latest.clone())).is_err());
}

#[test]
fn kh64_is_stable() {
    // pinned values: the hash is part of the format
    let v = [keyindex::kh64(b""), keyindex::kh64(b"a"), keyindex::kh64(b"abcdefgh"), keyindex::kh64(b"abcdefghi")];
    assert_eq!(v.iter().collect::<std::collections::HashSet<_>>().len(), 4);
    assert_ne!(keyindex::kh64(b"a"), keyindex::kh64(b"a\0"), "length is part of the hash");
    assert_eq!(v, [0xe220a8397b1dcdaf, 0x5000610acb7d76f1, 0xe94324115e9414c2, 0xbe3ac7d31a786a48]);
}
