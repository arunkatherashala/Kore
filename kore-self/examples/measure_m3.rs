//! Track M3 (docs/m3_design.md): World M3 from the January 2024 NYC yellow-taxi file. Measurement only.
//!   cargo run -p kore-self --example measure_m3 --offline --release -- export --source FILE --out DIR   [tuning seeds 101..120]
//!   cargo run -p kore-self --example measure_m3 --offline --release -- tune   --source FILE --out FILE.tsv
//!   cargo run -p kore-self --example measure_m3 --offline --release -- report --source FILE --tuned FILE.tsv --out-dir DIR
//!   cargo run -p kore-self --example measure_m3 --offline --release -- sweep  --source FILE --out FILE.tsv
//!
//! The source file is untrusted input outside the repository: its SHA-256 is re-checked against the code constant
//! before every world construction (a mismatch stops the process), every cap of `source::load` applies, and free
//! disk on the output volume must be at least 4 GB before anything is written. Files are kept in memory
//! (`MemSandbox`); only `export` writes rendered files (for the DuckDB cross-check, to a directory that is deleted
//! afterwards). Committed outputs hold only policy, seed, reward, energy and counts.
//!
//! `export`: for every tuning seed, every file of the pool and every task a life can meet (steps 0..6000 x every
//! kind; the task of (step, kind) does not depend on the clock), deduplicated, with the truth and the dialect.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::world_m3::baselines::{LinCfg, UcbCfg};
use being::world_m3::engine::ToolCache;
use being::world_m3::learner::LearnerTCfg;
use being::world_m3::measure::{run_life_cached, LifeT, Pol};
use being::world_m3::source::{self, col_name, Base};
use being::world_m3::tools::{run_plan, ToolCtx};
use being::world_m3::world::*;
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

const TUNE: (u64, u64) = (101, 120);
const REPORT: (u64, u64) = (401, 460);
const MIN_FREE_MB: u64 = 4096;

// ------------------------------------------------------------------ statistics (as measure_w2.rs)

fn mean(x: &[f64]) -> f64 {
    x.iter().sum::<f64>() / x.len().max(1) as f64
}
fn sd(x: &[f64]) -> f64 {
    let m = mean(x);
    (x.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (x.len() as f64 - 1.0).max(1.0)).sqrt()
}
fn ln_gamma(x: f64) -> f64 {
    const G: [f64; 6] = [76.18009172947146, -86.50532032941677, 24.01409824083091, -1.231739572450155, 0.1208650973866179e-2, -0.5395239384953e-5];
    let mut y = x;
    let tmp = x + 5.5;
    let tmp = tmp - (x + 0.5) * tmp.ln();
    let mut ser = 1.000000000190015;
    for g in G {
        y += 1.0;
        ser += g / y;
    }
    -tmp + (2.5066282746310005 * ser / x).ln()
}
fn betacf(a: f64, b: f64, x: f64) -> f64 {
    let (qab, qap, qam) = (a + b, a + 1.0, a - 1.0);
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < 1e-300 {
        d = 1e-300;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..300 {
        let m = m as f64;
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < 1e-300 {
            d = 1e-300;
        }
        c = 1.0 + aa / c;
        if c.abs() < 1e-300 {
            c = 1e-300;
        }
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < 1e-300 {
            d = 1e-300;
        }
        c = 1.0 + aa / c;
        if c.abs() < 1e-300 {
            c = 1e-300;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < 1e-15 {
            break;
        }
    }
    h
}
fn ibeta(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let bt = (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) { bt * betacf(a, b, x) / a } else { 1.0 - bt * betacf(b, a, 1.0 - x) / b }
}
fn t_cdf(t: f64, df: f64) -> f64 {
    let x = df / (df + t * t);
    let tail = 0.5 * ibeta(df / 2.0, 0.5, x);
    if t > 0.0 { 1.0 - tail } else { tail }
}
fn tq(df: usize) -> f64 {
    let (mut lo, mut hi) = (0.0, 50.0);
    for _ in 0..200 {
        let m = (lo + hi) / 2.0;
        if t_cdf(m, df as f64) < 0.975 { lo = m } else { hi = m }
    }
    (lo + hi) / 2.0
}
fn ci(x: &[f64]) -> (f64, f64, f64) {
    let m = mean(x);
    let h = tq(x.len() - 1) * sd(x) / (x.len() as f64).sqrt();
    (m, m - h, m + h)
}
fn diff(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter().zip(b).map(|(x, y)| x - y).collect()
}
fn wtl(d: &[f64]) -> (usize, usize, usize) {
    let w = d.iter().filter(|v| **v > 1e-12).count();
    let l = d.iter().filter(|v| **v < -1e-12).count();
    (w, d.len() - w - l, l)
}
fn paired(a: &[f64], b: &[f64]) -> String {
    let d = diff(a, b);
    let (m, lo, hi) = ci(&d);
    let (w, t, l) = wtl(&d);
    format!("{m:+.3} [{lo:+.3}, {hi:+.3}] ({w}/{t}/{l})")
}
/// One-sided p of "mean of d <= 0" against "mean > 0" (paired t).
fn p_le0(d: &[f64]) -> f64 {
    let n = d.len() as f64;
    let (m, s) = (mean(d), sd(d));
    if s == 0.0 {
        return if m > 0.0 { 0.0 } else { 1.0 };
    }
    1.0 - t_cdf(m / (s / n.sqrt()), n - 1.0)
}
/// One-sided p of "mean of d >= 0" against "mean < 0".
fn p_ge0(d: &[f64]) -> f64 {
    let n = d.len() as f64;
    let (m, s) = (mean(d), sd(d));
    if s == 0.0 {
        return if m < 0.0 { 0.0 } else { 1.0 };
    }
    t_cdf(m / (s / n.sqrt()), n - 1.0)
}
fn holm(p: &[f64], alpha: f64) -> Vec<bool> {
    let mut idx: Vec<usize> = (0..p.len()).collect();
    idx.sort_by(|a, b| p[*a].partial_cmp(&p[*b]).unwrap());
    let mut rej = vec![false; p.len()];
    for (k, &i) in idx.iter().enumerate() {
        if p[i] <= alpha / (p.len() - k) as f64 {
            rej[i] = true;
        } else {
            break;
        }
    }
    rej
}

#[cfg(windows)]
fn peak_mem() -> u64 {
    #[repr(C)]
    #[derive(Default)]
    struct Pmc {
        cb: u32,
        page_fault_count: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_nonpaged: usize,
        quota_nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(h: isize, p: *mut Pmc, cb: u32) -> i32;
    }
    let mut p = Pmc { cb: std::mem::size_of::<Pmc>() as u32, ..Default::default() };
    // SAFETY: p is a properly sized struct; the handle is the current process pseudo-handle.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut p, p.cb) };
    if ok != 0 { p.peak_working_set as u64 } else { 0 }
}
#[cfg(not(windows))]
fn peak_mem() -> u64 {
    0
}

#[cfg(windows)]
fn free_mb(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
    }
    let w: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let (mut a, mut t, mut f) = (0u64, 0u64, 0u64);
    // SAFETY: NUL-terminated wide string; valid out pointers.
    let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut a, &mut t, &mut f) };
    (ok != 0).then_some(a / (1024 * 1024))
}
#[cfg(not(windows))]
fn free_mb(_dir: &Path) -> Option<u64> {
    None
}

