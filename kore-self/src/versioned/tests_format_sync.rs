//! Format sync (docs/format_sync.md): logs, index runs, checkpoints and brain files written by dev's
//! kore-store (d9edbcfc, before the sync) must open with the synced kore-store with the same content.
//!
//! The fixtures live in `kore-store/tests/fixtures_dev/kore-self` and were written ONCE by the ignored
//! generator below with dev's code:
//!   KORE_GEN_DEV_FIXTURES=<dir> cargo test -p kore-self --offline gen_dev_format_fixtures -- --ignored
//! `kore-self.expect` pins what dev read back (version hashes, checkpoint, rows, and a value digest of
//! every `.kore` file read with kore-store). Never re-pin to hide a decode change.

use crate::versioned::*;
use kore_store::{DictSet, SharedDict};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "../../../kore-store/tests/common/dev_data.rs"]
#[allow(dead_code)]
mod dev_data;

const OWNER: &str = "tester";

fn tick_clock() -> impl Fn() -> String + Send + 'static {
    let n = Arc::new(AtomicU64::new(0));
    move || {
        let k = n.fetch_add(1, Ordering::SeqCst) + 1;
        format!("2026-03-{:02}T{:02}:{:02}:00Z", 1 + k / 3600 % 28, k / 60 % 60, k % 60)
    }
}

fn ev(day: u64, text: &str) -> NewEvent {
    NewEvent { source: "s".into(), occurred_at: format!("2025-01-{:02}", day % 28 + 1), kind: "note".into(), text: text.into(), tags: vec!["t:1".into(), "topic:x".into()], redacted: false }
}

/// Deterministic batches: events, revisions with JSON rationale, a prediction and its outcome.
fn commits(l: &mut VersionedLog, from: u64, n: u64) {
    let mut last_ev: Option<String> = None;
    for k in from..from + n {
        let mut b = Batch::default();
        for j in 0..1 + k % 3 {
            b.events.push(ev(k + j, &format!("user {} said: remember the meeting about project {} ({k}.{j})", k % 7, k % 11)));
        }
        if let Some(c) = last_ev.clone() {
            if k % 2 == 0 {
                b.revisions.push(NewRevision { entity_type: "belief".into(), key: format!("k{}", k % 5), stance: format!("x{}", k % 3), confidence: 0.25 + (k % 4) as f64 / 8.0,
                    rationale: format!("{{\"why\":\"s{k}\",\"nested\":{{\"a\":[1,2,{{\"b\":null}}]}}}}"), supersedes: None, valid_from: "2025-01-01".into(), author: "rule:r".into(), caused_by: vec![c.clone()] });
            }
            if k % 5 == 0 {
                let p = NewPrediction { made_at: "2025-01-01".into(), question_id: format!("q{k}"), target: "t".into(), options: vec!["yes".into(), "no".into()], probs: vec![0.25, 0.75],
                    model_id: "m".into(), model_version: "1".into(), features_cite: vec![c.clone()], resolves_by: None };
                let pid = prediction_id(&p).unwrap();
                b.predictions.push(p);
                b.outcomes.push(NewOutcome { pred_id: pid, resolved_at: "2025-02-01".into(), actual: "yes".into(), outcome_event_id: None });
            }
        }
        let info = l.commit_batch(b).unwrap();
        last_ev = info.event_ids.last().cloned().or(last_ev);
    }
}

fn json_rows() -> Vec<serde_json::Value> {
    (0..12).map(|k| serde_json::json!({
        "step": k, "name": format!("n{k}"), "score": k as f64 / 3.0, "ok": k % 2 == 0,
        "meta": {"a": [k, {"b": "caf\u{e9}"}], "c": null}, "mixed": if k % 3 == 0 { serde_json::json!(1) } else { serde_json::json!("one") },
    })).collect()
}

