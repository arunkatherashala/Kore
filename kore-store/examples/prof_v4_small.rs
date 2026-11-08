//! Where a small version-4 write spends its time (realistic events blocks of 1, 10 and 1000 rows).
//!
//!   prof_v4_small --data rdata_10000.jsonl [--reps 50]
//!
//! For each block size: the whole `v4::write` (default options, with and without the shared
//! dictionary), then every column's candidate transforms and each stage the writer would try on the
//! raw bytes of each transform, timed one by one on the calling thread (median microseconds).

#[path = "support/realistic.rs"]
mod realistic;

use kore_store::{codec, v4, CompactOptions, Stage, Transform};
use std::time::Instant;

fn med(mut v: Vec<f64>) -> f64 { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[v.len() / 2] * 1e6 }

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let get = |k: &str| a.iter().position(|x| x == k).map(|i| a[i + 1].clone());
    let data = std::path::PathBuf::from(get("--data").expect("--data"));
    let reps: usize = get("--reps").map(|s| s.parse().unwrap()).unwrap_or(50);
    let evs = realistic::load(&data, 10_000);
    let d = realistic::dict(&evs[..1000], 128 << 10);
    for size in [1usize, 10, 1000] {
        let blocks: Vec<_> = (0..reps).map(|k| realistic::block(&evs[1000 + k * size % 8000..1000 + k * size % 8000 + size])).collect();
        for (label, o) in [("no dict", CompactOptions::new()), ("dict", CompactOptions::new().with_dict("text", d.clone()))] {
            let mut t = vec![];
            let mut bytes = 0;
            for b in &blocks { let s = Instant::now(); bytes += v4::write(b, &o).unwrap().len(); t.push(s.elapsed().as_secs_f64()); }
            println!("rows {size:5} {label:8}: v4::write median {:9.1} us, mean bytes {:.1}", med(t), bytes as f64 / reps as f64);
        }
        // replay per column
        let b0 = &blocks[0];
        for c in &b0.columns {
            let cands = [Transform::Plain, Transform::Delta, Transform::Varint, Transform::Dict, Transform::HexFixed, Transform::TsText, Transform::Tokens];
            for t in cands {
                let Ok(raw) = codec::encode_transform(&c.data, t) else { continue };
                let mut line = format!("  {:12} {:9} raw {:7}:", c.name, format!("{t:?}"), raw.len());
                let mut tm = |name: &str, f: &dyn Fn() -> usize| {
                    let mut v = vec![]; let mut sz = 0;
                    for _ in 0..reps.min(20) { let s = Instant::now(); sz = f(); v.push(s.elapsed().as_secs_f64()); }
                    line += &format!(" {name} {:.1}us/{}B", med(v), sz);
                };
                tm("z1", &|| zstd::bulk::compress(&raw, 1).unwrap().len());
                tm("lz4", &|| codec::encode_stage(&raw, Stage::Lz4).len());
                tm("hz", &|| codec::encode_stage(&raw, Stage::Hz).len());
                tm("z19", &|| zstd::bulk::compress(&raw, 19).unwrap().len());
                tm("z9", &|| zstd::bulk::compress(&raw, 9).unwrap().len());
                if c.name == "text" {
                    let dd = d.clone();
                    let o = CompactOptions::new().with_dict("text", dd);
                    let blk = kore_core::DataBlock::new(vec![c.clone()]).unwrap();
                    tm("w+dict", &|| v4::write(&blk, &o).unwrap().len());
                }
                println!("{line}");
            }
        }
    }
}
