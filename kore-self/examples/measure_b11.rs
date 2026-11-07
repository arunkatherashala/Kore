//! Track B11 measurements (synthetic worlds, author-defined rewards; see docs/measured.md, Track B11).
//!   cargo run -p kore-self --example measure_b11 --offline --release -- decline --seeds 1..4 --steps 400000 --window 50000
//!   cargo run ... -- b1 [--seeds 1..20]          # B1 menu numbers with the B1 skip test and the calibrated one
//! Run data is printed (and optionally written with --out) and never committed.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;
#[path = "measure_b11_parts/worldo.rs"]
mod worldo;
#[path = "measure_b11_parts/worldo_log.rs"]
mod worldo_log;

use being::measure::{mean_ci, run_menu};
use being::measure_b11::{decline_run, oracle_best_lambda, renewal_oracle, DeclineCfg, DeclineWin};
use being::planner::{ModelChooser, SkipRule};
use rayon::prelude::*;

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

fn seeds(args: &[String], def: (u64, u64)) -> Vec<u64> {
    let s = arg(args, "--seeds").unwrap_or(format!("{}..{}", def.0, def.1));
    let (a, b) = s.split_once("..").expect("--seeds a..b");
    (a.parse().unwrap()..=b.parse().unwrap()).collect()
}

fn ci(xs: &[f64]) -> String {
    let (m, lo, hi) = mean_ci(xs);
    format!("{m:.3} [{lo:.3}, {hi:.3}]")
}

fn cmd_decline(args: &[String]) {
    let ss = seeds(args, (1, 4));
    let steps: u64 = arg(args, "--steps").map_or(400_000, |x| x.parse().unwrap());
    let window: u64 = arg(args, "--window").map_or(50_000, |x| x.parse().unwrap());
    let disc: u64 = arg(args, "--discover-every").map_or(10, |x| x.parse().unwrap());
    let rules: Vec<SkipRule> = arg(args, "--rules").unwrap_or("rate,calibrated,settled".into()).split(',').map(|x| SkipRule::parse(x).expect("skip rule")).collect();
    let t0 = std::time::Instant::now();
    let orc: Vec<(u64, f64, f64)> = ss
        .par_iter()
        .map(|&s| {
            let (r, l) = oracle_best_lambda(s, 30_000);
            let (rr, rho) = renewal_oracle(s, 30_000, r, 4);
            println!("world {s}: skip oracle (one threshold for all options) {r:.3} reward per energy at threshold {l:.2}; renewal oracle {rr:.3} (rho {rho:.3})");
            // the reference of the step values and of the regret: the renewal oracle
            (s, rr, -1.0)
        })
        .collect();
    let jobs: Vec<(u64, SkipRule, f64, f64)> = orc.iter().flat_map(|&(s, r, l)| rules.iter().map(move |&k| (s, k, r, l))).collect();
    let res: Vec<((u64, SkipRule, f64), Vec<DeclineWin>)> = jobs
        .par_iter()
        .map(|&(s, k, r, l)| ((s, k, r), decline_run(&DeclineCfg { world_seed: s, steps, window, budget: 3000, skip_rule: k, lambda: l, rho: r, discover_every: disc })))
        .collect();
    eprintln!("decline runs: {:.0} s", t0.elapsed().as_secs_f64());
    for ((s, k, r), ws) in &res {
        println!("\n#### world {s}, skip rule {}, renewal oracle {r:.3}\n", k.code());
        println!("| steps | reward per energy | regret vs renewal oracle | attempt share | oracle attempt share | over-skips per 1000 | under-skips per 1000 | loss from skip decisions per 1000 steps | loss from option choice per 1000 steps | mean bonus of best option | mean bonus of attempted option | best mean score within 0.25 below the bar, per 1000 | mean skip bar | mean abs error of p on attempts | live rules |");
        println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
        for w in ws {
            let n = w.steps as f64;
            println!(
                "| {}k-{}k | {:.3} | {:+.3} | {:.3} | {:.3} | {:.1} | {:.1} | {:.1} | {:.1} | {:.4} | {:.4} | {:.1} | {:.3} | {:.4} | {} |",
                w.from / 1000, (w.from + w.steps) / 1000, w.rpe(), r - w.rpe(), w.attempts as f64 / n, w.oracle_attempts as f64 / n, 1000.0 * w.over_skip as f64 / n, 1000.0 * w.under_skip as f64 / n,
                1000.0 * w.loss_skip / n, 1000.0 * w.loss_choice / n, w.bonus / n, w.bonus_attempted / w.attempts.max(1) as f64, 1000.0 * w.near_bar as f64 / n, w.bar / n, w.est_err / w.attempts.max(1) as f64, w.rules_end
            );
        }
    }
    // paired summary: first full window after the first vs the last window
    println!("\n#### Paired over worlds: reward per energy in the second window and the last window, by skip rule\n");
    println!("| skip rule | worlds | second window | last window | last minus second | regret last window |");
    println!("|---|---|---|---|---|---|");
    for &k in &rules {
        let rows: Vec<&((u64, SkipRule, f64), Vec<DeclineWin>)> = res.iter().filter(|x| x.0 .1 == k).collect();
        let a: Vec<f64> = rows.iter().map(|x| x.1[1.min(x.1.len() - 1)].rpe()).collect();
        let b: Vec<f64> = rows.iter().map(|x| x.1.last().unwrap().rpe()).collect();
        let d: Vec<f64> = a.iter().zip(&b).map(|(x, y)| y - x).collect();
        let rg: Vec<f64> = rows.iter().map(|x| x.0 .2 - x.1.last().unwrap().rpe()).collect();
        println!("| {} | {} | {} | {} | {} | {} |", k.code(), rows.len(), ci(&a), ci(&b), ci(&d), ci(&rg));
    }
    for j in 1..rules.len() {
        let mut d = vec![];
        for s in &ss {
            let x: Vec<&Vec<DeclineWin>> = rules.iter().map(|k| &res.iter().find(|r| r.0 .0 == *s && r.0 .1 == *k).unwrap().1).collect();
            let tot = |ws: &Vec<DeclineWin>| {
                let (r, c) = ws.iter().skip(1).fold((0i64, 0i64), |a, w| (a.0 + w.reward, a.1 + w.cost));
                r as f64 / c as f64
            };
            d.push(tot(x[j]) - tot(x[0]));
        }
        println!("\n{} minus {} over all windows after the first, paired by world: {}", rules[j].code(), rules[0].code(), ci(&d));
    }
}

