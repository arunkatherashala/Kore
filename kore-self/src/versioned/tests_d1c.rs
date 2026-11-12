//! Track D1c tests: the fast open (index runs + verified prefix + tail) gives exactly what the full
//! open gives, for every version and every lookup; tampering, crashes during run writes and merges,
//! and `forget` never make it use a wrong run; key and format rules. Temp dirs only.

use super::index_runs::{sample_segments, RunCrash};
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

const OWNER: &str = "tester";
const FAST: OpenOptions = OpenOptions { fast_open: true };

fn tick_clock() -> impl Fn() -> String + Send + 'static {
    let n = Arc::new(AtomicU64::new(0));
    move || {
        let k = n.fetch_add(1, Ordering::SeqCst) + 1;
        format!("2026-03-{:02}T{:02}:{:02}:00Z", 1 + k / 3600 % 28, k / 60 % 60, k % 60)
    }
}
fn fixed() -> String {
    "2027-01-01T00:00:00Z".to_string()
}

fn ev(occurred: &str, text: &str) -> NewEvent {
    NewEvent { source: "s".into(), occurred_at: occurred.into(), kind: "note".into(), text: text.into(), tags: vec!["t:1".into()], redacted: false }
}
fn rev(key: &str, stance: &str, causes: Vec<String>) -> NewRevision {
    NewRevision { entity_type: "belief".into(), key: key.into(), stance: stance.into(), confidence: 0.5, rationale: format!("{{\"why\":\"{stance}\"}}"), supersedes: None, valid_from: "2025-01-01".into(), author: "rule:r".into(), caused_by: causes }
}
fn pred(q: &str, cite: Vec<String>) -> NewPrediction {
    NewPrediction { made_at: "2025-01-01".into(), question_id: q.into(), target: "t".into(), options: vec!["yes".into(), "no".into()], probs: vec![0.25, 0.75], model_id: "m".into(), model_version: "1".into(), features_cite: cite, resolves_by: None }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// Random batches: events (some re-submitted), revisions (re-revised, re-submitted), predictions with
/// outcomes in the same or a later batch.
struct Gen {
    rng: Rng,
    texts: u64,
    events: Vec<String>,
    revs: Vec<NewRevision>,
    open_preds: Vec<String>,
}
impl Gen {
    fn new(seed: u64) -> Gen {
        Gen { rng: Rng(seed), texts: 0, events: vec![], revs: vec![], open_preds: vec![] }
    }
    fn batch(&mut self) -> Batch {
        let mut b = Batch::default();
        let r = self.rng.below(10);
        let n_ev = 1 + self.rng.below(3);
        for _ in 0..n_ev {
            self.texts += 1;
            b.events.push(ev(&format!("2025-01-{:02}", self.texts % 28 + 1), &format!("text {}", self.texts)));
        }
        if r >= 3 && !self.events.is_empty() {
            let c = self.events[self.rng.below(self.events.len() as u64) as usize].clone();
            let key = format!("k{}", self.rng.below(6));
            if r == 9 && !self.revs.is_empty() {
                b.revisions.push(self.revs[self.rng.below(self.revs.len() as u64) as usize].clone());
            } else {
                let rv = rev(&key, &format!("x{}", self.rng.below(4)), vec![c.clone()]);
                self.revs.push(rv.clone());
                b.revisions.push(rv);
            }
            if r == 5 {
                let p = pred(&format!("q{}", self.texts), vec![c]);
                if self.rng.below(2) == 0 {
                    let pid = prediction_id(&p).unwrap();
                    b.outcomes.push(NewOutcome { pred_id: pid, resolved_at: "2025-02-01".into(), actual: "yes".into(), outcome_event_id: None });
                } else {
                    self.open_preds.push(prediction_id(&p).unwrap());
                }
                b.predictions.push(p);
            }
        }
        if r == 2 && !self.open_preds.is_empty() {
            let pid = self.open_preds.remove(0);
            let oe = self.events.last().cloned();
            b.outcomes.push(NewOutcome { pred_id: pid, resolved_at: "2025-02-02".into(), actual: "no".into(), outcome_event_id: oe });
        }
        if r == 1 {
            // a re-submitted old event (duplicate, skipped)
            b.events.push(ev("2025-01-02", "text 1"));
        }
        b
    }
    fn commit(&mut self, l: &mut VersionedLog) {
        let b = self.batch();
        let info = l.commit_batch(b).unwrap();
        for e in info.event_ids {
            if l.has_event(&e) && !self.events.contains(&e) {
                self.events.push(e);
            }
        }
    }
}

fn copy_dir(a: &Path, b: &Path) {
    fs::create_dir_all(b).unwrap();
    for f in fs::read_dir(a).unwrap().flatten() {
        let p = f.path();
        let q = b.join(f.file_name());
        if p.is_dir() {
            copy_dir(&p, &q);
        } else {
            fs::copy(&p, &q).unwrap();
        }
    }
}

fn files_of(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, d: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for f in fs::read_dir(d).unwrap().flatten() {
            let p = f.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                out.insert(p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"), fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// The fast open of `root` equals the full open: index (every map), event segments, format, every
/// `open_at(v)`, every lookup incl. absent keys, and the same next commits write the same bytes.
fn assert_fast_equals_full(root: &Path, rng: &mut Rng) -> OpenReport {
    let full = VersionedLog::open(root, OWNER).unwrap();
    let fast = VersionedLog::open_with(root, OWNER, FAST).unwrap();
    assert!(full.index.base.is_none());
    let m = fast.index.materialize().unwrap();
    assert!(m.same_plain(&full.index), "materialized fast index differs from the full index");
    assert_eq!(fast.index.forgotten, full.index.forgotten);
    assert_eq!(fast.index.content_idem, full.index.content_idem);
    assert_eq!(fast.index.next_ord, full.index.next_ord);
    assert_eq!(fast.ev_segs, full.ev_segs);
    assert_eq!(fast.format, full.format);
    assert_eq!(fast.entries, full.entries);
    for v in 0..=full.latest_version() {
        assert_eq!(fast.open_at(AsOf::Version(v)).unwrap().content_hash(), full.open_at(AsOf::Version(v)).unwrap().content_hash(), "open_at({v})");
    }
    for (k, &o) in &full.index.events {
        let id = hex(k);
        assert_eq!(fast.index.ev_ord(&id).unwrap(), Some(o));
        assert_eq!(fast.get_event(&id).unwrap(), full.get_event(&id).unwrap());
    }
    for (id, v) in &full.index.revs {
        assert_eq!(fast.index.rev_get(id).unwrap().as_ref(), Some(v));
    }
    for (hk, v) in &full.index.heads {
        assert_eq!(fast.index.head_get(hk).unwrap().as_ref(), Some(v));
    }
    for (d, v) in &full.index.content_ids {
        assert_eq!(fast.index.content_get(d).unwrap().as_ref(), Some(v));
    }
    for (id, v) in &full.index.preds {
        assert_eq!(fast.index.pred_get(id).unwrap().as_ref(), Some(v));
    }
    for id in &full.index.outcomes {
        assert!(fast.index.outcome_has(id).unwrap());
    }
    for i in 0..50u64 {
        let id = sha256_hex(&[format!("absent{}", rng.next() + i).as_bytes()]);
        assert!(!fast.has_event(&id) && !full.has_event(&id));
        assert!(fast.index.rev_get(&id).unwrap().is_none() && fast.index.content_get(&id).unwrap().is_none());
        assert!(!fast.index.pred_has(&id).unwrap() && !fast.index.outcome_has(&id).unwrap());
        assert!(fast.index.head_get(&("belief".into(), format!("absent{i}"))).unwrap().is_none());
    }
    let rep = fast.open_report().clone();
    let mut g = Gen::new(rng.next());
    g.events = full.index.events.keys().take(5).map(|k| hex(k)).collect();
    g.revs = vec![];
    g.texts = 100_000;
    drop(full);
    drop(fast);
    // the same next batches after either open write byte-identical files
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
    copy_dir(root, &a);
    copy_dir(root, &b);
    let mut la = VersionedLog::open(&a, OWNER).unwrap();
    let mut lb = VersionedLog::open_with(&b, OWNER, FAST).unwrap();
    la.set_clock(fixed);
    lb.set_clock(fixed);
    for _ in 0..3 {
        let batch = g.batch();
        let ia = la.commit_batch(batch.clone());
        let ib = lb.commit_batch(batch);
        assert_eq!(format!("{ia:?}"), format!("{ib:?}"));
    }
    drop(la);
    drop(lb);
    assert_eq!(files_of(&a), files_of(&b), "next commits differ after fast vs full open");
    rep
}

fn v5_log(root: &Path) -> VersionedLog {
    let mut l = VersionedLog::open_or_create_with_format(root, OWNER, tick_clock(), LogFormat::V3).unwrap();
    l.append_events(&[ev("2025-01-01", "first")]).unwrap();
    assert!(l.upgrade_to_v5().unwrap());
    l
}

fn force(l: &mut VersionedLog) -> RunsReport {
    l.maintain_runs_inner(None, 1, 1).unwrap()
}

#[test]
fn fast_open_equals_full_for_every_version_random_logs() {
    for seed in 1..=5u64 {
        let d = tempfile::tempdir().unwrap();
        let mut rng = Rng(seed * 7919);
        let start = if seed % 2 == 0 { LogFormat::V1 } else { LogFormat::V3 };
        let mut l = VersionedLog::open_or_create_with_format(d.path(), OWNER, tick_clock(), start).unwrap();
        let mut g = Gen::new(seed);
        // history before the upgrade (format 1 or 3 segments)
        for _ in 0..rng.below(8) {
            g.commit(&mut l);
        }
        assert!(l.upgrade_to_v5().unwrap());
        let mut runs_written = 0;
        for step in 0..70 {
            match rng.below(12) {
                0 | 1 => {
                    let r = force(&mut l);
                    assert!(r.written.is_some());
                    runs_written += 1;
                    assert!(l.index_runs().len() <= index_runs::MAX_RUNS);
                }
                2 if step > 40 && seed % 2 == 1 && g.events.len() > 3 => {
                    let id = g.events.remove(rng.below(g.events.len() as u64) as usize);
                    g.revs.retain(|r| !r.caused_by.contains(&id));
                    l.forget_events(&[id]).unwrap();
                }
                _ => g.commit(&mut l),
            }
            if step % 23 == 22 {
                drop(l);
                assert_fast_equals_full(d.path(), &mut rng);
                l = VersionedLog::open_with(d.path(), OWNER, FAST).unwrap();
                l.set_clock(tick_clock());
            }
        }
        force(&mut l);
        runs_written += 1;
        drop(l);
        let rep = assert_fast_equals_full(d.path(), &mut rng);
        assert!(rep.fast_used, "seed {seed}: {rep:?}");
        assert!(runs_written > 3);
        VersionedLog::open(d.path(), OWNER).unwrap().verify().unwrap();
    }
}

#[test]
fn merges_keep_at_most_k_runs_and_equal_full() {
    let d = tempfile::tempdir().unwrap();
    let mut l = v5_log(d.path());
    let mut g = Gen::new(42);
    let mut merged = 0;
    for _ in 0..14 {
        for _ in 0..3 {
            g.commit(&mut l);
        }
        let r = force(&mut l);
        merged += r.merged.is_some() as usize;
        assert!(l.index_runs().len() <= index_runs::MAX_RUNS);
        assert!(index_runs::stray_runs(&l.dir, &l.entries).is_empty(), "merged-away runs are deleted");
    }
    assert!(merged >= 5, "merged {merged}");
    g.commit(&mut l);
    drop(l);
    let rep = assert_fast_equals_full(d.path(), &mut Rng(3));
    assert!(rep.fast_used && rep.runs_used == index_runs::MAX_RUNS && rep.tail_versions == 2, "{rep:?}");
    VersionedLog::open(d.path(), OWNER).unwrap().verify().unwrap();
}

#[test]
fn default_cadence_and_format_rules() {
    let d = tempfile::tempdir().unwrap();
    let mut l = VersionedLog::open_or_create_with_format(d.path(), OWNER, tick_clock(), LogFormat::V3).unwrap();
    l.append_events(&[ev("2025-01-01", "a")]).unwrap();
    assert!(l.maintain_index_runs().unwrap().skipped.unwrap().contains("format 3"));
    drop(l);
    // format 3 with the key on: the full open
    let f = VersionedLog::open_with(d.path(), OWNER, FAST).unwrap();
    assert!(!f.open_report().fast_used && f.open_report().fallback_reason.as_deref().unwrap().contains("not a format-5 log"));
    assert!(f.index.base.is_none());
    let full = VersionedLog::open(d.path(), OWNER).unwrap();
    assert!(f.index.same_as(&full.index));
    drop(f);
    drop(full);
    let before = read_manifest_raw(&d.path().join(OWNER).join("log")).unwrap();
    let mut l = VersionedLog::open(d.path(), OWNER).unwrap();
    l.set_clock(tick_clock());
    assert!(l.upgrade_to_v5().unwrap());
    assert!(!l.upgrade_to_v5().unwrap());
    assert!(!l.upgrade_to_v3().unwrap() && !l.upgrade_to_v2().unwrap());
    assert!(l.upgrade_to_v4().is_err());
    let after = read_manifest_raw(&l.dir).unwrap();
    let body = |r: &[u8]| r[r.iter().position(|&b| b == b'\n').unwrap() + 1..].to_vec();
    assert!(after.starts_with(b"KORE-VLOG-MANIFEST 4\n"));
    assert!(body(&after).starts_with(&body(&before)), "old entry lines kept byte for byte");
    drop(l);
    let mut l = VersionedLog::open_with(d.path(), OWNER, FAST).unwrap();
    assert!(l.open_report().fallback_reason.as_deref().unwrap().contains("no index run"));
    l.set_clock(tick_clock());
    // D-3: the first call writes the first run, the next one is not due
    let r = l.maintain_index_runs().unwrap();
    assert!(r.written.is_some() && r.listed_at == Some(l.latest_version()));
    assert!(l.maintain_index_runs().unwrap().skipped.unwrap().contains("not due"));
    for i in 0..5 {
        l.append_events(&[ev("2025-01-03", &format!("b{i}"))]).unwrap();
    }
    assert!(l.maintain_index_runs().unwrap().skipped.is_some());
    drop(l);
    let rep = assert_fast_equals_full(d.path(), &mut Rng(9));
    assert!(rep.fast_used && rep.tail_versions == 6 && rep.runs_used == 1, "{rep:?}");
    // a run listing under an older header is refused by the parse
    let raw = read_manifest_raw(&d.path().join(OWNER).join("log")).unwrap();
    let mut old = b"KORE-VLOG-MANIFEST 3\n".to_vec();
    old.extend_from_slice(&body(&raw));
    assert!(manifest::parse_ex(&old).err().unwrap().contains("format-5 header"));
    // a format-4 log is not upgraded
    let d4 = tempfile::tempdir().unwrap();
    let mut l4 = VersionedLog::open_or_create_with_format(d4.path(), OWNER, tick_clock(), LogFormat::V4).unwrap();
    assert!(l4.upgrade_to_v5().is_err());
}

/// A format-5 log with more than 64 segments before its newest run, then a short tail.
fn long_log(root: &Path) -> VersionedLog {
    let mut l = v5_log(root);
    let mut g = Gen::new(5);
    for _ in 0..60 {
        g.commit(&mut l);
    }
    force(&mut l);
    for _ in 0..4 {
        g.commit(&mut l);
    }
    l
}

fn flip(p: &Path, at: usize) {
    let mut b = fs::read(p).unwrap();
    let i = at.min(b.len() - 1);
    b[i] ^= 0x01;
    fs::write(p, b).unwrap();
}

#[test]
fn tampered_runs_are_never_used() {
    for case in ["flip", "truncate", "swap", "missing"] {
        let d = tempfile::tempdir().unwrap();
        let l = long_log(d.path());
        let run = l.dir.join(&l.index_runs()[0].file);
        let dir = l.dir.clone();
        drop(l);
        match case {
            "flip" => flip(&run, 5000),
            "truncate" => {
                let b = fs::read(&run).unwrap();
                fs::write(&run, &b[..b.len() - 10]).unwrap();
            }
            "swap" => {
                let d2 = tempfile::tempdir().unwrap();
                let mut o = v5_log(d2.path());
                let mut g = Gen::new(77);
                for _ in 0..61 {
                    g.commit(&mut o);
                }
                force(&mut o);
                fs::copy(o.dir.join(&o.index_runs()[0].file), &run).unwrap();
            }
            _ => fs::remove_file(&run).unwrap(),
        }
        let f = VersionedLog::open_with(d.path(), OWNER, FAST).unwrap();
        let why = f.open_report().fallback_reason.clone().unwrap_or_default();
        assert!(!f.open_report().fast_used && why.contains("index run rejected"), "{case}: {why}");
        let full = VersionedLog::open(d.path(), OWNER).unwrap();
        assert!(f.index.same_as(&full.index), "{case}: the fallback gives the full index");
        let e = verify_dir(&dir).unwrap_err();
        match case {
            "missing" => assert!(matches!(e, VerifyError::MissingSegment { .. }), "{case}: {e:?}"),
            _ => assert!(matches!(e, VerifyError::ChecksumMismatch { .. }), "{case}: {e:?}"),
        }
    }
}

#[test]
fn unlisted_run_and_tampered_manifest() {
    let d = tempfile::tempdir().unwrap();
    let l = long_log(d.path());
    let dir = l.dir.clone();
    let listed = l.index_runs()[0].clone();
    drop(l);
    // an unlisted run file (here a valid copy under another name) is not used; verify reports it
    let stray = dir.join(index_runs::run_file(0, 1));
    fs::copy(dir.join(&listed.file), &stray).unwrap();
    let f = VersionedLog::open_with(d.path(), OWNER, FAST).unwrap();
    assert!(f.open_report().fast_used && f.open_report().runs_used == 1);
    assert!(matches!(f.verify().unwrap_err(), VerifyError::ExtraSegment { .. }));
    drop(f);
    let mut l = VersionedLog::open(d.path(), OWNER).unwrap();
    assert!(l.recover().unwrap().iter().any(|s| s.ends_with("0000000001.kore")));
    l.verify().unwrap();
    drop(l);
    // a listing changed in the MANIFEST breaks the hash chain: no open uses it
    let m = dir.join("MANIFEST");
    let raw = String::from_utf8(fs::read(&m).unwrap()).unwrap();
    let bad = raw.replacen(&listed.sha256, &"0".repeat(64), 1);
    fs::write(&m, bad).unwrap();
    assert!(matches!(VersionedLog::open_with(d.path(), OWNER, FAST).err().unwrap(), LogError::Verify(VerifyError::ChainBroken { .. })));
    assert!(VersionedLog::open(d.path(), OWNER).is_err());
    // the MANIFEST cut back to before the listing: no run is used, the result is the full open
    let keep: String = raw.split_inclusive('\n').take_while(|l| !l.contains("index_runs")).collect();
    fs::write(&m, keep).unwrap();
    let f = VersionedLog::open_with(d.path(), OWNER, FAST).unwrap();
    assert!(!f.open_report().fast_used);
    let full = VersionedLog::open(d.path(), OWNER).unwrap();
    assert!(f.index.same_as(&full.index));
    assert!(f.verify().is_err(), "the now unlisted run file is reported");
}

fn active_listing_to(entries: &[Entry]) -> usize {
    index_runs::active_listing(entries).last().unwrap().to as usize
}

#[test]
fn old_segment_tamper_sampled_or_not() {
    let d = tempfile::tempdir().unwrap();
    let l = long_log(d.path());
    let dir = l.dir.clone();
    let entries = l.entries.clone();
    drop(l);
    let upto = active_listing_to(&entries);
    let head = entries.last().unwrap().hash.clone();
    let sample = sample_segments(&entries, upto, &head);
    assert_eq!(sample.len(), index_runs::SAMPLE_SEGMENTS);
    assert_eq!(sample, sample_segments(&entries, upto, &head), "reproducible");
    let all = all_segs(&entries[..=upto]);
    assert!(all.len() > index_runs::SAMPLE_SEGMENTS + 10, "{}", all.len());
    // not sampled: the fast open succeeds (D-1); reading that segment and verify fail
    let (ei, si) = *all.iter().find(|p| !sample.contains(p) && entries[p.0].segments[p.1].table == "events").unwrap();
    let seg = entries[ei].segments[si].clone();
    let bytes = fs::read(dir.join(&seg.file)).unwrap();
    flip(&dir.join(&seg.file), bytes.len() / 2);
    let f = VersionedLog::open_with(d.path(), OWNER, FAST).unwrap();
    assert!(f.open_report().fast_used && f.open_report().sampled_segments == index_runs::SAMPLE_SEGMENTS);
    assert!(matches!(f.open_at(AsOf::Version(ei as u64)).unwrap_err(), LogError::Verify(VerifyError::ChecksumMismatch { .. })));
    assert!(matches!(f.verify().unwrap_err(), VerifyError::ChecksumMismatch { .. }));
    drop(f);
    assert!(VersionedLog::open(d.path(), OWNER).is_err(), "the full open still checks every segment");
    fs::write(dir.join(&seg.file), &bytes).unwrap();
    // sampled: the fast open falls back to the full open, which fails on it
    let (ei, si) = sample[sample.len() / 2];
    let seg = entries[ei].segments[si].clone();
    flip(&dir.join(&seg.file), 3);
    let e = VersionedLog::open_with(d.path(), OWNER, FAST).err().unwrap();
    assert!(matches!(e, LogError::Verify(VerifyError::ChecksumMismatch { .. })), "{e:?}");
}

#[test]
fn kill_during_run_write_and_merge() {
    for crash in [RunCrash::BeforeRename, RunCrash::AfterRename, RunCrash::AfterCommit] {
        let d = tempfile::tempdir().unwrap();
        let mut l = v5_log(d.path());
        let mut g = Gen::new(11);
        // 8 runs, so that the next run also merges
        for _ in 0..index_runs::MAX_RUNS {
            g.commit(&mut l);
            g.commit(&mut l);
            force(&mut l);
        }
        g.commit(&mut l);
        let listed_before = l.index_runs();
        assert!(l.maintain_runs_inner(Some(crash), 1, 1).is_err());
        drop(l);
        let dir = d.path().join(OWNER).join("log");
        let listed_now = index_runs::active_listing(&manifest::parse(&read_manifest_raw(&dir).unwrap()).unwrap()).to_vec();
        if crash == RunCrash::AfterCommit {
            assert_ne!(listed_now, listed_before);
        } else {
            assert_eq!(listed_now, listed_before, "{crash:?}: an uncommitted run is not listed");
        }
        let rep = assert_fast_equals_full(d.path(), &mut Rng(5));
        assert!(rep.fast_used, "{crash:?}: {rep:?}");
        // leftovers (temp file, unlisted runs, merged-away runs) are strays until recover removes them
        assert!(verify_dir(&dir).is_err(), "{crash:?}");
        let mut l = VersionedLog::open(d.path(), OWNER).unwrap();
        assert!(!l.recover().unwrap().is_empty());
        l.verify().unwrap();
        l.set_clock(tick_clock());
        g.commit(&mut l);
        force(&mut l);
        drop(l);
        assert!(assert_fast_equals_full(d.path(), &mut Rng(6)).fast_used);
    }
}

#[test]
fn torn_listing_append_is_ignored() {
    let d = tempfile::tempdir().unwrap();
    let mut l = long_log(d.path());
    let dir = l.dir.clone();
    let before = read_manifest_raw(&dir).unwrap();
    let old = l.index_runs();
    force(&mut l);
    drop(l);
    // the crash cut the listing line: half of it is on disk
    let after = read_manifest_raw(&dir).unwrap();
    let cut = before.len() + (after.len() - before.len()) / 2;
    fs::write(dir.join("MANIFEST"), &after[..cut]).unwrap();
    let f = VersionedLog::open_with(d.path(), OWNER, FAST).unwrap();
    assert!(f.open_report().fast_used);
    assert_eq!(f.index_runs(), old);
    let full = VersionedLog::open(d.path(), OWNER).unwrap();
    assert!(f.index.materialize().unwrap().same_plain(&full.index));
    drop(f);
    drop(full);
    let mut l = VersionedLog::open(d.path(), OWNER).unwrap();
    l.recover().unwrap();
    l.verify().unwrap();
    drop(l);
    assert!(assert_fast_equals_full(d.path(), &mut Rng(8)).fast_used);
}

#[test]
fn forget_invalidates_runs_and_falls_back() {
    let d = tempfile::tempdir().unwrap();
    let mut l = v5_log(d.path());
    let mut g = Gen::new(21);
    for _ in 0..10 {
        g.commit(&mut l);
    }
    let early = g.events[1].clone();
    force(&mut l);
    let mid_start = g.events.len();
    for _ in 0..10 {
        g.commit(&mut l);
    }
    let mid = g.events[mid_start + 1].clone();
    force(&mut l);
    for _ in 0..3 {
        g.commit(&mut l);
    }
    // forget an event between the two runs: the second run is stale, the first still used
    l.forget_events(&[mid.clone()]).unwrap();
    drop(l);
    let rep = assert_fast_equals_full(d.path(), &mut Rng(1));
    assert!(rep.fast_used && rep.runs_used == 1 && rep.runs_stale == 1, "{rep:?}");
    VersionedLog::open(d.path(), OWNER).unwrap().verify().unwrap();
    let mut f = VersionedLog::open_with(d.path(), OWNER, FAST).unwrap();
    assert!(f.is_forgotten(&mid) && !f.has_event(&mid));
    // forget an event before the first run: every run is stale, the full open is used
    f.set_clock(tick_clock());
    f.forget_events(&[early]).unwrap();
    drop(f);
    let rep = assert_fast_equals_full(d.path(), &mut Rng(2));
    assert!(!rep.fast_used && rep.fallback_reason.as_deref().unwrap().contains("stale"), "{rep:?}");
    // after a new run (from version 0) the fast open is used again; the stale files are deleted
    let mut l = VersionedLog::open_with(d.path(), OWNER, FAST).unwrap();
    l.set_clock(tick_clock());
    let r = l.maintain_index_runs().unwrap();
    assert!(r.written.as_ref().unwrap().from == 0 && r.runs == 1 && r.deleted == 2, "{r:?}");
    drop(l);
    let rep = assert_fast_equals_full(d.path(), &mut Rng(4));
    assert!(rep.fast_used, "{rep:?}");
    VersionedLog::open(d.path(), OWNER).unwrap().verify().unwrap();
}

#[test]
fn reingest_of_forgotten_event_suppressed_with_runs() {
    let d = tempfile::tempdir().unwrap();
    let mut l = v5_log(d.path());
    let e = ev("2025-01-09", "secret");
    let id = l.append_events(&[e.clone()]).unwrap().event_ids[0].clone();
    l.append_events(&[ev("2025-01-09", "other")]).unwrap();
    l.forget_events(&[id]).unwrap();
    force(&mut l);
    drop(l);
    let mut f = VersionedLog::open_with(d.path(), OWNER, FAST).unwrap();
    assert!(f.open_report().fast_used);
    f.set_clock(tick_clock());
    let info = f.append_events(&[e]).unwrap();
    assert_eq!(info.suppressed, 1);
    assert!(!info.created);
}

/// Owner rule (storage is KORE only): every run file is a KORE row-group file that the existing
/// kore-store reader decodes, and no .json / .jsonl file is created in the log directory.
#[test]
fn run_files_are_kore_and_no_json_files() {
    let d = tempfile::tempdir().unwrap();
    let mut l = v5_log(d.path());
    let mut g = Gen::new(99);
    for i in 0..30 {
        g.commit(&mut l);
        if i % 3 == 2 {
            force(&mut l);
        }
    }
    let id = g.events[2].clone();
    l.forget_events(&[id]).unwrap();
    force(&mut l);
    let _ = l.maintain_runs_inner(Some(RunCrash::BeforeRename), 1, 1);
    let dir = l.dir.clone();
    let runs = l.index_runs();
    drop(l);
    assert!(!runs.is_empty());
    for r in &runs {
        assert!(r.file.ends_with(".kore"));
        let bytes = fs::read(dir.join(&r.file)).unwrap();
        let block = kore_store::rowgroups::read(&bytes, None, None).expect("a KORE row-group file");
        assert!(block.num_rows > 0);
        let idx = kore_store::rowgroups::index(&bytes).unwrap();
        assert!(idx.groups.iter().all(|g| kore_store::KoreReader::from_bytes(&bytes[g.offset as usize..(g.offset + g.len) as usize]).is_ok()));
    }
    let files = files_of(&dir);
    let bad: Vec<&String> = files.keys().filter(|f| f.ends_with(".json") || f.ends_with(".jsonl")).collect();
    assert!(bad.is_empty(), "{bad:?}");
}
