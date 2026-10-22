//! Tests for the versioned log. All use `tempfile::tempdir`; nothing touches
//! the real home directory.

use super::sql::{block_strings, check_readonly, run_readonly_sql};
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// ─── helpers ─────────────────────────────────────────────────────────────────

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Deterministic clock: each call is one minute later (n < 1440).
fn tick_clock() -> impl Fn() -> String + Send + 'static {
    let n = Arc::new(AtomicU64::new(0));
    move || {
        let k = n.fetch_add(1, Ordering::SeqCst) + 1;
        format!("2026-03-01T{:02}:{:02}:00Z", k / 60, k % 60)
    }
}

fn ticking(log: &mut VersionedLog) {
    log.set_clock(tick_clock());
}

fn new_log(root: &Path) -> VersionedLog {
    VersionedLog::open_or_create_with_clock(root, "tester", tick_clock()).unwrap()
}

fn ev(source: &str, occurred: &str, text: &str) -> NewEvent {
    NewEvent { source: source.into(), occurred_at: occurred.into(), kind: "note".into(), text: text.into(), tags: vec![], redacted: false }
}

fn rev(key: &str, stance: &str, conf: f64, valid_from: &str, author: &str, causes: Vec<String>) -> NewRevision {
    NewRevision {
        entity_type: "belief".into(), key: key.into(), stance: stance.into(), confidence: conf, rationale: format!("because {stance}"),
        supersedes: None, valid_from: valid_from.into(), author: author.into(), caused_by: causes,
    }
}

fn manifest_bytes(l: &VersionedLog) -> Vec<u8> {
    fs::read(l.dir().join("MANIFEST")).unwrap()
}

fn seg_files(dir: &Path) -> Vec<PathBuf> {
    let mut v = vec![];
    for t in ALL_TABLES {
        if let Ok(rd) = fs::read_dir(dir.join(t.name())) {
            v.extend(rd.flatten().map(|f| f.path()));
        }
    }
    v.sort();
    v
}

