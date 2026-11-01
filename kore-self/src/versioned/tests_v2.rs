//! Format 2 tests: old-fixture compatibility, upgrade/migration, the v1 test
//! scenarios repeated on v2 logs, tamper detection, idempotence, lazy open.
//! Temp dirs only.

use super::*;
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

fn tick_clock() -> impl Fn() -> String + Send + 'static {
    let n = Arc::new(AtomicU64::new(0));
    move || {
        let k = n.fetch_add(1, Ordering::SeqCst) + 1;
        format!("2026-03-01T{:02}:{:02}:00Z", k / 60, k % 60)
    }
}

fn new_log(root: &Path, f: LogFormat) -> VersionedLog {
    VersionedLog::open_or_create_with_format(root, "tester", tick_clock(), f).unwrap()
}

fn ev(source: &str, occurred: &str, text: &str, tags: &[&str]) -> NewEvent {
    NewEvent { source: source.into(), occurred_at: occurred.into(), kind: "note".into(), text: text.into(), tags: tags.iter().map(|s| s.to_string()).collect(), redacted: false }
}

fn rev(key: &str, stance: &str, conf: f64, vf: &str, author: &str, causes: Vec<String>) -> NewRevision {
    NewRevision {
        entity_type: "belief".into(), key: key.into(), stance: stance.into(), confidence: conf, rationale: format!("because {stance}"),
        supersedes: None, valid_from: vf.into(), author: author.into(), caused_by: causes,
    }
}

fn populated(root: &Path, f: LogFormat) -> VersionedLog {
    let mut l = new_log(root, f);
    let e = l.append_events(&[ev("s", "2025-01-01", "a", &["x", "y"]), ev("s", "2025-01-02", "b", &[])]).unwrap();
    l.revise(rev("k", "x", 0.6, "2025-01-03", "rule:r", vec![e.event_ids[0].clone()])).unwrap();
    l.append_events(&[ev("s", "2025-01-04", "c", &["x"])]).unwrap();
    l.revise(rev("k", "y", 0.7, "2025-01-05", "user", vec![e.event_ids[1].clone()])).unwrap();
    l.verify().unwrap();
    l
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for f in fs::read_dir(src).unwrap().flatten() {
        let p = f.path();
        if p.is_dir() {
            copy_dir(&p, &dst.join(f.file_name()));
        } else {
            fs::copy(&p, dst.join(f.file_name())).unwrap();
        }
    }
}

fn fixture_copy() -> (tempfile::TempDir, serde_json::Value) {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data").join("vlog_v1");
    let tmp = tempfile::tempdir().unwrap();
    copy_dir(&base.join("fixture"), &tmp.path().join("fixture"));
    let exp: serde_json::Value = serde_json::from_slice(&fs::read(base.join("expected.json")).unwrap()).unwrap();
    (tmp, exp)
}

fn check_against_expected(l: &VersionedLog, exp: &serde_json::Value, upto: u64) {
    for v in exp["versions"].as_array().unwrap().iter().filter(|v| v["v"].as_u64().unwrap() <= upto) {
        let n = v["v"].as_u64().unwrap();
        let s = l.open_at(AsOf::Version(n)).unwrap();
        assert_eq!(s.content_hash(), v["content_hash"].as_str().unwrap(), "content hash at version {n}");
        assert_eq!(s.recorded_at, v["recorded_at"].as_str().unwrap());
    }
}

// ─── old-fixture compatibility ───────────────────────────────────────────────

