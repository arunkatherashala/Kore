//! Track V1: the being's learner versus standard methods (and a language model) on the same worlds, seeds and
//! budgets. Prints the tables of docs/vs_world.md. Synthetic worlds, author-defined families and rewards.
//!
//!   cargo run -p kore-self --example measure_vs_world --offline --release -- <mode>
//!
//! Modes:
//!   tune-v2 | tune-l | tune-b2        hyper-parameter grids of every method, tuning seeds 101..120 only
//!   tune-v2-ext | tune-l-ext | tune-b2-ext   the second pass (beyond the first grid's edges)
//!   v2 | o | l | b2 | rules           reports, seeds 1..20 (World v2, World O, World L, the first world with
//!                                     the B2 pattern families, rule learning on revealed examples)
//!   llm out=DIR [seeds=1-5] [budget=80] [calls=60] [python=PATH] [script=PATH]
//!                                     the language-model player (Claude CLI); results checkpointed in DIR
//!   llm-report out=DIR                the LLM tables from DIR (and the same worlds for every other method)
//!   quick                             a small smoke run of everything (two seeds)
//!
//! Run data is printed or written to the given out dir only; nothing is committed.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;
#[path = "measure_vs_world_parts/llm_run.rs"]
mod llm_run;

use being::experiment::ExpCfg;
use being::lookahead::beingl::{run_wl, GreedyL, LookL, OracleL, OracleMyopicL};
use being::lookahead::plan::{LookCfg, Mode};
use being::lookahead::worldl::{Setting, WLCfg, WorldL, SETTINGS};
use being::measure::{perfect_rules, run_menu_ext, truth_in, OracleMenu, DISCOVER_EVERY};
use being::planner::{ModelChooser, SkipRule, DEFAULT_BETA, DEFAULT_SETTLE_K, DEFAULT_SKIP_FRAC};
use being::solver::{STRATEGIES, STRATEGIES_EXT};
use being::vs_world::adapt::{Player2, PlayerL, PlayerM};
use being::vs_world::core::{grid, grid_ext, Gran, Method};
use being::vs_world::mcts::{Mcts2, MctsL};
use being::world::World;
use being::world2::all_skills::{profile_use, PROFILE_EXP_PM, PROFILE_KAPPA};
use being::world2::baselines::Oracle2;
use being::world2::engine::LearnerCfg;
use being::world2::experiment::Exp2;
use being::world2::learner::Learner2;
use being::world2::measure::{run_w2, run_w2_with};
use being::world2::skills::SkillsCfg;
use being::world2::world::{W2Cfg, World2};
use being::world_o::engine::LearnerO;
use rayon::prelude::*;
use std::time::Instant;

pub const EVERY: u64 = 10;
pub const REPORT: std::ops::RangeInclusive<u64> = 1..=20;
pub const TUNING: std::ops::RangeInclusive<u64> = 101..=120;

// ---- statistics ----

/// Two-sided 95% t quantile for n - 1 degrees of freedom.
pub fn tq(n: usize) -> f64 {
    const T: [f64; 30] = [12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160, 2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056, 2.052, 2.048, 2.045, 2.042];
    if n < 2 { f64::NAN } else if n - 1 <= 30 { T[n - 2] } else { 1.96 }
}

pub fn mci(xs: &[f64]) -> (f64, f64, f64) {
    let n = xs.len();
    let m = xs.iter().sum::<f64>() / n.max(1) as f64;
    let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n as f64 - 1.0).max(1.0);
    let h = tq(n) * (var / n.max(1) as f64).sqrt();
    (m, m - h, m + h)
}

pub fn fci(xs: &[f64]) -> String {
    let (m, lo, hi) = mci(xs);
    format!("{m:.3} [{lo:.3}, {hi:.3}]")
}

pub fn fdiff(a: &[f64], b: &[f64]) -> String {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let (m, lo, hi) = mci(&d);
    let w = d.iter().filter(|x| **x > 1e-9).count();
    let l = d.iter().filter(|x| **x < -1e-9).count();
    format!("{m:+.3} [{lo:+.3}, {hi:+.3}] {w}/{l}/{}", d.len() - w - l)
}

fn mean(xs: &[f64]) -> f64 {
    xs.iter().sum::<f64>() / xs.len().max(1) as f64
}

// ---- tuned hyper-parameters (from the tune modes, seeds 101..120; see docs/vs_world.md) ----

pub fn chosen_v2() -> Vec<Method> {
    use Gran::*;
    vec![
        Method::EpsGreedy { c: 0.01, gran: Group, skip: 1.0 },
        Method::Ucb1Tuned { c: 0.3, gran: Group, skip: 1.0 },
        Method::KlUcb { c: 0.1, gran: Group, skip: 1.0 },
        Method::Thompson { prior: 0.5, gran: Group, skip: 1.0 },
        Method::Exp3 { gamma: 0.5, gran: Group },
        Method::LinUcb { alpha: 0.5, skip: 1.0 },
        Method::LinTs { v: 0.2, skip: 1.0 },
        Method::QLearn { eps: 0.1, gamma: 0.7 },
        Method::Sarsa { eps: 0.1, gamma: 1.0 },
        Method::Dqn { lr: 0.03, gamma: 1.0, eps: 1.0 },
    ]
}