/// Apply one random operation; returns true if it was a (possibly no-op) call.
fn random_op(l: &mut VersionedLog, rng: &mut Rng, ids: &mut Vec<String>, preds: &mut Vec<(String, Vec<String>)>, n: usize) {
    match rng.below(5) {
        0 | 1 => {
            let k = 1 + rng.below(4) as usize;
            let mut batch = vec![];
            for j in 0..k {
                // Small text space so duplicates actually occur.
                let t = format!("event text {}", rng.below(12));
                let day = 1 + rng.below(27);
                batch.push(ev("src", &format!("2025-02-{day:02}T08:00:00Z"), &format!("{t} #{j}{n}")));
                if rng.below(3) == 0 {
                    batch.push(ev("src", &format!("2025-02-{day:02}"), &t));
                }
            }
            let info = l.append_events(&batch).unwrap();
            for id in info.event_ids {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        2 if !ids.is_empty() => {
            let c = vec![ids[rng.below(ids.len() as u64) as usize].clone()];
            let key = format!("k{}", rng.below(3));
            l.revise(rev(&key, &format!("stance {}", rng.below(4)), (rng.below(101) as f64) / 100.0, "2026-01-01", "rule:r1", c)).unwrap();
        }
        3 if !ids.is_empty() => {
            let p = NewPrediction {
                made_at: "2026-01-02".into(), question_id: format!("q{}", rng.below(5)), target: "t".into(),
                options: vec!["a".into(), "b".into()], probs: vec![0.25, 0.75], model_id: "m".into(), model_version: "1".into(),
                features_cite: vec![ids[0].clone()], resolves_by: None,
            };
            let info = l.record_prediction(p).unwrap();
            preds.push((info.pred_ids[0].clone(), vec!["a".into(), "b".into()]));
        }
        _ => {
            if let Some((pid, _)) = preds.pop() {
                let _ = l.record_outcome(NewOutcome { pred_id: pid, resolved_at: "2026-02-01".into(), actual: "a".into(), outcome_event_id: None });
            }
        }
    }
}

// ─── invariants (property style) ─────────────────────────────────────────────

#[test]
fn time_travel_invariant_random_sequences() {
    for seed in 1..=6u64 {
        let dir = tempfile::tempdir().unwrap();
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut l = new_log(dir.path());
        let (mut ids, mut preds) = (vec![], vec![]);
        let mut seen: Vec<(u64, String, String)> = vec![]; // (v, snapshot_hash, content_hash) taken right after commit
        let mut last_v = l.latest_version();
        for n in 0..40 {
            random_op(&mut l, &mut rng, &mut ids, &mut preds, n);
            if l.latest_version() != last_v {
                last_v = l.latest_version();
                let s = l.open_at(AsOf::Version(last_v)).unwrap();
                seen.push((last_v, l.snapshot_hash(last_v).unwrap(), s.content_hash()));
            }
        }
        assert!(seen.len() >= 5, "seed {seed}: sequence too uneventful");
        let check = |l: &VersionedLog, tag: &str| {
            for (v, h, c) in &seen {
                assert_eq!(&l.snapshot_hash(*v).unwrap(), h, "seed {seed} {tag}: snapshot_hash({v}) changed");
                assert_eq!(&l.open_at(AsOf::Version(*v)).unwrap().content_hash(), c, "seed {seed} {tag}: content of v{v} changed");
            }
        };
        check(&l, "after later appends");
        let head_hash = l.snapshot_hash(l.latest_version()).unwrap();
        drop(l);
        let l2 = VersionedLog::open(dir.path(), "tester").unwrap();
        check(&l2, "after reopen");
        assert_eq!(l2.snapshot_hash(l2.latest_version()).unwrap(), head_hash);
        l2.verify().unwrap();
    }
}

#[test]
fn append_only_prefix_and_segments_never_modified() {
    let dir = tempfile::tempdir().unwrap();
    let mut rng = Rng(77);
    let mut l = new_log(dir.path());
    let (mut ids, mut preds) = (vec![], vec![]);
    let mut prev_manifest = manifest_bytes(&l);
    // path -> (mtime, sha256 of bytes)
    let mut recorded: HashMap<PathBuf, (std::time::SystemTime, String)> = HashMap::new();
    let mut commits = 0;
    for n in 0..45 {
        let before = l.latest_version();
        random_op(&mut l, &mut rng, &mut ids, &mut preds, n);
        let now = manifest_bytes(&l);
        if l.latest_version() == before {
            assert_eq!(now, prev_manifest, "no-op call must not touch the manifest");
        } else {
            commits += 1;
            assert!(now.len() > prev_manifest.len() && now.starts_with(&prev_manifest), "manifest(v) must be a strict prefix of manifest(v+1)");
        }
        prev_manifest = now;
        for f in seg_files(l.dir()) {
            let meta = fs::metadata(&f).unwrap().modified().unwrap();
            let sha = sha256_hex(&[&fs::read(&f).unwrap()]);
            match recorded.get(&f) {
                Some((m, s)) => assert!(*m == meta && *s == sha, "segment {} was modified", f.display()),
                None => {
                    recorded.insert(f, (meta, sha));
                }
            }
        }
    }
    assert!(commits >= 10);
    // Versions are strictly increasing prefixes of the same chain.
    let vs = l.versions();
    for w in vs.windows(2) {
        assert_eq!(w[1].v, w[0].v + 1);
        assert_ne!(w[0].manifest_hash, w[1].manifest_hash);
    }
}

#[test]
fn idempotent_ingest() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = new_log(dir.path());
    let batch = vec![ev("journal", "2025-05-01", "went for a run"), ev("journal", "2025-05-02", "skipped the run")];
    let a = l.append_events(&batch).unwrap();
    assert!(a.created);
    let (n, h, c) = (l.versions().len(), l.snapshot_hash(a.version).unwrap(), l.open_latest().content_hash());
    let b = l.append_events(&batch).unwrap();
    assert!(!b.created, "all-duplicate batch must not create a version");
    assert_eq!(b.version, a.version);
    assert_eq!(b.event_ids, a.event_ids);
    assert_eq!(l.versions().len(), n);
    assert_eq!(l.snapshot_hash(a.version).unwrap(), h);
    assert_eq!(l.open_latest().content_hash(), c);
    // Mixed batch adds only the new row; whitespace-normalised text is the same event.
    let m = l.append_events(&[ev("journal", "2025-05-01", "  went  for a\trun "), ev("journal", "2025-05-03", "new")]).unwrap();
    assert!(m.created);
    assert_eq!(m.new_rows["events"], 1);
    assert_eq!(l.open_latest().rows::<EventRow>().len(), 3);
    // Identical revision (retry) is idempotent too; a changed one is a new row.
    let id = l.revise(rev("k", "yes", 0.5, "2025-06-01", "user", vec![])).unwrap();
    let nv = l.latest_version();
    assert_eq!(l.revise(rev("k", "yes", 0.5, "2025-06-01", "user", vec![])).unwrap(), id);
    assert_eq!(l.latest_version(), nv, "retrying the same revision must not create a version");
    let id2 = l.revise(rev("k", "no", 0.5, "2025-06-02", "user", vec![])).unwrap();
    assert_ne!(id2, id);
    assert_eq!(l.latest_version(), nv + 1);
}

// ─── tamper detection ────────────────────────────────────────────────────────

fn populated(root: &Path) -> VersionedLog {
    let mut l = new_log(root);
    let e = l.append_events(&[ev("s", "2025-01-01", "a"), ev("s", "2025-01-02", "b")]).unwrap();
    l.revise(rev("k", "x", 0.6, "2025-01-03", "rule:r", vec![e.event_ids[0].clone()])).unwrap();
    l.append_events(&[ev("s", "2025-01-04", "c")]).unwrap();
    l.revise(rev("k", "y", 0.7, "2025-01-05", "user", vec![e.event_ids[1].clone()])).unwrap();
    l.verify().unwrap();
    l
}

#[test]
fn tamper_flipped_byte_names_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let l = populated(dir.path());
    let target = l.dir().join("events").join(format!("{:010}.kore", 1));
    let mut b = fs::read(&target).unwrap();
    let mid = b.len() / 2;
    b[mid] ^= 0x01;
    fs::write(&target, b).unwrap();
    match l.verify().unwrap_err() {
        VerifyError::ChecksumMismatch { file } => assert_eq!(file, "events/0000000001.kore"),
        e => panic!("unexpected {e:?}"),
    }
    assert!(l.open_at(AsOf::Version(2)).is_err(), "reading a tampered segment must fail, not return data");
}

