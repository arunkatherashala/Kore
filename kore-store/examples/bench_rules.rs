//! Rule-mode benchmark (docs/kore_rule_mode.md): per dataset and size, the version-4 writer at its
//! default setting and at zstd level 19, and rule mode (`CompactOptions::with_rules()`) at both settings;
//! sizes, paired encode/decode times, and per column the rule that won. It also dumps every block as Arrow
//! buffers (exact bits) for `scripts/kore_rules/py_side.py` (Parquet zstd-22, CSV + xz-9, raw + xz-9).
//!
//!   bench_rules --out DIR [--sizes 100000,1000000] [--sets sensors,orders,logs,sequences,random,events]
//!               [--events EVENTS.jsonl] [--reps 3] [--no-dump]
//!
//! `events` = the realistic Digital Life events table (docs/kore_size.md): the JSONL of
//! format_bench_gen_realistic.py turned into the 8 columns of the log's events table (event_id = sha256 of
//! source|occurred_at|normalised text, recorded_at = the fixed clock of the benchmark, redacted = false).

#[path = "support/rules_gen.rs"]
mod gen;

use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{rules, CompactOptions, KoreReader, KoreWriter, RuleOptions};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn events(path: &Path, n: usize) -> DataBlock {
    let text = std::fs::read_to_string(path).expect("read events jsonl");
    let mut c: Vec<Vec<Option<String>>> = vec![vec![]; 7];
    for line in text.lines().filter(|l| !l.is_empty()).take(n) {
        let v: Value = serde_json::from_str(line).unwrap();
        if v["t"] != "e" { continue; }
        let (src, occ, txt) = (v["source"].as_str().unwrap(), v["occurred_at"].as_str().unwrap(), v["text"].as_str().unwrap());
        let norm = txt.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut h = Sha256::new();
        h.update(src.as_bytes()); h.update(b"|"); h.update(occ.as_bytes()); h.update(b"|"); h.update(norm.as_bytes());
        c[0].push(Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect()));
        c[1].push(Some(occ.to_string()));
        c[2].push(Some("2026-01-01T00:00:00Z".to_string()));
        c[3].push(Some(src.to_string()));
        c[4].push(Some(v["kind"].as_str().unwrap().to_string()));
        c[5].push(Some(txt.to_string()));
        c[6].push(Some(v["tags"].as_str().unwrap().to_string()));
    }
    let rows = c[0].len();
    let names = ["event_id", "occurred_at", "recorded_at", "source", "kind", "text", "tags"];
    let mut cols: Vec<Column> = names.iter().zip(c).map(|(n, v)| Column::str_col(n, v)).collect();
    cols.push(Column::bool_col("redacted", vec![Some(false); rows]));
    DataBlock::new(cols).unwrap()
}

fn median(mut v: Vec<f64>) -> f64 { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); if v.is_empty() { f64::NAN } else { v[v.len() / 2] } }

