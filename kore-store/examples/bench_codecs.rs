//! Size and time of the legacy (v2) writer vs the opt-in v3 codecs on a realistic log shape.
//!
//! Data (seeded xorshift, seed 0x4B4F5245 unless given): `id` 64 lowercase hex chars (random, like a
//! SHA-256), `ts` i64 ms with small random gaps, `tag` / `source` / `kind` low-cardinality strings, `text`
//! 40..5000 bytes (log-uniform length) built from a pool of recurring sentences plus occasional unique
//! numbers/tokens.
//!
//! Usage: bench_codecs <rows> <reps> [seed_hex]
//! Variants are interleaved inside every repetition (v2, v3-auto, v3-lz4, v3-zstd, then reads), so slow
//! drift in machine load hits all of them alike. Times are in-memory (to_bytes / from_bytes), not disk.

use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{ColumnCodec, KoreReader, KoreWriter, Stage, Transform, WriteOptions};
use std::time::Instant;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 { let mut x = self.0; x ^= x << 13; x ^= x >> 7; x ^= x << 17; self.0 = x; x }
    fn below(&mut self, n: u64) -> u64 { self.next() % n }
}

const WORDS: &[&str] = &["the", "request", "failed", "with", "status", "retrying", "after", "timeout", "user", "opened", "settings",
    "index", "sync", "completed", "connection", "reset", "by", "peer", "worker", "queue", "backlog", "cache", "miss", "hit",
    "session", "token", "expired", "refreshing", "schema", "migration", "applied", "to", "table", "events", "batch", "flush"];

pub fn generate(n: usize, seed: u64) -> DataBlock {
    let mut rng = Rng(seed | 1);
    // recurring sentence pool
    let pool: Vec<String> = (0..300).map(|_| {
        let len = 4 + rng.below(12);
        let mut s = String::new();
        for k in 0..len { if k > 0 { s.push(' '); } s.push_str(WORDS[rng.below(WORDS.len() as u64) as usize]); }
        s.push('.');
        s
    }).collect();
    let tags: Vec<String> = (0..30).map(|i| format!("tag-{i}")).collect();
    let sources: Vec<&str> = vec!["cli", "daemon", "api", "scheduler", "ui", "importer", "agent", "hook"];
    let kinds: Vec<&str> = vec!["message", "tool_call", "tool_result", "error", "note", "decision", "metric", "heartbeat", "summary", "plan", "reply", "trace"];

    let mut id = Vec::with_capacity(n);
    let mut ts = Vec::with_capacity(n);
    let mut tag = Vec::with_capacity(n);
    let mut source = Vec::with_capacity(n);
    let mut kind = Vec::with_capacity(n);
    let mut text = Vec::with_capacity(n);
    let mut t = 1_700_000_000_000i64;
    for _ in 0..n {
        id.push(Some(format!("{:016x}{:016x}{:016x}{:016x}", rng.next(), rng.next(), rng.next(), rng.next())));
        t += 1 + rng.below(2000) as i64;
        ts.push(Some(t));
        // skewed pick: low indices more likely
        let z = |r: &mut Rng, m: usize| { let a = r.below(m as u64); let b = r.below(m as u64); a.min(b) as usize };
        tag.push(Some(tags[z(&mut rng, tags.len())].clone()));
        source.push(Some(sources[z(&mut rng, sources.len())].to_string()));
        kind.push(Some(kinds[z(&mut rng, kinds.len())].to_string()));
        // log-uniform length in [40, 5000]
        let u = (rng.next() >> 11) as f64 / (1u64 << 53) as f64;
        let target = (40.0 * (5000.0f64 / 40.0).powf(u)) as usize;
        let mut s = String::with_capacity(target + 64);
        while s.len() < target {
            let sent = &pool[z(&mut rng, pool.len())];
            s.push_str(sent);
            if rng.below(4) == 0 { s.push_str(&format!(" [{}]", rng.below(1_000_000))); }
            s.push(' ');
        }
        s.truncate(target);
        text.push(Some(s));
    }
    let col = |name: &str, data| Column { name: name.into(), data };
    DataBlock { num_rows: n, columns: vec![
        col("id", ColumnData::Str(id)), col("ts", ColumnData::Int64(ts)), col("tag", ColumnData::Str(tag)),
        col("source", ColumnData::Str(source)), col("kind", ColumnData::Str(kind)), col("text", ColumnData::Str(text)),
    ] }
}

fn median(v: &mut Vec<f64>) -> f64 { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[v.len() / 2] }