#[test]
fn tamper_manifest_truncated_missing_segment_reorder_edit() {
    // truncated mid-line
    let dir = tempfile::tempdir().unwrap();
    let l = populated(dir.path());
    let m = manifest_bytes(&l);
    fs::write(l.dir().join("MANIFEST"), &m[..m.len() - 7]).unwrap();
    assert!(matches!(l.verify().unwrap_err(), VerifyError::ManifestCorrupt(_)));
    assert!(VersionedLog::open(dir.path(), "tester").is_err());

    // truncated to zero bytes
    fs::write(l.dir().join("MANIFEST"), b"").unwrap();
    assert!(matches!(l.verify().unwrap_err(), VerifyError::ManifestCorrupt(_)));

    // truncated at a line boundary (drops the last commit): detected via the
    // unlisted segment and via an externally recorded head hash.
    let dir = tempfile::tempdir().unwrap();
    let l = populated(dir.path());
    let head = l.snapshot_hash(l.latest_version()).unwrap();
    let m = String::from_utf8(manifest_bytes(&l)).unwrap();
    let lines: Vec<&str> = m.lines().collect();
    fs::write(l.dir().join("MANIFEST"), format!("{}\n", lines[..lines.len() - 1].join("\n"))).unwrap();
    assert!(matches!(l.verify().unwrap_err(), VerifyError::ExtraSegment { .. }));
    assert!(l.verify_against(&head).is_err());

    // deleted segment
    let dir = tempfile::tempdir().unwrap();
    let l = populated(dir.path());
    fs::remove_file(l.dir().join("revisions").join(format!("{:010}.kore", 2))).unwrap();
    match l.verify().unwrap_err() {
        VerifyError::MissingSegment { file } => assert_eq!(file, "revisions/0000000002.kore"),
        e => panic!("unexpected {e:?}"),
    }

    // reordered entries
    let dir = tempfile::tempdir().unwrap();
    let l = populated(dir.path());
    let m = String::from_utf8(manifest_bytes(&l)).unwrap();
    let mut lines: Vec<&str> = m.lines().collect();
    lines.swap(2, 3);
    fs::write(l.dir().join("MANIFEST"), format!("{}\n", lines.join("\n"))).unwrap();
    assert!(matches!(l.verify().unwrap_err(), VerifyError::ChainBroken { .. }));

    // edited entry (row count) with the old hash
    let dir = tempfile::tempdir().unwrap();
    let l = populated(dir.path());
    let m = String::from_utf8(manifest_bytes(&l)).unwrap();
    let edited = m.replacen("\"rows\":2", "\"rows\":3", 1);
    assert_ne!(m, edited);
    fs::write(l.dir().join("MANIFEST"), edited).unwrap();
    assert!(matches!(l.verify().unwrap_err(), VerifyError::ChainBroken { .. }));
}

// ─── crash consistency ───────────────────────────────────────────────────────

