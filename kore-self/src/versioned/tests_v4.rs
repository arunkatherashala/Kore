//! Format 4 tests: kore-store v4 compact segments + shared dictionaries listed in the MANIFEST.
//! Round trip and equality with formats 1/3, upgrade from the committed 1/2/3 fixtures, the
//! committed format-4 fixtures, refusal by older readers, tamper detection (dictionary objects,
//! MANIFEST, segments), forget and dictionaries, streamed open / events-only read. Temp dirs only.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

fn tick_clock() -> impl Fn() -> String + Send + 'static {
    let n = Arc::new(AtomicU64::new(0));
    move || {
        let k = n.fetch_add(1, Ordering::SeqCst) + 1;
        format!("2026-03-{:02}T{:02}:{:02}:00Z", 1 + k / 1440, (k / 60) % 24, k % 60)
    }
}

fn ev(source: &str, occurred: &str, text: &str, tags: &[&str]) -> NewEvent {
    NewEvent { source: source.into(), occurred_at: occurred.into(), kind: "note".into(), text: text.into(), tags: tags.iter().map(|s| s.to_string()).collect(), redacted: false }
}

fn rev(key: &str, stance: &str, causes: Vec<String>) -> NewRevision {
    NewRevision { entity_type: "belief".into(), key: key.into(), stance: stance.into(), confidence: 0.5, rationale: format!("because {stance}"), supersedes: None, valid_from: "2025-01-01".into(), author: "rule:r".into(), caused_by: causes }
}

/// Small thresholds so that a test log gets dictionaries.
fn small_policy() -> DictPolicy {
    DictPolicy { train_events: 30, max_bytes: 4096, check_events: 0, check_min_text: 0, min_gain_pct: 5 }
}

fn text_of(b: u32, i: u32) -> String {
    format!("batch {b} item {i}: walked to the station, talked about the {} project, felt {} afterwards", ["garden", "kitchen", "budget", "travel"][(i % 4) as usize], ["fine", "tired", "glad"][(i % 3) as usize])
}

/// The same operations on any format: 6 batches of 20 events with revisions, a prediction + outcome.
fn build(root: &Path, f: LogFormat) -> VersionedLog {
    let mut l = VersionedLog::open_or_create_with_format(root, "tester", tick_clock(), f).unwrap();
    l.set_dict_policy(small_policy());
    let mut ids = vec![];
    for b in 0..6u32 {
        let evs: Vec<NewEvent> = (0..20u32).map(|i| ev(["journal", "chat"][(i % 2) as usize], &format!("2025-06-{:02}T10:{:02}:00Z", 1 + b, i), &text_of(b, i), if i % 3 == 0 { &["a", "b"] } else { &[] })).collect();
        let info = l.append_events(&evs).unwrap();
        ids.extend(info.event_ids);
        l.revise(rev(&format!("k{}", b % 3), &format!("s{b}"), vec![ids[ids.len() - 1].clone()])).unwrap();
    }
    let p = NewPrediction {
        made_at: "2026-01-01".into(), question_id: "q1".into(), target: "t".into(), options: vec!["yes".into(), "no".into()], probs: vec![0.5, 0.5],
        model_id: "m".into(), model_version: "1".into(), features_cite: vec![ids[2].clone()], resolves_by: None,
    };
    let pid = l.record_prediction(p).unwrap().pred_ids[0].clone();
    l.record_outcome(NewOutcome { pred_id: pid, resolved_at: "2026-01-02".into(), actual: "yes".into(), outcome_event_id: Some(ids[3].clone()) }).unwrap();
    l
}

fn manifest_bytes(l: &VersionedLog) -> Vec<u8> {
    fs::read(l.dir().join("MANIFEST")).unwrap()
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

fn all_files(p: &Path, out: &mut Vec<PathBuf>) {
    for f in fs::read_dir(p).unwrap().flatten() {
        if f.path().is_dir() {
            all_files(&f.path(), out);
        } else {
            out.push(f.path());
        }
    }
}

fn dict_files(l: &VersionedLog) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(l.dir().join("dicts")).map(|rd| rd.flatten().map(|f| f.path()).collect()).unwrap_or_default();
    v.sort();
    v
}

