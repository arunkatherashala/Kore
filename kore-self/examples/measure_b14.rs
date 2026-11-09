//! Track B14 measurements (synthetic first world, author-defined families and rewards; docs/measured.md, Track B14).
//!   cargo run -p kore-self --example measure_b14 --offline --release -- diag --seeds 101..120 --steps 200000 --window 10000
//! Run data is printed (and optionally written with --out <file>) and never committed.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::b14::measure::{oracle_book, renewal_rate, run_b14, OutB14, RunCfgB14, FAMILIES};
use being::measure::mean_ci;
use being::measure_b11::genesis;
use being::run_loop::world_for;
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

/// Reference rates per world: (renewal oracle without rules, with the oracle book).
fn references(ss: &[u64]) -> Vec<(u64, f64, f64)> {
    ss.par_iter()
        .map(|&s| {
            let w = world_for(&genesis(s, 3000));
            let book = oracle_book(&w, 60);
            (s, renewal_rate(s, 30_000, &[], 4), renewal_rate(s, 30_000, &book, 4))
        })
        .collect()
}

fn cmd_diag(args: &[String]) {
    let ss = seeds(args, (101, 120));
    let steps: u64 = arg(args, "--steps").map_or(200_000, |x| x.parse().unwrap());
    let window: u64 = arg(args, "--window").map_or(10_000, |x| x.parse().unwrap());
    let t0 = std::time::Instant::now();
    let refs = references(&ss);
    eprintln!("references: {:.0} s", t0.elapsed().as_secs_f64());
    let outs: Vec<(u64, OutB14)> = ss.par_iter().map(|&s| (s, run_b14(&RunCfgB14 { cover_n: 40, ..RunCfgB14::base(s, steps, window) }))).collect();
    eprintln!("runs: {:.0} s", t0.elapsed().as_secs_f64());
    for (s, o) in &outs {
        let (_, r0, rb) = refs.iter().find(|r| r.0 == *s).unwrap();
        println!("\n#### world {s}: renewal oracle without rules {r0:.3}, with the oracle book {rb:.3} (book gain {:+.3})\n", rb - r0);
        println!("| steps | reward per energy | regret vs oracle with book | attempt share | pattern attempts per 1000 | rule answers per 1000 (right) | accepted | retired | live | families covered (>= 0.9) |");
        println!("|---|---|---|---|---|---|---|---|---|---|");
        for w in &o.wins {
            let n = w.steps as f64;
            let cov = w.cover.iter().filter(|c| **c >= 0.9).count();
            println!(
                "| {}k-{}k | {:.3} | {:+.3} | {:.3} | {:.1} | {:.1} ({:.3}) | {} | {} | {} | {cov} |",
                w.from / 1000, (w.from + w.steps) / 1000, w.rpe(), rb - w.rpe(), w.attempts as f64 / n, 1000.0 * w.pattern_attempts as f64 / n,
                1000.0 * w.rule_answers as f64 / n, w.rule_right as f64 / w.rule_answers.max(1) as f64, w.accepted, w.retired, w.live_end
            );
        }
        if let Some(last) = o.wins.last() {
            let missing: Vec<String> = FAMILIES.iter().zip(&last.cover).filter(|(_, c)| **c < 0.9).map(|(k, c)| format!("{k:?} {c:.2}")).collect();
            println!("\nnot covered at the end: {}", missing.join(", "));
            println!("examples per difficulty 1..5: {:?}; pattern attempts per difficulty [fast, careful, deep, discovered]: {:?}", &o.ex_per_sig[1..], &o.pat_strat[1..]);
            println!("rules: {}", o.rules_final.iter().map(|r| format!("d{} {}{}{}", r.sig, r.xf.code(), r.form.code(), if r.retired { " (retired)" } else { "" })).collect::<Vec<_>>().join(", "));
        }
    }
    // pooled curve
    println!("\n#### Pooled over worlds (n = {}): reward per energy and regret per window\n", ss.len());
    println!("| steps | reward per energy | regret vs oracle with book | live rules | families covered |");
    println!("|---|---|---|---|---|");
    let nw = outs.iter().map(|o| o.1.wins.len()).min().unwrap_or(0);
    for i in 0..nw {
        let r: Vec<f64> = outs.iter().map(|o| o.1.wins[i].rpe()).collect();
        let g: Vec<f64> = outs.iter().map(|o| refs.iter().find(|x| x.0 == o.0).unwrap().2 - o.1.wins[i].rpe()).collect();
        let l: Vec<f64> = outs.iter().map(|o| o.1.wins[i].live_end as f64).collect();
        let c: Vec<f64> = outs.iter().map(|o| o.1.wins[i].cover.iter().filter(|c| **c >= 0.9).count() as f64).collect();
        let w0 = &outs[0].1.wins[i];
        println!("| {}k-{}k | {} | {} | {:.1} | {:.1} |", w0.from / 1000, (w0.from + w0.steps) / 1000, ci(&r), ci(&g), l.iter().sum::<f64>() / l.len() as f64, c.iter().sum::<f64>() / c.len() as f64);
    }
    let gains: Vec<f64> = refs.iter().map(|r| r.2 - r.1).collect();
    println!("\nbook gain per world (oracle with book minus without): {}", gains.iter().map(|g| format!("{g:+.3}")).collect::<Vec<_>>().join(" "));
}