fn main() {
    // the legacy writer appends a short text preview by default; switch it off so sizes are comparable
    std::env::set_var("KORE_READABLE_MODE", "none");
    let args: Vec<String> = std::env::args().collect();
    let n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(100_000);
    let reps: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let seed = args.get(3).and_then(|s| u64::from_str_radix(s, 16).ok()).unwrap_or(0x4B4F5245);
    let t0 = Instant::now();
    let block = generate(n, seed);
    let raw_text: usize = match &block.columns[5].data { ColumnData::Str(v) => v.iter().flatten().map(|s| s.len()).sum(), _ => 0 };
    println!("# rows={n} reps={reps} seed={seed:#x} generated in {:.1}s, text bytes={raw_text} (avg {:.0})", t0.elapsed().as_secs_f64(), raw_text as f64 / n as f64);

    let v3_auto = WriteOptions::auto();
    let v3_lz4 = WriteOptions { default: ColumnCodec::new(Transform::Auto, Stage::Lz4), columns: vec![] };
    let v3_zstd = WriteOptions { default: ColumnCodec::new(Transform::Auto, Stage::Zstd), columns: vec![] };

    type W<'a> = Box<dyn Fn() -> Vec<u8> + 'a>;
    let writers: Vec<(&str, W)> = vec![
        ("v2 (legacy writer)", Box::new(|| KoreWriter::to_bytes(&block))),
        ("v3 auto (lz4|hz)", Box::new(|| KoreWriter::to_bytes_with(&block, &v3_auto).unwrap())),
        ("v3 transforms+lz4", Box::new(|| KoreWriter::to_bytes_with(&block, &v3_lz4).unwrap())),
        ("v3 transforms+zstd", Box::new(|| KoreWriter::to_bytes_with(&block, &v3_zstd).unwrap())),
    ];
    let k = writers.len();
    let mut wt: Vec<Vec<f64>> = vec![vec![]; k];
    let mut rt: Vec<Vec<f64>> = vec![vec![]; k];
    let mut sizes = vec![0usize; k];
    let mut files: Vec<Vec<u8>> = vec![vec![]; k];
    for rep in 0..reps {
        for (i, (_, w)) in writers.iter().enumerate() {
            let t = Instant::now();
            let b = w();
            wt[i].push(t.elapsed().as_secs_f64());
            sizes[i] = b.len();
            files[i] = b;
        }
        for i in 0..k {
            let t = Instant::now();
            let back = KoreReader::from_bytes(&files[i]).unwrap();
            rt[i].push(t.elapsed().as_secs_f64());
            if rep == 0 {
                assert_eq!(back.num_rows, block.num_rows);
                // logical equality (the v2 reader may hand back StrDict for low-cardinality strings)
                for (a, b) in back.columns.iter().zip(&block.columns) {
                    let strs = |c: &ColumnData| match c {
                        ColumnData::Str(v) => Some(v.clone()),
                        ColumnData::StrDict { codes, dict } => Some(codes.iter().map(|&x| if x == u8::MAX { None } else { Some(dict[x as usize].clone()) }).collect()),
                        _ => None,
                    };
                    match (strs(&a.data), strs(&b.data)) {
                        (Some(x), Some(y)) => assert!(x == y, "{} mismatch in {}", writers[i].0, a.name),
                        _ => assert!(matches!((&a.data, &b.data), (ColumnData::Int64(x), ColumnData::Int64(y)) if x == y)),
                    }
                }
            }
        }
        files.iter_mut().for_each(|f| *f = vec![]);
        // keep the last repetition's files only for sizes; re-create next round
        let _ = rep;
    }
    println!("| variant | size bytes | vs v2 | write median s (min-max) | read median s (min-max) |");
    println!("|---|---:|---:|---|---|");
    let base = sizes[0] as f64;
    for i in 0..k {
        let (mut w, mut r) = (wt[i].clone(), rt[i].clone());
        let (wmin, wmax) = (w.iter().cloned().fold(f64::MAX, f64::min), w.iter().cloned().fold(0.0, f64::max));
        let (rmin, rmax) = (r.iter().cloned().fold(f64::MAX, f64::min), r.iter().cloned().fold(0.0, f64::max));
        println!("| {} | {} | {:.1}% | {:.2} ({:.2}-{:.2}) | {:.2} ({:.2}-{:.2}) |", writers[i].0, sizes[i], 100.0 * sizes[i] as f64 / base,
                 median(&mut w), wmin, wmax, median(&mut r), rmin, rmax);
    }
    // per-column sizes: encode each column alone
    println!("\n| column | v2 bytes | v3 auto bytes | v3 auto codec (transform/stage) |");
    println!("|---|---:|---:|---|");
    for c in &block.columns {
        let one = DataBlock { num_rows: n, columns: vec![c.clone()] };
        let v2 = KoreWriter::to_bytes(&one).len();
        let (t, s, p) = kore_store::codec::encode_column(&c.data, ColumnCodec::AUTO).unwrap();
        println!("| {} | {} | {} | {:?}/{:?} |", c.name, v2, p.len(), Transform::from_id(t).unwrap(), Stage::from_id(s).unwrap());
    }
}
