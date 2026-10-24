//! Scores the deterministic learners against synthetic ground truth and prints
//! the tables recorded in docs/measured.md. Synthetic-data numbers only.
//!   cargo run -p kore-self --example measure_learning --offline --release
//! Tagged lives first, then free-text lives read through keyword rules.

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

fn table(rows: &[Row], free: bool) {
    let extra = if free { " habit precision, planted contexts only | built-in-context habits per life |" } else { "" };
    println!("| noise | habit precision | habit recall |{extra} change precision | change recall | change abs error (days) | change delay (days) | change FP rate | stance precision | stance recall | stance key accuracy | min-n refused | would derive at min_n=1 |");
    println!("|---|---|---|{}---|---|---|---|---|---|---|---|---|---|", if free { "---|---|" } else { "" });
    for r in rows {
        let Row { noise, .. } = r;
        let x = if free { format!(" {} | {} |", fmt(r.col(|m| Some(m.habit_precision_planted))), fmt(r.col(|m| Some(m.habit_builtin_detected as f64)))) } else { String::new() };
        println!(
            "| {:.0}% | {} | {} |{x} {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
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
    for r in rows {
        let g = |f: fn(&measure::RunMetrics) -> Option<f64>| r.col(f).map(|a| a.mean).unwrap_or(f64::NAN);
        println!(
            "noise {:.0}%: means over {} seeds: habit precision {:.3}, habit recall {:.3}, change precision {:.3}, change FP rate {:.3} (5 unchanged topics per seed), change recall {:.3}, change delay {:.1} days",
            r.noise * 100.0,
            r.runs.len(),
            g(|m| Some(m.habit_precision)),
            g(|m| Some(m.habit_recall)),
            g(|m| Some(m.change_precision)),
            g(|m| Some(m.change_fp_rate)),
            g(|m| Some(m.change_recall)),
            g(|m| m.change_delay),
        );
    }
}

fn main() {
    let p = learn::Params::default();
    println!("params: {}", p.canonical());
    let seeds: Vec<u64> = if std::env::args().any(|a| a == "tune") { (101..=120).collect() } else { measure::SEEDS.to_vec() };
    println!("seeds: {:?}; each cell is median [min-max] over seeds\n", seeds);
    println!("## tagged lives (ctx:/act:/topic: tags)\n");
    table(&measure::sweep(&seeds, &measure::NOISE, &p), false);
    println!("\n## free-text lives (no tags; keyword rules from synth::free_text_rules)\n");
    table(&measure::sweep_free(&seeds, &measure::NOISE, &p), true);
}