/// Arrow buffers per column: validity bitmap (1 = valid), values (i64/f64 LE, bool bits) or int64 offsets
/// + bytes for strings.
fn dump(b: &DataBlock, dir: &Path) -> Value {
    std::fs::create_dir_all(dir).unwrap();
    let n = b.num_rows;
    let mut meta = vec![];
    for (k, c) in b.columns.iter().enumerate() {
        let mut valid = vec![0u8; n.div_ceil(8)];
        let set = |valid: &mut Vec<u8>, i: usize| valid[i / 8] |= 1 << (i % 8);
        let (ty, values, offsets): (&str, Vec<u8>, Option<Vec<u8>>) = match &c.data {
            ColumnData::Int64(v) => { let mut o = Vec::with_capacity(n * 8); for (i, x) in v.iter().enumerate() { if x.is_some() { set(&mut valid, i); } o.extend_from_slice(&x.unwrap_or(0).to_le_bytes()); } ("int64", o, None) }
            ColumnData::Float64(v) => { let mut o = Vec::with_capacity(n * 8); for (i, x) in v.iter().enumerate() { if x.is_some() { set(&mut valid, i); } o.extend_from_slice(&x.unwrap_or(0.0).to_bits().to_le_bytes()); } ("float64", o, None) }
            ColumnData::Bool(v) => { let mut o = vec![0u8; n.div_ceil(8)]; for (i, x) in v.iter().enumerate() { if x.is_some() { set(&mut valid, i); } if *x == Some(true) { o[i / 8] |= 1 << (i % 8); } } ("bool", o, None) }
            ColumnData::Str(v) => {
                let mut off = Vec::with_capacity((n + 1) * 8);
                let mut data = vec![];
                off.extend_from_slice(&0i64.to_le_bytes());
                for (i, x) in v.iter().enumerate() { if let Some(s) = x { set(&mut valid, i); data.extend_from_slice(s.as_bytes()); } off.extend_from_slice(&(data.len() as i64).to_le_bytes()); }
                ("large_string", data, Some(off))
            }
            ColumnData::StrDict { .. } => unreachable!(),
        };
        std::fs::write(dir.join(format!("{k}.valid")), &valid).unwrap();
        std::fs::write(dir.join(format!("{k}.values")), &values).unwrap();
        if let Some(o) = offsets { std::fs::write(dir.join(format!("{k}.offsets")), &o).unwrap(); }
        meta.push(json!({"name": c.name, "type": ty}));
    }
    let m = json!({"rows": n, "columns": meta});
    std::fs::write(dir.join("meta.json"), serde_json::to_string_pretty(&m).unwrap()).unwrap();
    m
}

