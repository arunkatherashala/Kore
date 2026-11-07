//! `measure_b11 worldo`: learning curves, regret and time to competence in World O.

use crate::being;
use crate::being::controller::Limits;
use crate::being::measure::mean_ci;
use crate::being::planner::SkipRule;
use crate::being::world2::rules_use::UseCfg;
use crate::being::world_o::engine::LearnerOCfg;
use crate::being::world_o::measure::{run_wo, FamRec, OracleO, RunWO, TruthO, COMP_K, COMP_LEVEL};
use crate::being::world_o::world::{WOCfg, WorldO, MAX_TIER};
use rayon::prelude::*;

pub const CONDS: [&str; 6] = ["current", "fix", "b7", "fix+b7", "oracle-planner", "oracle-knows-rules"];

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

fn ci(xs: &[f64]) -> String {
    if xs.is_empty() {
        return "-".into();
    }
    let (m, lo, hi) = mean_ci(xs);
    format!("{m:.3} [{lo:.3}, {hi:.3}]")
}

/// Controller of the World O measurement: no composition (compose families reuse a base rule as it
/// is), a rule cap and a candidate cap that do not bind in these runs (both are recorded).
pub fn wo_limits() -> Limits {
    Limits { max_live_rules: 200, max_candidates: 100_000, compose: false, max_blocks: 8, ..Limits::default() }
}

pub fn learner(cond: &str) -> LearnerOCfg {
    let b7 = UseCfg { rule_est: true, rule_only: true, ..UseCfg::default() };
    let fix = cond.contains("fix");
    let b = cond.contains("b7");
    LearnerOCfg { skip_rule: if fix { SkipRule::Settled(being::planner::DEFAULT_SETTLE_K) } else { SkipRule::Rate }, use_: if b { b7 } else { UseCfg::default() }, ..LearnerOCfg::default() }
}