#[test]
fn old_fixture_opens_verifies_and_queries_identically() {
    let (tmp, exp) = fixture_copy();
    let l = VersionedLog::open(tmp.path(), "fixture").unwrap();
    l.verify().unwrap();
    assert_eq!(l.format(), LogFormat::V1);
    assert_eq!(l.latest_version(), exp["latest"].as_u64().unwrap());
    for v in exp["versions"].as_array().unwrap() {
        assert_eq!(l.snapshot_hash(v["v"].as_u64().unwrap()).unwrap(), v["manifest_hash"].as_str().unwrap(), "manifest hash is the old chain, unchanged");
    }
    check_against_expected(&l, &exp, u64::MAX);
    let b = sql::run_readonly_sql(&l.open_latest(), "SELECT kind, COUNT(*) FROM events GROUP BY kind ORDER BY kind").unwrap();
    let want: Vec<Vec<String>> = serde_json::from_value(exp["sql_kind_counts"].clone()).unwrap();
    assert_eq!(sql::block_strings(&b), want);
    let ids: Vec<String> = l.open_latest().rows::<EventRow>().into_iter().map(|e| e.event_id).collect();
    let want_ids: Vec<String> = serde_json::from_value(exp["event_ids"].clone()).unwrap();
    assert_eq!(ids, want_ids);
    for id in &want_ids {
        assert!(l.has_event(id));
        assert_eq!(&l.get_event(id).unwrap().unwrap().event_id, id);
    }
    for f in exp["forgotten_ids"].as_array().unwrap() {
        let f = f.as_str().unwrap();
        assert!(!l.has_event(f) && l.is_forgotten(f));
    }
    assert!(l.get_event(&"0".repeat(64)).unwrap().is_none());
}

#[test]
fn old_fixture_still_accepts_v1_commits_and_stays_v1() {
    let (tmp, exp) = fixture_copy();
    let mut l = VersionedLog::open(tmp.path(), "fixture").unwrap();
    let before = l.latest_version();
    let info = l.append_events(&[ev("journal", "2025-09-01", "new on old log", &["t"])]).unwrap();
    assert!(info.created);
    l.verify().unwrap();
    assert_eq!(l.format(), LogFormat::V1, "no silent upgrade");
    let m = fs::read_to_string(l.dir().join("MANIFEST")).unwrap();
    assert!(!m.contains("\"enc\""), "no v2 segment was written");
    check_against_expected(&l, &exp, before);
    let l2 = VersionedLog::open(tmp.path(), "fixture").unwrap();
    assert!(l2.has_event(&info.event_ids[0]));
}

#[test]
fn in_place_upgrade_keeps_history_and_writes_v2_afterwards() {
    let (tmp, exp) = fixture_copy();
    let mut l = VersionedLog::open(tmp.path(), "fixture").unwrap();
    let before = l.latest_version();
    assert!(l.upgrade_to_v2().unwrap());
    assert!(!l.upgrade_to_v2().unwrap(), "second call is a no-op");
    assert_eq!(l.latest_version(), before + 1);
    let info = l.append_events(&[ev("journal", "2025-09-01", "after upgrade", &["t", "u"])]).unwrap();
    l.verify().unwrap();
    for v in exp["versions"].as_array().unwrap() {
        assert_eq!(l.snapshot_hash(v["v"].as_u64().unwrap()).unwrap(), v["manifest_hash"].as_str().unwrap(), "old entries untouched");
    }
    check_against_expected(&l, &exp, before);
    let m = fs::read_to_string(l.dir().join("MANIFEST")).unwrap();
    assert!(m.contains("\"enc\":2"));
    let l2 = VersionedLog::open(tmp.path(), "fixture").unwrap();
    assert_eq!(l2.format(), LogFormat::V2);
    l2.verify().unwrap();
    assert!(l2.has_event(&info.event_ids[0]));
    assert_eq!(l2.get_event(&info.event_ids[0]).unwrap().unwrap().tags, "t,u");
}

#[test]
fn migrate_copies_every_version_with_equal_content() {
    let (tmp, exp) = fixture_copy();
    let dst = tempfile::tempdir().unwrap();
    let rep = migrate_to_v2(tmp.path(), dst.path(), "fixture").unwrap();
    assert_eq!(rep.versions_copied, exp["latest"].as_u64().unwrap() + 1);
    let m = VersionedLog::open(dst.path(), "fixture").unwrap();
    m.verify().unwrap();
    assert_eq!(m.format(), LogFormat::V2);
    check_against_expected(&m, &exp, exp["latest"].as_u64().unwrap());
    for f in exp["forgotten_ids"].as_array().unwrap() {
        assert!(m.is_forgotten(f.as_str().unwrap()));
    }
    assert!(rep.dst_bytes < rep.src_bytes, "{rep:?}");
    // source untouched
    VersionedLog::open(tmp.path(), "fixture").unwrap().verify().unwrap();
    // refuses to overwrite
    assert!(migrate_to_v2(tmp.path(), dst.path(), "fixture").is_err());
}

