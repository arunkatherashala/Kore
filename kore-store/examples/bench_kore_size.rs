//! Store side of the kore_size benchmark (docs/kore_size.md).
//!
//!   bench_kore_size --log LOGDIR --work DIR --out RESULT.json [--reps 3] [--lookups 20] [--dict-bytes 131072]
//!
//! LOGDIR is a Digital Life versioned log written in log format 1 (every segment is a default kore-store
//! file, `KoreWriter::to_bytes`), e.g. from scripts/kore_size/dl_log_side.rs. For every segment this
//! program decodes it and writes it again
//!   * with the default writer (must reproduce the file on disk byte for byte; checked), and
//!   * with the opt-in version-4 writer (`to_bytes_compact`), the `text` column of `events` allowed to use
//!     one shared dictionary: the raw text of the first events segment (its last `--dict-bytes` bytes),
//!     stored once as a `.kdic` object.
//! The version-4 copy is written to WORK/v4 (same table directories and file names, the MANIFEST copied
//! as it is, the dictionary under `dicts/`). Then both directories are read back from disk:
//!   * full read: every segment of every table, all columns;
//!   * projection: the `kind` column of every events segment (count per kind);
//!   * point lookup: an event id found by scanning the `event_id` column of the events segments, then the
//!     `text` of that row (no index exists in the store).
//! Default and v4 are timed alternately inside each repetition (paired). Every v4 file is checked to
//! decode to the same values as its source in repetition 0.

use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{CompactOptions, DictSet, KoreReader, KoreWriter, SharedDict};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn dir_size(p: &Path) -> u64 {
    let mut t = 0;
    if let Ok(rd) = std::fs::read_dir(p) {
        for f in rd.flatten() {
            let m = f.metadata().unwrap();
            t += if m.is_dir() { dir_size(&f.path()) } else { m.len() };
        }
    }
    t
}

fn materialize(b: DataBlock) -> DataBlock {
    let cols = b.columns.into_iter().map(|c| match c.data {
        ColumnData::StrDict { codes, dict } => {
            let v = codes.iter().map(|&k| if k == 0 { None } else { Some(dict[k as usize - 1].clone()) }).collect();
            Column { name: c.name, data: ColumnData::Str(v) }
        }
        _ => c,
    }).collect();
    DataBlock { columns: cols, num_rows: b.num_rows }
}

fn strs(c: &ColumnData) -> Vec<Option<String>> {
    match c {
        ColumnData::Str(v) => v.clone(),
        ColumnData::StrDict { codes, dict } => codes.iter().map(|&k| if k == 0 { None } else { Some(dict[k as usize - 1].clone()) }).collect(),
        _ => panic!("not a string column"),
    }
}

fn same(a: &DataBlock, b: &DataBlock) -> bool {
    if a.num_rows != b.num_rows || a.columns.len() != b.columns.len() { return false; }
    a.columns.iter().zip(&b.columns).all(|(x, y)| {
        x.name == y.name && match (&x.data, &y.data) {
            (ColumnData::Int64(p), ColumnData::Int64(q)) => p == q,
            (ColumnData::Bool(p), ColumnData::Bool(q)) => p == q,
            (ColumnData::Float64(p), ColumnData::Float64(q)) => p.len() == q.len() && p.iter().zip(q).all(|(u, v)| u.map(f64::to_bits) == v.map(f64::to_bits)),
            _ => strs(&x.data) == strs(&y.data),
        }
    })
}

fn median(xs: &[f64]) -> f64 {
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

fn stats(xs: &[f64]) -> Value {
    json!({"median": median(xs), "min": xs.iter().cloned().fold(f64::INFINITY, f64::min), "max": xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max), "all": xs})
}

struct Seg { table: String, file: String, bytes: Vec<u8>, block: DataBlock }