fn sql_hash(s: &Snapshot) -> String {
    let mut out = String::new();
    for q in ["SELECT kind, source, COUNT(*) FROM events GROUP BY kind, source ORDER BY kind, source", "SELECT key, stance FROM beliefs_current ORDER BY key", "SELECT event_id, text, tags FROM events ORDER BY event_id"] {
        out.push_str(&format!("{:?}", sql::block_strings(&run_readonly_sql(s, q).unwrap())));
    }
    sha256_hex(&[out.as_bytes()])
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

// ─── round trip, equality with formats 1 and 3 ───────────────────────────────

#[test]
fn v4_round_trip_equals_formats_1_and_3_at_every_version() {
    let (d1, d3, d4) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (l1, l3, l4) = (build(d1.path(), LogFormat::V1), build(d3.path(), LogFormat::V3), build(d4.path(), LogFormat::V4));
    assert_eq!(l4.format(), LogFormat::V4);
    l4.verify().unwrap();
    assert!(manifest_bytes(&l4).starts_with(b"KORE-VLOG-MANIFEST 3\n"));
    assert_eq!(l1.latest_version(), l4.latest_version());
    for v in 0..=l4.latest_version() {
        let (a, b, c) = (l1.open_at(AsOf::Version(v)).unwrap(), l3.open_at(AsOf::Version(v)).unwrap(), l4.open_at(AsOf::Version(v)).unwrap());
        for t in [Table::Events, Table::Revisions, Table::RevisionCauses, Table::Predictions, Table::Outcomes] {
            assert_eq!(a.raw(t), c.raw(t), "v1 vs v4, version {v}, {}", t.name());
            assert_eq!(b.raw(t), c.raw(t), "v3 vs v4, version {v}, {}", t.name());
        }
        if v >= 1 {
            assert_eq!(sql_hash(&a), sql_hash(&c), "SQL results at version {v}");
            assert_eq!(sql_hash(&b), sql_hash(&c));
        }
    }
    // a dictionary was built (train_events = 30: by the second events commit), is listed once and used
    let ids = l4.dictionary_ids();
    assert_eq!(ids.len(), 1, "one dictionary, no drift checks");
    assert_eq!(dict_files(&l4).len(), 1);
    let listing = l4.entries.iter().position(|e| !e.dicts.is_empty()).unwrap();
    let mut used = 0;
    for (ei, e) in l4.entries.iter().enumerate() {
        for g in &e.segments {
            assert_eq!(g.enc, Some(4), "every segment of a format-4 log is a v4 segment");
            let b = fs::read(l4.dir().join(&g.file)).unwrap();
            assert!(kore_store::v4::is_v4(&b));
            let refs = v4log::seg_dict_ids(&b).unwrap();
            if !refs.is_empty() {
                assert_eq!(refs, ids);
                assert!(ei >= listing, "used only from the commit that lists it");
                assert_eq!(g.table, "events");
                used += 1;
            }
        }
    }
    assert!(used >= 5, "events segments from the listing commit on use the dictionary ({used})");
    // reopen (streamed) answers the same
    let r = VersionedLog::open(d4.path(), "tester").unwrap();
    assert!(r.head.lock().unwrap().is_none(), "format 4 opens streamed (ids only)");
    assert_eq!(r.open_latest().content_hash(), l4.open_latest().content_hash());
    for e in l1.open_latest().rows::<EventRow>() {
        assert_eq!(r.get_event(&e.event_id).unwrap().unwrap(), e);
    }
    assert_eq!(r.format(), LogFormat::V4);
}

#[test]
fn v4_commits_append_and_reopened_handle_keeps_using_the_dictionary() {
    let d = tempfile::tempdir().unwrap();
    let l = build(d.path(), LogFormat::V4);
    let id = l.dictionary_ids()[0].clone();
    drop(l);
    let mut r = VersionedLog::open(d.path(), "tester").unwrap();
    r.set_clock(tick_clock());
    r.set_dict_policy(small_policy());
    for i in 0..3 {
        let before = manifest_bytes(&r);
        let info = r.append_events(&[ev("s", "2025-09-01", &format!("after reopen {i} walked to the station"), &[])]).unwrap();
        let after = manifest_bytes(&r);
        assert!(after.starts_with(&before), "journal: append only");
        let g = &r.entries.last().unwrap().segments[0];
        assert_eq!(v4log::seg_dict_ids(&fs::read(r.dir().join(&g.file)).unwrap()).unwrap(), vec![id.clone()]);
        assert!(r.has_event(&info.event_ids[0]));
    }
    r.verify().unwrap();
    assert_eq!(r.dictionary_ids(), vec![id]);
    // identical re-submission is a no-op; upgrade calls on a format-4 log never downgrade
    let n = r.latest_version();
    assert!(!r.append_events(&[ev("s", "2025-09-01", "after reopen 0 walked to the station", &[])]).unwrap().created);
    assert!(!r.upgrade_to_v2().unwrap() && !r.upgrade_to_v3().unwrap() && !r.upgrade_to_v4().unwrap());
    assert_eq!(r.latest_version(), n);
    assert_eq!(r.format(), LogFormat::V4);
}

#[test]
fn v4_without_enough_events_has_no_dictionary_and_policy_zero_never_builds_one() {
    let d = tempfile::tempdir().unwrap();
    let mut l = VersionedLog::open_or_create_with_format(d.path(), "tester", tick_clock(), LogFormat::V4).unwrap();
    // default policy: 1000 events
    l.append_events(&[ev("s", "2025-01-01", "one", &[]), ev("s", "2025-01-02", "two", &[])]).unwrap();
    assert!(l.dictionary_ids().is_empty());
    let mut p = small_policy();
    p.train_events = 0;
    l.set_dict_policy(p);
    for b in 0..3 {
        l.append_events(&(0..20).map(|i| ev("s", "2025-02-01", &text_of(b, i), &[])).collect::<Vec<_>>()).unwrap();
    }
    assert!(l.dictionary_ids().is_empty() && dict_files(&l).is_empty());
    l.verify().unwrap();
    assert_eq!(VersionedLog::open(d.path(), "tester").unwrap().open_latest().raw(Table::Events).len(), 62);
}

#[test]
fn v4_drift_lists_a_new_dictionary_and_old_versions_stay_readable() {
    let d = tempfile::tempdir().unwrap();
    let mut l = VersionedLog::open_or_create_with_format(d.path(), "tester", tick_clock(), LogFormat::V4).unwrap();
    // small commits (5 events): there the dictionary carries most of the compression
    l.set_dict_policy(DictPolicy { train_events: 20, max_bytes: 4096, check_events: 40, check_min_text: 300, min_gain_pct: 5 });
    let topic_a = |i: u32| format!("garden diary {i}: watered the tomatoes, the basil and the beans, weeded the beds near the fence, compost turned");
    let topic_b = |i: u32| format!("SERVER LOG {i}: request handled in {} ms, cache miss on shard {}, retry policy exponential, backoff jitter applied", i * 7 % 97, i % 13);
    for b in 0..12u32 {
        l.append_events(&(0..5).map(|i| ev("s", "2025-01-01", &topic_a(b * 5 + i), &[])).collect::<Vec<_>>()).unwrap();
    }
    assert_eq!(l.dictionary_ids().len(), 1);
    let v_a = l.latest_version();
    let hash_a = l.open_at(AsOf::Version(v_a)).unwrap().content_hash();
    for b in 0..24u32 {
        l.append_events(&(0..5).map(|i| ev("s", "2025-01-02", &topic_b(b * 5 + i), &[])).collect::<Vec<_>>()).unwrap();
    }
    let ids = l.dictionary_ids();
    assert!(ids.len() >= 2, "content changed completely: a new dictionary object is listed ({ids:?})");
    assert_eq!(dict_files(&l).len(), ids.len());
    l.verify().unwrap();
    let r = VersionedLog::open(d.path(), "tester").unwrap();
    assert_eq!(r.open_at(AsOf::Version(v_a)).unwrap().content_hash(), hash_a, "time travel before the new dictionary");
    assert_eq!(r.open_latest().raw(Table::Events).len(), 180);
    // each dictionary object is unchanged since it was listed (content-addressed name = sha256)
    for f in dict_files(&r) {
        let sd = kore_store::SharedDict::read_file(&f).unwrap();
        assert_eq!(format!("{}.kdic", sd.id_hex()), f.file_name().unwrap().to_string_lossy());
    }
}

// ─── compatibility ───────────────────────────────────────────────────────────

#[test]
fn v4_is_refused_by_older_readers() {
    let d = tempfile::tempdir().unwrap();
    let l = build(d.path(), LogFormat::V4);
    // older binaries accept only the headers `... 1` and `... 2` (tested with a real pre-format-4 binary,
    // see docs/log_format4.md); the format-4 header is a third value
    let m = manifest_bytes(&l);
    assert!(m.starts_with(b"KORE-VLOG-MANIFEST 3\n"));
    assert!(!m.starts_with(manifest::HEADER.as_bytes()) && !m.starts_with(manifest::HEADER_J.as_bytes()));
    // a dictionary listing under an older header is refused even by this parser
    let mut m2 = m.clone();
    m2[19] = b'2';
    assert!(manifest::parse_ex(&m2).is_err());
    // segment readers that predate v4: the format-2 codec and the kore-store default reader refuse
    let g = l.entries.iter().rev().flat_map(|e| e.segments.iter()).find(|g| g.table == "events").unwrap();
    let b = fs::read(l.dir().join(&g.file)).unwrap();
    assert!(codec::decode_segment(Table::Events, &b).is_err());
    let e = kore_store::KoreReader::from_bytes(&b).unwrap_err().to_string();
    assert!(e.contains("dictionary"), "{e}");
    // with the dictionaries it reads
    assert!(kore_store::KoreReader::from_bytes_with_dicts(&b, &l.dicts).is_ok());
}

fn fixture(name: &str) -> (tempfile::TempDir, serde_json::Value) {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data").join(name);
    let tmp = tempfile::tempdir().unwrap();
    copy_dir(&base.join("fixture"), &tmp.path().join("fixture"));
    let exp: serde_json::Value = serde_json::from_slice(&fs::read(base.join("expected.json")).unwrap()).unwrap();
    (tmp, exp)
}

fn check_expected(l: &VersionedLog, exp: &serde_json::Value) {
    for v in exp["versions"].as_array().unwrap() {
        let n = v["v"].as_u64().unwrap();
        assert_eq!(l.snapshot_hash(n).unwrap(), v["manifest_hash"].as_str().unwrap(), "manifest hash at version {n}");
        let s = l.open_at(AsOf::Version(n)).unwrap();
        assert_eq!(s.content_hash(), v["content_hash"].as_str().unwrap(), "content hash at version {n}");
        assert_eq!(s.recorded_at, v["recorded_at"].as_str().unwrap());
    }
}

#[test]
fn explicit_upgrade_to_v4_from_format_1_2_and_3_fixtures() {
    for (name, hdr) in [("vlog_v1", &b"KORE-VLOG-MANIFEST 1\n"[..]), ("vlog_v2_r1/fixture", b"KORE-VLOG-MANIFEST 1\n"), ("vlog_v3", b"KORE-VLOG-MANIFEST 2\n")] {
        let (tmp, exp) = fixture(name);
        let mut l = VersionedLog::open(tmp.path(), "fixture").unwrap();
        l.set_clock(tick_clock());
        l.set_dict_policy(small_policy());
        assert_ne!(l.format(), LogFormat::V4);
        // ordinary commits never switch to format 4
        l.append_events(&[ev("journal", "2025-09-01", "plain commit", &[])]).unwrap();
        let mid = manifest_bytes(&l);
        assert!(mid.starts_with(hdr), "{name}: no silent upgrade");
        assert!(l.upgrade_to_v4().unwrap());
        assert!(!l.upgrade_to_v4().unwrap());
        assert_eq!(l.format(), LogFormat::V4);
        let after = manifest_bytes(&l);
        let nl = |b: &[u8]| b.iter().position(|&c| c == b'\n').unwrap() + 1;
        assert!(after.starts_with(b"KORE-VLOG-MANIFEST 3\n"));
        assert!(after[nl(&after)..].starts_with(&mid[nl(&mid)..]), "{name}: every old entry line is byte-identical");
        check_expected(&l, &exp);
        // enough new events for a dictionary
        let mut new_ids = vec![];
        for b in 0..3u32 {
            new_ids.extend(l.append_events(&(0..20).map(|i| ev("journal", "2025-09-02", &text_of(b, i), &[])).collect::<Vec<_>>()).unwrap().event_ids);
        }
        assert_eq!(l.dictionary_ids().len(), 1, "{name}");
        l.verify().unwrap();
        let after2 = manifest_bytes(&l);
        assert!(after2.starts_with(&after), "{name}: journal from here on");
        let mut r = VersionedLog::open(tmp.path(), "fixture").unwrap();
        assert_eq!(r.format(), LogFormat::V4);
        check_expected(&r, &exp);
        assert_eq!(r.open_latest().content_hash(), l.open_latest().content_hash());
        for id in &new_ids {
            assert_eq!(&r.get_event(id).unwrap().unwrap().event_id, id);
        }
        for f in exp["forgotten_ids"].as_array().unwrap() {
            assert!(r.is_forgotten(f.as_str().unwrap()));
        }
        // forget on the mixed log: an old-format event and a format-4 event (whose text is in the dictionary)
        r.set_clock(tick_clock());
        let old_id = exp["event_ids"][0].as_str().unwrap().to_string();
        let rep = r.forget_events(&[old_id.clone(), new_ids[5].clone()]).unwrap();
        assert!(rep.created && rep.dictionaries_rebuilt == 1, "{name}: {rep:?}");
        r.verify().unwrap();
        assert!(manifest_bytes(&r).starts_with(b"KORE-VLOG-MANIFEST 3\n"));
        assert!(!r.has_event(&old_id) && !r.has_event(&new_ids[5]));
        for f in dict_files(&r) {
            assert!(!contains(&fs::read(&f).unwrap(), text_of(0, 5).as_bytes()), "{name}: forgotten text left in a dictionary");
        }
        let r2 = VersionedLog::open(tmp.path(), "fixture").unwrap();
        r2.verify().unwrap();
        assert_eq!(r2.open_latest().content_hash(), r.open_latest().content_hash());
    }
}

fn check_v4_fixture(name: &str) {
    let (tmp, exp) = fixture(name);
    assert_eq!(exp["format"].as_i64().unwrap(), 4);
    let l = VersionedLog::open(tmp.path(), "fixture").unwrap();
    l.verify().unwrap();
    assert_eq!(l.format(), LogFormat::V4);
    assert_eq!(l.latest_version(), exp["latest"].as_u64().unwrap());
    assert!(!l.dictionary_ids().is_empty(), "the fixture uses a dictionary");
    check_expected(&l, &exp);
    let b = sql::run_readonly_sql(&l.open_latest(), "SELECT kind, COUNT(*) FROM events GROUP BY kind ORDER BY kind").unwrap();
    let want: Vec<Vec<String>> = serde_json::from_value(exp["sql_kind_counts"].clone()).unwrap();
    assert_eq!(sql::block_strings(&b), want);
    let want_ids: Vec<String> = serde_json::from_value(exp["event_ids"].clone()).unwrap();
    let l2 = VersionedLog::open(tmp.path(), "fixture").unwrap();
    for id in &want_ids {
        assert_eq!(&l2.get_event(id).unwrap().unwrap().event_id, id);
    }
    let mut streamed = vec![];
    l2.for_each_event(|e| {
        streamed.push(e.event_id);
        true
    })
    .unwrap();
    assert_eq!(streamed, want_ids);
    for f in exp["forgotten_ids"].as_array().unwrap() {
        assert!(!l.has_event(f.as_str().unwrap()) && l.is_forgotten(f.as_str().unwrap()));
    }
    if name.ends_with("mixed") {
        return; // one more commit (the upgrade), so later recorded_at values differ from the format-3 fixture
    }
    // same rows as the format-3 fixture written from the same operations (by the pre-format-4 binary)
    let (t3, _) = fixture("vlog_v3");
    let l3 = VersionedLog::open(t3.path(), "fixture").unwrap();
    for t in [Table::Events, Table::Revisions, Table::RevisionCauses, Table::Predictions, Table::Outcomes] {
        assert_eq!(l3.open_latest().raw(t), l.open_latest().raw(t), "{name}: {} equals the format-3 fixture", t.name());
    }
}

#[test]
fn committed_v4_fixtures_open_verify_and_query_identically() {
    check_v4_fixture("vlog_v4/fixture");
    check_v4_fixture("vlog_v4/fixture_mixed");
}

#[test]
fn committed_fixture_dirs_have_segments_and_a_manifest() {
    // the root .gitignore ignores *.kore; kore-self/tests/data/ has an exception. A missing segment would
    // also fail the fixture tests above in a fresh checkout.
    for name in ["vlog_v3/fixture/log", "vlog_v4/fixture/fixture/log", "vlog_v4/fixture_mixed/fixture/log"] {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data").join(name);
        let mut f = vec![];
        all_files(&p, &mut f);
        assert!(f.iter().any(|x| x.extension().map_or(false, |e| e == "kore")), "{name}");
        assert!(p.join("MANIFEST").exists());
    }
}

// ─── tamper ──────────────────────────────────────────────────────────────────

#[test]
fn v4_tamper_every_byte_of_dictionary_manifest_and_a_segment_is_detected() {
    let d = tempfile::tempdir().unwrap();
    let l = build(d.path(), LogFormat::V4);
    // every byte of the dictionary object
    let df = dict_files(&l)[0].clone();
    let orig = fs::read(&df).unwrap();
    for i in 0..orig.len() {
        let mut b = orig.clone();
        b[i] ^= 1 << (i % 8);
        fs::write(&df, &b).unwrap();
        assert!(matches!(l.verify(), Err(VerifyError::ChecksumMismatch { .. })), "dictionary byte {i}");
        assert!(VersionedLog::open(d.path(), "tester").is_err(), "open refuses a changed dictionary (byte {i})");
    }
    // truncated, extended, missing, replaced by another valid dictionary under the same name
    for b in [orig[..orig.len() - 1].to_vec(), [orig.clone(), vec![0]].concat()] {
        fs::write(&df, &b).unwrap();
        assert!(l.verify().is_err());
    }
    let other = kore_store::SharedDict::from_content(b"some other valid dictionary content").unwrap();
    fs::write(&df, other.to_bytes()).unwrap();
    assert!(matches!(l.verify(), Err(VerifyError::ChecksumMismatch { .. })));
    fs::remove_file(&df).unwrap();
    assert!(matches!(l.verify(), Err(VerifyError::MissingSegment { .. })));
    assert!(VersionedLog::open(d.path(), "tester").is_err());
    fs::write(&df, &orig).unwrap();
    l.verify().unwrap();
    // an unlisted dictionary file is reported
    let stray = l.dir().join("dicts").join(other.file_name());
    fs::write(&stray, other.to_bytes()).unwrap();
    assert!(matches!(l.verify(), Err(VerifyError::ExtraSegment { .. })));
    fs::remove_file(&stray).unwrap();
    // every byte of the MANIFEST (header included: any flip there is refused, because entries list dictionaries)
    let mpath = l.dir().join("MANIFEST");
    let morig = fs::read(&mpath).unwrap();
    for i in 0..morig.len() {
        let mut b = morig.clone();
        b[i] ^= 1 << (i % 8);
        fs::write(&mpath, &b).unwrap();
        assert!(l.verify().is_err(), "manifest byte {i} not detected");
    }
    fs::write(&mpath, &morig).unwrap();
    // every byte of an events segment that uses the dictionary
    let g = l.entries.iter().rev().flat_map(|e| e.segments.iter()).find(|g| g.table == "events").unwrap();
    let sp = l.dir().join(&g.file);
    let so = fs::read(&sp).unwrap();
    for i in 0..so.len() {
        let mut b = so.clone();
        b[i] ^= 1 << (i % 8);
        fs::write(&sp, &b).unwrap();
        assert!(matches!(l.verify(), Err(VerifyError::ChecksumMismatch { .. })), "segment byte {i}");
    }
    fs::write(&sp, &so).unwrap();
    l.verify().unwrap();
    // truncating the MANIFEST at every length never looks complete
    for cut in 0..morig.len() {
        fs::write(&mpath, &morig[..cut]).unwrap();
        if let Ok(r) = VersionedLog::open(d.path(), "tester") {
            assert!(r.latest_version() < l.latest_version());
        }
    }
    fs::write(&mpath, &morig).unwrap();
    l.verify().unwrap();
}

#[test]
fn v4_verify_refuses_a_dictionary_used_before_it_is_listed() {
    // forge a log whose dictionary listing sits on a later entry than its first use (re-sealed, so only
    // the listing rule can catch it)
    let d = tempfile::tempdir().unwrap();
    let l = build(d.path(), LogFormat::V4);
    let mut entries = l.entries.clone();
    let at = entries.iter().position(|e| !e.dicts.is_empty()).unwrap();
    let dr = entries[at].dicts.remove(0);
    let last = entries.len() - 1;
    entries[last].dicts.push(dr);
    let mut prev = ZERO_HASH.to_string();
    for e in entries.iter_mut() {
        e.reseal(&prev);
        prev = e.hash.clone();
    }
    fs::write(l.dir().join("MANIFEST"), manifest::render(&entries, 3)).unwrap();
    match l.verify() {
        Err(VerifyError::SegmentUnreadable { detail, .. }) => assert!(detail.contains("not listed"), "{detail}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn v4_crashed_commit_leaves_an_unlisted_dictionary_that_the_next_commit_removes() {
    let d = tempfile::tempdir().unwrap();
    let mut l = VersionedLog::open_or_create_with_format(d.path(), "tester", tick_clock(), LogFormat::V4).unwrap();
    l.set_dict_policy(small_policy());
    l.append_events(&[ev("s", "2025-01-01", "first", &[])]).unwrap();
    // a crash after the dictionary object was written, before the MANIFEST append
    let leftover = kore_store::SharedDict::from_content(b"dictionary of a commit that never happened").unwrap();
    fs::create_dir_all(l.dir().join("dicts")).unwrap();
    fs::write(l.dir().join("dicts").join(leftover.file_name()), leftover.to_bytes()).unwrap();
    assert!(matches!(l.verify(), Err(VerifyError::ExtraSegment { .. })));
    let r = VersionedLog::open(d.path(), "tester").unwrap();
    assert_eq!(r.latest_version(), l.latest_version(), "open ignores it");
    l.append_events(&[ev("s", "2025-01-02", "second", &[])]).unwrap();
    l.verify().unwrap();
    assert!(dict_files(&l).is_empty());
}

// ─── forget ──────────────────────────────────────────────────────────────────

#[test]
fn v4_forget_rebuilds_the_dictionary_that_held_the_text() {
    let d = tempfile::tempdir().unwrap();
    let mut l = build(d.path(), LogFormat::V4);
    let old_dict = dict_files(&l)[0].clone();
    let old_id = l.dictionary_ids()[0].clone();
    let all = l.open_latest().rows::<EventRow>();
    // an event whose text is a member of the dictionary (the newest texts at the listing commit)
    let sd = kore_store::SharedDict::read_file(&old_dict).unwrap();
    let victim = all.iter().rev().find(|e| v4log::members(&sd).iter().any(|m| *m == e.text.as_bytes())).unwrap().clone();
    assert!(contains(sd.content(), victim.text.as_bytes()));
    let versions_before: Vec<Snapshot> = (0..=l.latest_version()).map(|v| l.open_at(AsOf::Version(v)).unwrap()).collect();
    let rep = l.forget_events(&[victim.event_id.clone()]).unwrap();
    assert!(rep.created);
    assert_eq!((rep.dictionaries_rebuilt, rep.dictionaries_removed), (1, 0));
    l.verify().unwrap();
    // the old object is gone, the new one does not hold the text, and no file of the log has it in clear
    assert!(!old_dict.exists());
    let new_ids = l.dictionary_ids();
    assert_eq!(new_ids.len(), 1);
    assert_ne!(new_ids[0], old_id);
    let mut files = vec![];
    all_files(l.dir(), &mut files);
    for f in &files {
        assert!(!contains(&fs::read(f).unwrap(), victim.text.as_bytes()), "{} holds the forgotten text", f.display());
    }
    let nd = kore_store::SharedDict::read_file(&dict_files(&l)[0]).unwrap();
    assert_eq!(v4log::members(&nd).len(), v4log::members(&sd).len() - 1, "only the forgotten text was removed");
    // no version brings it back; every other row of every version is unchanged
    for (v, before) in versions_before.iter().enumerate() {
        let now = l.open_at(AsOf::Version(v as u64)).unwrap();
        let want: Vec<EventRow> = before.rows::<EventRow>().into_iter().filter(|e| e.event_id != victim.event_id).collect();
        assert_eq!(now.rows::<EventRow>(), want, "version {v}");
    }
    // re-ingest is refused; a reopened handle agrees and keeps writing with the rebuilt dictionary
    assert_eq!(l.append_events(&[ev(&victim.source, &victim.occurred_at, &victim.text, &[])]).unwrap().suppressed, 1);
    let mut r = VersionedLog::open(d.path(), "tester").unwrap();
    r.set_clock(tick_clock());
    assert!(r.is_forgotten(&victim.event_id));
    let info = r.append_events(&[ev("s", "2025-12-01", "a new event after the forget, walked to the station", &[])]).unwrap();
    let g = &r.entries.last().unwrap().segments[0];
    assert_eq!(v4log::seg_dict_ids(&fs::read(r.dir().join(&g.file)).unwrap()).unwrap(), new_ids);
    assert!(r.has_event(&info.event_ids[0]));
    r.verify().unwrap();
}

#[test]
fn v4_forget_of_every_dictionary_text_drops_the_dictionary() {
    let d = tempfile::tempdir().unwrap();
    let mut l = VersionedLog::open_or_create_with_format(d.path(), "tester", tick_clock(), LogFormat::V4).unwrap();
    // a dictionary that holds exactly the texts of the commit that builds it
    l.set_dict_policy(DictPolicy { train_events: 3, max_bytes: 200, check_events: 0, check_min_text: 0, min_gain_pct: 5 });
    let a = l.append_events(&[ev("s", "2025-01-01", "alpha text one", &[]), ev("s", "2025-01-02", "beta text two", &[]), ev("s", "2025-01-03", "gamma text three", &[])]).unwrap();
    assert_eq!(l.dictionary_ids().len(), 1);
    let b = l.append_events(&[ev("s", "2025-01-04", "delta text four", &[])]).unwrap();
    let rep = l.forget_events(&a.event_ids).unwrap();
    assert_eq!((rep.dictionaries_rebuilt, rep.dictionaries_removed), (0, 1));
    l.verify().unwrap();
    assert!(l.dictionary_ids().is_empty() && dict_files(&l).is_empty());
    assert_eq!(l.get_event(&b.event_ids[0]).unwrap().unwrap().text, "delta text four");
    let r = VersionedLog::open(d.path(), "tester").unwrap();
    assert_eq!(r.open_latest().rows::<EventRow>().len(), 1);
    // a forget of an event that is not in any dictionary leaves the dictionaries alone
    let d2 = tempfile::tempdir().unwrap();
    let mut l2 = build(d2.path(), LogFormat::V4);
    // (the dictionary was built at the second events commit: events of later commits are not in it)
    let first = l2.open_latest().rows::<EventRow>()[110].clone();
    let before = l2.dictionary_ids();
    let rep = l2.forget_events(&[first.event_id]).unwrap();
    assert_eq!((rep.dictionaries_rebuilt, rep.dictionaries_removed), (0, 0));
    assert_eq!(l2.dictionary_ids(), before);
    l2.verify().unwrap();
}

// ─── restart path ────────────────────────────────────────────────────────────

#[test]
fn v4_streamed_open_events_only_read_and_peek() {
    let d = tempfile::tempdir().unwrap();
    let l = build(d.path(), LogFormat::V4);
    let dir = l.dir().to_path_buf();
    let rows = l.open_latest().rows::<EventRow>();
    drop(l);
    let entries = manifest::parse_ex(&read_manifest_raw(&dir).unwrap()).unwrap().entries;
    let full = load_state_full(&dir, &entries).unwrap();
    let st = load_state_streamed(&dir, &entries).unwrap().expect("a regular format-4 log is streamed");
    assert!(st.index.same_as(&full.index));
    assert_eq!(st.ev_segs, full.ev_segs);
    assert_eq!(st.format, LogFormat::V4);
    let r = VersionedLog::open(d.path(), "tester").unwrap();
    let mut seen = vec![];
    r.for_each_event(|e| {
        seen.push(e);
        true
    })
    .unwrap();
    assert_eq!(seen, rows);
    assert!(r.head.lock().unwrap().is_none(), "the events-only read keeps nothing");
    let mut peeked = vec![];
    VersionedLog::peek_events(d.path(), "tester", |e| {
        peeked.push(e);
        peeked.len() < 25
    })
    .unwrap();
    assert_eq!(&peeked[..], &rows[..25]);
    // a changed dictionary is caught by the peek and by open
    let df = dict_files(&r)[0].clone();
    let orig = fs::read(&df).unwrap();
    let mut b = orig.clone();
    b[20] ^= 4;
    fs::write(&df, &b).unwrap();
    assert!(VersionedLog::peek_events(d.path(), "tester", |_| true).is_err());
    assert!(VersionedLog::open(d.path(), "tester").is_err());
    fs::write(&df, &orig).unwrap();
    // a changed segment is caught by the events-only read
    let g = r.entries.iter().rev().flat_map(|e| e.segments.iter()).find(|g| g.table == "events").unwrap();
    let sp = r.dir().join(&g.file);
    let so = fs::read(&sp).unwrap();
    let mut b = so.clone();
    let k = b.len() / 2;
    b[k] ^= 1;
    fs::write(&sp, &b).unwrap();
    assert!(r.for_each_event(|_| true).is_err());
    fs::write(&sp, &so).unwrap();
    r.verify().unwrap();
}