// ─── v1 scenarios on v2 ──────────────────────────────────────────────────────

#[test]
fn v1_and_v2_hold_identical_rows_for_identical_operations() {
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (populated(d1.path(), LogFormat::V1), populated(d2.path(), LogFormat::V2));
    assert_eq!(a.latest_version(), b.latest_version());
    // genesis rows differ (format_version), every later content is equal
    for v in 1..=a.latest_version() {
        let (sa, sb) = (a.open_at(AsOf::Version(v)).unwrap(), b.open_at(AsOf::Version(v)).unwrap());
        for t in [Table::Events, Table::Revisions, Table::RevisionCauses] {
            assert_eq!(sa.raw(t), sb.raw(t), "{} at v{v}", t.name());
        }
    }
    let (ra, rb) = (a.open_latest(), b.open_latest());
    let q = "SELECT source, kind, COUNT(*) FROM events GROUP BY source, kind ORDER BY source, kind";
    assert_eq!(sql::block_strings(&sql::run_readonly_sql(&ra, q).unwrap()), sql::block_strings(&sql::run_readonly_sql(&rb, q).unwrap()));
}

#[test]
fn v2_time_travel_prefix_and_idempotent_ingest() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = populated(dir.path(), LogFormat::V2);
    let hashes: Vec<String> = (0..=l.latest_version()).map(|v| l.open_at(AsOf::Version(v)).unwrap().content_hash()).collect();
    let manifest_before = fs::read(l.dir().join("MANIFEST")).unwrap();
    let seg_before = fs::read(l.dir().join("events").join("0000000001.kore")).unwrap();
    // idempotent ingest: same events again create no version
    let n = l.latest_version();
    let again = l.append_events(&[ev("s", "2025-01-01", "a", &["x", "y"]), ev("s", "2025-01-02", "b", &[])]).unwrap();
    assert!(!again.created);
    assert_eq!(l.latest_version(), n);
    assert_eq!(fs::read(l.dir().join("MANIFEST")).unwrap(), manifest_before);
    // append-only: earlier segments and manifest prefix are byte-identical after a new commit
    l.append_events(&[ev("s", "2025-02-01", "new", &[])]).unwrap();
    let manifest_after = fs::read(l.dir().join("MANIFEST")).unwrap();
    assert!(manifest_after.starts_with(&manifest_before));
    assert_eq!(fs::read(l.dir().join("events").join("0000000001.kore")).unwrap(), seg_before);
    for (v, h) in hashes.iter().enumerate() {
        assert_eq!(&l.open_at(AsOf::Version(v as u64)).unwrap().content_hash(), h);
    }
    let t = l.open_at(AsOf::Time("2026-03-01T00:03:00Z".into())).unwrap();
    assert_eq!(t.version, 2);
    // reopen: index and snapshot agree with what was written
    let l2 = VersionedLog::open(dir.path(), "tester").unwrap();
    assert_eq!(l2.open_latest().content_hash(), l.open_latest().content_hash());
    assert_eq!(l2.event_count(), 4);
}

#[test]
fn v2_failed_batch_leaves_index_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = populated(dir.path(), LogFormat::V2);
    let n = l.event_count();
    let bad = Batch {
        events: vec![ev("s", "2025-03-01", "will be rolled back", &[])],
        revisions: vec![rev("zzz", "x", 0.5, "2025-03-01", "rule:r", vec!["f".repeat(64)])],
        ..Default::default()
    };
    assert!(l.commit_batch(bad).is_err());
    assert_eq!(l.event_count(), n);
    let id = event_id("s", "2025-03-01T00:00:00Z", "will be rolled back");
    assert!(!l.has_event(&id));
    // the same event commits fine afterwards, with a consistent ordinal
    let info = l.append_events(&[ev("s", "2025-03-01", "will be rolled back", &[])]).unwrap();
    assert!(info.created);
    assert_eq!(l.get_event(&id).unwrap().unwrap().text, "will be rolled back");
}