fn same(a: &DataBlock, b: &DataBlock) -> bool {
    a.num_rows == b.num_rows && a.columns.len() == b.columns.len()
        && a.columns.iter().zip(&b.columns).all(|(x, y)| x.name == y.name && rules::digest(&x.data) == rules::digest(&y.data))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |k: &str| args.iter().position(|a| a == k).map(|i| args[i + 1].clone());
    let out = PathBuf::from(get("--out").expect("--out DIR"));
    let sizes: Vec<usize> = get("--sizes").unwrap_or("100000".into()).split(',').map(|s| s.parse().unwrap()).collect();
    let sets: Vec<String> = get("--sets").unwrap_or("sensors,orders,logs,sequences,random".into()).split(',').map(String::from).collect();
    let reps: usize = get("--reps").map(|s| s.parse().unwrap()).unwrap_or(3);
    let no_dump = args.iter().any(|a| a == "--no-dump");
    std::fs::create_dir_all(&out).unwrap();
    for set in &sets {
        for &n in &sizes {
            let t = Instant::now();
            let b = match set.as_str() {
                "sensors" => gen::sensors(n, 20261107),
                "orders" => gen::orders(n, 20261107),
                "logs" => gen::logs(n, 20261107),
                "sequences" => gen::sequences(n, 20261107),
                "random" => gen::random(n, 20261107),
                // the control without the uniform-[0,1) doubles: no column has a rule (worst case of rule mode)
                "random_norule" => { let mut b = gen::random(n, 20261107); b.columns.retain(|c| c.name != "f64_unit"); b }
                "events" => events(Path::new(&get("--events").expect("--events JSONL")), n),
                s => panic!("unknown set {s}"),
            };
            eprintln!("[{set} {n}] generated in {:.1}s", t.elapsed().as_secs_f64());
            let values = (b.num_rows * b.columns.len()) as f64;
            let o_def = CompactOptions::default();
            let o_19 = CompactOptions::default().with_zstd_level(19);
            let r_def = RuleOptions::default();
            let r_19 = o_19.clone().with_rules();
            // paired encode: v4 default, rules default (alternating order per rep)
            let (mut e_v4, mut e_r, mut e_ratio) = (vec![], vec![], vec![]);
            let (mut v4b, mut rb) = (vec![], vec![]);
            for rep in 0..reps {
                let mut run_v4 = || { let t = Instant::now(); let x = KoreWriter::to_bytes_compact(&b, &o_def).unwrap(); (t.elapsed().as_secs_f64(), x) };
                let mut run_r = || { let t = Instant::now(); let x = KoreWriter::to_bytes_rules(&b, &r_def).unwrap(); (t.elapsed().as_secs_f64(), x) };
                let ((a, x), (c, y)) = if rep % 2 == 0 { let p = run_v4(); (p, run_r()) } else { let q = run_r(); (run_v4(), q) };
                e_v4.push(a); e_r.push(c); e_ratio.push(c / a);
                v4b = x; rb = y;
            }
            let t19 = Instant::now();
            let v4b19 = KoreWriter::to_bytes_compact(&b, &o_19).unwrap();
            let e_v4_19 = t19.elapsed().as_secs_f64();
            let t19 = Instant::now();
            let (rb19, _) = rules::write_explained(&b, &r_19).unwrap();
            let e_r_19 = t19.elapsed().as_secs_f64();
            let (_, report) = rules::write_explained(&b, &r_def).unwrap();
            // paired decode
            let (mut d_v4, mut d_r, mut d_ratio) = (vec![], vec![], vec![]);
            for rep in 0..reps {
                let mut dv = || { let t = Instant::now(); let x = KoreReader::from_bytes(&v4b).unwrap(); (t.elapsed().as_secs_f64(), x) };
                let mut dr = || { let t = Instant::now(); let x = KoreReader::from_bytes(&rb).unwrap(); (t.elapsed().as_secs_f64(), x) };
                let ((a, x), (c, y)) = if rep % 2 == 0 { let p = dv(); (p, dr()) } else { let q = dr(); (dv(), q) };
                assert!(same(&b, &x) && same(&b, &y), "round trip failed");
                d_v4.push(a); d_r.push(c); d_ratio.push(c / a);
            }
            assert!(same(&b, &KoreReader::from_bytes(&rb19).unwrap()));
            assert!(rb.len() <= v4b.len() && rb19.len() <= v4b19.len());
            let raw_bytes: usize = b.columns.iter().map(|c| match &c.data {
                ColumnData::Int64(v) => v.len() * 8, ColumnData::Float64(v) => v.len() * 8, ColumnData::Bool(v) => v.len().div_ceil(8),
                ColumnData::Str(v) => v.iter().flatten().map(|s| s.len() + 8).sum(), _ => 0 }).sum();
            let cols: Vec<Value> = report.iter().map(|c| json!({"name": c.name, "v4_bytes": c.v4_bytes,
                "rule": c.rule.as_ref().map(|r| r.0.clone()), "rule_bytes": c.rule.as_ref().map(|r| r.1)})).collect();
            let mut res = json!({
                "set": set, "rows": b.num_rows, "cols": b.columns.len(), "values": values, "raw_bytes": raw_bytes,
                "v4_default": v4b.len(), "v4_19": v4b19.len(), "rules_default": rb.len(), "rules_19": rb19.len(),
                "rules_is_v5": rules::is_v5(&rb),
                "encode_v4_s": e_v4, "encode_rules_s": e_r, "encode_ratio_median": median(e_ratio.clone()), "encode_ratio": e_ratio,
                "encode_v4_19_s": e_v4_19, "encode_rules_19_s": e_r_19,
                "decode_v4_s": d_v4, "decode_rules_s": d_r, "decode_ratio_median": median(d_ratio.clone()), "decode_ratio": d_ratio,
                "columns": cols,
            });
            eprintln!("[{set} {n}] v4 {} B ({:.3} B/value), v4-19 {} B, rules {} B ({:.3} B/value), rules-19 {} B; encode x{:.2}, decode x{:.2}",
                v4b.len(), v4b.len() as f64 / values, v4b19.len(), rb.len(), rb.len() as f64 / values, rb19.len(), median(e_ratio), median(d_ratio));
            for c in &report { eprintln!("    {:<28} v4 {:>10}  {}", c.name, c.v4_bytes, c.rule.as_ref().map_or("-".to_string(), |r| format!("{} B  {}", r.1, r.0))); }
            if !no_dump { res["dump"] = dump(&b, &out.join(format!("dump_{set}_{n}"))); }
            std::fs::write(out.join(format!("store_{set}_{n}.json")), serde_json::to_string_pretty(&res).unwrap()).unwrap();
        }
    }
}
