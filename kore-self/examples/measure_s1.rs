//! Track S1 (docs/s1_self_tune_prereg.md): the being tunes its own beta during its life. Measurement only.
//!   cargo run -p kore-self --example measure_s1 --offline --release -- equiv   [tuning seeds 101..120 only]
//!   cargo run -p kore-self --example measure_s1 --offline --release -- primary --out FILE.tsv   [report seeds 321..380]
//!   cargo run -p kore-self --example measure_s1 --offline --release -- sweep --out-dir DIR [--b1p-table FILE] [--retune]   [report seeds 321..380]
//!   cargo run -p kore-self --example measure_s1 --offline --release -- cost   [report seeds 321..380, one thread]
//! Every command takes `--seeds A..B` (default: the report seeds; tuning seeds 101..120 only for a smoke check,
//! whose numbers are not results: the output then says so on every summary line).
//!
//! `equiv`: the pre-registered equivalence and safety tests 1 and 2 on every tuning seed 101..120 (World T, penalty
//! 0, energy weight 1, budget 6000; `--penalty PM` for another penalty). Per seed: (1) the self-tuned life in the
//! measurement loop, keeping the state at the start of every block; for every block, B1 (file key on, prior weight 2)
//! with that block's beta run from that state in a fresh engine must take the same decisions; the harness function
//! `self_tune::run_life_s1` must take the same decisions; (2) the logged being (`run_t_self`, a log on disk) in one
//! run, with a restart in the middle of a block from the full fold, and with that restart from a checkpoint (the
//! restore must use it): same decisions, reward and energy; (3) on every final log, `replay_t` (every learner
//! decision) and `self_tune::replay` (every block pick and promotion) report 0 mismatches.
//!
//! `primary`: PRIMARY (penalty 10 x reward, energy weight 1: S1 minus B1 with beta 2, reward per energy; pass: 95%
//! paired t-CI lower bound > 0 and S1 wins in >= 36 of 60), the guard (penalty 0, weight 1: the CI not entirely below
//! 0), (d) OOD (`ood = true`, penalty 0, weight 1: S1 minus B1 beta 2), (e) cost (wall per life, extra revisions and
//! cause links per life), (f) wrong answers accepted and rules on noisy series, plus the block choices at those points.
//!
//! `sweep`: (a) the 6 x 5 grid: at every point S1 minus B1 (beta 2), S1 minus B0 (tuned per point on seeds 101..120
//! as in W2: by default the configurations are picked from W2's committed tuning table
//! `docs/data/w2_sweep_tune_seeds101-120.tsv` with W2's rule; `--retune` recomputes it), S1 minus B1p (the W2b
//! table, `--b1p-table FILE` with lines `penalty_pm cost_pm beta`; left out and said so without it), S1 minus the
//! better LinUCB variant (LinUCB, LinUCB + file, alphas tuned as in W2); wrong answers per 100 and skip share for
//! every policy; Holm over the 30 points for "S1 minus B1 (beta 2) < 0" and "S1 minus B0 < 0". (b) S1 minus B1 with
//! the best fixed beta of the grid for that world in hindsight. (c) share of blocks per beta, final incumbent per
//! point, promotions per life. (f) over the sweep.
//!
//! Files are the generator's bytes in memory (`MemSandbox`, as W2). Statistics as in measure_w2 (paired
//! t-intervals, 95%, t quantile for n - 1 degrees of freedom; one-sided paired t; Holm step-down at FWER 0.05).

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::policy::Policy;
use being::world_t::baselines::LinCfg;
use being::world_t::engine::{replay_t, run_t_self, ChooserT, EngineT, MemSandbox, RunTCfg, StateT, ToolCache, DEFAULT_DISCOVER_EVERY};
use being::world_t::learner::{LearnerT, LearnerTCfg};
use being::world_t::measure::{run_life_cached, LifeT, Pol};
use being::world_t::self_tune::{self, SelfTune, SelfTunedT, BLOCK, GRID};
use being::world_t::world::{Plan, WTCfg, WorldT, DEFAULT_BUDGET};
use rayon::prelude::*;
use std::collections::HashMap;

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

const TUNE: (u64, u64) = (101, 120);
const REPORT: (u64, u64) = (321, 380);
const HARSH: (u32, u32) = (10_000, 1000);
const MAIN: (u32, u32) = (0, 1000);

fn seeds(args: &[String], def: (u64, u64)) -> Vec<u64> {
    let (a, b) = match arg(args, "--seeds") {
        Some(s) => {
            let (a, b) = s.split_once("..").expect("--seeds A..B");
            (a.parse().unwrap(), b.parse().unwrap())
        }
        None => def,
    };
    (a..=b).collect()
}

/// A run on tuning seeds is a smoke check of the code, not a measurement.
fn tag_line(ss: &[u64]) -> &'static str {
    if ss.iter().any(|s| (TUNE.0..=TUNE.1).contains(s)) {
        "[SMOKE CHECK on tuning seeds: not a result] "
    } else if ss.iter().all(|s| (REPORT.0..=REPORT.1).contains(s)) {
        ""
    } else {
        "[seeds outside the pre-registered report seeds: not a result] "
    }
}