fn main() {
    let v: Vec<String> = std::env::args().collect();
    let get = |k: &str| v.iter().position(|a| a == k).map(|i| v[i + 1].clone());
    let log = PathBuf::from(get("--log").expect("--log"));
    let work = PathBuf::from(get("--work").expect("--work"));
    let out = get("--out").expect("--out");
    let reps: usize = get("--reps").map(|s| s.parse().unwrap()).unwrap_or(3);
    let lookups: usize = get("--lookups").map(|s| s.parse().unwrap()).unwrap_or(20);
    let dict_bytes: usize = get("--dict-bytes").map(|s| s.parse().unwrap()).unwrap_or(128 << 10);
    let level: i32 = get("--zstd-level").map(|s| s.parse().unwrap()).unwrap_or(9);
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();

    // ---- load every segment
    let mut tables: Vec<String> = std::fs::read_dir(&log).unwrap().flatten().filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string()).collect();
    tables.sort();
    let mut segs: Vec<Seg> = vec![];
    for t in &tables {
        let mut files: Vec<String> = std::fs::read_dir(log.join(t)).unwrap().flatten()
            .map(|e| e.file_name().to_string_lossy().to_string()).filter(|f| f.ends_with(".kore")).collect();
        files.sort();
        for f in files {
            let bytes = std::fs::read(log.join(t).join(&f)).unwrap();
            let block = materialize(KoreReader::from_bytes(&bytes).unwrap());
            segs.push(Seg { table: t.clone(), file: f, bytes, block });
        }
    }
    let n_events: usize = segs.iter().filter(|s| s.table == "events").map(|s| s.block.num_rows).sum();
    eprintln!("[store] {} segments, {} events", segs.len(), n_events);

    // ---- the shared dictionary: tail of the first events segment's text
    let mk_dict = || -> SharedDict {
        let first = segs.iter().find(|s| s.table == "events").expect("events segment");
        let col = first.block.columns.iter().find(|c| c.name == "text").expect("text column");
        let mut content = Vec::new();
        for s in strs(&col.data).iter().flatten() { content.extend_from_slice(s.as_bytes()); }
        let start = content.len().saturating_sub(dict_bytes);
        SharedDict::from_content(&content[start..]).unwrap()
    };
    let opts_for = |d: &SharedDict, table: &str| -> CompactOptions {
        let o = CompactOptions::new().with_zstd_level(level);
        if table == "events" { o.with_dict("text", d.clone()) } else { o }
    };

    // ---- write: default vs v4, paired per repetition (in memory; the files are written once afterwards)
    let mut w_def = vec![];
    let mut w_v4 = vec![];
    let mut identical = 0usize;
    let mut v4_bytes: Vec<Vec<u8>> = vec![];
    let mut dict = mk_dict();
    for rep in 0..reps {
        let t = Instant::now();
        let mut eq = 0;
        for s in &segs { let b = KoreWriter::to_bytes(&s.block); if b == s.bytes { eq += 1; } }
        w_def.push(t.elapsed().as_secs_f64());
        identical = eq;
        let t = Instant::now();
        let d = mk_dict();
        let outv: Vec<Vec<u8>> = segs.iter().map(|s| KoreWriter::to_bytes_compact(&s.block, &opts_for(&d, &s.table)).unwrap()).collect();
        w_v4.push(t.elapsed().as_secs_f64());
        if rep == 0 {
            let ds = DictSet::new().with(d.clone());
            for (s, b) in segs.iter().zip(&outv) {
                let back = KoreReader::from_bytes_with_dicts(b, &ds).unwrap();
                assert!(same(&back, &s.block), "v4 round trip differs in {}/{}", s.table, s.file);
            }
            v4_bytes = outv;
            dict = d;
        } else {
            assert!(outv == v4_bytes, "v4 output not deterministic");
        }
        eprintln!("[store] write rep {rep}: default {:.3}s v4 {:.3}s", w_def[rep], w_v4[rep]);
    }

    // ---- lay out the v4 copy on disk
    let v4dir = work.join("v4");
    for t in &tables { std::fs::create_dir_all(v4dir.join(t)).unwrap(); }
    for (s, b) in segs.iter().zip(&v4_bytes) { std::fs::write(v4dir.join(&s.table).join(&s.file), b).unwrap(); }
    std::fs::create_dir_all(v4dir.join("dicts")).unwrap();
    std::fs::write(v4dir.join("dicts").join(dict.file_name()), dict.to_bytes()).unwrap();
    for e in std::fs::read_dir(&log).unwrap().flatten() {
        if e.path().is_file() { std::fs::copy(e.path(), v4dir.join(e.file_name())).unwrap(); }
    }

    // per table / per column sizes
    let mut by_table_def = BTreeMap::<String, u64>::new();
    let mut by_table_v4 = BTreeMap::<String, u64>::new();
    let mut col_def = BTreeMap::<String, u64>::new();
    let mut col_v4 = BTreeMap::<String, u64>::new();
    let mut choice = BTreeMap::<String, BTreeMap<String, u64>>::new();
    for (s, b) in segs.iter().zip(&v4_bytes) {
        *by_table_def.entry(s.table.clone()).or_default() += s.bytes.len() as u64;
        *by_table_v4.entry(s.table.clone()).or_default() += b.len() as u64;
        let info = kore_store::v4::info(b).unwrap();
        for (name, _, t, st, _, len) in info.columns {
            let key = format!("{}.{}", s.table, name);
            *col_v4.entry(key.clone()).or_default() += len as u64;
            *choice.entry(key).or_default().entry(format!("t{t}/s{st}")).or_default() += 1;
        }
        if s.table == "events" {
            // default-writer size of each column alone (approximate share; the file has a shared header)
            for c in &s.block.columns {
                let one = DataBlock { columns: vec![c.clone()], num_rows: s.block.num_rows };
                *col_def.entry(format!("events.{}", c.name)).or_default() += KoreWriter::to_bytes(&one).len() as u64;
            }
        }
    }

    // ---- reads from disk
    let seg_paths = |root: &Path, table: &str| -> Vec<PathBuf> {
        segs.iter().filter(|s| s.table == table).map(|s| root.join(&s.table).join(&s.file)).collect()
    };
    let all_paths = |root: &Path| -> Vec<PathBuf> { segs.iter().map(|s| root.join(&s.table).join(&s.file)).collect() };
    let ev_def = seg_paths(&log, "events");
    let ev_v4 = seg_paths(&v4dir, "events");
    let (all_def, all_v4) = (all_paths(&log), all_paths(&v4dir));
    let ids_all: Vec<String> = segs.iter().filter(|s| s.table == "events").flat_map(|s| strs(&s.block.columns[0].data).into_iter().map(|x| x.unwrap())).collect();
    assert_eq!(segs.iter().find(|s| s.table == "events").unwrap().block.columns[0].name, "event_id");
    let texts_all: Vec<String> = segs.iter().filter(|s| s.table == "events")
        .flat_map(|s| strs(&s.block.columns.iter().find(|c| c.name == "text").unwrap().data).into_iter().map(|x| x.unwrap())).collect();
    let lk: Vec<usize> = (0..lookups).map(|k| ((k as u64 * 2654435761 + 12345) % n_events as u64) as usize).collect();
    let want_lk: usize = lk.iter().map(|&i| texts_all[i].len()).sum();

    let full = |paths: &[PathBuf], dicts: Option<&Path>| -> usize {
        let ds = dicts.map(|d| DictSet::load_dir(d).unwrap()).unwrap_or_default();
        paths.iter().map(|p| { let b = std::fs::read(p).unwrap(); KoreReader::from_bytes_with_dicts(&b, &ds).unwrap().num_rows }).sum()
    };
    let proj = |paths: &[PathBuf]| -> BTreeMap<String, usize> {
        let mut m = BTreeMap::new();
        for p in paths {
            let b = KoreReader::read_file_columns(p, &["kind"]).unwrap();
            for s in strs(&b.columns[0].data).into_iter().flatten() { *m.entry(s).or_default() += 1; }
        }
        m
    };
    let lookup = |paths: &[PathBuf], dicts: Option<&Path>, id: &str| -> usize {
        let ds = dicts.map(|d| DictSet::load_dir(d).unwrap()).unwrap_or_default();
        for p in paths {
            let bytes = std::fs::read(p).unwrap();
            let b = KoreReader::from_bytes_columns_with_dicts(&bytes, &["event_id"], &ds).unwrap();
            if let Some(r) = strs(&b.columns[0].data).iter().position(|x| x.as_deref() == Some(id)) {
                let t = KoreReader::from_bytes_columns_with_dicts(&bytes, &["text"], &ds).unwrap();
                return strs(&t.columns[0].data)[r].as_ref().unwrap().len();
            }
        }
        panic!("id not found");
    };
    let dicts_dir = v4dir.join("dicts");
    let (mut f_def, mut f_v4, mut p_def, mut p_v4, mut l_def, mut l_v4) = (vec![], vec![], vec![], vec![], vec![], vec![]);
    // warm-up (also checks equal results)
    assert_eq!(full(&all_def, None), full(&all_v4, Some(&dicts_dir)));
    let pr = proj(&ev_def);
    assert_eq!(pr, proj(&ev_v4));
    for rep in 0..reps {
        let t = Instant::now(); full(&all_def, None); f_def.push(t.elapsed().as_secs_f64());
        let t = Instant::now(); full(&all_v4, Some(&dicts_dir)); f_v4.push(t.elapsed().as_secs_f64());
        let t = Instant::now(); proj(&ev_def); p_def.push(t.elapsed().as_secs_f64());
        let t = Instant::now(); proj(&ev_v4); p_v4.push(t.elapsed().as_secs_f64());
        let t = Instant::now(); let a: usize = lk.iter().map(|&i| lookup(&ev_def, None, &ids_all[i])).sum(); l_def.push(t.elapsed().as_secs_f64() / lookups as f64);
        let t = Instant::now(); let b: usize = lk.iter().map(|&i| lookup(&ev_v4, Some(&dicts_dir), &ids_all[i])).sum(); l_v4.push(t.elapsed().as_secs_f64() / lookups as f64);
        assert_eq!(a, want_lk);
        assert_eq!(b, want_lk);
        eprintln!("[store] read rep {rep}: full {:.3}/{:.3} proj {:.3}/{:.3} lookup {:.4}/{:.4}", f_def[rep], f_v4[rep], p_def[rep], p_v4[rep], l_def[rep], l_v4[rep]);
    }
    let ratio = |a: &[f64], b: &[f64]| -> Value { stats(&a.iter().zip(b).map(|(x, y)| y / x).collect::<Vec<_>>()) };

    let res = json!({
        "n_events": n_events, "segments": segs.len(), "reps": reps, "lookups": lookups, "dict_bytes_requested": dict_bytes, "zstd_level": level,
        "default_rewrite_identical_files": identical,
        "default": {"size_bytes": dir_size(&log), "by_table": by_table_def, "events_columns_alone": col_def,
            "write_s": stats(&w_def), "full_read_s": stats(&f_def), "projection_kind_s": stats(&p_def), "lookup_per_op_s": stats(&l_def)},
        "v4": {"size_bytes": dir_size(&v4dir), "by_table": by_table_v4, "columns": col_v4, "codec_choice": choice,
            "dict_id": dict.id_hex(), "dict_object_bytes": dict.to_bytes().len(),
            "write_s": stats(&w_v4), "full_read_s": stats(&f_v4), "projection_kind_s": stats(&p_v4), "lookup_per_op_s": stats(&l_v4)},
        "paired_ratio_v4_over_default": {"write": ratio(&w_def, &w_v4), "full_read": ratio(&f_def, &f_v4),
            "projection": ratio(&p_def, &p_v4), "lookup": ratio(&l_def, &l_v4)},
        "projection_result": pr, "lookup_check": want_lk,
    });
    std::fs::write(&out, serde_json::to_string_pretty(&res).unwrap()).unwrap();
    eprintln!("[store] default {} bytes, v4 {} bytes", res["default"]["size_bytes"], res["v4"]["size_bytes"]);
}