#[test]
fn crash_between_segment_write_and_manifest_rename() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = populated(dir.path());
    let v = l.latest_version();
    let h = l.snapshot_hash(v).unwrap();
    let content = l.open_latest().content_hash();
    let manifest_before = manifest_bytes(&l);
    l.simulate_crash_after_segments(Batch { events: vec![ev("s", "2025-02-01", "lost")], ..Default::default() }).unwrap();
    drop(l);

    let mut l = VersionedLog::open_or_create_with_clock(dir.path(), "tester", tick_clock()).unwrap();
    assert_eq!(l.latest_version(), v, "version must be unchanged after the crash");
    assert_eq!(l.snapshot_hash(v).unwrap(), h);
    assert_eq!(l.open_latest().content_hash(), content, "stray segments must be ignored");
    assert_eq!(manifest_bytes(&l), manifest_before);
    assert!(matches!(l.verify().unwrap_err(), VerifyError::ExtraSegment { .. }), "strays are reported until cleaned");
    let removed = l.recover().unwrap();
    assert!(removed.len() >= 2, "stray segment and temp file removed: {removed:?}");
    l.verify().unwrap();
    // Writing after recovery works and reuses the sequence number.
    let info = l.append_events(&[ev("s", "2025-02-01", "kept")]).unwrap();
    assert_eq!(info.version, v + 1);
    l.verify().unwrap();
}

#[test]
fn commit_cleans_strays_itself() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = populated(dir.path());
    l.simulate_crash_after_segments(Batch { events: vec![ev("s", "2025-02-01", "lost")], ..Default::default() }).unwrap();
    l.append_events(&[ev("s", "2025-02-02", "after crash")]).unwrap();
    l.verify().unwrap();
    assert_eq!(l.open_latest().rows::<EventRow>().len(), 4);
}

// ─── bitemporal ──────────────────────────────────────────────────────────────

#[test]
fn bitemporal_backfill_and_open_at_time() {
    let dir = tempfile::tempdir().unwrap();
    // recorded_at comes from the clock; occurred_at is always caller-supplied.
    let mut l = VersionedLog::open_or_create_with_clock(dir.path(), "bt", || "2026-05-30T00:00:00Z".to_string()).unwrap();
    let times = Arc::new(std::sync::Mutex::new(vec![
        "2026-05-01T00:00:00Z".to_string(), // goes backwards: must be clamped to the previous recorded_at
        "2026-06-02T00:00:00Z".to_string(),
        "2026-06-01T00:00:00Z".to_string(),
    ]));
    let t2 = times.clone();
    l.set_clock(move || t2.lock().unwrap().pop().unwrap_or_else(|| "2026-06-09T00:00:00Z".into()));
    // pop order: 06-01, 06-02, then 05-01 (clamped)
    let a = l.append_events(&[ev("import", "2019-01-01", "old fact")]).unwrap();
    let b = l.append_events(&[ev("import", "2018-05-05", "older fact")]).unwrap();
    assert_eq!(a.recorded_at, "2026-06-01T00:00:00Z");
    assert_eq!(b.recorded_at, "2026-06-02T00:00:00Z");
    let s = l.open_at(AsOf::Time("2026-06-01T12:00:00Z".into())).unwrap();
    let evs: Vec<EventRow> = s.rows();
    assert_eq!(evs.len(), 1, "open_at(time) is by recorded_at, not occurred_at");
    assert_eq!(evs[0].occurred_at, "2019-01-01T00:00:00Z");
    assert_eq!(s.version, a.version);
    let s2 = l.open_at(AsOf::Time("2026-06-02".into())).unwrap();
    assert_eq!(s2.version, b.version);
    assert_eq!(s2.rows::<EventRow>().len(), 2, "backfilled older event appears only in the later version");
    assert!(l.open_at(AsOf::Time("2020-01-01".into())).is_err());
    // Monotonic clamp
    let c = l.append_events(&[ev("import", "2017-01-01", "third")]).unwrap();
    assert_eq!(c.recorded_at, b.recorded_at, "a clock that goes backwards is clamped to the last recorded_at");
}

// ─── validation ──────────────────────────────────────────────────────────────

