//! Measures the existing learner against baselines and oracles in World v2 and prints the tables
//! recorded in docs/measured.md (section "World v2"). Synthetic world, author-defined families.
//!   cargo run -p kore-self --example measure_world_v2 --offline --release -- [report|tune|quick]
//! Report seeds 1..=20; tuning seeds 101..=120 (used only to choose beta and the UCB1 constant).

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::controller::Controller;
use being::measure::mean_ci;
use being::solver::Strategy;
use being::world2::baselines::{Always2, Oracle2, Random2, Ucb2};
use being::world2::learner::Learner2;
use being::world2::measure::{run_w2, W2Run};
use being::world2::world::{FType, W2Cfg, World2, FTYPES};
use rayon::prelude::*;

const EVERY: u64 = 10;

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

const POLICIES: [&str; 11] = ["random", "always-fast", "always-careful", "always-deep", "ucb1", "learner-no-discovery", "learner", "learner-default-caps", "learner-compose", "oracle-planner", "oracle-knows-rules"];

fn run_policy(name: &str, cfg: &W2Cfg, beta: f64, c: f64) -> W2Run {
    let l = Learner2 { beta, ..Learner2::default() };
    let caps = |compose: bool| {
        let mut ctl = Controller::defaults();
        ctl.composition = compose;
        Some(ctl.limits(compose, false).0)
    };
    match name {
        "random" => run_w2(cfg, &mut Random2::new(cfg.seed), 0, None),
        "always-fast" => run_w2(cfg, &mut Always2 { strategy: Strategy::Fast }, 0, None),
        "always-careful" => run_w2(cfg, &mut Always2 { strategy: Strategy::Careful }, 0, None),
        "always-deep" => run_w2(cfg, &mut Always2 { strategy: Strategy::Deep }, 0, None),
        "ucb1" => run_w2(cfg, &mut Ucb2 { c }, 0, None),
        "learner-no-discovery" => run_w2(cfg, &mut l.clone(), 0, None),
        "learner" => run_w2(cfg, &mut l.clone(), EVERY, None),
        "learner-default-caps" => run_w2(cfg, &mut l.clone(), EVERY, caps(false)),
        "learner-compose" => run_w2(cfg, &mut l.clone(), EVERY, caps(true)),
        "oracle-planner" => run_w2(cfg, &mut Oracle2::new(World2::new(cfg.clone()), false), 0, None),
        "oracle-knows-rules" => run_w2(cfg, &mut Oracle2::new(World2::new(cfg.clone()), true), 0, None),
        _ => unreachable!(),
    }
}

struct Row {
    runs: Vec<W2Run>,
}

fn rows(cond: &Cond, seeds: std::ops::RangeInclusive<u64>, beta: f64, c: f64, which: &[&str]) -> Vec<Row> {
    let seeds: Vec<u64> = seeds.collect();
    seeds.par_iter().map(|s| Row { runs: which.iter().map(|p| run_policy(p, &cond.cfg(*s), beta, c)).collect() }).collect()
}

fn line(name: &str, xs: &[f64]) {
    let (m, lo, hi) = mean_ci(xs);
    let min = xs.iter().cloned().fold(f64::MAX, f64::min);
    let max = xs.iter().cloned().fold(f64::MIN, f64::max);
    println!("| {name} | {m:.3} | [{lo:.3}, {hi:.3}] | {min:.3} to {max:.3} |");
}

fn diff(name: &str, a: &[f64], b: &[f64]) {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let (m, lo, hi) = mean_ci(&d);
    let w = d.iter().filter(|x| **x > 1e-9).count();
    let l = d.iter().filter(|x| **x < -1e-9).count();
    println!("| {name} | {m:+.3} | [{lo:+.3}, {hi:+.3}] | {w} / {l} / {} |", d.len() - w - l);
}

fn col(rs: &[Row], i: usize) -> Vec<f64> {
    rs.iter().map(|r| r.runs[i].per_energy()).collect()
}

fn idx(name: &str) -> usize {
    POLICIES.iter().position(|p| *p == name).unwrap()
}

