//! Track B10: all skills together in World v2. Measures the "all skills" profile (B7 switches A, A2, B, C, G;
//! B9 experiments; B3b adaptation; rule retirement; optionally B8 lookahead) against the current learner, B7
//! alone and oracle-knows-rules, with the ablations that show how the parts interact, and prints the tables
//! recorded in docs/measured.md (section "Track B10"). Synthetic world, author-defined families.
//!   cargo run -p kore-self --example measure_all_skills --offline --release -- [tune|report]
//! Tuning seeds 101..=120 (used only to choose the retirement threshold and the experiment setting); report seeds 1..=20.
//! `report` takes optional arguments: retirement upper bound (per mille), window, minimum answers, kappa (x1000), experiment budget (per mille).

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::experiment::ExpCfg;
use being::lookahead::adapt::Look2;
use being::lookahead::plan::{LookCfg, Mode};
use being::measure::mean_ci;
use being::world2::all_skills::profile_use;
use being::world2::baselines::Oracle2;
use being::world2::engine::LearnerCfg;
use being::world2::experiment::Exp2;
use being::world2::learner::Learner2;
use being::world2::measure::{run_w2, run_w2_with, W2Run};
use being::world2::skills::SkillsCfg;
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

/// Strong drift (for the retirement measurement): 60 changes per 10 000 energy.
fn drift_conds() -> Vec<Cond> {
    vec![
        Cond { name: "medium drift60", families: 12, budget: 3000, noise_pm: 0, drift: 60, curve: 0 },
        Cond { name: "large drift60", families: 24, budget: 6000, noise_pm: 0, drift: 60, curve: 0 },
        Cond { name: "large hard60", families: 24, budget: 6000, noise_pm: 100, drift: 60, curve: 100 },
    ]
}

#[derive(Clone, Copy)]
struct Set {
    retire: SkillsCfg,
    kappa: f64,
    exp_pm: u32,
}

