//! `measure_b12 report`: World O learning curves, regret, order-vs-competence correlation, competence per
//! tier over time, rules and wrong rules, for the Track B12 conditions (paired by world).

use crate::being::world_o::measure::{FamRec, COMP_K, COMP_LEVEL};
use crate::being::world_o::measure_b12::{FamDiag, RunB12};
use crate::being::world_o::world::{WOCfg, MAX_TIER};
use crate::report::{arg, ci, learner_b12, run_cond, seeds, Tune};
use crate::worldo::spearman;
use rayon::prelude::*;

pub const DEFAULT_CONDS: &str = "current,fix+b7,prior,explore,fam,form,all,all+fix+b7,oracle-knows-rules";

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

/// Spearman (family index vs attempts to competence, not reached = slowest) over a world's learnable top-tier families.
pub fn top_rho(r: &RunB12) -> Option<f64> {
    let top: Vec<&FamRec> = r.run.fams.iter().filter(|f| f.learnable() && f.tier >= MAX_TIER).collect();
    if top.len() < 6 {
        return None;
    }
    let x: Vec<f64> = top.iter().map(|f| f.fid as f64).collect();
    let y: Vec<f64> = top.iter().map(|f| f.competent_after.map_or(1e9, |a| a.0 as f64)).collect();
    let rho = spearman(&x, &y);
    rho.is_finite().then_some(rho)
}

fn span(r: &RunB12, a: usize, b: usize) -> f64 {
    let (x, y) = r.run.wins[a..b].iter().fold((0i64, 0i64), |acc, w| (acc.0 + w.true_reward, acc.1 + w.cost));
    x as f64 / y.max(1) as f64
}

