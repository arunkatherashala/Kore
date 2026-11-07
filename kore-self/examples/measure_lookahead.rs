//! Track B8: planning ahead. Measures the lookahead planner against the B1 one-step planner, UCB1 and
//! the oracles in World L (settings where one choice changes later options, plus a control) and in the
//! existing worlds (first menu world, World v2) as controls. Prints the tables of docs/measured.md,
//! section "Track B8: planning ahead". Synthetic worlds, author-defined rewards.
//!   cargo run -p kore-self --example measure_lookahead --offline --release -- [tune|report|cost|quick]
//! Tuning seeds 101..=120 (used only to choose the planner's constants); report seeds 1..=20.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::chooser::{UcbMenu, DEFAULT_EXPLORE_C};
use being::lookahead::adapt::{Look2, LookMenu};
use being::lookahead::beingl::{run_wl, GreedyL, LookL, OracleL, OracleMyopicL, RunL, Ucb1L};
use being::lookahead::plan::{LookCfg, Mode};
use being::lookahead::worldl::{Setting, WLCfg, WorldL, SETTINGS};
use being::measure::{mean_ci, run_menu, truth, OracleMenu};
use being::planner::ModelChooser;
use being::solver::STRATEGIES;
use being::world2::baselines::{Oracle2, Ucb2};
use being::world2::learner::Learner2;
use being::world2::measure::run_w2;
use being::world2::world::{W2Cfg, World2};
use rayon::prelude::*;
use std::time::Instant;

const NODES: u64 = 20_000;

#[derive(Clone, Copy, Debug)]
struct Tun {
    beta_root: f64,
    samples: usize,
    width: usize,
}

const CHOSEN: Tun = Tun { beta_root: being::lookahead::plan::DEFAULT_BETA_ROOT, samples: being::lookahead::plan::DEFAULT_SAMPLES, width: being::lookahead::plan::DEFAULT_WIDTH };

fn lcfg(depth: u32, t: Tun) -> LookCfg {
    LookCfg { samples: t.samples, width: t.width, beta_root: t.beta_root, ..LookCfg::new(depth, NODES) }
}

/// "pure-dN" or "hybrid-dN"
fn lcfg_of(p: &str, t: Tun) -> LookCfg {
    let d: u32 = p[p.len() - 1..].parse().unwrap();
    let mut c = lcfg(d, t);
    c.mode = if p.starts_with("pure") { Mode::Pure } else { Mode::Hybrid };
    c
}

#[derive(Clone, Copy, Debug, Default)]
struct Res {
    rate: f64,
    nodes: f64,
    max_nodes: u64,
    micros: f64,
    steps: u64,
    trunc: u64,
    /// share of energy on key/vault/probe/job families (World L)
    special: f64,
}

fn res_l(r: &RunL, us: f64) -> Res {
    let tot = r.cost.max(1) as f64;
    Res { rate: r.per_energy(), nodes: r.nodes as f64 / r.steps.max(1) as f64, max_nodes: r.max_nodes, micros: us / r.steps.max(1) as f64, steps: r.steps, trunc: r.truncations, special: (r.kind_energy[1] + r.kind_energy[2] + r.kind_energy[3] + r.kind_energy[4]) as f64 / tot }
}

// ---- World L ----

const WL_POL: [&str; 9] = ["greedy-b1", "ucb1", "pure-d1", "pure-d2", "pure-d3", "hybrid-d2", "hybrid-d3", "oracle-one-step", "oracle-dp"];

fn run_wl_pol(p: &str, cfg: &WLCfg, t: Tun) -> Res {
    let t0 = Instant::now();
    let r = match p {
        "greedy-b1" => run_wl(cfg, &mut GreedyL::default()),
        "ucb1" => run_wl(cfg, &mut Ucb1L { c: 0.05 }),
        "oracle-one-step" => run_wl(cfg, &mut OracleMyopicL::new(WorldL::new(cfg.clone()))),
        "oracle-dp" => run_wl(cfg, &mut OracleL::new(WorldL::new(cfg.clone()))),
        _ => run_wl(cfg, &mut LookL::new(Some(lcfg_of(p, t)))),
    };
    res_l(&r, t0.elapsed().as_secs_f64() * 1e6)
}

