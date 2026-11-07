//! Breakdown of an indexed point lookup (key index over events.event_id of version-4 segments).
//!
//!   prof_lookup --log LOGDIR [--n 200]
//!
//! Re-encodes the events segments of a log-format-1 directory as version 4 (text with a shared
//! dictionary, as in bench_kore_size), writes them to a temporary directory next to nothing else, builds
//! the key index and times each step of a lookup separately (median over --n random ids, microseconds).

use kore_core::ColumnData;
use kore_store::{CompactOptions, DictSet, KeyIndex, KeyIndexBuilder, KoreReader, KoreWriter, SharedDict, Verify};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Instant;

fn med(mut v: Vec<f64>) -> f64 { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[v.len() / 2] * 1e6 }

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let get = |k: &str| a.iter().position(|x| x == k).map(|i| a[i + 1].clone());
    let log = PathBuf::from(get("--log").expect("--log"));
    let n: usize = get("--n").map(|s| s.parse().unwrap()).unwrap_or(200);
    let mut files: Vec<PathBuf> = std::fs::read_dir(log.join("events")).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |e| e == "kore")).collect();
    files.sort();
    let blocks: Vec<_> = files.iter().map(|p| KoreReader::read_file(p).unwrap()).collect();
    let strs = |c: &ColumnData| -> Vec<Option<String>> { (0..c.len()).map(|r| c.get_str(r).map(|s| s.to_string())).collect() };
    let blocks: Vec<_> = blocks.into_iter().map(|b| kore_core::DataBlock { num_rows: b.num_rows, columns: b.columns.into_iter().map(|c| match c.data {
        ColumnData::StrDict { .. } => kore_core::Column { data: ColumnData::Str(strs(&c.data)), name: c.name }, _ => c }).collect() }).collect();
    let mut content = vec![];
    for s in strs(&blocks[0].columns.iter().find(|c| c.name == "text").unwrap().data).into_iter().flatten() { content.extend_from_slice(s.as_bytes()); }
    let d = SharedDict::from_content(&content[content.len().saturating_sub(128 << 10)..]).unwrap();
    let ds = DictSet::new().with(d.clone());
    let tmp = std::env::temp_dir().join(format!("prof_lookup_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let mut ib = KeyIndexBuilder::new("event_id");
    let mut ids = vec![];
    for (i, b) in blocks.iter().enumerate() {
        let bytes = KoreWriter::to_bytes_compact(b, &CompactOptions::new().with_dict("text", d.clone())).unwrap();
        let name = format!("{i:06}.kore");
        std::fs::write(tmp.join(&name), &bytes).unwrap();
        let k = strs(&b.columns[0].data);
        ib.add_file_keys(&name, &bytes, k.iter().map(|x| x.as_deref())).unwrap();
        ids.extend(k.into_iter().flatten());
    }
    let idx = ib.build().unwrap();
    println!("{} files, {} keys, index {} bytes", blocks.len(), idx.num_keys(), idx.to_bytes().len());
    let pick: Vec<&String> = (0..n).map(|k| &ids[(k * 7919 + 13) % ids.len()]).collect();
    let (mut t_cand, mut t_read, mut t_sha, mut t_key, mut t_text, mut t_both, mut t_total, mut t_lenonly) = (vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    for _ in 0..2 {
        for key in &pick {
            let t = Instant::now(); let c = idx.candidates(key); t_cand.push(t.elapsed().as_secs_f64());
            let f = &idx.files()[c[0]];
            let t = Instant::now(); let bytes = std::fs::read(tmp.join(&f.name)).unwrap(); t_read.push(t.elapsed().as_secs_f64());
            let t = Instant::now(); let _: [u8; 32] = Sha256::digest(&bytes).into(); t_sha.push(t.elapsed().as_secs_f64());
            let t = Instant::now(); let _ = KoreReader::from_bytes_columns_with_dicts(&bytes, &["event_id"], &ds).unwrap(); t_key.push(t.elapsed().as_secs_f64());
            let t = Instant::now(); let _ = KoreReader::from_bytes_columns_with_dicts(&bytes, &["text"], &ds).unwrap(); t_text.push(t.elapsed().as_secs_f64());
            let t = Instant::now(); let _ = KoreReader::from_bytes_columns_with_dicts(&bytes, &["event_id", "text"], &ds).unwrap(); t_both.push(t.elapsed().as_secs_f64());
            let rd = |f: &kore_store::keyindex::IndexedFile| std::fs::read(tmp.join(&f.name)).map_err(|e| kore_core::KoreError::InvalidArgument(e.to_string()));
            let t = Instant::now(); let h = idx.lookup(key, &["text"], &ds, Verify::Sha256, rd).unwrap(); t_total.push(t.elapsed().as_secs_f64());
            assert_eq!(h.len(), 1);
            let t = Instant::now(); let _ = idx.lookup(key, &["text"], &ds, Verify::Length, rd).unwrap(); t_lenonly.push(t.elapsed().as_secs_f64());
        }
    }
    let t = Instant::now(); let _ = KeyIndex::from_bytes(idx.to_bytes()).unwrap(); let load = t.elapsed().as_secs_f64();
    println!("median us: candidates {:.1} | read file {:.1} | sha256 {:.1} | decode event_id {:.1} | decode text {:.1} | decode both {:.1} | lookup total {:.1} (length-only {:.1}) | index from_bytes {:.0}",
        med(t_cand), med(t_read), med(t_sha), med(t_key), med(t_text), med(t_both), med(t_total), med(t_lenonly), load * 1e6);
    let _ = std::fs::remove_dir_all(&tmp);
}