/// Stop (exit 2) when free disk on the volume of `dir` is below 4 GB (or unknown).
fn disk_guard(dir: &Path) {
    let probe = if dir.exists() { dir.to_path_buf() } else { dir.parent().map_or(PathBuf::from("."), |p| p.to_path_buf()) };
    match free_mb(&probe) {
        Some(f) if f >= MIN_FREE_MB => {}
        f => {
            eprintln!("STOP: free disk {f:?} MB is below {MIN_FREE_MB} MB; nothing is written");
            std::process::exit(2);
        }
    }
}

// ------------------------------------------------------------------ worlds

struct Src {
    path: PathBuf,
    base: Arc<Base>,
}

impl Src {
    fn open(args: &[String]) -> Src {
        let path = PathBuf::from(arg(args, "--source").expect("--source FILE"));
        let t0 = std::time::Instant::now();
        let base = source::load(&path).unwrap_or_else(|e| {
            eprintln!("STOP: the source is refused: {e}");
            std::process::exit(2)
        });
        eprintln!("# source checked and loaded in {} ms ({} rows; peak memory {} MB)", t0.elapsed().as_millis(), base.rows, peak_mem() / (1 << 20));
        Src { path, base: Arc::new(base) }
    }
    /// A world, after re-checking the file's SHA-256 (a mismatch stops the process).
    fn world(&self, cfg: M3Cfg) -> Arc<WorldT> {
        if let Err(e) = source::check_hash(&self.path) {
            eprintln!("STOP: {e}");
            std::process::exit(2);
        }
        Arc::new(WorldT::new(cfg, self.base.clone()))
    }
}

fn wcfg(seed: u64, penalty_pm: u32, cost_pm: u32) -> M3Cfg {
    M3Cfg { penalty_pm, cost_pm, ..M3Cfg::new(seed, DEFAULT_BUDGET) }
}

// ------------------------------------------------------------------ export (cross-check input)

fn esc(s: &str) -> String {
    s.replace('\t', " ").replace('\n', " ")
}

