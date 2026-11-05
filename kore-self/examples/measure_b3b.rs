//! Track B3b: second attempt at adaptation in the drifting world (synthetic world, author-defined schedule).
//!   cargo run -p kore-self --example measure_b3b --offline --release -- diag      (diagnosis of attempt 1, tuning seeds)
//! Tuning seeds 101..=120 only for every choice; report seeds 1..=20.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;
#[path = "../src/safety.rs"]
mod safety;
#[path = "../src/redact.rs"]
mod redact;
#[path = "measure_being_parts/authorize_shim.rs"]
mod authorize;
#[path = "measure_being_parts/chat_shim/mod.rs"]
mod chat;

use being::chooser::{GreedyMenu, RandomMenu, UcbMenu};
use being::measure::{mean_ci, truth_flag, AdaptSpec};
use being::measure_b3b::*;
use being::model::AdaptMode;
use being::solver::Strategy;
use being::world::{Domain, World};

const BUDGET: i64 = 6000;

fn mci(v: &[f64]) -> String {
    let (m, lo, hi) = mean_ci(v);
    format!("{m:.3} [{lo:.3}, {hi:.3}]")
}

fn diag(seeds: std::ops::RangeInclusive<u64>) {
    let n = seeds.clone().count();
    println!("## Diagnosis of attempt 1, seeds {:?}, budget {BUDGET}, n = {n}\n", seeds);
    let novel = AdaptSpec { mode: AdaptMode::Full, half_life: 1e12, h: 1e12, ..AdaptSpec::full() };
    let names = ["B1", "attempt-1 adaptive", "cold-start only", "UCB1", "greedy", "random", "oracle (B3, no skip)", "oracle + skip rule", "B1 told when fast degrades (reset fast cells)"];
    let mut runs: Vec<Vec<XRun>> = vec![vec![]; names.len()];
    let mut ats = vec![];
    let mut units_truth = vec![];
    for seed in seeds.clone() {
        let a = at(seed, BUDGET);
        ats.push(a);
        let clocks = change_clocks(seed, BUDGET);
        let mut hook = told_reset_fast(clocks[2]);
        let r = vec![
            run_x(seed, BUDGET, true, &AdaptSpec::off(), &mut model_chooser(&AdaptSpec::off()), None),
            run_x(seed, BUDGET, true, &AdaptSpec::full(), &mut model_chooser(&AdaptSpec::full()), None),
            run_x(seed, BUDGET, true, &novel, &mut model_chooser(&novel), None),
            run_x(seed, BUDGET, true, &AdaptSpec::off(), &mut UcbMenu::new(seed, being::chooser::DEFAULT_EXPLORE_C), None),
            run_x(seed, BUDGET, true, &AdaptSpec::off(), &mut GreedyMenu { strategy: Strategy::Careful }, None),
            run_x(seed, BUDGET, true, &AdaptSpec::off(), &mut RandomMenu::new(seed), None),
            run_x(seed, BUDGET, true, &AdaptSpec::off(), &mut schedule_oracle(seed, BUDGET, true), None),
            run_x(seed, BUDGET, true, &AdaptSpec::off(), &mut SkipOracle { inner: schedule_oracle(seed, BUDGET, true), skip_frac: 1.0 }, None),
            run_x(seed, BUDGET, true, &AdaptSpec::off(), &mut model_chooser(&AdaptSpec::off()), Some(&mut hook)),
        ];
        for (i, x) in r.into_iter().enumerate() {
            runs[i].push(x);
        }
        let w = World::with_drift(seed, false, Some(BUDGET));
        units_truth.push((truth_flag(&w, &[], false), w.profile.clone()));
    }
    println!("### Whole life and windows of 300 energy units (reward per energy, mean [95% interval])\n");
    println!("| policy | whole life | 300 before change 1 | 300 after new domain | 300 after shift | 300 after fast degrades |");
    println!("|---|---|---|---|---|---|");
    for (i, nm) in names.iter().enumerate() {
        let col = |f: &dyn Fn(usize, &XRun) -> f64| runs[i].iter().enumerate().map(|(j, x)| f(j, x)).collect::<Vec<f64>>();
        println!(
            "| {nm} | {} | {} | {} | {} | {} |",
            mci(&col(&|_, x| x.per_budget())),
            mci(&col(&|j, x| window(&x.run, ats[j][0] - 300, ats[j][0]))),
            mci(&col(&|j, x| window(&x.run, ats[j][0], ats[j][0] + 300))),
            mci(&col(&|j, x| window(&x.run, ats[j][1], ats[j][1] + 300))),
            mci(&col(&|j, x| window(&x.run, ats[j][2], ats[j][2] + 300))),
        );
    }
    println!("\n### Paired: oracle + skip minus policy, per window (the headroom adaptation could recover)\n");
    println!("| policy | 300 before change 1 | 300 after new domain | 300 after shift | 300 after fast degrades |");
    println!("|---|---|---|---|---|");
    for (i, nm) in names.iter().enumerate() {
        let d = |w: usize| -> String {
            let v: Vec<f64> = (0..n)
                .map(|j| {
                    let (f, t) = if w == 0 { (ats[j][0] - 300, ats[j][0]) } else { (ats[j][w - 1], ats[j][w - 1] + 300) };
                    window(&runs[7][j].run, f, t) - window(&runs[i][j].run, f, t)
                })
                .collect();
            mci(&v)
        };
        println!("| {nm} | {} | {} | {} | {} |", d(0), d(1), d(2), d(3));
    }
    println!("\n### New domain: attempts in units between its appearance and the profile shift (per world, mean)\n");
    println!("| policy | units attempts | of all attempts | success rate | reward per energy on units | fast / careful / deep share on units |");
    println!("|---|---|---|---|---|---|");
    for (i, nm) in names.iter().enumerate() {
        let (mut nu, mut na, mut su, mut ru, mut cu) = (0u64, 0u64, 0u64, 0i64, 0i64);
        let mut sm = [0u64; 3];
        for (j, x) in runs[i].iter().enumerate() {
            let (f, t) = (ats[j][0], ats[j][1]);
            let (a, s) = count(&x.run, f, t, |r| r.domain == Some(Domain::Units));
            nu += a;
            su += s;
            na += count(&x.run, f, t, |_| true).0;
            let (r, c) = reward_cost(&x.run, f, t, |r| r.domain == Some(Domain::Units));
            ru += r;
            cu += c;
            for r in x.run.recs.iter().filter(|r| r.at >= f && r.at < t && r.domain == Some(Domain::Units)) {
                if let Some(s) = r.strategy {
                    sm[s.idx().min(2)] += 1;
                }
            }
        }
        let tot = sm.iter().sum::<u64>().max(1) as f64;
        println!(
            "| {nm} | {:.1} | {:.3} | {:.3} | {:.3} | {:.2} / {:.2} / {:.2} |",
            nu as f64 / n as f64,
            nu as f64 / na.max(1) as f64,
            su as f64 / nu.max(1) as f64,
            ru as f64 / cu.max(1) as f64,
            sm[0] as f64 / tot,
            sm[1] as f64 / tot,
            sm[2] as f64 / tot
        );
    }
    println!("\n### Fast after it degrades: attempts with `fast` in the first 600 energy units after the change (per world, mean)\n");
    println!("| policy | fast attempts | of which difficulty 1-2 | fast success rate | fast attempts in the 600 before |");
    println!("|---|---|---|---|---|");
    for (i, nm) in names.iter().enumerate() {
        let (mut fa, mut fe, mut fs, mut fb) = (0u64, 0u64, 0u64, 0u64);
        for (j, x) in runs[i].iter().enumerate() {
            let t = ats[j][2];
            for (r, d) in x.run.recs.iter().zip(&x.diff) {
                if r.attempted && r.strategy == Some(Strategy::Fast) {
                    if r.at >= t && r.at < t + 600 {
                        fa += 1;
                        fs += r.correct as u64;
                        fe += (*d <= 2) as u64;
                    } else if r.at >= t - 600 && r.at < t {
                        fb += 1;
                    }
                }
            }
        }
        println!("| {nm} | {:.1} | {:.1} | {:.3} | {:.1} |", fa as f64 / n as f64, fe as f64 / n as f64, fs as f64 / fa.max(1) as f64, fb as f64 / n as f64);
    }
    println!("\n### Hidden truth of units (mean over worlds): success probability by difficulty, strategies fast / careful / deep\n");
    for k in 0..5 {
        let m = |s: usize| units_truth.iter().map(|(t, _)| t.p[Domain::Units.idx()][k][s]).sum::<f64>() / n as f64;
        let o = |s: usize| units_truth.iter().map(|(t, _)| (0..4).map(|d| t.p[d][k][s]).sum::<f64>() / 4.0).sum::<f64>() / n as f64;
        println!("difficulty {}: units {:.2} / {:.2} / {:.2}; mean of the four old domains {:.2} / {:.2} / {:.2}", k + 1, m(0), m(1), m(2), o(0), o(1), o(2));
    }
    let rs = units_truth.iter().map(|(_, p)| p.reward_scale[4]).sum::<f64>() / n as f64;
    let ro = units_truth.iter().map(|(_, p)| (0..4).map(|d| p.reward_scale[d]).sum::<f64>() / 4.0).sum::<f64>() / n as f64;
    println!("reward scale: units {rs:.2}, old domains {ro:.2}");
    println!("\n### Alarms of the attempt-1 detector (all worlds)\n");
    let mut lines = vec![];
    for (j, x) in runs[1].iter().enumerate() {
        for a in &x.run.alarms {
            let reg = ats[j].iter().filter(|t| a.energy >= **t).count();
            lines.push(format!("seed-index {j} regime {reg} {} {} drop {} at +{} energy after the last change", a.domain.name(), a.strategy.name(), a.drop, a.energy - if reg > 0 { ats[j][reg - 1] } else { 0 }));
        }
    }
    println!("{} alarms", lines.len());
    for l in lines {
        println!("{l}");
    }
}

