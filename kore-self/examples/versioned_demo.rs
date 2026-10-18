//! Demo of the versioned log: ~20 dated events, 3 belief revisions citing
//! them, the same query AS OF versions 1, 2, 3, then verify and a tamper check.
//! Uses only a temporary directory. Run:
//!   cargo run -p kore-self --example versioned_demo --offline

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;

use std::path::Path;
use versioned::*;

const QUERY: &str = "SELECT r.valid_from, r.stance, r.confidence, r.rationale, e.occurred_at, e.source, e.text \
FROM revisions r JOIN revision_causes c ON r.rev_id = c.rev_id JOIN events e ON c.event_id = e.event_id \
WHERE r.entity_type='belief' AND r.key='remote work' ORDER BY r.valid_from";

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for f in std::fs::read_dir(from).unwrap().flatten() {
        let dst = to.join(f.file_name());
        if f.path().is_dir() {
            copy_dir(&f.path(), &dst);
        } else {
            std::fs::copy(f.path(), dst).unwrap();
        }
    }
}

fn main() {
    let tmp = std::env::temp_dir().join(format!("kore-self-vlog-demo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let root = tmp.join("root");
    let mut log = VersionedLog::open_or_create(&root, "demo").unwrap();

    let events: Vec<NewEvent> = (0..20)
        .map(|i| NewEvent {
            source: if i % 2 == 0 { "journal" } else { "chat" }.into(),
            occurred_at: format!("2025-{:02}-{:02}", 1 + i / 5, 3 + i),
            kind: "note".into(),
            text: format!("day {i}: {}", ["worked from home, got a lot done", "office day, long commute", "team lunch at the office", "remote call ran long, tired"][i as usize % 4]),
            tags: vec!["work".into()],
            redacted: false,
        })
        .collect();
    let id = |i: usize| event_id(&events[i].source, &normalize_ts(&events[i].occurred_at).unwrap(), &events[i].text);
    let rev = |stance: &str, conf: f64, from: &str, author: &str, cites: &[usize]| NewRevision {
        entity_type: "belief".into(), key: "remote work".into(), stance: stance.into(), confidence: conf,
        rationale: format!("{} cited events", cites.len()), supersedes: None, valid_from: from.into(),
        author: author.into(), caused_by: cites.iter().map(|&i| id(i)).collect(),
    };
    let r1 = rev("prefer office", 0.70, "2025-01-20", "rule:stance-v1", &[1, 2]);
    let r2 = rev("mixed", 0.55, "2025-02-20", "rule:stance-v1", &[5, 6, 7]);
    let r3 = rev("prefer remote", 0.80, "2025-03-25", "user", &[12, 0]);

    log.commit_batch(Batch { events: events.clone(), revisions: vec![r1], ..Default::default() }).unwrap(); // v1
    log.revise(r2).unwrap(); // v2
    log.revise(r3).unwrap(); // v3

    println!("versions:");
    for v in log.versions() {
        println!("  v{} recorded_at={} events={} revisions={} hash={}", v.v, v.recorded_at, v.rows["events"], v.rows["revisions"], &v.manifest_hash[..12]);
    }
    for v in 1..=3 {
        println!("\n=== AS OF version {v} ===");
        let snap = log.open_at(AsOf::Version(v)).unwrap();
        println!("{}", sql::render(&run_readonly_sql(&snap, QUERY).unwrap(), 38));
    }

    println!("\nverify (original log): {:?}", log.verify().map(|_| "OK"));

    let copy_root = tmp.join("tampered");
    copy_dir(&root, &copy_root);
    let seg = copy_root.join("demo").join("log").join("events").join(format!("{:010}.kore", 1));
    let mut bytes = std::fs::read(&seg).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x01;
    std::fs::write(&seg, bytes).unwrap();
    let copy = VersionedLog::open(&copy_root, "demo");
    // Opening reads every segment and refuses a tampered log; verify_dir names the file.
    println!("open (tampered copy, one byte flipped in events/{:010}.kore): {}", 1, match &copy { Ok(_) => "opened".to_string(), Err(e) => format!("REFUSED: {e}") });
    match verify_dir(&copy_root.join("demo").join("log")) {
        Ok(()) => println!("verify (tampered copy): OK  <-- unexpected"),
        Err(e) => println!("verify (tampered copy): FAIL: {e}"),
    }
    println!("verify (original log, untouched): {:?}", log.verify().map(|_| "OK"));
    let _ = std::fs::remove_dir_all(&tmp);
}