pub fn chosen_l() -> Vec<Method> {
    use Gran::*;
    vec![
        Method::EpsGreedy { c: 0.01, gran: Cell, skip: 0.0 },
        Method::Ucb1Tuned { c: 1.0, gran: Cell, skip: 0.0 },
        Method::KlUcb { c: 1.0, gran: Cell, skip: 0.0 },
        Method::Thompson { prior: 4.0, gran: Cell, skip: 0.0 },
        Method::Exp3 { gamma: 0.3, gran: Cell },
        Method::LinUcb { alpha: 1.0, skip: 0.0 },
        Method::LinTs { v: 0.5, skip: 0.0 },
        Method::QLearn { eps: 0.1, gamma: 0.7 },
        Method::Sarsa { eps: 0.1, gamma: 0.7 },
        Method::Dqn { lr: 0.03, gamma: 1.0, eps: 1.0 },
    ]
}

pub fn chosen_b2() -> Vec<Method> {
    use Gran::*;
    vec![
        Method::EpsGreedy { c: 0.03, gran: Cell, skip: 1.0 },
        Method::Ucb1Tuned { c: 0.1, gran: Cell, skip: 1.0 },
        Method::KlUcb { c: 0.03, gran: Cell, skip: 1.0 },
        Method::Thompson { prior: 0.1, gran: Cell, skip: 1.0 },
        Method::Exp3 { gamma: 0.5, gran: Group },
        Method::LinUcb { alpha: 0.5, skip: 1.0 },
        Method::LinTs { v: 0.2, skip: 1.0 },
        Method::QLearn { eps: 0.1, gamma: 1.0 },
        Method::Sarsa { eps: 0.1, gamma: 1.0 },
        Method::Dqn { lr: 0.03, gamma: 1.0, eps: 1.0 },
    ]
}

// ---- World v2 ----

#[derive(Clone, Copy)]
pub struct Cond {
    pub name: &'static str,
    pub families: usize,
    pub budget: i64,
    pub noise_pm: u32,
    pub drift: u32,
    pub curve: u32,
}

impl Cond {
    pub fn cfg(&self, seed: u64) -> W2Cfg {
        W2Cfg { noise_pm: self.noise_pm, drift_per_10k: self.drift, curve_pct: self.curve, ..W2Cfg::new(seed, self.families, self.budget) }
    }
}

pub fn conds() -> Vec<Cond> {
    let mut v = vec![];
    for (size, families, budget) in [("small", 6usize, 1500i64), ("medium", 12, 3000), ("large", 24, 6000)] {
        for (kind, noise_pm, drift, curve) in [("clean", 0u32, 0u32, 0u32), ("noisy", 100, 0, 0), ("drift", 0, 20, 0), ("hard", 100, 20, 100)] {
            v.push(Cond { name: Box::leak(format!("{size} {kind}").into_boxed_str()), families, budget, noise_pm, drift, curve });
        }
    }
    v
}

/// The being's own variants and the references in World v2.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Ours2 {
    Current,
    Recommended,
    AllSkills,
    Mcts,
    OraclePlanner,
    OracleRules,
}

impl Ours2 {
    pub fn name(&self) -> &'static str {
        match self {
            Ours2::Current => "ours: learner (current)",
            Ours2::Recommended => "ours: B7 + skip fix (recommended)",
            Ours2::AllSkills => "ours: all skills (B7+B9+B3b+retire)",
            Ours2::Mcts => "MCTS on the TRUE simulator (sees hidden)",
            Ours2::OraclePlanner => "oracle-planner (sees hidden)",
            Ours2::OracleRules => "oracle-knows-rules (sees hidden)",
        }
    }
}

pub const OURS2: [Ours2; 6] = [Ours2::Current, Ours2::Recommended, Ours2::AllSkills, Ours2::Mcts, Ours2::OraclePlanner, Ours2::OracleRules];

pub fn run_ours2(p: Ours2, cfg: &W2Cfg) -> f64 {
    match p {
        Ours2::Current => run_w2(cfg, &mut Learner2::default(), EVERY, None).per_energy(),
        Ours2::Recommended => {
            let lc = LearnerCfg { use_: profile_use(), ..LearnerCfg::default() };
            let mut l = LearnerO { inner: lc.learner(), skip_rule: SkipRule::Settled(DEFAULT_SETTLE_K), l2l: being::world_o::b12::L2LCfg::default(), view: None };
            run_w2_with(cfg, &mut l, EVERY, None, lc).per_energy()
        }
        Ours2::AllSkills => {
            let lc = LearnerCfg { use_: profile_use(), skills: SkillsCfg { adapt: true, retire: true, ..SkillsCfg::default() }, ..LearnerCfg::default() };
            let mut x = Exp2::new(lc.learner(), ExpCfg::eig(PROFILE_KAPPA, PROFILE_EXP_PM, 5000), World2::new(cfg.clone()), cfg.budget);
            run_w2_with(cfg, &mut x, EVERY, None, lc).per_energy()
        }
        Ours2::Mcts => run_w2(cfg, &mut Mcts2::new(World2::new(cfg.clone()), cfg.seed), 0, None).per_energy(),
        Ours2::OraclePlanner => run_w2(cfg, &mut Oracle2::new(World2::new(cfg.clone()), false), 0, None).per_energy(),
        Ours2::OracleRules => run_w2(cfg, &mut Oracle2::new(World2::new(cfg.clone()), true), 0, None).per_energy(),
    }
}

pub fn run_m2(m: &Method, cfg: &W2Cfg) -> f64 {
    run_w2(cfg, &mut Player2::new(m.agent(cfg.seed)), 0, None).per_energy()
}

