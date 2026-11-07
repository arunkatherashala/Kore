//! Track B7: does using the discovered rules raise reward? Measures the B7 switches (`world2/rules_use.rs`)
//! against the current learner and oracle-knows-rules in World v2 and prints the tables recorded in
//! docs/measured.md (section "Track B7"). Synthetic world, author-defined families.
//!   cargo run -p kore-self --example measure_rules_use --offline --release -- [tune|report]
//! Tuning seeds 101..=120 (used only to choose `rule_k` and the robust threshold); report seeds 1..=20.
//! Extra arguments for `report`: rule_k, robust threshold in per mille.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::measure::mean_ci;
use being::world2::baselines::Oracle2;
use being::world2::engine::LearnerCfg;
use being::world2::learner::Learner2;
use being::world2::measure::{run_w2, run_w2_with, W2Run};
use being::world2::rules_use::UseCfg;
use being::world2::world::{FType, W2Cfg, World2};
use rayon::prelude::*;

const EVERY: u64 = 10;

#[derive(Clone, Copy)]
struct Cond {
    name: &'static str,
    families: usize,
    budget: i64,
    noise_pm: u32,
    drift: u32,
    curve: u32,
}

impl Cond {
    fn cfg(&self, seed: u64) -> W2Cfg {
        W2Cfg { noise_pm: self.noise_pm, drift_per_10k: self.drift, curve_pct: self.curve, ..W2Cfg::new(seed, self.families, self.budget) }
    }
}

fn conds() -> Vec<Cond> {
    let mut v = vec![];
    for (size, families, budget) in [("small", 6usize, 1500i64), ("medium", 12, 3000), ("large", 24, 6000)] {
        for (kind, noise_pm, drift, curve) in [("clean", 0u32, 0u32, 0u32), ("noisy", 100, 0, 0), ("drift", 0, 20, 0), ("hard", 100, 20, 100)] {
            v.push(Cond { name: Box::leak(format!("{size} {kind}").into_boxed_str()), families, budget, noise_pm, drift, curve });
        }
    }
    v
}

#[derive(Clone, Copy)]
enum Pol {
    NoDiscovery,
    Learner,
    Use(UseCfg),
    OracleRules,
    /// The B7 switches without any discovery pass (with A2: no `discovered` arm at all).
    UseNoDisc(UseCfg),
}

fn run(p: Pol, cfg: &W2Cfg) -> W2Run {
    match p {
        Pol::NoDiscovery => run_w2(cfg, &mut Learner2::default(), 0, None),
        Pol::Learner => run_w2(cfg, &mut Learner2::default(), EVERY, None),
        Pol::Use(u) => {
            let lc = LearnerCfg { use_: u, ..LearnerCfg::default() };
            run_w2_with(cfg, &mut lc.learner(), EVERY, None, lc)
        }
        Pol::OracleRules => run_w2(cfg, &mut Oracle2::new(World2::new(cfg.clone()), true), 0, None),
        Pol::UseNoDisc(u) => {
            let lc = LearnerCfg { use_: u, ..LearnerCfg::default() };
            run_w2_with(cfg, &mut lc.learner(), 0, None, lc)
        }
    }
}

fn rows(cond: &Cond, seeds: &[u64], pols: &[Pol]) -> Vec<Vec<W2Run>> {
    seeds.par_iter().map(|s| pols.iter().map(|p| run(*p, &cond.cfg(*s))).collect()).collect()
}

fn col(rs: &[Vec<W2Run>], i: usize) -> Vec<f64> {
    rs.iter().map(|r| r[i].per_energy()).collect()
}

fn fmt_ci(xs: &[f64]) -> String {
    let (m, lo, hi) = mean_ci(xs);
    format!("{m:.3} [{lo:.3}, {hi:.3}]")
}

fn fmt_diff(a: &[f64], b: &[f64]) -> String {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let (m, lo, hi) = mean_ci(&d);
    let w = d.iter().filter(|x| **x > 1e-9).count();
    let l = d.iter().filter(|x| **x < -1e-9).count();
    format!("{m:+.3} [{lo:+.3}, {hi:+.3}] {w}/{l}/{}", d.len() - w - l)
}

