//! `measure_b12` modes.

use crate::being::measure::mean_ci;
use crate::being::world_o::engine::LearnerOCfg;
use crate::being::world_o::measure_b12::{run_b12, FamDiag, RunB12, FIRST_N};
use crate::being::world_o::world::WOCfg;
use crate::worldo::{learner, wo_limits};
use rayon::prelude::*;

pub fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

pub fn seeds(args: &[String], def: (u64, u64)) -> Vec<u64> {
    let s = arg(args, "--seeds").unwrap_or(format!("{}..{}", def.0, def.1));
    let (a, b) = s.split_once("..").expect("--seeds a..b");
    (a.parse().unwrap()..=b.parse().unwrap()).collect()
}

pub fn ci(xs: &[f64]) -> String {
    if xs.is_empty() {
        return "-".into();
    }
    let (m, lo, hi) = mean_ci(xs);
    format!("{m:.3} [{lo:.3}, {hi:.3}]")
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

/// Learner of a condition: `+`-joined parts of `current`, `fix`, `b7`, `prior`, `explore`, `fam`, `form`, `all` (= prior+explore+form).
pub fn learner_b12(cond: &str, t: &Tune) -> LearnerOCfg {
    let parts: Vec<&str> = cond.split('+').collect();
    let has = |x: &str| parts.contains(&x) || (parts.contains(&"all") && ["prior", "explore", "form"].contains(&x));
    let mut base = String::from("current");
    if has("fix") {
        base.push_str("+fix");
    }
    if has("b7") {
        base.push_str("+b7");
    }
    let mut l = learner(&base);
    if has("prior") {
        l.l2l.prior_k = t.prior_k;
    }
    if has("explore") {
        l.l2l.explore_n = t.explore_n;
        l.l2l.explore_w_milli = t.explore_w_milli;
        l.l2l.explore_pm = t.explore_pm;
    }
    if has("form") {
        l.l2l.form_prior = 2;
    } else if has("fam") {
        l.l2l.form_prior = 1;
    }
    l
}

/// The B12 constants of a measurement (defaults: the values chosen on the tuning seeds).
#[derive(Debug, Clone, Copy)]
pub struct Tune {
    pub prior_k: u32,
    pub explore_n: u32,
    pub explore_w_milli: u32,
    pub explore_pm: u32,
}

impl Tune {
    pub fn from_args(args: &[String]) -> Tune {
        let g = |k: &str, d: u32| arg(args, k).map_or(d, |x| x.parse().unwrap());
        Tune { prior_k: g("--prior-k", 8), explore_n: g("--explore-n", 10), explore_w_milli: g("--explore-w", 500), explore_pm: g("--explore-pm", 50) }
    }
}

pub fn run_cond(cond: &str, l: LearnerOCfg, c: &WOCfg, window: i64) -> RunB12 {
    use crate::being::world_o::measure::{OracleO, TruthO};
    use crate::being::world_o::world::WorldO;
    if cond.starts_with("oracle") {
        let mut o = OracleO { world: WorldO::new(c.clone()), truth: TruthO::default(), knows_rules: cond == "oracle-knows-rules" };
        return run_b12(c, LearnerOCfg::default(), Some(&mut o), 0, None, u64::MAX, window, false);
    }
    run_b12(c, l, None, 10, Some(wo_limits()), u64::MAX, window, true)
}

/// Step 1: how new families are met.
pub fn cmd_diag(args: &[String]) {
    let ss = seeds(args, (101, 120));
    let budget: i64 = arg(args, "--budget").map_or(1_200_000, |x| x.parse().unwrap());
    let window: i64 = arg(args, "--window").map_or(100_000, |x| x.parse().unwrap());
    let cond = arg(args, "--cond").unwrap_or("current".into());
    let t0 = std::time::Instant::now();
    let t = Tune::from_args(args);
    let res: Vec<RunB12> = ss.par_iter().map(|&s| run_cond(&cond, learner_b12(&cond, &t), &WOCfg::new(s, budget), window)).collect();
    eprintln!("diag runs: {:.0} s", t0.elapsed().as_secs_f64());
    println!("World O, condition {cond}, budget {budget}, seeds {}..{} (n = {})\n", ss[0], ss[ss.len() - 1], ss.len());
    let groups: [(&str, Box<dyn Fn(&FamDiag) -> bool>); 6] = [
        ("tier 0-1", Box::new(|f: &FamDiag| f.tier <= 1)),
        ("tier 2-3", Box::new(|f: &FamDiag| (2..=3).contains(&f.tier))),
        ("tier 4, index 32-95", Box::new(|f: &FamDiag| f.tier == 4 && f.fid < 96)),
        ("tier 4, index 96-159", Box::new(|f: &FamDiag| f.tier == 4 && (96..160).contains(&f.fid))),
        ("tier 4, index 160+", Box::new(|f: &FamDiag| f.tier == 4 && f.fid >= 160)),
        ("all", Box::new(|_: &FamDiag| true)),
    ];
    println!("#### First {FIRST_N} menus on which a family appears (pooled over worlds; per family means)\n");
    println!("| families | type | n | attempts on it per menu | menus skipped per menu | examples given | attempts in first {FIRST_N} menus | start estimate p (true best strategy) | true p | start estimate minus true | share whose start score is below the skip bar | attempts before the first rule (median, seq) | rules per family |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    let all: Vec<&FamDiag> = res.iter().flat_map(|r| r.diag.iter()).filter(|f| f.first_menus >= 20).collect();
    for (gname, g) in &groups {
        for ty in ["seq", "compose", "grid", "knap", "formula", "all"] {
            let v: Vec<&&FamDiag> = all.iter().filter(|f| g(f) && (ty == "all" || f.ftype.map(|t| t.name()) == Some(ty))).collect();
            if v.is_empty() {
                continue;
            }
            let n = v.len() as f64;
            let m = |f: &dyn Fn(&FamDiag) -> f64| v.iter().map(|x| f(x)).sum::<f64>() / n;
            let st: Vec<&&&FamDiag> = v.iter().filter(|f| f.start_seen).collect();
            let ns = st.len().max(1) as f64;
            println!(
                "| {gname} | {ty} | {} | {:.3} | {:.3} | {:.1} | {:.1} | {:.3} | {:.3} | {:+.3} | {:.2} | {:.0} | {:.1} |",
                v.len(),
                m(&|f| f.first_attempts as f64 / f.first_menus as f64),
                m(&|f| f.first_skips as f64 / f.first_menus as f64),
                m(&|f| f.first_examples as f64),
                m(&|f| f.first_attempts as f64),
                st.iter().map(|f| f.start_est_p).sum::<f64>() / ns,
                st.iter().map(|f| f.start_true_p).sum::<f64>() / ns,
                st.iter().map(|f| f.start_est_p - f.start_true_p).sum::<f64>() / ns,
                st.iter().filter(|f| f.start_margin < 0.0).count() as f64 / ns,
                median(v.iter().filter(|f| f.ftype.map(|t| t.name()) == Some("seq")).filter_map(|f| f.attempts_before_rule.map(|a| a as f64)).collect()),
                m(&|f| f.rules as f64),
            );
        }
    }
    println!("\n#### Whole life per family (pooled)\n");
    println!("| families | type | n | menus | attempts | attempts per menu | examples | seq families with no rule | compose attempts with a base rule at that difficulty |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for (gname, g) in &groups {
        for ty in ["seq", "compose", "all"] {
            let v: Vec<&&FamDiag> = all.iter().filter(|f| g(f) && (ty == "all" || f.ftype.map(|t| t.name()) == Some(ty))).collect();
            if v.is_empty() {
                continue;
            }
            let n = v.len() as f64;
            let ca: u64 = v.iter().map(|f| f.compose_attempts).sum();
            let cc: u64 = v.iter().map(|f| f.compose_covered).sum();
            let seqs: Vec<&&&FamDiag> = v.iter().filter(|f| f.ftype.map(|t| t.name()) == Some("seq")).collect();
            println!(
                "| {gname} | {ty} | {} | {:.0} | {:.0} | {:.3} | {:.0} | {:.2} | {} |",
                v.len(),
                v.iter().map(|f| f.menus as f64).sum::<f64>() / n,
                v.iter().map(|f| f.attempts as f64).sum::<f64>() / n,
                v.iter().map(|f| f.attempts as f64 / f.menus.max(1) as f64).sum::<f64>() / n,
                v.iter().map(|f| f.examples as f64).sum::<f64>() / n,
                seqs.iter().filter(|f| f.rules == 0).count() as f64 / seqs.len().max(1) as f64,
                if ca > 0 { format!("{:.3} of {}", cc as f64 / ca as f64, ca) } else { "-".into() },
            );
        }
    }
    println!("\n#### Rule-search inputs at window ends (mean over worlds)\n");
    println!("| window | examples held | signatures with examples | with >= 4 examples | with a live rule | rules accepted in window | wrong rules accepted |");
    println!("|---|---|---|---|---|---|---|");
    let nwin = res.iter().map(|r| r.search.len()).min().unwrap_or(0);
    let n = res.len() as f64;
    for i in 0..nwin {
        let s = |f: &dyn Fn(&RunB12) -> f64| res.iter().map(|r| f(r)).sum::<f64>() / n;
        println!(
            "| {}k-{}k | {:.0} | {:.1} | {:.1} | {:.1} | {:.1} | {:.2} |",
            i as i64 * window / 1000,
            (i as i64 + 1) * window / 1000,
            s(&|r| r.search[i].examples as f64),
            s(&|r| r.search[i].sigs as f64),
            s(&|r| r.search[i].sigs4 as f64),
            s(&|r| r.search[i].sigs_ruled as f64),
            s(&|r| r.run.wins[i].rules_accepted as f64),
            s(&|r| r.judged.iter().filter(|j| j.window == i as u64 && j.wrong()).count() as f64),
        );
    }
}



pub fn cmd_v2(_args: &[String]) {
    eprintln!("not yet");
}

/// Debug: why sequence families get no rule (one world, the open sequence families at some energy points).
pub fn cmd_seqdebug(args: &[String]) {
    use crate::being::discover::{self, evaluate_x, forms, Xf};
    use crate::being::world2::solve::sig2_split;
    use crate::being::world2::world::FType;
    use crate::being::world_o::engine::{EngineO, StateO};
    let seed: u64 = arg(args, "--seed").map_or(101, |x| x.parse().unwrap());
    let until: i64 = arg(args, "--energy").map_or(400_000, |x| x.parse().unwrap());
    let cfg = WOCfg::new(seed, 1_200_000);
    let l = learner("current");
    let mut st = StateO::default();
    st.start(cfg.clone(), l, String::new());
    let mut own = l.learner();
    let mut e = EngineO::new(st, Some(wo_limits()));
    let mut energy = 0i64;
    let mut next_dump = 100_000i64;
    while energy < until {
        let o = e.step(&mut own).unwrap();
        energy += o.outcome.cost;
        if e.state.s2.step % 10 == 0 {
            e.discover_pass();
        }
        if energy >= next_dump {
            next_dump += 100_000;
            println!("\n== energy {energy}, step {}, examples {}, rules {}", e.state.s2.step, e.state.s2.examples.len(), e.state.s2.rules.len());
            for (slot, fid) in e.state.known.clone() {
                let f = &e.world.fams[fid as usize];
                if f.ftype != FType::Seq {
                    continue;
                }
                let class = e.world.class_of(fid, e.state.s2.clock);
                let mut line = format!("fid {fid} tier {} class {:?} drift {:?} :", f.tier, class, f.drift_at);
                for d in 1..=5u8 {
                    let sig = crate::being::world2::solve::sig2(slot, d);
                    let ex: Vec<discover::Example> = e.state.s2.examples.iter().filter(|x| x.sig == sig).cloned().collect();
                    let nr = e.state.s2.rules.iter().filter(|r| r.sig == sig).count();
                    // best form: most right with no wrong
                    let mut best = String::new();
                    let mut bestw = String::new();
                    for fm in forms() {
                        let (r, w, _) = evaluate_x(Xf::ID, &fm, &ex);
                        if w == 0 && r > 0 && best.is_empty() {
                            best = format!("{}:{}r", fm.code(), r);
                        }
                        if w > 0 && r > 0 && bestw.is_empty() && r >= 3 {
                            bestw = format!("{}:{}r{}w", fm.code(), r, w);
                        }
                    }
                    let lens: Vec<usize> = ex.iter().map(|x| x.terms.len()).take(1).collect();
                    let _ = sig2_split(sig);
                    line.push_str(&format!(" d{d}[ex {} len {:?} rules {nr} ok {best} mixed {bestw}]", ex.len(), lens));
                }
                println!("{line}");
            }
        }
    }
}