#[test]
fn caused_by_and_other_validation() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = new_log(dir.path());
    let v0 = l.latest_version();
    let e = l.append_events(&[ev("s", "2025-01-01", "fact")]).unwrap();
    let good = e.event_ids[0].clone();
    // unknown event id rejected
    let err = l.revise(rev("k", "x", 0.5, "2025-01-02", "user", vec!["deadbeef".into()])).unwrap_err();
    assert!(matches!(err, LogError::NotFound(_)), "{err:?}");
    // rule author without causes rejected
    let err = l.revise(rev("k", "x", 0.5, "2025-01-02", "rule:r1", vec![])).unwrap_err();
    assert!(matches!(err, LogError::Invalid(_)), "{err:?}");
    // bad author / confidence / entity / timestamps / kind
    assert!(l.revise(rev("k", "x", 0.5, "2025-01-02", "robot", vec![])).is_err());
    assert!(l.revise(rev("k", "x", 1.5, "2025-01-02", "user", vec![])).is_err());
    assert!(l.revise(rev("k", "x", f64::NAN, "2025-01-02", "user", vec![])).is_err());
    assert!(l.revise(rev("k", "x", 0.5, "yesterday", "user", vec![])).is_err());
    assert!(l.revise(NewRevision { entity_type: "mood".into(), ..rev("k", "x", 0.5, "2025-01-02", "user", vec![]) }).is_err());
    assert!(l.append_events(&[NewEvent { kind: "gossip".into(), ..ev("s", "2025-01-01", "t") }]).is_err());
    assert!(l.append_events(&[ev("s", "2025-13-01", "t")]).is_err());
    assert!(l.append_events(&[ev("s", "2025-02-30", "t")]).is_err());
    assert!(l.append_events(&[ev("a|b", "2025-01-01", "t")]).is_err());
    assert!(l.append_events(&[ev("s", "2025-01-01", "   ")]).is_err());
    assert!(l.revise(NewRevision { supersedes: Some("nope".into()), ..rev("k", "x", 0.5, "2025-01-02", "user", vec![]) }).is_err());
    // a failed batch commits nothing, even the valid events in it
    let before = l.latest_version();
    let r = l.commit_batch(Batch {
        events: vec![ev("s", "2025-03-03", "valid but batch fails")],
        revisions: vec![rev("k", "x", 0.5, "2025-01-02", "rule:r", vec![])],
        ..Default::default()
    });
    assert!(r.is_err());
    assert_eq!(l.latest_version(), before);
    assert!(before > v0);
    // a revision may cite an event from the same batch
    let ne = ev("s", "2025-04-04", "in same batch");
    let eid = event_id("s", "2025-04-04T00:00:00Z", "in same batch");
    let info = l.commit_batch(Batch { events: vec![ne], revisions: vec![rev("k", "x", 0.5, "2025-04-05", "rule:r", vec![eid])], ..Default::default() }).unwrap();
    assert_eq!(info.new_rows["revision_causes"], 1);
    // uncited user revision is allowed but flagged
    l.revise(rev("k2", "z", 0.5, "2025-04-06", "user", vec![])).unwrap();
    let s = l.open_latest();
    let b = run_readonly_sql(&s, "SELECT key, unsupported FROM revisions ORDER BY valid_from").unwrap();
    assert_eq!(block_strings(&b), vec![vec!["k", "0"], vec!["k2", "1"]]);
    l.verify().unwrap();
}

