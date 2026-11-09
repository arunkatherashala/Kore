//! Track E1 measurements (synthetic first world, author-defined families and rewards; docs/measured.md, Track E1).
//!   cargo run -p kore-self --example measure_e1 --offline --release -- diag --seeds 101..104 --steps 50000
//!   cargo run -p kore-self --example measure_e1 --offline --release -- arms --seeds 1..20 --steps 200000 --arms b14,b14+e1
//! Run data is printed (and optionally written with --per-world <file>) and never committed.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::b14::curiosity::{CurCfg, DEFAULT_CUR};
use being::b14::library::LibCfg;
use being::b14::measure::{oracle_book, renewal_rate, RunCfgB14, FAMILIES};
use being::e1::evolve::{EvoCfg, DEFAULT_EVO, EVALS_PER_ENERGY};
use being::e1::measure::{all_families, fresh_check, run_e1, OutE1, RunCfgE1};
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

fn paired(d: &[f64]) -> String {
    let w = d.iter().filter(|x| **x > 1e-12).count();
    let t = d.iter().filter(|x| x.abs() <= 1e-12).count();
    format!("{} ({w}/{t}/{} of {})", ci_n(d), d.len() - w - t, d.len())
}

/// An arm: parts joined by `+`: `base`, `lib`, `cur<pm>`, `b14` (= lib+cur20), `e1[:<pm>[:<every>[:<pop>[:<gens>]]]]`.
fn arm_cfg(spec: &str, s: u64, steps: u64, window: u64, cover_n: u64) -> RunCfgE1 {
    let mut c = RunCfgB14 { cover_n, ..RunCfgB14::base(s, steps, window) };
    let mut evo: Option<EvoCfg> = None;
    for part in spec.split('+') {
        match part {
            "base" => {}
            "lib" => c.library = Some(LibCfg::default()),
            "b14" => {
                c.library = Some(LibCfg::default());
                c.curiosity = Some(CurCfg { pm: 20, ..DEFAULT_CUR });
            }
            _ if part.starts_with("cur") => c.curiosity = Some(CurCfg { pm: part[3..].parse().expect("cur<pm>"), ..DEFAULT_CUR }),
            _ if part.starts_with("e1") => {
                let mut e = DEFAULT_EVO;
                let xs: Vec<&str> = part.split(':').skip(1).collect();
                if let Some(v) = xs.first() {
                    e.pm = v.parse().unwrap();
                }
                if let Some(v) = xs.get(1) {
                    e.every = v.parse().unwrap();
                }
                if let Some(v) = xs.get(2) {
                    e.pop = v.parse().unwrap();
                }
                if let Some(v) = xs.get(3) {
                    e.gens = v.parse().unwrap();
                }
                evo = Some(e);
            }
            _ => panic!("unknown arm part {part}"),
        }
    }
    RunCfgE1 { b14: c, evo }
}

fn post(o: &OutE1, from: u64) -> f64 {
    let (r, c) = o.wins.iter().filter(|w| w.from >= from).fold((0i64, 0i64), |a, w| (a.0 + w.reward, a.1 + w.cost));
    if c > 0 { r as f64 / c as f64 } else { 0.0 }
}

