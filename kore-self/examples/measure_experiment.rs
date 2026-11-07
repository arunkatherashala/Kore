//! Track B9 measurement: experiments chosen by expected information gain, against passive discovery
//! (the current learner) and random probes with the same experiment budget, in World v2 (all seven
//! conditions) and the B2 / B5 menu worlds. Synthetic worlds, author-defined families. Prints the
//! tables recorded in docs/measured.md ("Track B9: experiments").
//!   cargo run -p kore-self --example measure_experiment --offline --release -- [quick|tune|report] [kappa] [cap_pm]
//! Report seeds 1..=20; tuning seeds 101..=120 (used only to choose kappa and the budget).

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::experiment::DEFAULT_KAPPA;
use being::measure::mean_ci;
use being::measure_b9::{menu_world, w2_world, Arm, MenuWorld};
use being::world2::world::W2Cfg;
use rayon::prelude::*;

const MAX_N: u64 = 5000;

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

const CONDS: [Cond; 7] = [
    Cond { name: "small clean", families: 6, budget: 1500, noise_pm: 0, drift: 0, curve: 0 },
    Cond { name: "medium clean", families: 12, budget: 3000, noise_pm: 0, drift: 0, curve: 0 },
    Cond { name: "medium noisy", families: 12, budget: 3000, noise_pm: 100, drift: 0, curve: 0 },
    Cond { name: "medium drift", families: 12, budget: 3000, noise_pm: 0, drift: 20, curve: 0 },
    Cond { name: "medium hard", families: 12, budget: 3000, noise_pm: 100, drift: 20, curve: 100 },
    Cond { name: "large clean", families: 24, budget: 6000, noise_pm: 0, drift: 0, curve: 0 },
    Cond { name: "large hard", families: 24, budget: 6000, noise_pm: 100, drift: 20, curve: 100 },
];

fn fmt_ci(xs: &[f64]) -> String {
    let (m, lo, hi) = mean_ci(xs);
    format!("{m:.3} [{lo:.3}, {hi:.3}]")
}

fn fmt_diff(a: &[f64], b: &[f64]) -> String {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let (m, lo, hi) = mean_ci(&d);
    let w = d.iter().filter(|x| **x > 1e-9).count();
    let l = d.iter().filter(|x| **x < -1e-9).count();
    format!("{m:+.3} [{lo:+.3}, {hi:+.3}] {w}/{l}/{}", d.len() - w - l)
}

fn col(rows: &[Vec<Arm>], name: &str, f: impl Fn(&Arm) -> f64) -> Vec<f64> {
    rows.iter().map(|r| f(r.iter().find(|a| a.name == name).expect("arm"))).collect()
}

fn has(rows: &[Vec<Arm>], name: &str) -> bool {
    rows.first().map_or(false, |r| r.iter().any(|a| a.name == name))
}

fn print_block(title: &str, rows: &[Vec<Arm>]) {
    println!("\n### {title} (n = {} worlds)\n", rows.len());
    let arms: Vec<String> = rows[0].iter().map(|a| a.name.clone()).collect();
    println!("| arm | reward per energy | targets covered per world | energy share until covered (1 = never) | mean steps until covered | rules / wrong at acceptance (all worlds) | experiments per world | experiment energy share |");
    println!("|---|---|---|---|---|---|---|---|");
    for a in &arms {
        let rw = col(rows, a, |x| x.per_energy());
        let found = col(rows, a, |x| x.speed.found as f64);
        let ef = col(rows, a, |x| x.speed.energy_frac);
        let st = col(rows, a, |x| x.speed.steps);
        let rules: usize = rows.iter().map(|r| r.iter().find(|x| &x.name == a).unwrap().speed.rules).sum();
        let wrong: usize = rows.iter().map(|r| r.iter().find(|x| &x.name == a).unwrap().speed.wrong).sum();
        let ne = col(rows, a, |x| x.exp.n as f64);
        let sh = col(rows, a, |x| x.exp_share());
        let tg = rows[0].iter().find(|x| &x.name == a).unwrap().speed.targets;
        if a == "oracle-knows-rules" {
            println!("| {a} | {} | - | - | - | - | - | - |", fmt_ci(&rw));
        } else {
            println!("| {a} | {} | {} (of {tg} in world 1) | {} | {:.1} | {rules} / {wrong} | {:.1} | {:.3} |", fmt_ci(&rw), fmt_ci(&found), fmt_ci(&ef), mean_ci(&st).0, mean_ci(&ne).0, mean_ci(&sh).0);
        }
    }
    let e = "experiments";
    println!("\n| paired difference (wins/losses/ties) | reward per energy | targets covered | energy share until covered (negative = faster) |");
    println!("|---|---|---|---|");
    for b in ["passive", "random-matched", "random-cap", "split-only", "support-only"] {
        if !has(rows, b) {
            continue;
        }
        println!(
            "| experiments - {b} | {} | {} | {} |",
            fmt_diff(&col(rows, e, |x| x.per_energy()), &col(rows, b, |x| x.per_energy())),
            fmt_diff(&col(rows, e, |x| x.speed.found as f64), &col(rows, b, |x| x.speed.found as f64)),
            fmt_diff(&col(rows, e, |x| x.speed.energy_frac), &col(rows, b, |x| x.speed.energy_frac))
        );
    }
    if has(rows, "oracle-knows-rules") {
        for a in ["passive", "experiments", "random-matched", "random-cap"] {
            println!("| regret: oracle-knows-rules - {a} | {} | | |", fmt_diff(&col(rows, "oracle-knows-rules", |x| x.per_energy()), &col(rows, a, |x| x.per_energy())));
        }
    }
    let rej: f64 = col(rows, e, |x| x.exp_rejecting as f64).iter().sum();
    let ne: f64 = col(rows, e, |x| x.exp.n as f64).iter().sum();
    let refused: u64 = rows.iter().map(|r| r.iter().find(|x| x.name == e).unwrap().exp.refused_energy + r.iter().find(|x| x.name == e).unwrap().exp.refused_count).sum();
    let eig: f64 = rows.iter().map(|r| r.iter().find(|x| x.name == e).unwrap().exp.eig_sum).sum();
    let sup: f64 = rows.iter().map(|r| r.iter().find(|x| x.name == e).unwrap().exp.support_sum).sum();
    println!("\nexperiments: {ne:.0} in all worlds, {rej:.0} of them rejected at least one live hypothesis of the plain grammar; mean expected split {:.3} bits and support gain {:.3} per experiment; experiments refused by the budget: {refused}", eig / ne.max(1.0), sup / ne.max(1.0));
}

