//! Scores the baseline predictors on synthetic ground truth and prints the
//! tables recorded in docs/measured.md. Synthetic data only: no statement
//! about real-user prediction accuracy follows from these numbers.
//!   cargo run -p kore-self --example measure_predict --offline --release
//!   cargo run -p kore-self --example measure_predict --offline --release -- --tune
//! Report seeds 1..20; parameters are chosen on seeds 101..120 only (`--tune`).

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/learn/mod.rs"]
mod learn;
#[path = "../src/predict/mod.rs"]
mod predict;

use predict::backtest::{rolling_origin, Backtest};
use predict::baselines::{Config, KINDS};
use predict::scenarios::{self, Scenario};
use predict::score::{self, Point, Verdict};

const N: usize = 400;
const START: usize = 20;
const REPORT: std::ops::RangeInclusive<u64> = 1..=20;
const TUNE: std::ops::RangeInclusive<u64> = 101..=120;
const SCENARIOS: [Scenario; 3] = [Scenario::Context, Scenario::Noise, Scenario::Drift];

fn run(cfg: &Config, s: Scenario, seed: u64) -> Backtest {
    let obs = scenarios::generate(s, seed, N);
    rolling_origin(&predict::baselines::all(cfg), &obs, START)
}

fn median(v: &mut Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

fn mm(v: &[f64]) -> String {
    let mut s = v.to_vec();
    let (lo, hi) = (s.iter().cloned().fold(f64::INFINITY, f64::min), s.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    format!("{:.3} [{:.3}-{:.3}]", median(&mut s), lo, hi)
}

fn ci(c: &score::Ci) -> String {
    format!("{:+.4} [{:+.4}, {:+.4}]", c.mean, c.lo, c.hi)
}

fn tune() {
    println!("Tuning on seeds 101..120 (mean log loss of answered forecasts, averaged over the three scenarios)");
    let eval = |cfg: &Config, kind: &str| -> f64 {
        let mut tot = 0.0;
        for s in SCENARIOS {
            let mut v = Vec::new();
            for seed in TUNE {
                let bt = run(cfg, s, seed);
                let i = bt.index(kind).unwrap();
                v.push(score::summarize(&bt.answered(i)).log_loss);
            }
            tot += v.iter().sum::<f64>() / v.len() as f64;
        }
        tot / SCENARIOS.len() as f64
    };
    println!("| predictor | parameter | value | mean log loss |\n|---|---|---|---|");
    for a in [0.1, 0.25, 0.5, 1.0, 2.0] {
        println!("| frequency | alpha | {a} | {:.4} |", eval(&Config { alpha: a, ..Config::default() }, "frequency"));
    }
    for h in [5.0, 10.0, 20.0, 30.0, 50.0, 100.0] {
        println!("| recency | half_life | {h} | {:.4} |", eval(&Config { half_life: h, ..Config::default() }, "recency"));
    }
    for e in [0.05, 0.1, 0.2, 0.3, 0.5, 0.7] {
        println!("| last-choice | last_eps | {e} | {:.4} |", eval(&Config { last_eps: e, ..Config::default() }, "last-choice"));
    }
    for m in [0.5, 1.0, 2.0, 5.0, 10.0, 20.0] {
        println!("| context-count | ctx_m | {m} | {:.4} |", eval(&Config { ctx_m: m, ..Config::default() }, "context-count"));
    }
}

fn report(cfg: &Config) {
    println!("Config: {cfg:?}");
    println!("N = {N} observations per run, forecasts from origin {START} (so {} per run), seeds 1..20\n", N - START);
    for s in SCENARIOS {
        println!("### {}\n", s.name());
        let bts: Vec<Backtest> = REPORT.map(|seed| run(cfg, s, seed)).collect();
        println!("| predictor | coverage | Brier | log loss | accuracy | ECE |\n|---|---|---|---|---|---|");
        for (i, k) in KINDS.iter().enumerate() {
            let sums: Vec<score::Summary> = bts.iter().map(|b| score::summarize(&b.answered(i))).collect();
            let cov: Vec<f64> = bts.iter().map(|b| b.coverage(i)).collect();
            println!(
                "| {} | {:.3} | {} | {} | {} | {} |",
                k.id(),
                cov.iter().sum::<f64>() / cov.len() as f64,
                mm(&sums.iter().map(|x| x.brier).collect::<Vec<_>>()),
                mm(&sums.iter().map(|x| x.log_loss).collect::<Vec<_>>()),
                mm(&sums.iter().map(|x| x.accuracy).collect::<Vec<_>>()),
                mm(&sums.iter().map(|x| x.ece).collect::<Vec<_>>()),
            );
        }
        println!("\nPaired comparison on forecasts both predictors made: gain = baseline Brier minus predictor Brier (positive = predictor better), mean over 20 seeds with a 95% bootstrap interval over the seeds, then the per-seed verdicts (block bootstrap, 95%).\n");
        println!("| predictor vs baseline | mean Brier gain [95% over seeds] | mean log-loss gain [95% over seeds] | seeds better | seeds worse | seeds no difference |\n|---|---|---|---|---|---|");
        for (i, k) in KINDS.iter().enumerate() {
            for (j, b) in KINDS.iter().enumerate() {
                if i == j || *b == predict::baselines::Kind::LastChoice || *b == predict::baselines::Kind::Recency {
                    continue;
                }
                let (mut gb, mut gl) = (Vec::new(), Vec::new());
                let (mut better, mut worse, mut none) = (0, 0, 0);
                for (si, bt) in bts.iter().enumerate() {
                    let (pp, bp) = bt.common(i, j);
                    if pp.is_empty() {
                        continue;
                    }
                    let c = score::paired(&pp, &bp, 1000 + si as u64);
                    gb.push(c.d_brier.mean);
                    gl.push(c.d_log_loss.mean);
                    match c.verdict {
                        Verdict::Better => better += 1,
                        Verdict::Worse => worse += 1,
                        _ => none += 1,
                    }
                }
                println!("| {} vs {} | {} | {} | {better} | {worse} | {none} |", k.id(), b.id(), ci(&score::mean_ci(&gb, 7)), ci(&score::mean_ci(&gl, 8)));
            }
        }
        // recency vs frequency is the drift question; list it for every scenario for symmetry
        {
            let (i, j) = (bts[0].index("recency").unwrap(), bts[0].index("frequency").unwrap());
            let (mut gb, mut better, mut worse, mut none) = (Vec::new(), 0, 0, 0);
            for (si, bt) in bts.iter().enumerate() {
                let (pp, bp) = bt.common(i, j);
                let c = score::paired(&pp, &bp, 2000 + si as u64);
                gb.push(c.d_brier.mean);
                match c.verdict {
                    Verdict::Better => better += 1,
                    Verdict::Worse => worse += 1,
                    _ => none += 1,
                }
            }
            println!("| recency vs frequency | {} | | {better} | {worse} | {none} |", ci(&score::mean_ci(&gb, 9)));
        }
        println!("\nCalibration (top label, forecasts pooled over the 20 seeds, 10 bins):\n");
        for id in ["frequency", "context-count"] {
            let i = bts[0].index(id).unwrap();
            let pooled: Vec<Point> = bts.iter().flat_map(|b| b.answered(i)).collect();
            let bins = score::calibration(&pooled, score::BINS);
            println!("{id}: n = {}, pooled ECE = {:.4}", pooled.len(), score::ece(&bins, pooled.len()));
            println!("| bin | n | mean confidence | hit rate |\n|---|---|---|---|");
            for b in bins.iter().filter(|b| b.n > 0) {
                println!("| {:.1}-{:.1} | {} | {:.3} | {:.3} |", b.lo, b.hi, b.n, b.mean_conf, b.accuracy);
            }
            println!();
        }
    }
}

fn main() {
    if std::env::args().any(|a| a == "--tune") {
        tune();
    } else {
        report(&Config::default());
    }
}
