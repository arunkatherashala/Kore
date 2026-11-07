//! Writes a small versioned log in log FORMAT 3 or 4 plus an `expected.json` (per-version manifest
//! hash, content hash, row counts, a SQL result), like `gen_vlog_fixture_v2`. Used once per fixture:
//!
//!   gen_vlog_fixture_v4 OUT_DIR 3        format 3 (journal MANIFEST), written by the pre-format-4 binary
//!   gen_vlog_fixture_v4 OUT_DIR 4        format 4 (v4 compact segments + shared dictionary)
//!   gen_vlog_fixture_v4 OUT_DIR mixed4   format 1 first, then `upgrade_to_v4`
//!
//! The committed fixtures are `kore-self/tests/data/vlog_v3/` and `kore-self/tests/data/vlog_v4/`.
#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;

use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use versioned::sql::block_strings;
use versioned::*;

fn ev(source: &str, at: &str, kind: &str, text: &str, tags: &[&str]) -> NewEvent {
    NewEvent { source: source.into(), occurred_at: at.into(), kind: kind.into(), text: text.into(), tags: tags.iter().map(|s| s.to_string()).collect(), redacted: false }
}

fn rev(key: &str, stance: &str, conf: f64, vf: &str, author: &str, causes: Vec<String>) -> NewRevision {
    NewRevision {
        entity_type: "belief".into(), key: key.into(), stance: stance.into(), confidence: conf, rationale: format!("because {stance}"),
        supersedes: None, valid_from: vf.into(), author: author.into(), caused_by: causes,
    }
}

mod fmt {
    use super::versioned::*;
    /// (format the log is created with, batch after which the log is upgraded)
    pub fn plan(which: &str) -> (LogFormat, Option<u32>) {
        match which {
            "3" => (LogFormat::V3, None),
            "4" => (LogFormat::V4, None),
            "mixed4" => (LogFormat::V1, Some(1)),
            other => panic!("unknown fixture kind {other}"),
        }
    }
    /// Small dictionary thresholds so that the 100-event fixture has a dictionary (and the forget
    /// below removes texts that are in it).
    pub fn prepare(l: &mut VersionedLog) {
        l.set_dict_policy(DictPolicy { train_events: 20, max_bytes: 4096, check_events: 0, check_min_text: 0, min_gain_pct: 5 });
    }
    pub fn upgrade(l: &mut VersionedLog) {
        assert!(l.upgrade_to_v4().unwrap());
    }
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("OUT_DIR"));
    let which = std::env::args().nth(2).expect("3 | 4 | mixed4");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let n = Arc::new(AtomicU64::new(0));
    let clock = move || {
        let k = n.fetch_add(1, Ordering::SeqCst) + 1;
        format!("2026-03-01T{:02}:{:02}:00Z", k / 60, k % 60)
    };
    let (create, upgrade_at) = fmt::plan(&which);
    let mut l = VersionedLog::open_or_create_with_format(&out, "fixture", clock, create).unwrap();
    fmt::prepare(&mut l);
    let kinds = ["note", "action", "decision", "outcome", "statement"];
    let srcs = ["journal", "chat", "calendar"];
    let mut ids: Vec<String> = vec![];
    for b in 0..5u32 {
        let evs: Vec<NewEvent> = (0..20u32)
            .map(|i| {
                let k = b * 20 + i;
                let tags: Vec<&str> = match k % 4 { 0 => vec![], 1 => vec!["alpha"], 2 => vec!["alpha", "beta"], _ => vec!["gamma", "beta", "delta"] };
                ev(srcs[(k % 3) as usize], &format!("2025-06-{:02}T{:02}:{:02}:{:02}Z", 1 + k % 28, k % 24, (k * 7) % 60, (k * 13) % 60), kinds[(k % 5) as usize],
                   &format!("fixture event {k} with some text, unicode caf\u{e9} \u{4e16}\u{754c} and repeated words words words"), &tags)
            })
            .collect();
        ids.extend(l.append_events(&evs).unwrap().event_ids);
        if upgrade_at == Some(b) {
            fmt::upgrade(&mut l);
        }
        if b % 2 == 1 {
            l.revise(rev("topic-a", &format!("stance {b}"), 0.25 + b as f64 / 10.0, "2025-07-01", "rule:r1", vec![ids[(b * 5) as usize].clone(), ids[(b * 5 + 1) as usize].clone()])).unwrap();
        }
    }
    l.revise(rev("topic-a", "stance 3", 0.55, "2025-07-01", "rule:r1", vec![ids[15].clone(), ids[16].clone()])).unwrap();
    l.revise(rev("topic-b", "x", 0.5, "2025-07-02", "user", vec![])).unwrap();
    let p = NewPrediction {
        made_at: "2026-01-01".into(), question_id: "q1".into(), target: "run tomorrow".into(), options: vec!["yes".into(), "no".into()],
        probs: vec![0.7, 0.3], model_id: "freq".into(), model_version: "0".into(), features_cite: vec![ids[3].clone(), ids[4].clone()], resolves_by: Some("2026-01-02".into()),
    };
    let pid = l.record_prediction(p).unwrap().pred_ids[0].clone();
    l.record_outcome(NewOutcome { pred_id: pid, resolved_at: "2026-01-02".into(), actual: "yes".into(), outcome_event_id: Some(ids[5].clone()) }).unwrap();
    let r = l.forget_events(&[ids[7].clone(), ids[40].clone()]).unwrap();
    assert!(r.created);
    l.append_events(&[ev("journal", "2025-08-01", "note", "after the forget", &["omega"])]).unwrap();
    l.verify().unwrap();

    let mut versions = vec![];
    for vi in l.versions() {
        let s = l.open_at(AsOf::Version(vi.v)).unwrap();
        versions.push(json!({"v": vi.v, "recorded_at": vi.recorded_at, "manifest_hash": vi.manifest_hash, "content_hash": s.content_hash(), "rows": vi.rows}));
    }
    let head = l.open_latest();
    let b = run_readonly_sql(&head, "SELECT kind, COUNT(*) FROM events GROUP BY kind ORDER BY kind").unwrap();
    let all_ids: Vec<String> = head.rows::<EventRow>().into_iter().map(|e| e.event_id).collect();
    let forgotten = [ids[7].clone(), ids[40].clone()];
    let exp = json!({
        "owner": "fixture", "format": l.format().number(), "versions": versions, "latest": l.latest_version(),
        "sql_kind_counts": block_strings(&b), "event_ids": all_ids, "forgotten_ids": forgotten,
        "head_event_hex_sample": ids[10],
    });
    std::fs::write(out.join("expected.json"), serde_json::to_string_pretty(&exp).unwrap()).unwrap();
    println!("wrote fixture ({which}), {} versions, head {}", l.latest_version() + 1, l.snapshot_hash(l.latest_version()).unwrap());
}
