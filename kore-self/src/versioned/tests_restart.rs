//! Restart path tests: the streamed open builds exactly the index of the full read, the events-only
//! read gives exactly the snapshot's event rows, the peek reads only up to where it stops, and an
//! irregular log falls back to the full read. Temp dirs only.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

fn tick_clock() -> impl Fn() -> String + Send + 'static {
    let n = Arc::new(AtomicU64::new(0));
    move || {
        let k = n.fetch_add(1, Ordering::SeqCst) + 1;
        format!("2026-03-01T{:02}:{:02}:00Z", k / 60, k % 60)
    }
}

fn ev(source: &str, occurred: &str, text: &str) -> NewEvent {
    NewEvent { source: source.into(), occurred_at: occurred.into(), kind: "note".into(), text: text.into(), tags: vec!["t:1".into()], redacted: false }
}

fn rev(key: &str, stance: &str, causes: Vec<String>) -> NewRevision {
    NewRevision { entity_type: "belief".into(), key: key.into(), stance: stance.into(), confidence: 0.5, rationale: format!("{{\"why\":\"{stance}\"}}"), supersedes: None, valid_from: "2025-01-01".into(), author: "rule:r".into(), caused_by: causes }
}

fn pred(i: u32) -> NewPrediction {
    NewPrediction { made_at: "2025-01-01".into(), question_id: format!("q{i}"), target: "t".into(), options: vec!["yes".into(), "no".into()], probs: vec![0.25, 0.75], model_id: "m".into(), model_version: "1".into(), features_cite: vec![], resolves_by: None }
}

/// A log with every table: events, revisions (some re-revised, some re-submitted), causes, predictions, outcomes.
fn busy(root: &Path, f: LogFormat) -> VersionedLog {
    let mut l = VersionedLog::open_or_create_with_format(root, "tester", tick_clock(), f).unwrap();
    for i in 0..30u32 {
        let a = l.append_events(&[ev("s", &format!("2025-01-{:02}", i % 28 + 1), &format!("a{i}")), ev("being:loop", "2025-02-01", &format!("b{i}"))]).unwrap();
        let key = format!("k{}", i % 7);
        let mut b = Batch::default();
        b.events.push(ev("s", "2025-03-01", &format!("c{i}")));
        b.revisions.push(rev(&key, &format!("x{}", i % 3), a.event_ids.clone()));
        b.revisions.push(rev(&format!("u{i}"), "y", vec![a.event_ids[0].clone()]));
        if i % 4 == 0 {
            b.predictions.push(pred(i));
        }
        l.commit_batch(b).unwrap();
        if i % 4 == 0 {
            let pid = prediction_id(&pred(i)).unwrap();
            l.record_outcome(NewOutcome { pred_id: pid, resolved_at: "2025-02-01".into(), actual: "yes".into(), outcome_event_id: None }).unwrap();
        }
    }
    l
}

fn entries_of(dir: &Path) -> Vec<Entry> {
    manifest::parse_ex(&read_manifest_raw(dir).unwrap()).unwrap().entries
}

/// The streamed open must give the index, event segment list and format of the full read.
fn assert_streamed_equals_full(dir: &Path) {
    let entries = entries_of(dir);
    let full = load_state_full(dir, &entries).unwrap();
    let st = load_state_streamed(dir, &entries).unwrap().expect("a regular log is streamed");
    assert!(st.index.same_as(&full.index), "index differs");
    assert_eq!(st.ev_segs, full.ev_segs);
    assert_eq!(st.format, full.format);
    assert!(st.head.is_none());
}

fn snapshot_events(l: &VersionedLog) -> Vec<EventRow> {
    l.open_at(AsOf::Version(l.latest_version())).unwrap().rows()
}

fn streamed_events(l: &VersionedLog) -> Vec<EventRow> {
    let mut v = vec![];
    l.for_each_event(|e| {
        v.push(e);
        true
    })
    .unwrap();
    v
}

#[test]
fn streamed_open_builds_the_same_index_as_the_full_read() {
    for f in [LogFormat::V2, LogFormat::V3] {
        let d = tempfile::tempdir().unwrap();
        let l = busy(d.path(), f);
        let dir = l.dir().to_path_buf();
        drop(l);
        assert_streamed_equals_full(&dir);
        // and after a forget (rewritten segments, forgotten ids)
        let mut l = VersionedLog::open(d.path(), "tester").unwrap();
        let victim = streamed_events(&l)[3].event_id.clone();
        l.forget_events(&[victim.clone()]).unwrap();
        drop(l);
        assert_streamed_equals_full(&dir);
        let l = VersionedLog::open(d.path(), "tester").unwrap();
        assert!(l.is_forgotten(&victim) && !l.has_event(&victim));
    }
}