/// Two-sided 95% t quantile for n - 1 degrees of freedom.
fn tq(n: usize) -> f64 {
    const T: [f64; 30] = [12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160, 2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056, 2.052, 2.048, 2.045, 2.042];
    if n < 2 { f64::NAN } else if n - 1 <= 30 { T[n - 2] } else { 1.96 }
}

fn ci_n(xs: &[f64]) -> String {
    let n = xs.len();
    if n == 0 {
        return "-".into();
    }
    let m = xs.iter().sum::<f64>() / n as f64;
    let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n as f64 - 1.0).max(1.0);
    let h = tq(n) * (var / n as f64).sqrt();
    format!("{m:+.3} [{:+.3}, {:+.3}]", m - h, m + h)
}

/// "mean [CI] (wins/ties/losses)" of paired differences.
fn paired(d: &[f64]) -> String {
    let w = d.iter().filter(|x| **x > 1e-12).count();
    let t = d.iter().filter(|x| x.abs() <= 1e-12).count();
    format!("{} ({w}/{t}/{} of {})", ci_n(d), d.len() - w - t, d.len())
}

/// An arm: `base`, `lib`, `cur<pm>[:<ei_w>]`, `lib+cur<pm>[:<ei_w>]`, optional suffix `@settled`.
fn arm_cfg(spec: &str, s: u64, steps: u64, window: u64) -> RunCfgB14 {
    use being::b14::curiosity::{CurCfg, DEFAULT_CUR};
    use being::b14::library::LibCfg;
    let (spec, solved) = match spec.strip_suffix("@solved") {
        Some(x) => (x, true),
        None => (spec, false),
    };
    let (spec, settled) = match spec.strip_suffix("@settled") {
        Some(x) => (x, true),
        None => (spec, false),
    };
    let mut c = RunCfgB14::base(s, steps, window);
    if solved {
        c.preload = oracle_book(&world_for(&genesis(s, 3000)), 60);
    }
    if settled {
        c.skip_rule = being::planner::SkipRule::Settled(being::planner::DEFAULT_SETTLE_K);
    }
    for part in spec.split('+') {
        if part == "base" {
        } else if part == "lib" {
            c.library = Some(LibCfg::default());
        } else if part == "libnoest" {
            c.library = Some(LibCfg { estimate: false, ..LibCfg::default() });
        } else if let Some(x) = part.strip_prefix("cur") {
            let mut it = x.split(':');
            let pm: u32 = it.next().unwrap().parse().expect("cur<pm>");
            let ei: f64 = it.next().map_or(DEFAULT_CUR.ei_w, |v| v.parse().unwrap());
            c.curiosity = Some(CurCfg { pm, ei_w: ei, ..DEFAULT_CUR });
        } else {
            panic!("unknown arm part {part}");
        }
    }
    c
}

