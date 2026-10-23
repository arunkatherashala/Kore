//! Scores the deterministic learners against synthetic ground truth and prints
//! the table recorded in docs/measured.md. Synthetic-data numbers only.
//!   cargo run -p kore-self --example measure_learning --offline --release

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/learn/mod.rs"]
mod learn;
#[path = "../src/synth.rs"]
mod synth;
#[path = "../src/measure.rs"]
mod measure;

use measure::{Agg, Row};

fn fmt(a: Option<Agg>) -> String {
    match a {
        Some(a) => format!("{:.2} [{:.2}-{:.2}]", a.median, a.min, a.max),
        None => "n/a".into(),
    }
}

fn main() {
    let p = learn::Params::default();
    let rows = measure::sweep(&measure::SEEDS, &measure::NOISE, &p);
    println!("params: {}", p.canonical());
    println!("seeds: {:?}; each cell is median [min-max] over seeds\n", measure::SEEDS);
    println!("| noise | habit precision | habit recall | change precision | change recall | change abs error (days) | change delay (days) | change FP rate | stance precision | stance recall | stance key accuracy | min-n refused | would derive at min_n=1 |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for r in &rows {
        let Row { noise, .. } = r;
        println!(
            "| {:.0}% | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            noise * 100.0,
            fmt(r.col(|m| Some(m.habit_precision))),
            fmt(r.col(|m| Some(m.habit_recall))),
            fmt(r.col(|m| Some(m.change_precision))),
            fmt(r.col(|m| Some(m.change_recall))),
            fmt(r.col(|m| Some(m.change_abs_error))),
            fmt(r.col(|m| m.change_delay)),
            fmt(r.col(|m| Some(m.change_fp_rate))),
            fmt(r.col(|m| Some(m.stance_precision))),
            fmt(r.col(|m| Some(m.stance_recall))),
            fmt(r.col(|m| Some(m.stance_key_accuracy))),
            fmt(r.col(|m| Some(m.minn_refused))),
            fmt(r.col(|m| Some(m.minn1_would_derive))),
        );
    }
    println!();
    for r in &rows {
        let g = |f: fn(&measure::RunMetrics) -> Option<f64>| r.col(f).map(|a| a.mean).unwrap_or(f64::NAN);
        println!(
            "noise {:.0}%: means over {} seeds: habit recall {:.3}, change FP rate {:.3} (5 unchanged topics per seed), change recall {:.3}, change delay {:.1} days",
            r.noise * 100.0,
            r.runs.len(),
            g(|m| Some(m.habit_recall)),
            g(|m| Some(m.change_fp_rate)),
            g(|m| Some(m.change_recall)),
            g(|m| m.change_delay),
        );
    }
}
