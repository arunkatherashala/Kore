//! Track W2 (docs/b1_default_prereg.md): the file-keyed estimator B1 in the World T being. Measurement only.
//!   cargo run -p kore-self --example measure_w2 --offline --release -- equiv   [tuning seeds 101..120 only]
//!   cargo run -p kore-self --example measure_w2 --offline --release -- primary --out FILE.tsv   [report seeds 201..260]
//!   cargo run -p kore-self --example measure_w2 --offline --release -- cost --pol b0|b1   [report seeds 201..260, one thread]
//!   cargo run -p kore-self --example measure_w2 --offline --release -- sweep --out-dir DIR   [tune 101..120, report 201..260]
//!   cargo run -p kore-self --example measure_w2 --offline --release -- lincheck   [plain LinUCB unchanged: Gate 0 B2 rows]
//!
//! `equiv`: the pre-registered equivalence test. On every tuning seed 101..120 (World T, penalty 0, energy weight 1,
//! budget 6000) it runs (1) the Gate 0 harness B1 (`FileKeyed` and `life_t`, copied verbatim from
//! `examples/measure_n1diag.rs` at d34657b7) with beta 2, (2) the in-being B1 (`LearnerTCfg::file_key`) in the same
//! measurement loop, (3) the in-being B1 as a logged being (`run_t_key`, a log on disk) in one run, (4) the same
//! with a restart in the middle of the life from the full fold of the log, and (5) the same with the restart from
//! a checkpoint (Track D1b; the restore must use it). It asserts the same decision (plan or skip) at every step and
//! the same reward and energy, and that the harness's reward per energy equals the Gate 0 tuning row
//! (`docs/data/n1_gate0_tune_seeds101-120.tsv`, column `b1_beta2000`, 6 decimals).
//!
//! Files are the generator's bytes in memory (`MemSandbox`, as Gate 0). Rows are printed tab-separated; summary
//! lines start with '#'. Statistics: paired t-intervals (95%, t quantile computed for n - 1 degrees of freedom).

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::policy::Policy;
use being::world_t::baselines::{LinCfg, UcbCfg};
use being::world_t::engine::{run_t_key, Cell, ChooserT, EngineT, MemSandbox, RunTCfg, StateT, ToolCache, TrueT, DEFAULT_DISCOVER_EVERY};
use being::world_t::learner::{ChoiceT, LearnerT, LearnerTCfg};
use being::world_t::measure::{run_life_cached, LifeT, Pol};
use being::world_t::world::{Plan, ViewT, WTCfg, WorldT, DEFAULT_BUDGET};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

const TUNE: (u64, u64) = (101, 120);
const REPORT: (u64, u64) = (201, 260);
const B1_BETA: u32 = 2000;

fn b0_cfg() -> LearnerTCfg {
    LearnerTCfg::default()
}
fn b1_cfg() -> LearnerTCfg {
    LearnerTCfg { beta_milli: B1_BETA, file_key: true, ..Default::default() }
}

// ------------------------------------------------------------------ statistics

fn mean(x: &[f64]) -> f64 {
    x.iter().sum::<f64>() / x.len().max(1) as f64
}
fn sd(x: &[f64]) -> f64 {
    let m = mean(x);
    (x.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (x.len() as f64 - 1.0).max(1.0)).sqrt()
}

/// ln Gamma (Lanczos).
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

/// Regularized incomplete beta I_x(a, b).
fn ibeta(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let bt = (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        bt * betacf(a, b, x) / a
    } else {
        1.0 - bt * betacf(b, a, 1.0 - x) / b
    }
}

/// P(T <= t) for Student's t with `df` degrees of freedom.
fn t_cdf(t: f64, df: f64) -> f64 {
    let x = df / (df + t * t);
    let tail = 0.5 * ibeta(df / 2.0, 0.5, x);
    if t > 0.0 { 1.0 - tail } else { tail }
}

/// Two-sided 95% t quantile.
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

/// "mean [lo, hi] (W/T/L)" of a - b, paired.
fn paired(a: &[f64], b: &[f64]) -> String {
    let d = diff(a, b);
    let (m, lo, hi) = ci(&d);
    let (w, t, l) = wtl(&d);
    format!("{m:+.3} [{lo:+.3}, {hi:+.3}] ({w}/{t}/{l})")
}

/// One-sided p of "mean of d < 0" (paired t).
fn p_below0(d: &[f64]) -> f64 {
    let n = d.len() as f64;
    let (m, s) = (mean(d), sd(d));
    if s == 0.0 {
        return if m < 0.0 { 0.0 } else { 1.0 };
    }
    t_cdf(m / (s / n.sqrt()), n - 1.0)
}

/// Holm step-down at family-wise level `alpha`: which hypotheses are rejected.
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

/// Peak working set of this process in bytes (Windows; 0 elsewhere).
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
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut p, p.cb) };
    if ok != 0 { p.peak_working_set as u64 } else { 0 }
}
#[cfg(not(windows))]
fn peak_mem() -> u64 {
    0
}

// ------------------------------------------------------------------ the Gate 0 harness (verbatim copy)

#[derive(Default, Clone, Copy)]
struct GateLife {
    reward: i64,
    cost: i64,
}

impl GateLife {
    fn rpe(&self) -> f64 {
        self.reward as f64 / self.cost.max(1) as f64
    }
}

/// B1, the file-keyed being of Gate 0 (copied verbatim from examples/measure_n1diag.rs at d34657b7; only the
/// comment shortened): the being's own Beta estimator per (template, file, plan), its prior the being's cell
/// estimate; the file counts are the cell's own increments from the action on that file.
struct FileKeyed {
    l: LearnerT,
    files: HashMap<(usize, String, usize), Cell>,
    pending: Option<((usize, String, usize), (u8, u8, u8), Cell)>,
}