/// Per (domain, difficulty, strategy) counts in a window after change `w` (0 = new domain), pooled over worlds.
fn breakdown(seeds: std::ops::RangeInclusive<u64>, w: usize, len: i64) {
    let mut tabs: Vec<std::collections::BTreeMap<String, (u64, u64, i64, i64)>> = vec![Default::default(), Default::default()];
    let mut skips = [0u64; 2];
    for seed in seeds {
        let a = at(seed, BUDGET);
        let clocks = change_clocks(seed, BUDGET);
        let mut hook = told_reset_fast(clocks[2]);
        let first = if std::env::var("B3B_TOLD").is_ok() {
            run_x(seed, BUDGET, true, &AdaptSpec::off(), &mut model_chooser(&AdaptSpec::off()), Some(&mut hook))
        } else {
            run_x(seed, BUDGET, true, &AdaptSpec::off(), &mut model_chooser(&AdaptSpec::off()), None)
        };
        let r = [first, run_x(seed, BUDGET, true, &AdaptSpec::off(), &mut SkipOracle { inner: schedule_oracle(seed, BUDGET, true), skip_frac: 1.0 }, None)];
        for (i, x) in r.iter().enumerate() {
            for (rec, d) in x.run.recs.iter().zip(&x.diff) {
                if rec.at < a[w] || rec.at >= a[w] + len {
                    continue;
                }
                if !rec.attempted {
                    skips[i] += 1;
                    continue;
                }
                let key = format!("{} d{} {}", rec.domain.unwrap().name(), d, rec.strategy.unwrap().name());
                let e = tabs[i].entry(key).or_default();
                e.0 += 1;
                e.1 += rec.correct as u64;
                e.2 += rec.reward;
                e.3 += rec.cost;
            }
        }
    }
    println!("window after change {w}, {len} energy; skips B1 {} oracle+skip {}", skips[0], skips[1]);
    let mut keys: Vec<&String> = tabs[0].keys().chain(tabs[1].keys()).collect();
    keys.sort();
    keys.dedup();
    println!("| cell | B1 n | B1 succ | B1 r/e | oracle n | oracle succ | oracle r/e |");
    for k in keys {
        let a = tabs[0].get(k).copied().unwrap_or_default();
        let b = tabs[1].get(k).copied().unwrap_or_default();
        let f = |x: (u64, u64, i64, i64)| format!("{} | {:.2} | {:.2}", x.0, x.1 as f64 / x.0.max(1) as f64, x.2 as f64 / x.3.max(1) as f64);
        println!("| {k} | {} | {} |", f(a), f(b));
    }
}