struct RuleStats {
    seq_rules: usize,
    seq_wrong: usize,
    const_rules: usize,
    const_wrong: usize,
    seq_answered: usize,
    seq_answered_right: usize,
    const_answered: usize,
    const_answered_right: usize,
}

fn rule_stats(runs: &[&W2Run]) -> RuleStats {
    let mut s = RuleStats { seq_rules: 0, seq_wrong: 0, const_rules: 0, const_wrong: 0, seq_answered: 0, seq_answered_right: 0, const_answered: 0, const_answered_right: 0 };
    for r in runs {
        s.seq_rules += r.rules.len();
        s.seq_wrong += r.rules.iter().filter(|x| x.wrong_at_accept).count();
        s.const_rules += r.consts.len();
        s.const_wrong += r.consts.iter().filter(|x| x.wrong_at_accept).count();
        for x in r.recs.iter().filter(|x| x.attempted && x.rule_used) {
            if x.ftype == Some(FType::Formula) {
                s.const_answered += 1;
                s.const_answered_right += x.true_correct as usize;
            } else {
                s.seq_answered += 1;
                s.seq_answered_right += x.true_correct as usize;
            }
        }
    }
    s
}

/// A variant by name: `A` rule estimate, `A2` discovered only where a rule applies, `B` robust acceptance,
/// `C` fitted formula constants, `G` fitted grid move rules, joined with `+`.
fn ucfg(name: &str, rule_k: u32, theta: u32) -> UseCfg {
    let mut u = UseCfg { rule_k, ..UseCfg::default() };
    for p in name.split('+') {
        match p {
            "A" => u.rule_est = true,
            "A2" => u.rule_only = true,
            "B" => u.robust_milli = theta,
            "C" => u.fit_const = true,
            "G" => u.fit_grid = true,
            _ => panic!("unknown switch {p}"),
        }
    }
    u
}