impl FileKeyed {
    fn new(cfg: LearnerTCfg) -> FileKeyed {
        FileKeyed { l: LearnerT { cfg }, files: HashMap::new(), pending: None }
    }
}

impl ChooserT for FileKeyed {
    fn name(&self) -> String {
        "b1-file-keyed (measurement only)".into()
    }
    fn choose(&mut self, st: &StateT, v: &ViewT) -> ChoiceT {
        if let Some((fk, ck, before)) = self.pending.take() {
            let after = st.cells.get(&ck).copied().unwrap_or_default();
            if after.n > before.n {
                let e = self.files.entry(fk).or_default();
                e.n += after.n - before.n;
                e.succ += after.succ - before.succ;
                e.wrong += after.wrong - before.wrong;
                e.spent += after.spent - before.spent;
            }
        }
        let (l, files) = (&self.l, &self.files);
        let ch = l.choose_with(st, v, |p| {
            let ce = l.estimate(st, v, p);
            let fc = files.get(&(v.tmpl.idx(), v.file.clone(), p.idx())).copied().unwrap_or_default();
            l.estimate_from(v, p, fc, (ce.p, ce.p_wrong, "cell estimate"), ce.energy)
        });
        if let Some(p) = ch.plan {
            let ck = (v.tmpl.idx() as u8, v.ctx.idx() as u8, p.idx() as u8);
            self.pending = Some(((v.tmpl.idx(), v.file.clone(), p.idx()), ck, st.cells.get(&ck).copied().unwrap_or_default()));
        }
        ch
    }
}

// ------------------------------------------------------------------ one measured life (the Gate 0 loop `life_t`)

/// A life in the Gate 0 measurement loop (`life_t` of measure_n1diag: start state, engine with `measure`, a B2
/// discovery pass every 10 steps), with every step's truth and file kept.
struct Life {
    truths: Vec<TrueT>,
    files: Vec<String>,
    reward: i64,
    cost: i64,
    file_keys: usize,
    rules: usize,
    rules_noisy: usize,
    wall: f64,
}

impl Life {
    fn rpe(&self) -> f64 {
        self.reward as f64 / self.cost.max(1) as f64
    }
    fn plans(&self) -> Vec<Option<Plan>> {
        self.truths.iter().map(|t| t.plan).collect()
    }
}

fn life(cfg: &WTCfg, ch: &mut dyn ChooserT, lcfg: LearnerTCfg) -> Life {
    let t0 = std::time::Instant::now();
    let mut st = StateT::default();
    st.start(cfg.clone(), lcfg, "genesis".into());
    let mut eng = EngineT::new(st, None);
    eng.measure = true;
    let world = WorldT::new(cfg.clone());
    let mem = MemSandbox(&world);
    let (mut truths, mut files, mut reward, mut cost) = (vec![], vec![], 0i64, 0i64);
    while let Ok(s) = eng.step(ch, &mem) {
        reward += s.truth.reward;
        cost += s.truth.cost;
        let f = s.events.iter().filter(|e| e.kind == "action").flat_map(|e| e.tags.iter()).find_map(|t| t.strip_prefix("file:")).unwrap_or("").to_string();
        files.push(f);
        truths.push(s.truth);
        if eng.state.step % DEFAULT_DISCOVER_EVERY == 0 {
            eng.discover_pass();
        }
    }
    let rules = eng.state.rules.len();
    let rules_noisy = eng.state.rules.iter().filter(|r| r.sig >= 120).count();
    Life { truths, files, reward, cost, file_keys: eng.state.file_keys(), rules, rules_noisy, wall: t0.elapsed().as_secs_f64() }
}

// ------------------------------------------------------------------ equivalence test

fn tune_rows() -> HashMap<u64, String> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("docs").join("data").join("n1_gate0_tune_seeds101-120.tsv");
    let text = std::fs::read_to_string(&p).expect("docs/data/n1_gate0_tune_seeds101-120.tsv");
    let mut lines = text.lines();
    let head: Vec<&str> = lines.next().unwrap().split('\t').collect();
    let k = head.iter().position(|h| *h == "b1_beta2000").expect("column b1_beta2000");
    let k0 = head.iter().position(|h| *h == "b0").expect("column b0");
    lines.filter(|l| !l.starts_with('#') && !l.trim().is_empty()).map(|l| {
        let c: Vec<&str> = l.split('\t').collect();
        (c[0].parse().unwrap(), format!("{}\t{}", c[k0], c[k]))
    }).collect()
}