fn type_stats(runs: &[&W2Run]) -> Vec<(FType, f64, f64)> {
    FTYPES
        .iter()
        .map(|t| {
            let (mut e, mut r, mut tot) = (0i64, 0i64, 0i64);
            for run in runs {
                for x in &run.recs {
                    tot += x.cost;
                    if x.ftype == Some(*t) && x.attempted {
                        e += x.cost;
                        r += x.true_reward;
                    }
                }
            }
            (*t, if tot > 0 { e as f64 / tot as f64 } else { 0.0 }, if e > 0 { r as f64 / e as f64 } else { 0.0 })
        })
        .collect()
}

fn report(cond: &Cond, beta: f64, c: f64) {
    let rs = rows(cond, 1..=20, beta, c, &POLICIES);
    println!("\n### {} ({} families, budget {}, noise {} per mille, drift {} per 10000 energy, curve {}%; seeds 1..20, n = 20 worlds)\n", cond.name, cond.families, cond.budget, cond.noise_pm, cond.drift, cond.curve);
    println!("| policy | true reward per energy | 95% interval of the mean | per-world range |");
    println!("|---|---|---|---|");
    for (i, p) in POLICIES.iter().enumerate() {
        line(p, &col(&rs, i));
    }
    let lr = col(&rs, idx("learner"));
    let best_always: Vec<f64> = rs.iter().map(|r| ["always-fast", "always-careful", "always-deep"].iter().map(|p| r.runs[idx(p)].per_energy()).fold(f64::MIN, f64::max)).collect();
    println!("\n| paired difference | mean | 95% interval | better / worse / tie (of 20) |");
    println!("|---|---|---|---|");
    diff("learner - random", &lr, &col(&rs, idx("random")));
    diff("learner - best always-X of the world (hindsight)", &lr, &best_always);
    diff("learner - UCB1", &lr, &col(&rs, idx("ucb1")));
    diff("learner - learner without discovery", &lr, &col(&rs, idx("learner-no-discovery")));
    diff("learner with default controller caps - learner", &col(&rs, idx("learner-default-caps")), &lr);
    diff("learner with caps and composition - learner", &col(&rs, idx("learner-compose")), &lr);
    diff("oracle-planner - learner", &col(&rs, idx("oracle-planner")), &lr);
    diff("oracle-knows-rules - learner", &col(&rs, idx("oracle-knows-rules")), &lr);
    let frac: Vec<f64> = rs.iter().map(|r| r.runs[idx("learner")].per_energy() / r.runs[idx("oracle-knows-rules")].per_energy().max(1e-9)).collect();
    let fracp: Vec<f64> = rs.iter().map(|r| r.runs[idx("learner")].per_energy() / r.runs[idx("oracle-planner")].per_energy().max(1e-9)).collect();
    let (m, lo, hi) = mean_ci(&frac);
    let (mp, lop, hip) = mean_ci(&fracp);
    println!("\nlearner as a share of oracle-knows-rules: {m:.3} [{lo:.3}, {hi:.3}]; of oracle-planner: {mp:.3} [{lop:.3}, {hip:.3}]");

    // where the learner spends energy and what it earns there (true reward), against the oracles
    let lruns: Vec<&W2Run> = rs.iter().map(|r| &r.runs[idx("learner")]).collect();
    let pruns: Vec<&W2Run> = rs.iter().map(|r| &r.runs[idx("oracle-planner")]).collect();
    let oruns: Vec<&W2Run> = rs.iter().map(|r| &r.runs[idx("oracle-knows-rules")]).collect();
    let (lt, pt, ot) = (type_stats(&lruns), type_stats(&pruns), type_stats(&oruns));
    println!("\n| family type | learner: share of energy | learner: true reward per energy there | oracle-planner: share / rate | oracle-knows-rules: share / rate |");
    println!("|---|---|---|---|---|");
    for i in 0..lt.len() {
        println!("| {} | {:.3} | {:.3} | {:.3} / {:.3} | {:.3} / {:.3} |", lt[i].0.name(), lt[i].1, lt[i].2, pt[i].1, pt[i].2, ot[i].1, ot[i].2);
    }
    let skip_share: f64 = lruns.iter().map(|r| r.recs.iter().filter(|x| !x.attempted).map(|x| x.cost).sum::<i64>() as f64 / r.cost.max(1) as f64).sum::<f64>() / 20.0;
    // rules
    let acc: Vec<f64> = lruns.iter().map(|r| r.rules.len() as f64).collect();
    let wrong: usize = lruns.iter().map(|r| r.rules.iter().filter(|x| x.wrong_at_accept).count()).sum();
    let total_rules: usize = lruns.iter().map(|r| r.rules.len()).sum();
    let stale: usize = lruns.iter().map(|r| r.rules.iter().filter(|x| x.right_share_end.map_or(false, |s| s < 0.9)).count()).sum();
    let seq_att: usize = lruns.iter().map(|r| r.recs.iter().filter(|x| x.attempted && matches!(x.ftype, Some(FType::Seq) | Some(FType::Compose))).count()).sum();
    let rule_att: Vec<&being::world2::engine::TrueRec> = lruns.iter().flat_map(|r| r.recs.iter().filter(|x| x.attempted && x.rule_used)).collect();
    let rule_ok = rule_att.iter().filter(|x| x.true_correct).count();
    // reported vs true success
    let att: Vec<&being::world2::engine::TrueRec> = lruns.iter().flat_map(|r| r.recs.iter().filter(|x| x.attempted)).collect();
    let rep = att.iter().filter(|x| x.reported_correct).count() as f64 / att.len().max(1) as f64;
    let tru = att.iter().filter(|x| x.true_correct).count() as f64 / att.len().max(1) as f64;
    let pred = att.iter().filter_map(|x| x.predicted_p).sum::<f64>() / att.len().max(1) as f64;
    let (am, alo, ahi) = mean_ci(&acc);
    println!("\nlearner details: skipped energy share {skip_share:.3}; rules accepted per world {am:.1} [{alo:.1}, {ahi:.1}], {total_rules} in total, {wrong} wrong on fresh tasks at acceptance, {stale} right on fewer than 90% of fresh tasks at the end of the life; sequence/compose attempts {seq_att}, answered by a rule {} ({} right by the checker); mean predicted success {pred:.3}, reported success {rep:.3}, true success {tru:.3}", rule_att.len(), rule_ok);
    // hidden-rule diagnostics (the measurement knows the world)
    let mut disc = 0usize;
    let (mut fe_bad, mut fr_bad, mut fe_all) = (0i64, 0i64, 0i64);
    let (mut ge_wrong, mut ge_all) = (0i64, 0i64);
    for (si, r) in lruns.iter().enumerate() {
        let w = World2::new(cond.cfg(si as u64 + 1));
        for x in r.recs.iter().filter(|x| x.attempted) {
            disc += (x.strategy == Some(Strategy::Discovered)) as usize;
            let (Some(f), Some(t)) = (x.fam, x.ftype) else { continue };
            let reg = w.regime(f, x.clock);
            if t == FType::Formula {
                fe_all += x.cost;
                if reg.g != 10 {
                    fe_bad += x.cost;
                    fr_bad += x.true_reward;
                }
            }
            if t == FType::Grid {
                ge_all += x.cost;
                let right = if reg.eight { Strategy::Deep } else { Strategy::Careful };
                if x.strategy != Some(right) && !(x.strategy == Some(Strategy::Discovered) && right == Strategy::Careful) {
                    ge_wrong += x.cost;
                }
            }
        }
    }
    println!("hidden rules: attempts with the discovered strategy {disc}; formula energy on families whose hidden g is not 10: {:.3} of formula energy, true reward per energy there {:.3}; grid energy with a strategy that does not match the hidden move rule: {:.3} of grid energy", fe_bad as f64 / fe_all.max(1) as f64, fr_bad as f64 / fe_bad.max(1) as f64, ge_wrong as f64 / ge_all.max(1) as f64);
    // late life
    let b = cond.budget;
    let late_l: Vec<f64> = lruns.iter().map(|r| r.window_e(3 * b / 4, b + 1)).collect();
    let late_o: Vec<f64> = oruns.iter().map(|r| r.window_e(3 * b / 4, b + 1)).collect();
    let early_l: Vec<f64> = lruns.iter().map(|r| r.window_e(0, b / 4)).collect();
    let early_o: Vec<f64> = oruns.iter().map(|r| r.window_e(0, b / 4)).collect();
    let sh = |a: &[f64], o: &[f64]| mean_ci(&a.iter().zip(o).map(|(x, y)| x / y.max(1e-9)).collect::<Vec<_>>());
    let (e1, e2, e3) = sh(&early_l, &early_o);
    let (l1, l2, l3) = sh(&late_l, &late_o);
    println!("learner / oracle-knows-rules, first quarter of the energy: {e1:.3} [{e2:.3}, {e3:.3}]; last quarter: {l1:.3} [{l2:.3}, {l3:.3}]");
}