fn table_wl(setting: Setting, budget: i64, seeds: std::ops::RangeInclusive<u64>, pols: &[&str], t: Tun) -> Vec<Vec<Res>> {
    let seeds: Vec<u64> = seeds.collect();
    seeds.par_iter().map(|s| pols.iter().map(|p| run_wl_pol(p, &WLCfg { seed: *s, setting, budget }, t)).collect()).collect()
}

fn col(rows: &[Vec<Res>], i: usize) -> Vec<f64> {
    rows.iter().map(|r| r[i].rate).collect()
}

fn line(name: &str, xs: &[f64]) {
    let (m, lo, hi) = mean_ci(xs);
    println!("| {name} | {m:.3} | [{lo:.3}, {hi:.3}] |");
}

fn diff(name: &str, a: &[f64], b: &[f64]) {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let (m, lo, hi) = mean_ci(&d);
    let w = d.iter().filter(|x| **x > 1e-9).count();
    let l = d.iter().filter(|x| **x < -1e-9).count();
    println!("| {name} | {m:+.3} | [{lo:+.3}, {hi:+.3}] | {w} / {l} / {} |", d.len() - w - l);
}

fn report_wl(setting: Setting, budget: i64, t: Tun) {
    let rs = table_wl(setting, budget, 1..=20, &WL_POL, t);
    println!("\n#### World L, setting {}, budget {budget} (report seeds 1..20, n = 20 worlds)\n", setting.name());
    println!("| policy | reward per energy | 95% interval | nodes per step (mean / max) | energy share on key, vault, probe, job |");
    println!("|---|---|---|---|---|");
    for (i, p) in WL_POL.iter().enumerate() {
        let (m, lo, hi) = mean_ci(&col(&rs, i));
        let nodes = rs.iter().map(|r| r[i].nodes).sum::<f64>() / rs.len() as f64;
        let mx = rs.iter().map(|r| r[i].max_nodes).max().unwrap_or(0);
        let sp = rs.iter().map(|r| r[i].special).sum::<f64>() / rs.len() as f64;
        println!("| {p} | {m:.3} | [{lo:.3}, {hi:.3}] | {nodes:.0} / {mx} | {sp:.3} |");
    }
    println!("\n| paired difference | mean | 95% interval | better / worse / tie (of 20) |");
    println!("|---|---|---|---|");
    let g = col(&rs, 0);
    for d in 2..=6usize {
        diff(&format!("{} - greedy-b1", WL_POL[d]), &col(&rs, d), &g);
    }
    for d in [3usize, 4, 5, 6] {
        diff(&format!("{} - ucb1", WL_POL[d]), &col(&rs, d), &col(&rs, 1));
    }
    for d in [3usize, 4, 5, 6] {
        diff(&format!("oracle-dp - {}", WL_POL[d]), &col(&rs, 8), &col(&rs, d));
    }
    diff("oracle-dp - greedy-b1", &col(&rs, 8), &g);
    diff("oracle-dp - oracle-one-step (value of planning with known parameters)", &col(&rs, 8), &col(&rs, 7));
    let tr: Vec<u64> = (2..=6).map(|i| rs.iter().map(|r| r[i].trunc).sum()).collect();
    println!("\nnode cap {NODES}: decisions whose search was cut by the cap (pure d1, d2, d3, hybrid d2, d3): {tr:?}");
}

// ---- controls: first menu world ----

const M1_POL: [&str; 7] = ["greedy-b1", "ucb1-domain", "pure-d1", "pure-d2", "hybrid-d2", "hybrid-d3", "oracle"];