fn open_log(root: &std::path::Path) -> versioned::VersionedLog {
    versioned::VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

/// The logged being (`run_t_key`, key on) in runs of `chunks` steps (the last chunk runs to the end of the
/// budget), restarting from the log between chunks; `every` > 0: checkpoints on. Returns the truths of all steps,
/// whether every restart used a checkpoint, and the decisions replayed / mismatched on the final log.
fn logged(cfg: &WTCfg, chunks: &[u64], every: u64) -> (Vec<TrueT>, bool, (u64, u64)) {
    let d = tempfile::tempdir().unwrap();
    let w = WorldT::new(cfg.clone());
    let mut truths = vec![];
    let mut all_ckpt = true;
    for (i, &n) in chunks.iter().enumerate() {
        let mut log = open_log(d.path());
        let mut p = Policy::new(n);
        p.checkpoint_every = every;
        let rc = RunTCfg { world: Some(cfg.clone()), learner: Some(b0_cfg()), discover_every: DEFAULT_DISCOVER_EVERY, ctl: None, sql: None };
        let s = run_t_key(&mut log, &rc, Some(true), &p, None, &MemSandbox(&w)).unwrap();
        if i > 0 {
            all_ckpt &= s.ckpt.restored.from_version.is_some();
        }
        if every > 0 && i + 1 < chunks.len() {
            assert!(s.ckpt.written.is_some(), "a checkpoint is written at the end of chunk {i}: {:?}", s.ckpt);
        }
        truths.extend(s.truths);
    }
    let log = open_log(d.path());
    let rows: Vec<versioned::EventRow> = log.open_latest().rows();
    let rp = being::world_t::engine::replay_t(&rows);
    (truths, all_ckpt, rp)
}

fn cmd_equiv() {
    let t0 = std::time::Instant::now();
    let rows = tune_rows();
    let ss: Vec<u64> = (TUNE.0..=TUNE.1).collect();
    let out: Vec<(u64, String, bool)> = ss
        .par_iter()
        .map(|&s| {
            let cfg = WTCfg::new(s, DEFAULT_BUDGET);
            let b0 = life(&cfg, &mut LearnerT { cfg: b0_cfg() }, b0_cfg());
            let gb1 = { let c = LearnerTCfg { beta_milli: B1_BETA, ..Default::default() }; life(&cfg, &mut FileKeyed::new(c), c) };
            let ib1 = life(&cfg, &mut LearnerT { cfg: b1_cfg() }, b1_cfg());
            let n = ib1.truths.len() as u64;
            let mid = (n / 2).max(100);
            let big = 1_000_000u64;
            let (one, _, rp1) = logged(&cfg, &[big], 0);
            let (fold, _, rp2) = logged(&cfg, &[mid, big], 0);
            let (ck, used, rp3) = logged(&cfg, &[mid, big], 100);
            let sum = |t: &[TrueT]| (t.iter().map(|x| x.reward).sum::<i64>(), t.iter().map(|x| x.cost).sum::<i64>());
            let plans = |t: &[TrueT]| t.iter().map(|x| x.plan).collect::<Vec<_>>();
            let gp = gb1.plans();
            let same = [plans(&ib1.truths) == gp, plans(&one) == gp, plans(&fold) == gp, plans(&ck) == gp];
            let rc = [(ib1.reward, ib1.cost), sum(&one), sum(&fold), sum(&ck)].iter().all(|x| *x == (gb1.reward, gb1.cost));
            let first_diff = plans(&ib1.truths).iter().zip(&gp).position(|(a, b)| a != b);
            let tsv = rows.get(&s).cloned().unwrap_or_default();
            let mine = format!("{:.6}\t{:.6}", b0.rpe(), gb1.rpe());
            let tsv_ok = tsv == mine;
            let rp_ok = [rp1, rp2, rp3].iter().all(|(n, bad)| *n > 0 && *bad == 0);
            let ok = same.iter().all(|x| *x) && rc && tsv_ok && used && rp_ok;
            let line = format!(
                "{s}\t{}\t{:.6}\t{:.6}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:?}\t{}",
                n, gb1.rpe(), ib1.rpe(), tsv.replace('\t', "/"), tsv_ok, same[0], same[1], same[2], same[3], rc, used, rp_ok, first_diff, ok
            );
            eprintln!("world {s} done at {:.0} s", t0.elapsed().as_secs_f64());
            (s, line, ok)
        })
        .collect();
    println!("seed\tsteps\tharness_b1_rpe\tin_being_b1_rpe\tgate0_tsv_b0/b1_beta2000\ttsv_equal\tsame_decisions_in_being\tsame_logged_one_run\tsame_logged_restart_fold\tsame_logged_restart_checkpoint\tsame_reward_and_energy_all\tcheckpoint_used_at_restart\treplay_ok\tfirst_differing_step\tall_ok");
    for (_, l, _) in &out {
        println!("{l}");
    }
    let n_ok = out.iter().filter(|r| r.2).count();
    println!("# equivalence: {n_ok} of {} tuning seeds identical in every check; wall {:.0} s", out.len(), t0.elapsed().as_secs_f64());
    if n_ok != out.len() {
        std::process::exit(1);
    }
}

// ------------------------------------------------------------------ PRIMARY and secondaries (a), (c) to (f)

#[derive(Clone, Debug, Default)]
struct Mx {
    rpe: f64,
    wrong100: f64,
    right_ans: f64,
    skip: f64,
    tasks: usize,
    cold_rpe: f64,
    cold_tasks: usize,
    tenths: Vec<f64>,
    slope: f64,
    file_keys: usize,
    wrong_accepted: usize,
    rules: usize,
    rules_noisy: usize,
    wall: f64,
}

fn mx(l: &Life, budget: i64) -> Mx {
    let n = l.truths.len().max(1) as f64;
    let ans = l.truths.iter().filter(|t| t.answered).count() as f64;
    let ok = l.truths.iter().filter(|t| t.correct).count() as f64;
    let wrong = l.truths.iter().filter(|t| t.wrong).count() as f64;
    let skip = l.truths.iter().filter(|t| t.plan.is_none()).count() as f64;
    // (d) each file's first 3 tasks in this life
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let (mut cr, mut cc, mut cn) = (0i64, 0i64, 0usize);
    for (t, f) in l.truths.iter().zip(&l.files) {
        let k = seen.entry(f.as_str()).or_default();
        if *k < 3 {
            cr += t.reward;
            cc += t.cost;
            cn += 1;
        }
        *k += 1;
    }
    // M1: reward per energy per tenth of the life (by energy spent before the step), and its least-squares slope
    let mut tr = vec![(0i64, 0i64); 10];
    let mut spent = 0i64;
    for t in &l.truths {
        let k = ((spent * 10) / budget).clamp(0, 9) as usize;
        tr[k].0 += t.reward;
        tr[k].1 += t.cost;
        spent += t.cost;
    }
    let tenths: Vec<f64> = tr.iter().map(|(r, c)| *r as f64 / (*c).max(1) as f64).collect();
    let xm = 4.5;
    let ym = mean(&tenths);
    let slope = tenths.iter().enumerate().map(|(i, y)| (i as f64 - xm) * (y - ym)).sum::<f64>() / (0..10).map(|i| (i as f64 - xm).powi(2)).sum::<f64>();
    Mx {
        rpe: l.rpe(),
        wrong100: 100.0 * wrong / n,
        right_ans: ok / ans.max(1.0),
        skip: skip / n,
        tasks: l.truths.len(),
        cold_rpe: cr as f64 / cc.max(1) as f64,
        cold_tasks: cn,
        tenths,
        slope,
        file_keys: l.file_keys,
        wrong_accepted: l.truths.iter().filter(|t| t.wrong_accepted).count(),
        rules: l.rules,
        rules_noisy: l.rules_noisy,
        wall: l.wall,
    }
}

fn cmd_primary(args: &[String]) {
    let out = arg(args, "--out").expect("--out FILE.tsv (per-world rows)");
    let t0 = std::time::Instant::now();
    let ss: Vec<u64> = (REPORT.0..=REPORT.1).collect();
    // per seed: [default B0, default B1, ood B0, ood B1]
    let rows: Vec<(u64, Vec<Mx>)> = ss
        .par_iter()
        .map(|&s| {
            let mut v = vec![];
            for ood in [false, true] {
                let cfg = WTCfg { ood, ..WTCfg::new(s, DEFAULT_BUDGET) };
                v.push(mx(&life(&cfg, &mut LearnerT { cfg: b0_cfg() }, b0_cfg()), cfg.budget));
                v.push(mx(&life(&cfg, &mut LearnerT { cfg: b1_cfg() }, b1_cfg()), cfg.budget));
            }
            eprintln!("world {s} done at {:.0} s", t0.elapsed().as_secs_f64());
            (s, v)
        })
        .collect();
    let names = ["default\tB0", "default\tB1", "ood\tB0", "ood\tB1"];
    let mut tsv = String::from("seed\tworld\tpolicy\trpe\twrong_per100\tright_of_answered\tskip_share\ttasks\tcold_rpe\tcold_tasks\tslope_per_tenth\ttenths\tfile_keys\twrong_accepted\trules\trules_noisy\twall_s\n");
    for (s, v) in &rows {
        for (k, m) in v.iter().enumerate() {
            tsv.push_str(&format!(
                "{s}\t{}\t{:.6}\t{:.4}\t{:.4}\t{:.4}\t{}\t{:.6}\t{}\t{:.6}\t{}\t{}\t{}\t{}\t{}\t{:.2}\n",
                names[k], m.rpe, m.wrong100, m.right_ans, m.skip, m.tasks, m.cold_rpe, m.cold_tasks, m.slope, m.tenths.iter().map(|x| format!("{x:.4}")).collect::<Vec<_>>().join(";"), m.file_keys, m.wrong_accepted, m.rules, m.rules_noisy, m.wall
            ));
        }
    }
    std::fs::write(&out, &tsv).expect("write --out");
    let col = |k: usize, f: &dyn Fn(&Mx) -> f64| rows.iter().map(|r| f(&r.1[k])).collect::<Vec<f64>>();
    let (b0, b1) = (col(0, &|m| m.rpe), col(1, &|m| m.rpe));
    let d = diff(&b1, &b0);
    let (m, lo, hi) = ci(&d);
    let (w, t, l) = wtl(&d);
    println!("# report seeds {}..{} (n = {}), World T penalty 0, energy weight 1, budget {DEFAULT_BUDGET}, default files; t quantile {:.4}", REPORT.0, REPORT.1, ss.len(), tq(ss.len() - 1));
    for (k, nm) in ["B0 (default being)", "B1 (in-being, wt_file_key on, beta 2)"].iter().enumerate() {
        let (a, b, c) = ci(&col(k, &|m| m.rpe));
        println!("# {nm}: reward per energy {a:.3} [{b:.3}, {c:.3}]");
    }
    let pass = lo >= 0.05 && m >= 0.10 && w >= 45;
    println!("# PRIMARY B1 minus B0: {m:+.3} [{lo:+.3}, {hi:+.3}] ({w}/{t}/{l}), paired sd {:.3}; rule lower >= +0.05: {}, mean >= +0.10: {}, wins >= 45: {} -> {}", sd(&d), lo >= 0.05, m >= 0.10, w >= 45, if pass { "MET" } else { "NOT MET" });
    let (w0, w1) = (col(0, &|m| m.wrong100), col(1, &|m| m.wrong100));
    println!("# (a) wrong answers per 100 tasks: B0 {:.2}, B1 {:.2}; B1 minus B0 {} (W = B1 more)", mean(&w0), mean(&w1), paired(&w1, &w0));
    println!("#     right share of answers: B0 {:.4}, B1 {:.4}; skip share B0 {:.4}, B1 {:.4}", mean(&col(0, &|m| m.right_ans)), mean(&col(1, &|m| m.right_ans)), mean(&col(0, &|m| m.skip)), mean(&col(1, &|m| m.skip)));
    let (o0, o1) = (col(2, &|m| m.rpe), col(3, &|m| m.rpe));
    let (om, olo, ohi) = ci(&diff(&o1, &o0));
    println!("# (c) OOD (ood = true): B0 {:.3}, B1 {:.3}; B1 minus B0 {}; significantly negative (CI entirely below 0): {}", mean(&o0), mean(&o1), paired(&o1, &o0), ohi < 0.0);
    let _ = (om, olo);
    println!("#     OOD wrong per 100: B0 {:.2}, B1 {:.2}; B1 minus B0 {}", mean(&col(2, &|m| m.wrong100)), mean(&col(3, &|m| m.wrong100)), paired(&col(3, &|m| m.wrong100), &col(2, &|m| m.wrong100)));
    let (c0, c1) = (col(0, &|m| m.cold_rpe), col(1, &|m| m.cold_rpe));
    println!("# (d) cold start, reward per energy on each file's first 3 tasks: B0 {:.3}, B1 {:.3}; B1 minus B0 {}; tasks counted per life B0 {:.1}, B1 {:.1}", mean(&c0), mean(&c1), paired(&c1, &c0), mean(&col(0, &|m| m.cold_tasks as f64)), mean(&col(1, &|m| m.cold_tasks as f64)));
    println!("#     OOD cold start: B1 minus B0 {}", paired(&col(3, &|m| m.cold_rpe), &col(2, &|m| m.cold_rpe)));
    let fk = col(1, &|m| m.file_keys as f64);
    println!("# (e) file keys per life (B1): mean {:.0}, min {:.0}, max {:.0}; OOD mean {:.0}; wall per life ({} lives in parallel) B0 {:.2} s, B1 {:.2} s (OOD B0 {:.2}, B1 {:.2}); peak working set of this process {:.0} MB",
        mean(&fk), fk.iter().cloned().fold(f64::MAX, f64::min), fk.iter().cloned().fold(0.0, f64::max), mean(&col(3, &|m| m.file_keys as f64)), rayon::current_num_threads(),
        mean(&col(0, &|m| m.wall)), mean(&col(1, &|m| m.wall)), mean(&col(2, &|m| m.wall)), mean(&col(3, &|m| m.wall)), peak_mem() as f64 / 1e6);
    let sumc = |k: usize, f: &dyn Fn(&Mx) -> usize| rows.iter().map(|r| f(&r.1[k])).sum::<usize>();
    println!("# (f) wrong answers accepted (paid): B0 {}, B1 {}, OOD B0 {}, B1 {}; rules accepted B0 {}, B1 {} (on noisy series: B0 {}, B1 {}; OOD B0 {}, B1 {})", sumc(0, &|m| m.wrong_accepted), sumc(1, &|m| m.wrong_accepted), sumc(2, &|m| m.wrong_accepted), sumc(3, &|m| m.wrong_accepted), sumc(0, &|m| m.rules), sumc(1, &|m| m.rules), sumc(0, &|m| m.rules_noisy), sumc(1, &|m| m.rules_noisy), sumc(2, &|m| m.rules_noisy), sumc(3, &|m| m.rules_noisy));
    println!("# M1 (secondary): slope of reward per energy per tenth of the life, B0 {:.4}, B1 {:.4}; B1 minus B0 {}", mean(&col(0, &|m| m.slope)), mean(&col(1, &|m| m.slope)), paired(&col(1, &|m| m.slope), &col(0, &|m| m.slope)));
    for k in 0..2 {
        let tt: Vec<String> = (0..10).map(|i| format!("{:.3}", mean(&rows.iter().map(|r| r.1[k].tenths[i]).collect::<Vec<_>>()))).collect();
        println!("#     tenths {}: {}", ["B0", "B1"][k], tt.join(" "));
    }
    println!("# wall {:.0} s; per-world rows in {out}", t0.elapsed().as_secs_f64());
}

fn cmd_cost(args: &[String]) {
    let pol = arg(args, "--pol").expect("--pol b0|b1");
    let c = match pol.as_str() { "b0" => b0_cfg(), "b1" => b1_cfg(), _ => panic!("--pol b0|b1") };
    let t0 = std::time::Instant::now();
    let mut walls = vec![];
    let mut keys = vec![];
    for s in REPORT.0..=REPORT.1 {
        let cfg = WTCfg::new(s, DEFAULT_BUDGET);
        let l = life(&cfg, &mut LearnerT { cfg: c }, c);
        walls.push(l.wall);
        keys.push(l.file_keys as f64);
    }
    println!("# cost {pol}: {} lives one after another on one thread; wall per life {:.3} s (min {:.3}, max {:.3}); file keys per life {:.0}; peak working set of the process {:.1} MB; total {:.0} s",
        walls.len(), mean(&walls), walls.iter().cloned().fold(f64::MAX, f64::min), walls.iter().cloned().fold(0.0, f64::max), mean(&keys), peak_mem() as f64 / 1e6, t0.elapsed().as_secs_f64());
}

// ------------------------------------------------------------------ the W1b sweep with B1

const BETAS: [u32; 6] = [500, 1000, 2000, 4000, 8000, 16000];
const UCB_C: [f64; 6] = [0.001, 0.003, 0.01, 0.03, 0.1, 0.3];
const LIN_A: [f64; 6] = [0.1, 0.3, 1.0, 3.0, 10.0, 30.0];
const PENALTY: [u32; 6] = [0, 100, 316, 1000, 3162, 10000];
const COST: [u32; 5] = [100, 316, 1000, 3162, 10000];

fn being_cfg(b: u32) -> LearnerTCfg {
    LearnerTCfg { beta_milli: b, prior_w_milli: 2000, pooled: true, skip: true, value_ucb: true, file_key: false, file_beta_table: false }
}
fn ucb_cfg(c: f64) -> UcbCfg {
    UcbCfg { c, per_ctx: true, skip: true }
}
fn lin_cfg(a: f64) -> LinCfg {
    LinCfg { alpha: a, skip: true }
}
fn wcfg(seed: u64, p: u32, c: u32) -> WTCfg {
    WTCfg { penalty_pm: p, cost_pm: c, ..WTCfg::new(seed, DEFAULT_BUDGET) }
}
fn grid() -> Vec<(u32, u32)> {
    PENALTY.iter().flat_map(|p| COST.iter().map(move |c| (*p, *c))).collect()
}

/// The tuned policies of W1b (being, UCB1, LinUCB) plus LinUCB with the file feature, 6 configurations each.
fn tune_pols() -> Vec<(String, String, Pol)> {
    let mut v: Vec<(String, String, Pol)> = BETAS.iter().map(|b| ("B0".to_string(), format!("beta={}", *b as f64 / 1000.0), Pol::Being(being_cfg(*b)))).collect();
    v.extend(UCB_C.iter().map(|c| ("UCB1".to_string(), format!("c={c}"), Pol::Ucb(ucb_cfg(*c)))));
    v.extend(LIN_A.iter().map(|a| ("LinUCB".to_string(), format!("alpha={a}"), Pol::Lin(lin_cfg(*a)))));
    v.extend(LIN_A.iter().map(|a| ("LinUCB+file".to_string(), format!("alpha={a}"), Pol::LinFile(lin_cfg(*a)))));
    v
}

fn pick(scores: &[f64]) -> usize {
    let mut bi = 0;
    for (i, s) in scores.iter().enumerate() {
        if *s > scores[bi] {
            bi = i;
        }
    }
    bi
}

#[derive(Clone, Debug, Default)]
struct M {
    rpe: f64,
    wrong100: f64,
    right_ans: f64,
    coverage: f64,
    skip: f64,
    wrong_accepted: usize,
    rules_noisy: usize,
}

fn metrics(l: &LifeT) -> M {
    let n = l.truths.len().max(1) as f64;
    let ans = l.truths.iter().filter(|t| t.answered).count() as f64;
    let ok = l.truths.iter().filter(|t| t.correct).count() as f64;
    let skip = l.truths.iter().filter(|t| t.plan.is_none()).count() as f64;
    let wrong = l.truths.iter().filter(|t| t.wrong).count() as f64;
    M { rpe: l.rpe(), right_ans: ok / ans.max(1.0), coverage: ans / n, skip: skip / n, wrong100: 100.0 * wrong / n, wrong_accepted: l.truths.iter().filter(|t| t.wrong_accepted).count(), rules_noisy: l.rules_noisy }
}

fn cmd_sweep(args: &[String]) {
    let dir = arg(args, "--out-dir").expect("--out-dir DIR (per-world TSV files)");
    std::fs::create_dir_all(&dir).unwrap();
    let g = grid();
    let pols = tune_pols();
    let t0 = std::time::Instant::now();
    // stage 1: tuning, seeds 101..120 only: per seed, per grid point, per configuration
    let tseeds: Vec<u64> = (TUNE.0..=TUNE.1).collect();
    let tune: Vec<Vec<Vec<f64>>> = tseeds
        .par_iter()
        .map(|s| {
            let mut cache = ToolCache::new();
            let r = g.iter().map(|(p, c)| pols.iter().map(|(_, _, pol)| run_life_cached(&wcfg(*s, *p, *c), *pol, None, &mut cache).rpe()).collect()).collect();
            eprintln!("tuning world {s} done at {:.0} s", t0.elapsed().as_secs_f64());
            r
        })
        .collect();
    let mut tt = String::from("seed\tpenalty_pm\tcost_pm\tpolicy\tconfig\trpe\n");
    for (si, s) in tseeds.iter().enumerate() {
        for (gi, (p, c)) in g.iter().enumerate() {
            for (k, (nm, cf, _)) in pols.iter().enumerate() {
                tt.push_str(&format!("{s}\t{p}\t{c}\t{nm}\t{cf}\t{:.6}\n", tune[si][gi][k]));
            }
        }
    }
    std::fs::write(format!("{dir}/w2_sweep_tune_seeds101-120.tsv"), tt).unwrap();
    let twall = t0.elapsed().as_secs_f64();
    // chosen per point: (being beta, ucb c, lin alpha, linfile alpha), with the tuning means
    let mut chosen = vec![];
    println!("Tuning on seeds 101..120 (mean reward per energy; chosen value, * = edge of its 6-value grid); B1 beta fixed at 2 (not tuned); tuning wall {twall:.0} s");
    println!("| penalty | weight | B0 beta | UCB1 c | LinUCB alpha | LinUCB+file alpha |");
    println!("|---|---|---|---|---|---|");
    for (gi, (p, c)) in g.iter().enumerate() {
        let per: Vec<f64> = (0..pols.len()).map(|k| mean(&tune.iter().map(|s| s[gi][k]).collect::<Vec<_>>())).collect();
        let ix = [pick(&per[0..6]), pick(&per[6..12]), pick(&per[12..18]), pick(&per[18..24])];
        let e = |i: usize| if ix[i] == 0 || ix[i] == 5 { "*" } else { "" };
        println!("| {:.3} | {:.3} | {}{} ({:.3}) | {}{} ({:.3}) | {}{} ({:.3}) | {}{} ({:.3}) |", *p as f64 / 1000.0, *c as f64 / 1000.0, BETAS[ix[0]] as f64 / 1000.0, e(0), per[ix[0]], UCB_C[ix[1]], e(1), per[6 + ix[1]], LIN_A[ix[2]], e(2), per[12 + ix[2]], LIN_A[ix[3]], e(3), per[18 + ix[3]]);
        chosen.push((BETAS[ix[0]], UCB_C[ix[1]], LIN_A[ix[2]], LIN_A[ix[3]]));
    }
    // stage 2: report seeds 201..260 with each point's tuned configurations; B1 with beta 2
    let names = ["B1", "B0", "B0 beta 2", "LinUCB", "LinUCB+file", "UCB1", "always SQL", "random plan"];
    let t1 = std::time::Instant::now();
    let rseeds: Vec<u64> = (REPORT.0..=REPORT.1).collect();
    let rep: Vec<Vec<Vec<M>>> = rseeds
        .par_iter()
        .map(|s| {
            let mut cache = ToolCache::new();
            let r = g
                .iter()
                .zip(&chosen)
                .map(|((p, c), t)| {
                    let cfg = wcfg(*s, *p, *c);
                    let ps = [Pol::Being(b1_cfg()), Pol::Being(being_cfg(t.0)), Pol::Being(being_cfg(2000)), Pol::Lin(lin_cfg(t.2)), Pol::LinFile(lin_cfg(t.3)), Pol::Ucb(ucb_cfg(t.1)), Pol::AlwaysSql, Pol::Random];
                    let mut out: Vec<M> = vec![];
                    for (k, pol) in ps.iter().enumerate() {
                        // B0 at beta 2 is the tuned B0 when the tuning chose 2: the same life
                        if k == 2 && t.0 == 2000 {
                            out.push(out[1].clone());
                            continue;
                        }
                        out.push(metrics(&run_life_cached(&cfg, *pol, None, &mut cache)));
                    }
                    out
                })
                .collect();
            eprintln!("report world {s} done at {:.0} s", t0.elapsed().as_secs_f64());
            r
        })
        .collect();
    let rwall = t1.elapsed().as_secs_f64();
    let mut rt = String::from("seed\tpenalty_pm\tcost_pm\tpolicy\tconfig\trpe\twrong_per100\tright_of_answered\tcoverage\tskip_share\twrong_accepted\trules_noisy\n");
    for (si, s) in rseeds.iter().enumerate() {
        for (gi, (p, c)) in g.iter().enumerate() {
            let t = chosen[gi];
            let cf = ["beta=2".to_string(), format!("beta={}", t.0 as f64 / 1000.0), "beta=2".into(), format!("alpha={}", t.2), format!("alpha={}", t.3), format!("c={}", t.1), "-".into(), "-".into()];
            for (k, nm) in names.iter().enumerate() {
                let m = &rep[si][gi][k];
                rt.push_str(&format!("{s}\t{p}\t{c}\t{nm}\t{}\t{:.6}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{}\t{}\n", cf[k], m.rpe, m.wrong100, m.right_ans, m.coverage, m.skip, m.wrong_accepted, m.rules_noisy));
            }
        }
    }
    std::fs::write(format!("{dir}/w2_sweep_report_seeds201-260.tsv"), rt).unwrap();
    println!("\nReport seeds {}..{} (n = {}); report wall {rwall:.0} s", REPORT.0, REPORT.1, rseeds.len());
    println!("\nPaired differences, reward per energy, mean [95% CI] (wins/ties/losses of B1):");
    println!("| penalty | weight | B1 minus B0 (tuned) | B1 minus LinUCB | B1 minus LinUCB+file | B1 minus better LinUCB | B1 minus always SQL | B1 minus UCB1 | B1 minus B0 beta 2 | one-sided p (B1-B0<0) |");
    println!("|---|---|---|---|---|---|---|---|---|---|");
    let at = |gi: usize, k: usize, f: &dyn Fn(&M) -> f64| rep.iter().map(|s| f(&s[gi][k])).collect::<Vec<f64>>();
    let rpe = |m: &M| m.rpe;
    let mut pv = vec![];
    let mut pv2 = vec![];
    let (mut up, mut down, mut neg) = (0, 0, vec![]);
    let mut lin_rows = vec![];
    for (gi, (p, c)) in g.iter().enumerate() {
        let b1 = at(gi, 0, &rpe);
        let b0 = at(gi, 1, &rpe);
        let d = diff(&b1, &b0);
        let (m, lo, hi) = ci(&d);
        if lo > 0.0 {
            up += 1;
        }
        if hi < 0.0 {
            down += 1;
        }
        if m < 0.0 {
            neg.push(format!("penalty {:.3}, weight {:.3}: {}", *p as f64 / 1000.0, *c as f64 / 1000.0, paired(&b1, &b0)));
        }
        let pp = p_below0(&d);
        pv.push(pp);
        pv2.push(p_below0(&diff(&b1, &at(gi, 2, &rpe))));
        let (l, lf) = (at(gi, 3, &rpe), at(gi, 4, &rpe));
        let better = if mean(&lf) > mean(&l) { (&lf, "LinUCB+file") } else { (&l, "LinUCB") };
        lin_rows.push((ci(&diff(&b1, better.0)), better.1, ci(&diff(&lf, &l))));
        println!("| {:.3} | {:.3} | {} | {} | {} | {} ({}) | {} | {} | {} | {:.2e} |", *p as f64 / 1000.0, *c as f64 / 1000.0, paired(&b1, &b0), paired(&b1, &l), paired(&b1, &lf), paired(&b1, better.0), better.1, paired(&b1, &at(gi, 6, &rpe)), paired(&b1, &at(gi, 5, &rpe)), paired(&b1, &at(gi, 2, &rpe)), pp);
    }
    let rej = holm(&pv, 0.05);
    let rej2 = holm(&pv2, 0.05);
    println!("\nB1 minus B0 (tuned): interval above 0 at {up} of 30 points, below 0 at {down}");
    println!("Points with a negative mean B1 minus B0: {}", if neg.is_empty() { "none".to_string() } else { neg.join("; ") });
    println!("Holm (one-sided paired t of B1 minus B0 < 0, 30 points, FWER 0.05): regressions rejected after Holm at {} points{}", rej.iter().filter(|x| **x).count(), g.iter().zip(&rej).filter(|(_, r)| **r).map(|((p, c), _)| format!(" [penalty {:.3}, weight {:.3}]", *p as f64 / 1000.0, *c as f64 / 1000.0)).collect::<String>());
    println!("Supplementary (not the pre-registered B0): the same Holm test against B0 with beta 2 at every point: rejected at {} points", rej2.iter().filter(|x| **x).count());
    let lin_ahead = lin_rows.iter().filter(|r| r.0 .1 > 0.0).count();
    let lin_behind = lin_rows.iter().filter(|r| r.0 .2 < 0.0).count();
    let lf_better = lin_rows.iter().filter(|r| r.2 .1 > 0.0).count();
    let lf_worse = lin_rows.iter().filter(|r| r.2 .2 < 0.0).count();
    println!("B1 against the better LinUCB variant per point: interval above 0 at {lin_ahead}, below 0 at {lin_behind}; LinUCB+file minus LinUCB: above 0 at {lf_better}, below 0 at {lf_worse}");
    // wrong answers per 100 for every policy
    println!("\nWrong answers per 100 tasks (mean over the report seeds) and skip share:");
    println!("| penalty | weight | {} | B1 minus B0 wrong per 100 |", names.join(" | "));
    println!("|---|---|{}---|", "---|".repeat(names.len()));
    let w = |m: &M| m.wrong100;
    for (gi, (p, c)) in g.iter().enumerate() {
        let cells: Vec<String> = (0..names.len()).map(|k| format!("{:.2} / {:.2}", mean(&at(gi, k, &w)), mean(&at(gi, k, &|m: &M| m.skip)))).collect();
        println!("| {:.3} | {:.3} | {} | {} |", *p as f64 / 1000.0, *c as f64 / 1000.0, cells.join(" | "), paired(&at(gi, 0, &w), &at(gi, 1, &w)));
    }
    // M4: the being is dominated at a point if a baseline has higher reward per energy (CI of baseline minus being above 0) AND fewer wrong answers per 100 (mean)
    println!("\nM4 (Pareto, docs/milestones.md): a point is dominated if a baseline has higher reward per energy (paired CI above 0) and fewer wrong answers per 100 tasks (mean)");
    for (bk, bname) in [(0usize, "B1"), (1, "B0 (tuned)")] {
        let mut dom = vec![];
        for (gi, (p, c)) in g.iter().enumerate() {
            for (k, nm) in names.iter().enumerate().skip(3) {
                let (_, lo, _) = ci(&diff(&at(gi, k, &rpe), &at(gi, bk, &rpe)));
                if lo > 0.0 && mean(&at(gi, k, &w)) < mean(&at(gi, bk, &w)) {
                    dom.push(format!("penalty {:.3}, weight {:.3}: {nm} {} reward per energy, wrong per 100 {:.2} vs {:.2}", *p as f64 / 1000.0, *c as f64 / 1000.0, paired(&at(gi, k, &rpe), &at(gi, bk, &rpe)), mean(&at(gi, k, &w)), mean(&at(gi, bk, &w))));
                }
            }
        }
        println!("  {bname}: dominated at {} points{}", dom.len(), if dom.is_empty() { String::new() } else { format!(": {}", dom.join("; ")) });
    }
    let wa: usize = rep.iter().flat_map(|s| s.iter().map(|g| g[0].wrong_accepted + g[1].wrong_accepted)).sum();
    let rn: usize = rep.iter().flat_map(|s| s.iter().map(|g| g[0].rules_noisy)).sum();
    println!("\n(f) over the sweep: wrong answers accepted (B1 + B0) {wa}; B1 rules on noisy series {rn}");
    println!("total wall {:.0} s", t0.elapsed().as_secs_f64());
}

/// Plain LinUCB after the Track W2 refactor of `LinArm` (dimension as a field): the Gate 0 B2 rows (LinUCB alpha 1,
/// skip on; seeds 1..3 of `docs/data/n1_gate0_report_seeds1-60.tsv`, a regression check, not a measurement).
fn cmd_lincheck() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("docs").join("data").join("n1_gate0_report_seeds1-60.tsv");
    let text = std::fs::read_to_string(&p).unwrap();
    let mut ok = true;
    for l in text.lines().skip(1).take(3) {
        let c: Vec<&str> = l.split('\t').collect();
        let s: u64 = c[0].parse().unwrap();
        let got = run_life_cached(&WTCfg::new(s, DEFAULT_BUDGET), Pol::Lin(LinCfg { alpha: 1.0, skip: true }), None, &mut ToolCache::new()).rpe();
        println!("seed {s}: Gate 0 b2_linucb {} now {got:.6}", c[3]);
        ok &= format!("{got:.6}") == c[3];
    }
    println!("# plain LinUCB unchanged: {ok}");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("equiv") => cmd_equiv(),
        Some("primary") => cmd_primary(&args[1..]),
        Some("cost") => cmd_cost(&args[1..]),
        Some("sweep") => cmd_sweep(&args[1..]),
        Some("lincheck") => cmd_lincheck(),
        _ => eprintln!("usage: measure_w2 equiv | primary --out F | cost --pol b0|b1 | sweep --out-dir D | lincheck"),
    }
}