fn w2_rows(c: &Cond, seeds: std::ops::RangeInclusive<u64>, kappa: f64, cap: u32, random: bool) -> Vec<Vec<Arm>> {
    let seeds: Vec<u64> = seeds.collect();
    seeds.par_iter().map(|s| w2_world(&c.cfg(*s), kappa, cap, MAX_N, random)).collect()
}

fn menu_rows(mw: MenuWorld, budget: i64, seeds: std::ops::RangeInclusive<u64>, kappa: f64, cap: u32, random: bool) -> Vec<Vec<Arm>> {
    let seeds: Vec<u64> = seeds.collect();
    seeds.par_iter().map(|s| menu_world(mw, *s, budget, kappa, cap, MAX_N, random)).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(|s| s.as_str()).unwrap_or("quick");
    let kappa: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_KAPPA);
    let cap: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(being::measure_b9::DEFAULT_CAP_PM);
    let t0 = std::time::Instant::now();
    match mode {
        "quick" => {
            print_block("World v2 medium clean, seeds 101..104", &w2_rows(&CONDS[1], 101..=104, kappa, cap, true));
            print_block("B2 world, budget 3000, seeds 101..104", &menu_rows(MenuWorld::B2, 3000, 101..=104, kappa, cap, true));
        }
        "tune" => {
            // tuning seeds only; objective fixed in advance: paired reward per energy (experiments - passive), then energy share until covered
            println!("tuning seeds 101..120; grid kappa x cap_pm; medium clean World v2 and B2 world budget 3000");
            println!("\n| world | kappa | cap_pm | reward diff vs passive | covered diff | energy-share diff | wrong rules (exp / passive) | experiments per world |");
            println!("|---|---|---|---|---|---|---|---|");
            for (wname, which) in [("W2 medium clean", 0), ("B2 3000", 1)] {
                for k in [0.1, 0.25, 0.5, 1.0, 2.0] {
                    for c in [50u32, 100, 200] {
                        let rows = if which == 0 { w2_rows(&CONDS[1], 101..=120, k, c, false) } else { menu_rows(MenuWorld::B2, 3000, 101..=120, k, c, false) };
                        let wr = |n: &str| rows.iter().map(|r| r.iter().find(|a| a.name == n).unwrap().speed.wrong).sum::<usize>();
                        println!(
                            "| {wname} | {k} | {c} | {} | {} | {} | {} / {} | {:.1} |",
                            fmt_diff(&col(&rows, "experiments", |x| x.per_energy()), &col(&rows, "passive", |x| x.per_energy())),
                            fmt_diff(&col(&rows, "experiments", |x| x.speed.found as f64), &col(&rows, "passive", |x| x.speed.found as f64)),
                            fmt_diff(&col(&rows, "experiments", |x| x.speed.energy_frac), &col(&rows, "passive", |x| x.speed.energy_frac)),
                            wr("experiments"),
                            wr("passive"),
                            mean_ci(&col(&rows, "experiments", |x| x.exp.n as f64)).0
                        );
                    }
                }
            }
        }
        "report" | "report-w2" | "report-menu" => {
            println!("report seeds 1..20, kappa {kappa}, experiment budget {cap} per mille of the life budget");
            if mode != "report-menu" {
                for c in CONDS.iter() {
                    let title = format!("World v2 {} ({} families, budget {}, noise {} pm, drift {}, curve {}%)", c.name, c.families, c.budget, c.noise_pm, c.drift, c.curve);
                    print_block(&title, &w2_rows(c, 1..=20, kappa, cap, true));
                    eprintln!("{} done at {:.0}s", c.name, t0.elapsed().as_secs_f64());
                }
            }
            if mode != "report-w2" {
                for (mw, b) in [(MenuWorld::B2, 3000), (MenuWorld::B2, 600), (MenuWorld::B5, 3000)] {
                    print_block(&format!("{:?} menu world, budget {b}", mw), &menu_rows(mw, b, 1..=20, kappa, cap, true));
                    eprintln!("{mw:?} {b} done at {:.0}s", t0.elapsed().as_secs_f64());
                }
            }
        }
        _ => eprintln!("modes: quick | tune | report | report-w2 | report-menu"),
    }
    eprintln!("total {:.0}s", t0.elapsed().as_secs_f64());
}
