//! `measure_l2 diag`: why later families are not learned faster (tuning seeds only).

use crate::being::world_o::measure::FamRec;
use crate::being::world_o::measure_b12::RunB12;
use crate::being::world_o::world::{WOCfg, MAX_TIER};
use crate::full::top_rho;
use crate::report::{arg, ci, learner_b12, run_cond, seeds, Tune};
use crate::worldo::spearman;
use rayon::prelude::*;

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

pub fn cmd_diag(args: &[String]) {
    let ss = seeds(args, (101, 120));
    let budget: i64 = arg(args, "--budget").map_or(1_200_000, |x| x.parse().unwrap());
    let window: i64 = arg(args, "--window").map_or(100_000, |x| x.parse().unwrap());
    let conds: Vec<String> = arg(args, "--conds").unwrap_or("current,form".into()).split(',').map(|s| s.to_string()).collect();
    let t = Tune::from_args(args);
    let t0 = std::time::Instant::now();
    let jobs: Vec<(String, u64)> = conds.iter().flat_map(|c| ss.iter().map(move |s| (c.clone(), *s))).collect();
    let res: Vec<((String, u64), RunB12)> = jobs.par_iter().map(|(c, s)| ((c.clone(), *s), run_cond(c, learner_b12(c, &t), &WOCfg::new(*s, budget), window))).collect();
    eprintln!("runs: {:.0} s", t0.elapsed().as_secs_f64());
    for c in &conds {
        let rs: Vec<&RunB12> = res.iter().filter(|r| &r.0 .0 == c).map(|r| &r.1).collect();
        let rho: Vec<f64> = rs.iter().filter_map(|r| top_rho(r)).collect();
        // censoring: families whose life ends after the budget
        let rho_c: Vec<f64> = rs.iter().filter_map(|r| {
            let top: Vec<&FamRec> = r.run.fams.iter().filter(|f| f.learnable() && f.tier >= MAX_TIER && f.retire <= budget).collect();
            if top.len() < 6 { return None; }
            let x: Vec<f64> = top.iter().map(|f| f.fid as f64).collect();
            let y: Vec<f64> = top.iter().map(|f| f.competent_after.map_or(1e9, |a| a.0 as f64)).collect();
            Some(spearman(&x, &y)).filter(|v| v.is_finite())
        }).collect();
        println!("\n## {c}: top_rho {} ; uncensored-only {}", ci(&rho), ci(&rho_c));
        for (lo, hi) in [(32u64, 96u64), (96, 160), (160, 400)] {
            let bs: Vec<(u64, u64, u64)> = rs.iter().flat_map(|r| bad_split(r, budget)).filter(|x| x.0 >= lo && x.0 < hi).collect();
            let n = bs.len().max(1) as f64;
            println!("seq {lo}-{hi}: n {} bad attempts before first rule {:.2}, after it {:.2}", bs.len(), bs.iter().map(|x| x.1 as f64).sum::<f64>() / n, bs.iter().map(|x| x.2 as f64).sum::<f64>() / n);
        }
        println!("| type | block | n | reached | median att | p75 att | censored | not reached & censored | first-10 ratio |");
        println!("|---|---|---|---|---|---|---|---|---|");
        for ty in ["seq", "compose", "grid", "knap", "formula", "all"] {
            let mut b = 32u64;
            while b < 260 {
                let g: Vec<&FamRec> = rs.iter().flat_map(|r| r.run.fams.iter()).filter(|f| f.learnable() && f.tier >= MAX_TIER && f.fid >= b && f.fid < b + 48 && (ty == "all" || f.ftype.map(|t| t.name()) == Some(ty))).collect();
                if !g.is_empty() {
                    let reached: Vec<f64> = g.iter().filter_map(|f| f.competent_after.map(|a| a.0 as f64)).collect();
                    let mut all: Vec<f64> = g.iter().map(|f| f.competent_after.map_or(1e9, |a| a.0 as f64)).collect();
                    all.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let p75 = all[(all.len() * 3 / 4).min(all.len() - 1)];
                    let cens = g.iter().filter(|f| f.retire > budget).count();
                    let nrc = g.iter().filter(|f| f.retire > budget && f.competent_after.is_none()).count();
                    let r10: f64 = g.iter().map(|f| { let x = &f.ratios[..f.ratios.len().min(10)]; x.iter().map(|v| *v as f64).sum::<f64>() / x.len().max(1) as f64 }).sum::<f64>() / g.len() as f64;
                    println!("| {ty} | {b}-{} | {} | {:.0}% | {:.0} | {} | {} | {} | {:.3} |", b + 47, g.len(), 100.0 * reached.len() as f64 / g.len() as f64, median(reached), if p75 >= 1e9 { "never".into() } else { format!("{p75:.0}") }, cens, nrc, r10);
                }
                b += 48;
            }
        }
    }
}

/// Where the bad attempts of a top-tier sequence family come from: before its first rule, after it.
pub fn bad_split(r: &RunB12, budget: i64) -> Vec<(u64, u64, u64)> {
    let mut v = vec![];
    for f in r.run.fams.iter().filter(|f| f.learnable() && f.tier >= MAX_TIER && f.retire <= budget && f.ftype.map(|t| t.name()) == Some("seq")) {
        let Some(d) = r.diag.iter().find(|d| d.fid == f.fid) else { continue };
        let Some(c) = f.competent_after else { continue };
        let last_bad = c.0.saturating_sub(10);
        let before = d.attempts_before_rule.unwrap_or(last_bad).min(last_bad);
        v.push((f.fid, before, last_bad - before));
    }
    v
}
