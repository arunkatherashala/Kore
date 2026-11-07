//! Encode cost of one events segment: format-2 codec vs format-4 (kore-store v4 compact), with and
//! without a shared dictionary, for small commits (1, 10 events) and a batch of 1000. No disk I/O.
//!
//!   prof_v4_encode DATA.jsonl [REPS]
#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;

use serde_json::Value;
use std::time::Instant;
use versioned::*;

fn main() {
    let path = std::env::args().nth(1).expect("DATA.jsonl");
    let reps: usize = std::env::args().nth(2).map(|s| s.parse().unwrap()).unwrap_or(200);
    let text = std::fs::read_to_string(path).unwrap();
    let mut rows: Vec<Vec<Cell>> = vec![];
    for line in text.lines() {
        let o: Value = serde_json::from_str(line).unwrap();
        if o["t"] != "e" {
            continue;
        }
        let s = |k: &str| o[k].as_str().unwrap().to_string();
        let at = normalize_ts(&s("occurred_at")).unwrap();
        let id = event_id(&s("source"), &at, &s("text"));
        let r = EventRow { event_id: id, occurred_at: at, recorded_at: "2026-03-01T00:00:00Z".into(), source: s("source"), kind: s("kind"), text: s("text"), tags: s("tags"), redacted: false };
        rows.push(r.to_cells());
        if rows.len() >= 3000 {
            break;
        }
    }
    let texts: Vec<String> = rows[..1000].iter().rev().map(|r| r[5].s()).collect();
    let dict = v4log::dict_from_texts(texts.iter().map(|s| s.as_str()), 128 << 10).unwrap();
    // single-event segments (the being commits one event at a time): mean bytes over 500 different events
    let one: Vec<&[Vec<Cell>]> = (1000..1500).map(|i| &rows[i..i + 1]).collect();
    let mean = |f: &dyn Fn(&[Vec<Cell>]) -> usize| one.iter().map(|s| f(s)).sum::<usize>() as f64 / one.len() as f64;
    println!(
        "1-event segments, mean of 500: format-2 codec {:.1} B | v4 no dict {:.1} B | v4 + dict {:.1} B (event text mean {:.1} B)",
        mean(&|s| codec::encode_segment(Table::Events, s).len()),
        mean(&|s| v4log::encode(Table::Events, s, None).len()),
        mean(&|s| v4log::encode(Table::Events, s, Some(&dict)).len()),
        mean(&|s| s[0][5].s().len())
    );
    for n in [1usize, 10, 1000] {
        let seg = &rows[1000..1000 + n];
        let r = if n == 1000 { reps / 20 + 1 } else { reps };
        let time = |f: &dyn Fn() -> usize| -> (f64, usize) {
            let mut b = f();
            let t = Instant::now();
            for _ in 0..r {
                b = f();
            }
            (t.elapsed().as_secs_f64() / r as f64 * 1e3, b)
        };
        let (a, sa) = time(&|| codec::encode_segment(Table::Events, seg).len());
        let (b, sb) = time(&|| v4log::encode(Table::Events, seg, None).len());
        let (c, sc) = time(&|| v4log::encode(Table::Events, seg, Some(&dict)).len());
        let info = kore_store::v4::info(&v4log::encode(Table::Events, seg, Some(&dict))).unwrap();
        let cols: Vec<String> = info.columns.iter().map(|c| format!("{}:t{}s{}{}={}", c.0, c.2, c.3, if c.4.is_some() { "+dict" } else { "" }, c.5)).collect();
        println!("  v4 + dict columns (transform, stage, payload bytes): {}", cols.join(" "));
        println!("{n} events: format-2 codec {a:.3} ms {sa} B | v4 no dict {b:.3} ms {sb} B | v4 + dict {c:.3} ms {sc} B");
    }
}
