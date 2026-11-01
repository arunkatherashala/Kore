//! Benchmark harness, KORE side of the log-format comparison (see docs/format_bench.md).
//! Uses the versioned log through its existing public API only; temp dirs only.
//!
//!   cargo run --release -p kore-self --example bench_log_formats --offline -- \
//!       --data DATA.jsonl --n N --work WORKDIR --out RESULT.json [--build-reps 5] [--reps 5]
//!
//! The data file comes from benches/format_bench_gen.py (same file the Python
//! harness reads). Every query returns a `check` that must equal the Python side.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Instant;
use versioned::*;

const BATCH: usize = 1000;
const OWNER: &str = "bench";

struct Rev {
    key: String,
    stance: String,
    conf: f64,
    rationale: String,
    valid_from: String,
    cause: usize,
}

struct Args {
    data: String,
    n: usize,
    work: PathBuf,
    out: String,
    build_reps: usize,
    reps: usize,
    lookups: usize,
    sql_lookups: usize,
    tamper_trials: usize,
    seed: u64,
    size_only: bool,
    format: LogFormat,
}

fn parse_args() -> Args {
    let v: Vec<String> = std::env::args().collect();
    let get = |k: &str| v.iter().position(|a| a == k).map(|i| v[i + 1].clone());
    let n: usize = get("--n").expect("--n").parse().unwrap();
    Args {
        data: get("--data").expect("--data"),
        n,
        work: PathBuf::from(get("--work").expect("--work")),
        out: get("--out").expect("--out"),
        build_reps: get("--build-reps").map(|s| s.parse().unwrap()).unwrap_or(5),
        reps: get("--reps").map(|s| s.parse().unwrap()).unwrap_or(5),
        lookups: get("--lookups").map(|s| s.parse().unwrap()).unwrap_or(100),
        sql_lookups: get("--sql-lookups").map(|s| s.parse().unwrap()).unwrap_or(if n <= 100_000 { 100 } else { 20 }),
        tamper_trials: get("--tamper-trials").map(|s| s.parse().unwrap()).unwrap_or(40),
        size_only: v.iter().any(|a| a == "--size-only"),
        format: match get("--format").as_deref() { Some("2") => LogFormat::V2, Some("3") => LogFormat::V3, _ => LogFormat::V1 },
        seed: get("--seed").map(|s| s.parse().unwrap()).unwrap_or(7),
    }
}

fn load(path: &str) -> (Vec<NewEvent>, Vec<Rev>) {
    let text = std::fs::read_to_string(path).expect("read data");
    let (mut ev, mut rv) = (vec![], vec![]);
    for line in text.lines() {
        let o: Value = serde_json::from_str(line).unwrap();
        let s = |k: &str| o[k].as_str().unwrap().to_string();
        if o["t"] == "e" {
            let tags = s("tags");
            ev.push(NewEvent {
                source: s("source"),
                occurred_at: s("occurred_at"),
                kind: s("kind"),
                text: s("text"),
                tags: if tags.is_empty() { vec![] } else { tags.split(',').map(|x| x.to_string()).collect() },
                redacted: false,
            });
        } else {
            rv.push(Rev { key: s("key"), stance: s("stance"), conf: o["conf"].as_f64().unwrap(), rationale: s("rationale"), valid_from: s("valid_from"), cause: o["cause"].as_u64().unwrap() as usize });
        }
    }
    (ev, rv)
}

fn eid(e: &NewEvent) -> String {
    event_id(&e.source, &normalize_ts(&e.occurred_at).unwrap(), &e.text)
}

fn append_event(k: usize) -> NewEvent {
    // 2030-01-01T00:00:00Z = 1893456000 ; k seconds later, formatted like the Python side (k < 28 days here)
    let secs = k as u64;
    let (d, r) = (secs / 86400, secs % 86400);
    NewEvent {
        source: "bench-append".into(),
        occurred_at: format!("2030-01-{:02}T{:02}:{:02}:{:02}Z", 1 + d, r / 3600, r % 3600 / 60, r % 60),
        kind: "note".into(),
        text: format!("append event {k} of the bench"),
        tags: vec!["bench".into()],
        redacted: false,
    }
}

fn median(xs: &[f64]) -> f64 {
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

fn stats(xs: &[f64]) -> Value {
    json!({"median": median(xs), "min": xs.iter().cloned().fold(f64::INFINITY, f64::min), "max": xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max), "reps": xs.len()})
}

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