/// Summary of one arm on a seed range: whole-life drift and stationary means, paired vs B1, post-change windows.
struct Sum {
    drift: Vec<f64>,
    stat: Vec<f64>,
    post: Vec<f64>,
    alarms_d: f64,
    alarms_s: f64,
}

fn summarize(seeds: &[u64], arm: &Arm) -> Sum {
    let mut s = Sum { drift: vec![], stat: vec![], post: vec![], alarms_d: 0.0, alarms_s: 0.0 };
    for &seed in seeds {
        let a = at(seed, BUDGET);
        let d = run_arm(seed, BUDGET, true, arm);
        let st = run_arm(seed, BUDGET, false, arm);
        s.drift.push(d.per_budget());
        s.stat.push(st.per_budget());
        s.post.push((0..3).map(|i| window(&d.run, a[i], a[i] + 300)).sum::<f64>() / 3.0);
        s.alarms_d += d.run.alarms.len() as f64 / seeds.len() as f64;
        s.alarms_s += st.run.alarms.len() as f64 / seeds.len() as f64;
    }
    s
}

fn tune_line(name: &str, s: &Sum, base: &Sum) {
    let dd: Vec<f64> = s.drift.iter().zip(&base.drift).map(|(a, b)| a - b).collect();
    let ds: Vec<f64> = s.stat.iter().zip(&base.stat).map(|(a, b)| a - b).collect();
    let dp: Vec<f64> = s.post.iter().zip(&base.post).map(|(a, b)| a - b).collect();
    let wl = |v: &[f64]| format!("{}/{}", v.iter().filter(|x| **x > 1e-9).count(), v.iter().filter(|x| **x < -1e-9).count());
    let m = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    println!(
        "{name}: drift {:.3} ({:+.3}, {}) stationary {:.3} ({:+.3}, {}) post-change 300 {:.3} ({:+.3}, {}) alarms/run {:.1} / {:.1}",
        m(&s.drift), m(&dd), wl(&dd), m(&s.stat), m(&ds), wl(&ds), m(&s.post), m(&dp), wl(&dp), s.alarms_d, s.alarms_s
    );
}

