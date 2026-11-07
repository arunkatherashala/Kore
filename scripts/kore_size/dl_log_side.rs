//! Digital Life side of the kore_size benchmark (docs/kore_size.md).
//!
//! This file is NOT built in this repository. It is copied into `kore-self/examples/` of a Digital Life
//! checkout (main-Digital-Life-AI) whose `kore-store/` directory was replaced by this branch's
//! `kore-store/` (see scripts/kore_size/run.sh). It uses the versioned log through its public API only.
//!
//!   dl_log_side --data DATA.jsonl --n N --work DIR --format 1|3 --out RESULT.json [--reps 3] [--lookups 100]
//!
//! It builds the log (batches of 1000 events, like bench_log_formats), KEEPS the directory (the store
//! side re-encodes the format 1 segments), and measures: build time, size on disk, open + decode of the
//! latest snapshot (full read), and get_event (point lookup by id on a freshly opened log). The clock is
//! fixed ("2026-01-01T00:00:00Z", the same constant as the Python side) so sizes are deterministic.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Instant;
use versioned::*;

const BATCH: usize = 1000;
const OWNER: &str = "bench";
const REC: &str = "2026-01-01T00:00:00Z";

struct Rev { key: String, stance: String, conf: f64, rationale: String, valid_from: String, cause: usize }

fn load(path: &str) -> (Vec<NewEvent>, Vec<Rev>) {
    let text = std::fs::read_to_string(path).expect("read data");
    let (mut ev, mut rv) = (vec![], vec![]);
    for line in text.lines() {
        let o: Value = serde_json::from_str(line).unwrap();
        let s = |k: &str| o[k].as_str().unwrap().to_string();
        if o["t"] == "e" {
            let tags = s("tags");
            ev.push(NewEvent {
                source: s("source"),
                occurred_at: s("occurred_at"),
                kind: s("kind"),
                text: s("text"),
                tags: if tags.is_empty() { vec![] } else { tags.split(',').map(|x| x.to_string()).collect() },
                redacted: false,
            });
        } else {
            rv.push(Rev { key: s("key"), stance: s("stance"), conf: o["conf"].as_f64().unwrap(), rationale: s("rationale"), valid_from: s("valid_from"), cause: o["cause"].as_u64().unwrap() as usize });
        }
    }
    (ev, rv)
}

fn median(xs: &[f64]) -> f64 {
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

fn stats(xs: &[f64]) -> Value {
    json!({"median": median(xs), "min": xs.iter().cloned().fold(f64::INFINITY, f64::min), "max": xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max), "reps": xs.len(), "all": xs})
}

fn dir_size(p: &Path) -> u64 {
    let mut t = 0;
    if let Ok(rd) = std::fs::read_dir(p) {
        for f in rd.flatten() {
            let m = f.metadata().unwrap();
            t += if m.is_dir() { dir_size(&f.path()) } else { m.len() };
        }
    }
    t
}

fn main() {
    let v: Vec<String> = std::env::args().collect();
    let get = |k: &str| v.iter().position(|a| a == k).map(|i| v[i + 1].clone());
    let n: usize = get("--n").expect("--n").parse().unwrap();
    let data = get("--data").expect("--data");
    let work = PathBuf::from(get("--work").expect("--work"));
    let out = get("--out").expect("--out");
    let reps: usize = get("--reps").map(|s| s.parse().unwrap()).unwrap_or(3);
    let lookups: usize = get("--lookups").map(|s| s.parse().unwrap()).unwrap_or(100);
    let format = match get("--format").as_deref() { Some("3") => LogFormat::V3, Some("2") => LogFormat::V2, _ => LogFormat::V1 };

    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let (events, revs) = load(&data);
    assert_eq!(events.len(), n);
    let ids: Vec<String> = events.iter().map(|e| event_id(&e.source, &normalize_ts(&e.occurred_at).unwrap(), &e.text)).collect();
    let nb = (n + BATCH - 1) / BATCH;
    let batches: Vec<Batch> = (0..nb)
        .map(|b| {
            let (lo, hi) = (b * BATCH, ((b + 1) * BATCH).min(n));
            let revisions = revs.iter().filter(|r| r.cause >= lo && r.cause < hi)
                .map(|r| NewRevision {
                    entity_type: "belief".into(), key: r.key.clone(), stance: r.stance.clone(), confidence: r.conf,
                    rationale: r.rationale.clone(), supersedes: None, valid_from: r.valid_from.clone(),
                    author: "rule:bench".into(), caused_by: vec![ids[r.cause].clone()],
                })
                .collect();
            Batch { events: events[lo..hi].to_vec(), revisions, ..Default::default() }
        })
        .collect();

    // ---- build (one rep: the directory is kept for the store side)
    let root = work.join("log");
    let mut log = VersionedLog::open_or_create_with_format(&root, OWNER, || REC.to_string(), format).unwrap();
    let t = Instant::now();
    for b in &batches { log.commit_batch(b.clone()).unwrap(); }
    let build_s = t.elapsed().as_secs_f64();
    assert_eq!(log.latest_version(), nb as u64);
    drop(log);
    let logdir = root.join(OWNER).join("log");
    let mut per_table = serde_json::Map::new();
    if let Ok(rd) = std::fs::read_dir(&logdir) {
        for f in rd.flatten() {
            let m = f.metadata().unwrap();
            per_table.insert(f.file_name().to_string_lossy().into(), json!(if m.is_dir() { dir_size(&f.path()) } else { m.len() }));
        }
    }
    let mut res = json!({"n": n, "format": format.number(), "batches": nb, "reps": reps, "lookups": lookups,
        "build_s": build_s, "size_bytes": dir_size(&root), "size_by_entry": Value::Object(per_table),
        "log_dir": logdir.to_string_lossy()});
    eprintln!("[dl f{}] built in {build_s:.2}s, {} bytes", format.number(), res["size_bytes"]);

    // ---- full read: open (sha256-verifies every segment) + decode the latest snapshot
    let mut xs = vec![];
    for _ in 0..reps {
        let t = Instant::now();
        let l = VersionedLog::open(&root, OWNER).unwrap();
        let s = l.open_latest();
        assert_eq!(s.raw(Table::Events).len(), n);
        xs.push(t.elapsed().as_secs_f64());
    }
    res["full_read_s"] = stats(&xs);

    // ---- point lookup by id on a freshly opened log
    let idx: Vec<usize> = (0..lookups).map(|k| ((k as u64 * 2654435761 + 12345) % n as u64) as usize).collect();
    let want: usize = idx.iter().map(|&i| events[i].text.len()).sum();
    let mut xs = vec![];
    for _ in 0..reps {
        let fresh = VersionedLog::open(&root, OWNER).unwrap();
        let t = Instant::now();
        let got: usize = idx.iter().map(|&i| fresh.get_event(&ids[i]).unwrap().expect("found").text.len()).sum();
        xs.push(t.elapsed().as_secs_f64() / lookups as f64);
        assert_eq!(got, want);
    }
    res["lookup_per_op_s"] = stats(&xs);
    res["lookup_check"] = json!(want);
    std::fs::write(&out, serde_json::to_string_pretty(&res).unwrap()).unwrap();
}