/// Rows of one condition: [ours..., methods...] per seed.
fn rows2(c: &Cond, seeds: &[u64], ours: &[Ours2], ms: &[Method]) -> Vec<Vec<f64>> {
    let jobs: Vec<(usize, usize)> = (0..seeds.len()).flat_map(|i| (0..ours.len() + ms.len()).map(move |j| (i, j))).collect();
    let vals: Vec<f64> = jobs
        .par_iter()
        .map(|(i, j)| {
            let cfg = c.cfg(seeds[*i]);
            if *j < ours.len() { run_ours2(ours[*j], &cfg) } else { run_m2(&ms[*j - ours.len()], &cfg) }
        })
        .collect();
    let w = ours.len() + ms.len();
    (0..seeds.len()).map(|i| vals[i * w..(i + 1) * w].to_vec()).collect()
}

fn col(rs: &[Vec<f64>], j: usize) -> Vec<f64> {
    rs.iter().map(|r| r[j]).collect()
}

fn tune_print<F: Fn(&Method) -> f64 + Sync>(label: &str, ext: bool, score: F) -> Vec<Method> {
    let label: String = if ext { format!("{label}, second pass (values beyond the first grid's edges, average-reward variants)") } else { label.to_string() };
    println!("\n### Tuning, {label} (seeds 101..120 only; objective = mean reward per energy over the tuning conditions)\n");
    println!("| method | configuration | objective |");
    println!("|---|---|---|");
    let mut chosen = vec![];
    for g in if ext { grid_ext() } else { grid() } {
        let scores: Vec<f64> = g.par_iter().map(|m| score(m)).collect();
        let mut bi = 0;
        for (i, s) in scores.iter().enumerate() {
            println!("| {} | {} | {s:.3} |", g[i].family(), g[i].label());
            if *s > scores[bi] {
                bi = i;
            }
        }
        println!("| **{} chosen** | **{}** | **{:.3}** |", g[bi].family(), g[bi].label(), scores[bi]);
        chosen.push(g[bi]);
    }
    println!("\nchosen: {:?}", chosen);
    chosen
}

fn tune_v2(ext: bool) {
    let cs: Vec<Cond> = conds().into_iter().filter(|c| c.name == "medium clean" || c.name == "medium hard").collect();
    let seeds: Vec<u64> = TUNING.collect();
    tune_print("World v2 medium clean + medium hard", ext, |m| {
        let v: Vec<f64> = cs.iter().flat_map(|c| seeds.iter().map(move |s| (c, *s))).collect::<Vec<_>>().into_iter().map(|(c, s)| run_m2(m, &c.cfg(s))).collect();
        mean(&v)
    });
}

fn report_v2(seeds: Vec<u64>, only: Option<&str>) {
    let ms = chosen_v2();
    println!("\n## World v2, report seeds {}..{} (n = {} worlds per condition)\n", seeds[0], seeds[seeds.len() - 1], seeds.len());
    println!("Methods: {}", ms.iter().map(|m| m.label()).collect::<Vec<_>>().join("; "));
    let mut summary = vec![];
    for c in conds() {
        if only.map_or(false, |o| o != c.name) {
            continue;
        }
        let t0 = Instant::now();
        let rs = rows2(&c, &seeds, &OURS2, &ms);
        eprintln!("World v2 {}: {:.0} s", c.name, t0.elapsed().as_secs_f64());
        println!("\n### {} (families {}, budget {}, noise {} per mille, drift {} per 10k, curve {})\n", c.name, c.families, c.budget, c.noise_pm, c.drift, c.curve);
        println!("| policy | true reward per energy, mean [95% interval] | current minus policy (better/worse/tie) | recommended minus policy |");
        println!("|---|---|---|---|");
        let cur = col(&rs, 0);
        let rec = col(&rs, 1);
        for (j, o) in OURS2.iter().enumerate() {
            println!("| {} | {} | {} | {} |", o.name(), fci(&col(&rs, j)), fdiff(&cur, &col(&rs, j)), fdiff(&rec, &col(&rs, j)));
        }
        let mut best = (0usize, f64::MIN);
        for (k, m) in ms.iter().enumerate() {
            let x = col(&rs, OURS2.len() + k);
            println!("| {} | {} | {} | {} |", m.label(), fci(&x), fdiff(&cur, &x), fdiff(&rec, &x));
            if mean(&x) > best.1 {
                best = (k, mean(&x));
            }
        }
        summary.push((c.name, rs, best.0));
    }
    println!("\n### World v2 summary (true reward per energy; best method = highest mean on these report seeds, chosen after the fact, which favours the methods)\n");
    println!("| condition | ours current | ours recommended | ours all skills | best standard method | its value | recommended minus best method | current minus best method | MCTS true simulator | oracle-planner | oracle-knows-rules |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    for (name, rs, b) in &summary {
        let bm = col(rs, OURS2.len() + b);
        println!(
            "| {name} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            fci(&col(rs, 0)),
            fci(&col(rs, 1)),
            fci(&col(rs, 2)),
            ms[*b].family(),
            fci(&bm),
            fdiff(&col(rs, 1), &bm),
            fdiff(&col(rs, 0), &bm),
            fci(&col(rs, 3)),
            fci(&col(rs, 4)),
            fci(&col(rs, 5))
        );
    }
    println!("\n### World v2: every method against ours (recommended), paired, per condition: mean difference and wins/losses/ties\n");
    print!("| method |");
    for (name, _, _) in &summary {
        print!(" {name} |");
    }
    println!();
    print!("|---|");
    for _ in &summary {
        print!("---|");
    }
    println!();
    for (k, m) in ms.iter().enumerate() {
        print!("| {} |", m.family());
        for (_, rs, _) in &summary {
            print!(" {} |", fdiff(&col(rs, 1), &col(rs, OURS2.len() + k)));
        }
        println!();
    }
}

