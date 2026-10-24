//! Measures the learned chooser against baselines in the simulated world and
//! prints the tables recorded in docs/measured.md (synthetic world, author-defined rewards).
//!   cargo run -p kore-self --example measure_being --offline --release
//! Report seeds 1..=20; tuning seeds 101..=120 (used only to choose the exploration constant).

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::chooser::DEFAULT_EXPLORE_C;
use being::measure::*;
use being::world::{Domain, DOMAINS};

fn col(rows: &[SeedRow], f: impl Fn(&SeedRow) -> f64) -> (f64, f64, f64) {
    mean_ci(&rows.iter().map(f).collect::<Vec<_>>())
}

fn line(name: &str, rows: &[SeedRow], f: impl Fn(&SeedRow) -> f64) {
    let (m, lo, hi) = col(rows, &f);
    let min = rows.iter().map(&f).fold(f64::MAX, f64::min);
    let max = rows.iter().map(&f).fold(f64::MIN, f64::max);
    println!("| {name} | {m:.3} | [{lo:.3}, {hi:.3}] | {min:.3}-{max:.3} |");
}

fn report(label: &str, seeds: std::ops::RangeInclusive<u64>, c: f64) {
    let rows: Vec<SeedRow> = seeds.map(|s| seed_row(s, c, BUDGET)).collect();
    println!("\n### {label} (c = {c}, budget {BUDGET}, n = {} worlds)\n", rows.len());
    let mut best = [0; 4];
    for r in &rows {
        best[r.best.idx()] += 1;
    }
    println!("Hidden best domain per world: arithmetic {} / algebra {} / physics {} / pattern {}\n", best[0], best[1], best[2], best[3]);
    println!("| policy | mean reward per energy unit | 95% interval of the mean | per-world range |");
    println!("|---|---|---|---|");
    line("random domain", &rows, |r| r.random.per_budget());
    for (i, d) in DOMAINS.iter().enumerate() {
        line(&format!("always {}", d.name()), &rows, |r| r.always[i].per_budget());
    }
    line("oracle (hidden best domain)", &rows, |r| r.oracle.per_budget());
    line("learned chooser (UCB1)", &rows, |r| r.learned.per_budget());
    println!("\n| quantity | mean | 95% interval of the mean | range |");
    println!("|---|---|---|---|");
    line("regret per energy, learned vs oracle", &rows, |r| r.oracle.per_budget() - r.learned.per_budget());
    line("regret per energy, random vs oracle", &rows, |r| r.oracle.per_budget() - r.random.per_budget());
    line("total reward, learned (points)", &rows, |r| r.learned.reward as f64);
    line("total reward, random (points)", &rows, |r| r.random.reward as f64);
    line("share of picks on the best domain, steps 1-30", &rows, |r| share(&r.learned.picks, r.best, 0, 30));
    line("share of picks on the best domain, steps 31-80", &rows, |r| share(&r.learned.picks, r.best, 30, 80));
    line("share of picks on the best domain, step 81 on", &rows, |r| share(&r.learned.picks, r.best, 80, usize::MAX));
    line("share of picks on the best domain, random policy", &rows, |r| share(&r.random.picks, r.best, 0, usize::MAX));
    let conc: Vec<f64> = rows.iter().filter_map(|r| concentration_step(&r.learned.picks, r.best)).map(|x| x as f64).collect();
    let mut s = conc.clone();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("\nConcentration step (first step where the trailing 20 picks are >= 60% best domain): reached in {} of {} worlds, median {:?}", conc.len(), rows.len(), s.get(s.len() / 2));
    let beats = rows.iter().filter(|r| r.learned.per_budget() > r.random.per_budget()).count();
    println!("Learned beats random in {beats} of {} worlds.", rows.len());
    for (i, d) in DOMAINS.iter().enumerate() {
        let w = rows.iter().filter(|r| r.learned.per_budget() > r.always[i].per_budget()).count();
        println!("Learned beats always-{} in {w} of {} worlds.", d.name(), rows.len());
    }
    let steps: Vec<f64> = rows.iter().map(|r| r.learned.steps as f64).collect();
    println!("Steps per budget (learned): mean {:.1}", steps.iter().sum::<f64>() / steps.len() as f64);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|a| a == "tune").unwrap_or(false) {
        for c in [0.0, 0.01, 0.02, 0.05, 0.1, 0.2, 0.3, 0.5, 0.8, 1.0, 1.5] {
            let rows: Vec<SeedRow> = (101..=120).map(|s| seed_row(s, c, BUDGET)).collect();
            let (m, _, _) = col(&rows, |r| r.learned.per_budget());
            let (o, _, _) = col(&rows, |r| r.oracle.per_budget());
            println!("tuning seeds 101..120, c = {c}: learned {m:.3} of oracle {o:.3}");
        }
        return;
    }
    report("Tuning seeds 101..120", 101..=120, DEFAULT_EXPLORE_C);
    report("Report seeds 1..20", 1..=20, DEFAULT_EXPLORE_C);
    let _ = Domain::Arithmetic;
}
