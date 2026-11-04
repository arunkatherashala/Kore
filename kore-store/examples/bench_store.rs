//! Store-level read/write benchmark that builds against every kore-store copy (it uses only the API the
//! old Digital Life copy has: `KoreWriter::to_bytes` / `write_file`, `KoreReader::from_bytes` /
//! `read_file`), so the same source can be timed against the old store, a980727d and this branch.
//!
//! Usage: bench_store <rows> <reps> <workdir> <out.json> [keep]   (keep: leave the written files in workdir)
//!
//! Data: `support/events_gen.rs` (Digital Life `events` shape). Two layouts of the same rows:
//!   seg   files of 1000 rows each (what kore-self log format 1 writes: one file per batch)
//!   one   a single file holding all rows
//! Operations, timed `reps` times each and reported as median/min/max milliseconds:
//!   write   encode all files in memory (to_bytes)
//!   open    read_file of every file from disk (OS cache warm after the first rep)
//!   scan    from_bytes of every file from memory (full decode)
//!   proj    decode only the `kind` column (old store: no projection API, so full decode + drop)
//!   lookup  20 (env BENCH_LOOKUPS) point lookups by event_id: find the file and row via the id column, then decode that file
//!   asof    (only where the store has a version log; `-` otherwise) read_at_version of the middle
//!           version of a 10-version KVLG log holding the `seg` files of the first 10k rows

use kore_core::DataBlock;
use kore_store::{KoreReader, KoreWriter};
use std::time::Instant;

#[path = "support/events_gen.rs"]
mod events_gen;

// The next two functions are the only places that differ between store copies; the copy built against
// the old store replaces them (see docs/fileformat_speed.md).
fn project(b: &[u8], cols: &[&str]) -> DataBlock { KoreReader::from_bytes_columns(b, cols).unwrap() } // NEWAPI-PROJECT
fn asof(seg_files: &[Vec<u8>]) -> Option<Box<dyn Fn() -> usize>> { asof_new(seg_files) } // NEWAPI-ASOF

#[allow(dead_code)]
fn asof_new(seg_files: &[Vec<u8>]) -> Option<Box<dyn Fn() -> usize>> {
    let mut log: Option<Vec<u8>> = None;
    for (i, f) in seg_files.iter().take(10).enumerate() {
        log = Some(kore_store::versioned::append_raw(log.as_deref(), f, 100 * (i as u64 + 1)).unwrap());
    }
    let log = log?;
    Some(Box::new(move || KoreReader::read_at_version(&log, 550).unwrap().num_rows))
}

fn stats(v: &[f64]) -> String {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    format!("{{\"median\": {:.4}, \"min\": {:.4}, \"max\": {:.4}, \"reps\": {}}}", s[s.len() / 2], s[0], s[s.len() - 1], s.len())
}

fn time(reps: usize, mut f: impl FnMut() -> usize) -> (Vec<f64>, usize) {
    let mut out = vec![];
    let mut chk = 0;
    for _ in 0..reps {
        let t = Instant::now();
        chk = std::hint::black_box(f());
        out.push(t.elapsed().as_secs_f64() * 1e3);
    }
    (out, chk)
}

fn str_at(b: &DataBlock, col: usize, row: usize) -> String {
    match b.columns[col].data.get_value(row) { kore_core::Value::Str(s) => s, v => format!("{v:?}") }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let rows: usize = a[0].parse().unwrap();
    let reps: usize = a[1].parse().unwrap();
    let work = std::path::PathBuf::from(&a[2]);
    let out = &a[3];
    std::fs::create_dir_all(&work).unwrap();

    let full = events_gen::events(rows, 7);
    let segs: Vec<DataBlock> = (0..rows).step_by(1000)
        .map(|s| full.select_rows(&(s..(s + 1000).min(rows)).collect::<Vec<_>>())).collect();
    let mut r = 0x9E37_79B9_7F4A_7C15u64;
    let lookups: usize = std::env::var("BENCH_LOOKUPS").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    let ids: Vec<String> = (0..lookups).map(|_| { r ^= r << 13; r ^= r >> 7; r ^= r << 17; str_at(&full, 0, (r % rows as u64) as usize) }).collect();

    let mut json = format!("{{\"rows\": {rows}, \"reps\": {reps}, \"lookups\": {lookups}");
    for (layout, blocks) in [("seg", &segs), ("one", &vec![full.clone()])] {
        let (w, _) = time(reps, || blocks.iter().map(|b| KoreWriter::to_bytes(b).len()).sum());
        let files: Vec<Vec<u8>> = blocks.iter().map(KoreWriter::to_bytes).collect();
        let size: usize = files.iter().map(|f| f.len()).sum();
        let dir = work.join(layout);
        std::fs::create_dir_all(&dir).unwrap();
        let paths: Vec<_> = blocks.iter().enumerate().map(|(i, b)| {
            let p = dir.join(format!("{i:06}.kore"));
            KoreWriter::write_file(&p, b).unwrap();
            p
        }).collect();
        let disk: u64 = paths.iter().map(|p| std::fs::metadata(p).unwrap().len()).sum();
        let (open, c1) = time(reps, || paths.iter().map(|p| KoreReader::read_file(p).unwrap().num_rows).sum());
        let (scan, c2) = time(reps, || files.iter().map(|f| KoreReader::from_bytes(f).unwrap().num_rows).sum());
        let (proj, c3) = time(reps, || files.iter().map(|f| project(f, &["kind"]).columns[0].data.len()).sum());
        assert_eq!((c1, c2, c3), (rows, rows, rows));
        let (look, c4) = time(reps, || {
            let mut found = 0;
            for id in &ids {
                for f in &files {
                    let idc = project(f, &["event_id"]);
                    if let Some(row) = (0..idc.num_rows).find(|&i| str_at(&idc, 0, i) == *id) {
                        let b = KoreReader::from_bytes(f).unwrap();
                        if str_at(&b, 0, row) == *id { found += 1; }
                        break;
                    }
                }
            }
            found
        });
        assert_eq!(c4, ids.len());
        json += &format!(", \"{layout}\": {{\"files\": {}, \"bytes\": {size}, \"disk_bytes\": {disk}, \"write_ms\": {}, \"open_ms\": {}, \"scan_ms\": {}, \"proj_ms\": {}, \"lookup_ms\": {}",
            files.len(), stats(&w), stats(&open), stats(&scan), stats(&proj), stats(&look));
        if layout == "seg" {
            match asof(&files) {
                Some(f) => { let (t, n) = time(reps, || f()); assert_eq!(n, 1000); json += &format!(", \"asof_ms\": {}", stats(&t)); }
                None => json += ", \"asof_ms\": null",
            }
        }
        json += "}";
        if a.get(4).map(String::as_str) != Some("keep") { let _ = std::fs::remove_dir_all(&dir); }
    }
    json += "}\n";
    std::fs::write(out, &json).unwrap();
    print!("{json}");
}