fn run_m1(p: &str, seed: u64, budget: i64, t: Tun) -> Res {
    let t0 = Instant::now();
    let mut nodes = 0.0;
    let mut mx = 0;
    let r = match p {
        "greedy-b1" => run_menu(seed, budget, &mut ModelChooser::new(1.5)),
        "ucb1-domain" => run_menu(seed, budget, &mut UcbMenu::new(seed, DEFAULT_EXPLORE_C)),
        "oracle" => run_menu(seed, budget, &mut OracleMenu { truth: truth(seed), strategies: STRATEGIES.to_vec() }),
        _ => {
            let mut c = LookMenu::new(Some(lcfg_of(p, t)), ModelChooser::new(1.5));
            let r = run_menu(seed, budget, &mut c);
            nodes = c.stats.nodes as f64 / c.stats.decisions.max(1) as f64;
            mx = c.stats.max_nodes;
            r
        }
    };
    Res { rate: r.per_budget(), nodes, max_nodes: mx, micros: t0.elapsed().as_secs_f64() * 1e6 / r.recs.len().max(1) as f64, steps: r.recs.len() as u64, trunc: 0, special: 0.0 }
}

fn report_m1(budget: i64, seeds: std::ops::RangeInclusive<u64>, t: Tun, label: &str) -> Vec<Vec<Res>> {
    let seeds: Vec<u64> = seeds.collect();
    let rs: Vec<Vec<Res>> = seeds.par_iter().map(|s| M1_POL.iter().map(|p| run_m1(p, *s, budget, t)).collect()).collect();
    println!("\n#### First menu world (B1), budget {budget} ({label}, n = {} worlds)\n", rs.len());
    println!("| policy | reward per energy | 95% interval | nodes per step (mean / max) |");
    println!("|---|---|---|---|");
    for (i, p) in M1_POL.iter().enumerate() {
        let (m, lo, hi) = mean_ci(&col(&rs, i));
        let nodes = rs.iter().map(|r| r[i].nodes).sum::<f64>() / rs.len() as f64;
        let mx = rs.iter().map(|r| r[i].max_nodes).max().unwrap_or(0);
        println!("| {p} | {m:.3} | [{lo:.3}, {hi:.3}] | {nodes:.0} / {mx} |");
    }
    println!("\n| paired difference | mean | 95% interval | better / worse / tie |");
    println!("|---|---|---|---|");
    let g = col(&rs, 0);
    for d in 2..=5usize {
        diff(&format!("{} - greedy-b1", M1_POL[d]), &col(&rs, d), &g);
    }
    diff("hybrid-d2 - ucb1-domain", &col(&rs, 4), &col(&rs, 1));
    diff("hybrid-d3 - ucb1-domain", &col(&rs, 5), &col(&rs, 1));
    diff("oracle - hybrid-d2", &col(&rs, 6), &col(&rs, 4));
    diff("oracle - hybrid-d3", &col(&rs, 6), &col(&rs, 5));
    rs
}

// ---- controls: World v2 ----

const W2_POL: [&str; 7] = ["learner-b1", "ucb1", "pure-d1", "pure-d2", "hybrid-d2", "hybrid-d3", "oracle-planner"];

fn run_v2p(p: &str, cfg: &W2Cfg, t: Tun) -> Res {
    let t0 = Instant::now();
    let (mut nodes, mut mx, mut tr) = (0.0, 0, 0);
    let r = match p {
        "learner-b1" => run_w2(cfg, &mut Learner2::default(), 10, None),
        "ucb1" => run_w2(cfg, &mut Ucb2 { c: 0.05 }, 0, None),
        "oracle-planner" => run_w2(cfg, &mut Oracle2::new(World2::new(cfg.clone()), false), 0, None),
        _ => {
            let mut c = Look2::new(Some(lcfg_of(p, t)));
            let r = run_w2(cfg, &mut c, 10, None);
            nodes = c.stats.nodes as f64 / c.stats.decisions.max(1) as f64;
            mx = c.stats.max_nodes;
            tr = c.stats.truncations;
            r
        }
    };
    Res { rate: r.per_energy(), nodes, max_nodes: mx, micros: t0.elapsed().as_secs_f64() * 1e6 / r.steps.max(1) as f64, steps: r.steps, trunc: tr, special: 0.0 }
}

