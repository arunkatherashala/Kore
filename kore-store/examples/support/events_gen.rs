//! Digital Life `events`-shaped test table: 7 string columns (hex id, two ISO timestamps, source, kind,
//! text, tags with some nulls) and a bool, as kore-self log format 1 stores it. Deterministic.

use kore_core::{Column, DataBlock};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 { let mut x = self.0; x ^= x << 13; x ^= x >> 7; x ^= x << 17; self.0 = x; x }
    fn below(&mut self, n: u64) -> u64 { self.next() % n }
}

const WORDS: &[&str] = &["the", "request", "failed", "with", "status", "retrying", "after", "timeout", "user", "opened",
    "settings", "index", "sync", "completed", "connection", "reset", "peer", "worker", "queue", "cache", "miss", "hit"];

pub fn events(n: usize, seed: u64) -> DataBlock {
    let mut r = Rng(seed | 1);
    let sources = ["cli", "daemon", "api", "scheduler", "ui", "importer", "agent", "hook"];
    let kinds = ["message", "tool_call", "tool_result", "error", "note"];
    let mut cols: Vec<Vec<Option<String>>> = vec![Vec::with_capacity(n); 7];
    let mut red = Vec::with_capacity(n);
    let mut t = 1_761_000_000u64;
    for i in 0..n {
        cols[0].push(Some(format!("{:016x}{:016x}{:016x}{:016x}", r.next(), r.next(), r.next(), r.next())));
        t += r.below(90);
        let ts = format!("2025-{:02}-{:02}T{:02}:{:02}:{:02}Z", 1 + (t / 2_600_000) % 12, 1 + (t / 86400) % 28, (t / 3600) % 24, (t / 60) % 60, t % 60);
        cols[1].push(Some(ts.clone()));
        cols[2].push(Some(ts));
        cols[3].push(Some(sources[r.below(8) as usize].to_string()));
        cols[4].push(Some(kinds[r.below(5) as usize].to_string()));
        let len = 6 + r.below(30);
        let mut s = format!("event {i}:");
        for _ in 0..len { s.push(' '); s.push_str(WORDS[r.below(WORDS.len() as u64) as usize]); }
        cols[5].push(Some(s));
        cols[6].push(if r.below(3) == 0 { None } else { Some(format!("t{},t{}", r.below(20), r.below(20))) });
        red.push(Some(false));
    }
    let names = ["event_id", "occurred_at", "recorded_at", "source", "kind", "text", "tags"];
    let mut out: Vec<Column> = names.iter().zip(cols).map(|(n, v)| Column::str_col(*n, v)).collect();
    out.push(Column::bool_col("redacted", red));
    DataBlock::new(out).unwrap()
}