fn post(o: &OutB14, from: u64) -> f64 {
    let (r, c) = o.wins.iter().filter(|w| w.from >= from).fold((0i64, 0i64), |a, w| (a.0 + w.reward, a.1 + w.cost));
    if c > 0 { r as f64 / c as f64 } else { 0.0 }
}

/// OLS slope of window reward per energy against the window midpoint, per 100k steps, over windows from `from`.
fn slope(o: &OutB14, from: u64) -> f64 {
    let pts: Vec<(f64, f64)> = o.wins.iter().filter(|w| w.from >= from && w.steps > 0).map(|w| ((w.from as f64 + w.steps as f64 / 2.0) / 100_000.0, w.rpe())).collect();
    let n = pts.len() as f64;
    if n < 2.0 {
        return 0.0;
    }
    let (mx, my) = (pts.iter().map(|p| p.0).sum::<f64>() / n, pts.iter().map(|p| p.1).sum::<f64>() / n);
    let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    sxy / sxx
}

fn cmd_arms(args: &[String]) {
    let ss = seeds(args, (101, 120));
    let steps: u64 = arg(args, "--steps").map_or(100_000, |x| x.parse().unwrap());
    let window: u64 = arg(args, "--window").map_or(10_000, |x| x.parse().unwrap());
    let from: u64 = arg(args, "--post").map_or(50_000, |x| x.parse().unwrap());
    let arms: Vec<String> = arg(args, "--arms").unwrap_or("base,lib,cur20,lib+cur20".into()).split(',').map(|x| x.to_string()).collect();
    let t0 = std::time::Instant::now();
    let refs = references(&ss);
    let jobs: Vec<(u64, usize)> = ss.iter().flat_map(|&s| (0..arms.len()).map(move |a| (s, a))).collect();
    let res: Vec<((u64, usize), OutB14)> = jobs.par_iter().map(|&(s, a)| ((s, a), run_b14(&RunCfgB14 { cover_n: 40, ..arm_cfg(&arms[a], s, steps, window) }))).collect();
    eprintln!("arms: {:.0} s", t0.elapsed().as_secs_f64());
    let get = |s: u64, a: usize| &res.iter().find(|r| r.0 == (s, a)).unwrap().1;
    let gain = |s: u64| refs.iter().find(|r| r.0 == s).map_or(0.0, |r| r.2 - r.1);
    let rule_w: Vec<u64> = ss.iter().copied().filter(|&s| gain(s) >= 0.1).collect();
    let simple_w: Vec<u64> = ss.iter().copied().filter(|&s| gain(s) < 0.1).collect();
    println!("seeds {}..{}, {steps} steps, windows of {window}, post-plateau part from step {from}; rule worlds (oracle book gain >= 0.1): {:?}; simple worlds: {:?}\n", ss[0], ss[ss.len() - 1], rule_w, simple_w);
    // curves
    println!("| steps | {} |", arms.join(" | "));
    println!("|---|{}", "---|".repeat(arms.len()));
    let nw = res.iter().map(|r| r.1.wins.len()).min().unwrap_or(0);
    for i in 0..nw {
        let w0 = &get(ss[0], 0).wins[i];
        let cells: Vec<String> = (0..arms.len()).map(|a| {
            let xs: Vec<f64> = ss.iter().map(|&s| get(s, a).wins[i].rpe()).collect();
            let (m, lo, hi) = mean_ci(&xs);
            format!("{m:.3} [{lo:.3}, {hi:.3}]")
        }).collect();
        println!("| {}k-{}k | {} |", w0.from / 1000, (w0.from + w0.steps) / 1000, cells.join(" | "));
    }
    println!("\n| arm | post-plateau reward per energy | minus {} (all) | rule worlds | simple worlds | slope per 100k steps after {}k | slope minus {} | regret vs oracle with book (post) | curiosity energy share | rules live (end) | families covered (end) | library reuses |", arms[0], from / 1000, arms[0]);
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
    for a in 0..arms.len() {
        let p: Vec<f64> = ss.iter().map(|&s| post(get(s, a), from)).collect();
        let d = |ws: &[u64]| -> Vec<f64> { ws.iter().map(|&s| post(get(s, a), from) - post(get(s, 0), from)).collect() };
        let sl: Vec<f64> = ss.iter().map(|&s| slope(get(s, a), from)).collect();
        let sld: Vec<f64> = ss.iter().map(|&s| slope(get(s, a), from) - slope(get(s, 0), from)).collect();
        let rg: Vec<f64> = ss.iter().map(|&s| refs.iter().find(|r| r.0 == s).unwrap().2 - post(get(s, a), from)).collect();
        let share: Vec<f64> = ss.iter().map(|&s| get(s, a).cur_energy as f64 / get(s, a).total_cost.max(1) as f64).collect();
        let live = ss.iter().map(|&s| get(s, a).wins.last().unwrap().live_end as f64).sum::<f64>() / ss.len() as f64;
        let cov = ss.iter().map(|&s| get(s, a).wins.last().unwrap().cover.iter().filter(|c| **c >= 0.9).count() as f64).sum::<f64>() / ss.len() as f64;
        let reuse = ss.iter().map(|&s| get(s, a).wins.iter().map(|w| w.lib_reused).sum::<u64>() as f64).sum::<f64>() / ss.len() as f64;
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {:.4} | {live:.1} | {cov:.1} | {reuse:.1} |",
            arms[a], ci_n(&p), paired(&d(&ss)), paired(&d(&rule_w)), paired(&d(&simple_w)), ci_n(&sl), paired(&sld), ci_n(&rg), share.iter().sum::<f64>() / share.len() as f64
        );
    }
    // relative difference (for the cost criterion)
    println!("\nRelative post-plateau difference (arm / {} - 1), all worlds:", arms[0]);
    for a in 1..arms.len() {
        let d: Vec<f64> = ss.iter().map(|&s| post(get(s, a), from) / post(get(s, 0), from) - 1.0).collect();
        println!("  {}: {}", arms[a], paired(&d));
    }
    if let Some(path) = arg(args, "--per-world") {
        let mut txt = String::new();
        for &s in &ss {
            txt.push_str(&format!("{s} gain {:.3}", gain(s)));
            for a in 0..arms.len() {
                txt.push_str(&format!(" {}={:.4}", arms[a], post(get(s, a), from)));
            }
            txt.push('\n');
        }
        std::fs::write(path, txt).unwrap();
    }
}