#[test]
fn supersedes_views_and_valid_time() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = new_log(dir.path());
    let e = l.append_events(&[ev("s", "2025-01-01", "x")]).unwrap().event_ids;
    let r1 = l.revise(rev("work", "office", 0.8, "2025-01-10", "rule:a", e.clone())).unwrap();
    let r2 = l.revise(rev("work", "remote", 0.7, "2025-03-10", "rule:a", e.clone())).unwrap();
    l.revise(rev("sleep", "early", 0.5, "2025-02-01", "rule:a", e.clone())).unwrap();
    let revs: Vec<RevisionRow> = l.open_latest().rows();
    assert_eq!(revs[0].supersedes, None);
    assert_eq!(revs[1].supersedes.as_deref(), Some(r1.as_str()), "default supersedes = previous head");
    let s = l.open_latest();
    let cur = run_readonly_sql(&s, "SELECT key, stance FROM beliefs_current ORDER BY key").unwrap();
    assert_eq!(block_strings(&cur), vec![vec!["sleep", "early"], vec!["work", "remote"]]);
    // valid-time view: before r2 became valid, the earlier revision is in effect.
    let early = l.open_latest().with_valid_at("2025-02-15").unwrap();
    let asof = run_readonly_sql(&early, "SELECT key, stance FROM beliefs_asof ORDER BY key").unwrap();
    assert_eq!(block_strings(&asof), vec![vec!["sleep", "early"], vec!["work", "office"]]);
    // explicit supersedes must match the key
    let other = l.revise(rev("sleep", "late", 0.5, "2025-05-01", "rule:a", e)).unwrap();
    let bad = NewRevision { supersedes: Some(r2), ..rev("sleep", "x", 0.5, "2025-05-02", "user", vec![]) };
    assert!(l.revise(bad).is_err());
    let _ = other;
    // Corrections are rows, never edits: row count only grows.
    let n: Vec<u64> = l.versions().iter().map(|v| v.rows["revisions"]).collect();
    assert!(n.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn predictions_and_outcomes() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = new_log(dir.path());
    let e = l.append_events(&[ev("s", "2025-01-01", "x")]).unwrap().event_ids;
    let p = NewPrediction {
        made_at: "2026-01-01".into(), question_id: "q1".into(), target: "run tomorrow".into(), options: vec!["yes".into(), "no".into()],
        probs: vec![0.7, 0.3], model_id: "freq".into(), model_version: "0".into(), features_cite: e.clone(), resolves_by: Some("2026-01-02".into()),
    };
    let info = l.record_prediction(p.clone()).unwrap();
    assert!(!l.record_prediction(p.clone()).unwrap().created, "same prediction twice is a no-op");
    assert!(l.record_prediction(NewPrediction { probs: vec![0.5, 0.6], ..p.clone() }).is_err());
    assert!(l.record_prediction(NewPrediction { options: vec!["yes".into(), "yes".into()], ..p.clone() }).is_err());
    assert!(l.record_prediction(NewPrediction { features_cite: vec!["zz".into()], ..p.clone() }).is_err());
    let pid = info.pred_ids[0].clone();
    let o = NewOutcome { pred_id: pid.clone(), resolved_at: "2026-01-02".into(), actual: "maybe".into(), outcome_event_id: None };
    assert!(l.record_outcome(o.clone()).is_err(), "actual must be one of the options");
    assert!(l.record_outcome(NewOutcome { pred_id: "nope".into(), ..o.clone() }).is_err());
    l.record_outcome(NewOutcome { actual: "yes".into(), outcome_event_id: Some(e[0].clone()), ..o.clone() }).unwrap();
    assert!(matches!(l.record_outcome(NewOutcome { actual: "no".into(), ..o }).unwrap_err(), LogError::Conflict(_)));
    let s = l.open_latest();
    let b = run_readonly_sql(&s, "SELECT p.question_id, o.actual FROM predictions p JOIN outcomes o ON p.pred_id = o.pred_id").unwrap();
    assert_eq!(block_strings(&b), vec![vec!["q1", "yes"]]);
    l.verify().unwrap();
}

#[test]
fn diff_between_versions() {
    let dir = tempfile::tempdir().unwrap();
    let l = populated(dir.path());
    let d = l.diff(1, 3).unwrap();
    assert_eq!((d.from, d.to), (1, 3));
    assert_eq!(d.revisions.len(), 1);
    assert_eq!(d.events.len(), 1);
    assert_eq!(d.events[0].text, "c");
    let all = l.diff(0, l.latest_version()).unwrap();
    assert_eq!(all.events.len(), 3);
    assert_eq!(all.revisions.len(), 2);
    assert_eq!(all.causes.len(), 2);
    assert!(l.diff(3, 1).is_err());
    assert!(l.diff(0, 99).is_err());
    assert!(l.open_at(AsOf::Version(99)).is_err());
}

#[test]
fn unicode_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let mut l = new_log(dir.path());
    let texts = ["Ran 5 km \u{1F3C3}\u{200D}\u{2640}\u{FE0F} before work \u{1F31E}", "\u{4eca}\u{65e5}\u{306f}\u{5728}\u{5b85}\u{52e4}\u{52d9}\u{3092}\u{9078}\u{3093}\u{3060}", "caf\u{e9} \u{2014} na\u{ef}ve \u{1F600}\u{1F600}"];
    let batch: Vec<NewEvent> = texts.iter().enumerate().map(|(i, t)| NewEvent { tags: vec!["\u{65e5}\u{8a18}".into(), format!("t{i}")], ..ev("\u{30e1}\u{30e2}", &format!("2025-01-0{}", i + 1), t) }).collect();
    l.append_events(&batch).unwrap();
    let r = l.revise(NewRevision { rationale: "\u{7406}\u{7531} \u{1F4A1}".into(), ..rev("\u{30ea}\u{30e2}\u{30fc}\u{30c8}\u{30ef}\u{30fc}\u{30af}", "\u{8cdb}\u{6210} \u{1F44D}", 0.9, "2025-02-01", "user", vec![]) }).unwrap();
    drop(l);
    let l = VersionedLog::open(dir.path(), "tester").unwrap();
    l.verify().unwrap();
    let s = l.open_latest();
    let evs: Vec<EventRow> = s.rows();
    for (i, t) in texts.iter().enumerate() {
        assert_eq!(evs[i].text, *t);
        assert_eq!(evs[i].source, "\u{30e1}\u{30e2}");
    }
    assert_eq!(evs[0].tags, "\u{65e5}\u{8a18},t0");
    let rv: Vec<RevisionRow> = s.rows();
    assert_eq!(rv[0].stance, "\u{8cdb}\u{6210} \u{1F44D}");
    assert_eq!(rv[0].rev_id, r);
    // Unicode survives SQL too.
    let b = run_readonly_sql(&s, "SELECT text FROM events WHERE source = '\u{30e1}\u{30e2}' ORDER BY occurred_at").unwrap();
    assert_eq!(block_strings(&b)[0][0], texts[0]);
    // Rendering with a narrow width must not split a character.
    let _ = super::sql::render(&b, 5);
}