// ---- World O ----

fn report_o(seeds: Vec<u64>, budget: i64, window: i64) {
    use being::world_o::measure::{run_wo, OracleO, RunWO, TruthO};
    use being::world_o::world::{WOCfg, WorldO, MAX_TIER};
    let ms = chosen_v2();
    let names: Vec<String> = ["ours: current", "ours: fix+b7 (recommended)", "oracle-planner (sees hidden)", "oracle-knows-rules (sees hidden)"].iter().map(|s| s.to_string()).chain(ms.iter().map(|m| m.family().to_string())).collect();
    let jobs: Vec<(usize, u64)> = (0..names.len()).flat_map(|j| seeds.iter().map(move |s| (j, *s))).collect();
    let t0 = Instant::now();
    let limits = || being::controller::Limits { max_live_rules: 200, max_candidates: 100_000, compose: false, max_blocks: 8, ..being::controller::Limits::default() };
    let res: Vec<RunWO> = jobs
        .par_iter()
        .map(|(j, s)| {
            let c = WOCfg::new(*s, budget);
            match j {
                0 => run_wo(&c, being::world_o::engine::LearnerOCfg::default(), None, EVERY, Some(limits()), u64::MAX, window, true),
                1 => {
                    let b7 = being::world2::rules_use::UseCfg { rule_est: true, rule_only: true, ..Default::default() };
                    let lc = being::world_o::engine::LearnerOCfg { skip_rule: SkipRule::Settled(DEFAULT_SETTLE_K), use_: b7, ..Default::default() };
                    run_wo(&c, lc, None, EVERY, Some(limits()), u64::MAX, window, true)
                }
                2 | 3 => {
                    let mut o = OracleO { world: WorldO::new(c.clone()), truth: TruthO::default(), knows_rules: *j == 3 };
                    run_wo(&c, Default::default(), Some(&mut o), 0, None, u64::MAX, window, false)
                }
                _ => {
                    let mut p = Player2::new(ms[*j - 4].agent(*s));
                    run_wo(&c, Default::default(), Some(&mut p), 0, Some(limits()), u64::MAX, window, true)
                }
            }
        })
        .collect();
    eprintln!("World O: {:.0} s", t0.elapsed().as_secs_f64());
    let get = |j: usize, s: u64| &res[jobs.iter().position(|x| *x == (j, s)).unwrap()];
    let nwin = res.iter().map(|r| r.wins.len()).min().unwrap_or(0);
    println!("\n## World O (open-ended), budget {budget} energy units, windows of {window}, seeds {}..{} (n = {})\n", seeds[0], seeds[seeds.len() - 1], seeds.len());
    println!("Standard methods use the World v2 tuning (not re-tuned in World O). Regret = oracle-knows-rules minus the policy, per window, paired by world.\n");
    let w = |i: usize| format!("{}k-{}k", i as i64 * window / 1000, (i as i64 + 1) * window / 1000);
    print!("| policy | whole run (after window 1) |");
    for i in 0..nwin {
        print!(" regret {} |", w(i));
    }
    println!();
    print!("|---|---|");
    for _ in 0..nwin {
        print!("---|");
    }
    println!();
    let span = |r: &RunWO, a: usize, b: usize| {
        let (x, y) = r.wins[a..b].iter().fold((0i64, 0i64), |acc, w| (acc.0 + w.true_reward, acc.1 + w.cost));
        x as f64 / y.max(1) as f64
    };
    for (j, n) in names.iter().enumerate() {
        let tot: Vec<f64> = seeds.iter().map(|s| span(get(j, *s), 1, nwin)).collect();
        print!("| {n} | {} |", fci(&tot));
        for i in 0..nwin {
            let v: Vec<f64> = seeds.iter().map(|s| get(3, *s).wins[i].rpe() - get(j, *s).wins[i].rpe()).collect();
            print!(" {} |", fci(&v));
        }
        println!();
    }
    println!("\n| policy minus ours (recommended), whole run after window 1 | difference (better/worse/tie) | minus ours (current) |");
    println!("|---|---|---|");
    let tot = |j: usize| -> Vec<f64> { seeds.iter().map(|s| span(get(j, *s), 1, nwin)).collect() };
    for (j, n) in names.iter().enumerate().skip(2) {
        println!("| {n} | {} | {} |", fdiff(&tot(j), &tot(1)), fdiff(&tot(j), &tot(0)));
    }
    println!("\n#### Order versus competence (top tier, Spearman of family index with attempts to competence, per world; negative = faster later) and share of learnable families reached\n");
    println!("| policy | worlds | Spearman mean [95% interval] | learnable families reached competence |");
    println!("|---|---|---|---|");
    for (j, n) in names.iter().enumerate() {
        if j == 2 || j == 3 {
            continue;
        }
        let mut rhos = vec![];
        let (mut learn, mut reach) = (0usize, 0usize);
        for s in &seeds {
            let r = get(j, *s);
            for f in r.fams.iter().filter(|f| f.learnable()) {
                learn += 1;
                reach += f.competent_after.is_some() as usize;
            }
            let top: Vec<_> = r.fams.iter().filter(|f| f.learnable() && f.tier >= MAX_TIER).collect();
            if top.len() >= 6 {
                let x: Vec<f64> = top.iter().map(|f| f.fid as f64).collect();
                let y: Vec<f64> = top.iter().map(|f| f.competent_after.map_or(1e9, |a| a.0 as f64)).collect();
                let rho = spearman(&x, &y);
                if rho.is_finite() {
                    rhos.push(rho);
                }
            }
        }
        println!("| {n} | {} | {} | {reach} of {learn} ({:.0}%) |", rhos.len(), if rhos.len() >= 2 { fci(&rhos) } else { "-".into() }, 100.0 * reach as f64 / learn.max(1) as f64);
    }
}