impl Default for Set {
    fn default() -> Self {
        Set { retire: SkillsCfg { retire: true, ..SkillsCfg::default() }, kappa: being::world2::all_skills::PROFILE_KAPPA, exp_pm: being::world2::all_skills::PROFILE_EXP_PM }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Pol {
    Learner,
    /// Passive learner plus experiments (B9 alone).
    B9,
    B7,
    B7B9,
    B7Adapt,
    B7Retire,
    B7AdaptRetire,
    /// B7 + B9 + B3b + retirement.
    All,
    /// All + B8 lookahead (hybrid, depth 2).
    AllLook,
    /// The current learner plus retirement only.
    Retire,
    OracleRules,
}

impl Pol {
    fn name(&self) -> &'static str {
        match self {
            Pol::Learner => "learner (current)",
            Pol::B9 => "B9 alone",
            Pol::B7 => "B7-all",
            Pol::B7B9 => "B7+B9",
            Pol::B7Adapt => "B7+B3b",
            Pol::B7Retire => "B7+retire",
            Pol::B7AdaptRetire => "B7+B3b+retire",
            Pol::All => "all (B7+B9+B3b+retire)",
            Pol::AllLook => "all + B8",
            Pol::Retire => "learner+retire",
            Pol::OracleRules => "oracle-knows-rules",
        }
    }
}

fn look_cfg() -> LookCfg {
    let mut c = LookCfg::new(2, being::lookahead::ctl::DEFAULT_NODES);
    c.mode = Mode::Hybrid;
    c
}

fn run(p: Pol, cfg: &W2Cfg, s: &Set) -> W2Run {
    let lc = |use_: bool, adapt: bool, retire: bool| LearnerCfg {
        use_: if use_ { profile_use() } else { Default::default() },
        skills: SkillsCfg { adapt, retire, ..s.retire },
        ..LearnerCfg::default()
    };
    let exp = ExpCfg::eig(s.kappa, s.exp_pm, 5000);
    let with = |l: LearnerCfg, e: bool, look: bool| -> W2Run {
        if e {
            let mut x = Exp2::new(l.learner(), exp, World2::new(cfg.clone()), cfg.budget);
            if look {
                x.look = Some(Look2::with_learner(Some(look_cfg()), l.learner()));
            }
            run_w2_with(cfg, &mut x, EVERY, None, l)
        } else {
            run_w2_with(cfg, &mut l.learner(), EVERY, None, l)
        }
    };
    match p {
        Pol::Learner => run_w2(cfg, &mut Learner2::default(), EVERY, None),
        Pol::B9 => with(lc(false, false, false), true, false),
        Pol::B7 => with(lc(true, false, false), false, false),
        Pol::B7B9 => with(lc(true, false, false), true, false),
        Pol::B7Adapt => with(lc(true, true, false), false, false),
        Pol::B7Retire => with(lc(true, false, true), false, false),
        Pol::B7AdaptRetire => with(lc(true, true, true), false, false),
        Pol::All => with(lc(true, true, true), true, false),
        Pol::AllLook => with(lc(true, true, true), true, true),
        Pol::Retire => with(lc(false, false, true), false, false),
        Pol::OracleRules => run_w2(cfg, &mut Oracle2::new(World2::new(cfg.clone()), true), 0, None),
    }
}

fn rows(cond: &Cond, seeds: &[u64], pols: &[Pol], s: &Set) -> Vec<Vec<W2Run>> {
    seeds.par_iter().map(|sd| pols.iter().map(|p| run(*p, &cond.cfg(*sd), s)).collect()).collect()
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

#[derive(Default, Clone, Copy)]
struct RStats {
    seq: usize,
    seq_wrong: usize,
    consts: usize,
    consts_wrong: usize,
    /// live (not retired) at the end of the life and right on fewer than 90% of fresh tasks then
    stale_end: usize,
    retired: usize,
    /// retired while right on fewer than 90% of fresh tasks (a stale rule) / at least 90% (a false retirement)
    retired_stale: usize,
    retired_ok: usize,
    answered: usize,
    answered_right: usize,
    experiments: usize,
    changes: usize,
}

fn rstats(runs: &[&W2Run]) -> RStats {
    let mut s = RStats::default();
    for r in runs {
        let retired: Vec<&str> = r.retired.iter().map(|x| x.rule_id.as_str()).collect();
        s.seq += r.rules.len();
        s.seq_wrong += r.rules.iter().filter(|x| x.wrong_at_accept).count();
        s.consts += r.consts.len();
        s.consts_wrong += r.consts.iter().filter(|x| x.wrong_at_accept).count();
        s.stale_end += r.rules.iter().filter(|x| !retired.contains(&x.rule.id.as_str()) && x.right_share_end.map_or(false, |v| v < 0.9)).count();
        s.stale_end += r.consts.iter().filter(|x| !retired.contains(&x.rule.id.as_str()) && x.right_share_end.map_or(false, |v| v < 0.9)).count();
        s.retired += r.retired.len();
        s.retired_stale += r.retired.iter().filter(|x| x.right_share_then.map_or(true, |v| v < 0.9)).count();
        s.retired_ok += r.retired.iter().filter(|x| x.right_share_then.map_or(false, |v| v >= 0.9)).count();
        for x in r.recs.iter().filter(|x| x.attempted && x.rule_used) {
            s.answered += 1;
            s.answered_right += x.true_correct as usize;
        }
        s.changes += r.state.changes.len();
    }
    s
}

fn print_rules(names: &[&str], results: &[(Cond, Vec<Vec<W2Run>>)], idx: &[usize]) {
    println!("\n### Rules (20 worlds pooled): sequence rules accepted (wrong at acceptance), constants (wrong); stale at the end (live, right on < 90% of fresh tasks); retired (of them stale at the time / still right); answers by a rule (right by the checker); B3b changes\n");
    println!("| condition | arm | seq (wrong) | constants (wrong) | stale at end | retired (stale / still right) | rule answers (right) | B3b changes |");
    println!("|---|---|---|---|---|---|---|---|");
    for (cd, rs) in results {
        for &i in idx {
            let runs: Vec<&W2Run> = rs.iter().map(|r| &r[i]).collect();
            let s = rstats(&runs);
            println!(
                "| {} | {} | {} ({}) | {} ({}) | {} | {} ({} / {}) | {} ({:.1}%) | {} |",
                cd.name, names[i], s.seq, s.seq_wrong, s.consts, s.consts_wrong, s.stale_end, s.retired, s.retired_stale, s.retired_ok, s.answered, 100.0 * s.answered_right as f64 / s.answered.max(1) as f64, s.changes
            );
        }
    }
}

fn share_on(r: &W2Run, t: FType) -> f64 {
    let tot: i64 = r.recs.iter().map(|x| x.cost).sum();
    let on: i64 = r.recs.iter().filter(|x| x.ftype == Some(t)).map(|x| x.cost).sum();
    on as f64 / tot.max(1) as f64
}

fn tune() {
    let seeds: Vec<u64> = (101..=120).collect();
    // retirement threshold, under drift (where it matters) and clean (where it must not hurt)
    let cs: Vec<Cond> = conds().into_iter().filter(|c| c.name == "medium clean" || c.name == "medium drift" || c.name == "large drift" || c.name == "medium hard").chain(drift_conds().into_iter().take(1)).collect();
    println!("### Tuning retirement (seeds 101..120): B7+retire minus B7-all, reward per energy; stale rules at end; retired (stale / still right)\n");
    println!("| setting | condition | B7+retire | minus B7-all | stale at end: B7-all / B7+retire | retired (stale / still right) |");
    println!("|---|---|---|---|---|---|");
    for (ub, w, m) in [(300u32, 10u32, 6u32), (400, 10, 6), (500, 10, 6), (600, 10, 6), (500, 6, 4), (500, 16, 8)] {
        let s = Set { retire: SkillsCfg { retire: true, retire_window: w, retire_min: m, retire_ub_milli: ub, adapt: false }, ..Set::default() };
        for c in &cs {
            let rs = rows(c, &seeds, &[Pol::B7, Pol::B7Retire], &s);
            let (a, b) = (col(&rs, 0), col(&rs, 1));
            let r0: Vec<&W2Run> = rs.iter().map(|r| &r[0]).collect();
            let r1: Vec<&W2Run> = rs.iter().map(|r| &r[1]).collect();
            let (s0, s1) = (rstats(&r0), rstats(&r1));
            println!("| ub {ub} w {w} min {m} | {} | {} | {} | {} / {} | {} ({} / {}) |", c.name, fmt_ci(&b), fmt_diff(&b, &a), s0.stale_end, s1.stale_end, s1.retired, s1.retired_stale, s1.retired_ok);
        }
    }
    // experiment setting with B7 on
    let cs: Vec<Cond> = conds().into_iter().filter(|c| c.name.starts_with("medium") || c.name == "large clean").collect();
    println!("\n### Tuning experiments on top of B7 (seeds 101..120): B7+B9 minus B7-all\n");
    println!("| kappa / budget pm | condition | B7+B9 | minus B7-all |");
    println!("|---|---|---|---|");
    let mut sums = vec![];
    for (k, pm) in [(0.1f64, 50u32), (0.25, 50), (0.25, 100), (1.0, 50), (1.0, 100)] {
        let s = Set { kappa: k, exp_pm: pm, ..Set::default() };
        let mut sum = 0.0;
        for c in &cs {
            let rs = rows(c, &seeds, &[Pol::B7, Pol::B7B9], &s);
            let (a, b) = (col(&rs, 0), col(&rs, 1));
            sum += mean_ci(&b.iter().zip(&a).map(|(x, y)| x - y).collect::<Vec<_>>()).0;
            println!("| {k} / {pm} | {} | {} | {} |", c.name, fmt_ci(&b), fmt_diff(&b, &a));
        }
        sums.push((k, pm, sum));
    }
    println!("\nsum of mean differences over the conditions: {}", sums.iter().map(|(k, p, s)| format!("{k}/{p}: {s:+.3}")).collect::<Vec<_>>().join(", "));
}

fn report(s: Set, with_look: bool) {
    let seeds: Vec<u64> = (1..=20).collect();
    let mut pols = vec![Pol::Learner, Pol::B9, Pol::B7, Pol::B7B9, Pol::B7Adapt, Pol::B7Retire, Pol::B7AdaptRetire, Pol::All];
    if with_look {
        pols.push(Pol::AllLook);
    }
    pols.push(Pol::OracleRules);
    let names: Vec<&str> = pols.iter().map(|p| p.name()).collect();
    let ix = |p: Pol| pols.iter().position(|x| *x == p).expect("arm");
    let io = ix(Pol::OracleRules);
    println!(
        "Track B10 report (seeds 1..20, n = 20 worlds per condition); score = TRUE reward per energy. Retirement: window {}, at least {}, upper bound {} per mille; experiments kappa {}, {} per mille; lookahead hybrid depth 2\n",
        s.retire.retire_window, s.retire.retire_min, s.retire.retire_ub_milli, s.kappa, s.exp_pm
    );
    let results: Vec<(Cond, Vec<Vec<W2Run>>)> = conds().into_iter().map(|cd| {
        let r = rows(&cd, &seeds, &pols, &s);
        eprintln!("done {}", cd.name);
        (cd, r)
    }).collect();
    println!("### True reward per energy, mean [95% interval]\n");
    println!("| condition | {} |", names.join(" | "));
    println!("|---|{}", "---|".repeat(names.len()));
    for (cd, rs) in &results {
        print!("| {} |", cd.name);
        for i in 0..names.len() {
            print!(" {} |", fmt_ci(&col(rs, i)));
        }
        println!();
    }
    let mut pairs: Vec<(usize, usize)> = vec![(ix(Pol::All), ix(Pol::Learner)), (ix(Pol::All), ix(Pol::B7)), (io, ix(Pol::All)), (ix(Pol::B9), ix(Pol::Learner)), (ix(Pol::B7B9), ix(Pol::B7)), (ix(Pol::B7Adapt), ix(Pol::B7)), (ix(Pol::B7Retire), ix(Pol::B7)), (ix(Pol::B7AdaptRetire), ix(Pol::B7)), (ix(Pol::All), ix(Pol::B7AdaptRetire))];
    if with_look {
        pairs.push((ix(Pol::AllLook), ix(Pol::All)));
    }
    println!("\n### Paired differences, mean [95% interval] better/worse/tie of 20\n");
    println!("| condition | {} |", pairs.iter().map(|(a, b)| format!("{} - {}", names[*a], names[*b])).collect::<Vec<_>>().join(" | "));
    println!("|---|{}", "---|".repeat(pairs.len()));
    for (cd, rs) in &results {
        print!("| {} |", cd.name);
        for (a, b) in &pairs {
            print!(" {} |", fmt_diff(&col(rs, *a), &col(rs, *b)));
        }
        println!();
    }
    println!("\n### Interaction: B9 on top of B7 minus B9 on top of the current learner ((B7+B9 - B7) - (B9 - learner)), and share of oracle-knows-rules\n");
    println!("| condition | interaction | learner / oracle | B7-all / oracle | all / oracle |");
    println!("|---|---|---|---|---|");
    for (cd, rs) in &results {
        let inter: Vec<f64> = rs.iter().map(|r| (r[ix(Pol::B7B9)].per_energy() - r[ix(Pol::B7)].per_energy()) - (r[ix(Pol::B9)].per_energy() - r[ix(Pol::Learner)].per_energy())).collect();
        let sh = |i: usize| fmt_ci(&rs.iter().map(|x| x[i].per_energy() / x[io].per_energy().max(1e-9)).collect::<Vec<_>>());
        let (m, lo, hi) = mean_ci(&inter);
        println!("| {} | {m:+.3} [{lo:+.3}, {hi:+.3}] | {} | {} | {} |", cd.name, sh(ix(Pol::Learner)), sh(ix(Pol::B7)), sh(ix(Pol::All)));
    }
    let idx: Vec<usize> = (0..io).collect();
    print_rules(&names, &results, &idx);
    println!("\n### Energy share on formula and sequence families (mean over worlds)\n");
    println!("| condition | {} |", names.join(" | "));
    println!("|---|{}", "---|".repeat(names.len()));
    for (cd, rs) in &results {
        print!("| {} |", cd.name);
        for i in 0..names.len() {
            let f: f64 = rs.iter().map(|r| share_on(&r[i], FType::Formula)).sum::<f64>() / rs.len() as f64;
            let q: f64 = rs.iter().map(|r| share_on(&r[i], FType::Seq) + share_on(&r[i], FType::Compose)).sum::<f64>() / rs.len() as f64;
            print!(" {f:.3} / {q:.3} |");
        }
        println!();
    }
    // strong drift: retirement
    let dpols = vec![Pol::Learner, Pol::Retire, Pol::B7, Pol::B7Retire, Pol::All, Pol::OracleRules];
    let dnames: Vec<&str> = dpols.iter().map(|p| p.name()).collect();
    let dres: Vec<(Cond, Vec<Vec<W2Run>>)> = drift_conds().into_iter().map(|cd| {
        let r = rows(&cd, &seeds, &dpols, &s);
        (cd, r)
    }).collect();
    println!("\n### Strong drift (60 changes per 10 000 energy), reward per energy and paired differences\n");
    println!("| condition | {} | learner+retire - learner | B7+retire - B7-all | all - B7-all |", dnames.join(" | "));
    println!("|---|{}---|---|---|", "---|".repeat(dnames.len()));
    for (cd, rs) in &dres {
        print!("| {} |", cd.name);
        for i in 0..dnames.len() {
            print!(" {} |", fmt_ci(&col(rs, i)));
        }
        println!(" {} | {} | {} |", fmt_diff(&col(rs, 1), &col(rs, 0)), fmt_diff(&col(rs, 3), &col(rs, 2)), fmt_diff(&col(rs, 4), &col(rs, 2)));
    }
    print_rules(&dnames, &dres, &[0, 1, 2, 3, 4]);
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let num = |i: usize, d: u32| a.get(i).and_then(|x| x.parse::<u32>().ok()).unwrap_or(d);
    match a.first().map(|s| s.as_str()) {
        Some("tune") => tune(),
        Some("report") | Some("report-look") | None => {
            let d = Set::default();
            let s = Set {
                retire: SkillsCfg { retire_ub_milli: num(1, d.retire.retire_ub_milli), retire_window: num(2, d.retire.retire_window), retire_min: num(3, d.retire.retire_min), ..d.retire },
                kappa: num(4, (d.kappa * 1000.0).round() as u32) as f64 / 1000.0,
                exp_pm: num(5, d.exp_pm),
            };
            report(s, a.first().map_or(false, |x| x == "report-look"));
        }
        Some(x) => eprintln!("unknown mode {x}; use tune, report or report-look"),
    }
}