#[test]
fn v2_revision_replay_is_idempotent_but_v1_is_not() {
    for (f, idempotent) in [(LogFormat::V1, false), (LogFormat::V2, true)] {
        let dir = tempfile::tempdir().unwrap();
        let mut l = new_log(dir.path(), f);
        let e = l.append_events(&[ev("s", "2025-01-01", "a", &[])]).unwrap().event_ids;
        let r1 = rev("k", "one", 0.5, "2025-01-02", "rule:r", e.clone());
        let r2 = rev("k", "two", 0.6, "2025-01-03", "rule:r", e.clone());
        l.revise(r1.clone()).unwrap();
        l.revise(r2.clone()).unwrap();
        let n = l.latest_version();
        // replay in original order
        l.revise(r1).unwrap();
        l.revise(r2).unwrap();
        if idempotent {
            assert_eq!(l.latest_version(), n, "replay created versions on a v2 log");
        } else {
            assert_eq!(l.latest_version(), n + 2, "v1 behaviour is preserved");
        }
        l.verify().unwrap();
    }
}

#[test]
fn v2_forget_rewrites_v2_segments_and_blocks_reingest() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = populated(dir.path(), LogFormat::V2);
    let e = l.open_latest().rows::<EventRow>();
    let secret = e[0].event_id.clone();
    let r = l.forget_events(&[secret.clone()]).unwrap();
    assert!(r.created);
    l.verify().unwrap();
    assert!(!l.has_event(&secret) && l.is_forgotten(&secret));
    for v in 0..=l.latest_version() {
        assert!(l.open_at(AsOf::Version(v)).unwrap().rows::<EventRow>().iter().all(|r| r.event_id != secret));
    }
    let info = l.append_events(&[ev("s", "2025-01-01", "a", &["x", "y"])]).unwrap();
    assert_eq!(info.suppressed, 1);
    let m = fs::read_to_string(l.dir().join("MANIFEST")).unwrap();
    assert!(m.contains("\"enc\":2"));
    let l2 = VersionedLog::open(dir.path(), "tester").unwrap();
    assert!(l2.is_forgotten(&secret));
    assert_eq!(l2.open_latest().content_hash(), l.open_latest().content_hash());
}

#[test]
fn v2_tamper_every_sampled_byte_flip_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let l = populated(dir.path(), LogFormat::V2);
    let mut files = vec![];
    for t in ALL_TABLES {
        if let Ok(rd) = fs::read_dir(l.dir().join(t.name())) {
            files.extend(rd.flatten().map(|f| f.path()));
        }
    }
    assert!(files.len() >= 6);
    let mut flips = 0;
    for f in files {
        let orig = fs::read(&f).unwrap();
        for i in 0..orig.len() {
            let mut b = orig.clone();
            b[i] ^= 1 << (i % 8);
            fs::write(&f, &b).unwrap();
            assert!(matches!(l.verify(), Err(VerifyError::ChecksumMismatch { .. })), "flip at {} byte {i} not detected", f.display());
            assert!(VersionedLog::open(dir.path(), "tester").is_err(), "open accepted a flipped byte");
            flips += 1;
        }
        fs::write(&f, &orig).unwrap();
    }
    l.verify().unwrap();
    assert!(flips > 500);
    // manifest edits
    let m = fs::read_to_string(l.dir().join("MANIFEST")).unwrap();
    let edited = m.replacen("\"enc\":2", "\"enc\":9", 1);
    fs::write(l.dir().join("MANIFEST"), edited).unwrap();
    assert!(matches!(l.verify().unwrap_err(), VerifyError::ChainBroken { .. }));
    fs::write(l.dir().join("MANIFEST"), &m).unwrap();
    // a segment replaced by a v1-encoded copy of the same rows is also caught
    let seg = l.dir().join("events").join("0000000001.kore");
    let rows = load_segment(l.dir(), &l.entries[1].segments[0]).unwrap();
    fs::write(&seg, kore_store::KoreWriter::to_bytes(&rows_to_block(Table::Events, &rows))).unwrap();
    assert!(matches!(l.verify().unwrap_err(), VerifyError::ChecksumMismatch { .. }));
}

