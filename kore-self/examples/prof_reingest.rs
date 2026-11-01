//! Phase timers for re-ingesting identical events (item "re-ingest at 1M").
//!   prof_reingest DATA.jsonl N WORKDIR FORMAT(1|2) [ROUNDS]
//! Builds a log of N events (batches of 1000), then times: validation alone
//! (`check_new_event`), the full no-op `append_events` pass, and the same pass
//! after reopening the log (cold index), per round.
#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;

use serde_json::Value;
use std::time::Instant;
use versioned::*;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: usize = a[2].parse().unwrap();
    let work = std::path::PathBuf::from(&a[3]);
    let fmt = if a[4] == "2" { LogFormat::V2 } else { LogFormat::V1 };
    let rounds: usize = a.get(5).map(|s| s.parse().unwrap()).unwrap_or(2);
    let text = std::fs::read_to_string(&a[1]).unwrap();
    let mut evs: Vec<NewEvent> = vec![];
    for line in text.lines() {
        let o: Value = serde_json::from_str(line).unwrap();
        if o["t"] != "e" {
            continue;
        }
        let s = |k: &str| o[k].as_str().unwrap().to_string();
        let tags = s("tags");
        evs.push(NewEvent {
            source: s("source"), occurred_at: s("occurred_at"), kind: s("kind"), text: s("text"),
            tags: if tags.is_empty() { vec![] } else { tags.split(',').map(|x| x.to_string()).collect() }, redacted: false,
        });
        if evs.len() == n {
            break;
        }
    }
    drop(text);
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let mut log = VersionedLog::open_or_create_with_format(&work, "p", || "2026-03-01T00:00:00Z".to_string(), fmt).unwrap();
    let t = Instant::now();
    for c in evs.chunks(1000) {
        log.append_events(c).unwrap();
    }
    println!("format {:?} build {:.2}s versions {}", fmt, t.elapsed().as_secs_f64(), log.latest_version());
    for r in 0..rounds {
        let t = Instant::now();
        let mut ok = 0;
        for e in &evs {
            if VersionedLog::check_new_event(e).is_ok() {
                ok += 1;
            }
        }
        let tv = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let mut created = 0;
        for c in evs.chunks(1000) {
            if log.append_events(c).unwrap().created {
                created += 1;
            }
        }
        let tr = t.elapsed().as_secs_f64();
        println!("round {r}: validate-only {tv:.2}s ({ok}); append_events no-op pass {tr:.2}s created {created}; difference {:.2}s", tr - tv);
    }
    let t = Instant::now();
    let log2 = VersionedLog::open(&work, "p").unwrap();
    println!("reopen {:.2}s", t.elapsed().as_secs_f64());
    drop(log);
    let mut log2 = log2;
    let t = Instant::now();
    for c in evs.chunks(1000) {
        log2.append_events(c).unwrap();
    }
    println!("no-op pass on reopened (lazy) log {:.2}s", t.elapsed().as_secs_f64());
    drop(log2);
    let _ = std::fs::remove_dir_all(&work);
}
