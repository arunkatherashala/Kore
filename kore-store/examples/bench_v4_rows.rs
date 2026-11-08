//! Before/after timings of the version-4 row reader, the faster (same bytes) writer search, the explicit
//! `quick()` preset and the container-5 directory, on realistic Digital Life events (docs/kore_speed_v4.md
//! section 6).
//!
//!   bench_v4_rows --data rdata_N.jsonl --n N --work DIR --out results.json [--reps 3] [--lookups 200]
//!
//! Segments: 1000 events each (log format 4 shape, `support/realistic.rs`); `text` uses one shared
//! dictionary built from the first 1000 texts (128 KiB), as log format 4 does after its first commit.
//! Every measurement runs before and after in the same repetition, alternating which goes first;
//! ratios are after / before of the same repetition.
//!
//! * get one event (all 8 columns of one row): before = `KoreReader::from_bytes_with_dicts` (whole
//!   segment) then that row; after = `v4::read_row`. Both from bytes in memory ("decode") and from disk
//!   with sha256 like kore-self `get_event` ("read+sha256+decode").
//! * one column of one row (`text`): before = projection of `text`, then the row; after = `read_row`.
//! * write: before = `v4::write_reference` (the b44d2a91 search), after = `v4::write` (same bytes,
//!   checked for every block); `quick()` and `fast()` (other bytes); for 1-event, 10-event and
//!   1000-event blocks, one at a time.
//! * size: version 4 vs container version 5 (`with_compact_directory`), per 1-event segment and total.

#[path = "support/realistic.rs"]
mod realistic;

use kore_core::DataBlock;
use kore_store::{v4, CompactOptions, DictSet, KoreReader};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::Instant;

fn med(v: &mut [f64]) -> f64 { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); if v.is_empty() { 0.0 } else { v[v.len() / 2] } }

struct Rng(u64);
impl Rng { fn next(&mut self) -> u64 { let mut x = self.0; x ^= x << 13; x ^= x >> 7; x ^= x << 17; self.0 = x; x } }

/// (before, after) medians of one repetition, in seconds per operation.
type Pair = (f64, f64);