#[derive(Clone, Copy)]
struct Cond {
    name: &'static str,
    families: usize,
    budget: i64,
    noise_pm: u32,
    drift: u32,
    curve: u32,
}

impl Cond {
    fn cfg(&self, seed: u64) -> W2Cfg {
        W2Cfg { noise_pm: self.noise_pm, drift_per_10k: self.drift, curve_pct: self.curve, ..W2Cfg::new(seed, self.families, self.budget) }
    }
}

const W2_CONDS: [Cond; 4] = [
    Cond { name: "small, clean", families: 6, budget: 1500, noise_pm: 0, drift: 0, curve: 0 },
    Cond { name: "medium, clean", families: 12, budget: 3000, noise_pm: 0, drift: 0, curve: 0 },
    Cond { name: "medium, noisy feedback", families: 12, budget: 3000, noise_pm: 100, drift: 0, curve: 0 },
    Cond { name: "medium, hard (noise, drift, rising difficulty)", families: 12, budget: 3000, noise_pm: 100, drift: 20, curve: 100 },
];

fn report_v2(c: &Cond, seeds: std::ops::RangeInclusive<u64>, t: Tun, label: &str) -> Vec<Vec<Res>> {
    let seeds: Vec<u64> = seeds.collect();
    let rs: Vec<Vec<Res>> = seeds.par_iter().map(|s| W2_POL.iter().map(|p| run_v2p(p, &c.cfg(*s), t)).collect()).collect();
    println!("\n#### World v2, {} ({} families, budget {}, noise {}, drift {}, curve {}%; {label}, n = {} worlds; true reward per energy)\n", c.name, c.families, c.budget, c.noise_pm, c.drift, c.curve, rs.len());
    println!("| policy | true reward per energy | 95% interval | nodes per step (mean / max) | depth-cut decisions |");
    println!("|---|---|---|---|---|");
    for (i, p) in W2_POL.iter().enumerate() {
        let (m, lo, hi) = mean_ci(&col(&rs, i));
        let nodes = rs.iter().map(|r| r[i].nodes).sum::<f64>() / rs.len() as f64;
        let mx = rs.iter().map(|r| r[i].max_nodes).max().unwrap_or(0);
        let tr: u64 = rs.iter().map(|r| r[i].trunc).sum();
        println!("| {p} | {m:.3} | [{lo:.3}, {hi:.3}] | {nodes:.0} / {mx} | {tr} |");
    }
    println!("\n| paired difference | mean | 95% interval | better / worse / tie |");
    println!("|---|---|---|---|");
    let g = col(&rs, 0);
    for d in 2..=5usize {
        diff(&format!("{} - learner-b1", W2_POL[d]), &col(&rs, d), &g);
    }
    diff("hybrid-d2 - ucb1", &col(&rs, 4), &col(&rs, 1));
    diff("hybrid-d3 - ucb1", &col(&rs, 5), &col(&rs, 1));
    diff("oracle-planner - hybrid-d2", &col(&rs, 6), &col(&rs, 4));
    diff("oracle-planner - hybrid-d3", &col(&rs, 6), &col(&rs, 5));
    rs
}

// ---- tuning ----