fn walk(base: &Path, d: &Path, out: &mut Vec<(String, PathBuf)>) {
    let mut v: Vec<_> = fs::read_dir(d).unwrap().flatten().map(|e| e.path()).collect();
    v.sort();
    for p in v {
        if p.is_dir() {
            walk(base, &p, out);
        } else {
            out.push((p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"), p));
        }
    }
}

/// How kore-store reads one `.kore` file of the fixture: row-group container, dictionary-aware reader,
/// or the error class (format-2/3/5 log segments are kore-self's own codec, not kore-store files).
fn kore_store_view(p: &Path, dicts: &DictSet) -> String {
    let b = fs::read(p).unwrap();
    if kore_store::rowgroups::is_row_group_file(&b) {
        return match kore_store::rowgroups::read(&b, None, None) {
            Ok(x) => format!("rowgroups\n{}", dev_data::digest(&x)),
            Err(_) => "rowgroups-unreadable\n".into(),
        };
    }
    match kore_store::KoreReader::from_bytes_with_dicts(&b, dicts) {
        Ok(x) => format!("kore\n{}", dev_data::digest(&x)),
        Err(_) => "not-a-kore-store-file\n".into(),
    }
}

fn dicts_of(dir: &Path) -> DictSet {
    let mut s = DictSet::new();
    let mut files = vec![];
    if dir.exists() {
        walk(dir, dir, &mut files);
    }
    for (_, p) in files {
        if p.extension().map_or(false, |e| e == "kdic") {
            s.add(SharedDict::read_file(&p).unwrap());
        }
    }
    s
}

/// The pinned description of the fixture set (what dev wrote and reads back).
fn describe(root: &Path) -> String {
    let mut out = String::new();
    for (name, fmt) in [("vlog_v5", LogFormat::V5), ("vlog_v4", LogFormat::V4)] {
        let l = VersionedLog::open(&root.join(name), OWNER).unwrap();
        assert_eq!(l.format(), fmt);
        l.verify().unwrap();
        out += &format!("== log {name} format {} latest {}\n", l.format().number(), l.latest_version());
        for v in 0..=l.latest_version() {
            out += &format!("v {v} {}\n", l.snapshot_hash(v).unwrap());
        }
        out += &format!("runs {}\n", l.index_runs().len());
        for (v, f) in l.checkpoint_files("fsync") {
            let c = l.read_checkpoint("fsync", &f).unwrap();
            out += &format!("ckpt {v} {f} step {} content {} payload {}\n", c.step, c.content_hash, hex(&c.payload));
        }
    }
    let rows = crate::kore_rows::read(&root.join("brain").join("rows.kore")).unwrap();
    out += &format!("== rows {}\n", serde_json::to_string(&rows).unwrap());
    let payload = crate::aru_store::read_payload(&root.join("brain").join("payload.kore")).unwrap().unwrap();
    out += &format!("== payload {payload}\n");
    let mut files = vec![];
    walk(root, root, &mut files);
    for (name, p) in files {
        if !name.ends_with(".kore") {
            continue;
        }
        let log = name.split('/').next().unwrap();
        out += &format!("== file {name} {}", kore_store_view(&p, &dicts_of(&root.join(log))));
    }
    out
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("kore-store").join("tests").join("fixtures_dev").join("kore-self")
}

#[test]
#[ignore]
fn gen_dev_format_fixtures() {
    let Ok(dir) = std::env::var("KORE_GEN_DEV_FIXTURES") else { return };
    let root = PathBuf::from(dir);
    assert!(!root.exists(), "{} exists", root.display());
    generate(&root);
    let d = describe(&root);
    assert!(d.contains("runs 1"), "{d}");
    fs::write(root.join("kore-self.expect"), d).unwrap();
}

/// The fixture set (deterministic: tick clock, fixed data).
fn generate(root: &Path) {
    // format 5: kore-self segments, an index run (kore-store row groups) and a checkpoint (kore-store file)
    let p5 = root.join("vlog_v5");
    let mut l = VersionedLog::open_or_create_with_format(&p5, OWNER, tick_clock(), LogFormat::V5).unwrap();
    commits(&mut l, 0, 40);
    let r = l.maintain_index_runs().unwrap();
    assert!(r.skipped.is_none(), "{r:?}");
    commits(&mut l, 40, 6);
    l.write_checkpoint("fsync", 7, b"checkpoint payload \x00\xff with bytes".to_vec()).unwrap();
    drop(l);
    // format 4: kore-store version-4 segments with a shared dictionary
    let p4 = root.join("vlog_v4");
    let mut l = VersionedLog::open_or_create_with_format(&p4, OWNER, tick_clock(), LogFormat::V4).unwrap();
    l.set_dict_policy(DictPolicy { train_events: 30, max_bytes: 4096, check_events: 0, check_min_text: 0, min_gain_pct: 5 });
    commits(&mut l, 0, 30);
    l.write_checkpoint("fsync", 3, vec![]).unwrap();
    drop(l);
    // brain files
    fs::create_dir_all(root.join("brain")).unwrap();
    crate::kore_rows::write(&root.join("brain").join("rows.kore"), &json_rows()).unwrap();
    crate::aru_store::write_payload(&root.join("brain").join("payload.kore"), "{\"plan\":{\"steps\":[1,2,3]},\"note\":\"caf\u{e9}\"}").unwrap();
}

/// Old files (dev's writer) open with the current code: same versions, same content hashes, same
/// index-run use, same checkpoint, same brain rows, same values of every `.kore` file.
#[test]
fn dev_written_logs_and_brain_files_read_the_same_with_the_synced_format() {
    let src = fixture_root();
    let want = fs::read_to_string(src.join("kore-self.expect")).unwrap().replace("\r\n", "\n");
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("fx");
    copy_tree(&src, &root);
    fs::remove_file(root.join("kore-self.expect")).unwrap();
    let got = describe(&root);
    if got != want {
        for (a, b) in want.lines().zip(got.lines()) {
            assert_eq!(a, b, "first difference against kore-self.expect");
        }
        panic!("kore-self.expect: {} lines pinned, {} now", want.lines().count(), got.lines().count());
    }
    // the fast open (index runs) gives the full open's answers and uses the dev-written run
    let fast = VersionedLog::open_with(&root.join("vlog_v5"), OWNER, OpenOptions { fast_open: true }).unwrap();
    let full = VersionedLog::open(&root.join("vlog_v5"), OWNER).unwrap();
    assert_eq!(fast.latest_version(), full.latest_version());
    assert_eq!(fast.try_open_latest().unwrap().content_hash(), full.try_open_latest().unwrap().content_hash());
    assert_eq!(fast.event_count(), full.event_count());
    // and the old log keeps working: a new commit on top verifies
    let mut l = VersionedLog::open(&root.join("vlog_v4"), OWNER).unwrap();
    l.set_clock(tick_clock());
    commits(&mut l, 100, 3);
    l.verify().unwrap();
}

fn copy_tree(a: &Path, b: &Path) {
    fs::create_dir_all(b).unwrap();
    for f in fs::read_dir(a).unwrap().flatten() {
        let p = f.path();
        let q = b.join(f.file_name());
        if p.is_dir() {
            copy_tree(&p, &q);
        } else {
            fs::copy(&p, &q).unwrap();
        }
    }
}

/// Writing the same fixture set again with the current code gives the bytes dev wrote, file for file
/// (logs, MANIFESTs, dictionaries, index run, checkpoints, brain files). Differences are listed.
#[test]
fn rewriting_the_fixture_set_gives_the_bytes_dev_wrote() {
    let src = fixture_root();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("fx");
    generate(&root);
    let (mut a, mut b) = (vec![], vec![]);
    walk(&src, &src, &mut a);
    walk(&root, &root, &mut b);
    a.retain(|(n, _)| n != "kore-self.expect");
    let names = |v: &[(String, PathBuf)]| v.iter().map(|x| x.0.clone()).collect::<Vec<_>>();
    assert_eq!(names(&a), names(&b), "same file set");
    let differ: Vec<&str> = a.iter().zip(&b).filter(|(x, y)| fs::read(&x.1).unwrap() != fs::read(&y.1).unwrap()).map(|(x, _)| x.0.as_str()).collect();
    assert!(differ.is_empty(), "{} of {} files differ: {differ:?}", differ.len(), a.len());
}
