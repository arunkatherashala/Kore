//! Digital Life `events` blocks from a realistic JSONL file (kore-self `format_bench_gen_realistic.py`,
//! the dataset of docs/kore_size.md), shaped like log format 4 segments: event_id (64 hex chars, here the
//! sha256 of the JSON line), occurred_at, recorded_at (one commit time per block), source, kind, text,
//! tags, redacted. The shared dictionary is built like kore-self `dict_from_texts`: whole texts, newest
//! that fit into `max_bytes`, oldest first, separated by 0xFF.
#![allow(dead_code)]

use kore_core::{Column, DataBlock};
use kore_store::SharedDict;
use sha2::{Digest, Sha256};

pub struct Ev { pub id: String, pub occurred_at: String, pub source: String, pub kind: String, pub text: String, pub tags: String }

pub fn load(path: &std::path::Path, n: usize) -> Vec<Ev> {
    let s = std::fs::read_to_string(path).expect("read jsonl");
    let mut out = Vec::with_capacity(n);
    for line in s.lines() {
        if out.len() >= n { break; }
        let v: serde_json::Value = serde_json::from_str(line).expect("json");
        if v["t"] != "e" { continue; }
        let g = |k: &str| v[k].as_str().unwrap_or("").to_string();
        let id: String = Sha256::digest(line.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        out.push(Ev { id, occurred_at: g("occurred_at"), source: g("source"), kind: g("kind"), text: g("text"), tags: g("tags") });
    }
    out
}

pub fn block(evs: &[Ev]) -> DataBlock {
    let s = |f: &dyn Fn(&Ev) -> String| -> Vec<Option<String>> { evs.iter().map(|e| Some(f(e))).collect() };
    let rec = evs.last().map(|e| e.occurred_at.clone()).unwrap_or_default();
    DataBlock::new(vec![
        Column::str_col("event_id", s(&|e| e.id.clone())),
        Column::str_col("occurred_at", s(&|e| e.occurred_at.clone())),
        Column::str_col("recorded_at", s(&|_| rec.clone())),
        Column::str_col("source", s(&|e| e.source.clone())),
        Column::str_col("kind", s(&|e| e.kind.clone())),
        Column::str_col("text", s(&|e| e.text.clone())),
        Column::str_col("tags", s(&|e| e.tags.clone())),
        Column::bool_col("redacted", evs.iter().map(|_| Some(false)).collect()),
    ]).unwrap()
}

pub fn dict(evs: &[Ev], max_bytes: usize) -> SharedDict {
    let mut picked: Vec<&str> = vec![];
    let mut size = 0;
    for e in evs.iter().rev() {
        if size + e.text.len() + 1 > max_bytes { break; }
        size += e.text.len() + 1;
        picked.push(&e.text);
    }
    let mut c = vec![];
    for t in picked.iter().rev() { if !c.is_empty() { c.push(0xFF); } c.extend_from_slice(t.as_bytes()); }
    SharedDict::from_content(&c).unwrap()
}