#[test]
fn a_reopened_log_behaves_like_before_on_new_commits() {
    // same commits on a handle that was opened streamed and on one that holds the full snapshot
    let d = tempfile::tempdir().unwrap();
    let l = busy(d.path(), LogFormat::V3);
    drop(l);
    let d2 = tempfile::tempdir().unwrap();
    copy(d.path(), d2.path());
    let mut a = VersionedLog::open(d.path(), "tester").unwrap();
    let mut b = VersionedLog::open(d2.path(), "tester").unwrap();
    let _ = b.open_latest();
    a.set_clock(tick_clock());
    b.set_clock(tick_clock());
    for l in [&mut a, &mut b] {
        let ids = streamed_events(l).iter().map(|e| e.event_id.clone()).collect::<Vec<_>>();
        // a re-submitted revision (format 2 idempotence), a repeated event, a new revision of an old key
        let r1 = l.revise(rev("k3", "x0", vec![ids[0].clone(), ids[1].clone()])).unwrap();
        let info = l.append_events(&[ev("s", "2025-01-01", "a0")]).unwrap();
        assert!(!info.created);
        let r2 = l.revise(rev("k3", "new", vec![ids[5].clone()])).unwrap();
        assert!(!r1.is_empty() && !r2.is_empty());
    }
    assert_eq!(a.latest_version(), b.latest_version());
    let (sa, sb) = (a.open_latest(), b.open_latest());
    assert_eq!(sa.content_hash(), sb.content_hash());
}

fn copy(a: &Path, b: &Path) {
    fs::create_dir_all(b).unwrap();
    for e in fs::read_dir(a).unwrap().flatten() {
        let (p, q) = (e.path(), b.join(e.file_name()));
        if p.is_dir() {
            copy(&p, &q);
        } else {
            fs::copy(&p, &q).unwrap();
        }
    }
}

#[test]
fn events_only_read_equals_the_snapshot_rows_lazy_full_and_after_commits() {
    for f in [LogFormat::V1, LogFormat::V2, LogFormat::V3] {
        let d = tempfile::tempdir().unwrap();
        drop(busy(d.path(), f));
        let mut l = VersionedLog::open(d.path(), "tester").unwrap();
        assert_eq!(streamed_events(&l), snapshot_events(&l), "{f:?} after open");
        l.append_events(&[ev("s", "2025-05-05", "later")]).unwrap();
        assert_eq!(streamed_events(&l), snapshot_events(&l), "{f:?} after a commit");
        let _ = l.open_latest();
        assert_eq!(streamed_events(&l), snapshot_events(&l), "{f:?} with the snapshot in memory");
        // early stop
        let mut n = 0;
        l.for_each_event(|_| {
            n += 1;
            n < 5
        })
        .unwrap();
        assert_eq!(n, 5);
    }
}

#[test]
fn peek_reads_events_in_log_order_and_stops_early() {
    for f in [LogFormat::V1, LogFormat::V3] {
        let d = tempfile::tempdir().unwrap();
        let l = busy(d.path(), f);
        let all = snapshot_events(&l);
        drop(l);
        let mut seen = vec![];
        VersionedLog::peek_events(d.path(), "tester", |e| {
            seen.push(e);
            true
        })
        .unwrap();
        assert_eq!(seen, all);
        let mut first = vec![];
        VersionedLog::peek_events(d.path(), "tester", |e| {
            first.push(e);
            false
        })
        .unwrap();
        assert_eq!(first, all[..1].to_vec());
    }
}