fn cmd_export(args: &[String]) {
    let src = Src::open(args);
    let out = PathBuf::from(arg(args, "--out").expect("--out DIR"));
    let steps: u64 = arg(args, "--steps").map_or(6000, |s| s.parse().unwrap());
    disk_guard(&out);
    std::fs::create_dir_all(&out).unwrap();
    let seeds: Vec<u64> = (TUNE.0..=TUNE.1).collect();
    let counts: Vec<(u64, usize, usize)> = seeds
        .par_iter()
        .map(|&s| {
            let w = src.world(wcfg(s, 0, 1000));
            let dir = out.join(format!("seed_{s}"));
            std::fs::create_dir_all(&dir).unwrap();
            let mut meta = std::fs::File::create(dir.join("files.meta.tsv")).unwrap();
            writeln!(meta, "name\tpool\tctx\tdelim\theader\tnull_tok\tdecimal_comma\tus_ts\tpad\tcrlf\tbom\tcols\tday\tbad_line\tzones\trows").unwrap();
            for f in &w.files {
                std::fs::write(dir.join(&f.name), &f.bytes).unwrap();
                match &f.logical {
                    Logical::Table(t) => {
                        let rd = t.rd;
                        let cols: Vec<&str> = t.cols.iter().map(|c| col_name(*c)).collect();
                        writeln!(meta, "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t\t{}", f.name, f.kind_pool, f.ctx.name(), rd.delim, rd.header as u8, if rd.null_tok.is_empty() { "<empty>" } else { rd.null_tok }, rd.decimal_comma as u8, rd.us_ts as u8, rd.pad as u8, rd.crlf as u8, rd.bom as u8, cols.join(","), t.day, t.bad.map_or(0, |b| b.0), t.rows.len()).unwrap();
                    }
                    Logical::Series { values, .. } => {
                        let z: Vec<String> = w.zones.iter().map(|z| z.to_string()).collect();
                        writeln!(meta, "{}\tseries\t{}\t44\t1\t<empty>\t0\t0\t0\t0\t0\t\t0\t0\t{}\t{}", f.name, f.ctx.name(), z.join(","), values.len()).unwrap();
                    }
                }
            }
            let mut seen: BTreeSet<String> = BTreeSet::new();
            let mut tf = std::fs::File::create(dir.join("tasks.tsv")).unwrap();
            writeln!(tf, "file\ttmpl\tparam\tc0\tc1\tc2\tc3\tprefix\ttol\ttruth").unwrap();
            let mut n = 0;
            for step in 0..steps {
                for k in KINDS {
                    let t = w.task_of(step, k);
                    let v = &t.view;
                    let line = format!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", v.file, v.tmpl.name(), esc(&v.param), v.cols[0], v.cols[1], v.cols[2], v.cols[3], v.prefix, v.tol, t.truth.text());
                    n += 1;
                    if seen.insert(line.clone()) {
                        writeln!(tf, "{line}").unwrap();
                    }
                }
            }
            (s, n, seen.len())
        })
        .collect();
    for (s, n, d) in counts {
        println!("seed {s}: {n} (step, kind) tasks, {d} distinct");
    }
}

// ------------------------------------------------------------------ policies and grids

const BETAS: [u32; 6] = [500, 1000, 2000, 4000, 8000, 16000];
const UCB_C: [f64; 6] = [0.001, 0.003, 0.01, 0.03, 0.1, 0.3];
const LIN_A: [f64; 6] = [0.1, 0.3, 1.0, 3.0, 10.0, 30.0];

fn b_cfg(beta: u32, file_key: bool, skip: bool) -> LearnerTCfg {
    LearnerTCfg { beta_milli: beta, file_key, skip, ..Default::default() }
}

/// A tuned policy family: name, grid values (as f64), and the policy of a value.
#[derive(Clone, Copy)]
enum Fam {
    B0,
    B1,
    Ucb,
    Lin,
    LinFile,
}

impl Fam {
    fn all() -> [Fam; 5] {
        [Fam::B0, Fam::B1, Fam::Ucb, Fam::Lin, Fam::LinFile]
    }
    fn name(self) -> &'static str {
        match self {
            Fam::B0 => "B0",
            Fam::B1 => "B1",
            Fam::Ucb => "UCB1",
            Fam::Lin => "LinUCB",
            Fam::LinFile => "LinUCB+file",
        }
    }
    fn grid(self) -> Vec<f64> {
        match self {
            Fam::B0 | Fam::B1 => BETAS.iter().map(|b| *b as f64 / 1000.0).collect(),
            Fam::Ucb => UCB_C.to_vec(),
            Fam::Lin | Fam::LinFile => LIN_A.to_vec(),
        }
    }
    /// The two values beyond an edge (grid ratio continued).
    fn extend(self, low: bool) -> Vec<f64> {
        let g = self.grid();
        let (a, b) = if low { (g[0], g[1]) } else { (g[5], g[4]) };
        let r = a / b;
        vec![a * r, a * r * r]
    }
    fn pol(self, x: f64, skip: bool) -> Pol {
        match self {
            Fam::B0 => Pol::Being(b_cfg((x * 1000.0).round() as u32, false, skip)),
            Fam::B1 => Pol::Being(b_cfg((x * 1000.0).round() as u32, true, skip)),
            Fam::Ucb => Pol::Ucb(UcbCfg { c: x, per_ctx: true, skip }),
            Fam::Lin => Pol::Lin(LinCfg { alpha: x, skip }),
            Fam::LinFile => Pol::LinFile(LinCfg { alpha: x, skip }),
        }
    }
}