/// Transfer to a new world: a source life (B14 on, world `s + offset`) builds a library; the target life
/// in world `s` runs with that library imported vs without (both with B14 on), and the base being.
/// Steps to competence per hidden family = the first checkpoint (every `--every` steps) at which the
/// rule book answers at least 90% of fresh tasks of the family right; not reached = the horizon.
fn cmd_transfer(args: &[String]) {
    let ss = seeds(args, (101, 120));
    let steps: u64 = arg(args, "--steps").map_or(30_000, |x| x.parse().unwrap());
    let every: u64 = arg(args, "--every").map_or(1000, |x| x.parse().unwrap());
    let src_steps: u64 = arg(args, "--source-steps").map_or(50_000, |x| x.parse().unwrap());
    let offset: u64 = arg(args, "--offset").map_or(500, |x| x.parse().unwrap());
    let arm: String = arg(args, "--arm").unwrap_or("lib+cur20".into());
    let t0 = std::time::Instant::now();
    let rows: Vec<(u64, usize, Vec<OutB14>)> = ss
        .par_iter()
        .map(|&s| {
            let src = run_b14(&arm_cfg(&arm, s + offset, src_steps, src_steps));
            let lib = src.library.clone();
            let mk = |a: &str, imp: bool| {
                let mut c = RunCfgB14 { cover_n: 40, ..arm_cfg(a, s, steps, every) };
                if imp {
                    c.import = lib.clone();
                }
                run_b14(&c)
            };
            (s, lib.len(), vec![mk(&arm, true), mk(&arm, false), mk("base", false)])
        })
        .collect();
    eprintln!("transfer: {:.0} s", t0.elapsed().as_secs_f64());
    let first = |o: &OutB14, f: usize| -> f64 { o.wins.iter().find(|w| w.cover.get(f).map_or(false, |c| *c >= 0.9)).map_or(steps as f64, |w| (w.from + w.steps) as f64) };
    let names = ["with library", "without library", "base"];
    println!("seeds {}..{}, target {steps} steps (checkpoints every {every}), source life {src_steps} steps in world seed + {offset}, arm {arm}\n", ss[0], ss[ss.len() - 1]);
    println!("| world | entries imported | families competent at the end (with / without / base) | mean steps to competence over all 14 families (with / without / base) | reused from the library |");
    println!("|---|---|---|---|---|");
    let mut d_steps = vec![];
    let mut d_steps_base = vec![];
    let mut d_r10 = vec![];
    let mut d_rall = vec![];
    for (s, n, o) in &rows {
        let comp: Vec<usize> = o.iter().map(|x| x.wins.last().unwrap().cover.iter().filter(|c| **c >= 0.9).count()).collect();
        let mean: Vec<f64> = o.iter().map(|x| (0..FAMILIES.len()).map(|f| first(x, f)).sum::<f64>() / FAMILIES.len() as f64).collect();
        let reused: u64 = o[0].wins.iter().map(|w| w.lib_reused).sum();
        println!("| {s} | {n} | {} / {} / {} | {:.0} / {:.0} / {:.0} | {reused} |", comp[0], comp[1], comp[2], mean[0], mean[1], mean[2]);
        d_steps.push(mean[1] - mean[0]);
        d_steps_base.push(mean[2] - mean[1]);
        let rp = |x: &OutB14, upto: u64| {
            let (r, c) = x.wins.iter().filter(|w| w.from < upto).fold((0i64, 0i64), |a, w| (a.0 + w.reward, a.1 + w.cost));
            r as f64 / c.max(1) as f64
        };
        d_r10.push(rp(&o[0], 10_000) - rp(&o[1], 10_000));
        d_rall.push(rp(&o[0], steps) - rp(&o[1], steps));
    }
    println!("\n{} vs {}: steps to competence SAVED per family (positive = faster) {}; reward per energy difference in the first 10k steps {}; over the {steps} steps {}", names[0], names[1], paired(&d_steps), paired(&d_r10), paired(&d_rall));
    println!("{} vs {}: steps to competence SAVED per family {}", names[1], names[2], paired(&d_steps_base));
}