fn slope(o: &OutE1, from: u64) -> f64 {
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

/// Families covered (>= 0.9 of fresh tasks right) at the end: (of the 14 expressible, of all 18).
fn covered(o: &OutE1) -> (f64, f64) {
    let c = &o.wins.last().unwrap().cover;
    let n14 = c.iter().take(FAMILIES.len()).filter(|x| **x >= 0.9).count();
    let n18 = c.iter().filter(|x| **x >= 0.9).count();
    (n14 as f64, n18 as f64)
}

/// Evolved rules that are wrong on fresh tasks (applied and wrong on any family of their difficulty), and the number checked.
fn wrong_rules(s: u64, o: &OutE1) -> (usize, usize, Vec<String>) {
    let mut bad = vec![];
    for (_, r) in &o.evolved {
        let fc = fresh_check(s, r, 200);
        if fc.iter().any(|(_, ap, ok)| ap > ok) {
            bad.push(format!("{} {}{} {:?}", r.id, r.xf.code(), r.form.code(), fc));
        }
    }
    (bad.len(), o.evolved.len(), bad)
}

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
    let ss = seeds(args, (101, 104));
    let steps: u64 = arg(args, "--steps").map_or(50_000, |x| x.parse().unwrap());
    let window: u64 = arg(args, "--window").map_or(10_000, |x| x.parse().unwrap());
    let arm: String = arg(args, "--arm").unwrap_or("b14+e1".into());
    let t0 = std::time::Instant::now();
    let outs: Vec<(u64, OutE1)> = ss.par_iter().map(|&s| (s, run_e1(&arm_cfg(&arm, s, steps, window, 40)))).collect();
    eprintln!("runs: {:.0} s", t0.elapsed().as_secs_f64());
    let fams = all_families();
    for (s, o) in &outs {
        println!("\n#### world {s}, arm {arm}: {} passes, {} evaluations ({:.4} of the energy spent at {EVALS_PER_ENERGY} per unit), evolution time {:.1} s of {:.1} s, {} verified, {} rejected\n", o.evo_passes, o.evo_evals, o.evo_evals as f64 / EVALS_PER_ENERGY as f64 / o.total_cost.max(1) as f64, o.evo_nanos as f64 / 1e9, o.run_nanos as f64 / 1e9, o.evo_verified, o.evo_rejected);
        println!("| steps | reward per energy | rule answers (right) | evolved-rule answers (right) | accepted (evolved) | live | covered 14 / 18 |");
        println!("|---|---|---|---|---|---|---|");
        for w in &o.wins {
            let c14 = w.cover.iter().take(FAMILIES.len()).filter(|c| **c >= 0.9).count();
            let c18 = w.cover.iter().filter(|c| **c >= 0.9).count();
            println!("| {}k-{}k | {:.3} | {} ({}) | {} ({}) | {} ({}) | {} | {c14} / {c18} |", w.from / 1000, (w.from + w.steps) / 1000, w.rpe(), w.rule_answers, w.rule_right, w.evo_answers, w.evo_right, w.accepted, w.evo_accepted, w.live_end);
        }
        if let Some(last) = o.wins.last() {
            let missing: Vec<String> = fams.iter().zip(&last.cover).filter(|(_, c)| **c < 0.9).map(|(k, c)| format!("{k:?} {c:.2}")).collect();
            println!("\nnot covered at the end: {}", missing.join(", "));
        }
        println!("evolved rules:");
        for (st, r) in &o.evolved {
            let fc = fresh_check(*s, r, 200);
            println!("  step {st}: d{} {} {} {}{} [{}] fresh: {:?}", r.sig, r.id, if r.retired { "(retired)" } else { "" }, if r.xf.is_id() { String::new() } else { format!("{} then ", r.xf.code()) }, r.form.dsl(), r.form.code(), fc);
        }
        let st = o.state.as_ref().unwrap();
        println!("rules at the end: {}", st.rules.iter().map(|r| format!("d{} {}/{}{}", r.sig, r.xf.code(), r.form.code(), if r.retired { " (retired)" } else { "" })).collect::<Vec<_>>().join(", "));
        let mut per = [0u64; 6];
        for e in &st.examples {
            per[(e.sig as usize).min(5)] += 1;
        }
        println!("examples per difficulty 1..5: {:?}", &per[1..]);
    }
}