const FIXED: [(&str, Pol); 3] = [("random", Pol::Random), ("always_sql", Pol::AlwaysSql), ("always_cheapest", Pol::Cheapest)];

fn life(w: &Arc<WorldT>, p: Pol, cache: &mut ToolCache) -> LifeT {
    run_life_cached(w, p, None, cache)
}

/// Mean reward per energy per (family, value) over the seeds, at one reward design point.
fn tune_point(src: &Src, seeds: &[u64], pen: u32, cost: u32, fams: &[(Fam, Vec<f64>)]) -> Vec<(u64, String, f64, f64, i64, i64)> {
    seeds
        .par_iter()
        .flat_map_iter(|&s| {
            let w = src.world(wcfg(s, pen, cost));
            let mut cache = ToolCache::new();
            let mut rows = vec![];
            for (f, vals) in fams {
                for x in vals {
                    let l = life(&w, f.pol(*x, true), &mut cache);
                    rows.push((s, f.name().to_string(), *x, l.rpe(), l.reward, l.cost));
                }
            }
            for (n, p) in FIXED {
                let l = life(&w, p, &mut cache);
                rows.push((s, n.to_string(), 0.0, l.rpe(), l.reward, l.cost));
            }
            rows
        })
        .collect()
}

/// Per family: (chosen value, its mean, at an edge?) from tuning rows.
fn choose(rows: &[(u64, String, f64, f64, i64, i64)], fam: &str) -> (f64, f64) {
    let mut m: BTreeMap<String, (f64, Vec<f64>)> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.1 == fam) {
        m.entry(format!("{:020.6}", r.2)).or_insert((r.2, vec![])).1.push(r.3);
    }
    let mut best = (0.0, f64::NEG_INFINITY);
    for (x, v) in m.values() {
        let mm = mean(v);
        if mm > best.1 {
            best = (*x, mm);
        }
    }
    best
}

fn tune_full(src: &Src, seeds: &[u64], pen: u32, cost: u32) -> (Vec<(u64, String, f64, f64, i64, i64)>, Vec<String>) {
    let fams: Vec<(Fam, Vec<f64>)> = Fam::all().iter().map(|f| (*f, f.grid())).collect();
    let mut rows = tune_point(src, seeds, pen, cost, &fams);
    let mut ext_notes = vec![];
    // one edge extension per family whose optimum lies on a grid edge
    let mut ext: Vec<(Fam, Vec<f64>)> = vec![];
    for f in Fam::all() {
        let (x, _) = choose(&rows, f.name());
        let g = f.grid();
        if (x - g[0]).abs() < 1e-12 || (x - g[5]).abs() < 1e-12 {
            let low = (x - g[0]).abs() < 1e-12;
            let e = f.extend(low);
            ext_notes.push(format!("{}: optimum {x} on the {} edge; extended by {:?}", f.name(), if low { "low" } else { "high" }, e));
            ext.push((f, e));
        }
    }
    if !ext.is_empty() {
        let more = tune_point(src, seeds, pen, cost, &ext);
        rows.extend(more.into_iter().filter(|r| !FIXED.iter().any(|f| f.0 == r.1)));
    }
    (rows, ext_notes)
}