fn tune(which: &str) {
    use being::adapt_b3b::{SwitchCfg, DEFAULT_SWITCH};
    let seeds: Vec<u64> = (101..=120).collect();
    let base = summarize(&seeds, &Arm::b1());
    tune_line("B1", &base, &base);
    let nov = being::planner::DEFAULT_NOVELTY;
    let mut arms: Vec<(String, Arm)> = vec![];
    let none = SwitchCfg { detect: false, prior_check: false, ..DEFAULT_SWITCH };
    match which {
        "quick" => {
            // the first settings tried (before the grids below), kept explicit so the line is reproducible
            let old = SwitchCfg { window: 8, nats: 4.0, min_old: 8.0, prior_nats: 1.5, prior_keep: 0.25, prior_max_n: 8.0, ..DEFAULT_SWITCH };
            arms.push(("attempt 1".into(), Arm::attempt1()));
            arms.push(("cold start only".into(), Arm::cold_start()));
            arms.push(("switch mode, nothing on".into(), Arm::switch(none, 0.0)));
            arms.push(("M1 default".into(), Arm::switch(SwitchCfg { prior_check: false, ..old }, 0.0)));
            arms.push(("M2 default".into(), Arm::switch(SwitchCfg { detect: false, ..old }, 0.0)));
            arms.push(("M1+M2 default".into(), Arm::switch(old, 0.0)));
        }
        "nov" => {
            arms.push(("cold start only".into(), Arm::cold_start()));
        }
        "combo" => {
            let m1 = SwitchCfg { window: 6, nats: 8.0, min_old: 8.0, ..DEFAULT_SWITCH };
            let m2 = SwitchCfg { prior_nats: 1.0, prior_keep: 0.1, prior_max_n: 16.0, ..DEFAULT_SWITCH };
            let both = SwitchCfg { window: 6, nats: 8.0, min_old: 8.0, prior_nats: 1.0, prior_keep: 0.1, prior_max_n: 16.0, ..DEFAULT_SWITCH };
            arms.push(("M1 chosen".into(), Arm::switch(SwitchCfg { prior_check: false, ..m1 }, 0.0)));
            arms.push(("M2 chosen".into(), Arm::switch(SwitchCfg { detect: false, ..m2 }, 0.0)));
            arms.push(("M1+M2".into(), Arm::switch(both, 0.0)));
            for sp in [0.5, 0.75] {
                arms.push((format!("M1+M3 spread {sp}"), Arm::switch(SwitchCfg { prior_check: false, spread: sp, ..m1 }, 0.0)));
                arms.push((format!("M1+M2+M3 spread {sp}"), Arm::switch(SwitchCfg { spread: sp, ..both }, 0.0)));
            }
        }
        "m4" => {
            let both = SwitchCfg { window: 6, nats: 8.0, min_old: 8.0, prior_nats: 1.0, prior_keep: 0.1, prior_max_n: 16.0, ..DEFAULT_SWITCH };
            for below in [f64::INFINITY, 200.0, 50.0, 20.0] {
                arms.push((format!("M4 only, while group below {below}"), Arm::switch(SwitchCfg { cross: true, cross_below: below, ..none }, 0.0)));
                arms.push((format!("M1+M2+M4, while group below {below}"), Arm::switch(SwitchCfg { cross: true, cross_below: below, ..both }, 0.0)));
            }
        }
        "check" => {
            let mut a = Arm::cold_start();
            a.spec.novelty = 0.0;
            arms.push(("Full hl 1e12 h 1e12 novelty 0".into(), a));
            let mut b = Arm::cold_start();
            b.spec.half_life = 1e300;
            arms.push(("Full hl 1e300 h 1e12 novelty 0.25".into(), b));
        }
        "told" => {
            for (nm, which) in [("told what: new domain", [true, false, false]), ("told what: shift cells", [false, true, false]), ("told what: degradation", [false, false, true]), ("told what: all three", [true, true, true])] {
                let mut s = Sum { drift: vec![], stat: base.stat.clone(), post: vec![], alarms_d: 0.0, alarms_s: 0.0 };
                for &seed in &seeds {
                    let a = at(seed, BUDGET);
                    let mut hook = told_what(seed, BUDGET, which, 20);
                    let d = run_x(seed, BUDGET, true, &AdaptSpec::off(), &mut model_chooser(&AdaptSpec::off()), Some(&mut hook));
                    s.drift.push(d.per_budget());
                    s.post.push((0..3).map(|i| window(&d.run, a[i], a[i] + 300)).sum::<f64>() / 3.0);
                }
                tune_line(nm, &s, &base);
            }
            let mut s = Sum { drift: vec![], stat: base.stat.clone(), post: vec![], alarms_d: 0.0, alarms_s: 0.0 };
            for &seed in &seeds {
                let a = at(seed, BUDGET);
                let mut hook = told_similar(seed, BUDGET, 20);
                let d = run_x(seed, BUDGET, true, &AdaptSpec::off(), &mut model_chooser(&AdaptSpec::off()), Some(&mut hook));
                s.drift.push(d.per_budget());
                s.post.push((0..3).map(|i| window(&d.run, a[i], a[i] + 300)).sum::<f64>() / 3.0);
            }
            tune_line("told: new domain starts from the old domains' mean true rates (best case of a similar-domain prior)", &s, &base);
        }
        "m1" => {
            for w in [6usize, 8, 12, 16] {
                for nats in [3.0, 4.0, 6.0, 8.0] {
                    for mo in [8.0, 16.0] {
                        arms.push((format!("M1 window {w} nats {nats} min_old {mo}"), Arm::switch(SwitchCfg { window: w, nats, min_old: mo, prior_check: false, ..DEFAULT_SWITCH }, 0.0)));
                    }
                }
            }
        }
        "m2" => {
            for pn in [1.0, 1.5, 2.0, 3.0] {
                for keep in [0.1, 0.25, 0.5] {
                    for mx in [8.0, 16.0] {
                        arms.push((format!("M2 nats {pn} keep {keep} max_n {mx}"), Arm::switch(SwitchCfg { prior_nats: pn, prior_keep: keep, prior_max_n: mx, detect: false, ..DEFAULT_SWITCH }, 0.0)));
                    }
                }
            }
        }
        _ => {}
    }
    for (nm, a) in arms {
        let s = summarize(&seeds, &a);
        tune_line(&nm, &s, &base);
    }
}

