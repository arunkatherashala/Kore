//! Version-4 integer transform selection (docs/kore_v4_int_select.md): file sizes, write and read times of
//! the version-4 writer at its default setting and at zstd level 19, per dataset, plus per i64 column the
//! transform that was written and its payload size. With `--probe` it also prints, per i64 column, the size
//! of every transform (Plain, Delta, Varint) after the zstd-1 pre-score and after the real stage search.
//!
//!   bench_v4_int_select --sets dump:DIR,sensors,orders,logs,random[,...] [--sizes 1000000] [--reps 3]
//!                       [--levels default,level19] [--probe]
//!
//! `dump:DIR` is an Arrow-buffer dump as written by `bench_rules` / `scripts/kore_rules/export_*.py`.

#[path = "support/rules_gen.rs"]
mod gen;

use kore_core::{ColumnData, DataBlock};
use kore_store::{codec, rules, v4, CompactOptions, DictSet, KoreReader, KoreWriter, Transform};
use serde_json::Value;
use std::path::Path;
use std::time::Instant;

fn load_dump(dir: &Path, n: usize) -> DataBlock {
    let meta: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("meta.json")).unwrap()).unwrap();
    let rows = meta["rows"].as_u64().unwrap() as usize;
    let take = if n == 0 { rows } else { n.min(rows) };
    let mut cols = vec![];
    for (k, c) in meta["columns"].as_array().unwrap().iter().enumerate() {
        let valid = std::fs::read(dir.join(format!("{k}.valid"))).unwrap();
        let values = std::fs::read(dir.join(format!("{k}.values"))).unwrap();
        let ok = |i: usize| valid[i / 8] >> (i % 8) & 1 == 1;
        let w = |i: usize| u64::from_le_bytes(values[i * 8..i * 8 + 8].try_into().unwrap());
        let data = match c["type"].as_str().unwrap() {
            "int64" => ColumnData::Int64((0..take).map(|i| if ok(i) { Some(w(i) as i64) } else { None }).collect()),
            "float64" => ColumnData::Float64((0..take).map(|i| if ok(i) { Some(f64::from_bits(w(i))) } else { None }).collect()),
            "bool" => ColumnData::Bool((0..take).map(|i| if ok(i) { Some(values[i / 8] >> (i % 8) & 1 == 1) } else { None }).collect()),
            "large_string" => {
                let off = std::fs::read(dir.join(format!("{k}.offsets"))).unwrap();
                let o = |i: usize| i64::from_le_bytes(off[i * 8..i * 8 + 8].try_into().unwrap()) as usize;
                ColumnData::Str((0..take).map(|i| if ok(i) { Some(String::from_utf8(values[o(i)..o(i + 1)].to_vec()).unwrap()) } else { None }).collect())
            }
            t => panic!("type {t}"),
        };
        cols.push(kore_core::Column { name: c["name"].as_str().unwrap().to_string(), data });
    }
    DataBlock::new(cols).unwrap()
}

fn median(mut v: Vec<f64>) -> f64 { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[v.len() / 2] }

fn same(a: &DataBlock, b: &DataBlock) -> bool {
    a.num_rows == b.num_rows && a.columns.iter().zip(&b.columns).all(|(x, y)| x.name == y.name && rules::digest(&x.data) == rules::digest(&y.data))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |k: &str| args.iter().position(|a| a == k).map(|i| args[i + 1].clone());
    let sets: Vec<String> = get("--sets").unwrap_or("sensors,orders,logs,random".into()).split(',').map(String::from).collect();
    let n: usize = get("--sizes").map(|s| s.parse().unwrap()).unwrap_or(1_000_000);
    let reps: usize = get("--reps").map(|s| s.parse().unwrap()).unwrap_or(3);
    let probe = args.iter().any(|a| a == "--probe");
    let levels = get("--levels").unwrap_or("default,level19".into());
    for set in &sets {
        let b = match set.as_str() {
            "sensors" => gen::sensors(n, 20261107),
            "orders" => gen::orders(n, 20261107),
            "logs" => gen::logs(n, 20261107),
            "sequences" => gen::sequences(n, 20261107),
            "random" => gen::random(n, 20261107),
            d if d.starts_with("dump:") => load_dump(Path::new(&d[5..]), 0),
            s => panic!("set {s}"),
        };
        let name = set.strip_prefix("dump:").map(|d| Path::new(d).file_name().unwrap().to_string_lossy().to_string()).unwrap_or(set.clone());
        for (label, o) in [("default", CompactOptions::default()), ("level19", CompactOptions::default().with_zstd_level(19))] {
            if !levels.split(',').any(|l| l == label) { continue; }
            let mut wt = vec![];
            let mut rt = vec![];
            let mut bytes = vec![];
            for _ in 0..reps {
                let t = Instant::now();
                bytes = KoreWriter::to_bytes_compact(&b, &o).unwrap();
                wt.push(t.elapsed().as_secs_f64() * 1e3);
                let t = Instant::now();
                let back = KoreReader::from_bytes_with_dicts(&bytes, &DictSet::new()).unwrap();
                rt.push(t.elapsed().as_secs_f64() * 1e3);
                assert!(same(&b, &back), "{name} {label}: round trip differs");
            }
            println!("{name} rows={} {label} bytes={} write_ms={:.0} read_ms={:.0}", b.num_rows, bytes.len(), median(wt), median(rt));
            let info = v4::info(&bytes).unwrap();
            for (c, ci) in b.columns.iter().zip(&info.columns) {
                if matches!(c.data, ColumnData::Int64(_)) { println!("  {name} {label} col {} {:?}", c.name, ci); }
            }
        }
        if probe {
            for c in &b.columns {
                if !matches!(c.data, ColumnData::Int64(_)) { continue; }
                for t in [Transform::Plain, Transform::Delta, Transform::Varint] {
                    let raw = codec::encode_transform(&c.data, t).unwrap();
                    let z1 = zstd::bulk::compress(&raw, 1).unwrap().len();
                    let z9 = v4::stage_payload(&raw, 3, 9, None).len();
                    let z19 = v4::stage_payload(&raw, 3, 19, None).len();
                    println!("  probe {name} {} {t:?}: raw={} zstd1={} zstd9={} zstd19={}", c.name, raw.len(), z1, z9, z19);
                }
            }
        }
    }
}
