//! Track W2b (docs/w2b_prereg.md): B1 with a penalty-aware beta (B1p, `wt_file_beta = table`). Measurement only.
//!   cargo run -p kore-self --example measure_w2b --offline --release -- tune --out FILE.tsv      [tuning seeds 101..120 only]
//!   cargo run -p kore-self --example measure_w2b --offline --release -- table                    [the table from the tuning file]
//!   cargo run -p kore-self --example measure_w2b --offline --release -- equiv --out FILE.tsv     [tuning seeds 101..120 only]
//!   cargo run -p kore-self --example measure_w2b --offline --release -- primary --out FILE.tsv   [report seeds 261..320]
//!   cargo run -p kore-self --example measure_w2b --offline --release -- sweep --out FILE.tsv     [report seeds 261..320]
//!
//! `tune`: on seeds 101..120, at each of the 30 points of the 6 x 5 sweep (wrong-answer penalty x energy weight),
//! 6 configurations each of B0 (beta), B1 (`wt_file_key` on, beta; the B1p candidates), UCB1 (c), LinUCB (alpha)
//! and LinUCB with the file feature (alpha): the same grids as W1b and W2. Selection rule: the highest mean
//! reward per energy (first of equal ones). It checks that the B0, UCB1 and LinUCB rows equal the W2 tuning file
//! (`docs/data/w2_sweep_tune_seeds101-120.tsv`, same code and seeds) and prints the B1p table.
//! `table`: the B1p table and the other policies' choices recomputed from the committed tuning file; it must equal
//! `learner::W2B_BETA_TABLE` (every later command checks this first and stops if not).
//! `equiv`: the two pre-registered equivalence tests on seeds 101..120, at every point: (1) where the table picks
//! beta 2, B1p equals W2's B1 (in-being, beta 2) step for step; (2) B1p equals the Gate 0 harness B1 (`FileKeyed`,
//! copied verbatim) with the table's beta, in the measurement loop, as a logged being in one run, with a restart
//! in the middle of the life from the full fold, and with the restart from a checkpoint (which must be used);
//! decisions, reward and energy identical, every logged decision replays; and B1p's reward per energy equals the
//! tuning file's B1 row with that beta (6 decimals).
//!
//! Files are the generator's bytes in memory (`MemSandbox`). Rows tab-separated; summary lines start with '#'.
//! Statistics: paired t-intervals (95%, t quantile for n - 1 degrees of freedom); Holm over the 30 points.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::policy::Policy;
use being::world_t::baselines::{LinCfg, UcbCfg};
use being::world_t::engine::{run_t_keys, Cell, ChooserT, EngineT, MemSandbox, RunTCfg, StateT, ToolCache, TrueT, DEFAULT_DISCOVER_EVERY};
use being::world_t::learner::{table_beta, ChoiceT, LearnerT, LearnerTCfg, W2B_BETA_TABLE};
use being::world_t::measure::{run_life_cached, LifeT, Pol};
use being::world_t::world::{Plan, ViewT, WTCfg, WorldT, DEFAULT_BUDGET};
use rayon::prelude::*;
use std::collections::HashMap;

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

const TUNE: (u64, u64) = (101, 120);
const REPORT: (u64, u64) = (261, 320);

const BETAS: [u32; 6] = [500, 1000, 2000, 4000, 8000, 16000];
const UCB_C: [f64; 6] = [0.001, 0.003, 0.01, 0.03, 0.1, 0.3];
const LIN_A: [f64; 6] = [0.1, 0.3, 1.0, 3.0, 10.0, 30.0];
const PENALTY: [u32; 6] = [0, 100, 316, 1000, 3162, 10000];
const COST: [u32; 5] = [100, 316, 1000, 3162, 10000];