pub fn spearman(x: &[f64], y: &[f64]) -> f64 {
    let rank = |v: &[f64]| {
        let mut idx: Vec<usize> = (0..v.len()).collect();
        idx.sort_by(|a, b| v[*a].partial_cmp(&v[*b]).unwrap());
        let mut r = vec![0.0; v.len()];
        let mut i = 0;
        while i < idx.len() {
            let mut j = i;
            while j + 1 < idx.len() && v[idx[j + 1]] == v[idx[i]] {
                j += 1;
            }
            for k in i..=j {
                r[idx[k]] = (i + j) as f64 / 2.0;
            }
            i = j + 1;
        }
        r
    };
    let (a, b) = (rank(x), rank(y));
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let cov: f64 = a.iter().zip(&b).map(|(p, q)| (p - ma) * (q - mb)).sum();
    let va: f64 = a.iter().map(|p| (p - ma).powi(2)).sum();
    let vb: f64 = b.iter().map(|q| (q - mb).powi(2)).sum();
    cov / (va * vb).sqrt()
}

// ---- World L ----

const OURS_L: [&str; 6] = ["ours: B1 planner (current)", "ours: lookahead pure d3", "ours: lookahead hybrid d2 (all-skills setting)", "MCTS on the TRUE simulator (sees hidden, incl. phase)", "oracle one-step (sees hidden)", "oracle DP (sees hidden)"];

fn run_ours_l(j: usize, cfg: &WLCfg) -> f64 {
    let look = |d: u32, mode: Mode| {
        let mut c = LookCfg::new(d, 20_000);
        c.mode = mode;
        c
    };
    let r = match j {
        0 => run_wl(cfg, &mut GreedyL::default()),
        1 => run_wl(cfg, &mut LookL::new(Some(look(3, Mode::Pure)))),
        2 => run_wl(cfg, &mut LookL::new(Some(look(2, Mode::Hybrid)))),
        3 => run_wl(cfg, &mut MctsL::new(WorldL::new(cfg.clone()), cfg.seed)),
        4 => run_wl(cfg, &mut OracleMyopicL::new(WorldL::new(cfg.clone()))),
        _ => run_wl(cfg, &mut OracleL::new(WorldL::new(cfg.clone()))),
    };
    r.per_energy()
}

fn run_m_l(m: &Method, cfg: &WLCfg) -> f64 {
    run_wl(cfg, &mut PlayerL::new(m.agent(cfg.seed))).per_energy()
}

fn tune_l(ext: bool) {
    let seeds: Vec<u64> = TUNING.collect();
    tune_print("World L, four settings, budget 400", ext, |m| {
        let v: Vec<f64> = SETTINGS.iter().flat_map(|st| seeds.iter().map(move |s| (*st, *s))).collect::<Vec<_>>().into_iter().map(|(st, s)| run_m_l(m, &WLCfg { seed: s, setting: st, budget: 400 })).collect();
        mean(&v)
    });
}

fn report_l(seeds: Vec<u64>) {
    let ms = chosen_l();
    println!("\n## World L, report seeds {}..{} (n = {})\n", seeds[0], seeds[seeds.len() - 1], seeds.len());
    println!("Methods: {}", ms.iter().map(|m| m.label()).collect::<Vec<_>>().join("; "));
    let mut summary = vec![];
    for budget in [400i64, 1600] {
        for st in SETTINGS {
            let w = OURS_L.len() + ms.len();
            let jobs: Vec<(usize, usize)> = (0..seeds.len()).flat_map(|i| (0..w).map(move |j| (i, j))).collect();
            let vals: Vec<f64> = jobs
                .par_iter()
                .map(|(i, j)| {
                    let cfg = WLCfg { seed: seeds[*i], setting: st, budget };
                    if *j < OURS_L.len() { run_ours_l(*j, &cfg) } else { run_m_l(&ms[*j - OURS_L.len()], &cfg) }
                })
                .collect();
            let rs: Vec<Vec<f64>> = (0..seeds.len()).map(|i| vals[i * w..(i + 1) * w].to_vec()).collect();
            println!("\n### World L {} budget {budget}\n", st.name());
            println!("| policy | reward per energy | B1 (current) minus policy | best of ours minus policy |");
            println!("|---|---|---|---|");
            let b1 = col(&rs, 0);
            let our_best = (0..3).max_by(|a, b| mean(&col(&rs, *a)).partial_cmp(&mean(&col(&rs, *b))).unwrap()).unwrap();
            let ob = col(&rs, our_best);
            for (j, n) in OURS_L.iter().enumerate() {
                println!("| {n} | {} | {} | {} |", fci(&col(&rs, j)), fdiff(&b1, &col(&rs, j)), fdiff(&ob, &col(&rs, j)));
            }
            let mut best = (0usize, f64::MIN);
            for (k, m) in ms.iter().enumerate() {
                let x = col(&rs, OURS_L.len() + k);
                println!("| {} | {} | {} | {} |", m.label(), fci(&x), fdiff(&b1, &x), fdiff(&ob, &x));
                if mean(&x) > best.1 {
                    best = (k, mean(&x));
                }
            }
            summary.push((format!("{} {budget}", st.name()), rs, best.0, our_best));
        }
    }
    println!("\n### World L summary (best method chosen after the fact on these seeds)\n");
    println!("| setting, budget | ours B1 (current) | best of ours (which) | best standard method | its value | B1 minus best method | best of ours minus best method | MCTS true simulator | oracle DP |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for (name, rs, b, ob) in &summary {
        let bm = col(rs, OURS_L.len() + b);
        println!("| {name} | {} | {} ({}) | {} | {} | {} | {} | {} | {} |", fci(&col(rs, 0)), fci(&col(rs, *ob)), OURS_L[*ob].trim_start_matches("ours: "), ms[*b].family(), fci(&bm), fdiff(&col(rs, 0), &bm), fdiff(&col(rs, *ob), &bm), fci(&col(rs, 3)), fci(&col(rs, 5)));
    }
}