#[test]
fn peek_and_events_only_read_detect_tampering() {
    let d = tempfile::tempdir().unwrap();
    let l = busy(d.path(), LogFormat::V3);
    let dir = l.dir().to_path_buf();
    let entries = l.entries.clone();
    drop(l);
    let first_ev = entries.iter().flat_map(|e| e.segments.iter()).find(|g| g.table == "events").unwrap().file.clone();
    let open = VersionedLog::open(d.path(), "tester").unwrap();
    let p = dir.join(&first_ev);
    let mut b = fs::read(&p).unwrap();
    let k = b.len() / 2;
    b[k] ^= 0x01;
    fs::write(&p, &b).unwrap();
    assert!(matches!(VersionedLog::peek_events(d.path(), "tester", |_| false), Err(LogError::Verify(VerifyError::ChecksumMismatch { .. }))));
    assert!(open.for_each_event(|_| true).is_err(), "a segment changed after open is refused by the events-only read");
    assert!(VersionedLog::open(d.path(), "tester").is_err());
    b[k] ^= 0x01;
    fs::write(&p, &b).unwrap();
    // a flipped byte in a revisions segment: the streamed open refuses the log
    let rv = entries.iter().flat_map(|e| e.segments.iter()).find(|g| g.table == "revisions").unwrap().file.clone();
    let p = dir.join(&rv);
    let mut b = fs::read(&p).unwrap();
    b[3] ^= 0x10;
    fs::write(&p, &b).unwrap();
    assert!(VersionedLog::open(d.path(), "tester").is_err());
    b[3] ^= 0x10;
    fs::write(&p, &b).unwrap();
    // the manifest chain is checked by the peek
    let m = dir.join("MANIFEST");
    let raw = fs::read_to_string(&m).unwrap();
    fs::write(&m, raw.replacen("\"rows\":2", "\"rows\":3", 1)).unwrap();
    assert!(VersionedLog::peek_events(d.path(), "tester", |_| false).is_err());
}

#[test]
fn an_irregular_log_falls_back_to_the_full_read_with_the_same_index() {
    // move the revision_causes segment of one commit into a commit of its own (never written by the
    // API, but readable by the old code): the streamed open must not guess, the full read decides
    let d = tempfile::tempdir().unwrap();
    let l = busy(d.path(), LogFormat::V3);
    let dir = l.dir().to_path_buf();
    let raw = l.manifest_raw.clone();
    drop(l);
    let entries = manifest::parse_ex(&raw).unwrap().entries;
    // the new log: owner "irregular" next to it, segment files renamed to their new version numbers
    let ndir = d.path().join("irregular").join("log");
    let mut out: Vec<Entry> = vec![];
    let mut split = false;
    let mut push = |out: &mut Vec<Entry>, at: &str, segs: Vec<SegRef>| {
        let prev = out.last().map(|x| x.hash.clone()).unwrap_or_else(|| ZERO_HASH.to_string());
        let v = out.len() as u64;
        let segs: Vec<SegRef> = segs
            .into_iter()
            .map(|mut g| {
                let file = format!("{}/{:010}.kore", g.table, v);
                fs::create_dir_all(ndir.join(&g.table)).unwrap();
                fs::copy(dir.join(&g.file), ndir.join(&file)).unwrap();
                g.file = file;
                g
            })
            .collect();
        out.push(Entry::new(v, at.to_string(), segs, &prev));
    };
    for e in entries {
        if !split && e.segments.iter().any(|g| g.table == "revision_causes") {
            let (c, rest): (Vec<SegRef>, Vec<SegRef>) = e.segments.iter().cloned().partition(|g| g.table == "revision_causes");
            push(&mut out, &e.recorded_at, rest);
            push(&mut out, &e.recorded_at, c);
            split = true;
        } else {
            push(&mut out, &e.recorded_at, e.segments.clone());
        }
    }
    let mut text = format!("{}\n", manifest::HEADER_J);
    for e in &out {
        text.push_str(&serde_json::to_string(e).unwrap());
        text.push('\n');
    }
    fs::write(ndir.join("MANIFEST"), text).unwrap();
    let dir = ndir;
    let entries = entries_of(&dir);
    assert!(load_state_streamed(&dir, &entries).unwrap().is_none(), "irregular: no streamed index");
    let full = load_state_full(&dir, &entries).unwrap();
    let l = VersionedLog::open(d.path(), "irregular").unwrap();
    l.verify().unwrap();
    assert!(l.index.same_as(&full.index));
    assert_eq!(streamed_events(&l), snapshot_events(&l));
}

#[test]
fn old_fixtures_open_streamed_with_the_same_index_and_rows() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data");
    let mut n = 0;
    for fx in [data.join("vlog_v1"), data.join("vlog_v2_r1").join("fixture"), data.join("vlog_v2_r1").join("fixture_mixed")] {
        if !fx.join("fixture").exists() {
            continue;
        }
        let tmp = tempfile::tempdir().unwrap();
        copy(&fx.join("fixture"), &tmp.path().join("fixture"));
        let l = VersionedLog::open(tmp.path(), "fixture").unwrap();
        l.verify().unwrap();
        assert_eq!(streamed_events(&l), snapshot_events(&l), "{}", fx.display());
        let entries = entries_of(l.dir());
        let full = load_state_full(l.dir(), &entries).unwrap();
        assert!(l.index.same_as(&full.index), "{}", fx.display());
        n += 1;
    }
    assert_eq!(n, 3, "fixtures found");
}