fn tune() {
    let cond = Cond { name: "tuning", families: 12, budget: 3000, noise_pm: 50, drift: 10, curve: 0 };
    println!("\n### Tuning (seeds 101..120, 12 families, budget 3000, noise 50, drift 10)\n");
    println!("| learner beta | true reward per energy | 95% interval |");
    println!("|---|---|---|");
    for beta in [0.5, 1.0, 1.5, 2.0, 3.0] {
        let rs = rows(&cond, 101..=120, beta, 0.05, &["learner"]);
        let (m, lo, hi) = mean_ci(&rs.iter().map(|r| r.runs[0].per_energy()).collect::<Vec<_>>());
        println!("| {beta} | {m:.3} | [{lo:.3}, {hi:.3}] |");
    }
    println!("\n| UCB1 c | true reward per energy | 95% interval |");
    println!("|---|---|---|");
    for c in [0.02, 0.05, 0.2, 0.5, 1.0] {
        let rs = rows(&cond, 101..=120, 1.5, c, &["ucb1"]);
        let (m, lo, hi) = mean_ci(&rs.iter().map(|r| r.runs[0].per_energy()).collect::<Vec<_>>());
        println!("| {c} | {m:.3} | [{lo:.3}, {hi:.3}] |");
    }
}

fn quick() {
    for seed in 1..=3u64 {
        let cfg = W2Cfg { noise_pm: 50, drift_per_10k: 10, ..W2Cfg::new(seed, 12, 3000) };
        let w = World2::new(cfg.clone());
        println!("seed {seed}: families {:?}", w.fams.iter().map(|f| format!("{}:{}", f.name(), if matches!(f.ftype, FType::Seq) { w.regime(f.idx, 0).class.name().to_string() } else if matches!(f.ftype, FType::Formula) { format!("g{}", w.regime(f.idx, 0).g) } else if matches!(f.ftype, FType::Grid) { if w.regime(f.idx, 0).eight { "8".into() } else { "4".into() } } else { String::new() })).collect::<Vec<_>>());
        println!("  drift events: {}", w.drift.len());
        for p in POLICIES {
            let t0 = std::time::Instant::now();
            let r = run_policy(p, &cfg, 1.5, 0.05);
            println!("  {p:24} true {:.3} reported {:.3} steps {} rules {} ({} ms)", r.per_energy(), r.reported_reward as f64 / r.cost.max(1) as f64, r.steps, r.rules.len(), t0.elapsed().as_millis());
        }
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "report".into());
    let beta: f64 = std::env::args().nth(2).and_then(|x| x.parse().ok()).unwrap_or(1.5);
    let c: f64 = std::env::args().nth(3).and_then(|x| x.parse().ok()).unwrap_or(0.05);
    match mode.as_str() {
        "quick" => quick(),
        "tune" => tune(),
        _ => {
            println!("World v2 measurement (learner beta {beta}, UCB1 c {c}); score = TRUE reward per energy unit");
            let conds = [
                Cond { name: "small, clean", families: 6, budget: 1500, noise_pm: 0, drift: 0, curve: 0 },
                Cond { name: "medium, clean", families: 12, budget: 3000, noise_pm: 0, drift: 0, curve: 0 },
                Cond { name: "medium, noisy feedback", families: 12, budget: 3000, noise_pm: 100, drift: 0, curve: 0 },
                Cond { name: "medium, drifting rules", families: 12, budget: 3000, noise_pm: 0, drift: 20, curve: 0 },
                Cond { name: "medium, hard (noise, drift, rising difficulty)", families: 12, budget: 3000, noise_pm: 100, drift: 20, curve: 100 },
                Cond { name: "large, clean", families: 24, budget: 6000, noise_pm: 0, drift: 0, curve: 0 },
                Cond { name: "large, hard (noise, drift, rising difficulty)", families: 24, budget: 6000, noise_pm: 100, drift: 20, curve: 100 },
            ];
            for c0 in conds.iter() {
                report(c0, beta, c);
            }
        }
    }
}