#[test]
fn v2_concurrent_writer_is_a_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = populated(dir.path(), LogFormat::V2);
    let mut b = VersionedLog::open(dir.path(), "tester").unwrap();
    a.append_events(&[ev("s", "2025-04-01", "from a", &[])]).unwrap();
    assert!(matches!(b.append_events(&[ev("s", "2025-04-02", "from b", &[])]), Err(LogError::Conflict(_))));
}

#[test]
fn v2_get_event_without_materialising_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = new_log(dir.path(), LogFormat::V2);
    let mut ids = vec![];
    for b in 0..5 {
        let evs: Vec<NewEvent> = (0..50).map(|i| ev("s", "2025-05-01", &format!("event {b}-{i} text"), &["t"])).collect();
        ids.extend(l.append_events(&evs).unwrap().event_ids);
    }
    drop(l);
    let l = VersionedLog::open(dir.path(), "tester").unwrap();
    assert!(l.head.lock().unwrap().is_none(), "all-v2 log: rows are not decoded at open");
    assert_eq!(l.event_count(), 250);
    for (k, id) in ids.iter().enumerate() {
        let r = l.get_event(id).unwrap().unwrap();
        assert_eq!(&r.event_id, id);
        assert_eq!(r.text, format!("event {}-{} text", k / 50, k % 50));
    }
    assert!(l.head.lock().unwrap().is_none(), "lookups did not materialise the snapshot");
    assert_eq!(l.open_latest().rows::<EventRow>().len(), 250);
    assert!(l.head.lock().unwrap().is_some());
}

#[test]
fn v2_log_is_not_readable_as_v1_by_the_old_reader() {
    let dir = tempfile::tempdir().unwrap();
    let l = populated(dir.path(), LogFormat::V2);
    let bytes = fs::read(l.dir().join("events").join("0000000001.kore")).unwrap();
    assert!(kore_store::KoreReader::from_bytes(&bytes).is_err(), "old reader must refuse, not misread");
}

// ─── round-1 format-2 fixtures (written by the round-1 binary) ───────────────

fn r1_fixture_copy(name: &str) -> (tempfile::TempDir, serde_json::Value) {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data").join("vlog_v2_r1").join(name);
    let tmp = tempfile::tempdir().unwrap();
    copy_dir(&base.join("fixture"), &tmp.path().join("fixture"));
    let exp: serde_json::Value = serde_json::from_slice(&fs::read(base.join("expected.json")).unwrap()).unwrap();
    (tmp, exp)
}

fn check_r1_fixture(name: &str, expect_fmt: i64) {
    let (tmp, exp) = r1_fixture_copy(name);
    assert_eq!(exp["format"].as_i64().unwrap(), expect_fmt);
    let l = VersionedLog::open(tmp.path(), "fixture").unwrap();
    l.verify().unwrap();
    assert_eq!(l.format(), LogFormat::V2);
    assert_eq!(l.latest_version(), exp["latest"].as_u64().unwrap());
    for v in exp["versions"].as_array().unwrap() {
        assert_eq!(l.snapshot_hash(v["v"].as_u64().unwrap()).unwrap(), v["manifest_hash"].as_str().unwrap(), "chain unchanged");
    }
    check_against_expected(&l, &exp, u64::MAX);
    let b = sql::run_readonly_sql(&l.open_latest(), "SELECT kind, COUNT(*) FROM events GROUP BY kind ORDER BY kind").unwrap();
    let want: Vec<Vec<String>> = serde_json::from_value(exp["sql_kind_counts"].clone()).unwrap();
    assert_eq!(sql::block_strings(&b), want);
    let want_ids: Vec<String> = serde_json::from_value(exp["event_ids"].clone()).unwrap();
    let ids: Vec<String> = l.open_latest().rows::<EventRow>().into_iter().map(|e| e.event_id).collect();
    assert_eq!(ids, want_ids);
    // a lazily opened second handle answers get_event from segments
    let l2 = VersionedLog::open(tmp.path(), "fixture").unwrap();
    for id in &want_ids {
        assert_eq!(&l2.get_event(id).unwrap().unwrap().event_id, id);
    }
    for f in exp["forgotten_ids"].as_array().unwrap() {
        assert!(!l.has_event(f.as_str().unwrap()) && l.is_forgotten(f.as_str().unwrap()));
    }
}

