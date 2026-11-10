//! `measure_l2 report`: the pre-registered Track L2 criteria on World O (paired by world), and what the meta-prior did.

use crate::being::world_o::measure::FamRec;
use crate::being::world_o::measure_b12::RunB12;
use crate::being::world_o::world::{WOCfg, MAX_TIER};
use crate::conds::{learner_l2, L2Tune};
use crate::full::top_rho;
use crate::report::{arg, ci, run_cond, seeds, Tune};
use rayon::prelude::*;

/// Attempts to competence with "not reached" counted as this cap (for the thirds comparison).
pub const CAP: f64 = 100.0;

fn span(r: &RunB12, a: usize, b: usize) -> f64 {
    let (x, y) = r.run.wins[a..b.min(r.run.wins.len())].iter().fold((0i64, 0i64), |acc, w| (acc.0 + w.true_reward, acc.1 + w.cost));
    x as f64 / y.max(1) as f64
}

fn top(r: &RunB12) -> Vec<&FamRec> {
    r.run.fams.iter().filter(|f| f.learnable() && f.tier >= MAX_TIER).collect()
}

/// Mean attempts to competence (capped) of the last third minus the first third of a world's top-tier families.
pub fn thirds(r: &RunB12) -> Option<(f64, f64)> {
    let t = top(r);
    if t.len() < 6 {
        return None;
    }
    let n3 = t.len() / 3;
    let m = |v: &[&FamRec]| v.iter().map(|f| f.competent_after.map_or(CAP, |a| (a.0 as f64).min(CAP))).sum::<f64>() / v.len() as f64;
    Some((m(&t[..n3]), m(&t[t.len() - n3..])))
}

/// Regret proxy in a family's first window: 1 - mean ratio of its first 10 attempts (top tier), mean per world.
pub fn first_window(r: &RunB12) -> Option<f64> {
    let t = top(r);
    if t.is_empty() {
        return None;
    }
    Some(t.iter().map(|f| {
        let x = &f.ratios[..f.ratios.len().min(10)];
        1.0 - x.iter().map(|v| *v as f64).sum::<f64>() / x.len().max(1) as f64
    }).sum::<f64>() / t.len() as f64)
}

pub fn cmd_report(args: &[String]) {
    let ss = seeds(args, (1, 20));
    let budget: i64 = arg(args, "--budget").map_or(1_200_000, |x| x.parse().unwrap());
    let window: i64 = arg(args, "--window").map_or(100_000, |x| x.parse().unwrap());
    let conds: Vec<String> = arg(args, "--conds").unwrap_or("current,form,l2,form+l2".into()).split(',').map(|s| s.to_string()).collect();
    let refs: Vec<String> = arg(args, "--refs").unwrap_or("current,form".into()).split(',').map(|s| s.to_string()).collect();
    let t = Tune::from_args(args);
    let lt = L2Tune::from_args(args);
    let t0 = std::time::Instant::now();
    let jobs: Vec<(String, u64)> = conds.iter().flat_map(|c| ss.iter().map(move |s| (c.clone(), *s))).collect();
    let res: Vec<((String, u64), RunB12)> = jobs.par_iter().map(|(c, s)| ((c.clone(), *s), run_cond(c, learner_l2(c, &t, &lt), &WOCfg::new(*s, budget), window))).collect();
    eprintln!("world O runs: {:.0} s", t0.elapsed().as_secs_f64());
    let get = |c: &str, s: u64| res.iter().find(|r| r.0 .0 == c && r.0 .1 == s).map(|r| &r.1);
    let nwin = res.iter().map(|r| r.1.run.wins.len()).min().unwrap_or(0);
    println!("World O, default config, budget {budget}, window {window}, seeds {}..{} (n = {}); B12 constants {:?}; L2 {:?}", ss[0], ss[ss.len() - 1], ss.len(), t, lt);
    let wl = |d: &[f64]| format!("{} of {}", d.iter().filter(|x| **x < 0.0).count(), d.len());
    let ww = |d: &[f64]| format!("{} / {} / {}", d.iter().filter(|x| **x > 0.0).count(), d.iter().filter(|x| **x == 0.0).count(), d.iter().filter(|x| **x < 0.0).count());
    println!("\n#### Per condition\n");
    println!("| condition | Spearman (index vs attempts to competence, top tier; negative = later faster) | attempts to competence, first third of top-tier families | last third | last minus first, paired | first-10 regret proxy | reward per energy after window 1 | rules accepted | competent share top tier |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for c in &conds {
        let rs: Vec<&RunB12> = ss.iter().filter_map(|s| get(c, *s)).collect();
        let rho: Vec<f64> = rs.iter().filter_map(|r| top_rho(r)).collect();
        let th: Vec<(f64, f64)> = rs.iter().filter_map(|r| thirds(r)).collect();
        let d: Vec<f64> = th.iter().map(|x| x.1 - x.0).collect();
        let fw: Vec<f64> = rs.iter().filter_map(|r| first_window(r)).collect();
        let rw: Vec<f64> = rs.iter().map(|r| span(r, 1, nwin)).collect();
        let comp: Vec<f64> = rs.iter().map(|r| { let t = top(r); t.iter().filter(|f| f.competent_after.is_some()).count() as f64 / t.len().max(1) as f64 }).collect();
        println!(
            "| {c} | {} ({} negative) | {} | {} | {} ({} lower) | {} | {} | {:.1} | {} |",
            ci(&rho),
            wl(&rho),
            ci(&th.iter().map(|x| x.0).collect::<Vec<_>>()),
            ci(&th.iter().map(|x| x.1).collect::<Vec<_>>()),
            ci(&d),
            wl(&d),
            ci(&fw),
            ci(&rw),
            rs.iter().map(|r| r.run.rules_total as f64).sum::<f64>() / rs.len().max(1) as f64,
            ci(&comp)
        );
    }
    println!("\n#### Paired differences (condition minus reference, same worlds): wins / ties / losses of n\n");
    println!("| comparison | reward per energy after window 1 | Spearman | last-third minus first-third attempts | first-10 regret proxy |");
    println!("|---|---|---|---|---|");
    for r0 in &refs {
        for c in &conds {
            if c == r0 {
                continue;
            }
            let pr = |f: &dyn Fn(&RunB12) -> Option<f64>| -> Vec<f64> { ss.iter().filter_map(|s| Some(f(get(c, *s)?)? - f(get(r0, *s)?)?)).collect() };
            let dr = pr(&|r| Some(span(r, 1, nwin)));
            let dp = pr(&|r| top_rho(r));
            let dt = pr(&|r| thirds(r).map(|x| x.1 - x.0));
            let df = pr(&|r| first_window(r));
            println!("| {c} minus {r0} | {} ({}) | {} ({}) | {} ({}) | {} ({}) |", ci(&dr), ww(&dr), ci(&dp), ww(&dp), ci(&dt), ww(&dt), ci(&df), ww(&df));
        }
    }
    println!("\n#### Wrong rules (every accepted rule of a sequence family judged on 60 sampled tasks at acceptance)\n");
    println!("| condition | judged | wrong |");
    println!("|---|---|---|");
    for c in &conds {
        let rs: Vec<&RunB12> = ss.iter().filter_map(|s| get(c, *s)).collect();
        let j: Vec<_> = rs.iter().flat_map(|r| r.judged.iter()).filter(|j| j.applies > 0).collect();
        println!("| {c} | {} | {} |", j.len(), j.iter().filter(|j| j.wrong()).count());
    }
}