fn tune() {
    println!("### Tuning (seeds 101..120): World L, all four settings, budget 400; mean over seeds of (planner - greedy-b1); controls: menu world 600 and World v2 (12 families, 3000, noise 50, drift 10)\n");
    println!("| mode | simulated learning | depth | beta_root | samples | width | lambda | plain | keys | probe | sizes | World L mean | menu world | World v2 |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    let greedy: Vec<Vec<f64>> = SETTINGS.iter().map(|s| col(&table_wl(*s, 400, 101..=120, &["greedy-b1"], CHOSEN), 0)).collect();
    let tc = Cond { name: "tuning", families: 12, budget: 3000, noise_pm: 50, drift: 10, curve: 0 };
    let seeds: Vec<u64> = (101..=120).collect();
    let g1: Vec<f64> = seeds.par_iter().map(|s| run_m1("greedy-b1", *s, 600, CHOSEN).rate).collect();
    let g2: Vec<f64> = seeds.par_iter().map(|s| run_v2p("learner-b1", &tc.cfg(*s), CHOSEN).rate).collect();
    let only = std::env::args().nth(2).unwrap_or_default();
    let mut grid: Vec<(&str, u32, Tun, f64, bool)> = vec![];
    for learn in [true, false] {
        for depth in [2u32, 3] {
            grid.push(("pure", depth, Tun { beta_root: 1.5, samples: 2, width: 3 }, 1.0, learn));
            for lambda in [0.5, 1.0] {
                grid.push(("hybrid", depth, Tun { beta_root: 1.5, samples: 2, width: 3 }, lambda, learn));
            }
        }
    }
    for (mode, depth, t, lambda, learn) in grid {
        if !only.is_empty() && only != mode {
            continue;
        }
        let name = format!("{mode}-d{depth}");
        let mut c = lcfg_of(&name, t);
        c.lambda = lambda;
        c.learn = learn;
        let mut ds = vec![];
        for (si, s) in SETTINGS.iter().enumerate() {
            let l: Vec<f64> = seeds.par_iter().map(|sd| run_wl(&WLCfg { seed: *sd, setting: *s, budget: 400 }, &mut LookL::new(Some(c))).per_energy()).collect();
            ds.push(l.iter().zip(&greedy[si]).map(|(a, b)| a - b).sum::<f64>() / 20.0);
        }
        let m = ds.iter().sum::<f64>() / ds.len() as f64;
        let l1: Vec<f64> = seeds.par_iter().map(|s| run_menu(*s, 600, &mut LookMenu::new(Some(c), ModelChooser::new(1.5))).per_budget()).collect();
        let l2: Vec<f64> = seeds.par_iter().map(|s| run_w2(&tc.cfg(*s), &mut Look2::new(Some(c)), 10, None).per_energy()).collect();
        let d1 = l1.iter().zip(&g1).map(|(a, b)| a - b).sum::<f64>() / 20.0;
        let d2 = l2.iter().zip(&g2).map(|(a, b)| a - b).sum::<f64>() / 20.0;
        println!("| {mode} | {learn} | {depth} | {} | {} | {} | {lambda} | {:+.3} | {:+.3} | {:+.3} | {:+.3} | {m:+.3} | {d1:+.3} | {d2:+.3} |", t.beta_root, t.samples, t.width, ds[0], ds[1], ds[2], ds[3]);
    }
}

fn cost() {
    println!("
### Where the energy goes (report seeds 1..20, budget 400): share of energy on key, vault, probe, job families
");
    println!("| setting | policy | key | vault | probe | job |");
    println!("|---|---|---|---|---|---|");
    for s in [Setting::Keys, Setting::Probe] {
        for p in WL_POL {
            let ks: Vec<[f64; 4]> = (1..=20u64).collect::<Vec<_>>().par_iter().map(|seed| {
                let cfg = WLCfg { seed: *seed, setting: s, budget: 400 };
                let r = match p {
                    "greedy-b1" => run_wl(&cfg, &mut GreedyL::default()),
                    "ucb1" => run_wl(&cfg, &mut Ucb1L { c: 0.05 }),
                    "oracle-one-step" => run_wl(&cfg, &mut OracleMyopicL::new(WorldL::new(cfg.clone()))),
                    "oracle-dp" => run_wl(&cfg, &mut OracleL::new(WorldL::new(cfg.clone()))),
                    _ => run_wl(&cfg, &mut LookL::new(Some(lcfg_of(p, CHOSEN)))),
                };
                let t = r.cost.max(1) as f64;
                [r.kind_energy[1] as f64 / t, r.kind_energy[2] as f64 / t, r.kind_energy[3] as f64 / t, r.kind_energy[4] as f64 / t]
            }).collect();
            let m = |i: usize| ks.iter().map(|k| k[i]).sum::<f64>() / ks.len() as f64;
            println!("| {} | {p} | {:.3} | {:.3} | {:.3} | {:.3} |", s.name(), m(0), m(1), m(2), m(3));
        }
    }
    println!("\n### Cost per step (sequential, one thread, seeds 1..5; wall clock on the measuring machine)\n");
    println!("| world | policy | steps per life | nodes per step (mean / max) | microseconds per step |");
    println!("|---|---|---|---|---|");
    for s in [Setting::Keys, Setting::Probe] {
        for p in ["greedy-b1", "pure-d1", "pure-d2", "pure-d3", "hybrid-d2", "hybrid-d3"] {
            let rs: Vec<Res> = (1..=5).map(|seed| run_wl_pol(p, &WLCfg { seed, setting: s, budget: 400 }, CHOSEN)).collect();
            let n = rs.len() as f64;
            println!("| World L {} 400 | {p} | {:.0} | {:.0} / {} | {:.1} |", s.name(), rs.iter().map(|r| r.steps as f64).sum::<f64>() / n, rs.iter().map(|r| r.nodes).sum::<f64>() / n, rs.iter().map(|r| r.max_nodes).max().unwrap(), rs.iter().map(|r| r.micros).sum::<f64>() / n);
        }
    }
    for p in ["greedy-b1", "pure-d2", "hybrid-d2", "hybrid-d3"] {
        let rs: Vec<Res> = (1..=5).map(|seed| run_m1(p, seed, 600, CHOSEN)).collect();
        let n = rs.len() as f64;
        println!("| menu world 600 | {p} | {:.0} | {:.0} / {} | {:.1} |", rs.iter().map(|r| r.steps as f64).sum::<f64>() / n, rs.iter().map(|r| r.nodes).sum::<f64>() / n, rs.iter().map(|r| r.max_nodes).max().unwrap(), rs.iter().map(|r| r.micros).sum::<f64>() / n);
    }
    let c = W2_CONDS[1];
    for p in ["learner-b1", "pure-d2", "hybrid-d2", "hybrid-d3"] {
        let rs: Vec<Res> = (1..=5).map(|seed| run_v2p(p, &c.cfg(seed), CHOSEN)).collect();
        let n = rs.len() as f64;
        println!("| World v2 medium clean | {p} | {:.0} | {:.0} / {} | {:.1} |", rs.iter().map(|r| r.steps as f64).sum::<f64>() / n, rs.iter().map(|r| r.nodes).sum::<f64>() / n, rs.iter().map(|r| r.max_nodes).max().unwrap(), rs.iter().map(|r| r.micros).sum::<f64>() / n);
    }
}

fn quick() {
    for s in SETTINGS {
        for seed in 1..=2u64 {
            let cfg = WLCfg { seed, setting: s, budget: 400 };
            let w = WorldL::new(cfg.clone());
            println!("{} seed {seed}: fams {:?}", s.name(), w.fams.iter().map(|f| format!("{:?} c{} r{} p{:.2}/{:.2}/{:.2}", f.kind, f.cost, f.reward, f.p[0], f.p[1], f.p[2])).collect::<Vec<_>>());
            for p in WL_POL {
                let r = run_wl_pol(p, &cfg, CHOSEN);
                println!("  {p:16} rate {:.3} steps {} nodes/step {:.0} special {:.3} us/step {:.1}", r.rate, r.steps, r.nodes, r.special, r.micros);
            }
        }
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "report".into());
    match mode.as_str() {
        "quick" => quick(),
        "tune" => tune(),
        "cost" => cost(),
        _ => {
            println!("Track B8 measurement; planner constants {CHOSEN:?}, node cap {NODES}");
            for s in SETTINGS {
                report_wl(s, 400, CHOSEN);
            }
            for s in SETTINGS {
                report_wl(s, 1600, CHOSEN);
            }
            report_m1(600, 1..=20, CHOSEN, "report seeds 1..20");
            report_m1(3000, 1..=20, CHOSEN, "report seeds 1..20");
            for c in W2_CONDS.iter() {
                report_v2(c, 1..=20, CHOSEN, "report seeds 1..20");
            }
        }
    }
}