fn cmd_tune(args: &[String]) {
    let src = Src::open(args);
    let out = PathBuf::from(arg(args, "--out").expect("--out FILE"));
    disk_guard(out.parent().unwrap_or(Path::new(".")));
    let seeds: Vec<u64> = (TUNE.0..=TUNE.1).collect();
    let t0 = std::time::Instant::now();
    let (rows, notes) = tune_full(&src, &seeds, 0, 1000);
    let mut f = std::fs::File::create(&out).unwrap();
    writeln!(f, "policy\tvalue\tseed\treward\tenergy\treward_per_energy").unwrap();
    for r in &rows {
        writeln!(f, "{}\t{}\t{}\t{}\t{}\t{:.6}", r.1, r.2, r.0, r.4, r.5, r.3).unwrap();
    }
    println!("World M3 tuning: seeds {}..{}, budget {DEFAULT_BUDGET}, penalty 0, energy weight 1; wall {:.0} s; peak memory {} MB", TUNE.0, TUNE.1, t0.elapsed().as_secs_f64(), peak_mem() / (1 << 20));
    for n in &notes {
        println!("edge extension: {n}");
    }
    println!("| policy | value | mean reward per energy (20 seeds) |");
    println!("|---|---|---|");
    let mut fams: Vec<String> = rows.iter().map(|r| r.1.clone()).collect();
    fams.dedup();
    let mut seen = BTreeSet::new();
    for fam in fams {
        if !seen.insert(fam.clone()) {
            continue;
        }
        let mut vals: Vec<f64> = rows.iter().filter(|r| r.1 == fam).map(|r| r.2).collect();
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        vals.dedup();
        let (bx, _) = choose(&rows, &fam);
        for x in vals {
            let v: Vec<f64> = rows.iter().filter(|r| r.1 == fam && r.2 == x).map(|r| r.3).collect();
            println!("| {fam} | {x} | {:.4}{} |", mean(&v), if x == bx && !FIXED.iter().any(|f| f.0 == fam) { " (chosen)" } else { "" });
        }
    }
    let base: Vec<&str> = vec!["random", "always_sql", "always_cheapest", "UCB1", "LinUCB", "LinUCB+file"];
    let mut strongest = ("", f64::NEG_INFINITY);
    for b in &base {
        let (_, m) = choose(&rows, b);
        if m > strongest.1 {
            strongest = (b, m);
        }
    }
    println!("strongest baseline on the tuning seeds (named before the report run): {} ({:.4})", strongest.0, strongest.1);
}

// ------------------------------------------------------------------ report

struct Tuned {
    vals: BTreeMap<String, f64>,
    strongest: String,
}

fn read_tuned(p: &Path) -> Tuned {
    let text = std::fs::read_to_string(p).expect("the tuning TSV");
    let mut rows = vec![];
    for l in text.lines().skip(1) {
        let f: Vec<&str> = l.split('\t').collect();
        rows.push((f[2].parse::<u64>().unwrap(), f[0].to_string(), f[1].parse::<f64>().unwrap(), f[5].parse::<f64>().unwrap(), f[3].parse::<i64>().unwrap(), f[4].parse::<i64>().unwrap()));
    }
    let mut vals = BTreeMap::new();
    for f in Fam::all() {
        vals.insert(f.name().to_string(), choose(&rows, f.name()).0);
    }
    let mut strongest = (String::new(), f64::NEG_INFINITY);
    for b in ["random", "always_sql", "always_cheapest", "UCB1", "LinUCB", "LinUCB+file"] {
        let (_, m) = choose(&rows, b);
        if m > strongest.1 {
            strongest = (b.to_string(), m);
        }
    }
    Tuned { vals, strongest: strongest.0 }
}

#[derive(Clone, Default)]
struct Mx {
    rpe: f64,
    reward: i64,
    cost: i64,
    tasks: usize,
    answered: usize,
    right: usize,
    wrong: usize,
    skips: usize,
    best_sum: f64,
    rules: usize,
    wrong_accepted: usize,
    wall_ms: u64,
}

fn mx(l: &LifeT, wall_ms: u64) -> Mx {
    let t = &l.truths;
    Mx {
        rpe: l.rpe(),
        reward: l.reward,
        cost: l.cost,
        tasks: t.len(),
        answered: t.iter().filter(|x| x.answered).count(),
        right: t.iter().filter(|x| x.correct).count(),
        wrong: t.iter().filter(|x| x.wrong).count(),
        skips: t.iter().filter(|x| x.plan.is_none()).count(),
        best_sum: t.iter().map(|x| x.best_rpe).sum(),
        rules: l.rules,
        wrong_accepted: t.iter().filter(|x| x.wrong_accepted).count(),
        wall_ms,
    }
}