/// Short lives without refills (the budgets of the B1 to V1 tables): B14 on vs off.
fn cmd_short(args: &[String]) {
    let ss = seeds(args, (101, 120));
    let arms: Vec<String> = arg(args, "--arms").unwrap_or("base,lib+cur20".into()).split(',').map(|x| x.to_string()).collect();
    for budget in [600i64, 3000] {
        let res: Vec<Vec<f64>> = (0..arms.len())
            .map(|a| ss.par_iter().map(|&s| { let mut c = arm_cfg(&arms[a], s, 1_000_000, 1_000_000); c.budget = budget; c.refill = false; let o = run_b14(&c); o.wins.iter().map(|w| w.reward).sum::<i64>() as f64 / o.total_cost.max(1) as f64 }).collect())
            .collect();
        for a in 1..arms.len() {
            let d: Vec<f64> = res[a].iter().zip(&res[0]).map(|(x, y)| x - y).collect();
            println!("budget {budget}: {} {} vs {} {}: {}", arms[a], ci_n(&res[a]), arms[0], ci_n(&res[0]), paired(&d));
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("diag") => cmd_diag(&args[1..]),
        Some("arms") => cmd_arms(&args[1..]),
        Some("transfer") => cmd_transfer(&args[1..]),
        Some("short") => cmd_short(&args[1..]),
        _ => eprintln!("usage: measure_b14 diag [--seeds a..b] [--steps N] [--window N]"),
    }
}