#[test]
fn r1_v2_fixture_opens_verifies_and_queries_identically() {
    check_r1_fixture("fixture", 2);
}

#[test]
fn r1_v2_mixed_fixture_opens_verifies_and_queries_identically() {
    check_r1_fixture("fixture_mixed", 2);
}

#[test]
fn r1_v2_fixture_still_accepts_commits_and_keeps_its_shape() {
    let (tmp, exp) = r1_fixture_copy("fixture");
    let mut l = VersionedLog::open(tmp.path(), "fixture").unwrap();
    let before = l.latest_version();
    let m0 = fs::read_to_string(l.dir().join("MANIFEST")).unwrap();
    let info = l.append_events(&[ev("journal", "2025-09-01", "new on r1 v2 log", &["t"])]).unwrap();
    assert!(info.created);
    l.verify().unwrap();
    assert_eq!(l.format(), LogFormat::V2);
    let m1 = fs::read_to_string(l.dir().join("MANIFEST")).unwrap();
    assert!(m1.starts_with(&m0), "old manifest lines untouched");
    assert!(m1.starts_with("KORE-VLOG-MANIFEST 1\n"), "no silent manifest upgrade");
    check_against_expected(&l, &exp, before);
    assert!(VersionedLog::open(tmp.path(), "fixture").unwrap().has_event(&info.event_ids[0]));
}

// ─── format 3: journal MANIFEST ──────────────────────────────────────────────

fn manifest_bytes(l: &VersionedLog) -> Vec<u8> {
    fs::read(l.dir().join("MANIFEST")).unwrap()
}

#[test]
fn v3_commits_append_to_the_manifest_and_history_is_identical_to_v2() {
    let d3 = tempfile::tempdir().unwrap();
    let d2 = tempfile::tempdir().unwrap();
    let mut l3 = populated(d3.path(), LogFormat::V3);
    let l2 = populated(d2.path(), LogFormat::V2);
    assert_eq!(l3.format(), LogFormat::V3);
    assert!(manifest_bytes(&l3).starts_with(b"KORE-VLOG-MANIFEST 2\n"));
    // same rows at every version (hashes differ: the schema_meta row records the format)
    for v in 0..=l2.latest_version() {
        let (a, b) = (l3.open_at(AsOf::Version(v)).unwrap(), l2.open_at(AsOf::Version(v)).unwrap());
        for t in [Table::Events, Table::Revisions, Table::RevisionCauses] {
            assert_eq!(a.raw(t), b.raw(t), "version {v}");
        }
    }
    // each commit only appends: the old bytes are an exact prefix and the file grows by one line
    for i in 0..5 {
        let before = manifest_bytes(&l3);
        l3.append_events(&[ev("s", "2025-02-01", &format!("more {i}"), &[])]).unwrap();
        let after = manifest_bytes(&l3);
        assert!(after.starts_with(&before));
        assert_eq!(after[before.len()..].iter().filter(|&&b| b == b'\n').count(), 1);
        assert_eq!(*after.last().unwrap(), b'\n');
    }
    l3.verify().unwrap();
    let r = VersionedLog::open(d3.path(), "tester").unwrap();
    assert_eq!(r.latest_version(), l3.latest_version());
    assert_eq!(r.open_latest().content_hash(), l3.open_latest().content_hash());
    assert_eq!(r.format(), LogFormat::V3);
    // an identical re-submission is still a no-op
    let n = l3.latest_version();
    assert!(!l3.append_events(&[ev("s", "2025-02-01", "more 0", &[])]).unwrap().created);
    assert_eq!(l3.latest_version(), n);
}