fn cmd_arms(args: &[String]) {
    let ss = seeds(args, (101, 120));
    let steps: u64 = arg(args, "--steps").map_or(60_000, |x| x.parse().unwrap());
    let window: u64 = arg(args, "--window").map_or(10_000, |x| x.parse().unwrap());
    let from: u64 = arg(args, "--post").map_or(50_000, |x| x.parse().unwrap());
    let arms: Vec<String> = arg(args, "--arms").unwrap_or("b14,b14+e1".into()).split(',').map(|x| x.to_string()).collect();
    let t0 = std::time::Instant::now();
    let refs = references(&ss);
    let jobs: Vec<(u64, usize)> = ss.iter().flat_map(|&s| (0..arms.len()).map(move |a| (s, a))).collect();
    let res: Vec<((u64, usize), OutE1)> = jobs
        .par_iter()
        .map(|&(s, a)| {
            let mut o = run_e1(&arm_cfg(&arms[a], s, steps, window, 40));
            o.state = None;
            ((s, a), o)
        })
        .collect();
    eprintln!("arms: {:.0} s", t0.elapsed().as_secs_f64());
    let get = |s: u64, a: usize| &res.iter().find(|r| r.0 == (s, a)).unwrap().1;
    let gain = |s: u64| refs.iter().find(|r| r.0 == s).map_or(0.0, |r| r.2 - r.1);
    let rule_w: Vec<u64> = ss.iter().copied().filter(|&s| gain(s) >= 0.1).collect();
    let simple_w: Vec<u64> = ss.iter().copied().filter(|&s| gain(s) < 0.1).collect();
    println!("seeds {}..{}, {steps} steps, windows of {window}, post part from step {from}; rule worlds (oracle book gain >= 0.1): {:?}; simple worlds: {:?}\n", ss[0], ss[ss.len() - 1], rule_w, simple_w);
    println!("| steps | {} |", arms.join(" | "));
    println!("|---|{}", "---|".repeat(arms.len()));
    let nw = res.iter().map(|r| r.1.wins.len()).min().unwrap_or(0);
    for i in 0..nw {
        let w0 = &get(ss[0], 0).wins[i];
        let cells: Vec<String> = (0..arms.len())
            .map(|a| {
                let xs: Vec<f64> = ss.iter().map(|&s| get(s, a).wins[i].rpe()).collect();
                ci_n(&xs)
            })
            .collect();
        println!("| {}k-{}k | {} |", w0.from / 1000, (w0.from + w0.steps) / 1000, cells.join(" | "));
    }
    println!("\n| arm | post reward per energy | minus {} (all) | rule worlds | simple worlds | slope per 100k after {}k | slope minus {} | covered 14 (end) | minus {} | covered 18 (end) | minus {} |", arms[0], from / 1000, arms[0], arms[0], arms[0]);
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    for a in 0..arms.len() {
        let p: Vec<f64> = ss.iter().map(|&s| post(get(s, a), from)).collect();
        let d = |ws: &[u64]| -> Vec<f64> { ws.iter().map(|&s| post(get(s, a), from) - post(get(s, 0), from)).collect() };
        let sl: Vec<f64> = ss.iter().map(|&s| slope(get(s, a), from)).collect();
        let sld: Vec<f64> = ss.iter().map(|&s| slope(get(s, a), from) - slope(get(s, 0), from)).collect();
        let c14: Vec<f64> = ss.iter().map(|&s| covered(get(s, a)).0).collect();
        let c14d: Vec<f64> = ss.iter().map(|&s| covered(get(s, a)).0 - covered(get(s, 0)).0).collect();
        let c18: Vec<f64> = ss.iter().map(|&s| covered(get(s, a)).1).collect();
        let c18d: Vec<f64> = ss.iter().map(|&s| covered(get(s, a)).1 - covered(get(s, 0)).1).collect();
        println!("| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |", arms[a], ci_n(&p), paired(&d(&ss)), paired(&d(&rule_w)), paired(&d(&simple_w)), ci_n(&sl), paired(&sld), ci_n(&c14), paired(&c14d), ci_n(&c18), paired(&c18d));
    }
    println!("\nRelative post difference (arm / {} - 1): ", arms[0]);
    for a in 1..arms.len() {
        let d: Vec<f64> = ss.iter().map(|&s| post(get(s, a), from) / post(get(s, 0), from) - 1.0).collect();
        let ds: Vec<f64> = simple_w.iter().map(|&s| post(get(s, a), from) / post(get(s, 0), from) - 1.0).collect();
        println!("  {}: all {}; simple worlds {}", arms[a], paired(&d), paired(&ds));
    }
    println!("\nEvolution per arm (sums / means over worlds):");
    for a in 0..arms.len() {
        let os: Vec<&OutE1> = ss.iter().map(|&s| get(s, a)).collect();
        let n = os.len() as f64;
        let evolved: usize = os.iter().map(|o| o.evolved.len()).sum();
        let (mut bad, mut chk) = (0usize, 0usize);
        let mut bad_txt = vec![];
        for &s in &ss {
            let (b, c, t) = wrong_rules(s, get(s, a));
            bad += b;
            chk += c;
            bad_txt.extend(t);
        }
        let ans: u64 = os.iter().map(|o| o.wins.iter().map(|w| w.evo_answers).sum::<u64>()).sum();
        let right: u64 = os.iter().map(|o| o.wins.iter().map(|w| w.evo_right).sum::<u64>()).sum();
        let share: Vec<f64> = os.iter().map(|o| o.evo_evals as f64 / EVALS_PER_ENERGY as f64 / o.total_cost.max(1) as f64).collect();
        let tshare: Vec<f64> = os.iter().map(|o| o.evo_nanos as f64 / o.run_nanos.max(1) as f64).collect();
        let secs: f64 = os.iter().map(|o| o.evo_nanos as f64 / 1e9).sum::<f64>() / n;
        let run_secs: f64 = os.iter().map(|o| o.run_nanos as f64 / 1e9).sum::<f64>() / n;
        let rej: u64 = os.iter().map(|o| o.evo_rejected).sum();
        let ver: u64 = os.iter().map(|o| o.evo_verified).sum();
        let passes: u64 = os.iter().map(|o| o.evo_passes).sum();
        let after_ap: u64 = os.iter().map(|o| o.evo_after.0).sum();
        let after_ok: u64 = os.iter().map(|o| o.evo_after.1).sum();
        println!(
            "  {}: passes {passes}, evolved rules accepted {evolved} (wrong on fresh tasks: {bad} of {chk}), candidates verified {ver}, rejected by the check {rej}; evolved rules on the being's later revealed examples: applicable {after_ap}, prediction right {after_ok} (precision before the checker {:.4}); evolved-rule answers given {ans}, judged right {right} ({:.4}, includes execution slips); energy-equivalent share {}; time share {}; evolution {secs:.1} s of {run_secs:.1} s per life",
            arms[a], after_ok as f64 / after_ap.max(1) as f64, right as f64 / ans.max(1) as f64, ci_n(&share), ci_n(&tshare)
        );
        for t in bad_txt {
            println!("    WRONG: {t}");
        }
        // which families do evolved rules cover
        let mut fam_count: std::collections::BTreeMap<String, usize> = Default::default();
        for &s in &ss {
            for (_, r) in &get(s, a).evolved {
                for (k, ap, ok) in fresh_check(s, r, 100) {
                    if ap > 0 && ok * 10 >= 9 * 100 {
                        *fam_count.entry(format!("{k:?}")).or_default() += 1;
                    }
                }
            }
        }
        if !fam_count.is_empty() {
            println!("    families an evolved rule answers >= 90% of: {:?}", fam_count);
        }
    }
    if let Some(path) = arg(args, "--per-world") {
        let mut txt = String::new();
        for &s in &ss {
            txt.push_str(&format!("{s} gain {:.3}", gain(s)));
            for a in 0..arms.len() {
                txt.push_str(&format!(" {}={:.4}/{:.3}/{}", arms[a], post(get(s, a), from), slope(get(s, a), from), covered(get(s, a)).1));
            }
            txt.push('\n');
        }
        std::fs::write(path, txt).unwrap();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("diag") => cmd_diag(&args),
        Some("arms") => cmd_arms(&args),
        _ => eprintln!("usage: measure_e1 diag|arms [--seeds a..b] [--steps n] [--window n] [--post n] [--arms a,b] [--arm a]"),
    }
}
