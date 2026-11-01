//! Read-path and no-op re-ingest timings on an existing log, so two binaries can be run on the
//! very same files, interleaved.
//!   prof_openat DATA.jsonl N LOGROOT FORMAT(2|3) [REPS]
//! Builds the log under LOGROOT (owner "p") if it does not exist; otherwise only reads it.
//! Prints: open(), open_at(mid), open_at(latest), open()+open_latest(), get_event (random ids),
//! and one no-op re-ingest pass of all N events (batches of 1000) on a freshly opened handle.
#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;

use serde_json::Value;
use std::time::Instant;
use versioned::*;

fn med(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: usize = a[2].parse().unwrap();
    let root = std::path::PathBuf::from(&a[3]);
    let fmt = if a[4] == "3" { LogFormat::V3 } else { LogFormat::V2 };
    let reps: usize = a.get(5).map(|s| s.parse().unwrap()).unwrap_or(3);
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
    if !root.join("p").join("log").join("MANIFEST").exists() {
        std::fs::create_dir_all(&root).unwrap();
        let mut log = VersionedLog::open_or_create_with_format(&root, "p", || "2026-03-01T00:00:00Z".to_string(), fmt).unwrap();
        let t = Instant::now();
        for c in evs.chunks(1000) {
            log.append_events(c).unwrap();
        }
        println!("built {:?} log in {:.1}s", fmt, t.elapsed().as_secs_f64());
    }
    let mut t_open = vec![];
    let mut t_mid = vec![];
    let mut t_last = vec![];
    let mut t_both = vec![];
    for _ in 0..reps {
        let t = Instant::now();
        let l = VersionedLog::open(&root, "p").unwrap();
        t_open.push(t.elapsed().as_secs_f64());
        let latest = l.latest_version();
        let t = Instant::now();
        let s = l.open_at(AsOf::Version(latest / 2)).unwrap();
        t_mid.push(t.elapsed().as_secs_f64());
        drop(s);
        let t = Instant::now();
        let s = l.open_at(AsOf::Version(latest)).unwrap();
        t_last.push(t.elapsed().as_secs_f64());
        drop(s);
        let t = Instant::now();
        let l2 = VersionedLog::open(&root, "p").unwrap();
        let rows = l2.open_latest().raw(Table::Events).len();
        t_both.push(t.elapsed().as_secs_f64());
        assert_eq!(rows, n);
    }
    println!("open()                 median {:.3}s  all {:?}", med(t_open.clone()), t_open.iter().map(|x| (x * 1000.0).round() / 1000.0).collect::<Vec<_>>());
    println!("open_at(mid)           median {:.3}s  all {:?}", med(t_mid.clone()), t_mid.iter().map(|x| (x * 1000.0).round() / 1000.0).collect::<Vec<_>>());
    println!("open_at(latest)        median {:.3}s  all {:?}", med(t_last.clone()), t_last.iter().map(|x| (x * 1000.0).round() / 1000.0).collect::<Vec<_>>());
    println!("open()+open_latest()   median {:.3}s  all {:?}", med(t_both.clone()), t_both.iter().map(|x| (x * 1000.0).round() / 1000.0).collect::<Vec<_>>());
    let l = VersionedLog::open(&root, "p").unwrap();
    let ids: Vec<String> = (0..100).map(|k| {
        let e = &evs[((k as u64 * 2654435761 + 12345) % n as u64) as usize];
        versioned::event_id(&e.source, &normalize_ts(&e.occurred_at).unwrap(), &e.text)
    }).collect();
    let mut ts = vec![];
    for _ in 0..reps {
        let t = Instant::now();
        for id in &ids {
            l.get_event(id).unwrap().expect("found");
        }
        ts.push(t.elapsed().as_secs_f64() / ids.len() as f64 * 1000.0);
    }
    println!("get_event              median {:.3} ms  all {:?}", med(ts.clone()), ts.iter().map(|x| (x * 100.0).round() / 100.0).collect::<Vec<_>>());
    drop(l);
    let mut l = VersionedLog::open(&root, "p").unwrap();
    let t = Instant::now();
    let mut created = 0;
    for c in evs.chunks(1000) {
        if l.append_events(c).unwrap().created {
            created += 1;
        }
    }
    println!("no-op re-ingest pass   {:.2}s created {created}", t.elapsed().as_secs_f64());
}
