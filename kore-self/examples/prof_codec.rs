//! Timer profile of the format-2 encoder/decoder on the benchmark events.
//!   prof_codec DATA.jsonl N [REPS]
//! Reports per-phase seconds (encode_segment total, hz alone on the text/id/tags bodies, decode_segment).
#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;

use serde_json::Value;
use std::time::Instant;
use versioned::codec;
use versioned::hz;
use versioned::*;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() > 4 { micro(); return; }
    let n: usize = a[2].parse().unwrap();
    let reps: usize = a.get(3).map(|s| s.parse().unwrap()).unwrap_or(3);
    let text = std::fs::read_to_string(&a[1]).unwrap();
    let mut rows: Vec<Vec<Cell>> = vec![];
    for line in text.lines() {
        let o: Value = serde_json::from_str(line).unwrap();
        if o["t"] != "e" {
            continue;
        }
        let s = |k: &str| o[k].as_str().unwrap().to_string();
        let occ = normalize_ts(&s("occurred_at")).unwrap();
        rows.push(
            EventRow { event_id: event_id(&s("source"), &occ, &s("text")), occurred_at: occ, recorded_at: "2026-03-01T00:00:00Z".into(), source: s("source"), kind: s("kind"), text: s("text"), tags: s("tags"), redacted: false }
                .to_cells(),
        );
        if rows.len() == n {
            break;
        }
    }
    let chunks: Vec<&[Vec<Cell>]> = rows.chunks(1000).collect();
    let raw: usize = rows.iter().map(|r| r.iter().map(|c| if let Cell::S(s) = c { s.len() } else { 1 }).sum::<usize>()).sum();
    println!("rows {} chunks {} raw bytes {}", rows.len(), chunks.len(), raw);
    let mut best = |name: &str, f: &mut dyn FnMut() -> usize| {
        let mut ts = vec![];
        let mut out = 0;
        for _ in 0..reps {
            let t = Instant::now();
            out = f();
            ts.push(t.elapsed().as_secs_f64());
        }
        ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("{name:<28} median {:.3}s min {:.3}s  out {}", ts[ts.len() / 2], ts[0], out);
    };
    let mut segs: Vec<Vec<u8>> = vec![];
    best("encode_segment (all cols)", &mut || {
        segs = chunks.iter().map(|c| codec::encode_segment(Table::Events, c)).collect();
        segs.iter().map(|s| s.len()).sum()
    });
    best("decode_segment (all cols)", &mut || chunks.iter().zip(&segs).map(|(_, s)| codec::decode_segment(Table::Events, s).unwrap().len()).sum());
    best("decode ids only", &mut || segs.iter().map(|s| codec::decode_first_col_ids(Table::Events, s).unwrap().len()).sum());
    // hz alone over the raw text bodies (length varints + bytes, as enc_raw would build them)
    let bodies: Vec<Vec<u8>> = chunks
        .iter()
        .map(|c| {
            let mut o = vec![];
            for r in c.iter() {
                if let Cell::S(s) = &r[5] {
                    o.extend_from_slice(s.as_bytes());
                }
            }
            o
        })
        .collect();
    let tot: usize = bodies.iter().map(|b| b.len()).sum();
    let mut z: Vec<Vec<u8>> = vec![];
    best("hz::compress text bodies", &mut || {
        z = bodies.iter().map(|b| hz::compress(b)).collect();
        z.iter().map(|b| b.len()).sum()
    });
    println!("  (text bodies raw {tot} bytes)");
    best("hz::decompress text bodies", &mut || z.iter().map(|b| hz::decompress(b).unwrap().len()).sum());
    let ids: Vec<Vec<u8>> = chunks.iter().map(|c| c.iter().flat_map(|r| if let Cell::S(s) = &r[0] { codec::hex64_to_bytes(s).unwrap().to_vec() } else { vec![] }).collect()).collect();
    best("hz::compress id bodies", &mut || ids.iter().map(|b| hz::compress(b).len()).sum());
}

#[allow(dead_code)]
pub fn micro() {
    let mut x = 88172645463325252u64;
    let mut rnd = |n: usize| -> Vec<u8> {
        (0..n).map(|_| { x ^= x << 13; x ^= x >> 7; x ^= x << 17; x as u8 }).collect()
    };
    let big = rnd(3_200_000);
    for (name, f) in [("random 1x3.2MB", 1usize), ("random 100x32KB", 100usize)] {
        let t = Instant::now();
        let mut out = 0;
        for c in big.chunks(big.len() / f) {
            out += hz::compress(c).len();
        }
        println!("{name}: {:.3}s out {out}", t.elapsed().as_secs_f64());
    }
}