// ---- the first world with the B2 pattern families ----

const OURS_B2: [&str; 5] = ["ours: learner + discovery (current, B2)", "ours: learner, no discovery", "ours: learner + discovery + skip fix", "oracle, fixed solvers (sees hidden)", "oracle with perfect rules (sees hidden)"];

fn run_ours_b2(j: usize, seed: u64, budget: i64) -> f64 {
    let w = || World::with_extended(seed, true);
    let perfect = perfect_rules();
    match j {
        0 => run_menu_ext(w(), budget, &mut ModelChooser::new(DEFAULT_BETA), DISCOVER_EVERY, &[]).run.per_budget(),
        1 => run_menu_ext(w(), budget, &mut ModelChooser { no_discovered: true, ..ModelChooser::new(DEFAULT_BETA) }, 0, &[]).run.per_budget(),
        2 => run_menu_ext(w(), budget, &mut ModelChooser { skip_rule: SkipRule::Settled(DEFAULT_SETTLE_K), ..ModelChooser::new(DEFAULT_BETA) }, DISCOVER_EVERY, &[]).run.per_budget(),
        3 => run_menu_ext(w(), budget, &mut OracleMenu { truth: truth_in(&w(), &[]), strategies: STRATEGIES.to_vec() }, 0, &[]).run.per_budget(),
        _ => run_menu_ext(w(), budget, &mut OracleMenu { truth: truth_in(&w(), &perfect), strategies: STRATEGIES_EXT.to_vec() }, 0, &perfect).run.per_budget(),
    }
}

fn run_m_b2(m: &Method, seed: u64, budget: i64) -> f64 {
    run_menu_ext(World::with_extended(seed, true), budget, &mut PlayerM::new(m.agent(seed)), 0, &[]).run.per_budget()
}

fn tune_b2(ext: bool) {
    let seeds: Vec<u64> = TUNING.collect();
    tune_print("first world with the B2 pattern families, budgets 600 and 3000", ext, |m| {
        let v: Vec<f64> = [600i64, 3000].iter().flat_map(|b| seeds.iter().map(move |s| (*b, *s))).collect::<Vec<_>>().into_iter().map(|(b, s)| run_m_b2(m, s, b)).collect();
        mean(&v)
    });
}

fn report_b2(seeds: Vec<u64>) {
    let ms = chosen_b2();
    println!("\n## First world with the B2 pattern families (extended pattern world, pattern reward x2), report seeds {}..{} (n = {})\n", seeds[0], seeds[seeds.len() - 1], seeds.len());
    println!("Methods: {}", ms.iter().map(|m| m.label()).collect::<Vec<_>>().join("; "));
    let _ = DEFAULT_SKIP_FRAC;
    for budget in [600i64, 3000] {
        let w = OURS_B2.len() + ms.len();
        let jobs: Vec<(usize, usize)> = (0..seeds.len()).flat_map(|i| (0..w).map(move |j| (i, j))).collect();
        let vals: Vec<f64> = jobs.par_iter().map(|(i, j)| if *j < OURS_B2.len() { run_ours_b2(*j, seeds[*i], budget) } else { run_m_b2(&ms[*j - OURS_B2.len()], seeds[*i], budget) }).collect();
        let rs: Vec<Vec<f64>> = (0..seeds.len()).map(|i| vals[i * w..(i + 1) * w].to_vec()).collect();
        println!("\n### budget {budget}\n");
        println!("| policy | reward per energy | current minus policy | skip fix minus policy |");
        println!("|---|---|---|---|");
        for (j, n) in OURS_B2.iter().enumerate() {
            println!("| {n} | {} | {} | {} |", fci(&col(&rs, j)), fdiff(&col(&rs, 0), &col(&rs, j)), fdiff(&col(&rs, 2), &col(&rs, j)));
        }
        let mut best = (0usize, f64::MIN);
        for (k, m) in ms.iter().enumerate() {
            let x = col(&rs, OURS_B2.len() + k);
            println!("| {} | {} | {} | {} |", m.label(), fci(&x), fdiff(&col(&rs, 0), &x), fdiff(&col(&rs, 2), &x));
            if mean(&x) > best.1 {
                best = (k, mean(&x));
            }
        }
        let bm = col(&rs, OURS_B2.len() + best.0);
        println!("\nsummary budget {budget}: current {} | skip fix {} | best method {} {} | current minus best {} | oracle with rules {}", fci(&col(&rs, 0)), fci(&col(&rs, 2)), ms[best.0].family(), fci(&bm), fdiff(&col(&rs, 0), &bm), fci(&col(&rs, 4)));
    }
}