#[test]
fn misc_basics() {
    assert_eq!(normalize_ts("2024-02-29").unwrap(), "2024-02-29T00:00:00Z");
    assert_eq!(normalize_ts("2024-02-29T10:11:12").unwrap(), "2024-02-29T10:11:12Z");
    assert!(normalize_ts("2023-02-29").is_err());
    assert!(normalize_ts("2024-02-29T10:11:12+02:00").is_err());
    assert!(normalize_ts("2024-02-29T25:00:00Z").is_err());
    let n = now_iso();
    assert_eq!(normalize_ts(&n).unwrap(), n);
    assert!(n.starts_with("20"));
    let dir = tempfile::tempdir().unwrap();
    assert!(VersionedLog::open_or_create(dir.path(), "../evil").is_err());
    assert!(VersionedLog::open_or_create(dir.path(), "").is_err());
    let l = VersionedLog::open_or_create(dir.path(), "ok").unwrap();
    assert_eq!(l.latest_version(), 0);
    let s = l.open_latest();
    let meta: Vec<MetaRow> = s.rows();
    assert_eq!(meta.len(), 1);
    assert_eq!(meta[0].format_version, FORMAT_VERSION);
    l.verify().unwrap();
    // Re-opening via open_or_create does not create a new genesis.
    let l2 = VersionedLog::open_or_create(dir.path(), "ok").unwrap();
    assert_eq!(l2.snapshot_hash(0).unwrap(), l.snapshot_hash(0).unwrap());
    // A second writer handle is detected, not silently forked.
    let (mut a, mut b) = (VersionedLog::open_or_create(dir.path(), "ok").unwrap(), VersionedLog::open_or_create(dir.path(), "ok").unwrap());
    a.append_events(&[ev("s", "2025-01-01", "from a")]).unwrap();
    assert!(matches!(b.append_events(&[ev("s", "2025-01-01", "from b")]).unwrap_err(), LogError::Conflict(_)));
    // empty tables are queryable
    let b0 = run_readonly_sql(&l.open_latest(), "SELECT COUNT(*) AS n FROM events").unwrap();
    assert_eq!(block_strings(&b0), vec![vec!["0"]]);
    let j = run_readonly_sql(&l.open_latest(), "SELECT r.key FROM revisions r JOIN revision_causes c ON r.rev_id = c.rev_id JOIN events e ON c.event_id = e.event_id").unwrap();
    assert_eq!(j.num_rows, 0);
}

// ─── SQL ─────────────────────────────────────────────────────────────────────

const DEMO_QUERY: &str = "SELECT r.valid_from, r.stance, r.confidence, r.rationale, e.occurred_at, e.source, e.text \
FROM revisions r JOIN revision_causes c ON r.rev_id = c.rev_id JOIN events e ON c.event_id = e.event_id \
WHERE r.entity_type='belief' AND r.key='remote work' ORDER BY r.valid_from";

fn demo_log(root: &Path) -> (VersionedLog, Vec<String>) {
    let mut l = new_log(root);
    let events: Vec<NewEvent> = (0..20)
        .map(|i| ev(if i % 2 == 0 { "journal" } else { "chat" }, &format!("2025-{:02}-{:02}T09:00:00Z", 1 + i / 5, 3 + i), &format!("entry {i}: remote-work observation {i}")))
        .collect();
    let ids: Vec<String> = events.iter().map(|e| event_id(&e.source, &normalize_ts(&e.occurred_at).unwrap(), &e.text)).collect();
    // v1: all 20 events and the first belief revision
    l.commit_batch(Batch { events, revisions: vec![rev("remote work", "prefer office", 0.7, "2025-01-20", "rule:stance-v1", vec![ids[0].clone(), ids[1].clone()])], ..Default::default() }).unwrap();
    // v2, v3: later revisions citing later events
    l.revise(rev("remote work", "mixed", 0.55, "2025-02-20", "rule:stance-v1", vec![ids[5].clone(), ids[6].clone(), ids[7].clone()])).unwrap();
    l.revise(rev("remote work", "prefer remote", 0.8, "2025-03-25", "user", vec![ids[12].clone(), ids[1].clone()])).unwrap();
    (l, ids)
}