pub fn cmd_report(args: &[String]) {
    let ss = seeds(args, (1, 20));
    let budget: i64 = arg(args, "--budget").map_or(1_200_000, |x| x.parse().unwrap());
    let window: i64 = arg(args, "--window").map_or(100_000, |x| x.parse().unwrap());
    let block: u64 = arg(args, "--block").map_or(32, |x| x.parse().unwrap());
    let base = arg(args, "--base").unwrap_or("current".into());
    let t = Tune::from_args(args);
    let conds: Vec<String> = arg(args, "--conds").unwrap_or(DEFAULT_CONDS.into()).split(',').map(|s| s.to_string()).collect();
    let t0 = std::time::Instant::now();
    let jobs: Vec<(String, u64)> = conds.iter().flat_map(|c| ss.iter().map(move |s| (c.clone(), *s))).collect();
    let res: Vec<((String, u64), RunB12)> = jobs.par_iter().map(|(c, s)| ((c.clone(), *s), run_cond(c, learner_b12(c, &t), &WOCfg::new(*s, budget), window))).collect();
    eprintln!("world O runs: {:.0} s", t0.elapsed().as_secs_f64());
    let get = |c: &str, s: u64| res.iter().find(|r| r.0 .0 == c && r.0 .1 == s).map(|r| &r.1);
    let nwin = res.iter().map(|r| r.1.run.wins.len()).min().unwrap_or(0);
    let learners: Vec<&String> = conds.iter().filter(|c| !c.starts_with("oracle")).collect();
    let has_oracle = conds.iter().any(|c| c == "oracle-knows-rules");
    println!("World O, default config, budget {budget}, window {window}, seeds {}..{} (n = {}); B12 constants {:?}", ss[0], ss[ss.len() - 1], ss.len(), t);
    let w = |i: usize| format!("{}k-{}k", i as i64 * window / 1000, (i as i64 + 1) * window / 1000);
    let header = |first: &str, cs: &[&String]| {
        print!("| {first} |");
        for c in cs {
            print!(" {c} |");
        }
        println!();
        print!("|---|");
        for _ in cs {
            print!("---|");
        }
        println!();
    };
    let all: Vec<&String> = conds.iter().collect();
    println!("\n#### True reward per energy per window, mean [95% interval]\n");
    header("energy", &all);
    for i in 0..nwin {
        print!("| {} |", w(i));
        for c in &conds {
            let v: Vec<f64> = ss.iter().filter_map(|s| get(c, *s)).map(|r| r.run.wins[i].rpe()).collect();
            print!(" {} |", ci(&v));
        }
        println!();
    }
    if has_oracle {
        println!("\n#### Regret per window vs oracle-knows-rules (paired by world)\n");
        header("energy", &learners);
        for i in 0..nwin {
            print!("| {} |", w(i));
            for c in &learners {
                let v: Vec<f64> = ss.iter().filter_map(|s| Some(get("oracle-knows-rules", *s)?.run.wins[i].rpe() - get(c, *s)?.run.wins[i].rpe())).collect();
                print!(" {} |", ci(&v));
            }
            println!();
        }
        println!("\n#### Regret, first third vs last third of the windows (the first window left out), paired by world\n");
        println!("| condition | regret first third | regret last third | change |");
        println!("|---|---|---|---|");
        let third = (nwin.saturating_sub(1) / 3).max(1);
        for c in &learners {
            let rf: Vec<f64> = ss.iter().filter_map(|s| Some(span(get("oracle-knows-rules", *s)?, 1, 1 + third) - span(get(c, *s)?, 1, 1 + third))).collect();
            let rl: Vec<f64> = ss.iter().filter_map(|s| Some(span(get("oracle-knows-rules", *s)?, nwin - third, nwin) - span(get(c, *s)?, nwin - third, nwin))).collect();
            let d: Vec<f64> = rf.iter().zip(&rl).map(|(a, b)| b - a).collect();
            println!("| {c} | {} | {} | {} |", ci(&rf), ci(&rl), ci(&d));
        }
    }
    println!("\n#### Whole run after the first window, paired differences in reward per energy\n");
    println!("| comparison | difference | first better in |");
    println!("|---|---|---|");
    let mut refs: Vec<&str> = vec![base.as_str()];
    if conds.iter().any(|c| c == "fix+b7") && base != "fix+b7" {
        refs.push("fix+b7");
    }
    for r0 in &refs {
        for c in &learners {
            if c.as_str() == *r0 {
                continue;
            }
            let d: Vec<f64> = ss.iter().filter_map(|s| Some(span(get(c, *s)?, 1, nwin) - span(get(r0, *s)?, 1, nwin))).collect();
            if d.is_empty() {
                continue;
            }
            println!("| {c} minus {r0} | {} | {} of {} |", ci(&d), d.iter().filter(|x| **x > 0.0).count(), d.len());
        }
    }
    println!("\n#### Faster at learning later families? Top tier only: Spearman, per world, of the family index with attempts to competence (not reached = slowest); negative = faster later\n");
    println!("| condition | worlds | Spearman mean [95% interval] | minus {base}, paired |");
    println!("|---|---|---|---|");
    for c in &learners {
        let pairs: Vec<(f64, Option<f64>)> = ss.iter().filter_map(|s| Some((top_rho(get(c, *s)?)?, get(&base, *s).and_then(top_rho)))).collect();
        let v: Vec<f64> = pairs.iter().map(|p| p.0).collect();
        let d: Vec<f64> = pairs.iter().filter_map(|p| p.1.map(|b| p.0 - b)).collect();
        println!("| {c} | {} | {} | {} |", v.len(), ci(&v), if c.as_str() == base { "-".into() } else { ci(&d) });
    }
    println!("\n#### Competence per family block (order of appearance), pooled over worlds\n\nLearnable: mean best success the being's means allow on its attempted tasks >= 0.5. Competent: last {COMP_K} attempts >= {COMP_LEVEL} of the best expected success per energy. Per world share in brackets: mean [95% interval] over worlds.\n");
    println!("| condition | families | tiers | learnable | competent (pooled) | competent share per world | median attempts to competence | mean ratio first 10 attempts |");
    println!("|---|---|---|---|---|---|---|---|");
    for c in &learners {
        let maxf = ss.iter().filter_map(|s| get(c, *s)).flat_map(|r| r.run.fams.iter()).map(|f| f.fid).max().unwrap_or(0);
        let mut b = 0;
        while b <= maxf {
            let per: Vec<Vec<&FamRec>> = ss.iter().filter_map(|s| get(c, *s)).map(|r| r.run.fams.iter().filter(|f| f.learnable() && f.fid >= b && f.fid < b + block).collect()).collect();
            let g: Vec<&FamRec> = per.iter().flatten().copied().collect();
            if !g.is_empty() {
                let reached: Vec<&&FamRec> = g.iter().filter(|f| f.competent_after.is_some()).collect();
                let shares: Vec<f64> = per.iter().filter(|v| !v.is_empty()).map(|v| v.iter().filter(|f| f.competent_after.is_some()).count() as f64 / v.len() as f64).collect();
                let r10: Vec<f64> = g.iter().filter(|f| !f.ratios.is_empty()).map(|f| {
                    let x = &f.ratios[..f.ratios.len().min(10)];
                    x.iter().map(|v| *v as f64).sum::<f64>() / x.len() as f64
                }).collect();
                println!(
                    "| {c} | {}-{} | {}-{} | {} | {} ({:.0}%) | {} | {:.0} | {:.3} |",
                    b,
                    b + block - 1,
                    g.iter().map(|f| f.tier).min().unwrap(),
                    g.iter().map(|f| f.tier).max().unwrap(),
                    g.len(),
                    reached.len(),
                    100.0 * reached.len() as f64 / g.len() as f64,
                    ci(&shares),
                    median(reached.iter().map(|f| f.competent_after.unwrap().0 as f64).collect()),
                    r10.iter().sum::<f64>() / r10.len().max(1) as f64
                );
            }
            b += block;
        }
    }
    println!("\n#### Rules accepted per window (mean over worlds)\n");
    header("energy", &learners);
    for i in 0..nwin {
        print!("| {} |", w(i));
        for c in &learners {
            let v: Vec<f64> = ss.iter().filter_map(|s| get(c, *s)).map(|r| r.run.wins[i].rules_accepted as f64).collect();
            print!(" {:.1} |", v.iter().sum::<f64>() / v.len().max(1) as f64);
        }
        println!();
    }
    println!("\n#### Rules, wrong rules, rule answers, exploration (per world, mean; wrong = applies to sampled tasks of its family and difficulty and is wrong on more than 5% of them)\n");
    println!("| condition | rules accepted | judged (seq families) | wrong | wrong share | with reduced support | wrong among reduced | rule answers | right share | attempt share | exploration energy share | exploration steps |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
    for c in &learners {
        let rs: Vec<&RunB12> = ss.iter().filter_map(|s| get(c, *s)).collect();
        let n = rs.len().max(1) as f64;
        let judged: Vec<_> = rs.iter().flat_map(|r| r.judged.iter()).filter(|j| j.applies > 0).collect();
        let wrong = judged.iter().filter(|j| j.wrong()).count();
        let red: Vec<_> = judged.iter().filter(|j| j.reduced).collect();
        let redw = red.iter().filter(|j| j.wrong()).count();
        let uses: u64 = rs.iter().map(|r| r.run.wins.iter().map(|w| w.rule_uses).sum::<u64>()).sum();
        let right: u64 = rs.iter().map(|r| r.run.wins.iter().map(|w| w.rule_right).sum::<u64>()).sum();
        let att: f64 = rs.iter().map(|r| r.run.wins.iter().map(|w| w.attempts).sum::<u64>() as f64 / r.run.steps.max(1) as f64).sum::<f64>() / n;
        let ex: f64 = rs.iter().map(|r| r.explore_energy as f64 / r.run.cost.max(1) as f64).sum::<f64>() / n;
        println!(
            "| {c} | {:.1} | {} | {} | {:.4} | {} | {} | {:.0} | {:.3} | {att:.3} | {ex:.4} | {:.0} |",
            rs.iter().map(|r| r.run.rules_total as f64).sum::<f64>() / n,
            judged.len(),
            wrong,
            wrong as f64 / judged.len().max(1) as f64,
            red.len(),
            redw,
            uses as f64 / n,
            right as f64 / uses.max(1) as f64,
            rs.iter().map(|r| r.explore_attempts as f64).sum::<f64>() / n
        );
    }
    println!("\n#### How new top-tier families are met (first 100 menus on which each appears; pooled; per family means)\n");
    println!("| condition | families | attempts per menu | menus skipped per menu | examples given | rules per family (life) | seq families with a rule | attempts before the first rule (median, seq) |");
    println!("|---|---|---|---|---|---|---|---|");
    for c in &learners {
        let v: Vec<&FamDiag> = ss.iter().filter_map(|s| get(c, *s)).flat_map(|r| r.diag.iter()).filter(|f| f.tier >= MAX_TIER && f.first_menus >= 20).collect();
        let n = v.len().max(1) as f64;
        let seqs: Vec<&&FamDiag> = v.iter().filter(|f| f.ftype.map(|t| t.name()) == Some("seq")).collect();
        println!(
            "| {c} | {} | {:.3} | {:.3} | {:.1} | {:.2} | {:.2} | {:.0} |",
            v.len(),
            v.iter().map(|f| f.first_attempts as f64 / f.first_menus as f64).sum::<f64>() / n,
            v.iter().map(|f| f.first_skips as f64 / f.first_menus as f64).sum::<f64>() / n,
            v.iter().map(|f| f.first_examples as f64).sum::<f64>() / n,
            v.iter().map(|f| f.rules as f64).sum::<f64>() / n,
            seqs.iter().filter(|f| f.rules > 0).count() as f64 / seqs.len().max(1) as f64,
            median(seqs.iter().filter_map(|f| f.attempts_before_rule.map(|a| a as f64)).collect())
        );
    }
}