#[test]
fn v3_interrupted_append_is_ignored_reported_and_cleaned() {
    let dir = tempfile::tempdir().unwrap();
    let l = populated(dir.path(), LogFormat::V3);
    let good = manifest_bytes(&l);
    let head = l.open_latest().content_hash();
    // a crash in the middle of an append leaves part of a line (no newline)
    let mut f = fs::OpenOptions::new().append(true).open(l.dir().join("MANIFEST")).unwrap();
    f.write_all(b"{\"v\":99,\"recorded_at\":\"2026-").unwrap();
    f.sync_all().unwrap();
    drop(f);
    assert!(matches!(l.verify().unwrap_err(), VerifyError::ManifestCorrupt(_)), "verify reports the partial tail");
    let mut r = VersionedLog::open(dir.path(), "tester").unwrap();
    assert_eq!(r.open_latest().content_hash(), head, "open ignores the uncommitted tail");
    assert_eq!(r.latest_version(), l.latest_version());
    // the next commit removes the tail first, then appends
    r.set_clock(tick_clock());
    let info = r.append_events(&[ev("s", "2025-03-01", "after crash", &[])]).unwrap();
    assert!(info.created);
    r.verify().unwrap();
    let m = manifest_bytes(&r);
    assert!(m.starts_with(&good), "committed part untouched");
    assert!(!String::from_utf8_lossy(&m).contains("\"v\":99"));
    assert!(VersionedLog::open(dir.path(), "tester").unwrap().has_event(&info.event_ids[0]));
    // recover() also cleans it
    let mut f = fs::OpenOptions::new().append(true).open(r.dir().join("MANIFEST")).unwrap();
    f.write_all(b"{\"v\"").unwrap();
    drop(f);
    let mut r2 = VersionedLog::open(dir.path(), "tester").unwrap();
    assert!(r2.verify().is_err());
    r2.recover().unwrap();
    r2.verify().unwrap();
    // a format-1 style manifest never tolerates a partial tail
    let d1 = tempfile::tempdir().unwrap();
    let l1 = populated(d1.path(), LogFormat::V2);
    let mut f = fs::OpenOptions::new().append(true).open(l1.dir().join("MANIFEST")).unwrap();
    f.write_all(b"{\"v\"").unwrap();
    drop(f);
    assert!(VersionedLog::open(d1.path(), "tester").is_err());
}

#[test]
fn v3_tamper_every_manifest_byte_and_every_segment_byte_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let l = populated(dir.path(), LogFormat::V3);
    let mpath = l.dir().join("MANIFEST");
    let orig = fs::read(&mpath).unwrap();
    let hdr = b"KORE-VLOG-MANIFEST 2\n".len();
    let mut flips = 0;
    for i in 0..orig.len() {
        let mut b = orig.clone();
        b[i] ^= 1 << (i % 8);
        fs::write(&mpath, &b).unwrap();
        let v = l.verify();
        if i < hdr {
            // a header flip is either rejected or turns it into a plain format-1 header, which changes no entry
            assert!(v.is_err() || b[..hdr - 1] == *b"KORE-VLOG-MANIFEST 1", "header byte {i}");
        } else {
            assert!(v.is_err(), "manifest byte {i} not detected by verify");
        }
        flips += 1;
    }
    fs::write(&mpath, &orig).unwrap();
    l.verify().unwrap();
    assert!(flips > 500);
    // truncation to every length either fails or (cut exactly at a line end) is a rollback to an earlier version
    for cut in 0..orig.len() {
        fs::write(&mpath, &orig[..cut]).unwrap();
        if let Ok(r) = VersionedLog::open(dir.path(), "tester") {
            assert!(r.latest_version() < l.latest_version(), "a truncated manifest must not look complete");
        }
    }
    fs::write(&mpath, &orig).unwrap();
    // segment flips behave as in format 2
    let seg = l.dir().join("events").join("0000000001.kore");
    let so = fs::read(&seg).unwrap();
    for i in 0..so.len() {
        let mut b = so.clone();
        b[i] ^= 1 << (i % 8);
        fs::write(&seg, &b).unwrap();
        assert!(matches!(l.verify(), Err(VerifyError::ChecksumMismatch { .. })));
    }
    fs::write(&seg, &so).unwrap();
    l.verify().unwrap();
}