fn cmd_b1(args: &[String]) {
    let ss = seeds(args, (1, 20));
    let rules: Vec<SkipRule> = arg(args, "--rules").unwrap_or("calibrated,settled".into()).split(',').map(|x| SkipRule::parse(x).expect("skip rule")).collect();
    for budget in [600i64, 3000] {
        let base: Vec<f64> = ss.par_iter().map(|&s| run_menu(s, budget, &mut ModelChooser::new(being::planner::DEFAULT_BETA)).per_budget()).collect();
        for &k in &rules {
            let b: Vec<f64> = ss.par_iter().map(|&s| run_menu(s, budget, &mut ModelChooser { skip_rule: k, ..ModelChooser::new(being::planner::DEFAULT_BETA) }).per_budget()).collect();
            let d: Vec<f64> = b.iter().zip(&base).map(|(x, y)| x - y).collect();
            let wins = d.iter().filter(|x| **x > 0.0).count();
            let ties = d.iter().filter(|x| **x == 0.0).count();
            println!("| {budget} | {} | {} | {} | {} | {} | {wins} / {ties} / {} |", ss.len(), k.code(), ci(&base), ci(&b), ci(&d), d.len() - wins - ties);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("decline") => cmd_decline(&args[1..]),
        Some("worldo-log") => worldo_log::cmd_worldo_log(&args[1..]),
        Some("worldo") => worldo::cmd_worldo(&args[1..], seeds(&args[1..], (1, 20))),
        Some("b1") => {
            println!("| budget | worlds | skip test | B1 skip test | this skip test | difference | better / equal / worse |");
            println!("|---|---|---|---|---|---|---|");
            cmd_b1(&args[1..])
        }
        _ => eprintln!("usage: measure_b11 decline|b1 [--seeds a..b] [--steps N] [--window N]"),
    }
}