/// The arms of the report, in table order. Learners first (indices 0..LEARNERS), then fixed policies and references.
const LEARNERS: usize = 7;

fn report_arms() -> Vec<(&'static str, Arm)> {
    use being::adapt_b3b::{SwitchCfg, DEFAULT_SWITCH};
    let only = |f: fn(SwitchCfg) -> SwitchCfg| Arm::switch(f(DEFAULT_SWITCH), 0.0);
    vec![
        ("B1 (no adaptation)", Arm::b1()),
        ("attempt 1 (B3 adaptive)", Arm::attempt1()),
        ("attempt-1 cold start only", Arm::cold_start()),
        ("B3b M1 windowed switch only", only(|c| SwitchCfg { prior_check: false, ..c })),
        ("B3b M2 prior conflict only", only(|c| SwitchCfg { detect: false, ..c })),
        ("B3b M1+M2 (the B3b learner)", only(|c| c)),
        ("B3b M1+M2+M4 cross-domain prior (group below 20)", only(|c| SwitchCfg { cross: true, cross_below: 20.0, ..c })),
    ]
}

struct World1 {
    at: [i64; 3],
    /// drift runs: learners, then UCB1, greedy, random, oracle (B3), oracle + skip, told-what upper bound
    d: Vec<XRun>,
    /// stationary runs of the learners
    s: Vec<XRun>,
}

const REF_NAMES: [&str; 6] = ["UCB1 domain chooser", "greedy by listed reward per cost", "random", "oracle (B3, never skips)", "oracle + skip rule", "B1 told what changed (upper bound, not a learner)"];