fn cmd_report(args: &[String]) {
    let src = Src::open(args);
    let tuned = read_tuned(Path::new(&arg(args, "--tuned").expect("--tuned FILE")));
    let out = PathBuf::from(arg(args, "--out-dir").expect("--out-dir DIR"));
    disk_guard(&out);
    std::fs::create_dir_all(&out).unwrap();
    let v = |k: &str| tuned.vals[k];
    let pols: Vec<(String, Pol)> = vec![
        ("B0".into(), Fam::B0.pol(v("B0"), true)),
        ("B1".into(), Fam::B1.pol(v("B1"), true)),
        ("random".into(), Pol::Random),
        ("always_sql".into(), Pol::AlwaysSql),
        ("always_cheapest".into(), Pol::Cheapest),
        ("UCB1".into(), Fam::Ucb.pol(v("UCB1"), true)),
        ("LinUCB".into(), Fam::Lin.pol(v("LinUCB"), true)),
        ("LinUCB+file".into(), Fam::LinFile.pol(v("LinUCB+file"), true)),
        ("reward_best_oracle".into(), Pol::RewardBest),
        ("B0_skip_off".into(), Fam::B0.pol(v("B0"), false)),
        ("UCB1_skip_off".into(), Fam::Ucb.pol(v("UCB1"), false)),
        ("LinUCB_skip_off".into(), Fam::Lin.pol(v("LinUCB"), false)),
    ];
    let seeds: Vec<u64> = (REPORT.0..=REPORT.1).collect();
    println!("World M3 REPORT run: seeds {}..{} (n = {}), budget {DEFAULT_BUDGET}, penalty 0, energy weight 1; tuned values {:?}; strongest baseline named before this run: {}", REPORT.0, REPORT.1, seeds.len(), tuned.vals, tuned.strongest);
    let t0 = std::time::Instant::now();
    let run = |ood: bool| -> Vec<(u64, Vec<Mx>)> {
        let mut r: Vec<(u64, Vec<Mx>)> = seeds
            .par_iter()
            .map(|&s| {
                let w = src.world(M3Cfg { ood, ..wcfg(s, 0, 1000) });
                let mut cache = ToolCache::new();
                (s, pols.iter().map(|(_, p)| {
                    let t = std::time::Instant::now();
                    let l = life(&w, *p, &mut cache);
                    mx(&l, t.elapsed().as_millis() as u64)
                }).collect())
            })
            .collect();
        r.sort_by_key(|x| x.0);
        r
    };
    let main = run(false);
    let main_wall = t0.elapsed().as_secs_f64();
    let ood = run(true);
    let names: Vec<&str> = pols.iter().map(|p| p.0.as_str()).collect();
    for (tag, res) in [("report", &main), ("ood", &ood)] {
        let mut f = std::fs::File::create(out.join(format!("m3_{tag}_seeds401-460.tsv"))).unwrap();
        writeln!(f, "policy\tseed\treward\tenergy\treward_per_energy\ttasks\tanswered\tright\twrong\tskips\trules").unwrap();
        for (s, ms) in res.iter() {
            for (n, m) in names.iter().zip(ms) {
                writeln!(f, "{n}\t{s}\t{}\t{}\t{:.6}\t{}\t{}\t{}\t{}\t{}\t{}", m.reward, m.cost, m.rpe, m.tasks, m.answered, m.right, m.wrong, m.skips, m.rules).unwrap();
            }
        }
    }
    let col = |res: &Vec<(u64, Vec<Mx>)>, n: &str| -> Vec<f64> {
        let i = names.iter().position(|x| *x == n).unwrap();
        res.iter().map(|r| r.1[i].rpe).collect()
    };
    let b0 = col(&main, "B0");
    let six = ["random", "always_sql", "always_cheapest", "UCB1", "LinUCB", "LinUCB+file"];
    println!("\nMean reward per energy (report seeds):");
    for n in &names {
        let x = col(&main, n);
        let (m, lo, hi) = ci(&x);
        println!("  {n}: {m:.4} [{lo:.4}, {hi:.4}]");
    }
    let sb = col(&main, &tuned.strongest);
    let d = diff(&b0, &sb);
    let (m, lo, hi) = ci(&d);
    let (w, t, l) = wtl(&d);
    let primary = lo > 0.0 && m >= 0.05 && w >= 36;
    println!("\nPRIMARY: B0 - {} = {m:+.4} [{lo:+.4}, {hi:+.4}], W/T/L {w}/{t}/{l}; threshold: lower bound > 0 AND mean >= +0.05 AND >= 36 wins: {}", tuned.strongest, if primary { "MET" } else { "NOT MET" });
    println!("\nS1: B0 minus each baseline (one-sided paired t of 'B0 - baseline <= 0', Holm over six at 0.05):");
    let ps: Vec<f64> = six.iter().map(|b| p_le0(&diff(&b0, &col(&main, b)))).collect();
    let rej = holm(&ps, 0.05);
    for (i, b) in six.iter().enumerate() {
        println!("  B0 - {b}: {}  p = {:.2e}  rejected: {}", paired(&b0, &col(&main, b)), ps[i], rej[i]);
    }
    let s1 = rej.iter().all(|x| *x);
    println!("S1 holds: {s1}");
    println!("M3 MET: {}", primary && s1);
    println!("\nS2: per policy (report seeds, pooled): wrong answers per 100 tasks / coverage (answered / tasks) / skip share");
    for (i, n) in names.iter().enumerate() {
        let (tk, an, wr, sk): (usize, usize, usize, usize) = main.iter().fold((0, 0, 0, 0), |a, r| (a.0 + r.1[i].tasks, a.1 + r.1[i].answered, a.2 + r.1[i].wrong, a.3 + r.1[i].skips));
        println!("  {n}: {:.2} / {:.3} / {:.3}", 100.0 * wr as f64 / tk.max(1) as f64, an as f64 / tk.max(1) as f64, sk as f64 / tk.max(1) as f64);
    }
    let orc = col(&main, "reward_best_oracle");
    let gap: Vec<f64> = b0.iter().zip(&orc).zip(&sb).map(|((b, o), s)| if (o - s).abs() > 1e-12 { (b - s) / (o - s) } else { 0.0 }).collect();
    println!("\nS3: share of the gap (strongest baseline -> reward-best oracle) closed by B0: mean {:.3} (pooled {:.3})", mean(&gap), (mean(&b0) - mean(&sb)) / (mean(&orc) - mean(&sb)));
    println!("\nS4: B1 - B0 = {}", paired(&col(&main, "B1"), &b0));
    println!("\nS7 (OOD, never tuned): B0 minus each baseline");
    let b0o = col(&ood, "B0");
    for b in six {
        println!("  B0 - {b}: {}", paired(&b0o, &col(&ood, b)));
    }
    let wa: usize = main.iter().chain(ood.iter()).map(|r| r.1.iter().map(|m| m.wrong_accepted).sum::<usize>()).sum();
    let rules: usize = main.iter().map(|r| r.1[0].rules).sum();
    let walls: Vec<f64> = main.iter().map(|r| r.1[0].wall_ms as f64).collect();
    println!("\nS8: wall {main_wall:.0} s for the report lives ({} lives); B0 life mean {:.0} ms; peak memory {} MB; base table load in the process start line", seeds.len() * pols.len(), mean(&walls), peak_mem() / (1 << 20));
    println!("S9: rules accepted by B0 over the 60 report lives: {rules}; wrong answers paid: {wa}");
}

