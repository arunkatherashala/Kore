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
use being::linval::{LIN_L, LIN_MENU, LIN_V2};
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
    /// Track B13: the current learner with the context-aware chooser estimate.
    B13Current,
    /// Track B13: the recommended profile (B7 + settled skip) with the context-aware chooser estimate.
    B13Recommended,
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
            Ours2::B13Current => "ours: current + B13 chooser",
            Ours2::B13Recommended => "ours: B13 profile (B7 + skip fix + B13 chooser)",
        }
    }
}

pub const OURS2: [Ours2; 8] = [Ours2::Current, Ours2::Recommended, Ours2::AllSkills, Ours2::Mcts, Ours2::OraclePlanner, Ours2::OracleRules, Ours2::B13Current, Ours2::B13Recommended];
/// Columns of the B13 rows in `OURS2`.
pub const B13C: usize = 6;
pub const B13R: usize = 7;

pub fn run_ours2(p: Ours2, cfg: &W2Cfg) -> f64 {
    match p {
        Ours2::Current => run_w2(cfg, &mut Learner2::default(), EVERY, None).per_energy(),
        Ours2::Recommended => {
            let lc = LearnerCfg { use_: profile_use(), ..LearnerCfg::default() };
            let mut l = LearnerO { inner: lc.learner(), skip_rule: SkipRule::Settled(DEFAULT_SETTLE_K), l2l: being::world_o::b12::L2LCfg::default(), view: None, l2: None, l2view: None };
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
        Ours2::B13Current => {
            let lc = LearnerCfg { lin: Some(LIN_V2), ..LearnerCfg::default() };
            run_w2_with(cfg, &mut lc.learner(), EVERY, None, lc).per_energy()
        }
        Ours2::B13Recommended => {
            let lc = LearnerCfg { use_: profile_use(), lin: Some(LIN_V2), ..LearnerCfg::default() };
            let mut l = LearnerO { inner: lc.learner(), skip_rule: SkipRule::Settled(DEFAULT_SETTLE_K), l2l: being::world_o::b12::L2LCfg::default(), view: None, l2: None, l2view: None };
            run_w2_with(cfg, &mut l, EVERY, None, lc).per_energy()
        }
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
        println!("| policy | true reward per energy, mean [95% interval] | current minus policy (better/worse/tie) | recommended minus policy | B13 profile minus policy | current + B13 minus policy |");
        println!("|---|---|---|---|---|---|");
        let cur = col(&rs, 0);
        let rec = col(&rs, 1);
        let b13r = col(&rs, B13R);
        let b13c = col(&rs, B13C);
        for (j, o) in OURS2.iter().enumerate() {
            println!("| {} | {} | {} | {} | {} | {} |", o.name(), fci(&col(&rs, j)), fdiff(&cur, &col(&rs, j)), fdiff(&rec, &col(&rs, j)), fdiff(&b13r, &col(&rs, j)), fdiff(&b13c, &col(&rs, j)));
        }
        let mut best = (0usize, f64::MIN);
        for (k, m) in ms.iter().enumerate() {
            let x = col(&rs, OURS2.len() + k);
            println!("| {} | {} | {} | {} | {} | {} |", m.label(), fci(&x), fdiff(&cur, &x), fdiff(&rec, &x), fdiff(&b13r, &x), fdiff(&b13c, &x));
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
    println!("\n### World v2, Track B13 summary (best method = highest mean on these report seeds, chosen after the fact)\n");
    println!("| condition | ours current | current + B13 | ours recommended | B13 profile | LinUCB | best standard method | its value | B13 profile minus LinUCB | B13 profile minus best method | current + B13 minus LinUCB | B13 profile minus recommended | current + B13 minus current |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    let lin_k = ms.iter().position(|m| m.family() == "LinUCB").unwrap();
    for (name, rs, b) in &summary {
        let bm = col(rs, OURS2.len() + b);
        let lu = col(rs, OURS2.len() + lin_k);
        println!(
            "| {name} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            fci(&col(rs, 0)),
            fci(&col(rs, B13C)),
            fci(&col(rs, 1)),
            fci(&col(rs, B13R)),
            fci(&lu),
            ms[*b].family(),
            fci(&bm),
            fdiff(&col(rs, B13R), &lu),
            fdiff(&col(rs, B13R), &bm),
            fdiff(&col(rs, B13C), &lu),
            fdiff(&col(rs, B13R), &col(rs, 1)),
            fdiff(&col(rs, B13C), &col(rs, 0))
        );
    }
    println!("\n### World v2: every method against the B13 profile, paired, per condition\n");
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
            print!(" {} |", fdiff(&col(rs, B13R), &col(rs, OURS2.len() + k)));
        }
        println!();
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
    const NO: usize = 6;
    let names: Vec<String> = ["ours: current", "ours: fix+b7 (recommended)", "oracle-planner (sees hidden)", "oracle-knows-rules (sees hidden)", "ours: current + B13 chooser", "ours: fix+b7 + B13 chooser"].iter().map(|s| s.to_string()).chain(ms.iter().map(|m| m.family().to_string())).collect();
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
                4 => run_wo(&c, being::world_o::engine::LearnerOCfg { lin: Some(LIN_V2), ..Default::default() }, None, EVERY, Some(limits()), u64::MAX, window, true),
                5 => {
                    let b7 = being::world2::rules_use::UseCfg { rule_est: true, rule_only: true, ..Default::default() };
                    let lc = being::world_o::engine::LearnerOCfg { skip_rule: SkipRule::Settled(DEFAULT_SETTLE_K), use_: b7, lin: Some(LIN_V2), ..Default::default() };
                    run_wo(&c, lc, None, EVERY, Some(limits()), u64::MAX, window, true)
                }
                _ => {
                    let mut p = Player2::new(ms[*j - NO].agent(*s));
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
    println!("\n| policy minus ours (recommended), whole run after window 1 | difference (better/worse/tie) | minus ours (current) | minus ours (current + B13) | minus ours (fix+b7 + B13) |");
    println!("|---|---|---|---|---|");
    let tot = |j: usize| -> Vec<f64> { seeds.iter().map(|s| span(get(j, *s), 1, nwin)).collect() };
    for (j, n) in names.iter().enumerate().skip(2) {
        println!("| {n} | {} | {} | {} | {} |", fdiff(&tot(j), &tot(1)), fdiff(&tot(j), &tot(0)), fdiff(&tot(j), &tot(4)), fdiff(&tot(j), &tot(5)));
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

const OURS_L: [&str; 8] = ["ours: B1 planner (current)", "ours: lookahead pure d3", "ours: lookahead hybrid d2 (all-skills setting)", "MCTS on the TRUE simulator (sees hidden, incl. phase)", "oracle one-step (sees hidden)", "oracle DP (sees hidden)", "ours: B1 + B13 chooser (B1 skip)", "ours: B1 + B13 chooser, no skip (B13 profile)"];

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
        5 => run_wl(cfg, &mut OracleL::new(WorldL::new(cfg.clone()))),
        6 => run_wl(cfg, &mut GreedyL { lin: Some(LIN_L), ..GreedyL::default() }),
        _ => run_wl(cfg, &mut GreedyL { lin: Some(LIN_L), skip_frac: 0.0, ..GreedyL::default() }),
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
            println!("| policy | reward per energy | B1 (current) minus policy | best of ours minus policy | B13 profile minus policy |");
            println!("|---|---|---|---|---|");
            let b1 = col(&rs, 0);
            let our_best = (0..3).max_by(|a, b| mean(&col(&rs, *a)).partial_cmp(&mean(&col(&rs, *b))).unwrap()).unwrap();
            let ob = col(&rs, our_best);
            let b13 = col(&rs, 7);
            for (j, n) in OURS_L.iter().enumerate() {
                println!("| {n} | {} | {} | {} | {} |", fci(&col(&rs, j)), fdiff(&b1, &col(&rs, j)), fdiff(&ob, &col(&rs, j)), fdiff(&b13, &col(&rs, j)));
            }
            let mut best = (0usize, f64::MIN);
            for (k, m) in ms.iter().enumerate() {
                let x = col(&rs, OURS_L.len() + k);
                println!("| {} | {} | {} | {} | {} |", m.label(), fci(&x), fdiff(&b1, &x), fdiff(&ob, &x), fdiff(&b13, &x));
                if mean(&x) > best.1 {
                    best = (k, mean(&x));
                }
            }
            summary.push((format!("{} {budget}", st.name()), rs, best.0, our_best));
        }
    }
    println!("\n### World L, Track B13 summary (best method chosen after the fact on these seeds)\n");
    println!("| setting, budget | ours B1 (current) | B1 + B13 (B1 skip) | B13 profile (no skip) | LinUCB | best standard method | its value | B13 profile minus LinUCB | B13 profile minus best method | B1 + B13 (B1 skip) minus best method | B13 profile minus B1 | B13 profile minus lookahead pure d3 |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
    let lin_k = ms.iter().position(|m| m.family() == "LinUCB").unwrap();
    for (name, rs, b, _) in &summary {
        let bm = col(rs, OURS_L.len() + b);
        let lu = col(rs, OURS_L.len() + lin_k);
        println!("| {name} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |", fci(&col(rs, 0)), fci(&col(rs, 6)), fci(&col(rs, 7)), fci(&lu), ms[*b].family(), fci(&bm), fdiff(&col(rs, 7), &lu), fdiff(&col(rs, 7), &bm), fdiff(&col(rs, 6), &bm), fdiff(&col(rs, 7), &col(rs, 0)), fdiff(&col(rs, 7), &col(rs, 1)));
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

const OURS_B2: [&str; 7] = ["ours: learner + discovery (current, B2)", "ours: learner, no discovery", "ours: learner + discovery + skip fix", "oracle, fixed solvers (sees hidden)", "oracle with perfect rules (sees hidden)", "ours: learner + discovery + B13 chooser", "ours: learner + discovery + skip fix + B13 chooser (B13 profile)"];

fn run_ours_b2(j: usize, seed: u64, budget: i64) -> f64 {
    let w = || World::with_extended(seed, true);
    let perfect = perfect_rules();
    match j {
        0 => run_menu_ext(w(), budget, &mut ModelChooser::new(DEFAULT_BETA), DISCOVER_EVERY, &[]).run.per_budget(),
        1 => run_menu_ext(w(), budget, &mut ModelChooser { no_discovered: true, ..ModelChooser::new(DEFAULT_BETA) }, 0, &[]).run.per_budget(),
        2 => run_menu_ext(w(), budget, &mut ModelChooser { skip_rule: SkipRule::Settled(DEFAULT_SETTLE_K), ..ModelChooser::new(DEFAULT_BETA) }, DISCOVER_EVERY, &[]).run.per_budget(),
        3 => run_menu_ext(w(), budget, &mut OracleMenu { truth: truth_in(&w(), &[]), strategies: STRATEGIES.to_vec() }, 0, &[]).run.per_budget(),
        4 => run_menu_ext(w(), budget, &mut OracleMenu { truth: truth_in(&w(), &perfect), strategies: STRATEGIES_EXT.to_vec() }, 0, &perfect).run.per_budget(),
        5 => run_menu_ext(w(), budget, &mut ModelChooser { lin: Some(LIN_MENU), ..ModelChooser::new(DEFAULT_BETA) }, DISCOVER_EVERY, &[]).run.per_budget(),
        _ => run_menu_ext(w(), budget, &mut ModelChooser { skip_rule: SkipRule::Settled(DEFAULT_SETTLE_K), lin: Some(LIN_MENU), ..ModelChooser::new(DEFAULT_BETA) }, DISCOVER_EVERY, &[]).run.per_budget(),
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
        println!("| policy | reward per energy | current minus policy | skip fix minus policy | B13 profile minus policy | current + B13 minus policy |");
        println!("|---|---|---|---|---|---|");
        for (j, n) in OURS_B2.iter().enumerate() {
            println!("| {n} | {} | {} | {} | {} | {} |", fci(&col(&rs, j)), fdiff(&col(&rs, 0), &col(&rs, j)), fdiff(&col(&rs, 2), &col(&rs, j)), fdiff(&col(&rs, 6), &col(&rs, j)), fdiff(&col(&rs, 5), &col(&rs, j)));
        }
        let mut best = (0usize, f64::MIN);
        for (k, m) in ms.iter().enumerate() {
            let x = col(&rs, OURS_B2.len() + k);
            println!("| {} | {} | {} | {} | {} | {} |", m.label(), fci(&x), fdiff(&col(&rs, 0), &x), fdiff(&col(&rs, 2), &x), fdiff(&col(&rs, 6), &x), fdiff(&col(&rs, 5), &x));
            if mean(&x) > best.1 {
                best = (k, mean(&x));
            }
        }
        let bm = col(&rs, OURS_B2.len() + best.0);
        println!("\nsummary budget {budget}: current {} | skip fix {} | best method {} {} | current minus best {} | oracle with rules {}", fci(&col(&rs, 0)), fci(&col(&rs, 2)), ms[best.0].family(), fci(&bm), fdiff(&col(&rs, 0), &bm), fci(&col(&rs, 4)));
        let lin_k = ms.iter().position(|m| m.family() == "LinUCB").unwrap();
        let lu = col(&rs, OURS_B2.len() + lin_k);
        println!("B13 summary budget {budget}: current + B13 {} | B13 profile {} | LinUCB {} | B13 profile minus LinUCB {} | B13 profile minus best method ({}) {} | current + B13 minus LinUCB {} | B13 profile minus current {} | B13 profile minus skip fix {}", fci(&col(&rs, 5)), fci(&col(&rs, 6)), fci(&lu), fdiff(&col(&rs, 6), &lu), ms[best.0].family(), fdiff(&col(&rs, 6), &bm), fdiff(&col(&rs, 5), &lu), fdiff(&col(&rs, 6), &col(&rs, 0)), fdiff(&col(&rs, 6), &col(&rs, 2)));
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

// ---- Track B13 review additions: the rules-or-LinUCB fallback and regret curves ----

/// Policies of the B13 extra report in World v2 (column order).
const X2: [&str; 6] = ["oracle-knows-rules (sees hidden)", "ours: B13 profile (B7 + skip fix + B13 chooser)", "ours: current + B13 chooser", "ours: recommended (B7 + skip fix)", "LinUCB (V1 tuning)", "rules-or-LinUCB fallback (B7 rule estimate where a rule covers, LinUCB elsewhere)"];

fn run_x2(j: usize, cfg: &W2Cfg) -> being::world2::measure::W2Run {
    match j {
        0 => run_w2(cfg, &mut Oracle2::new(World2::new(cfg.clone()), true), 0, None),
        1 => {
            let lc = LearnerCfg { use_: profile_use(), lin: Some(LIN_V2), ..LearnerCfg::default() };
            let mut l = LearnerO { inner: lc.learner(), skip_rule: SkipRule::Settled(DEFAULT_SETTLE_K), l2l: being::world_o::b12::L2LCfg::default(), view: None, l2: None, l2view: None };
            run_w2_with(cfg, &mut l, EVERY, None, lc)
        }
        2 => {
            let lc = LearnerCfg { lin: Some(LIN_V2), ..LearnerCfg::default() };
            run_w2_with(cfg, &mut lc.learner(), EVERY, None, lc)
        }
        3 => {
            let lc = LearnerCfg { use_: profile_use(), ..LearnerCfg::default() };
            let mut l = LearnerO { inner: lc.learner(), skip_rule: SkipRule::Settled(DEFAULT_SETTLE_K), l2l: being::world_o::b12::L2LCfg::default(), view: None, l2: None, l2view: None };
            run_w2_with(cfg, &mut l, EVERY, None, lc)
        }
        4 => run_w2(cfg, &mut Player2::new(Method::LinUcb { alpha: 0.5, skip: 1.0 }.agent(cfg.seed)), 0, None),
        _ => {
            // the same rule machinery as the B13 profile (B7 B, C, G: robust acceptance, fitted constants), discovery every 10 steps
            let lc = LearnerCfg { use_: profile_use(), ..LearnerCfg::default() };
            run_w2_with(cfg, &mut being::vs_world::fallback::RulesOrLin::new(0.5, 1.0), EVERY, None, lc)
        }
    }
}

/// Window sizes of the regret curves: the life's energy in 10, 5 and 2 windows.
const NWINS: [i64; 3] = [10, 5, 2];

/// Ordering of regret means of (B13 profile, LinUCB, fallback) in one window, and the paired B13-minus-other tests.
fn order_line(reg: &[Vec<f64>; 3]) -> (bool, i32, i32) {
    let m: Vec<f64> = reg.iter().map(|r| mean(r)).collect();
    let lowest = m[0] <= m[1] && m[0] <= m[2];
    // B13 regret minus the other's: below 0 = B13 better; sign of the interval
    let sig = |o: usize| -> i32 {
        let d: Vec<f64> = reg[0].iter().zip(&reg[o]).map(|(x, y)| x - y).collect();
        let (_, lo, hi) = mci(&d);
        if hi < 0.0 { 1 } else if lo > 0.0 { -1 } else { 0 }
    };
    (lowest, sig(1), sig(2))
}

fn report_b13_v2(seeds: Vec<u64>) {
    println!("\n## Track B13 review additions, World v2, report seeds {}..{} (n = {})\n", seeds[0], seeds[seeds.len() - 1], seeds.len());
    println!("Policies: {}\n", X2.join("; "));
    println!("Regret = oracle-knows-rules minus the policy, true reward per energy within each window of the life's energy, paired by world, mean [95% interval]. Window sizes: a tenth, a fifth and a half of the life's energy budget.\n");
    let mut rows_sum = vec![];
    let mut order_sum: Vec<(String, i64, usize, usize, usize, usize, usize, usize)> = vec![];
    for c in conds() {
        let t0 = Instant::now();
        let jobs: Vec<(usize, usize)> = (0..seeds.len()).flat_map(|i| (0..X2.len()).map(move |j| (i, j))).collect();
        let runs: Vec<(f64, Vec<Vec<f64>>, u64)> = jobs
            .par_iter()
            .map(|(i, j)| {
                let cfg = c.cfg(seeds[*i]);
                let r = run_x2(*j, &cfg);
                let wins: Vec<Vec<f64>> = NWINS.iter().map(|n| { let w = cfg.budget / n; (0..*n).map(|k| r.window_e(k * w, (k + 1) * w)).collect() }).collect();
                let rule_answers = r.recs.iter().filter(|x| x.rule_used).count() as u64;
                (r.per_energy(), wins, rule_answers)
            })
            .collect();
        eprintln!("B13 extra, World v2 {}: {:.0} s", c.name, t0.elapsed().as_secs_f64());
        let get = |j: usize| -> Vec<&(f64, Vec<Vec<f64>>, u64)> { (0..seeds.len()).map(|i| &runs[i * X2.len() + j]).collect() };
        let rpe = |j: usize| -> Vec<f64> { get(j).iter().map(|r| r.0).collect() };
        let regret = |j: usize, z: usize, k: usize| -> Vec<f64> { (0..seeds.len()).map(|i| get(0)[i].1[z][k] - get(j)[i].1[z][k]).collect() };
        println!("\n### {}\n", c.name);
        println!("| policy | true reward per energy | rule answers per life |\n|---|---|---|");
        for j in 0..X2.len() {
            let ra: Vec<f64> = get(j).iter().map(|r| r.2 as f64).collect();
            println!("| {} | {} | {:.0} |", X2[j], fci(&rpe(j)), mean(&ra));
        }
        println!("\n| paired | difference |\n|---|---|");
        println!("| B13 profile minus fallback | {} |", fdiff(&rpe(1), &rpe(5)));
        println!("| current + B13 minus fallback | {} |", fdiff(&rpe(2), &rpe(5)));
        println!("| fallback minus LinUCB | {} |", fdiff(&rpe(5), &rpe(4)));
        println!("| fallback minus recommended | {} |", fdiff(&rpe(5), &rpe(3)));
        for (z, n) in NWINS.iter().enumerate() {
            println!("\nRegret per window, {n} windows of {} energy:\n", c.budget / n);
            print!("| policy |");
            for k in 0..*n {
                print!(" w{} |", k + 1);
            }
            println!();
            print!("|---|");
            for _ in 0..*n {
                print!("---|");
            }
            println!();
            for j in 1..X2.len() {
                print!("| {} |", X2[j]);
                for k in 0..*n as usize {
                    print!(" {} |", fci(&regret(j, z, k)));
                }
                println!();
            }
            print!("| B13 profile regret minus fallback regret |");
            for k in 0..*n as usize {
                print!(" {} |", fdiff(&regret(1, z, k), &regret(5, z, k)));
            }
            println!();
            let (mut low, mut bl, mut wl, mut bf, mut wf) = (0, 0, 0, 0, 0);
            for k in 0..*n as usize {
                let (lowest, sl, sf) = order_line(&[regret(1, z, k), regret(4, z, k), regret(5, z, k)]);
                low += lowest as usize;
                bl += (sl > 0) as usize;
                wl += (sl < 0) as usize;
                bf += (sf > 0) as usize;
                wf += (sf < 0) as usize;
            }
            order_sum.push((c.name.to_string(), *n, *n as usize, low, bl, wl, bf, wf));
        }
        rows_sum.push((c.name, (0..X2.len()).map(rpe).collect::<Vec<_>>()));
    }
    println!("\n### Summary: rules-or-LinUCB fallback against the B13 chooser (World v2)\n");
    println!("| condition | fallback | B13 profile | current + B13 | LinUCB | B13 profile minus fallback | current + B13 minus fallback | fallback minus LinUCB | fallback minus recommended |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for (n, r) in &rows_sum {
        println!("| {n} | {} | {} | {} | {} | {} | {} | {} | {} |", fci(&r[5]), fci(&r[1]), fci(&r[2]), fci(&r[4]), fdiff(&r[1], &r[5]), fdiff(&r[2], &r[5]), fdiff(&r[5], &r[4]), fdiff(&r[5], &r[3]));
    }
    println!("\n### Window-size sensitivity: does the ordering B13 profile < fallback, LinUCB (regret) hold in every window?\n");
    println!("| condition | windows | B13 profile has the lowest mean regret of the three | B13 below LinUCB, interval excludes 0 | B13 above LinUCB, interval excludes 0 | B13 below fallback, interval excludes 0 | B13 above fallback, interval excludes 0 |");
    println!("|---|---|---|---|---|---|---|");
    let mut tot = [0usize; 6];
    for (name, n, w, low, bl, wl, bf, wf) in &order_sum {
        println!("| {name} | {n} | {low} of {w} | {bl} | {wl} | {bf} | {wf} |");
        for (t, x) in tot.iter_mut().zip([*w, *low, *bl, *wl, *bf, *wf]) {
            *t += x;
        }
    }
    println!("| all | - | {} of {} | {} | {} | {} | {} |", tot[1], tot[0], tot[2], tot[3], tot[4], tot[5]);
    for n in NWINS {
        let sel: Vec<_> = order_sum.iter().filter(|x| x.1 == n).collect();
        let f = |g: fn(&&(String, i64, usize, usize, usize, usize, usize, usize)) -> usize| sel.iter().map(g).sum::<usize>();
        println!("| all, {n} windows | {n} | {} of {} | {} | {} | {} | {} |", f(|x| x.3), f(|x| x.2), f(|x| x.4), f(|x| x.5), f(|x| x.6), f(|x| x.7));
    }
}

fn report_b13_o(seeds: Vec<u64>, budget: i64, window: i64) {
    use being::world_o::measure::{run_wo, OracleO, RunWO, TruthO};
    use being::world_o::world::{WOCfg, WorldO};
    let names = ["oracle-knows-rules (sees hidden)", "ours: current + B13 chooser", "ours: current", "LinUCB (V1 World v2 tuning)", "rules-or-LinUCB fallback"];
    let limits = || being::controller::Limits { max_live_rules: 200, max_candidates: 100_000, compose: false, max_blocks: 8, ..being::controller::Limits::default() };
    let jobs: Vec<(usize, u64)> = (0..names.len()).flat_map(|j| seeds.iter().map(move |s| (j, *s))).collect();
    let t0 = Instant::now();
    let res: Vec<RunWO> = jobs
        .par_iter()
        .map(|(j, s)| {
            let c = WOCfg::new(*s, budget);
            match j {
                0 => {
                    let mut o = OracleO { world: WorldO::new(c.clone()), truth: TruthO::default(), knows_rules: true };
                    run_wo(&c, Default::default(), Some(&mut o), 0, None, u64::MAX, window, false)
                }
                1 => run_wo(&c, being::world_o::engine::LearnerOCfg { lin: Some(LIN_V2), ..Default::default() }, None, EVERY, Some(limits()), u64::MAX, window, true),
                2 => run_wo(&c, being::world_o::engine::LearnerOCfg::default(), None, EVERY, Some(limits()), u64::MAX, window, true),
                3 => {
                    let mut p = Player2::new(Method::LinUcb { alpha: 0.5, skip: 1.0 }.agent(*s));
                    run_wo(&c, Default::default(), Some(&mut p), 0, Some(limits()), u64::MAX, window, true)
                }
                _ => {
                    // the being's rule search runs (every 10 steps) and its rules answer through `discovered`
                    let mut f = being::vs_world::fallback::RulesOrLin::new(0.5, 1.0);
                    run_wo(&c, Default::default(), Some(&mut f), EVERY, Some(limits()), u64::MAX, window, true)
                }
            }
        })
        .collect();
    eprintln!("B13 extra, World O: {:.0} s", t0.elapsed().as_secs_f64());
    let get = |j: usize, s: u64| &res[jobs.iter().position(|x| *x == (j, s)).unwrap()];
    let nwin = res.iter().map(|r| r.wins.len()).min().unwrap_or(0);
    let span = |r: &RunWO, a: usize, b: usize| {
        let (x, y) = r.wins[a..b].iter().fold((0i64, 0i64), |acc, w| (acc.0 + w.true_reward, acc.1 + w.cost));
        x as f64 / y.max(1) as f64
    };
    // whole run after the first 100k energy (the V1 measure), from the 50k base windows
    let skip0 = (100_000 / window) as usize;
    let tot = |j: usize| -> Vec<f64> { seeds.iter().map(|s| span(get(j, *s), skip0, nwin)).collect() };
    println!("\n## Track B13 review additions, World O, budget {budget}, seeds {}..{} (n = {})\n", seeds[0], seeds[seeds.len() - 1], seeds.len());
    println!("| policy | whole run (after the first 100k energy) |\n|---|---|");
    for (j, n) in names.iter().enumerate() {
        println!("| {n} | {} |", fci(&tot(j)));
    }
    println!("\n| paired, whole run after the first 100k | difference |\n|---|---|");
    println!("| current + B13 minus fallback | {} |", fdiff(&tot(1), &tot(4)));
    println!("| current minus fallback | {} |", fdiff(&tot(2), &tot(4)));
    println!("| fallback minus LinUCB | {} |", fdiff(&tot(4), &tot(3)));
    println!("| current + B13 minus current | {} |", fdiff(&tot(1), &tot(2)));
    println!("| current + B13 minus LinUCB | {} |", fdiff(&tot(1), &tot(3)));
    let mut sum = vec![];
    for agg in [1usize, 2, 4] {
        let size = window * agg as i64;
        let nw = nwin / agg;
        println!("\nRegret per window (oracle-knows-rules minus the policy), windows of {size} energy:\n");
        print!("| policy |");
        for i in 0..nw {
            print!(" {}k-{}k |", i as i64 * size / 1000, (i as i64 + 1) * size / 1000);
        }
        println!();
        print!("|---|");
        for _ in 0..nw {
            print!("---|");
        }
        println!();
        let reg = |j: usize, i: usize| -> Vec<f64> { seeds.iter().map(|s| span(get(0, *s), i * agg, (i + 1) * agg) - span(get(j, *s), i * agg, (i + 1) * agg)).collect() };
        for (j, n) in names.iter().enumerate().skip(1) {
            print!("| {n} |");
            for i in 0..nw {
                print!(" {} |", fci(&reg(j, i)));
            }
            println!();
        }
        print!("| current + B13 regret minus fallback regret |");
        for i in 0..nw {
            print!(" {} |", fdiff(&reg(1, i), &reg(4, i)));
        }
        println!();
        let (mut low, mut bl, mut wl, mut bf, mut wf) = (0, 0, 0, 0, 0);
        for i in 0..nw {
            let (lowest, sl, sf) = order_line(&[reg(1, i), reg(3, i), reg(4, i)]);
            low += lowest as usize;
            bl += (sl > 0) as usize;
            wl += (sl < 0) as usize;
            bf += (sf > 0) as usize;
            wf += (sf < 0) as usize;
        }
        sum.push((size, nw, low, bl, wl, bf, wf));
    }
    println!("\n### Window-size sensitivity (World O): does current + B13 have lower regret than LinUCB and the fallback in every window?\n");
    println!("| window size | windows | current + B13 lowest mean regret of the three | below LinUCB, interval excludes 0 | above LinUCB, interval excludes 0 | below fallback, interval excludes 0 | above fallback, interval excludes 0 |\n|---|---|---|---|---|---|---|");
    for (size, nw, low, bl, wl, bf, wf) in sum {
        println!("| {size} | {nw} | {low} of {nw} | {bl} | {wl} | {bf} | {wf} |");
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
        "b13-v2" => report_b13_v2(report),
        "b13-o" => {
            let budget = args.get(2).and_then(|x| x.parse().ok()).unwrap_or(600_000);
            report_b13_o(report, budget, 50_000)
        }
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
