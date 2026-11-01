//! Commit cost as the number of versions grows (item "MANIFEST rewrite").
//!   prof_commit WORKDIR FORMAT(1|2|3) COMMITS [EVENTS_PER_COMMIT]
//! Appends COMMITS single-event commits (fsync per commit, as in production) and
//! reports the median / min / max milliseconds per commit in windows of 100 commits.
#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;

use std::time::Instant;
use versioned::*;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let work = std::path::PathBuf::from(&a[1]);
    let fmt = match a[2].as_str() {
        "1" => LogFormat::V1,
        "2" => LogFormat::V2,
        _ => LogFormat::V3,
    };
    let commits: usize = a[3].parse().unwrap();
    let per: usize = a.get(4).map(|s| s.parse().unwrap()).unwrap_or(1);
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let mut log = VersionedLog::open_or_create_with_format(&work, "p", || "2026-03-01T00:00:00Z".to_string(), fmt).unwrap();
    let mut times = Vec::with_capacity(commits);
    for c in 0..commits {
        let evs: Vec<NewEvent> = (0..per)
            .map(|k| NewEvent {
                source: "prof".into(),
                occurred_at: "2026-01-01T00:00:00Z".into(),
                kind: "note".into(),
                text: format!("commit {c} event {k} some text that is a little longer than a few words"),
                tags: vec!["t".into()],
                redacted: false,
            })
            .collect();
        let t = Instant::now();
        log.append_events(&evs).unwrap();
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let manifest = std::fs::metadata(work.join("p").join("log").join("MANIFEST")).unwrap().len();
    println!("format {:?} commits {commits} events/commit {per} final MANIFEST {manifest} bytes", fmt);
    let w = 100.min(commits);
    let mut lo = 0;
    while lo < commits {
        let hi = (lo + w).min(commits);
        let mut v: Vec<f64> = times[lo..hi].to_vec();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("commits {lo:>5}..{hi:<5} median {:.2} ms  min {:.2}  max {:.2}", v[v.len() / 2], v[0], v[v.len() - 1]);
        lo += if commits > 1000 { (commits / 6).max(w) } else { w };
    }
    let mut all = times.clone();
    all.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("all commits: median {:.2} ms, mean {:.2} ms", all[all.len() / 2], times.iter().sum::<f64>() / times.len() as f64);
    log.verify().unwrap();
    drop(log);
    let _ = std::fs::remove_dir_all(&work);
}
