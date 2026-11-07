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
use kore_store::{CompactOptions, DictSet, KeyIndex, KeyIndexBuilder, KoreReader, KoreWriter, SharedDict, Verify};
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

    // ---- optional: dump the logical rows (JSON arrays, version = segment number first) for the other formats
    if let Some(dump) = get("--dump") {
        use std::io::Write;
        let dump = PathBuf::from(dump);
        std::fs::create_dir_all(&dump).unwrap();
        let mut header = serde_json::Map::new();
        for t in &tables {
            let mut w = std::io::BufWriter::new(std::fs::File::create(dump.join(format!("{t}.jsonl"))).unwrap());
            for s in segs.iter().filter(|s| &s.table == t) {
                let version: u64 = s.file.trim_end_matches(".kore").parse().unwrap();
                header.insert(t.clone(), json!(s.block.columns.iter().map(|c| (c.name.clone(), format!("{:?}", kore_store::codec::dtype_of(&c.data)))).collect::<Vec<_>>()));
                for r in 0..s.block.num_rows {
                    let mut row = vec![json!(version)];
                    for c in &s.block.columns {
                        row.push(match &c.data {
                            ColumnData::Int64(v) => json!(v[r]),
                            ColumnData::Float64(v) => json!(v[r]),
                            ColumnData::Bool(v) => json!(v[r]),
                            ColumnData::Str(v) => json!(v[r]),
                            _ => unreachable!("materialized"),
                        });
                    }
                    serde_json::to_writer(&mut w, &row).unwrap();
                    w.write_all(b"
").unwrap();
                }
            }
        }
        std::fs::write(dump.join("columns.json"), serde_json::to_string_pretty(&header).unwrap()).unwrap();
        eprintln!("[store] dumped rows to {}", dump.display());
    }

    // ---- write: default vs v4, paired per repetition: encode + write every segment file (to the OS cache;
    // no fsync), into fresh directories
    let wdir = |name: &str, rep: usize| -> PathBuf {
        let d = work.join(format!("w_{name}_{rep}"));
        for t in &tables { std::fs::create_dir_all(d.join(t)).unwrap(); }
        d
    };
    let mut w_def = vec![];
    let mut w_v4 = vec![];
    let (mut w_v4_ref, mut w_v4_batch, mut w_v4_fast, mut w_def_batch) = (vec![], vec![], vec![], vec![]);
    let mut fast_bytes = 0u64;
    let mut identical = 0usize;
    let mut v4_bytes: Vec<Vec<u8>> = vec![];
    let mut dict = mk_dict();
    for rep in 0..reps {
        let (dd, dv) = (wdir("def", rep), wdir("v4", rep));
        std::fs::create_dir_all(dv.join("dicts")).unwrap();
        let t = Instant::now();
        let mut eq = 0;
        for s in &segs {
            let b = KoreWriter::to_bytes(&s.block);
            std::fs::write(dd.join(&s.table).join(&s.file), &b).unwrap();
            if b == s.bytes { eq += 1; }
        }
        w_def.push(t.elapsed().as_secs_f64());
        identical = eq;
        // v4 with the current writer and with the bbbefd2c reference writer (v4::write_reference), in
        // alternating order per repetition; both must give the same bytes
        let run_v4 = |reference: bool, dv: &Path| -> (f64, Vec<Vec<u8>>, SharedDict) {
            let t = Instant::now();
            let d = mk_dict();
            std::fs::write(dv.join("dicts").join(d.file_name()), d.to_bytes()).unwrap();
            let outv: Vec<Vec<u8>> = segs.iter().map(|s| {
                let o = opts_for(&d, &s.table);
                let b = if reference { kore_store::v4::write_reference(&s.block, &o).unwrap() } else { KoreWriter::to_bytes_compact(&s.block, &o).unwrap() };
                std::fs::write(dv.join(&s.table).join(&s.file), &b).unwrap();
                b
            }).collect();
            (t.elapsed().as_secs_f64(), outv, d)
        };
        let dr = wdir("v4ref", rep);
        std::fs::create_dir_all(dr.join("dicts")).unwrap();
        let ((tv, outv, d), (tr, outr, _)) = if rep % 2 == 0 {
            let a = run_v4(false, &dv); let b = run_v4(true, &dr); (a, b)
        } else {
            let b = run_v4(true, &dr); let a = run_v4(false, &dv); (a, b)
        };
        assert!(outv == outr, "v4 writer differs from the bbbefd2c reference writer");
        w_v4.push(tv);
        w_v4_ref.push(tr);
        // batch API: all segments encoded concurrently, then written (same bytes)
        let db = wdir("v4batch", rep);
        std::fs::create_dir_all(db.join("dicts")).unwrap();
        let t = Instant::now();
        let d2 = mk_dict();
        std::fs::write(db.join("dicts").join(d2.file_name()), d2.to_bytes()).unwrap();
        let opts: Vec<CompactOptions> = segs.iter().map(|s| opts_for(&d2, &s.table)).collect();
        let items: Vec<(&DataBlock, &CompactOptions)> = segs.iter().zip(&opts).map(|(s, o)| (&s.block, o)).collect();
        let outb = KoreWriter::to_bytes_compact_many(&items).unwrap();
        for (s, b) in segs.iter().zip(&outb) { std::fs::write(db.join(&s.table).join(&s.file), b).unwrap(); }
        w_v4_batch.push(t.elapsed().as_secs_f64());
        assert!(outb == outv, "batch writer output differs");
        // explicit fast preset (different bytes): one timed write per repetition, size from rep 0
        let df = wdir("v4fast", rep);
        std::fs::create_dir_all(df.join("dicts")).unwrap();
        let t = Instant::now();
        let d3 = mk_dict();
        std::fs::write(df.join("dicts").join(d3.file_name()), d3.to_bytes()).unwrap();
        let outf: Vec<Vec<u8>> = segs.iter().map(|s| {
            let o = if s.table == "events" { CompactOptions::fast().with_zstd_level(level).with_dict("text", d3.clone()) } else { CompactOptions::fast().with_zstd_level(level) };
            let b = KoreWriter::to_bytes_compact(&s.block, &o).unwrap();
            std::fs::write(df.join(&s.table).join(&s.file), &b).unwrap();
            b
        }).collect();
        w_v4_fast.push(t.elapsed().as_secs_f64());
        if rep == 0 {
            let ds = DictSet::new().with(d3.clone());
            for (s, b) in segs.iter().zip(&outf) { assert!(same(&KoreReader::from_bytes_with_dicts(b, &ds).unwrap(), &s.block)); }
            fast_bytes = outf.iter().map(|b| b.len() as u64).sum();
        }
        // default writer, segments encoded concurrently (the fair reference for the batch row)
        let dq = wdir("defbatch", rep);
        let t = Instant::now();
        let outd: Vec<Vec<u8>> = { use rayon::prelude::*; segs.par_iter().map(|s| KoreWriter::to_bytes(&s.block)).collect() };
        for (s, b) in segs.iter().zip(&outd) { std::fs::write(dq.join(&s.table).join(&s.file), b).unwrap(); }
        w_def_batch.push(t.elapsed().as_secs_f64());
        for x in [&dd, &dv, &dr, &db, &df, &dq] { let _ = std::fs::remove_dir_all(x); }
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
        eprintln!("[store] write rep {rep}: default {:.3}s v4 {:.3}s v4-reference {:.3}s v4-batch {:.3}s v4-fast {:.3}s default-batch {:.3}s",
            w_def[rep], w_v4[rep], w_v4_ref[rep], w_v4_batch[rep], w_v4_fast[rep], w_def_batch[rep]);
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
    let ratio_f = |a: &[f64], b: &[f64]| -> Value { stats(&a.iter().zip(b).map(|(x, y)| y / x).collect::<Vec<_>>()) };
    // ---- key index (opt-in KIDX object over events.event_id): build, size, lookups
    let idx_dir = work.join("v4_index");
    let def_idx_dir = work.join("def_index");
    let build_index = |bytes_of: &dyn Fn(usize) -> Vec<u8>| -> KeyIndex {
        let mut b = KeyIndexBuilder::new("event_id");
        for (i, s) in segs.iter().enumerate().filter(|(_, s)| s.table == "events") {
            let ColumnData::Str(v) = &s.block.columns[0].data else { panic!("materialized") };
            b.add_file_keys(&format!("events/{}", s.file), &bytes_of(i), v.iter().map(|x| x.as_deref())).unwrap();
        }
        b.build().unwrap()
    };
    let mut i_build = vec![];
    let mut idx_v4 = None;
    for _ in 0..reps {
        let t = Instant::now();
        let ix = build_index(&|i| v4_bytes[i].clone());
        std::fs::create_dir_all(&idx_dir).unwrap();
        std::fs::write(idx_dir.join(ix.file_name()), ix.to_bytes()).unwrap();
        i_build.push(t.elapsed().as_secs_f64());
        idx_v4 = Some(ix);
    }
    let idx_v4 = idx_v4.unwrap();
    let idx_path = idx_dir.join(idx_v4.file_name());
    let idx_def = build_index(&|i| segs[i].bytes.clone());
    std::fs::create_dir_all(&def_idx_dir).unwrap();
    std::fs::write(def_idx_dir.join(idx_def.file_name()), idx_def.to_bytes()).unwrap();
    let ds_v4 = DictSet::load_dir(&dicts_dir).unwrap();
    let lookup_idx = |idx: &KeyIndex, root: &Path, ds: &DictSet, id: &str, verify: Verify, reads: &mut usize| -> usize {
        let hits = idx.lookup(id, &["text"], ds, verify, |f| { *reads += 1; std::fs::read(root.join(&f.name)).map_err(|e| kore_core::KoreError::InvalidArgument(e.to_string())) }).unwrap();
        hits.iter().map(|h| h.values.columns[0].data.get_str(0).unwrap().len()).sum()
    };
    let missing: Vec<String> = (0..lookups.max(1) * 5).map(|k| format!("{:016x}{:048x}", 0xfeed_f00d_u64.wrapping_mul(k as u64 + 1), k)).collect();
    let (mut li_v4, mut li_v4_len, mut li_def, mut li_open, mut li_miss, mut i_load) = (vec![], vec![], vec![], vec![], vec![], vec![]);
    let mut miss_reads = 0usize;
    // warm-up
    let mut r0 = 0;
    assert_eq!(lk.iter().map(|&i| lookup_idx(&idx_v4, &v4dir, &ds_v4, &ids_all[i], Verify::Sha256, &mut r0)).sum::<usize>(), want_lk);
    for rep in 0..reps {
        let mut reads = 0;
        let t = Instant::now(); let a: usize = lk.iter().map(|&i| lookup_idx(&idx_v4, &v4dir, &ds_v4, &ids_all[i], Verify::Sha256, &mut reads)).sum(); li_v4.push(t.elapsed().as_secs_f64() / lookups as f64);
        let t = Instant::now(); let b: usize = lk.iter().map(|&i| lookup_idx(&idx_v4, &v4dir, &ds_v4, &ids_all[i], Verify::Length, &mut reads)).sum(); li_v4_len.push(t.elapsed().as_secs_f64() / lookups as f64);
        let t = Instant::now(); let c: usize = lk.iter().map(|&i| lookup_idx(&idx_def, &log, &DictSet::new(), &ids_all[i], Verify::Sha256, &mut reads)).sum(); li_def.push(t.elapsed().as_secs_f64() / lookups as f64);
        // "open": every operation loads (reads + verifies) the index object and the dictionaries from disk
        let t = Instant::now(); let d: usize = lk.iter().map(|&i| {
            let ix = KeyIndex::read_file(&idx_path).unwrap();
            let ds = DictSet::load_dir(&dicts_dir).unwrap();
            lookup_idx(&ix, &v4dir, &ds, &ids_all[i], Verify::Sha256, &mut reads)
        }).sum(); li_open.push(t.elapsed().as_secs_f64() / lookups as f64);
        let mut mr = 0;
        let t = Instant::now(); let m: usize = missing.iter().map(|k| lookup_idx(&idx_v4, &v4dir, &ds_v4, k, Verify::Sha256, &mut mr)).sum(); li_miss.push(t.elapsed().as_secs_f64() / missing.len() as f64);
        miss_reads = mr;
        let t = Instant::now(); let _ = KeyIndex::read_file(&idx_path).unwrap(); i_load.push(t.elapsed().as_secs_f64());
        assert_eq!((a, b, c, d, m), (want_lk, want_lk, want_lk, want_lk, 0));
        eprintln!("[store] index rep {rep}: lookup v4 {:.1}us (len-only {:.1}us), default+index {:.1}us, open+lookup {:.1}us, missing {:.1}us ({mr} file reads / {}), index load {:.2}ms",
            li_v4[rep] * 1e6, li_v4_len[rep] * 1e6, li_def[rep] * 1e6, li_open[rep] * 1e6, li_miss[rep] * 1e6, missing.len(), i_load[rep] * 1e3);
    }
    let index_res = json!({
        "column": "events.event_id", "keys": idx_v4.num_keys(), "files": idx_v4.files().len(),
        "v4_index_bytes": idx_v4.to_bytes().len(), "default_index_bytes": idx_def.to_bytes().len(), "id": idx_v4.id_hex(),
        "build_s": stats(&i_build), "load_verify_s": stats(&i_load),
        "lookup_v4_sha256_per_op_s": stats(&li_v4), "lookup_v4_length_only_per_op_s": stats(&li_v4_len),
        "lookup_default_files_per_op_s": stats(&li_def), "lookup_open_index_each_op_s": stats(&li_open),
        "lookup_missing_per_op_s": stats(&li_miss), "missing_keys": missing.len(), "missing_file_reads": miss_reads,
        "paired_ratio_indexed_over_scan_v4": ratio_f(&l_v4, &li_v4),
    });
    let _ = std::fs::remove_dir_all(&def_idx_dir);

    // optional: the same v4 files at a higher zstd level (size and one timed write only; reads are not
    // repeated because the decoder work is the same kind)
    let mut max_level = json!(null);
    if let Some(ml) = get("--max-level") {
        let ml: i32 = ml.parse().unwrap();
        let dv = wdir("v4max", 0);
        std::fs::create_dir_all(dv.join("dicts")).unwrap();
        let t = Instant::now();
        let d = mk_dict();
        std::fs::write(dv.join("dicts").join(d.file_name()), d.to_bytes()).unwrap();
        for s in &segs {
            let o = if s.table == "events" { CompactOptions::new().with_zstd_level(ml).with_dict("text", d.clone()) } else { CompactOptions::new().with_zstd_level(ml) };
            let b = KoreWriter::to_bytes_compact(&s.block, &o).unwrap();
            std::fs::write(dv.join(&s.table).join(&s.file), &b).unwrap();
        }
        let w = t.elapsed().as_secs_f64();
        for e in std::fs::read_dir(&log).unwrap().flatten() {
            if e.path().is_file() { std::fs::copy(e.path(), dv.join(e.file_name())).unwrap(); }
        }
        // spot check: every file decodes
        let ds = DictSet::load_dir(&dv.join("dicts")).unwrap();
        for s in &segs { let b = std::fs::read(dv.join(&s.table).join(&s.file)).unwrap(); assert!(same(&KoreReader::from_bytes_with_dicts(&b, &ds).unwrap(), &s.block)); }
        max_level = json!({"zstd_level": ml, "size_bytes": dir_size(&dv), "write_s": w});
        eprintln!("[store] v4 at level {ml}: {} bytes, write {w:.2}s", max_level["size_bytes"]);
        let _ = std::fs::remove_dir_all(&dv);
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
        "v4_max_level": max_level,
        "v4_reference_writer": {"write_s": stats(&w_v4_ref), "paired_ratio_v4_over_reference": ratio(&w_v4_ref, &w_v4)},
        "v4_batch_writer": {"write_s": stats(&w_v4_batch), "paired_ratio_over_default": ratio(&w_def, &w_v4_batch), "paired_ratio_over_reference": ratio(&w_v4_ref, &w_v4_batch)},
        "default_batch_writer": {"write_s": stats(&w_def_batch), "paired_ratio_v4_batch_over_default_batch": ratio(&w_def_batch, &w_v4_batch)},
        "v4_fast": {"size_bytes": dir_size(&v4dir) - v4_bytes.iter().map(|b| b.len() as u64).sum::<u64>() + fast_bytes,
            "write_s": stats(&w_v4_fast), "paired_ratio_over_default": ratio(&w_def, &w_v4_fast), "paired_ratio_over_reference": ratio(&w_v4_ref, &w_v4_fast)},
        "key_index": index_res,
        "projection_result": pr, "lookup_check": want_lk,
    });
    std::fs::write(&out, serde_json::to_string_pretty(&res).unwrap()).unwrap();
    eprintln!("[store] default {} bytes, v4 {} bytes", res["default"]["size_bytes"], res["v4"]["size_bytes"]);
}