fn b1_cfg(beta: u32) -> LearnerTCfg {
    LearnerTCfg { beta_milli: beta, file_key: true, ..Default::default() }
}
fn b0_cfg(beta: u32) -> LearnerTCfg {
    LearnerTCfg { beta_milli: beta, ..Default::default() }
}
fn wcfg(seed: u64, p: u32, c: u32) -> WTCfg {
    WTCfg { penalty_pm: p, cost_pm: c, ..WTCfg::new(seed, DEFAULT_BUDGET) }
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

// ------------------------------------------------------------------ one life and its metrics

#[derive(Clone, Debug, Default)]
struct M {
    rpe: f64,
    wrong100: f64,
    skip: f64,
    tasks: usize,
    wrong_accepted: usize,
    rules: usize,
    rules_noisy: usize,
    wall: f64,
    /// S1 only: blocks per grid beta, final incumbent, promotions, extra revisions and cause links.
    blocks: [usize; 6],
    incumbent: u32,
    promotions: usize,
    /// (from, to) of every promotion.
    promos: Vec<(u32, u32)>,
    revisions: usize,
    cause_links: usize,
}

fn metrics(l: &LifeT, wall: f64) -> M {
    let n = l.truths.len().max(1) as f64;
    let wrong = l.truths.iter().filter(|t| t.wrong).count() as f64;
    let skip = l.truths.iter().filter(|t| t.plan.is_none()).count() as f64;
    M { rpe: l.rpe(), wrong100: 100.0 * wrong / n, skip: skip / n, tasks: l.truths.len(), wrong_accepted: l.truths.iter().filter(|t| t.wrong_accepted).count(), rules: l.rules, rules_noisy: l.rules_noisy, wall, ..Default::default() }
}

fn life_pol(cfg: &WTCfg, pol: Pol, cache: &mut ToolCache) -> M {
    let t0 = std::time::Instant::now();
    let l = run_life_cached(cfg, pol, None, cache);
    metrics(&l, t0.elapsed().as_secs_f64())
}

fn life_s1(cfg: &WTCfg, cache: &mut ToolCache) -> M {
    let t0 = std::time::Instant::now();
    let (l, tl) = self_tune::run_life_s1(cfg, None, cache);
    let mut m = metrics(&l, t0.elapsed().as_secs_f64());
    m.blocks = tl.tune.blocks_per_beta();
    m.incumbent = tl.tune.incumbent_milli;
    m.promotions = tl.tune.promotions.len();
    m.promos = tl.tune.promotions.iter().map(|p| (p.from_milli, p.to_milli)).collect();
    m.revisions = tl.revisions;
    m.cause_links = tl.cause_links;
    m
}

fn bname(b: u32) -> String {
    format!("{}", b as f64 / 1000.0)
}

fn choices(ms: &[&M]) -> String {
    let tot: usize = ms.iter().map(|m| m.blocks.iter().sum::<usize>()).sum();
    let share: Vec<String> = (0..6).map(|i| format!("{} {:.3}", bname(GRID[i]), ms.iter().map(|m| m.blocks[i]).sum::<usize>() as f64 / tot.max(1) as f64)).collect();
    let inc: Vec<String> = GRID.iter().map(|g| format!("{} {}", bname(*g), ms.iter().filter(|m| m.incumbent == *g).count())).collect();
    let pr: Vec<f64> = ms.iter().map(|m| m.promotions as f64).collect();
    format!("block share per beta: {}; final incumbent (worlds): {}; promotions per life {:.2} (max {:.0}); blocks per life {:.1}", share.join(", "), inc.join(", "), mean(&pr), pr.iter().cloned().fold(0.0, f64::max), tot as f64 / ms.len().max(1) as f64)
}

// ------------------------------------------------------------------ equivalence and safety tests 1 and 2

fn open_log(root: &std::path::Path) -> versioned::VersionedLog {
    versioned::VersionedLog::open_or_create_with_clock(root, "being", || "2030-01-01T00:00:00Z".to_string()).unwrap()
}

/// The self-tuned life in the measurement loop, with the state at the start of every block.
fn s1_with_starts(cfg: &WTCfg) -> (Vec<Option<Plan>>, i64, i64, Vec<StateT>, SelfTune) {
    let mut st = StateT::default();
    st.start(cfg.clone(), self_tune::s1_learner(), "genesis".into());
    st.self_tune = SelfTune::on();
    let mut eng = EngineT::new(st, None);
    eng.measure = true;
    let w = WorldT::new(cfg.clone());
    let (mut plans, mut r, mut e, mut starts) = (vec![], 0, 0, vec![]);
    loop {
        if self_tune::before_step(&mut eng.state).is_some() {
            starts.push(eng.state.clone());
        }
        let Ok(s) = eng.step(&mut SelfTunedT, &MemSandbox(&w)) else { break };
        plans.push(s.truth.plan);
        r += s.truth.reward;
        e += s.truth.cost;
        if eng.state.step % DEFAULT_DISCOVER_EVERY == 0 {
            eng.discover_pass();
        }
    }
    (plans, r, e, starts, eng.state.self_tune.clone())
}

/// B1 with a fixed beta from a state, in a fresh engine, for at most `n` steps.
fn b1_from(st: &StateT, beta: u32, n: u64) -> Vec<Option<Plan>> {
    let w = WorldT::new(st.cfg.clone().unwrap());
    let mut eng = EngineT::new(st.clone(), None);
    eng.measure = true;
    let mut ch = LearnerT { cfg: b1_cfg(beta) };
    let mut plans = vec![];
    for _ in 0..n {
        let Ok(s) = eng.step(&mut ch as &mut dyn ChooserT, &MemSandbox(&w)) else { break };
        plans.push(s.truth.plan);
        if eng.state.step % DEFAULT_DISCOVER_EVERY == 0 {
            eng.discover_pass();
        }
    }
    plans
}

/// The logged self-tuned being in runs of `chunks` steps with a restart from the log between them; returns the plans,
/// reward, energy, whether every restart used a checkpoint, the final fold equals the live state, and the two replays.
fn logged(cfg: &WTCfg, chunks: &[u64], every: u64) -> (Vec<Option<Plan>>, i64, i64, bool, bool, (u64, u64), (u64, u64)) {
    let d = tempfile::tempdir().unwrap();
    let w = WorldT::new(cfg.clone());
    let (mut plans, mut r, mut e, mut used) = (vec![], 0, 0, true);
    let mut last = None;
    for (i, &n) in chunks.iter().enumerate() {
        let mut log = open_log(d.path());
        let mut p = Policy::new(n);
        p.checkpoint_every = every;
        let rc = RunTCfg { world: Some(cfg.clone()), learner: None, discover_every: DEFAULT_DISCOVER_EVERY, ctl: None, sql: None };
        let s = run_t_self(&mut log, &rc, Some(true), Some(true), &p, None, &MemSandbox(&w)).unwrap();
        if i > 0 {
            used &= s.ckpt.restored.from_version.is_some();
        }
        plans.extend(s.truths.iter().map(|t| t.plan));
        r += s.reward;
        e += s.cost;
        last = Some(s.state);
    }
    let log = open_log(d.path());
    let rows: Vec<versioned::EventRow> = log.open_latest().rows();
    let fold_ok = StateT::from_events(&rows) == last.unwrap();
    (plans, r, e, used, fold_ok, replay_t(&rows), self_tune::replay(&rows))
}

fn cmd_equiv(args: &[String]) {
    let t0 = std::time::Instant::now();
    let ss = seeds(args, TUNE);
    assert!(ss.iter().all(|s| (TUNE.0..=TUNE.1).contains(s)), "the equivalence test runs on tuning seeds 101..120 only");
    let pen: u32 = arg(args, "--penalty").map_or(0, |x| x.parse().unwrap());
    let out: Vec<(String, bool)> = ss
        .par_iter()
        .map(|&s| {
            let cfg = wcfg(s, pen, 1000);
            let (plans, r, e, starts, tune) = s1_with_starts(&cfg);
            // (1) every block equals B1 with that block's beta from the same state
            let mut bad_blocks = 0;
            for (k, st) in starts.iter().enumerate() {
                let from = k * BLOCK as usize;
                let to = (from + BLOCK as usize).min(plans.len());
                if b1_from(st, st.self_tune.blocks[k].beta_milli, BLOCK) != plans[from..to] {
                    bad_blocks += 1;
                }
            }
            let (hl, ht) = self_tune::run_life_s1(&cfg, None, &mut ToolCache::new());
            let harness_ok = hl.truths.iter().map(|t| t.plan).collect::<Vec<_>>() == plans && (hl.reward, hl.cost) == (r, e) && ht.tune == tune;
            // (2) the logged being; restarts in the middle of block 7 (step 1477) from the fold and from a checkpoint
            let mid = 7 * BLOCK + 77;
            let big = 1_000_000u64;
            let runs = [logged(&cfg, &[big], 0), logged(&cfg, &[mid, big], 0), logged(&cfg, &[mid, big], 100)];
            let same: Vec<bool> = runs.iter().map(|x| x.0 == plans && (x.1, x.2) == (r, e) && x.4).collect();
            let used = runs[2].3;
            let rp: Vec<String> = runs.iter().map(|x| format!("{}/{}+{}/{}", x.5 .0, x.5 .1, x.6 .0, x.6 .1)).collect();
            let rp_ok = runs.iter().all(|x| x.5 .0 as usize == plans.len() && x.5 .1 == 0 && x.6 .0 as usize == tune.blocks.len() + tune.promotions.len() && x.6 .1 == 0);
            let ok = bad_blocks == 0 && harness_ok && same.iter().all(|x| *x) && used && rp_ok;
            let line = format!(
                "{s}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                plans.len(), tune.blocks.len(), tune.promotions.len(), tune.blocks.iter().map(|b| bname(b.beta_milli)).collect::<Vec<_>>().join(";"), bad_blocks, harness_ok, same[0], same[1], same[2], used, rp.join(" "), ok
            );
            eprintln!("world {s} done at {:.0} s", t0.elapsed().as_secs_f64());
            (line, ok)
        })
        .collect();
    println!("seed\tsteps\tblocks\tpromotions\tbeta_per_block\tblocks_not_equal_to_b1\tharness_fn_same\tsame_logged_one_run\tsame_logged_restart_fold\tsame_logged_restart_checkpoint\tcheckpoint_used\treplay_t+self_tune_replay(checked/mismatches) one/fold/ckpt\tall_ok");
    for (l, _) in &out {
        println!("{l}");
    }
    let n_ok = out.iter().filter(|r| r.1).count();
    println!("# equivalence (penalty {pen} pm): {n_ok} of {} tuning seeds identical in every check; wall {:.0} s", out.len(), t0.elapsed().as_secs_f64());
    if n_ok != out.len() {
        std::process::exit(1);
    }
}

// ------------------------------------------------------------------ PRIMARY, guard, (d), (e), (f)

fn cmd_primary(args: &[String]) {
    let out = arg(args, "--out").expect("--out FILE.tsv (per-world rows)");
    let t0 = std::time::Instant::now();
    let ss = seeds(args, REPORT);
    let tl = tag_line(&ss);
    // per seed: [harsh S1, harsh B1, main S1, main B1, ood S1, ood B1]
    let rows: Vec<(u64, Vec<M>)> = ss
        .par_iter()
        .map(|&s| {
            let mut v = vec![];
            for cfg in [wcfg(s, HARSH.0, HARSH.1), wcfg(s, MAIN.0, MAIN.1), WTCfg { ood: true, ..wcfg(s, MAIN.0, MAIN.1) }] {
                let mut cache = ToolCache::new();
                v.push(life_s1(&cfg, &mut cache));
                v.push(life_pol(&cfg, Pol::Being(b1_cfg(2000)), &mut cache));
            }
            eprintln!("world {s} done at {:.0} s", t0.elapsed().as_secs_f64());
            (s, v)
        })
        .collect();
    let names = ["harsh\tS1", "harsh\tB1", "main\tS1", "main\tB1", "ood\tS1", "ood\tB1"];
    let mut tsv = String::from("seed\tpoint\tpolicy\trpe\twrong_per100\tskip_share\ttasks\twrong_accepted\trules\trules_noisy\tblocks_per_beta(0.5;1;2;4;8;16)\tfinal_incumbent\tpromotions\textra_revisions\tcause_links\twall_s\n");
    for (s, v) in &rows {
        for (k, m) in v.iter().enumerate() {
            tsv.push_str(&format!("{s}\t{}\t{:.6}\t{:.4}\t{:.4}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.2}\n", names[k], m.rpe, m.wrong100, m.skip, m.tasks, m.wrong_accepted, m.rules, m.rules_noisy, m.blocks.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(";"), bname(m.incumbent), m.promotions, m.revisions, m.cause_links, m.wall));
        }
    }
    std::fs::write(&out, &tsv).expect("write --out");
    let col = |k: usize, f: &dyn Fn(&M) -> f64| rows.iter().map(|r| f(&r.1[k])).collect::<Vec<f64>>();
    let rpe = |m: &M| m.rpe;
    println!("# {tl}seeds {}..{} (n = {}), World T, budget {DEFAULT_BUDGET}, default files; t quantile {:.4}", ss[0], ss[ss.len() - 1], ss.len(), tq(ss.len() - 1));
    for (k, nm) in names.iter().enumerate() {
        let (a, b, c) = ci(&col(k, &rpe));
        println!("# {tl}{}: reward per energy {a:.3} [{b:.3}, {c:.3}], wrong per 100 {:.2}, skip share {:.4}", nm.replace('\t', " "), mean(&col(k, &|m| m.wrong100)), mean(&col(k, &|m| m.skip)));
    }
    let d = diff(&col(0, &rpe), &col(1, &rpe));
    let (m, lo, hi) = ci(&d);
    let (w, t, l) = wtl(&d);
    let need = (ss.len() * 36 + 59) / 60;
    let pass = lo > 0.0 && w >= need;
    println!("# {tl}PRIMARY (penalty 10 x reward, weight 1) S1 minus B1 (beta 2): {m:+.3} [{lo:+.3}, {hi:+.3}] ({w}/{t}/{l}), paired sd {:.3}; rule lower > 0: {}, wins >= {need} of {}: {} -> {}", sd(&d), lo > 0.0, ss.len(), w >= need, if pass { "MET" } else { "NOT MET" });
    let dg = diff(&col(2, &rpe), &col(3, &rpe));
    let (_, _, ghi) = ci(&dg);
    println!("# {tl}GUARD (penalty 0, weight 1) S1 minus B1 (beta 2): {}; significantly negative (CI entirely below 0): {} -> guard {}", paired(&col(2, &rpe), &col(3, &rpe)), ghi < 0.0, if ghi < 0.0 { "FAILS" } else { "HOLDS" });
    let (_, _, ohi) = ci(&diff(&col(4, &rpe), &col(5, &rpe)));
    println!("# {tl}(d) OOD (ood = true, penalty 0, weight 1) S1 minus B1 (beta 2): {}; significantly negative: {}", paired(&col(4, &rpe), &col(5, &rpe)), ohi < 0.0);
    for (k, nm) in [(0usize, "harsh"), (2, "main"), (4, "ood")] {
        println!("# {tl}    wrong per 100 S1 minus B1, {nm}: {}", paired(&col(k, &|m| m.wrong100), &col(k + 1, &|m| m.wrong100)));
    }
    for (k, nm) in [(0usize, "harsh"), (2, "main"), (4, "ood")] {
        let ms: Vec<&M> = rows.iter().map(|r| &r.1[k]).collect();
        println!("# {tl}(c) choices at {nm}: {}", choices(&ms));
    }
    let wl = |k: usize| mean(&col(k, &|m| m.wall));
    println!("# {tl}(e) wall per life ({} lives in parallel): S1 harsh {:.2} s, main {:.2} s, ood {:.2} s; B1 harsh {:.2} s, main {:.2} s, ood {:.2} s (`cost` measures one thread)", rayon::current_num_threads(), wl(0), wl(2), wl(4), wl(1), wl(3), wl(5));
    for (k, nm) in [(0usize, "harsh"), (2, "main"), (4, "ood")] {
        let rv = col(k, &|m| m.revisions as f64);
        let cl = col(k, &|m| m.cause_links as f64);
        println!("# {tl}(e) extra revisions per life at {nm}: mean {:.1} (min {:.0}, max {:.0}); extra cause links per life: mean {:.0} (min {:.0}, max {:.0})", mean(&rv), rv.iter().cloned().fold(f64::MAX, f64::min), rv.iter().cloned().fold(0.0, f64::max), mean(&cl), cl.iter().cloned().fold(f64::MAX, f64::min), cl.iter().cloned().fold(0.0, f64::max));
    }
    let sumc = |k: usize, f: &dyn Fn(&M) -> usize| rows.iter().map(|r| f(&r.1[k])).sum::<usize>();
    println!("# {tl}(f) wrong answers accepted (paid): S1 {} / {} / {}, B1 {} / {} / {} (harsh / main / ood); rules on noisy series: S1 {} / {} / {}, B1 {} / {} / {}", sumc(0, &|m| m.wrong_accepted), sumc(2, &|m| m.wrong_accepted), sumc(4, &|m| m.wrong_accepted), sumc(1, &|m| m.wrong_accepted), sumc(3, &|m| m.wrong_accepted), sumc(5, &|m| m.wrong_accepted), sumc(0, &|m| m.rules_noisy), sumc(2, &|m| m.rules_noisy), sumc(4, &|m| m.rules_noisy), sumc(1, &|m| m.rules_noisy), sumc(3, &|m| m.rules_noisy), sumc(5, &|m| m.rules_noisy));
    println!("# {tl}wall {:.0} s; per-world rows in {out}", t0.elapsed().as_secs_f64());
}

fn cmd_cost(args: &[String]) {
    let ss = seeds(args, REPORT);
    let tl = tag_line(&ss);
    let t0 = std::time::Instant::now();
    let (mut ws, mut wb) = (vec![], vec![]);
    for &s in &ss {
        let cfg = wcfg(s, MAIN.0, MAIN.1);
        ws.push(life_s1(&cfg, &mut ToolCache::new()).wall);
        wb.push(life_pol(&cfg, Pol::Being(b1_cfg(2000)), &mut ToolCache::new()).wall);
    }
    println!("# {tl}(e) cost, penalty 0, weight 1, {} lives each one after another on one thread: wall per life S1 {:.3} s (min {:.3}, max {:.3}), B1 {:.3} s; peak working set of the process {:.1} MB; total {:.0} s", ss.len(), mean(&ws), ws.iter().cloned().fold(f64::MAX, f64::min), ws.iter().cloned().fold(0.0, f64::max), mean(&wb), peak_mem() as f64 / 1e6, t0.elapsed().as_secs_f64());
}

// ------------------------------------------------------------------ the sweep

const BETAS: [u32; 6] = GRID;
const UCB_C: [f64; 6] = [0.001, 0.003, 0.01, 0.03, 0.1, 0.3];
const LIN_A: [f64; 6] = [0.1, 0.3, 1.0, 3.0, 10.0, 30.0];
const PENALTY: [u32; 6] = [0, 100, 316, 1000, 3162, 10000];
const COST: [u32; 5] = [100, 316, 1000, 3162, 10000];

fn grid() -> Vec<(u32, u32)> {
    PENALTY.iter().flat_map(|p| COST.iter().map(move |c| (*p, *c))).collect()
}

fn lin_cfg(a: f64) -> LinCfg {
    LinCfg { alpha: a, skip: true }
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

/// Per point: (B0 beta, LinUCB alpha, LinUCB+file alpha), W2's rule (highest mean reward per energy on seeds
/// 101..120 over 6 configurations, the first on a tie). From W2's committed tuning table, or recomputed.
fn tuned(retune: bool) -> Vec<(u32, f64, f64)> {
    let g = grid();
    // per point, per policy (0 B0, 1 LinUCB, 2 LinUCB+file): 6 means
    let means: Vec<[Vec<f64>; 3]> = if retune {
        let tseeds: Vec<u64> = (TUNE.0..=TUNE.1).collect();
        let pols: Vec<Pol> = BETAS.iter().map(|b| Pol::Being(b0_cfg(*b))).chain(LIN_A.iter().map(|a| Pol::Lin(lin_cfg(*a)))).chain(LIN_A.iter().map(|a| Pol::LinFile(lin_cfg(*a)))).collect();
        let t: Vec<Vec<Vec<f64>>> = tseeds.par_iter().map(|s| { let mut cache = ToolCache::new(); g.iter().map(|(p, c)| pols.iter().map(|pol| run_life_cached(&wcfg(*s, *p, *c), *pol, None, &mut cache).rpe()).collect()).collect() }).collect();
        (0..g.len()).map(|gi| { let m = |k: usize| mean(&t.iter().map(|s| s[gi][k]).collect::<Vec<_>>()); [(0..6).map(m).collect(), (6..12).map(m).collect(), (12..18).map(m).collect()] }).collect()
    } else {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("docs").join("data").join("w2_sweep_tune_seeds101-120.tsv");
        let text = std::fs::read_to_string(&p).expect("docs/data/w2_sweep_tune_seeds101-120.tsv (or use --retune)");
        let mut acc: HashMap<(u32, u32, usize, usize), Vec<f64>> = HashMap::new();
        for l in text.lines().skip(1) {
            let c: Vec<&str> = l.split('\t').collect();
            if c.len() < 6 {
                continue;
            }
            let (pp, cc): (u32, u32) = (c[1].parse().unwrap(), c[2].parse().unwrap());
            let v: f64 = c[3 + 1].split_once('=').unwrap().1.parse().unwrap();
            let (k, i) = match c[3] {
                "B0" => (0, BETAS.iter().position(|b| (*b as f64 / 1000.0 - v).abs() < 1e-9).unwrap()),
                "LinUCB" => (1, LIN_A.iter().position(|a| (a - v).abs() < 1e-9).unwrap()),
                "LinUCB+file" => (2, LIN_A.iter().position(|a| (a - v).abs() < 1e-9).unwrap()),
                _ => continue,
            };
            acc.entry((pp, cc, k, i)).or_default().push(c[5].parse().unwrap());
        }
        g.iter().map(|(p, c)| [0usize, 1, 2].map(|k| (0..6).map(|i| { let v = &acc[&(*p, *c, k, i)]; assert_eq!(v.len(), 20); mean(v) }).collect::<Vec<f64>>())).collect()
    };
    means.iter().map(|m| (BETAS[pick(&m[0])], LIN_A[pick(&m[1])], LIN_A[pick(&m[2])])).collect()
}

/// The W2b table: lines `penalty_pm cost_pm beta` (beta in units, e.g. 0.5, or per mille, e.g. 500); other lines
/// are ignored. Every sweep point must be in it and every beta in the grid.
fn b1p_table(path: &str) -> HashMap<(u32, u32), u32> {
    let text = std::fs::read_to_string(path).expect("--b1p-table FILE");
    let mut t = HashMap::new();
    for l in text.lines() {
        let c: Vec<&str> = l.split(|ch: char| ch == '\t' || ch == ' ' || ch == ',').filter(|x| !x.is_empty()).collect();
        if c.len() < 3 {
            continue;
        }
        let (Ok(p), Ok(cc), Ok(b)) = (c[0].parse::<u32>(), c[1].parse::<u32>(), c[2].trim_start_matches("beta=").parse::<f64>()) else { continue };
        let bm = if b < 100.0 { (b * 1000.0).round() as u32 } else { b as u32 };
        assert!(GRID.contains(&bm), "B1p beta {b} at ({p}, {cc}) is not in the grid");
        t.insert((p, cc), bm);
    }
    for pt in grid() {
        assert!(t.contains_key(&pt), "the B1p table has no entry for {pt:?}");
    }
    t
}

fn cmd_sweep(args: &[String]) {
    let dir = arg(args, "--out-dir").expect("--out-dir DIR (per-world TSV files)");
    std::fs::create_dir_all(&dir).unwrap();
    let ss = seeds(args, REPORT);
    let tl = tag_line(&ss);
    let g = grid();
    let t0 = std::time::Instant::now();
    let chosen = tuned(args.iter().any(|a| a == "--retune"));
    let b1p = arg(args, "--b1p-table").map(|p| b1p_table(&p));
    println!("{tl}Tuned per point on seeds 101..120 ({}): B0 beta / LinUCB alpha / LinUCB+file alpha{}", if args.iter().any(|a| a == "--retune") { "recomputed" } else { "W2's committed tuning table" }, if b1p.is_some() { " / B1p beta (W2b table)" } else { "; B1p: no W2b table given (--b1p-table), left out" });
    for (gi, (p, c)) in g.iter().enumerate() {
        let t = chosen[gi];
        println!("  penalty {:.3} weight {:.3}: {} / {} / {}{}", *p as f64 / 1000.0, *c as f64 / 1000.0, bname(t.0), t.1, t.2, b1p.as_ref().map_or(String::new(), |m| format!(" / {}", bname(m[&(*p, *c)]))));
    }
    // per seed, per point: [S1, B1 beta 0.5 .. 16 (6), B0 tuned, LinUCB, LinUCB+file]
    let names = ["S1", "B1 beta 0.5", "B1 beta 1", "B1 beta 2", "B1 beta 4", "B1 beta 8", "B1 beta 16", "B0", "LinUCB", "LinUCB+file"];
    let rep: Vec<Vec<Vec<M>>> = ss
        .par_iter()
        .map(|s| {
            let mut cache = ToolCache::new();
            let r = g
                .iter()
                .zip(&chosen)
                .map(|((p, c), t)| {
                    let cfg = wcfg(*s, *p, *c);
                    let mut out = vec![life_s1(&cfg, &mut cache)];
                    for b in BETAS {
                        out.push(life_pol(&cfg, Pol::Being(b1_cfg(b)), &mut cache));
                    }
                    out.push(life_pol(&cfg, Pol::Being(b0_cfg(t.0)), &mut cache));
                    out.push(life_pol(&cfg, Pol::Lin(lin_cfg(t.1)), &mut cache));
                    out.push(life_pol(&cfg, Pol::LinFile(lin_cfg(t.2)), &mut cache));
                    out
                })
                .collect();
            eprintln!("world {s} done at {:.0} s", t0.elapsed().as_secs_f64());
            r
        })
        .collect();
    let mut rt = String::from("seed\tpenalty_pm\tcost_pm\tpolicy\tconfig\trpe\twrong_per100\tskip_share\twrong_accepted\trules_noisy\tblocks_per_beta(0.5;1;2;4;8;16)\tfinal_incumbent\tpromotions\textra_revisions\tcause_links\twall_s\n");
    for (si, s) in ss.iter().enumerate() {
        for (gi, (p, c)) in g.iter().enumerate() {
            let t = chosen[gi];
            for (k, nm) in names.iter().enumerate() {
                let m = &rep[si][gi][k];
                let cf = match k { 0 => "self-tuned".to_string(), 1..=6 => format!("beta={}", bname(BETAS[k - 1])), 7 => format!("beta={}", bname(t.0)), 8 => format!("alpha={}", t.1), _ => format!("alpha={}", t.2) };
                rt.push_str(&format!("{s}\t{p}\t{c}\t{nm}\t{cf}\t{:.6}\t{:.4}\t{:.4}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.2}\n", m.rpe, m.wrong100, m.skip, m.wrong_accepted, m.rules_noisy, m.blocks.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(";"), if k == 0 { bname(m.incumbent) } else { "-".into() }, m.promotions, m.revisions, m.cause_links, m.wall));
            }
        }
    }
    let fname = format!("{dir}/s1_sweep_seeds{}-{}.tsv", ss[0], ss[ss.len() - 1]);
    std::fs::write(&fname, rt).unwrap();
    let at = |gi: usize, k: usize, f: &dyn Fn(&M) -> f64| rep.iter().map(|s| f(&s[gi][k])).collect::<Vec<f64>>();
    let rpe = |m: &M| m.rpe;
    let b1p_k = |gi: usize| b1p.as_ref().map(|m| 1 + BETAS.iter().position(|b| *b == m[&g[gi]]).unwrap());
    println!("\n{tl}seeds {}..{} (n = {}); paired differences, reward per energy, mean [95% CI] (wins/ties/losses of S1)", ss[0], ss[ss.len() - 1], ss.len());
    println!("| penalty | weight | S1 minus B1 (beta 2) | S1 minus B0 (tuned) | S1 minus B1p | S1 minus better LinUCB | S1 minus best fixed beta in hindsight | p (S1-B1<0) | p (S1-B0<0) |");
    println!("|---|---|---|---|---|---|---|---|---|");
    let (mut pv1, mut pv0) = (vec![], vec![]);
    let mut hind = vec![];
    for (gi, (p, c)) in g.iter().enumerate() {
        let s1 = at(gi, 0, &rpe);
        let b1 = at(gi, 3, &rpe);
        let b0 = at(gi, 7, &rpe);
        let (l, lf) = (at(gi, 8, &rpe), at(gi, 9, &rpe));
        let better = if mean(&lf) > mean(&l) { (lf, "LinUCB+file") } else { (l, "LinUCB") };
        // (b) the best fixed beta of the grid for each world in hindsight
        let best: Vec<f64> = (0..ss.len()).map(|si| (1..=6).map(|k| rep[si][gi][k].rpe).fold(f64::MIN, f64::max)).collect();
        let gain_possible = mean(&diff(&best, &b1));
        let gain_found = mean(&diff(&s1, &b1));
        hind.push((paired(&s1, &best), gain_found, gain_possible));
        let p1 = p_below0(&diff(&s1, &b1));
        let p0 = p_below0(&diff(&s1, &b0));
        pv1.push(p1);
        pv0.push(p0);
        let pb = b1p_k(gi).map_or("left out (no W2b table)".to_string(), |k| paired(&s1, &at(gi, k, &rpe)));
        println!("| {:.3} | {:.3} | {} | {} | {} | {} ({}) | {} | {:.2e} | {:.2e} |", *p as f64 / 1000.0, *c as f64 / 1000.0, paired(&s1, &b1), paired(&s1, &b0), pb, paired(&s1, &better.0), better.1, paired(&s1, &best), p1, p0);
    }
    let r1 = holm(&pv1, 0.05);
    let r0 = holm(&pv0, 0.05);
    let list = |r: &[bool]| g.iter().zip(r).filter(|(_, x)| **x).map(|((p, c), _)| format!(" [penalty {:.3}, weight {:.3}]", *p as f64 / 1000.0, *c as f64 / 1000.0)).collect::<String>();
    println!("\n{tl}Holm (one-sided paired t, 30 points, FWER 0.05): S1 minus B1 (beta 2) < 0 rejected at {} points{}", r1.iter().filter(|x| **x).count(), list(&r1));
    println!("{tl}Holm (one-sided paired t, 30 points, FWER 0.05): S1 minus B0 (tuned) < 0 rejected at {} points{}", r0.iter().filter(|x| **x).count(), list(&r0));
    let neg = |k: usize| g.iter().enumerate().filter(|(gi, _)| mean(&diff(&at(*gi, 0, &rpe), &at(*gi, k, &rpe))) < 0.0).map(|(_, (p, c))| format!("({:.3}, {:.3})", *p as f64 / 1000.0, *c as f64 / 1000.0)).collect::<Vec<_>>().join(" ");
    println!("{tl}Points with a negative mean: S1 minus B1 (beta 2): {}; S1 minus B0: {}", neg(3), neg(7));
    println!("\n{tl}(b) Share of the possible gain over B1 (beta 2) that S1 finds, per point: mean(S1 - B1) / mean(best fixed beta in hindsight - B1)");
    for (gi, (p, c)) in g.iter().enumerate() {
        let (_, f, pz) = &hind[gi];
        println!("  penalty {:.3} weight {:.3}: found {f:+.3} of possible {pz:+.3}{}", *p as f64 / 1000.0, *c as f64 / 1000.0, if *pz > 1e-12 { format!(" ({:.2})", f / pz) } else { String::new() });
    }
    println!("\n{tl}Wrong answers per 100 tasks / skip share (means), per policy:");
    let pols: Vec<(usize, &str)> = vec![(0, "S1"), (3, "B1 beta 2"), (7, "B0"), (8, "LinUCB"), (9, "LinUCB+file")];
    println!("| penalty | weight | {} | B1p |", pols.iter().map(|x| x.1).collect::<Vec<_>>().join(" | "));
    println!("|---|---|{}---|", "---|".repeat(pols.len()));
    for (gi, (p, c)) in g.iter().enumerate() {
        let cell = |k: usize| format!("{:.2} / {:.3}", mean(&at(gi, k, &|m: &M| m.wrong100)), mean(&at(gi, k, &|m: &M| m.skip)));
        println!("| {:.3} | {:.3} | {} | {} |", *p as f64 / 1000.0, *c as f64 / 1000.0, pols.iter().map(|x| cell(x.0)).collect::<Vec<_>>().join(" | "), b1p_k(gi).map_or("-".to_string(), cell));
    }
    println!("\n{tl}(c) Choices per point:");
    for (gi, (p, c)) in g.iter().enumerate() {
        let ms: Vec<&M> = rep.iter().map(|s| &s[gi][0]).collect();
        println!("  penalty {:.3} weight {:.3}: {}", *p as f64 / 1000.0, *c as f64 / 1000.0, choices(&ms));
    }
    // (f): wrong answers accepted, noisy rules; promotions to a beta that is worse in hindsight for that world
    let wa: usize = rep.iter().flat_map(|s| s.iter().map(|x| x[0].wrong_accepted)).sum();
    let rn: usize = rep.iter().flat_map(|s| s.iter().map(|x| x[0].rules_noisy)).sum();
    let (mut np, mut worse) = (0, 0);
    for s in &rep {
        for x in s {
            for (from, to) in &x[0].promos {
                np += 1;
                let r = |b: u32| x[1 + BETAS.iter().position(|g| *g == b).unwrap()].rpe;
                if r(*to) < r(*from) {
                    worse += 1;
                }
            }
        }
    }
    println!("\n{tl}(f) over the sweep (S1): wrong answers accepted {wa}; rules on noisy series {rn}; promotions {np}, of which to a beta whose fixed-beta life in that world has lower reward per energy than the replaced incumbent's: {worse}");
    println!("{tl}per-world rows in {fname}; total wall {:.0} s", t0.elapsed().as_secs_f64());
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("equiv") => cmd_equiv(&args[1..]),
        Some("primary") => cmd_primary(&args[1..]),
        Some("cost") => cmd_cost(&args[1..]),
        Some("sweep") => cmd_sweep(&args[1..]),
        _ => eprintln!("usage: measure_s1 equiv [--seeds A..B] [--penalty PM] | primary --out F [--seeds A..B] | cost [--seeds A..B] | sweep --out-dir D [--b1p-table F] [--retune] [--seeds A..B]"),
    }
}
