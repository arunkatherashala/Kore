//! Where the version-4 writer spends its time (single-threaded replay of `Auto` on every column of every
//! segment of a log-format-1 directory).
//!
//!   prof_v4_write --log LOGDIR [--dict-bytes 131072]
//!
//! For every column it times, sequentially: the candidate transforms, their zstd-1 scores, then each stage
//! the writer would try on the winning transform (lz4, zstd at 19/level, hz, zstd with the dictionary).
//! Totals per step are printed. This replays the writer's decisions; it does not call it.

use kore_core::{ColumnData, DataBlock};
use kore_store::{codec, KoreReader, Stage, Transform};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let v: Vec<String> = std::env::args().collect();
    let get = |k: &str| v.iter().position(|a| a == k).map(|i| v[i + 1].clone());
    let log = PathBuf::from(get("--log").expect("--log"));
    let dict_bytes: usize = get("--dict-bytes").map(|s| s.parse().unwrap()).unwrap_or(128 << 10);
    let mut segs: Vec<(String, DataBlock)> = vec![];
    let mut tables: Vec<_> = std::fs::read_dir(&log).unwrap().flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
    tables.sort();
    for t in &tables {
        let mut files: Vec<_> = std::fs::read_dir(log.join(t)).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |e| e == "kore")).collect();
        files.sort();
        for f in files { segs.push((t.clone(), KoreReader::read_file(&f).unwrap())); }
    }
    let strs = |c: &ColumnData| -> Vec<Option<String>> { match c {
        ColumnData::Str(v) => v.clone(),
        ColumnData::StrDict { codes, dict } => codes.iter().map(|&k| if k == 0 { None } else { Some(dict[k as usize - 1].clone()) }).collect(),
        _ => panic!() } };
    let segs: Vec<(String, DataBlock)> = segs.into_iter().map(|(t, b)| {
        let cols = b.columns.into_iter().map(|c| match &c.data { ColumnData::StrDict { .. } => kore_core::Column { name: c.name.clone(), data: ColumnData::Str(strs(&c.data)) }, _ => c }).collect();
        (t, DataBlock { columns: cols, num_rows: b.num_rows })
    }).collect();
    let first = segs.iter().find(|s| s.0 == "events").unwrap();
    let mut content = vec![];
    for s in strs(&first.1.columns.iter().find(|c| c.name == "text").unwrap().data).iter().flatten() { content.extend_from_slice(s.as_bytes()); }
    let dict = content[content.len().saturating_sub(dict_bytes)..].to_vec();
    let cd19 = zstd::dict::EncoderDictionary::copy(&dict, 19);
    let cd9 = zstd::dict::EncoderDictionary::copy(&dict, 9);

    let mut zc = zstd::bulk::Compressor::new(19).unwrap();
    let mut zbuf: Vec<u8> = Vec::with_capacity(4 << 20);
    let mut tot: BTreeMap<String, f64> = BTreeMap::new();
    let mut add = |k: String, t: Instant| { *tot.entry(k).or_default() += t.elapsed().as_secs_f64(); };
    let t_all = Instant::now();
    for (table, b) in &segs {
        for c in &b.columns {
            let key = format!("{table}.{}", c.name);
            let cands: Vec<Transform> = match &c.data {
                ColumnData::Int64(_) => vec![Transform::Plain, Transform::Delta, Transform::Varint],
                ColumnData::Str(_) => vec![Transform::Plain, Transform::Dict, Transform::HexFixed, Transform::TsText, Transform::Tokens],
                _ => vec![Transform::Plain],
            };
            let mut best: Option<(usize, Vec<u8>)> = None;
            for t in cands {
                let t0 = Instant::now();
                let raw = match codec::encode_transform(&c.data, t) { Ok(r) => r, Err(_) => continue };
                add(format!("1 transform {t:?}"), t0);
                let t0 = Instant::now();
                let sc = if raw.len() < 64 { raw.len() } else { raw.len().min(zstd::bulk::compress(&raw, 1).unwrap().len() + 4) };
                add("2 score zstd1".into(), t0);
                if best.as_ref().map_or(true, |b| sc < b.0) { best = Some((sc, raw)); }
            }
            let raw = best.unwrap().1;
            let has_dict = table == "events" && c.name == "text";
            let t0 = Instant::now(); let _ = codec::encode_stage(&raw, Stage::Lz4); add("3 lz4".into(), t0);
            if !has_dict || raw.len() < 256 << 10 {
                let lvl = if raw.len() < 256 << 10 { 19 } else { 9 };
                let t0 = Instant::now(); let _ = zstd::bulk::compress(&raw, lvl).unwrap(); add(format!("4 zstd{lvl} ({key})"), t0);
                let t0 = Instant::now(); let _ = zstd::bulk::compress(&raw, lvl).unwrap(); add(format!("4b zstd{lvl} again"), t0);
                let t0 = Instant::now(); let _ = zc.compress_to_buffer(&raw, &mut zbuf).ok(); add(format!("4c zstd{lvl} reused context (19 only)"), t0);
            }
            if raw.len() < 128 << 10 { let t0 = Instant::now(); let _ = codec::encode_stage(&raw, Stage::Hz); add(format!("5 hz ({key})"), t0); }
            if has_dict {
                let t0 = Instant::now();
                let big = raw.len() >= 256 << 10;
                let mut z = zstd::bulk::Compressor::with_prepared_dictionary(if big { &cd9 } else { &cd19 }).unwrap();
                let _ = z.compress(&raw).unwrap();
                add(format!("6 zstd+dict lvl {}", if big { 9 } else { 19 }), t0);
            }
        }
    }
    let replay = t_all.elapsed().as_secs_f64();
    let ds = kore_store::SharedDict::from_content(&dict).unwrap();
    let opts = |t: &str| { let o = kore_store::CompactOptions::new(); if t == "events" { o.with_dict("text", ds.clone()) } else { o } };
    let t0 = Instant::now();
    let seq: Vec<Vec<u8>> = segs.iter().map(|(t, b)| kore_store::KoreWriter::to_bytes_compact(b, &opts(t)).unwrap()).collect();
    let t_seq = t0.elapsed().as_secs_f64();
    use rayon::prelude::*;
    let t0 = Instant::now();
    let par: Vec<Vec<u8>> = segs.par_iter().map(|(t, b)| kore_store::KoreWriter::to_bytes_compact(b, &opts(t)).unwrap()).collect();
    let t_par = t0.elapsed().as_secs_f64();
    assert!(seq == par);
    let t0 = Instant::now();
    for (_, b) in &segs { let _ = kore_store::KoreWriter::to_bytes(b); }
    println!("writer: v4 one segment at a time {t_seq:.3}s, v4 segments in parallel {t_par:.3}s, default writer {:.3}s, total v4 bytes {}", t0.elapsed().as_secs_f64(), seq.iter().map(|x| x.len()).sum::<usize>());
    let _ = replay;
    println!("segments {} total {:.3}s", segs.len(), t_all.elapsed().as_secs_f64());
    let mut rows: Vec<_> = tot.into_iter().collect();
    rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    for (k, s) in rows { println!("{s:9.3}s  {k}"); }
}