fn one_world(seed: u64, arms: &[(&str, Arm)]) -> World1 {
    let mut d: Vec<XRun> = arms.iter().map(|(_, a)| run_arm(seed, BUDGET, true, a)).collect();
    let s: Vec<XRun> = arms.iter().map(|(_, a)| run_arm(seed, BUDGET, false, a)).collect();
    let off = AdaptSpec::off();
    d.push(run_x(seed, BUDGET, true, &off, &mut UcbMenu::new(seed, being::chooser::DEFAULT_EXPLORE_C), None));
    d.push(run_x(seed, BUDGET, true, &off, &mut GreedyMenu { strategy: Strategy::Careful }, None));
    d.push(run_x(seed, BUDGET, true, &off, &mut RandomMenu::new(seed), None));
    d.push(run_x(seed, BUDGET, true, &off, &mut schedule_oracle(seed, BUDGET, true), None));
    d.push(run_x(seed, BUDGET, true, &off, &mut SkipOracle { inner: schedule_oracle(seed, BUDGET, true), skip_frac: 1.0 }, None));
    let mut hook = told_what(seed, BUDGET, [true, true, true], 20);
    d.push(run_x(seed, BUDGET, true, &off, &mut model_chooser(&off), Some(&mut hook)));
    World1 { at: at(seed, BUDGET), d, s }
}

fn paired(name: &str, a: &[f64], b: &[f64]) {
    let v: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let (m, lo, hi) = mean_ci(&v);
    let w = v.iter().filter(|x| **x > 1e-9).count();
    let l = v.iter().filter(|x| **x < -1e-9).count();
    println!("| {name} | {m:+.3} | [{lo:+.3}, {hi:+.3}] | {w} / {l} / {} |", v.len() - w - l);
}

type Flag = (i64, being::model::Detector, Domain, Strategy, Option<u8>, bool);