#[test]
fn demo_query_differs_per_version() {
    let dir = tempfile::tempdir().unwrap();
    let (l, ids) = demo_log(dir.path());
    assert_eq!(l.latest_version(), 3);
    let run = |v| block_strings(&run_readonly_sql(&l.open_at(AsOf::Version(v)).unwrap(), DEMO_QUERY).unwrap());
    let (r1, r2, r3) = (run(1), run(2), run(3));
    assert_eq!(r1.len(), 2);
    assert_eq!(r2.len(), 5);
    assert_eq!(r3.len(), 7);
    // Version 1 shows only the first stance, with exactly the events it cited.
    assert!(r1.iter().all(|r| r[1] == "prefer office" && r[2] == "0.7"));
    let mut texts: Vec<&str> = r1.iter().map(|r| r[6].as_str()).collect();
    texts.sort();
    assert_eq!(texts, vec!["entry 0: remote-work observation 0", "entry 1: remote-work observation 1"]);
    // Version 2 adds the second stance only.
    assert_eq!(r2.iter().filter(|r| r[1] == "mixed").count(), 3);
    assert!(r2.iter().all(|r| r[1] != "prefer remote"));
    // Version 3 has all three, ordered by valid_from, with the cited event texts.
    let froms: Vec<&str> = r3.iter().map(|r| r[0].as_str()).collect();
    let mut sorted = froms.clone();
    sorted.sort();
    assert_eq!(froms, sorted);
    assert_eq!(r3.iter().filter(|r| r[1] == "prefer remote").count(), 2);
    assert!(r3.iter().any(|r| r[1] == "prefer remote" && r[6].starts_with("entry 12:") && r[5] == "journal"));
    assert_eq!(ids.len(), 20);
    // Later versions never change what an earlier version answers.
    assert_eq!(run(1), r1);
    assert_eq!(run(2), r2);
}

#[test]
fn readonly_enforcement() {
    let dir = tempfile::tempdir().unwrap();
    let (l, _) = demo_log(dir.path());
    let s = l.open_latest();
    let hash_before = s.content_hash();
    let manifest_before = manifest_bytes(&l);
    for bad in [
        "INSERT INTO events VALUES ('a')",
        "delete from events",
        "DROP TABLE events",
        "SELECT 1; DROP TABLE events",
        "SELECT 1;DROP TABLE events;",
        "select 1 ; -- hi\n delete from events",
        "  UPDATE revisions SET stance = 'x'",
        "CREATE TABLE t AS SELECT 1",
        "WITH x AS (SELECT 1) DELETE FROM events",
        "SELECT * FROM events; SELECT 2",
        "LOAD TABLE t FROM 'x.kore'",
        "SHOW TABLES",
        "COPY events TO 'x'",
        "SELECT 1 /* unterminated",
        "SELECT 'unterminated",
        "",
        ";",
        "/**/DROP/**/TABLE events",
        "SELECT * INTO t FROM events",
    ] {
        assert!(run_readonly_sql(&s, bad).is_err(), "must reject: {bad:?}");
    }
    for ok in [
        "SELECT COUNT(*) AS n FROM events",
        "select count(*) as n from events;",
        "SELECT 'a;b' AS x FROM events LIMIT 1",
        "SELECT /* c */ COUNT(*) AS n FROM events -- trailing",
        "WITH w AS (SELECT key FROM revisions) SELECT COUNT(*) AS n FROM w",
        "SELECT text FROM events WHERE text = 'it''s; DROP TABLE x'",
    ] {
        assert!(check_readonly(ok).is_ok(), "must accept: {ok:?}");
    }
    assert_eq!(block_strings(&run_readonly_sql(&s, "SELECT COUNT(*) AS n FROM events").unwrap()), vec![vec!["20"]]);
    let w = run_readonly_sql(&s, "WITH w AS (SELECT key FROM revisions) SELECT COUNT(*) AS n FROM w").unwrap();
    assert_eq!(block_strings(&w), vec![vec!["3"]]);
    // Nothing changed on disk or in memory.
    assert_eq!(l.open_latest().content_hash(), hash_before);
    assert_eq!(manifest_bytes(&l), manifest_before);
    l.verify().unwrap();
}

#[path = "forget_tests.rs"]
mod forget_tests;