fn b0_cfg() -> LearnerTCfg {
    LearnerTCfg::default()
}
/// W2's B1: the file key on, beta fixed (2 in W2).
fn b1_cfg(b: u32) -> LearnerTCfg {
    LearnerTCfg { beta_milli: b, file_key: true, ..Default::default() }
}
/// B1p: the file key on, beta from the W2b table.
fn b1p_cfg() -> LearnerTCfg {
    LearnerTCfg { file_key: true, file_beta_table: true, ..Default::default() }
}
fn being_cfg(b: u32) -> LearnerTCfg {
    LearnerTCfg { beta_milli: b, ..Default::default() }
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
fn pt(p: u32, c: u32) -> String {
    format!("penalty {:.3}, weight {:.3}", p as f64 / 1000.0, c as f64 / 1000.0)
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

// ------------------------------------------------------------------ per-life metrics (as in examples/measure_w2.rs)

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

// ------------------------------------------------------------------ tuning (seeds 101..120 only)

/// The tuned policies, 6 configurations each, in this order (B0, B1, UCB1, LinUCB, LinUCB+file).
fn tune_pols() -> Vec<(String, String, Pol)> {
    let mut v: Vec<(String, String, Pol)> = BETAS.iter().map(|b| ("B0".to_string(), format!("beta={}", *b as f64 / 1000.0), Pol::Being(being_cfg(*b)))).collect();
    v.extend(BETAS.iter().map(|b| ("B1".to_string(), format!("beta={}", *b as f64 / 1000.0), Pol::Being(b1_cfg(*b)))));
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

/// The configuration chosen at one point: B0 beta, B1p beta, UCB1 c, LinUCB alpha, LinUCB+file alpha (indices into
/// the grids) and the tuning means of all 30 configurations.
#[derive(Clone, Debug)]
struct Chosen {
    ix: [usize; 5],
    means: Vec<f64>,
}

impl Chosen {
    fn b0(&self) -> u32 {
        BETAS[self.ix[0]]
    }
    fn b1p(&self) -> u32 {
        BETAS[self.ix[1]]
    }
    fn ucb(&self) -> f64 {
        UCB_C[self.ix[2]]
    }
    fn lin(&self) -> f64 {
        LIN_A[self.ix[3]]
    }
    fn linf(&self) -> f64 {
        LIN_A[self.ix[4]]
    }
}

fn choose_from(means: Vec<f64>) -> Chosen {
    let ix = [0, 1, 2, 3, 4].map(|k| pick(&means[6 * k..6 * k + 6]));
    Chosen { ix, means }
}

fn data_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("docs").join("data").join(name)
}

/// (seed, penalty, cost, policy, config) -> rpe text, from a tuning file of the W2 / W2b layout.
fn read_tune(name: &str) -> HashMap<(u64, u32, u32, String, String), String> {
    let text = std::fs::read_to_string(data_path(name)).unwrap_or_else(|e| panic!("docs/data/{name}: {e}"));
    text.lines().skip(1).filter(|l| !l.starts_with('#') && !l.trim().is_empty()).map(|l| {
        let c: Vec<&str> = l.split('\t').collect();
        ((c[0].parse().unwrap(), c[1].parse().unwrap(), c[2].parse().unwrap(), c[3].to_string(), c[4].to_string()), c[5].to_string())
    }).collect()
}

const TUNE_FILE: &str = "w2b_tune_seeds101-120.tsv";

/// The choices per point recomputed from the committed tuning file (means of the 6-decimal values).
fn chosen_from_file() -> Vec<Chosen> {
    let t = read_tune(TUNE_FILE);
    let pols = tune_pols();
    grid().iter().map(|(p, c)| {
        let means = pols.iter().map(|(nm, cf, _)| mean(&(TUNE.0..=TUNE.1).map(|s| t[&(s, *p, *c, nm.clone(), cf.clone())].parse::<f64>().unwrap()).collect::<Vec<_>>())).collect();
        choose_from(means)
    }).collect()
}

/// Stop unless the being's table equals the one recomputed from the tuning file.
fn check_table(ch: &[Chosen]) {
    let want: Vec<(u32, u32, u32)> = grid().iter().zip(ch).map(|((p, c), x)| (*p, *c, x.b1p())).collect();
    if want != W2B_BETA_TABLE.to_vec() {
        eprintln!("learner::W2B_BETA_TABLE differs from the table of docs/data/{TUNE_FILE}:\n  file  {want:?}\n  being {:?}", W2B_BETA_TABLE);
        std::process::exit(2);
    }
    for (p, c, b) in &want {
        assert_eq!(table_beta(*p, *c), *b, "table_beta at a grid point");
    }
}

fn print_choices(ch: &[Chosen]) {
    println!("| penalty | weight | B0 beta | B1p beta | UCB1 c | LinUCB alpha | LinUCB+file alpha |");
    println!("|---|---|---|---|---|---|---|");
    for ((p, c), x) in grid().iter().zip(ch) {
        let e = |k: usize| if x.ix[k] == 0 || x.ix[k] == 5 { "*" } else { "" };
        let m = |k: usize| x.means[6 * k + x.ix[k]];
        println!("| {:.3} | {:.3} | {}{} ({:.3}) | {}{} ({:.3}) | {}{} ({:.3}) | {}{} ({:.3}) | {}{} ({:.3}) |", *p as f64 / 1000.0, *c as f64 / 1000.0,
            x.b0() as f64 / 1000.0, e(0), m(0), x.b1p() as f64 / 1000.0, e(1), m(1), x.ucb(), e(2), m(2), x.lin(), e(3), m(3), x.linf(), e(4), m(4));
    }
}

fn cmd_tune(args: &[String]) {
    let out = arg(args, "--out").expect("--out FILE.tsv");
    let g = grid();
    let pols = tune_pols();
    let t0 = std::time::Instant::now();
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
    std::fs::write(&out, &tt).expect("write --out");
    // the rows that W2 also tuned (same code, same seeds) must be the W2 tuning file's
    let w2 = read_tune("w2_sweep_tune_seeds101-120.tsv");
    let (mut same, mut n) = (0, 0);
    for (si, s) in tseeds.iter().enumerate() {
        for (gi, (p, c)) in g.iter().enumerate() {
            for (k, (nm, cf, _)) in pols.iter().enumerate() {
                if let Some(v) = w2.get(&(*s, *p, *c, nm.clone(), cf.clone())) {
                    n += 1;
                    same += (*v == format!("{:.6}", tune[si][gi][k])) as usize;
                }
            }
        }
    }
    // the B1 rows at penalty 0, weight 1 against the Gate 0 tuning file (b1_beta<milli>)
    let g0 = std::fs::read_to_string(data_path("n1_gate0_tune_seeds101-120.tsv")).unwrap();
    let head: Vec<&str> = g0.lines().next().unwrap().split('\t').collect();
    let (mut same0, mut n0) = (0, 0);
    let gi0 = g.iter().position(|x| *x == (0, 1000)).unwrap();
    for l in g0.lines().skip(1).filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let cc: Vec<&str> = l.split('\t').collect();
        let s: u64 = cc[0].parse().unwrap();
        let Some(si) = tseeds.iter().position(|x| *x == s) else { continue };
        for (bi, b) in BETAS.iter().enumerate() {
            let k = head.iter().position(|h| *h == format!("b1_beta{b}")).unwrap();
            n0 += 1;
            same0 += (cc[k] == format!("{:.6}", tune[si][gi0][6 + bi])) as usize;
        }
    }
    let full: Vec<Chosen> = (0..g.len()).map(|gi| choose_from((0..pols.len()).map(|k| mean(&tune.iter().map(|s| s[gi][k]).collect::<Vec<_>>())).collect())).collect();
    println!("# tuning seeds {}..{}, {} points x {} configurations; wall {:.0} s; rows in {out}", TUNE.0, TUNE.1, g.len(), pols.len(), t0.elapsed().as_secs_f64());
    println!("# check: rows equal to the W2 tuning file (B0, UCB1, LinUCB, LinUCB+file): {same} of {n}; B1 rows at penalty 0, weight 1 equal to the Gate 0 tuning file: {same0} of {n0}");
    println!("# chosen (mean reward per energy on the tuning seeds; * = edge of the 6-value grid):");
    print_choices(&full);
    println!("# W2B_BETA_TABLE:");
    for (i, ((p, c), x)) in g.iter().zip(&full).enumerate() {
        print!("({p}, {c}, {}),{}", x.b1p(), if i % 5 == 4 { "\n" } else { " " });
    }
}

fn cmd_table() {
    let ch = chosen_from_file();
    print_choices(&ch);
    check_table(&ch);
    println!("# learner::W2B_BETA_TABLE equals the table of docs/data/{TUNE_FILE}");
    for (p, c) in [(0, 1000), (50, 1000), (178, 562), (10, 1000), (20000, 50000), (316, 178)] {
        println!("# table_beta({p}, {c}) = {}", table_beta(p, c));
    }
}

// ------------------------------------------------------------------ equivalence (seeds 101..120 only)

fn open_log(root: &std::path::Path) -> versioned::VersionedLog {
    versioned::VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

/// The logged being B1p (`run_t_keys`, wt_file_key on, wt_file_beta table) in runs of `chunks` steps, restarting
/// from the log between chunks; `every` > 0: checkpoints on. Returns the truths of all steps, whether every restart
/// used a checkpoint, the decisions replayed / mismatched on the final log, and whether its fold has the table on.
fn logged(cfg: &WTCfg, chunks: &[u64], every: u64) -> (Vec<TrueT>, bool, (u64, u64), bool) {
    let d = tempfile::tempdir().unwrap();
    let w = WorldT::new(cfg.clone());
    let mut truths = vec![];
    let mut all_ckpt = true;
    for (i, &n) in chunks.iter().enumerate() {
        let mut log = open_log(d.path());
        let mut p = Policy::new(n);
        p.checkpoint_every = every;
        let rc = RunTCfg { world: Some(cfg.clone()), learner: Some(b0_cfg()), discover_every: DEFAULT_DISCOVER_EVERY, ctl: None, sql: None };
        let s = run_t_keys(&mut log, &rc, Some(true), Some(true), &p, None, &MemSandbox(&w)).unwrap();
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
    let st = StateT::from_events(&rows);
    (truths, all_ckpt, rp, st.learner.file_key && st.learner.file_beta_table)
}

fn cmd_equiv(args: &[String]) {
    let out = arg(args, "--out").expect("--out FILE.tsv");
    let ch = chosen_from_file();
    check_table(&ch);
    let tf = read_tune(TUNE_FILE);
    let t0 = std::time::Instant::now();
    let jobs: Vec<(u32, u32, u64)> = grid().iter().flat_map(|(p, c)| (TUNE.0..=TUNE.1).map(move |s| (*p, *c, s))).collect();
    let rows: Vec<(String, bool, bool, bool)> = jobs
        .par_iter()
        .map(|&(p, c, s)| {
            let cfg = wcfg(s, p, c);
            let bt = table_beta(p, c);
            let gl = LearnerTCfg { beta_milli: bt, ..Default::default() };
            let gb = life(&cfg, &mut FileKeyed::new(gl), gl);
            let ib = life(&cfg, &mut LearnerT { cfg: b1p_cfg() }, b1p_cfg());
            let n = ib.truths.len() as u64;
            let mid = (n / 2).max(100);
            let big = 1_000_000u64;
            let (one, _, rp1, t1) = logged(&cfg, &[big], 0);
            let (fold, _, rp2, t2) = logged(&cfg, &[mid, big], 0);
            let (ck, used, rp3, t3) = logged(&cfg, &[mid, big], 100);
            let sum = |t: &[TrueT]| (t.iter().map(|x| x.reward).sum::<i64>(), t.iter().map(|x| x.cost).sum::<i64>());
            let plans = |t: &[TrueT]| t.iter().map(|x| x.plan).collect::<Vec<_>>();
            let gp = gb.plans();
            let same = [plans(&ib.truths) == gp, plans(&one) == gp, plans(&fold) == gp, plans(&ck) == gp];
            let rc = [(ib.reward, ib.cost), sum(&one), sum(&fold), sum(&ck)].iter().all(|x| *x == (gb.reward, gb.cost));
            let rp_ok = [rp1, rp2, rp3].iter().all(|(n, bad)| *n > 0 && *bad == 0) && t1 && t2 && t3;
            let tune_v = tf[&(s, p, c, "B1".to_string(), format!("beta={}", bt as f64 / 1000.0))].clone();
            let tune_ok = tune_v == format!("{:.6}", ib.rpe());
            let ok2 = same.iter().all(|x| *x) && rc && used && rp_ok && tune_ok;
            // test 1: where the table picks 2, B1p is W2's B1
            let ok1 = if bt == 2000 {
                let w2 = life(&cfg, &mut LearnerT { cfg: b1_cfg(2000) }, b1_cfg(2000));
                Some(w2.plans() == ib.plans() && (w2.reward, w2.cost) == (ib.reward, ib.cost))
            } else {
                None
            };
            let first_diff = plans(&ib.truths).iter().zip(&gp).position(|(a, b)| a != b);
            let line = format!(
                "{p}\t{c}\t{s}\t{}\t{n}\t{:.6}\t{:.6}\t{tune_v}\t{tune_ok}\t{}\t{}\t{}\t{}\t{rc}\t{used}\t{rp_ok}\t{:?}\t{}\t{ok2}",
                bt as f64 / 1000.0, gb.rpe(), ib.rpe(), same[0], same[1], same[2], same[3], first_diff, ok1.map_or("-".to_string(), |x| x.to_string())
            );
            eprintln!("point ({p}, {c}) world {s} done at {:.0} s", t0.elapsed().as_secs_f64());
            (line, ok2, ok1.is_some(), ok1.unwrap_or(true))
        })
        .collect();
    let mut tsv = String::from("penalty_pm\tcost_pm\tseed\ttable_beta\tsteps\tharness_b1_rpe\tb1p_rpe\ttune_file_b1_rpe\ttune_equal\tsame_decisions_in_being\tsame_logged_one_run\tsame_logged_restart_fold\tsame_logged_restart_checkpoint\tsame_reward_and_energy_all\tcheckpoint_used_at_restart\treplay_ok_and_table_at_genesis\tfirst_differing_step\ttest1_equal_to_w2_b1\ttest2_all_ok\n");
    for r in &rows {
        tsv.push_str(&r.0);
        tsv.push('\n');
    }
    std::fs::write(&out, &tsv).expect("write --out");
    let n2 = rows.iter().filter(|r| r.1).count();
    let n1 = rows.iter().filter(|r| r.2).count();
    let ok1 = rows.iter().filter(|r| r.2 && r.3).count();
    let pts2 = grid().iter().filter(|(p, c)| table_beta(*p, *c) == 2000).count();
    println!("# equivalence test 1 (points where the table picks beta 2: {pts2} of 30): B1p identical to W2's B1 in {ok1} of {n1} (point, seed) lives");
    println!("# equivalence test 2 (every point): B1p identical to the harness B1 with the table's beta in every check in {n2} of {} (point, seed) pairs; wall {:.0} s; rows in {out}", rows.len(), t0.elapsed().as_secs_f64());
    if n2 != rows.len() || ok1 != n1 {
        std::process::exit(1);
    }
}

// ------------------------------------------------------------------ PRIMARY with (a), (c), (f) and cost

fn cmd_primary(args: &[String]) {
    let out = arg(args, "--out").expect("--out FILE.tsv (per-world rows)");
    check_table(&chosen_from_file());
    let t0 = std::time::Instant::now();
    let ss: Vec<u64> = (REPORT.0..=REPORT.1).collect();
    let lc = [b0_cfg(), b1p_cfg(), b1_cfg(2000)];
    // per seed: [default B0, B1p, B1; ood B0, B1p, B1]
    let rows: Vec<(u64, Vec<Mx>)> = ss
        .par_iter()
        .map(|&s| {
            let mut v = vec![];
            for ood in [false, true] {
                let cfg = WTCfg { ood, ..WTCfg::new(s, DEFAULT_BUDGET) };
                for l in lc {
                    v.push(mx(&life(&cfg, &mut LearnerT { cfg: l }, l), cfg.budget));
                }
            }
            eprintln!("world {s} done at {:.0} s", t0.elapsed().as_secs_f64());
            (s, v)
        })
        .collect();
    let par_wall = t0.elapsed().as_secs_f64();
    let names = ["default\tB0", "default\tB1p", "default\tB1_beta2", "ood\tB0", "ood\tB1p", "ood\tB1_beta2"];
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
    let rpe = |m: &Mx| m.rpe;
    let (b0, b1p) = (col(0, &rpe), col(1, &rpe));
    let d = diff(&b1p, &b0);
    let (m, lo, hi) = ci(&d);
    let (w, t, l) = wtl(&d);
    println!("# report seeds {}..{} (n = {}), World T penalty 0, energy weight 1, budget {DEFAULT_BUDGET}, default files; t quantile {:.4}; table beta at this point {}", REPORT.0, REPORT.1, ss.len(), tq(ss.len() - 1), table_beta(0, 1000) as f64 / 1000.0);
    for (k, nm) in ["B0 (default being)", "B1p (wt_file_key on, wt_file_beta table)", "B1 (W2, beta 2)"].iter().enumerate() {
        let (a, b, c) = ci(&col(k, &rpe));
        println!("# {nm}: reward per energy {a:.3} [{b:.3}, {c:.3}]");
    }
    let pass = lo >= 0.05 && m >= 0.10 && w >= 45;
    println!("# PRIMARY B1p minus B0: {m:+.3} [{lo:+.3}, {hi:+.3}] ({w}/{t}/{l}), paired sd {:.3}; rule lower >= +0.05: {}, mean >= +0.10: {}, wins >= 45: {} -> {}", sd(&d), lo >= 0.05, m >= 0.10, w >= 45, if pass { "MET" } else { "NOT MET" });
    let same_b1 = rows.iter().filter(|r| r.1[1].rpe == r.1[2].rpe && r.1[4].rpe == r.1[5].rpe).count();
    println!("#   B1p minus B1 (beta 2): {}; worlds with B1p = B1 exactly (default and OOD): {same_b1} of {}", paired(&b1p, &col(2, &rpe)), ss.len());
    let wr = |m: &Mx| m.wrong100;
    println!("# (a) wrong answers per 100 tasks: B0 {:.2}, B1p {:.2}, B1 {:.2}; B1p minus B0 {} (W = B1p more)", mean(&col(0, &wr)), mean(&col(1, &wr)), mean(&col(2, &wr)), paired(&col(1, &wr), &col(0, &wr)));
    println!("#     right share of answers: B0 {:.4}, B1p {:.4}; skip share B0 {:.4}, B1p {:.4}", mean(&col(0, &|m| m.right_ans)), mean(&col(1, &|m| m.right_ans)), mean(&col(0, &|m| m.skip)), mean(&col(1, &|m| m.skip)));
    let (o0, o1) = (col(3, &rpe), col(4, &rpe));
    let (_, _, ohi) = ci(&diff(&o1, &o0));
    println!("# (c) OOD (ood = true): B0 {:.3}, B1p {:.3}; B1p minus B0 {}; significantly negative (CI entirely below 0): {}", mean(&o0), mean(&o1), paired(&o1, &o0), ohi < 0.0);
    println!("#     OOD wrong per 100: B0 {:.2}, B1p {:.2}; B1p minus B0 {}", mean(&col(3, &wr)), mean(&col(4, &wr)), paired(&col(4, &wr), &col(3, &wr)));
    let sumc = |k: usize, f: &dyn Fn(&Mx) -> usize| rows.iter().map(|r| f(&r.1[k])).sum::<usize>();
    println!("# (f) wrong answers accepted (paid): B0 {}, B1p {}, OOD B0 {}, B1p {}; rules accepted B0 {}, B1p {} (on noisy series: B0 {}, B1p {}; OOD B0 {}, B1p {})", sumc(0, &|m| m.wrong_accepted), sumc(1, &|m| m.wrong_accepted), sumc(3, &|m| m.wrong_accepted), sumc(4, &|m| m.wrong_accepted), sumc(0, &|m| m.rules), sumc(1, &|m| m.rules), sumc(0, &|m| m.rules_noisy), sumc(1, &|m| m.rules_noisy), sumc(3, &|m| m.rules_noisy), sumc(4, &|m| m.rules_noisy));
    let fk = col(1, &|m| m.file_keys as f64);
    println!("#     file keys per life (B1p): mean {:.0}, min {:.0}, max {:.0}; wall per life ({} lives in parallel) B0 {:.2} s, B1p {:.2} s; parallel part {par_wall:.0} s",
        mean(&fk), fk.iter().cloned().fold(f64::MAX, f64::min), fk.iter().cloned().fold(0.0, f64::max), rayon::current_num_threads(), mean(&col(0, &|m| m.wall)), mean(&col(1, &|m| m.wall)));
    // cost: the same default lives again, one after another on this thread (timing only; results must be equal)
    let mut walls = [vec![], vec![]];
    let mut equal = true;
    for (i, s) in ss.iter().enumerate() {
        let cfg = WTCfg::new(*s, DEFAULT_BUDGET);
        for k in 0..2 {
            let l = life(&cfg, &mut LearnerT { cfg: lc[k] }, lc[k]);
            equal &= l.rpe() == rows[i].1[k].rpe;
            walls[k].push(l.wall);
        }
    }
    println!("# (f) cost, the default lives again one after another on one thread (timing only; same results: {equal}): wall per life B0 {:.3} s (min {:.3}, max {:.3}), B1p {:.3} s (min {:.3}, max {:.3}); peak working set of this process {:.1} MB",
        mean(&walls[0]), walls[0].iter().cloned().fold(f64::MAX, f64::min), walls[0].iter().cloned().fold(0.0, f64::max), mean(&walls[1]), walls[1].iter().cloned().fold(f64::MAX, f64::min), walls[1].iter().cloned().fold(0.0, f64::max), peak_mem() as f64 / 1e6);
    println!("# wall {:.0} s; per-world rows in {out}", t0.elapsed().as_secs_f64());
}

// ------------------------------------------------------------------ the sweep (b), (d), (e)

fn cmd_sweep(args: &[String]) {
    let out = arg(args, "--out").expect("--out FILE.tsv (per-world rows)");
    let ch = chosen_from_file();
    check_table(&ch);
    let g = grid();
    println!("# tuned configurations per point (from docs/data/{TUNE_FILE}, seeds 101..120):");
    print_choices(&ch);
    let names = ["B1p", "B1 beta 2", "B0", "LinUCB", "LinUCB+file", "UCB1", "always SQL"];
    let t0 = std::time::Instant::now();
    let rseeds: Vec<u64> = (REPORT.0..=REPORT.1).collect();
    let rep: Vec<Vec<Vec<M>>> = rseeds
        .par_iter()
        .map(|s| {
            let mut cache = ToolCache::new();
            let r = g
                .iter()
                .zip(&ch)
                .map(|((p, c), t)| {
                    let cfg = wcfg(*s, *p, *c);
                    let ps = [Pol::Being(b1p_cfg()), Pol::Being(b1_cfg(2000)), Pol::Being(being_cfg(t.b0())), Pol::Lin(lin_cfg(t.lin())), Pol::LinFile(lin_cfg(t.linf())), Pol::Ucb(ucb_cfg(t.ucb())), Pol::AlwaysSql];
                    ps.iter().map(|pol| metrics(&run_life_cached(&cfg, *pol, None, &mut cache))).collect()
                })
                .collect();
            eprintln!("report world {s} done at {:.0} s", t0.elapsed().as_secs_f64());
            r
        })
        .collect();
    let rwall = t0.elapsed().as_secs_f64();
    let mut rt = String::from("seed\tpenalty_pm\tcost_pm\tpolicy\tconfig\trpe\twrong_per100\tright_of_answered\tcoverage\tskip_share\twrong_accepted\trules_noisy\n");
    for (si, s) in rseeds.iter().enumerate() {
        for (gi, (p, c)) in g.iter().enumerate() {
            let t = &ch[gi];
            let cf = [format!("beta={}", t.b1p() as f64 / 1000.0), "beta=2".into(), format!("beta={}", t.b0() as f64 / 1000.0), format!("alpha={}", t.lin()), format!("alpha={}", t.linf()), format!("c={}", t.ucb()), "-".into()];
            for (k, nm) in names.iter().enumerate() {
                let m = &rep[si][gi][k];
                rt.push_str(&format!("{s}\t{p}\t{c}\t{nm}\t{}\t{:.6}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{}\t{}\n", cf[k], m.rpe, m.wrong100, m.right_ans, m.coverage, m.skip, m.wrong_accepted, m.rules_noisy));
            }
        }
    }
    std::fs::write(&out, rt).expect("write --out");
    println!("\n# report seeds {}..{} (n = {}); report wall {rwall:.0} s", REPORT.0, REPORT.1, rseeds.len());
    let at = |gi: usize, k: usize, f: &dyn Fn(&M) -> f64| rep.iter().map(|s| f(&s[gi][k])).collect::<Vec<f64>>();
    let rpe = |m: &M| m.rpe;
    let w = |m: &M| m.wrong100;
    let sk = |m: &M| m.skip;
    // (b) B1p against every policy
    println!("\n(b) Paired differences, reward per energy, mean [95% CI] (wins/ties/losses of B1p):");
    println!("| penalty | weight | B1p beta | B1p minus B0 | one-sided p (B1p-B0<0) | B1p minus LinUCB | B1p minus LinUCB+file | B1p minus better LinUCB | B1p minus UCB1 | B1p minus always SQL |");
    println!("|---|---|---|---|---|---|---|---|---|---|");
    let (mut pv, mut pv_d) = (vec![], vec![]);
    let (mut up, mut down, mut neg, mut neg_d) = (0, 0, vec![], vec![]);
    let (mut lin_up, mut lin_down) = (0, 0);
    for (gi, (p, c)) in g.iter().enumerate() {
        let (b1p, b1, b0) = (at(gi, 0, &rpe), at(gi, 1, &rpe), at(gi, 2, &rpe));
        let d = diff(&b1p, &b0);
        let (m, lo, hi) = ci(&d);
        up += (lo > 0.0) as usize;
        down += (hi < 0.0) as usize;
        if m < 0.0 {
            neg.push(format!("{}: {}", pt(*p, *c), paired(&b1p, &b0)));
        }
        let dd = diff(&b1, &b0);
        if mean(&dd) < 0.0 {
            neg_d.push(format!("{}: {}", pt(*p, *c), paired(&b1, &b0)));
        }
        pv.push(p_below0(&d));
        pv_d.push(p_below0(&dd));
        let (l, lf) = (at(gi, 3, &rpe), at(gi, 4, &rpe));
        let better = if mean(&lf) > mean(&l) { (&lf, "LinUCB+file") } else { (&l, "LinUCB") };
        let (_, blo, bhi) = ci(&diff(&b1p, better.0));
        lin_up += (blo > 0.0) as usize;
        lin_down += (bhi < 0.0) as usize;
        println!("| {:.3} | {:.3} | {} | {} | {:.2e} | {} | {} | {} ({}) | {} | {} |", *p as f64 / 1000.0, *c as f64 / 1000.0, ch[gi].b1p() as f64 / 1000.0, paired(&b1p, &b0), pv[gi], paired(&b1p, &l), paired(&b1p, &lf), paired(&b1p, better.0), better.1, paired(&b1p, &at(gi, 5, &rpe)), paired(&b1p, &at(gi, 6, &rpe)));
    }
    let rej = holm(&pv, 0.05);
    let rej_d = holm(&pv_d, 0.05);
    let lst = |r: &[bool]| g.iter().zip(r).filter(|(_, x)| **x).map(|((p, c), _)| format!("[{}]", pt(*p, *c))).collect::<Vec<_>>().join(" ");
    println!("\nB1p minus B0: interval above 0 at {up} of 30 points, below 0 at {down}");
    println!("Holm (one-sided paired t of B1p minus B0 < 0, 30 points, FWER 0.05): regressions at {} points {}", rej.iter().filter(|x| **x).count(), lst(&rej));
    println!("Points with a negative mean B1p minus B0 ({}): {}", neg.len(), if neg.is_empty() { "none".to_string() } else { neg.join("; ") });
    println!("B1p against the better LinUCB variant per point: interval above 0 at {lin_up}, below 0 at {lin_down}");
    // (a) at every point, skip share and wrong per 100 for every policy
    println!("\n(a), (b) Wrong answers per 100 tasks / skip share (means over the report seeds), and B1p minus B0 wrong per 100:");
    println!("| penalty | weight | {} | B1p minus B0 wrong per 100 |", names.join(" | "));
    println!("|---|---|{}---|", "---|".repeat(names.len()));
    for (gi, (p, c)) in g.iter().enumerate() {
        let cells: Vec<String> = (0..names.len()).map(|k| format!("{:.2} / {:.3}", mean(&at(gi, k, &w)), mean(&at(gi, k, &sk)))).collect();
        println!("| {:.3} | {:.3} | {} | {} |", *p as f64 / 1000.0, *c as f64 / 1000.0, cells.join(" | "), paired(&at(gi, 0, &w), &at(gi, 2, &w)));
    }
    // (d) W2's B1 with beta 2 on the same seeds, and B1p minus B1 (what tuning alone changes)
    println!("\n(d) W2's B1 (beta 2) on seeds 261..320: B1 minus B0, and B1p minus B1:");
    println!("| penalty | weight | B1p beta | B1 minus B0 | B1p minus B1 |");
    println!("|---|---|---|---|---|");
    let mut same_pts = 0;
    for (gi, (p, c)) in g.iter().enumerate() {
        let (b1p, b1, b0) = (at(gi, 0, &rpe), at(gi, 1, &rpe), at(gi, 2, &rpe));
        if b1p == b1 {
            same_pts += 1;
        }
        println!("| {:.3} | {:.3} | {} | {} | {} |", *p as f64 / 1000.0, *c as f64 / 1000.0, ch[gi].b1p() as f64 / 1000.0, paired(&b1, &b0), paired(&b1p, &b1));
    }
    println!("B1 (beta 2) minus B0, Holm as above: regressions at {} points {}", rej_d.iter().filter(|x| **x).count(), lst(&rej_d));
    println!("Points with a negative mean B1 minus B0 ({}): {}", neg_d.len(), neg_d.join("; "));
    println!("Points where B1p and B1 are identical in every world: {same_pts} (table beta 2 at {})", ch.iter().filter(|x| x.b1p() == 2000).count());
    // (e) M4
    println!("\n(e) M4 (docs/milestones.md): a point is dominated if a baseline (LinUCB, LinUCB+file, UCB1, always SQL) has higher reward per energy (paired CI above 0) and fewer wrong answers per 100 tasks (mean)");
    for (bk, bname) in [(0usize, "B1p"), (1, "B1 beta 2"), (2, "B0 (tuned)")] {
        let mut pts = vec![];
        let mut pairs = 0;
        for (gi, (p, c)) in g.iter().enumerate() {
            let mut by = vec![];
            for (k, nm) in names.iter().enumerate().skip(3) {
                let (_, lo, _) = ci(&diff(&at(gi, k, &rpe), &at(gi, bk, &rpe)));
                if lo > 0.0 && mean(&at(gi, k, &w)) < mean(&at(gi, bk, &w)) {
                    by.push(format!("{nm} {} rpe, wrong per 100 {:.2} vs {:.2}", paired(&at(gi, k, &rpe), &at(gi, bk, &rpe)), mean(&at(gi, k, &w)), mean(&at(gi, bk, &w))));
                }
            }
            if !by.is_empty() {
                pairs += by.len();
                pts.push(format!("{}: {}", pt(*p, *c), by.join("; ")));
            }
        }
        println!("  {bname}: dominated at {} points ({pairs} (point, baseline) pairs){}", pts.len(), if pts.is_empty() { String::new() } else { format!(":\n    {}", pts.join("\n    ")) });
    }
    let wa: usize = rep.iter().flat_map(|s| s.iter().map(|g| g[0].wrong_accepted + g[1].wrong_accepted + g[2].wrong_accepted)).sum();
    let rn: usize = rep.iter().flat_map(|s| s.iter().map(|g| g[0].rules_noisy + g[1].rules_noisy + g[2].rules_noisy)).sum();
    println!("\n(f) over the sweep: wrong answers accepted (B1p + B1 + B0) {wa}; rules on noisy series (B1p + B1 + B0) {rn}");
    println!("total wall {:.0} s", t0.elapsed().as_secs_f64());
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("tune") => cmd_tune(&args[1..]),
        Some("table") => cmd_table(),
        Some("equiv") => cmd_equiv(&args[1..]),
        Some("primary") => cmd_primary(&args[1..]),
        Some("sweep") => cmd_sweep(&args[1..]),
        _ => eprintln!("usage: measure_w2b tune --out F | table | equiv --out F | primary --out F | sweep --out F"),
    }
}
