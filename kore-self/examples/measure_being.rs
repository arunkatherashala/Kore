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

fn mrow(rows: &[MenuRow], f: impl Fn(&MenuRow) -> f64) -> (f64, f64, f64) {
    mean_ci(&rows.iter().map(f).collect::<Vec<_>>())
}

fn menu_tune() {
    for budget in [BUDGET, LONG_BUDGET] {
        let base: Vec<MenuRow> = (101..=120).map(|s| menu_row(s, budget, 0.0, 0.0)).collect();
        println!("budget {budget}: random {:.3} greedy {:.3} ucb {:.3} oracle {:.3} (tuning seeds 101..120)", mrow(&base, |r| r.random.per_budget()).0, mrow(&base, |r| r.greedy.per_budget()).0, mrow(&base, |r| r.ucb.per_budget()).0, mrow(&base, |r| r.oracle.per_budget()).0);
        for beta in [1.0, 1.5, 2.0, 3.0] {
            for skip in [0.0, 0.6, 0.8, 1.0, 1.2] {
                let rows: Vec<MenuRow> = (101..=120).map(|s| menu_row(s, budget, beta, skip)).collect();
                println!("  beta {beta} skip_frac {skip}: model {:.3}", mrow(&rows, |r| r.model.per_budget()).0);
            }
        }
    }
}

fn mline(name: &str, rows: &[MenuRow], f: impl Fn(&MenuRow) -> f64) {
    let (m, lo, hi) = mrow(rows, &f);
    println!("| {name} | {m:.3} | [{lo:.3}, {hi:.3}] |");
}

fn menu_report(label: &str, seeds: std::ops::RangeInclusive<u64>, budget: i64, beta: f64, skip: f64) {
    let rows: Vec<MenuRow> = seeds.map(|s| menu_row(s, budget, beta, skip)).collect();
    println!("
### Menu world, {label} (budget {budget}, beta {beta}, skip_frac {skip}, n = {} worlds)
", rows.len());
    println!("| policy | mean reward per energy unit | 95% interval of the mean |");
    println!("|---|---|---|");
    mline("random (offer and strategy)", &rows, |r| r.random.per_budget());
    mline("greedy by listed reward per cost (careful)", &rows, |r| r.greedy.per_budget());
    mline("greedy by listed reward per cost (fast always)", &rows, |r| r.greedy_fast.per_budget());
    mline("greedy by listed reward per cost (deep always)", &rows, |r| r.greedy_deep.per_budget());
    mline("UCB1 domain chooser (careful)", &rows, |r| r.ucb.per_budget());
    mline("model learner, never skips", &rows, |r| r.model_noskip.per_budget());
    mline("model learner (with skipping)", &rows, |r| r.model.per_budget());
    mline("oracle (knows hidden success rates and energy)", &rows, |r| r.oracle.per_budget());
    println!("
| regret per energy vs oracle | mean | 95% interval of the mean |");
    println!("|---|---|---|");
    mline("random", &rows, |r| r.oracle.per_budget() - r.random.per_budget());
    mline("greedy", &rows, |r| r.oracle.per_budget() - r.greedy.per_budget());
    mline("greedy fast", &rows, |r| r.oracle.per_budget() - r.greedy_fast.per_budget());
    mline("greedy deep", &rows, |r| r.oracle.per_budget() - r.greedy_deep.per_budget());
    mline("UCB1", &rows, |r| r.oracle.per_budget() - r.ucb.per_budget());
    mline("model learner, never skips", &rows, |r| r.oracle.per_budget() - r.model_noskip.per_budget());
    mline("model learner (with skipping)", &rows, |r| r.oracle.per_budget() - r.model.per_budget());
    println!("
| paired difference (model minus other), reward per energy | mean | 95% interval of the mean | model wins |");
    println!("|---|---|---|---|");
    for (name, f) in [("model never skips", (|r: &MenuRow| r.model_noskip.per_budget()) as fn(&MenuRow) -> f64), ("random", (|r: &MenuRow| r.random.per_budget()) as fn(&MenuRow) -> f64), ("greedy", |r| r.greedy.per_budget()), ("greedy fast", |r| r.greedy_fast.per_budget()), ("greedy deep", |r| r.greedy_deep.per_budget()), ("UCB1", |r| r.ucb.per_budget())] {
        let (m, lo, hi) = mrow(&rows, |r| r.model.per_budget() - f(r));
        let w = rows.iter().filter(|r| r.model.per_budget() > f(r)).count();
        println!("| {name} | {m:+.3} | [{lo:+.3}, {hi:+.3}] | {w} of {} |", rows.len());
    }
    println!("
Learning curve, reward per energy by step window (mean over worlds):
");
    println!("| steps | random | greedy | UCB1 | model | oracle |");
    println!("|---|---|---|---|---|---|");
    let wins: Vec<(usize, usize)> = if budget > 1000 { vec![(0, 25), (25, 50), (50, 100), (100, 200), (200, 400), (400, 100000)] } else { vec![(0, 15), (15, 30), (30, 50), (50, 100000)] };
    for (a, b) in wins {
        let w = |f: fn(&MenuRow) -> &MenuRun| mrow(&rows, |r| f(r).window(a, b)).0;
        let b_txt = if b >= 100000 { "end".to_string() } else { b.to_string() };
        println!("| {}-{} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} |", a + 1, b_txt, w(|r| &r.random), w(|r| &r.greedy), w(|r| &r.ucb), w(|r| &r.model), w(|r| &r.oracle));
    }
    let n = rows.len() as f64;
    let st: Vec<f64> = (0..3).map(|i| rows.iter().map(|r| r.model.strategies[i] as f64).sum::<f64>() / n).collect();
    println!("
Model learner per world: {:.1} fast / {:.1} careful / {:.1} deep attempts, {:.1} skips, {:.1} attempts in total", st[0], st[1], st[2], rows.iter().map(|r| r.model.skips as f64).sum::<f64>() / n, st.iter().sum::<f64>());
    let runs: Vec<&MenuRun> = rows.iter().map(|r| &r.model).collect();
    let cal = calibration(&runs, 10);
    println!("
Calibration of the model's predicted success (all attempts after the 10th of each run; each prediction is made before its outcome is seen, so every point is out of sample), n = {} attempts:
", cal.n);
    println!("| predicted bin | attempts | mean predicted | observed rate |");
    println!("|---|---|---|---|");
    for (lo, c, p, o) in cal.bins.iter().filter(|b| b.1 > 0) {
        println!("| {:.1}-{:.1} | {c} | {p:.3} | {o:.3} |", lo, lo + 0.1);
    }
    println!("
Brier score of the model: {:.4}; Brier of predicting the running overall success rate: {:.4} (lower is better)", cal.brier, cal.brier_base_rate);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|a| a == "tune-menu").unwrap_or(false) {
        menu_tune();
        return;
    }
    if args.get(1).map(|a| a == "menu").unwrap_or(false) {
        use being::planner::{DEFAULT_BETA, DEFAULT_SKIP_FRAC};
        for budget in [BUDGET, LONG_BUDGET] {
            menu_report("tuning seeds 101..120", 101..=120, budget, DEFAULT_BETA, DEFAULT_SKIP_FRAC);
            menu_report("report seeds 1..20", 1..=20, budget, DEFAULT_BETA, DEFAULT_SKIP_FRAC);
        }
        return;
    }
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