// ------------------------------------------------------------------ S6 sweep

fn cmd_sweep(args: &[String]) {
    let src = Src::open(args);
    let out = PathBuf::from(arg(args, "--out").expect("--out FILE"));
    disk_guard(out.parent().unwrap_or(Path::new(".")));
    let tseeds: Vec<u64> = (TUNE.0..=TUNE.1).collect();
    let rseeds: Vec<u64> = (REPORT.0..=REPORT.1).collect();
    let mut f = std::fs::File::create(&out).unwrap();
    writeln!(f, "penalty_pm\tcost_pm\tpolicy\tvalue\tseed\treward_per_energy").unwrap();
    let mut ps = vec![];
    let mut lines = vec![];
    for pen in [0u32, 1000, 10000] {
        for cost in [316u32, 1000, 3162] {
            let (rows, _) = tune_full(&src, &tseeds, pen, cost);
            let fams = [Fam::B0, Fam::Ucb, Fam::Lin, Fam::LinFile];
            let vals: Vec<f64> = fams.iter().map(|fm| choose(&rows, fm.name()).0).collect();
            let mut strongest = ("", f64::NEG_INFINITY);
            for b in ["random", "always_sql", "always_cheapest", "UCB1", "LinUCB", "LinUCB+file"] {
                let (_, m) = choose(&rows, b);
                if m > strongest.1 {
                    strongest = (b, m);
                }
            }
            let sp = match strongest.0 {
                "random" => Pol::Random,
                "always_sql" => Pol::AlwaysSql,
                "always_cheapest" => Pol::Cheapest,
                "UCB1" => Fam::Ucb.pol(vals[1], true),
                "LinUCB" => Fam::Lin.pol(vals[2], true),
                _ => Fam::LinFile.pol(vals[3], true),
            };
            let bp = Fam::B0.pol(vals[0], true);
            let mut res: Vec<(u64, f64, f64)> = rseeds
                .par_iter()
                .map(|&s| {
                    let w = src.world(wcfg(s, pen, cost));
                    let mut cache = ToolCache::new();
                    (s, life(&w, bp, &mut cache).rpe(), life(&w, sp, &mut cache).rpe())
                })
                .collect();
            res.sort_by_key(|x| x.0);
            for r in &res {
                writeln!(f, "{pen}\t{cost}\tB0\t{}\t{}\t{:.6}", vals[0], r.0, r.1).unwrap();
                writeln!(f, "{pen}\t{cost}\t{}\t-\t{}\t{:.6}", strongest.0, r.0, r.2).unwrap();
            }
            let a: Vec<f64> = res.iter().map(|r| r.1).collect();
            let b: Vec<f64> = res.iter().map(|r| r.2).collect();
            ps.push(p_ge0(&diff(&a, &b)));
            lines.push(format!("| {:.3} | {:.3} | B0 beta {} vs {} | {} |", pen as f64 / 1000.0, cost as f64 / 1000.0, vals[0], strongest.0, paired(&a, &b)));
            eprintln!("# sweep point {pen} {cost} done");
        }
    }
    let rej = holm(&ps, 0.05);
    println!("S6 sweep (re-tuned per point on 101..120; report seeds 401..460): B0 minus the best baseline");
    println!("| penalty x reward | energy weight | comparison | B0 - best [95% CI] (W/T/L) | Holm regression |");
    for (l, r) in lines.iter().zip(&rej) {
        println!("{l} {r} |");
    }
}