fn tune() {
    let seeds: Vec<u64> = (101..=120).collect();
    let cs: Vec<Cond> = conds().into_iter().filter(|c| c.name.starts_with("medium")).collect();
    println!("### Tuning (seeds 101..120; medium conditions)
");
    println!("| variant | condition | true reward per energy | minus learner | seq rules (wrong) | constants (wrong) | rule-answered (right) |");
    println!("|---|---|---|---|---|---|---|");
    let mut vars: Vec<(String, UseCfg)> = vec![];
    for k in [2u32, 4, 8, 16, 32] {
        vars.push((format!("A+A2 rule_k {k}"), ucfg("A+A2", k, 0)));
    }
    vars.push(("A (8)".into(), ucfg("A", 8, 0)));
    for t in [300u32, 400, 500, 600, 700, 800] {
        vars.push((format!("A+A2+B (8) theta {t}"), ucfg("A+A2+B", 8, t)));
    }
    for v in ["C", "G", "A+C+G", "A+A2+C", "A+A2+G", "A+A2+C+G", "A+A2+B+C+G"] {
        vars.push((format!("{v} (8, theta 500)"), ucfg(v, 8, 500)));
    }
    for c in &cs {
        let mut pols = vec![Pol::Learner];
        pols.extend(vars.iter().map(|v| Pol::Use(v.1)));
        let rs = rows(c, &seeds, &pols);
        let base = col(&rs, 0);
        println!("| learner | {} | {} | | | | |", c.name, fmt_ci(&base));
        for (i, v) in vars.iter().enumerate() {
            let x = col(&rs, i + 1);
            let runs: Vec<&W2Run> = rs.iter().map(|r| &r[i + 1]).collect();
            let s = rule_stats(&runs);
            println!("| {} | {} | {} | {} | {} ({}) | {} ({}) | {} ({}) |", v.0, c.name, fmt_ci(&x), fmt_diff(&x, &base), s.seq_rules, s.seq_wrong, s.const_rules, s.const_wrong, s.seq_answered + s.const_answered, s.seq_answered_right + s.const_answered_right);
        }
    }
}

const VARIANTS: [&str; 9] = ["A", "A+A2", "B", "A+B", "C", "G", "A+C+G", "A+B+C+G", "A+A2+B+C+G"];

fn report(rule_k: u32, theta: u32) {
    let seeds: Vec<u64> = (1..=20).collect();
    let mut names: Vec<String> = vec!["no discovery".into(), "learner (current)".into()];
    let mut pols = vec![Pol::NoDiscovery, Pol::Learner];
    for v in VARIANTS {
        names.push(v.into());
        pols.push(Pol::Use(ucfg(v, rule_k, theta)));
    }
    names.push("oracle-knows-rules".into());
    pols.push(Pol::OracleRules);
    let (il, ifull, io) = (1usize, pols.len() - 2, pols.len() - 1);
    println!("Track B7 report (seeds 1..20, n = 20 worlds per condition; rule_k {rule_k}, robust threshold {theta} per mille); score = TRUE reward per energy
");
    let all = conds();
    let results: Vec<Vec<Vec<W2Run>>> = all.iter().map(|cd| rows(cd, &seeds, &pols)).collect();
    println!("### True reward per energy, mean [95% interval]
");
    println!("| condition | {} |", names.join(" | "));
    println!("|---|{}", "---|".repeat(names.len()));
    for (ci, cd) in all.iter().enumerate() {
        print!("| {} |", cd.name);
        for i in 0..names.len() {
            print!(" {} |", fmt_ci(&col(&results[ci], i)));
        }
        println!();
    }
    println!("
### Paired difference to the current learner, mean [95% interval] better/worse/tie of 20
");
    let vs: Vec<usize> = (0..names.len()).filter(|i| *i != il).collect();
    println!("| condition | {} |", vs.iter().map(|i| format!("{} - learner", names[*i])).collect::<Vec<_>>().join(" | "));
    println!("|---|{}", "---|".repeat(vs.len()));
    for (ci, cd) in all.iter().enumerate() {
        print!("| {} |", cd.name);
        for &i in &vs {
            print!(" {} |", fmt_diff(&col(&results[ci], i), &col(&results[ci], il)));
        }
        println!();
    }
    println!("
### oracle-knows-rules minus learner and minus the full variant ({}); share of oracle-knows-rules
", names[ifull]);
    println!("| condition | oracle - learner | oracle - {} | learner / oracle | {} / oracle |", names[ifull], names[ifull]);
    println!("|---|---|---|---|---|");
    for (ci, cd) in all.iter().enumerate() {
        let r = &results[ci];
        let sh = |i: usize| fmt_ci(&r.iter().map(|x| x[i].per_energy() / x[io].per_energy().max(1e-9)).collect::<Vec<_>>());
        println!("| {} | {} | {} | {} | {} |", cd.name, fmt_diff(&col(r, io), &col(r, il)), fmt_diff(&col(r, io), &col(r, ifull)), sh(il), sh(ifull));
    }
    println!("
### Rules: seq rules accepted (wrong on fresh tasks at acceptance), constants accepted (wrong); attempts answered by a rule (right by the checker); 20 worlds pooled
");
    println!("| condition | {} |", names[1..io].join(" | "));
    println!("|---|{}", "---|".repeat(io - 1));
    for (ci, cd) in all.iter().enumerate() {
        print!("| {} |", cd.name);
        for i in 1..io {
            let runs: Vec<&W2Run> = results[ci].iter().map(|r| &r[i]).collect();
            let s = rule_stats(&runs);
            print!(" {} ({}), {} ({}); {} ({}) |", s.seq_rules, s.seq_wrong, s.const_rules, s.const_wrong, s.seq_answered + s.const_answered, s.seq_answered_right + s.const_answered_right);
        }
        println!();
    }
    println!("
### Rules right on fewer than 90% of fresh tasks at the end of the life (pooled; drift makes rules stale)
");
    for (ci, cd) in all.iter().enumerate() {
        let mut line = format!("{}:", cd.name);
        for i in [il, ifull] {
            let runs: Vec<&W2Run> = results[ci].iter().map(|r| &r[i]).collect();
            let stale: usize = runs.iter().map(|r| r.rules.iter().filter(|x| x.right_share_end.map_or(false, |s| s < 0.9)).count() + r.consts.iter().filter(|x| x.right_share_end.map_or(false, |s| s < 0.9)).count()).sum();
            line.push_str(&format!(" {} {stale};", names[i]));
        }
        println!("{line}");
    }
}

/// What A2 does without rules: the learner with no `discovered` arm at all (no discovery), against the current learner.
fn ablation(rule_k: u32, theta: u32) {
    let seeds: Vec<u64> = (1..=20).collect();
    let names = ["learner (current)", "A2 alone", "no discovered arm (A2, no discovery)", "A+A2", "A+C+G", "A+A2+C+G", "C+G, no discovered arm for seq (A2+C+G)"];
    let pols = [
        Pol::Learner,
        Pol::Use(ucfg("A2", rule_k, theta)),
        Pol::UseNoDisc(ucfg("A2", rule_k, theta)),
        Pol::Use(ucfg("A+A2", rule_k, theta)),
        Pol::Use(ucfg("A+C+G", rule_k, theta)),
        Pol::Use(ucfg("A+A2+C+G", rule_k, theta)),
        Pol::Use(ucfg("A2+C+G", rule_k, theta)),
    ];
    println!("### Ablation of A2 (seeds 1..20, n = 20; paired differences, mean [95% interval] better/worse/tie)\n");
    println!("| condition | {} |", names[1..].iter().map(|n| format!("{n} - learner")).collect::<Vec<_>>().join(" | "));
    println!("|---|{}", "---|".repeat(names.len() - 1));
    let mut extra = vec![];
    for cd in conds() {
        let rs = rows(&cd, &seeds, &pols);
        print!("| {} |", cd.name);
        for i in 1..names.len() {
            print!(" {} |", fmt_diff(&col(&rs, i), &col(&rs, 0)));
        }
        println!();
        extra.push((cd.name, fmt_diff(&col(&rs, 3), &col(&rs, 2)), fmt_diff(&col(&rs, 5), &col(&rs, 2)), fmt_diff(&col(&rs, 5), &col(&rs, 4))));
    }
    println!("\n| condition | A+A2 - no discovered arm | A+A2+C+G - no discovered arm | A+A2+C+G - A+C+G |");
    println!("|---|---|---|---|");
    for (n, a, b, c) in extra {
        println!("| {n} | {a} | {b} | {c} |");
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "report".into());
    let rule_k: u32 = std::env::args().nth(2).and_then(|x| x.parse().ok()).unwrap_or(being::world2::rules_use::DEFAULT_RULE_K);
    let theta: u32 = std::env::args().nth(3).and_then(|x| x.parse().ok()).unwrap_or(400);
    match mode.as_str() {
        "tune" => tune(),
        "ablation" => ablation(rule_k, theta),
        "dist" => {
            let v = std::env::args().nth(4).unwrap_or_else(|| "A+A2".into());
            let cname = std::env::args().nth(5).unwrap_or_else(|| "medium clean".into());
            let cd = conds().into_iter().find(|c| c.name == cname).unwrap();
            let seeds: Vec<u64> = (101..=120).collect();
            let rs = rows(&cd, &seeds, &[Pol::Learner, Pol::Use(ucfg(&v, rule_k, theta)), Pol::OracleRules]);
            for (s, r) in seeds.iter().zip(&rs) {
                println!("seed {s}: learner {:.3} {v} {:.3} oracle {:.3} diff {:+.3} skips {} / {} steps {} / {}", r[0].per_energy(), r[1].per_energy(), r[2].per_energy(), r[1].per_energy() - r[0].per_energy(), r[0].skips, r[1].skips, r[0].steps, r[1].steps);
                if std::env::args().any(|a| a == "detail") && (r[1].per_energy() - r[0].per_energy()).abs() > 0.3 {
                    for k in 0..2 {
                        let mut g: std::collections::BTreeMap<String, (i64, i64, u32)> = Default::default();
                        for x in &r[k].recs {
                            let key = format!("{:?}/{}/{}", x.ftype, x.strategy.map_or("skip", |s| s.name()), if x.rule_used { "rule" } else { "-" });
                            let e = g.entry(key).or_default();
                            e.0 += x.cost;
                            e.1 += x.true_reward;
                            e.2 += 1;
                        }
                        let q = r[k].window_e(0, cd.budget / 4);
                        println!("   {} first-quarter rate {q:.3}: {:?}", if k == 0 { "learner" } else { "variant" }, g.iter().filter(|x| x.1 .0 > 100).map(|(k, v)| format!("{k} e{} r{:.2} n{}", v.0, v.1 as f64 / v.0.max(1) as f64, v.2)).collect::<Vec<_>>());
                    }
                }
            }
        }
        _ => report(rule_k, theta),
    }
}