#[test]
fn v3_concurrent_writer_is_a_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = populated(dir.path(), LogFormat::V3);
    let mut b = VersionedLog::open(dir.path(), "tester").unwrap();
    b.set_clock(tick_clock());
    a.append_events(&[ev("s", "2025-04-01", "from a", &[])]).unwrap();
    let e = b.append_events(&[ev("s", "2025-04-02", "from b", &[])]).unwrap_err();
    assert!(matches!(e, LogError::Conflict(_)), "{e:?}");
    a.verify().unwrap();
}

#[test]
fn explicit_upgrade_to_v3_keeps_old_lines_and_hashes_and_never_happens_silently() {
    for fixture in ["vlog_v1", "vlog_v2_r1"] {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data").join(fixture);
        let sub = if fixture == "vlog_v1" { base.clone() } else { base.join("fixture") };
        let tmp = tempfile::tempdir().unwrap();
        copy_dir(&sub.join("fixture"), &tmp.path().join("fixture"));
        let exp: serde_json::Value = serde_json::from_slice(&fs::read(sub.join("expected.json")).unwrap()).unwrap();
        let mut l = VersionedLog::open(tmp.path(), "fixture").unwrap();
        let before = manifest_bytes(&l);
        assert!(before.starts_with(b"KORE-VLOG-MANIFEST 1\n"));
        // ordinary commits never change the manifest flavour
        l.append_events(&[ev("journal", "2025-09-01", "plain commit", &[])]).unwrap();
        assert!(manifest_bytes(&l).starts_with(b"KORE-VLOG-MANIFEST 1\n"), "{fixture}: no silent upgrade");
        let mid = manifest_bytes(&l);
        assert!(l.upgrade_to_v3().unwrap());
        assert!(!l.upgrade_to_v3().unwrap());
        assert_eq!(l.format(), LogFormat::V3);
        let after = manifest_bytes(&l);
        let nl = |b: &[u8]| b.iter().position(|&c| c == b'\n').unwrap() + 1;
        assert!(after.starts_with(b"KORE-VLOG-MANIFEST 2\n"));
        assert!(after[nl(&after)..].starts_with(&mid[nl(&mid)..]), "{fixture}: every old entry line is byte-identical");
        l.verify().unwrap();
        l.append_events(&[ev("journal", "2025-09-02", "journal commit", &[])]).unwrap();
        let after2 = manifest_bytes(&l);
        assert!(after2.starts_with(&after));
        check_against_expected(&l, &exp, u64::MAX);
        for v in exp["versions"].as_array().unwrap() {
            assert_eq!(l.snapshot_hash(v["v"].as_u64().unwrap()).unwrap(), v["manifest_hash"].as_str().unwrap(), "{fixture}: chain unchanged");
        }
        let mut r = VersionedLog::open(tmp.path(), "fixture").unwrap();
        assert_eq!(r.format(), LogFormat::V3);
        assert_eq!(r.open_latest().content_hash(), l.open_latest().content_hash());
        // forget still works and keeps the journal header
        r.set_clock(tick_clock());
        let id = exp["event_ids"][0].as_str().unwrap().to_string();
        assert!(r.forget_events(&[id.clone()]).unwrap().created);
        r.verify().unwrap();
        assert!(manifest_bytes(&r).starts_with(b"KORE-VLOG-MANIFEST 2\n"));
        r.append_events(&[ev("journal", "2025-09-03", "after forget", &[])]).unwrap();
        r.verify().unwrap();
        assert!(!r.has_event(&id));
    }
}

#[test]
fn v3_get_event_reads_one_row_and_matches_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = new_log(dir.path(), LogFormat::V3);
    for b in 0..4 {
        let evs: Vec<NewEvent> = (0..300).map(|i| ev("s", "2025-05-01", &format!("batch {b} event {i} {}", "w ".repeat(i % 9)), if i % 3 == 0 { &["a", "b"] } else { &[] })).collect();
        l.append_events(&evs).unwrap();
    }
    let r = VersionedLog::open(dir.path(), "tester").unwrap();
    let all = l.open_latest().rows::<EventRow>();
    for e in all.iter().step_by(37) {
        assert_eq!(&r.get_event(&e.event_id).unwrap().unwrap(), e);
    }
    assert!(r.head.lock().unwrap().is_none(), "get_event did not materialise the snapshot");
}