fn pair(rep: usize, mut before: impl FnMut() -> f64, mut after: impl FnMut() -> f64) -> Pair {
    if rep % 2 == 0 { let b = before(); let a = after(); (b, a) } else { let a = after(); let b = before(); (b, a) }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let get = |k: &str| a.iter().position(|x| x == k).map(|i| a[i + 1].clone());
    let data = std::path::PathBuf::from(get("--data").expect("--data"));
    let n: usize = get("--n").expect("--n").parse().unwrap();
    let work = std::path::PathBuf::from(get("--work").expect("--work"));
    let out = get("--out").expect("--out");
    let reps: usize = get("--reps").map(|s| s.parse().unwrap()).unwrap_or(3);
    let lookups: usize = get("--lookups").map(|s| s.parse().unwrap()).unwrap_or(200);
    std::fs::create_dir_all(&work).unwrap();

    let t = Instant::now();
    let evs = realistic::load(&data, n);
    assert_eq!(evs.len(), n);
    let d = realistic::dict(&evs[..1000.min(n)], 128 << 10);
    let ds = DictSet::new().with(d.clone());
    let blocks: Vec<DataBlock> = evs.chunks(1000).map(realistic::block).collect();
    let def = CompactOptions::new().with_dict("text", d.clone());
    let segs: Vec<Vec<u8>> = blocks.iter().map(|b| v4::write(b, &def).unwrap()).collect();
    for (i, s) in segs.iter().enumerate() { std::fs::write(work.join(format!("{i:06}.kore")), s).unwrap(); }
    eprintln!("loaded {n} events, {} segments in {:.1}s", segs.len(), t.elapsed().as_secs_f64());

    let mut res: BTreeMap<String, Vec<Pair>> = BTreeMap::new();
    let mut facts: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let picks: Vec<(usize, usize)> = (0..lookups).map(|_| { let k = (r.next() % n as u64) as usize; (k / 1000, k % 1000) }).collect();

    // correctness of the row reader on the benchmark data: every picked row equals the full decode
    for &(s, row) in &picks {
        let full = KoreReader::from_bytes_with_dicts(&segs[s], &ds).unwrap();
        let one = v4::read_row(&segs[s], None, row, &ds).unwrap();
        for (c, o) in full.columns.iter().zip(&one.columns) { assert_eq!(c.data.get_value(row), o.data.get_value(0)); }
    }

    for rep in 0..reps {
        // get one event, decode only
        let p = pair(rep, || {
            let mut v = vec![];
            for &(s, row) in &picks { let t = Instant::now(); let b = KoreReader::from_bytes_with_dicts(&segs[s], &ds).unwrap(); std::hint::black_box(b.columns.iter().map(|c| c.data.get_value(row)).count()); v.push(t.elapsed().as_secs_f64()); }
            med(&mut v)
        }, || {
            let mut v = vec![];
            for &(s, row) in &picks { let t = Instant::now(); let b = v4::read_row(&segs[s], None, row, &ds).unwrap(); std::hint::black_box(b.columns.iter().map(|c| c.data.get_value(0)).count()); v.push(t.elapsed().as_secs_f64()); }
            med(&mut v)
        });
        res.entry("get_event_decode".into()).or_default().push(p);
        // get one event from disk with sha256 (kore-self get_event shape)
        let p = pair(rep, || {
            let mut v = vec![];
            for &(s, row) in &picks { let t = Instant::now(); let bytes = std::fs::read(work.join(format!("{s:06}.kore"))).unwrap(); std::hint::black_box(Sha256::digest(&bytes)); let b = KoreReader::from_bytes_with_dicts(&bytes, &ds).unwrap(); std::hint::black_box(b.columns.iter().map(|c| c.data.get_value(row)).count()); v.push(t.elapsed().as_secs_f64()); }
            med(&mut v)
        }, || {
            let mut v = vec![];
            for &(s, row) in &picks { let t = Instant::now(); let bytes = std::fs::read(work.join(format!("{s:06}.kore"))).unwrap(); std::hint::black_box(Sha256::digest(&bytes)); let b = v4::read_row(&bytes, None, row, &ds).unwrap(); std::hint::black_box(b.columns.iter().map(|c| c.data.get_value(0)).count()); v.push(t.elapsed().as_secs_f64()); }
            med(&mut v)
        });
        res.entry("get_event_disk".into()).or_default().push(p);
        // one column of one row
        let p = pair(rep, || {
            let mut v = vec![];
            for &(s, row) in &picks { let t = Instant::now(); let b = KoreReader::from_bytes_columns_with_dicts(&segs[s], &["text"], &ds).unwrap(); std::hint::black_box(b.columns[0].data.get_value(row)); v.push(t.elapsed().as_secs_f64()); }
            med(&mut v)
        }, || {
            let mut v = vec![];
            for &(s, row) in &picks { let t = Instant::now(); let b = v4::read_row(&segs[s], Some(&["text"]), row, &ds).unwrap(); std::hint::black_box(b.columns[0].data.get_value(0)); v.push(t.elapsed().as_secs_f64()); }
            med(&mut v)
        });
        res.entry("one_column_one_row".into()).or_default().push(p);
        // one column (tags) for a 100-row range
        let p = pair(rep, || {
            let mut v = vec![];
            for &(s, row) in picks.iter().take(50) { let lo = row.min(900); let t = Instant::now(); let b = KoreReader::from_bytes_columns_with_dicts(&segs[s], &["tags"], &ds).unwrap(); std::hint::black_box((lo..lo + 100).map(|i| b.columns[0].data.get_value(i)).count()); v.push(t.elapsed().as_secs_f64()); }
            med(&mut v)
        }, || {
            let mut v = vec![];
            for &(s, row) in picks.iter().take(50) { let lo = row.min(900); let t = Instant::now(); let b = v4::read_rows(&segs[s], Some(&["tags"]), lo..lo + 100, &ds).unwrap(); std::hint::black_box((0..100).map(|i| b.columns[0].data.get_value(i)).count()); v.push(t.elapsed().as_secs_f64()); }
            med(&mut v)
        });
        res.entry("tags_100_rows".into()).or_default().push(p);

        // writes: 1-event, 10-event and 1000-event blocks; at most 200 blocks per size per repetition
        for size in [1usize, 10, 100, 1000] {
            // never the first 1000 events (the dictionary was built from them)
            let starts: Vec<usize> = if size == 1000 { (1..n / 1000).take(200).map(|k| k * 1000).collect() }
                else { (0..200).map(|k| 1000 + (k * size * 7) % (n - 1000 - size)).collect() };
            let bl: Vec<DataBlock> = starts.iter().map(|&s| realistic::block(&evs[s..s + size])).collect();
            for (lbl, o) in [("dict", def.clone()), ("nodict", CompactOptions::new())] {
                let p = pair(rep, || {
                    let mut v = vec![];
                    for b in &bl { let t = Instant::now(); std::hint::black_box(v4::write_reference(b, &o).unwrap()); v.push(t.elapsed().as_secs_f64()); }
                    med(&mut v)
                }, || {
                    let mut v = vec![];
                    for b in &bl { let t = Instant::now(); std::hint::black_box(v4::write(b, &o).unwrap()); v.push(t.elapsed().as_secs_f64()); }
                    med(&mut v)
                });
                res.entry(format!("write_{size}_{lbl}_default")).or_default().push(p);
                // where the (same) work runs: forced calling thread / forced rayon, against the reference
                for (tname, par) in [("here", false), ("rayon", true)] {
                    let p = pair(rep, || {
                        let mut v = vec![];
                        for b in &bl { let t = Instant::now(); std::hint::black_box(v4::write_reference(b, &o).unwrap()); v.push(t.elapsed().as_secs_f64()); }
                        med(&mut v)
                    }, || {
                        let mut v = vec![];
                        for b in &bl { let t = Instant::now(); std::hint::black_box(v4::write_threads(b, &o, par).unwrap()); v.push(t.elapsed().as_secs_f64()); }
                        med(&mut v)
                    });
                    res.entry(format!("write_{size}_{lbl}_{tname}")).or_default().push(p);
                }
                for (pname, po) in [("quick", CompactOptions::quick()), ("fast", CompactOptions::fast())] {
                    let po = if lbl == "dict" { po.with_dict("text", d.clone()) } else { po };
                    let p = pair(rep, || {
                        let mut v = vec![];
                        for b in &bl { let t = Instant::now(); std::hint::black_box(v4::write_reference(b, &o).unwrap()); v.push(t.elapsed().as_secs_f64()); }
                        med(&mut v)
                    }, || {
                        let mut v = vec![];
                        for b in &bl { let t = Instant::now(); std::hint::black_box(v4::write(b, &po).unwrap()); v.push(t.elapsed().as_secs_f64()); }
                        med(&mut v)
                    });
                    res.entry(format!("write_{size}_{lbl}_{pname}")).or_default().push(p);
                }
                if rep == 0 {
                    // sizes and byte identity (deterministic)
                    let mut sz: BTreeMap<&str, usize> = BTreeMap::new();
                    for b in &bl {
                        let refb = v4::write_reference(b, &o).unwrap();
                        assert_eq!(v4::write(b, &o).unwrap(), refb, "write differs from the reference search");
                        assert_eq!(v4::write_threads(b, &o, false).unwrap(), refb);
                        assert_eq!(v4::write_threads(b, &o, true).unwrap(), refb);
                        *sz.entry("v4").or_default() += refb.len();
                        let q = v4::write(b, &if lbl == "dict" { CompactOptions::quick().with_dict("text", d.clone()) } else { CompactOptions::quick() }).unwrap();
                        assert!(v4::is_v4(&q));
                        *sz.entry("quick").or_default() += q.len();
                        let f = v4::write(b, &if lbl == "dict" { CompactOptions::fast().with_dict("text", d.clone()) } else { CompactOptions::fast() }).unwrap();
                        *sz.entry("fast").or_default() += f.len();
                        let v5 = v4::write(b, &o.clone().with_compact_directory(true)).unwrap();
                        assert!(v4::is_v5(&v5));
                        let back = KoreReader::from_bytes_with_dicts(&v5, &ds).unwrap();
                        let back4 = KoreReader::from_bytes_with_dicts(&refb, &ds).unwrap();
                        for (x, y) in back.columns.iter().zip(&back4.columns) { assert!((0..b.num_rows).all(|i| x.data.get_value(i) == y.data.get_value(i))); }
                        *sz.entry("v5").or_default() += v5.len();
                        let q5 = v4::write(b, &if lbl == "dict" { CompactOptions::quick().with_dict("text", d.clone()) } else { CompactOptions::quick() }.with_compact_directory(true)).unwrap();
                        *sz.entry("quick_v5").or_default() += q5.len();
                    }
                    let k = bl.len() as f64;
                    facts.insert(format!("bytes_{size}_{lbl}"), serde_json::json!({
                        "blocks": bl.len(), "v4_mean": sz["v4"] as f64 / k, "quick_mean": sz["quick"] as f64 / k, "fast_mean": sz["fast"] as f64 / k,
                        "v5_mean": sz["v5"] as f64 / k, "quick_v5_mean": sz["quick_v5"] as f64 / k, "identical_blocks": bl.len() }));
                }
            }
        }
        eprintln!("rep {rep} done");
    }
    // whole dataset (every 1000-event segment with the dictionary): bytes and byte identity
    let mut tot: BTreeMap<&str, usize> = BTreeMap::new();
    for (b, s) in blocks.iter().zip(&segs) {
        assert_eq!(&v4::write_reference(b, &def).unwrap(), s);
        *tot.entry("v4").or_default() += s.len();
        *tot.entry("quick").or_default() += v4::write(b, &CompactOptions::quick().with_dict("text", d.clone())).unwrap().len();
        *tot.entry("v5").or_default() += v4::write(b, &def.clone().with_compact_directory(true)).unwrap().len();
    }
    facts.insert("dataset_bytes".into(), serde_json::json!({ "segments": segs.len(), "dict_bytes": d.to_bytes().len(),
        "v4": tot["v4"], "quick": tot["quick"], "v5": tot["v5"], "identical_segments": segs.len() }));

    let mut j = serde_json::Map::new();
    j.insert("n".into(), n.into());
    j.insert("reps".into(), reps.into());
    j.insert("lookups".into(), lookups.into());
    for (k, v) in &res { j.insert(k.clone(), serde_json::json!(v.iter().map(|(b, a)| serde_json::json!({"before": b, "after": a})).collect::<Vec<_>>())); }
    for (k, v) in facts { j.insert(k, v); }
    std::fs::write(&out, serde_json::to_string_pretty(&j).unwrap()).unwrap();
    for (k, v) in &res {
        let mut ra: Vec<f64> = v.iter().map(|(b, a)| a / b).collect();
        let mut bb: Vec<f64> = v.iter().map(|p| p.0).collect();
        let mut aa: Vec<f64> = v.iter().map(|p| p.1).collect();
        println!("{k:28} before {:9.1} us  after {:9.1} us  ratio {:.3}", med(&mut bb) * 1e6, med(&mut aa) * 1e6, med(&mut ra));
    }
    let _ = std::fs::remove_dir_all(&work);
}