fn report(label: &str, seeds: std::ops::RangeInclusive<u64>) {
    use being::measure::units_competence;
    use being::model::Detector;
    let arms = report_arms();
    let ws: Vec<World1> = seeds.clone().map(|s| one_world(s, &arms)).collect();
    let n = ws.len();
    let names: Vec<&str> = arms.iter().map(|a| a.0).chain(REF_NAMES.iter().copied()).collect();
    let skip_i = LEARNERS + 4;
    let or_i = LEARNERS + 3;
    let w300 = |x: &XRun, w: &World1, i: usize| if i == 0 { window(&x.run, w.at[0] - 300, w.at[0]) } else { window(&x.run, w.at[i - 1], w.at[i - 1] + 300) };
    println!("\n## Track B3b report, {label}, budget {BUDGET}, n = {n} worlds\n");
    println!("### Reward per energy (mean [95% interval of the mean]); windows are 300 energy units\n");
    println!("| policy | whole life, drifting | whole life, stationary | 300 before change 1 | 300 after new domain | 300 after shift | 300 after fast degrades |");
    println!("|---|---|---|---|---|---|---|");
    for (i, nm) in names.iter().enumerate() {
        let col = |f: &dyn Fn(&World1) -> f64| mci(&ws.iter().map(f).collect::<Vec<f64>>());
        let stat = if i < LEARNERS { col(&|w| w.s[i].per_budget()) } else { "-".into() };
        println!("| {nm} | {} | {stat} | {} | {} | {} | {} |", col(&|w| w.d[i].per_budget()), col(&|w| w300(&w.d[i], w, 0)), col(&|w| w300(&w.d[i], w, 1)), col(&|w| w300(&w.d[i], w, 2)), col(&|w| w300(&w.d[i], w, 3)));
    }
    println!("\n### Regret against the oracle + skip rule (its reward per energy minus the policy's, same world, same window)\n");
    println!("Excess = mean regret over the three post-change windows minus the regret in the 300 before change 1 (what the changes cost beyond the policy's usual gap).\n");
    println!("| policy | before change 1 | after new domain | after shift | after fast degrades | mean of the three after | excess |");
    println!("|---|---|---|---|---|---|---|");
    let reg = |i: usize, w: &World1, k: usize| w300(&w.d[skip_i], w, k) - w300(&w.d[i], w, k);
    let post = |i: usize, w: &World1| (1..=3).map(|k| reg(i, w, k)).sum::<f64>() / 3.0;
    for (i, nm) in names.iter().enumerate() {
        let col = |f: &dyn Fn(&World1) -> f64| mci(&ws.iter().map(f).collect::<Vec<f64>>());
        println!("| {nm} | {} | {} | {} | {} | {} | {} |", col(&|w| reg(i, w, 0)), col(&|w| reg(i, w, 1)), col(&|w| reg(i, w, 2)), col(&|w| reg(i, w, 3)), col(&|w| post(i, w)), col(&|w| post(i, w) - reg(i, w, 0)));
    }
    println!("\n### Paired differences (first minus second; wins / losses / ties over {n} worlds)\n");
    println!("| comparison | mean | 95% interval | W / L / T |");
    println!("|---|---|---|---|");
    let whole = |i: usize| ws.iter().map(|w| w.d[i].per_budget()).collect::<Vec<f64>>();
    let stat = |i: usize| ws.iter().map(|w| w.s[i].per_budget()).collect::<Vec<f64>>();
    let postr = |i: usize| ws.iter().map(|w| (1..=3).map(|k| w300(&w.d[i], w, k)).sum::<f64>() / 3.0).collect::<Vec<f64>>();
    let ex = |i: usize| ws.iter().map(|w| post(i, w) - reg(i, w, 0)).collect::<Vec<f64>>();
    for i in 1..LEARNERS {
        paired(&format!("{}: whole life minus B1", names[i]), &whole(i), &whole(0));
        paired(&format!("{}: post-change windows minus B1", names[i]), &postr(i), &postr(0));
        paired(&format!("{}: stationary minus B1", names[i]), &stat(i), &stat(0));
    }
    for i in 3..LEARNERS {
        paired(&format!("{} minus attempt 1: whole life", names[i]), &whole(i), &whole(1));
        paired(&format!("{} minus attempt 1: post-change windows", names[i]), &postr(i), &postr(1));
    }
    for r in [LEARNERS, LEARNERS + 1, LEARNERS + 2] {
        paired(&format!("B3b learner minus {}: whole life", names[r]), &whole(5), &whole(r));
        paired(&format!("excess regret, {} minus B3b learner (positive: B3b loses less to the changes)", names[r]), &ex(r), &ex(5));
        paired(&format!("excess regret, {} minus B1 (positive: B1 loses less to the changes)", names[r]), &ex(r), &ex(0));
    }
    paired("excess regret, B1 minus B3b learner (positive: B3b loses less to the changes)", &ex(0), &ex(5));
    paired("excess regret, attempt 1 minus B3b learner", &ex(1), &ex(5));
    paired("oracle + skip minus B3b learner: whole life", &whole(skip_i), &whole(5));
    paired("oracle (B3) minus B3b learner: whole life", &whole(or_i), &whole(5));
    paired("told-what upper bound minus B1: whole life", &whole(LEARNERS + 5), &whole(0));
    paired("told-what upper bound minus B1: post-change windows", &postr(LEARNERS + 5), &postr(0));
    println!("\n### New domain: energy after it appears until the trailing 10 units attempts earn 80% of the B3 oracle's rate there\n");
    println!("| policy | worlds that reach it | median energy | units attempts in the first 300 energy | reward per energy on units, first 300 energy |");
    println!("|---|---|---|---|---|");
    let comp = |i: usize| ws.iter().map(|w| units_competence(&w.d[i].run, &w.d[or_i].run, w.at[0], 0.8)).collect::<Vec<Option<i64>>>();
    for (i, nm) in names.iter().enumerate() {
        let c = comp(i);
        let mut v: Vec<f64> = c.iter().flatten().map(|x| *x as f64).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = if v.is_empty() { "-".into() } else { format!("{:.0}", v[v.len() / 2]) };
        let (mut na, mut r, mut c2) = (0u64, 0i64, 0i64);
        for w in &ws {
            na += count(&w.d[i].run, w.at[0], w.at[0] + 300, |r| r.domain == Some(Domain::Units)).0;
            let (a, b) = reward_cost(&w.d[i].run, w.at[0], w.at[0] + 300, |r| r.domain == Some(Domain::Units));
            r += a;
            c2 += b;
        }
        println!("| {nm} | {} of {n} | {med} | {:.1} | {:.3} |", v.len(), na as f64 / n as f64, r as f64 / c2.max(1) as f64);
    }
    println!();
    for i in [1usize, 3, 4, 5, 6, LEARNERS, LEARNERS + 1] {
        let (a, b) = (comp(i), comp(0));
        let both: Vec<f64> = a.iter().zip(&b).filter_map(|(x, y)| Some((*x)? as f64 - (*y)? as f64)).collect();
        if both.len() >= 2 {
            let (m, lo, hi) = mean_ci(&both);
            let f = both.iter().filter(|x| **x < 0.0).count();
            let sl = both.iter().filter(|x| **x > 0.0).count();
            println!("* {} minus B1, worlds where both reach it ({}): {m:+.0} energy [{lo:+.0}, {hi:+.0}], faster in {f}, slower in {sl}", names[i], both.len());
        }
    }
    println!("\n### Flagged changes per world (mean; drifting / stationary)\n");
    println!("| learner | group CUSUM | cell switch | of which fast, difficulty 1-2, drop, after the degradation | prior conflict | prior conflicts in units |");
    println!("|---|---|---|---|---|---|");
    for i in 0..LEARNERS {
        let cnt = |stat: bool, f: &dyn Fn(&World1, &Flag) -> bool| ws.iter().map(|w| (if stat { &w.s[i] } else { &w.d[i] }).flags.iter().filter(|x| f(w, x)).count()).sum::<usize>() as f64 / n as f64;
        let k = |kind: Detector| move |_: &World1, x: &Flag| x.1 == kind;
        println!(
            "| {} | {:.2} / {:.2} | {:.2} / {:.2} | {:.2} | {:.2} / {:.2} | {:.2} |",
            names[i],
            cnt(false, &k(Detector::Group)),
            cnt(true, &k(Detector::Group)),
            cnt(false, &k(Detector::CellSwitch)),
            cnt(true, &k(Detector::CellSwitch)),
            cnt(false, &|w, x| x.1 == Detector::CellSwitch && x.3 == Strategy::Fast && x.4.map_or(false, |k| k <= 2) && x.0 >= w.at[2] && x.5),
            cnt(false, &k(Detector::PriorConflict)),
            cnt(true, &k(Detector::PriorConflict)),
            cnt(false, &|_, x| x.1 == Detector::PriorConflict && x.2 == Domain::Units),
        );
    }
    let mut delays = vec![];
    for w in &ws {
        if let Some(x) = w.d[5].flags.iter().find(|x| x.1 == Detector::CellSwitch && x.3 == Strategy::Fast && x.0 >= w.at[2]) {
            delays.push((x.0 - w.at[2]) as f64);
        }
    }
    delays.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med = delays.get(delays.len() / 2).map_or("-".to_string(), |d| format!("{d:.0}"));
    println!("\nB3b learner: a cell switch on fast after the degradation in {} of {n} worlds, median delay {med} energy units.", delays.len());
    let fp_before: usize = ws.iter().map(|w| w.d[5].flags.iter().filter(|x| x.1 == Detector::CellSwitch && x.0 < w.at[0]).count()).sum();
    println!("B3b learner: cell switches before the first change (drifting world): {fp_before} in {n} worlds.");
}