// ------------------------------------------------------------------ S5 determinism (tuning seeds)

fn cmd_determinism(args: &[String]) {
    let src = Src::open(args);
    let steps: u64 = arg(args, "--steps").map_or(500, |s| s.parse().unwrap());
    let seeds: Vec<u64> = (TUNE.0..=TUNE.1).collect();
    // per (template name, plan name): (keys, fixed keys, attempts, right)
    let per: Vec<BTreeMap<(String, String), (usize, usize, usize, usize)>> = seeds
        .par_iter()
        .map(|&s| {
            let w = src.world(wcfg(s, 0, 1000));
            let mut keys: BTreeMap<(usize, Tmpl, Plan), (usize, usize)> = BTreeMap::new();
            let mut seen = BTreeSet::new();
            for step in 0..steps {
                for k in KINDS {
                    let t = w.task_of(step, k);
                    if !seen.insert((t.file_idx, t.view.tmpl, t.view.param.clone(), t.view.cols.clone(), t.view.prefix)) {
                        continue;
                    }
                    let bytes = w.view_bytes(&t.view, &w.files[t.file_idx].bytes).to_vec();
                    for p in plans_for(t.view.tmpl) {
                        if *p == Plan::TsRule {
                            continue;
                        }
                        let o = run_plan(*p, &t.view, &bytes, &ToolCtx { rules: &[], sql: Default::default() });
                        let ok = o.answer.as_deref().map_or(false, |a| t.check(a));
                        let e = keys.entry((t.file_idx, t.view.tmpl, *p)).or_default();
                        e.0 += 1;
                        e.1 += ok as usize;
                    }
                }
            }
            let mut m: BTreeMap<(String, String), (usize, usize, usize, usize)> = BTreeMap::new();
            for ((_, tm, p), (n, ok)) in keys {
                let e = m.entry((tm.name().to_string(), p.name().to_string())).or_default();
                e.0 += 1;
                e.1 += (ok == 0 || ok == n) as usize;
                e.2 += n;
                e.3 += ok;
            }
            m
        })
        .collect();
    let mut tot: BTreeMap<(String, String), (usize, usize, usize, usize)> = BTreeMap::new();
    for m in per {
        for (k, v) in m {
            let e = tot.entry(k).or_default();
            e.0 += v.0;
            e.1 += v.1;
            e.2 += v.2;
            e.3 += v.3;
        }
    }
    let (k, fx): (usize, usize) = tot.values().fold((0, 0), |a, v| (a.0 + v.0, a.1 + v.1));
    println!("S5 (tuning seeds 101..120, distinct tasks of steps 0..{steps} x every kind): (template, file, plan) keys {k}, outcome the same on every task of the key {fx} = {:.1}%", 100.0 * fx as f64 / k.max(1) as f64);
    println!("| template | plan | keys | determined keys | attempts | right share |");
    println!("|---|---|---|---|---|---|");
    for ((t, p), v) in &tot {
        println!("| {t} | {p} | {} | {} | {} | {:.3} |", v.0, v.1, v.2, v.3 as f64 / v.2.max(1) as f64);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("export") => cmd_export(&args),
        Some("tune") => cmd_tune(&args),
        Some("report") => cmd_report(&args),
        Some("sweep") => cmd_sweep(&args),
        Some("determinism") => cmd_determinism(&args),
        Some("time") => {
            let src = Src::open(&args);
            let w = src.world(wcfg(101, 0, 1000));
            let mut cache = ToolCache::new();
            for p in [Fam::B0.pol(2.0, true), Pol::Cheapest, Fam::Lin.pol(1.0, true)] {
                let t = std::time::Instant::now();
                let l = life(&w, p, &mut cache);
                println!("{}: rpe {:.4}, {} tasks, {} ms", p.label(), l.rpe(), l.truths.len(), t.elapsed().as_millis());
            }
        }
        _ => eprintln!("usage: measure_m3 export|tune|report|sweep|determinism|time --source FILE ..."),
    }
}