pub fn wo_run(cond: &str, c: &WOCfg, steps: u64, window: i64) -> RunWO {
    match cond {
        "current" | "fix" | "b7" | "fix+b7" => run_wo(c, learner(cond), None, 10, Some(wo_limits()), steps, window, true),
        "oracle-planner" | "oracle-knows-rules" => {
            let mut o = OracleO { world: WorldO::new(c.clone()), truth: TruthO::default(), knows_rules: cond == "oracle-knows-rules" };
            run_wo(c, LearnerOCfg::default(), Some(&mut o), 0, None, steps, window, false)
        }
        _ => panic!("unknown condition {cond}"),
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

/// Spearman rank correlation (average ranks for ties).
pub fn spearman(x: &[f64], y: &[f64]) -> f64 {
    let rank = |v: &[f64]| {
        let mut idx: Vec<usize> = (0..v.len()).collect();
        idx.sort_by(|a, b| v[*a].partial_cmp(&v[*b]).unwrap());
        let mut r = vec![0.0; v.len()];
        let mut i = 0;
        while i < idx.len() {
            let mut j = i;
            while j + 1 < idx.len() && v[idx[j + 1]] == v[idx[i]] {
                j += 1;
            }
            for k in i..=j {
                r[idx[k]] = (i + j) as f64 / 2.0;
            }
            i = j + 1;
        }
        r
    };
    let (a, b) = (rank(x), rank(y));
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let cov: f64 = a.iter().zip(&b).map(|(p, q)| (p - ma) * (q - mb)).sum();
    let va: f64 = a.iter().map(|p| (p - ma).powi(2)).sum();
    let vb: f64 = b.iter().map(|q| (q - mb).powi(2)).sum();
    cov / (va * vb).sqrt()
}

pub fn cmd_worldo(args: &[String], ss: Vec<u64>) {
    let budget: i64 = arg(args, "--budget").map_or(300_000, |x| x.parse().unwrap());
    let steps: u64 = arg(args, "--steps").map_or(u64::MAX, |x| x.parse().unwrap());
    let window: i64 = arg(args, "--window").map_or(30_000, |x| x.parse().unwrap());
    let block: u64 = arg(args, "--block").map_or(16, |x| x.parse().unwrap());
    let conds: Vec<String> = arg(args, "--conds").map_or(CONDS.iter().map(|s| s.to_string()).collect(), |x| x.split(',').map(|s| s.to_string()).collect());
    let mk = |s: u64| WOCfg::new(s, budget);
    let t0 = std::time::Instant::now();
    let jobs: Vec<(String, u64)> = conds.iter().flat_map(|c| ss.iter().map(move |s| (c.clone(), *s))).collect();
    let res: Vec<((String, u64), RunWO)> = jobs.par_iter().map(|(c, s)| ((c.clone(), *s), wo_run(c, &mk(*s), steps, window))).collect();
    eprintln!("world O runs: {:.0} s", t0.elapsed().as_secs_f64());
    let get = |c: &str, s: u64| res.iter().find(|r| r.0 .0 == c && r.0 .1 == s).map(|r| &r.1);
    let nwin = res.iter().map(|r| r.1.wins.len()).min().unwrap_or(0);
    println!(
        "World O, default config, budget {budget} energy units, window {window} energy units, seeds {}..{} (n = {}), steps per run {}..{}",
        ss[0], ss[ss.len() - 1], ss.len(), res.iter().map(|r| r.1.steps).min().unwrap_or(0), res.iter().map(|r| r.1.steps).max().unwrap_or(0)
    );
    println!("\n#### True reward per energy per window, mean [95% interval] over worlds\n");
    print!("| energy window |");
    for c in &conds {
        print!(" {c} |");
    }
    println!(" mean tier of attempts ({}) | families opened |", conds[0]);
    print!("|---|");
    for _ in &conds {
        print!("---|");
    }
    println!("---|---|");
    let w = |i: usize| format!("{}k-{}k", i as i64 * window / 1000, (i as i64 + 1) * window / 1000);
    for i in 0..nwin {
        print!("| {} |", w(i));
        for c in &conds {
            let v: Vec<f64> = ss.iter().filter_map(|s| get(c, *s)).map(|r| r.wins[i].rpe()).collect();
            print!(" {} |", ci(&v));
        }
        let cur: Vec<&RunWO> = ss.iter().filter_map(|s| get(&conds[0], *s)).collect();
        let n = cur.len().max(1) as f64;
        let tier = cur.iter().map(|r| r.wins[i].mean_tier_x1000 as f64 / 1000.0).sum::<f64>() / n;
        let fo = cur.iter().map(|r| r.wins[i].families_opened as f64).sum::<f64>() / n;
        println!(" {tier:.2} | {fo:.1} |");
    }
    let learners: Vec<&String> = conds.iter().filter(|c| !c.starts_with("oracle")).collect();
    if conds.iter().any(|c| c == "oracle-knows-rules") {
        println!("\n#### Regret per window vs oracle-knows-rules (paired by world), mean [95% interval]\n");
        print!("| energy window |");
        for c in &learners {
            print!(" {c} |");
        }
        println!();
        print!("|---|");
        for _ in &learners {
            print!("---|");
        }
        println!();
        for i in 0..nwin {
            print!("| {} |", w(i));
            for c in &learners {
                let v: Vec<f64> = ss.iter().filter_map(|s| Some(get("oracle-knows-rules", *s)?.wins[i].rpe() - get(c, *s)?.wins[i].rpe())).collect();
                print!(" {} |", ci(&v));
            }
            println!();
        }
        println!("\n#### Trend: last third of the windows minus the first third (the first window left out), paired by world\n");
        println!("| condition | reward per energy, first third | last third | last minus first | regret, first third | regret, last third | regret change |");
        println!("|---|---|---|---|---|---|---|");
        let third = (nwin.saturating_sub(1) / 3).max(1);
        let span = |r: &RunWO, a: usize, b: usize| {
            let (x, y) = r.wins[a..b].iter().fold((0i64, 0i64), |acc, w| (acc.0 + w.true_reward, acc.1 + w.cost));
            x as f64 / y.max(1) as f64
        };
        for c in &conds {
            let f: Vec<f64> = ss.iter().filter_map(|s| get(c, *s)).map(|r| span(r, 1, 1 + third)).collect();
            let l: Vec<f64> = ss.iter().filter_map(|s| get(c, *s)).map(|r| span(r, nwin - third, nwin)).collect();
            let d: Vec<f64> = f.iter().zip(&l).map(|(a, b)| b - a).collect();
            let rf: Vec<f64> = ss.iter().filter_map(|s| Some(span(get("oracle-knows-rules", *s)?, 1, 1 + third) - span(get(c, *s)?, 1, 1 + third))).collect();
            let rl: Vec<f64> = ss.iter().filter_map(|s| Some(span(get("oracle-knows-rules", *s)?, nwin - third, nwin) - span(get(c, *s)?, nwin - third, nwin))).collect();
            let rd: Vec<f64> = rf.iter().zip(&rl).map(|(a, b)| b - a).collect();
            println!("| {c} | {} | {} | {} | {} | {} | {} |", ci(&f), ci(&l), ci(&d), ci(&rf), ci(&rl), ci(&rd));
        }
        println!("\n#### Paired differences over the whole run (after the first window), mean [95% interval], wins\n");
        println!("| comparison | difference in reward per energy | first better in |");
        println!("|---|---|---|");
        let tot = |r: &RunWO| span(r, 1, nwin);
        for (a, b) in [("fix", "current"), ("b7", "current"), ("fix+b7", "current"), ("fix+b7", "b7")] {
            if conds.iter().any(|c| c == a) && conds.iter().any(|c| c == b) {
                let d: Vec<f64> = ss.iter().filter_map(|s| Some(tot(get(a, *s)?) - tot(get(b, *s)?))).collect();
                let wins = d.iter().filter(|x| **x > 0.0).count();
                println!("| {a} minus {b} | {} | {wins} of {} |", ci(&d), d.len());
            }
        }
    }
    println!("\n#### Rules, skips, caps (per world, mean over worlds)\n");
    println!("| condition | steps | attempt share | rules accepted | rule answers | right share of rule answers | most live rules | refusals |");
    println!("|---|---|---|---|---|---|---|---|");
    for c in &conds {
        let rs: Vec<&RunWO> = ss.iter().filter_map(|s| get(c, *s)).collect();
        let n = rs.len().max(1) as f64;
        let att: f64 = rs.iter().map(|r| r.wins.iter().map(|w| w.attempts).sum::<u64>() as f64 / r.steps.max(1) as f64).sum::<f64>() / n;
        let uses: u64 = rs.iter().map(|r| r.wins.iter().map(|w| w.rule_uses).sum::<u64>()).sum();
        let right: u64 = rs.iter().map(|r| r.wins.iter().map(|w| w.rule_right).sum::<u64>()).sum();
        println!(
            "| {c} | {:.0} | {att:.3} | {:.1} | {:.0} | {:.3} | {:.1} | {:.1} |",
            rs.iter().map(|r| r.steps as f64).sum::<f64>() / n,
            rs.iter().map(|r| r.rules_total as f64).sum::<f64>() / n,
            uses as f64 / n,
            right as f64 / uses.max(1) as f64,
            rs.iter().map(|r| r.max_live_rules as f64).sum::<f64>() / n,
            rs.iter().map(|r| r.refusals as f64).sum::<f64>() / n
        );
    }
    println!("\n#### Rules accepted per window (mean over worlds)\n");
    print!("| energy window |");
    for c in &learners {
        print!(" {c} |");
    }
    println!();
    print!("|---|");
    for _ in &learners {
        print!("---|");
    }
    println!();
    for i in 0..nwin {
        print!("| {} |", w(i));
        for c in &learners {
            let v: Vec<f64> = ss.iter().filter_map(|s| get(c, *s)).map(|r| r.wins[i].rules_accepted as f64).collect();
            print!(" {:.1} |", v.iter().sum::<f64>() / v.len().max(1) as f64);
        }
        println!();
    }
    println!(
        "\n#### Time to competence on each new family, by order of appearance\n\nLearnable family: the mean best success probability the being's means allow on its attempted tasks is at least 0.5. Competent: the last {COMP_K} attempts at the family average at least {COMP_LEVEL} of the best expected success per energy those means allow on the same tasks. Attempts and energy are counted from the family's opening. Pooled over worlds.\n"
    );
    println!("| condition | families (index) | tiers | learnable families | reached competence | median attempts to competence | median energy since opening | mean ratio, first 10 attempts | mean ratio, attempts 11-50 |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for c in &learners {
        let all: Vec<&FamRec> = ss.iter().filter_map(|s| get(c, *s)).flat_map(|r| r.fams.iter()).filter(|f| f.learnable()).collect();
        let maxf = all.iter().map(|f| f.fid).max().unwrap_or(0);
        let mut b = 0;
        while b <= maxf {
            let g: Vec<&&FamRec> = all.iter().filter(|f| f.fid >= b && f.fid < b + block).collect();
            if !g.is_empty() {
                let reached: Vec<&&&FamRec> = g.iter().filter(|f| f.competent_after.is_some()).collect();
                let (tlo, thi) = (g.iter().map(|f| f.tier).min().unwrap(), g.iter().map(|f| f.tier).max().unwrap());
                println!(
                    "| {c} | {}-{} | {tlo}-{thi} | {} | {} ({:.0}%) | {:.0} | {:.0} | {:.3} | {:.3} |",
                    b,
                    b + block - 1,
                    g.len(),
                    reached.len(),
                    100.0 * reached.len() as f64 / g.len() as f64,
                    median(reached.iter().map(|f| f.competent_after.unwrap().0 as f64).collect()),
                    median(reached.iter().map(|f| f.competent_after.unwrap().1 as f64).collect()),
                    mean_ratio(&g, 0, 10),
                    mean_ratio(&g, 10, 50)
                );
            }
            b += block;
        }
    }
    println!("\n#### Does it get faster at learning new things? Top tier ({MAX_TIER}) only: Spearman correlation, per world, of the family index with attempts to competence (not reached counts as slowest); negative = faster later\n");
    println!("| condition | worlds with >= 6 learnable top-tier families | mean [95% interval] |");
    println!("|---|---|---|");
    for c in &learners {
        let mut rhos = vec![];
        for s in &ss {
            if let Some(r) = get(c, *s) {
                let top: Vec<&FamRec> = r.fams.iter().filter(|f| f.learnable() && f.tier >= MAX_TIER).collect();
                if top.len() >= 6 {
                    let x: Vec<f64> = top.iter().map(|f| f.fid as f64).collect();
                    let y: Vec<f64> = top.iter().map(|f| f.competent_after.map_or(1e9, |a| a.0 as f64)).collect();
                    let rho = spearman(&x, &y);
                    if rho.is_finite() {
                        rhos.push(rho);
                    }
                }
            }
        }
        println!("| {c} | {} | {} |", rhos.len(), ci(&rhos));
    }
    let _ = being::world_o::world::SLOTS;
}

/// Mean over families of the mean ratio (chosen option / best the means allow) of attempts `a..b` at the family.
fn mean_ratio(g: &[&&FamRec], a: usize, b: usize) -> f64 {
    let v: Vec<f64> = g.iter().filter(|f| f.ratios.len() > a).map(|f| {
        let s = &f.ratios[a..b.min(f.ratios.len())];
        s.iter().map(|x| *x as f64).sum::<f64>() / s.len() as f64
    }).collect();
    if v.is_empty() { f64::NAN } else { v.iter().sum::<f64>() / v.len() as f64 }
}