/// Other budgets: B3b minus B1, whole life, drifting (schedule scaled with the budget) and stationary.
fn budgets() {
    use being::adapt_b3b::DEFAULT_SWITCH;
    for (label, seeds) in [("report seeds 1..20", 1..=20u64), ("tuning seeds 101..120", 101..=120u64)] {
        for budget in [600i64, 3000] {
            for drift in [true, false] {
                let (mut a, mut b, mut flags) = (vec![], vec![], 0usize);
                for seed in seeds.clone() {
                    a.push(run_arm(seed, budget, drift, &Arm::switch(DEFAULT_SWITCH, 0.0)).per_budget());
                    let x = run_arm(seed, budget, drift, &Arm::b1());
                    b.push(x.per_budget());
                    flags += run_arm(seed, budget, drift, &Arm::switch(DEFAULT_SWITCH, 0.0)).flags.len();
                }
                let v: Vec<f64> = a.iter().zip(&b).map(|(x, y)| x - y).collect();
                let (m, lo, hi) = mean_ci(&v);
                let w = v.iter().filter(|x| **x > 1e-9).count();
                let l = v.iter().filter(|x| **x < -1e-9).count();
                println!("| {label} | {budget} | {} | {:.3} | {:.3} | {m:+.3} [{lo:+.3}, {hi:+.3}] | {w} / {l} / {} | {:.2} |", if drift { "drifting" } else { "stationary" }, mean_ci(&b).0, mean_ci(&a).0, v.len() - w - l, flags as f64 / v.len() as f64);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("diag") => diag(101..=120),
        Some("diag-report") => diag(1..=20),
        Some("report") => {
            report("report seeds 1..20", 1..=20);
            if args.get(2).map_or(false, |a| a == "with-tuning") {
                report("tuning seeds 101..120", 101..=120);
            }
        }
        Some("budgets") => budgets(),
        Some("tune") => tune(args.get(2).map_or("quick", |s| s.as_str())),
        Some("breakdown") => breakdown(101..=120, args.get(2).and_then(|x| x.parse().ok()).unwrap_or(0), args.get(3).and_then(|x| x.parse().ok()).unwrap_or(300)),
        _ => eprintln!("usage: measure_b3b diag | diag-report"),
    }
}