// ---- rule learning on revealed examples ----

fn report_rules(seeds: Vec<u64>) {
    use being::discover::{search, Example, Rule};
    use being::vs_world::rules::*;
    use being::world2::rules_use::{formula_answer, search_const, search_robust, CExample};
    use being::world2::solve::sig2;
    use being::world2::world::{FType, FormulaKind, TYPE_CYCLE};
    const FRESH: u64 = 50;
    let learners = ["ours: B2 search (exact fit + guard)", "ours: B7 robust guard (theta 0.4)", "brute-force enumeration (same DSL and guard)", "1-NN on prefix features", "3-NN on prefix features", "regression tree on prefix features"];
    println!("\n## Rule learning on revealed examples (World v2, 12 families, seeds {}..{}; per sequence signature (family, difficulty 1..5), k revealed examples, scored on {FRESH} fresh tasks)\n", seeds[0], seeds[seeds.len() - 1]);
    println!("Noise: revealed answers replaced by the checker's answer +-1..3 with probability 0.1 (the World v2 noise model). Right = exact next term; wrong = answered and not exact; abstain = no answer.\n");
    println!("| noise | k | learner | right share of fresh tasks | wrong share | abstain share | in-grammar signatures: right share | outside-grammar signatures: wrong share |");
    println!("|---|---|---|---|---|---|---|---|");
    for noise in [0.0f64, 0.1] {
        for k in [4u64, 8, 16, 32] {
            // per seed: per learner (right, wrong, n, right_in, n_in, wrong_out, n_out)
            let per: Vec<Vec<[u64; 7]>> = seeds
                .par_iter()
                .map(|seed| {
                    let w = World2::new(W2Cfg::new(*seed, 12, 3000));
                    let mut rng = being::vs_world::core::Rng::new(*seed ^ 0x7E57 ^ (k << 8) ^ ((noise * 1000.0) as u64));
                    let mut ex: Vec<Example> = vec![];
                    let mut fresh: Vec<(u8, Vec<i64>, i64, bool)> = vec![];
                    for f in (0..12).filter(|f| TYPE_CYCLE[f % 6] == FType::Seq) {
                        let in_g = w.regime(f, 0).class.in_grammar();
                        for d in 1..=5u8 {
                            for i in 0..k {
                                let t = w.task(i, 0, f, Some(d), 0, 0xA11);
                                let mut next = t.expected();
                                if rng.f64() < noise {
                                    next += (1 + rng.below(3) as i64) * if rng.f64() < 0.5 { 1 } else { -1 };
                                }
                                ex.push(Example { event_id: format!("{f}-{d}-{i}"), sig: sig2(f, d), terms: t.view.params.clone(), next });
                            }
                            for i in 0..FRESH {
                                let t = w.task(i, 0, f, Some(d), 0, 0xF2E5);
                                fresh.push((sig2(f, d), t.view.params.clone(), t.expected(), in_g));
                            }
                        }
                    }
                    let b2: Vec<Rule> = search(&ex).into_iter().map(|f| Rule::new(f.sig, f.form, f.train_ok, f.val_ok, 0)).collect();
                    let rob: Vec<Rule> = search_robust(&ex, 400).into_iter().map(|f| Rule::new(f.found.sig, f.found.form, 0, 0, 0)).collect();
                    let bf = guarded_search(&ex, &bf_predict);
                    let mut out = vec![[0u64; 7]; learners.len()];
                    let mut by_sig: std::collections::BTreeMap<u8, Vec<Example>> = Default::default();
                    for e in &ex {
                        by_sig.entry(e.sig).or_default().push(e.clone());
                    }
                    let trees: std::collections::BTreeMap<u8, Option<Tree>> = by_sig.iter().map(|(s, v)| (*s, tree_seq(v))).collect();
                    for (sig, terms, want, in_g) in &fresh {
                        let train = &by_sig[sig];
                        let answers = [
                            being::discover::apply(&b2, *sig, terms).map(|x| x.0),
                            being::discover::apply(&rob, *sig, terms).map(|x| x.0),
                            book_answer(&bf, *sig, terms, &bf_predict),
                            knn_seq(train, terms, 1),
                            knn_seq(train, terms, 3),
                            trees[sig].as_ref().and_then(|t| tree_seq_answer(t, terms)),
                        ];
                        for (li, a) in answers.iter().enumerate() {
                            let o = &mut out[li];
                            o[2] += 1;
                            match a {
                                Some(v) if v == want => {
                                    o[0] += 1;
                                    if *in_g {
                                        o[3] += 1;
                                    }
                                }
                                Some(_) => {
                                    o[1] += 1;
                                    if !*in_g {
                                        o[5] += 1;
                                    }
                                }
                                None => {}
                            }
                            if *in_g { o[4] += 1 } else { o[6] += 1 }
                        }
                    }
                    out
                })
                .collect();
            for (li, l) in learners.iter().enumerate() {
                let share = |a: usize, b: usize| -> Vec<f64> { per.iter().map(|p| p[li][a] as f64 / p[li][b].max(1) as f64).collect() };
                let abst: Vec<f64> = per.iter().map(|p| (p[li][2] - p[li][0] - p[li][1]) as f64 / p[li][2].max(1) as f64).collect();
                println!("| {noise} | {k} | {l} | {} | {} | {} | {} | {} |", fci(&share(0, 2)), fci(&share(1, 2)), fci(&abst), fci(&share(3, 4)), fci(&share(5, 6)));
            }
        }
    }
    // formula families: the hidden constant g
    println!("\n### Formula families (hidden constant g; per formula family, k revealed examples, {FRESH} fresh tasks)\n");
    println!("| noise | k | learner | right share | wrong share | abstain share |");
    println!("|---|---|---|---|---|---|");
    let fl = ["ours: fitted constant (B7 C, exact guard)", "ours: fitted constant (B7 C, robust theta 0.4)", "1-NN on the input", "regression tree on (kind, input)"];
    for noise in [0.0f64, 0.1] {
        for k in [2u64, 4, 8, 16] {
            let per: Vec<Vec<[u64; 3]>> = seeds
                .par_iter()
                .map(|seed| {
                    let w = World2::new(W2Cfg::new(*seed, 12, 3000));
                    let mut rng = being::vs_world::core::Rng::new(*seed ^ 0xF0 ^ (k << 8) ^ ((noise * 1000.0) as u64));
                    let mut out = vec![[0u64; 3]; fl.len()];
                    for f in (0..12).filter(|f| TYPE_CYCLE[f % 6] == FType::Formula) {
                        let mut cex = vec![];
                        let mut fex: Vec<FEx> = vec![];
                        for i in 0..k {
                            let t = w.task(i, 0, f, None, 0, 0xA12);
                            let fall = t.view.formula == Some(FormulaKind::Fall);
                            let mut a = t.expected();
                            if rng.f64() < noise {
                                a += (1 + rng.below(3) as i64) * if rng.f64() < 0.5 { 1 } else { -1 };
                            }
                            cex.push(CExample { event_id: format!("{f}-{i}"), fam: f, kind: if fall { "fall".into() } else { "weight".into() }, x: vec![t.view.params[0]], revealed: a });
                            fex.push((fall, t.view.params[0], a));
                        }
                        let g_exact = search_const(&cex, 3, 1, 0).last().map(|c| c.g);
                        let g_rob = search_const(&cex, 3, 1, 400).last().map(|c| c.g);
                        let tree = tree_formula(&fex);
                        for i in 0..FRESH {
                            let t = w.task(i, 0, f, None, 0, 0xF2E6);
                            let fall = t.view.formula == Some(FormulaKind::Fall);
                            let x = t.view.params[0];
                            let ans = [g_exact.map(|g| formula_answer(fall, g, x)), g_rob.map(|g| formula_answer(fall, g, x)), knn_formula(&fex, fall, x), tree.as_ref().map(|tr| tr.predict(&[fall as u8 as f64, x as f64]).round() as i64)];
                            for (li, a) in ans.iter().enumerate() {
                                out[li][2] += 1;
                                match a {
                                    Some(v) if *v == t.expected() => out[li][0] += 1,
                                    Some(_) => out[li][1] += 1,
                                    None => {}
                                }
                            }
                        }
                    }
                    out
                })
                .collect();
            for (li, l) in fl.iter().enumerate() {
                let share = |a: usize| -> Vec<f64> { per.iter().map(|p| p[li][a] as f64 / p[li][2].max(1) as f64).collect() };
                let abst: Vec<f64> = per.iter().map(|p| (p[li][2] - p[li][0] - p[li][1]) as f64 / p[li][2].max(1) as f64).collect();
                println!("| {noise} | {k} | {l} | {} | {} | {} |", fci(&share(0)), fci(&share(1)), fci(&abst));
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("quick");
    let t0 = Instant::now();
    let report: Vec<u64> = REPORT.collect();
    match mode {
        "tune-v2" => tune_v2(false),
        "tune-l" => tune_l(false),
        "tune-b2" => tune_b2(false),
        "tune-v2-ext" => tune_v2(true),
        "tune-l-ext" => tune_l(true),
        "tune-b2-ext" => tune_b2(true),
        "v2" => report_v2(report, args.get(2).map(|s| s.as_str())),
        "o" => {
            let budget = args.get(2).and_then(|x| x.parse().ok()).unwrap_or(600_000);
            report_o(report, budget, 100_000)
        }
        "l" => report_l(report),
        "b2" => report_b2(report),
        "rules" => report_rules(report),
        "llm" => llm_run::main_llm(&args),
        "llm-report" => llm_run::report(&args),
        "quick" => {
            report_v2(vec![1, 2], Some("small clean"));
            report_l(vec![1, 2]);
            report_b2(vec![1, 2]);
            report_rules(vec![1, 2]);
            report_o(vec![1, 2], 100_000, 50_000);
        }
        _ => eprintln!("unknown mode {mode}"),
    }
    eprintln!("total {:.0} s", t0.elapsed().as_secs_f64());
    let _ = Gran::Cell;
}
