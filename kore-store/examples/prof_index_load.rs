//! Key index build / load / candidates cost on synthetic sha256-like keys (no data files are read).
//!
//!   prof_index_load [--keys 1000000] [--files 1000] [--reps 5]

use kore_store::{KeyIndex, KeyIndexBuilder};
use std::time::Instant;

fn hex(k: u64) -> String {
    let mut x = k;
    let mut out = String::with_capacity(64);
    for _ in 0..4 {
        x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        out.push_str(&format!("{:016x}", z ^ (z >> 31)));
    }
    out
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let get = |k: &str, d: u64| a.iter().position(|x| x == k).map(|i| a[i + 1].parse().unwrap()).unwrap_or(d);
    let (n, files, reps) = (get("--keys", 1_000_000), get("--files", 1000), get("--reps", 5) as usize);
    let per = n / files;
    let keys: Vec<String> = (0..n).map(hex).collect();
    let t = Instant::now();
    let mut b = KeyIndexBuilder::new("event_id");
    for f in 0..files {
        let ks = &keys[(f * per) as usize..((f + 1) * per) as usize];
        b.add_file_keys(&format!("events/{f:06}.kore"), format!("file {f}").as_bytes(), ks.iter().map(|k| Some(k.as_str()))).unwrap();
    }
    let idx = b.build().unwrap();
    let build = t.elapsed().as_secs_f64();
    let bytes = idx.to_bytes().to_vec();
    let mut loads = vec![];
    for _ in 0..reps { let t = Instant::now(); let _ = KeyIndex::from_bytes(&bytes).unwrap(); loads.push(t.elapsed().as_secs_f64() * 1e3); }
    let t = Instant::now();
    let mut hit = 0;
    for k in keys.iter().step_by(997) { hit += idx.candidates(k).len(); }
    let cand = t.elapsed().as_secs_f64() / (n as f64 / 997.0) * 1e6;
    println!("{n} keys, {files} files: {} bytes ({:.2} B/key); build {build:.3}s (incl. one sha256 per file name stub); load+verify ms {loads:.1?}; candidates {cand:.2} us/op ({hit} hits)",
        bytes.len(), bytes.len() as f64 / n as f64);
}