fn list_files(p: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(p) {
        let mut v: Vec<_> = rd.flatten().collect();
        v.sort_by_key(|f| f.path());
        for f in v {
            if f.metadata().unwrap().is_dir() {
                list_files(&f.path(), out);
            } else {
                out.push(f.path());
            }
        }
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for f in std::fs::read_dir(from).unwrap().flatten() {
        let dst = to.join(f.file_name());
        if f.path().is_dir() {
            copy_dir(&f.path(), &dst);
        } else {
            std::fs::copy(f.path(), dst).unwrap();
        }
    }
}

/// Repeat `f` (1 warm-up), return per-run seconds and the last result.
fn timed<T>(reps: usize, mut f: impl FnMut() -> T) -> (Vec<f64>, T) {
    let mut last = f();
    let mut xs = vec![];
    for _ in 0..reps {
        let t = Instant::now();
        last = f();
        xs.push(t.elapsed().as_secs_f64());
    }
    (xs, last)
}

/// Same text as Python's json.dumps(x, sort_keys=True) of a list of lists of (str|int).
fn pyjson_rows(rows: &[Vec<String>], int_cols: &[usize]) -> String {
    let rs: Vec<String> = rows
        .iter()
        .map(|r| {
            let cs: Vec<String> = r.iter().enumerate().map(|(i, c)| if int_cols.contains(&i) { c.clone() } else { format!("\"{}\"", c) }).collect();
            format!("[{}]", cs.join(", "))
        })
        .collect();
    format!("[{}]", rs.join(", "))
}

fn chk(s: &str) -> String {
    sha256_hex(&[s.as_bytes()])[..16].to_string()
}

fn sorted_rows(b: &kore_core::DataBlock) -> Vec<Vec<String>> {
    let mut r = sql::block_strings(b);
    r.sort();
    r
}

fn ev_str(c: &Cell) -> &str {
    match c {
        Cell::S(s) => s,
        _ => "",
    }
}

struct Xor(u64);
impl Xor {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn main() {
    let a = parse_args();
    let n = a.n;
    let _ = std::fs::remove_dir_all(&a.work);
    std::fs::create_dir_all(&a.work).unwrap();
    let (events, revs) = load(&a.data);
    assert_eq!(events.len(), n);
    let ids: Vec<String> = events.iter().map(eid).collect();
    assert_eq!(ids.iter().collect::<std::collections::HashSet<_>>().len(), n);
    let nb = (n + BATCH - 1) / BATCH;
    let mk_batches = || -> Vec<Batch> {
        (0..nb)
            .map(|b| {
                let (lo, hi) = (b * BATCH, ((b + 1) * BATCH).min(n));
                let revisions = revs
                    .iter()
                    .filter(|r| r.cause >= lo && r.cause < hi)
                    .map(|r| NewRevision {
                        entity_type: "belief".into(),
                        key: r.key.clone(),
                        stance: r.stance.clone(),
                        confidence: r.conf,
                        rationale: r.rationale.clone(),
                        supersedes: None,
                        valid_from: r.valid_from.clone(),
                        author: "rule:bench".into(),
                        caused_by: vec![ids[r.cause].clone()],
                    })
                    .collect();
                Batch { events: events[lo..hi].to_vec(), revisions, ..Default::default() }
            })
            .collect()
    };
    let batches = mk_batches();
    let l = a.lookups;
    let lk_idx: Vec<usize> = (0..l).map(|k| ((k as u64 * 2654435761 + 12345) % n as u64) as usize).collect();
    let lk_ids: Vec<String> = lk_idx.iter().map(|&i| ids[i].clone()).collect();
    let (t1, t2) = (events[n * 45 / 100].occurred_at.clone(), events[n * 55 / 100].occurred_at.clone());
    let nbv = nb as u64;
    let mut asof_versions: Vec<u64> = vec![1, (nbv / 2).max(1), nbv];
    asof_versions.sort();
    asof_versions.dedup();

    let mut res = json!({"n": n, "batches": nb, "reps": a.reps, "build_reps": a.build_reps, "lookups": l, "sql_lookups": a.sql_lookups,
        "range": [t1, t2], "asof_versions": asof_versions, "seed": a.seed,
        "build": "cargo --release; versioned log API (commit_batch)"});

    // ---- build (batch of 1000), repeated into fresh dirs
    let mut times = vec![];
    let mut clone_cost = 0.0;
    let mut root = PathBuf::new();
    for rep in 0..a.build_reps {
        root = a.work.join(format!("build_{rep}"));
        let mut log = VersionedLog::open_or_create_with_format(&root, OWNER, now_iso, a.format).unwrap();
        let t = Instant::now();
        let mut cc = 0.0;
        for b in &batches {
            let c = Instant::now();
            let bb = b.clone();
            cc += c.elapsed().as_secs_f64();
            log.commit_batch(bb).unwrap();
        }
        let dt = t.elapsed().as_secs_f64();
        clone_cost = cc;
        times.push(dt);
        assert_eq!(log.latest_version(), nb as u64);
        drop(log);
        if rep + 1 < a.build_reps {
            std::fs::remove_dir_all(&root).unwrap();
        }
        eprintln!("[kore] build rep {rep}: {dt:.2}s (batch clone inside timer: {cc:.3}s)");
    }
    res["build_b1000_s"] = stats(&times);
    res["build_b1000_events_per_s"] = stats(&times.iter().map(|x| n as f64 / x).collect::<Vec<_>>());
    res["build_clone_cost_s"] = json!(clone_cost);
    let logdir = root.join(OWNER).join("log");
    let mut per_table = serde_json::Map::new();
    for t in ALL_TABLES {
        per_table.insert(t.name().into(), json!(dir_size(&logdir.join(t.name()))));
    }
    per_table.insert("MANIFEST".into(), json!(std::fs::metadata(logdir.join("MANIFEST")).unwrap().len()));
    res["size_bytes"] = json!(dir_size(&root));
    res["size_by_table"] = Value::Object(per_table);
    if a.format != LogFormat::V1 {
        // actual per-column bytes of the events segments (v2 only)
        let mut cols = std::collections::BTreeMap::<String, usize>::new();
        if let Ok(rd) = std::fs::read_dir(logdir.join("events")) {
            for f in rd.flatten() {
                let b = std::fs::read(f.path()).unwrap();
                for (n, sz) in versioned::codec::column_block_sizes(&b).unwrap() {
                    *cols.entry(n).or_default() += sz;
                }
            }
        }
        res["events_column_bytes_v2_actual"] = json!(cols);
    }
    let mut fl = vec![];
    list_files(&root, &mut fl);
    res["file_count"] = json!(fl.len());

    // ---- open (reads + sha256-verifies every segment, rebuilds hash index)
    let (xs, _) = timed(a.reps, || VersionedLog::open(&root, OWNER).unwrap());
    res["open_s"] = stats(&xs);
    res["log_format"] = json!(a.format.number());
    // open + decode of the latest snapshot: the same work the old open() did eagerly
    let (xs, _) = timed(a.reps, || {
        let l = VersionedLog::open(&root, OWNER).unwrap();
        let s = l.open_latest();
        s.raw(Table::Events).len()
    });
    res["open_plus_latest_snapshot_s"] = stats(&xs);
    // id -> row lookup on a freshly opened log (new API; the snapshot is not materialised first)
    {
        let fresh = VersionedLog::open(&root, OWNER).unwrap();
        let want: usize = lk_idx.iter().map(|&i| events[i].text.len()).sum();
        let (xs, c) = timed(a.reps, || lk_ids.iter().map(|id| fresh.get_event(id).unwrap().expect("found").text.len()).sum::<usize>());
        assert_eq!(c, want);
        res["get_event_per_op_s"] = stats(&xs.iter().map(|x| x / l as f64).collect::<Vec<_>>());
    }
    let mut log = VersionedLog::open(&root, OWNER).unwrap();
    let (xs, _) = timed(a.reps, || log.verify().unwrap());
    res["verify_s"] = stats(&xs);

    // ---- point lookup
    let snap = log.open_latest();
    let expected_check: usize = lk_idx.iter().map(|&i| events[i].text.len()).sum();
    let (xs, c) = timed(a.reps, || lk_ids.iter().filter(|i| log.has_event(i)).count());
    assert_eq!(c, l);
    let mut lk = json!({"has_event_existence_only_per_op_s": stats(&xs.iter().map(|x| x / l as f64).collect::<Vec<_>>())});
    let (xs, c) = timed(a.reps, || {
        let mut s = 0;
        for id in &lk_ids {
            let row = snap.raw(Table::Events).iter().find(|r| ev_str(&r[0]) == id).expect("found");
            s += ev_str(&row[5]).len();
        }
        s
    });
    assert_eq!(c, expected_check);
    lk["native_scan_per_op_s"] = stats(&xs.iter().map(|x| x / l as f64).collect::<Vec<_>>());
    lk["check"] = json!(c);
    lk["expected_check"] = json!(expected_check);
    // SQL path
    let mut ctx = kore_sql::KqlContext::new();
    let t = Instant::now();
    sql::register_into(&mut ctx, &snap);
    res["register_into_s"] = json!(t.elapsed().as_secs_f64());
    let sl = a.sql_lookups.min(l);
    let exp_sl: usize = lk_idx[..sl].iter().map(|&i| events[i].text.len()).sum();
    let (xs, c) = timed(a.reps, || {
        let mut s = 0;
        for id in &lk_ids[..sl] {
            let q = format!("SELECT event_id, occurred_at, source, kind, text, tags FROM events WHERE event_id = '{id}'");
            let b = ctx.query(&q).unwrap();
            assert_eq!(b.num_rows, 1);
            s += sql::block_strings(&b)[0][4].len();
        }
        s
    });
    assert_eq!(c, exp_sl);
    lk["sql_registered_ctx_per_op_s"] = stats(&xs.iter().map(|x| x / sl as f64).collect::<Vec<_>>());
    lk["sql_ops"] = json!(sl);
    let e2e_n = 5.min(sl);
    let (xs, _) = timed(a.reps.min(5), || {
        for id in &lk_ids[..e2e_n] {
            let q = format!("SELECT event_id, occurred_at, source, kind, text, tags FROM events WHERE event_id = '{id}'");
            assert_eq!(run_readonly_sql(&snap, &q).unwrap().num_rows, 1);
        }
    });
    lk["sql_end_to_end_run_readonly_sql_per_op_s"] = stats(&xs.iter().map(|x| x / e2e_n as f64).collect::<Vec<_>>());
    lk["sql_end_to_end_ops"] = json!(e2e_n);
    res["lookup"] = lk;
    eprintln!("[kore] lookup done");

    // ---- range / aggregate
    let q_range = format!("SELECT kind, COUNT(*) FROM events WHERE occurred_at >= '{t1}' AND occurred_at < '{t2}' GROUP BY kind");
    let q_full = "SELECT source, COUNT(*) FROM events GROUP BY source".to_string();
    let (xs, b) = timed(a.reps, || ctx.query(&q_range).unwrap());
    let sql_check = chk(&pyjson_rows(&sorted_rows(&b), &[1]));
    let mut rg = json!({"sql_registered_ctx_s": stats(&xs), "check": sql_check});
    let (xs, _) = timed(a.reps.min(5), || run_readonly_sql(&snap, &q_range).unwrap());
    rg["sql_end_to_end_s"] = stats(&xs);
    let (xs, c) = timed(a.reps, || {
        let mut m = std::collections::BTreeMap::<String, i64>::new();
        for r in snap.raw(Table::Events) {
            let ts = ev_str(&r[1]);
            if ts >= t1.as_str() && ts < t2.as_str() {
                *m.entry(ev_str(&r[4]).to_string()).or_default() += 1;
            }
        }
        m
    });
    let rows: Vec<Vec<String>> = c.iter().map(|(k, v)| vec![k.clone(), v.to_string()]).collect();
    rg["native_rust_loop_s"] = stats(&xs);
    rg["native_rust_check"] = json!(chk(&pyjson_rows(&rows, &[1])));
    res["range"] = rg;
    let (xs, b) = timed(a.reps, || ctx.query(&q_full).unwrap());
    let mut fa = json!({"sql_registered_ctx_s": stats(&xs), "check": chk(&pyjson_rows(&sorted_rows(&b), &[1]))});
    let (xs, _) = timed(a.reps.min(5), || run_readonly_sql(&snap, &q_full).unwrap());
    fa["sql_end_to_end_s"] = stats(&xs);
    let (xs, c) = timed(a.reps, || {
        let mut m = std::collections::BTreeMap::<String, i64>::new();
        for r in snap.raw(Table::Events) {
            *m.entry(ev_str(&r[3]).to_string()).or_default() += 1;
        }
        m
    });
    let rows: Vec<Vec<String>> = c.iter().map(|(k, v)| vec![k.clone(), v.to_string()]).collect();
    fa["native_rust_loop_s"] = stats(&xs);
    fa["native_rust_check"] = json!(chk(&pyjson_rows(&rows, &[1])));
    res["fullagg"] = fa;
    eprintln!("[kore] range done");
    if a.size_only {
        // storage-efficiency mode: build + size + open + lookup + range only
        res["size_only"] = json!(true);
        // per-column encoded size of the events table, re-encoding each column on its own with the
        // existing KoreWriter in 1000-row chunks (as the log stores them); includes ~30 B header per chunk-column
        let mut colb = serde_json::Map::new();
        let evrows = snap.raw(Table::Events);
        let ncols = Table::Events.columns().len();
        let mut tot = vec![0usize; ncols];
        for chunk in evrows.chunks(BATCH) {
            let blk = rows_to_block(Table::Events, chunk);
            for ci in 0..ncols {
                let one = kore_core::DataBlock::new(vec![blk.columns[ci].clone()]).unwrap();
                tot[ci] += kore_store::KoreWriter::to_bytes(&one).len();
            }
        }
        for (ci, (name, _)) in Table::Events.columns().iter().enumerate() {
            colb.insert(name.to_string(), json!(tot[ci]));
        }
        res["events_column_bytes_single_column_reencode"] = Value::Object(colb);
        std::fs::write(&a.out, serde_json::to_string_pretty(&res).unwrap()).unwrap();
        drop(log);
        let _ = std::fs::remove_dir_all(&a.work);
        return;
    }

    // ---- AS OF version
    let mut asof = serde_json::Map::new();
    for &v in &asof_versions {
        let (xs_open, s) = timed(a.reps, || log.open_at(AsOf::Version(v)).unwrap());
        let (xs_reg, c2) = timed(a.reps, || {
            let mut c = kore_sql::KqlContext::new();
            sql::register_into(&mut c, &s);
            c
        });
        let mut c2 = c2;
        let q_ev = "SELECT kind, COUNT(*) FROM events GROUP BY kind";
        let q_be = "SELECT key, stance FROM beliefs_current";
        let (xs_q, (b1, b2)) = timed(a.reps, || (c2.query(q_ev).unwrap(), c2.query(q_be).unwrap()));
        let ev_rows = sorted_rows(&b1);
        let be_rows = sorted_rows(&b2);
        let check = chk(&format!("[{}, {}]", pyjson_rows(&ev_rows, &[1]), pyjson_rows(&be_rows, &[])));
        let tot: Vec<f64> = (0..xs_open.len()).map(|i| xs_open[i] + xs_reg[i] + xs_q[i]).collect();
        asof.insert(v.to_string(), json!({"open_at_s": stats(&xs_open), "register_s": stats(&xs_reg), "queries_s": stats(&xs_q), "total_s": stats(&tot), "check": check,
            "events_in_snapshot": s.raw(Table::Events).len(), "snapshot_hash": log.snapshot_hash(v).unwrap()[..16].to_string()}));
        eprintln!("[kore] asof {v} done");
    }
    res["asof"] = Value::Object(asof);
    drop(snap);
    drop(ctx);

    // ---- tamper
    if a.tamper_trials > 0 && n <= 100_000 {
        let base_hash = log.open_latest().content_hash();
        let mut files = vec![];
        list_files(&logdir, &mut files);
        let sizes: Vec<u64> = files.iter().map(|f| std::fs::metadata(f).unwrap().len()).collect();
        let total: u64 = sizes.iter().sum();
        let mut rng = Xor(a.seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
        let (mut verify_det, mut open_only, mut silent, mut harmless) = (0, 0, 0, 0);
        let mut by_file = serde_json::Map::new();
        for _ in 0..a.tamper_trials {
            let mut off = rng.next() % total;
            let bit = (rng.next() % 8) as u8;
            let mut fi = 0;
            while off >= sizes[fi] {
                off -= sizes[fi];
                fi += 1;
            }
            let p = &files[fi];
            let mut bytes = std::fs::read(p).unwrap();
            bytes[off as usize] ^= 1 << bit;
            std::fs::write(p, &bytes).unwrap();
            let v = verify_dir(&logdir);
            let o = VersionedLog::open(&root, OWNER);
            let kind = if v.is_err() {
                verify_det += 1;
                "verify_detected"
            } else if o.is_err() {
                open_only += 1;
                "open_detected_only"
            } else if o.as_ref().unwrap().open_latest().content_hash() != base_hash {
                silent += 1;
                "SILENT"
            } else {
                harmless += 1;
                "harmless"
            };
            let name = p.strip_prefix(&logdir).unwrap().to_string_lossy().replace('\\', "/");
            let name = name.split('/').next().unwrap().to_string();
            let e = by_file.entry(format!("{name}:{kind}")).or_insert(json!(0));
            *e = json!(e.as_u64().unwrap() + 1);
            bytes[off as usize] ^= 1 << bit;
            std::fs::write(p, &bytes).unwrap();
        }
        assert!(verify_dir(&logdir).is_ok(), "restore failed");
        res["tamper"] = json!({"trials": a.tamper_trials, "detected_by_check": verify_det, "detected_only_by_open": open_only,
            "silent_data_change": silent, "harmless_no_change": harmless, "by_dir_and_outcome": by_file,
            "check_used": "verify_dir (what log.verify()/self_verify runs); open() also reported"});
        eprintln!("[kore] tamper done");
    }

    // ---- idempotent re-ingest: events only
    let rows_before = log.versions().last().unwrap().rows["events"];
    let ver_before = log.latest_version();
    let sz0 = dir_size(&root);
    let t = Instant::now();
    let mut created = 0;
    let mut suppressed_ids = 0;
    for b in &batches {
        let info = log.append_events(&b.events).unwrap();
        if info.created {
            created += 1;
        }
        suppressed_ids += info.event_ids.len();
    }
    let dt = t.elapsed().as_secs_f64();
    res["reingest_events"] = json!({"seconds": dt, "versions_before": ver_before, "versions_after": log.latest_version(), "new_versions_created": created,
        "events_rows_before": rows_before, "events_rows_after": log.versions().last().unwrap().rows["events"], "size_delta_bytes": dir_size(&root) as i64 - sz0 as i64,
        "event_ids_returned": suppressed_ids, "verify_ok": log.verify().is_ok()});
    // revisions replay on a copy (so later append numbers stay clean)
    {
        let rroot = a.work.join("replay");
        copy_dir(&root, &rroot);
        let mut rl = VersionedLog::open(&rroot, OWNER).unwrap();
        let r0 = rl.versions().last().unwrap().rows["revisions"];
        let v0 = rl.latest_version();
        let mut newv = 0;
        for b in &batches {
            if b.revisions.is_empty() {
                continue;
            }
            let info = rl.commit_batch(Batch { revisions: b.revisions.clone(), ..Default::default() }).unwrap();
            if info.created {
                newv += 1;
            }
        }
        res["reingest_revisions_replay"] = json!({"revisions_before": r0, "revisions_after": rl.versions().last().unwrap().rows["revisions"],
            "versions_before": v0, "versions_after": rl.latest_version(), "new_versions": newv,
            "note": "replaying an old revision with supersedes=None supersedes the CURRENT head, so it is recorded as a new revision unless identical to the head"});
        drop(rl);
        std::fs::remove_dir_all(&rroot).unwrap();
    }
    eprintln!("[kore] reingest done");

    // ---- append: batch of 1 (100 commits per rep) and tail batch of 1000, onto the N-event log
    let mut k = 0usize;
    let (mut x1, mut x2) = (vec![], vec![]);
    for _ in 0..5 {
        let evs: Vec<NewEvent> = (0..100).map(|i| append_event(k + i)).collect();
        k += 100;
        let t = Instant::now();
        for e in &evs {
            assert!(log.append_events(std::slice::from_ref(e)).unwrap().created);
        }
        x1.push(t.elapsed().as_secs_f64());
    }
    for _ in 0..5 {
        let evs: Vec<NewEvent> = (0..BATCH).map(|i| append_event(k + i)).collect();
        k += BATCH;
        let t = Instant::now();
        assert!(log.append_events(&evs).unwrap().created);
        x2.push(t.elapsed().as_secs_f64());
    }
    res["append"] = json!({"b1_events_per_s": stats(&x1.iter().map(|x| 100.0 / x).collect::<Vec<_>>()), "b1_ms_per_commit": stats(&x1.iter().map(|x| x / 100.0 * 1000.0).collect::<Vec<_>>()),
        "b1000_events_per_s": stats(&x2.iter().map(|x| BATCH as f64 / x).collect::<Vec<_>>()), "b1000_ms_per_batch": stats(&x2.iter().map(|x| x * 1000.0).collect::<Vec<_>>()),
        "b1_commits_per_rep": 100,
        "config": "temp-file write + fsync per segment, rename, fsync MANIFEST.tmp, rename; one version per commit",
        "verify_after": log.verify().is_ok()});

    std::fs::write(&a.out, serde_json::to_string_pretty(&res).unwrap()).unwrap();
    drop(log);
    let _ = std::fs::remove_dir_all(&a.work);
}
