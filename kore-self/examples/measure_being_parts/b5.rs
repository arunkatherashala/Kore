//! Track B5 measurement: composition of verified building blocks, rule pruning, caps, kill switch.
//!   cargo run -p kore-self --example measure_being --offline --release -- b5
//! prints the tables of docs/measured.md. Modes: b5 (all), b5-compose, b5-prune, b5-caps, b5-kill, b5-tune.
//! Report seeds 1..=20; tuning seeds 101..=120 (used only for the pruning settings). Synthetic worlds
//! written by the same author as the learner; hidden truth is used here to score the being, never by it.

use super::being::controller::{Limits, PruneCfg};
use super::being::measure::mean_ci;
use super::being::measure_b5::*;
use super::being::policy::{KillCheck, Policy, StopReason};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

const EVERY: u64 = 10;

fn line(name: &str, xs: &[f64]) {
    let (m, lo, hi) = mean_ci(xs);
    println!("| {name} | {m:.3} | [{lo:.3}, {hi:.3}] |");
}

fn paired(name: &str, a: &[f64], b: &[f64]) {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let (m, lo, hi) = mean_ci(&d);
    let (w, l) = (a.iter().zip(b).filter(|(x, y)| x > y).count(), a.iter().zip(b).filter(|(x, y)| x < y).count());
    println!("| {name} | {m:+.3} | [{lo:+.3}, {hi:+.3}] | {w} wins, {l} losses, {} ties of {} |", a.len() - w - l, a.len());
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(v[v.len() / 2])
}

fn comp_limits() -> Limits {
    Limits { compose: true, ..Default::default() }
}

pub fn compose_report(label: &str, seeds: std::ops::RangeInclusive<u64>, budget: i64) {
    let n = seeds.clone().count();
    let pol = || Policy::new(100_000);
    let mut a = vec![];
    let (mut b, mut c) = (vec![], vec![]);
    let (mut fb, mut fc) = (vec![], vec![]);
    for s in seeds.clone() {
        a.push(run_b5(s, budget, false, None, 0, &pol()));
        let rb = run_b5(s, budget, false, None, EVERY, &pol());
        let rc = run_b5(s, budget, false, Some(comp_limits()), EVERY, &pol());
        fb.push(analyze(s, &rb));
        fc.push(analyze(s, &rc));
        b.push(rb);
        c.push(rc);
    }
    let rpe = |v: &Vec<B5Out>| -> Vec<f64> { v.iter().map(|r| r.per_budget()).collect() };
    println!("\n### Extended2 pattern world, composition vs B2 grammar only, {label} (budget {budget}, discovery pass every {EVERY} steps, n = {n} worlds)\n");
    println!("| policy | mean reward per energy unit | 95% interval of the mean |");
    println!("|---|---|---|");
    line("model learner, no discovery", &rpe(&a));
    line("model learner + B2 discovery (17-form grammar only)", &rpe(&b));
    line("model learner + discovery with composition of verified blocks", &rpe(&c));
    println!("\n| paired difference, reward per energy | mean | 95% interval of the mean | first minus second wins |");
    println!("|---|---|---|---|");
    paired("composition minus grammar only", &rpe(&c), &rpe(&b));
    paired("grammar only minus no discovery", &rpe(&b), &rpe(&a));
    paired("composition minus no discovery", &rpe(&c), &rpe(&a));
    let gn = ["affine geometric (a*t+c)", "cubic in n", "second-order recurrence", "tribonacci-like", "alternating add", "period-3 cycle"];
    let cn = ["even/odd quadratics (parity split, poly 2)", "n^3 + alternating term (parity split, poly 3)", "cubic + geometric (second differences, order-3 recurrence)", "(-1)^n times a cubic (parity split, poly 3)"];
    println!("\nFamilies covered by the final rule book (>= 90% exact on 100 fresh tasks of the family; checked against hidden truth, the being never sees it):\n");
    println!("| hidden family | B2 grammar only: worlds | composition: worlds |");
    println!("|---|---|---|");
    let (mut g_b, mut g_c, mut c_b, mut c_c) = (0, 0, 0, 0);
    for (k, nm) in gn.iter().enumerate() {
        let (x, y) = (fb.iter().filter(|f| f.grammar[k]).count(), fc.iter().filter(|f| f.grammar[k]).count());
        g_b += x;
        g_c += y;
        println!("| in the 17-form grammar: {nm} | {x} of {n} | {y} of {n} |");
    }
    for (k, nm) in cn.iter().enumerate() {
        let (x, y) = (fb.iter().filter(|f| f.composable[k]).count(), fc.iter().filter(|f| f.composable[k]).count());
        c_b += x;
        c_c += y;
        println!("| outside the grammar, composable: {nm} | {x} of {n} | {y} of {n} |");
    }
    println!("\nRecall of grammar families: B2 {g_b} of {} ({:.1}%), composition {g_c} of {} ({:.1}%)", n * 6, 100.0 * g_b as f64 / (n * 6) as f64, n * 6, 100.0 * g_c as f64 / (n * 6) as f64);
    println!("Recall of composable families: B2 {c_b} of {} ({:.1}%), composition {c_c} of {} ({:.1}%)", n * 4, 100.0 * c_b as f64 / (n * 4) as f64, n * 4, 100.0 * c_c as f64 / (n * 4) as f64);
    for (name, fs) in [("B2 grammar only", &fb), ("composition", &fc)] {
        let (acc, ex, wr, comp): (usize, usize, usize, usize) = fs.iter().fold((0, 0, 0, 0), |x, f| (x.0 + f.rules, x.1 + f.exact, x.2 + f.wrong, x.3 + f.composed_rules));
        println!("{name}: rules accepted {acc} (composed {comp}); exact on 300 fresh tasks of their signature {ex}; wrong or overfit {wr}; worlds with a wrong rule {} of {n}; mean rules per world {:.2}", fs.iter().filter(|f| f.wrong > 0).count(), acc as f64 / n as f64);
        for (i, f) in fs.iter().enumerate() {
            for w in &f.wrong_rules {
                println!("  wrong rule in world {}: signature {} form {} applicable-and-wrong on {} of 300 fresh tasks", seeds.start() + i as u64, w.0, w.1, w.2);
            }
        }
    }
    println!("\nNegative controls (families outside the search space; fresh tasks, final rule book; share of tasks where some rule is applied / wrong share of those applications):\n");
    println!("| family | B2 grammar only | composition |");
    println!("|---|---|---|");
    let negn = ["product recurrence", "consecutive primes", "LCG mod 101", "squares mod 7"];
    for (k, nm) in negn.iter().enumerate() {
        let m = |fs: &Vec<Found>, f: fn(&(f64, f64)) -> f64| fs.iter().map(|x| f(&x.negative[k])).sum::<f64>() / n as f64;
        println!("| {nm} | {:.3} / {:.3} | {:.3} / {:.3} |", m(&fb, |x| x.0), m(&fb, |x| x.1), m(&fc, |x| x.0), m(&fc, |x| x.1));
    }
    // which compositions were accepted
    let mut tally: std::collections::BTreeMap<String, usize> = Default::default();
    for r in &c {
        for (_, ru) in &r.accepted {
            if !ru.xf.is_id() {
                *tally.entry(format!("signature {}: {} then {}", ru.sig, ru.xf.code(), ru.form.code())).or_default() += 1;
            }
        }
    }
    println!("\nComposed rules accepted (all worlds): {tally:?}");
    let att = |v: &Vec<B5Out>| v.iter().map(|r| r.uses.len() as f64).sum::<f64>() / n as f64;
    println!("Attempts answered by a rule per world: B2 {:.1}, composition {:.1}", att(&b), att(&c));
}

pub fn prune_scenario(label: &str, seeds: std::ops::RangeInclusive<u64>, budget: i64, cfg: PruneCfg, cap: usize, print: bool) -> (f64, f64, f64, f64) {
    let n = seeds.clone().count();
    let pol = || Policy::new(100_000);
    let lim = |prune: bool| Limits { compose: true, prune, prune_cfg: cfg, max_live_rules: cap, ..Default::default() };
    let t = drift_clock(budget);
    let (mut off, mut on, mut st_off, mut st_on) = (vec![], vec![], vec![], vec![]);
    for s in seeds.clone() {
        off.push(run_b5(s, budget, true, Some(lim(false)), EVERY, &pol()));
        on.push(run_b5(s, budget, true, Some(lim(true)), EVERY, &pol()));
        st_off.push(run_b5(s, budget, false, Some(lim(false)), EVERY, &pol()));
        st_on.push(run_b5(s, budget, false, Some(lim(true)), EVERY, &pol()));
    }
    // the world's rule change affects difficulty 2: the old rule is the plain affine form accepted before the change
    let drift_step = |r: &B5Out| r.trace.iter().position(|x| x.0 >= t).unwrap_or(r.trace.len()) as u64;
    let live_old = |r: &B5Out| r.accepted.iter().any(|(st, ru)| ru.sig == 2 && ru.xf.is_id() && *st <= drift_step(r) && matches!(ru.form, super::being::discover::Form::Lin { mask: 1, constant: true }));
    let aff: Vec<usize> = (0..n).filter(|i| live_old(&off[*i])).collect();
    let wrong = |v: &Vec<B5Out>, idx: &[usize]| -> Vec<f64> { idx.iter().map(|i| wrong_uses_after(&v[*i], t).1 as f64).collect() };
    let uses = |v: &Vec<B5Out>, idx: &[usize]| -> Vec<f64> { idx.iter().map(|i| wrong_uses_after(&v[*i], t).0 as f64).collect() };
    let post = |v: &Vec<B5Out>, idx: &[usize]| -> Vec<f64> { idx.iter().map(|i| v[*i].per_budget_after(t)).collect() };
    let all: Vec<usize> = (0..n).collect();
    let (w_off, w_on) = (wrong(&off, &aff), wrong(&on, &aff));
    let result = (mean_ci(&w_off).0, mean_ci(&w_on).0, mean_ci(&post(&off, &aff)).0, mean_ci(&post(&on, &aff)).0);
    if !print {
        let fr: f64 = st_on.iter().map(|r| r.retired.len() as f64).sum::<f64>() / n as f64;
        println!("  tuning {cfg:?}: affected worlds {} of {n}; wrong rule uses after the change: no pruning {:.2}, pruning {:.2}; reward/energy after the change {:.3} vs {:.3}; retirements per stationary world {:.2}", aff.len(), result.0, result.1, result.2, result.3, fr);
        return result;
    }
    println!("\n### Rule drift, pruning vs no pruning, {label} (budget {budget}, live-rule cap {}, retire below {:.2} after {} uses of the latest {}, n = {n} worlds)\n", if cap == usize::MAX { "none".to_string() } else { cap.to_string() }, cfg.threshold, cfg.min_n, cfg.window);
    println!("The sequence rule of one family (difficulty 2, affine-geometric) changes at half the budget; the old rule then fits every prefix of that family and gives a wrong next term. The being is not told.\n");
    println!("Worlds where the old rule was live at the change: {} of {n}. Statistics below are over those worlds unless stated.\n", aff.len());
    println!("| measure (after the change) | no pruning | pruning | paired difference (pruning minus none) |");
    println!("|---|---|---|---|");
    let row = |name: &str, a: &[f64], b: &[f64]| {
        let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| y - x).collect();
        let (m, lo, hi) = mean_ci(&d);
        let (wn, ln) = (a.iter().zip(b).filter(|(x, y)| y > x).count(), a.iter().zip(b).filter(|(x, y)| y < x).count());
        println!("| {name} | {:.3} | {:.3} | {m:+.3} [{lo:+.3}, {hi:+.3}], pruning higher in {wn}, lower in {ln} of {} |", mean_ci(a).0, mean_ci(b).0, a.len());
    };
    row("wrong answers produced by a rule, per world", &w_off, &w_on);
    row("attempts answered by a rule, per world", &uses(&off, &aff), &uses(&on, &aff));
    row("reward per energy unit", &post(&off, &aff), &post(&on, &aff));
    let all_post_off = post(&off, &all);
    let all_post_on = post(&on, &all);
    row("reward per energy unit, ALL worlds (including those where the old rule was not live)", &all_post_off, &all_post_on);
    let share = |v: &Vec<B5Out>| {
        let (u, w) = aff.iter().fold((0, 0), |a, i| {
            let x = wrong_uses_after(&v[*i], t);
            (a.0 + x.0, a.1 + x.1)
        });
        if u > 0 { w as f64 / u as f64 } else { 0.0 }
    };
    println!("\nShare of rule answers after the change that are wrong (pooled): no pruning {:.3}, pruning {:.3}", share(&off), share(&on));
    let ret: Vec<(usize, &(u64, String, String))> = aff.iter().flat_map(|i| on[*i].retired.iter().map(move |r| (*i, r))).collect();
    let mut reasons: std::collections::BTreeMap<String, usize> = Default::default();
    for (_, r) in &ret {
        *reasons.entry(r.2.clone()).or_default() += 1;
    }
    println!("Retirements in the pruning arm (affected worlds): {} ({reasons:?}); worlds with at least one retirement after the change: {} of {}", ret.len(), aff.iter().filter(|i| on[**i].retired.iter().any(|r| r.0 > drift_step(&on[**i]))).count(), aff.len());
    let lag: Vec<f64> = aff.iter().filter_map(|i| on[*i].retired.iter().find(|r| r.0 > drift_step(&on[*i])).map(|r| (r.0 - drift_step(&on[*i])) as f64)).collect();
    println!("Steps from the change to the first retirement after it: median {:?} over {} worlds", median(lag.clone()), lag.len());
    println!("\nRule count over time (live rules, mean over ALL worlds):\n");
    println!("| after step | no pruning | pruning |");
    println!("|---|---|---|");
    let marks = [25usize, 50, 100, 150, 200, 300, 400, 600];
    for m in marks {
        let f = |v: &Vec<B5Out>| v.iter().map(|r| live_at(r, &[m])[0] as f64).sum::<f64>() / n as f64;
        println!("| {m} | {:.2} | {:.2} |", f(&off), f(&on));
    }
    let (mo, mp) = (off.iter().map(|r| r.max_live()).max().unwrap_or(0), on.iter().map(|r| r.max_live()).max().unwrap_or(0));
    let (fo, fp) = (off.iter().map(|r| r.live.last().copied().unwrap_or(0) as f64).sum::<f64>() / n as f64, on.iter().map(|r| r.live.last().copied().unwrap_or(0) as f64).sum::<f64>() / n as f64);
    println!("Most live rules in any world at any step: no pruning {mo}, pruning {mp} (cap {}); mean live rules at the end: {fo:.2} vs {fp:.2}", if cap == usize::MAX { "none".to_string() } else { cap.to_string() });
    let refu = |v: &Vec<B5Out>| v.iter().map(|r| r.refusals.len()).sum::<usize>();
    println!("Controller refusals (cap hits) over all worlds: no pruning {}, pruning {}", refu(&off), refu(&on));
    println!("\nStationary control (same world without the rule change), pruning on vs off, n = {n}:\n");
    let sp = |v: &Vec<B5Out>| v.iter().map(|r| r.per_budget()).collect::<Vec<_>>();
    println!("| measure | no pruning | pruning | paired difference |");
    println!("|---|---|---|---|");
    row("reward per energy unit", &sp(&st_off), &sp(&st_on));
    let rt: Vec<f64> = st_on.iter().map(|r| r.retired.len() as f64).collect();
    let rm: Vec<String> = st_on.iter().flat_map(|r| r.retired.iter().map(|x| x.2.clone())).collect();
    println!("\nRetirements per stationary world with pruning on: mean {:.2}, total {} (reasons: {} merged, {} low_success)", mean_ci(&rt).0, rm.len(), rm.iter().filter(|x| *x == "merged").count(), rm.iter().filter(|x| *x == "low_success").count());
    result
}

pub fn caps_report(seeds: std::ops::RangeInclusive<u64>, budget: i64) {
    let n = seeds.clone().count();
    let lim = Limits { max_live_rules: 3, max_blocks: 2, max_candidates: 60, compose: true, prune: false, ..Default::default() };
    let mut pol = Policy::new(150);
    pol.max_energy = Some(1000);
    println!("\n### Caps never exceeded (extended2 world, budget {budget}, n = {n} worlds)\n");
    println!("Caps: live rules 3, building blocks 2, search candidates per pass 60, steps per run 150, energy per run 1000.\n");
    let runs: Vec<B5Out> = seeds.clone().map(|s| run_b5(s, budget, false, Some(lim.clone()), EVERY, &pol)).collect();
    let max_live = runs.iter().map(|r| r.max_live()).max().unwrap();
    let max_steps = runs.iter().map(|r| r.steps).max().unwrap();
    let max_cands = runs.iter().map(|r| r.max_candidates_seen).max().unwrap();
    let max_blocks = runs.iter().flat_map(|r| r.accepted.iter().map(|a| a.1.nblocks)).max().unwrap_or(0);
    let over = runs.iter().map(|r| (r.cost - 1000).max(0)).max().unwrap();
    let stops = runs.iter().fold([0; 3], |mut a, r| {
        match r.stopped {
            Some(StopReason::MaxSteps) => a[0] += 1,
            Some(StopReason::EnergyCap) => a[1] += 1,
            _ => a[2] += 1,
        }
        a
    });
    println!("| cap | limit | largest value seen in any world | exceeded |");
    println!("|---|---|---|---|");
    println!("| live rules | 3 | {max_live} | {} |", if max_live > 3 { "YES" } else { "no" });
    println!("| steps per run | 150 | {max_steps} | {} |", if max_steps > 150 { "YES" } else { "no" });
    println!("| search candidates per pass | 60 | {max_cands} | {} |", if max_cands > 60 { "YES" } else { "no" });
    println!("| building blocks used by a composed rule | 2 | {max_blocks} | {} |", if max_blocks > 2 { "YES" } else { "no" });
    println!("| energy per run (a step that starts below the cap may end above it) | 1000 | overshoot at most {over} units | {} |", if over > 0 { "overshoot by the last step" } else { "no" });
    println!("\nRuns ended by: step limit {}, energy cap {}, other {}", stops[0], stops[1], stops[2]);
    let mut by: std::collections::BTreeMap<String, usize> = Default::default();
    for r in &runs {
        for (k, v) in refusals_by_cap(r) {
            *by.entry(k).or_default() += v;
        }
    }
    println!("Refusals recorded (each is a log event and an audit row in the logged loop): {by:?}");
}

pub fn kill_report(seeds: std::ops::RangeInclusive<u64>, budget: i64) {
    println!("\n### Kill switch (extended2 world, budget {budget})\n");
    let mut stopped_exact = 0;
    let n = seeds.clone().count();
    for s in seeds {
        let calls = Arc::new(AtomicU64::new(0));
        let c2 = calls.clone();
        let mut pol = Policy::new(100_000);
        // the switch turns on during the 41st check: no more than 40 steps may run
        pol.kill = Some(KillCheck(Arc::new(move || c2.fetch_add(1, Ordering::SeqCst) + 1 >= 41)));
        let r = run_b5(s, budget, false, Some(comp_limits()), EVERY, &pol);
        if r.steps == 40 && r.stopped == Some(StopReason::Killed) {
            stopped_exact += 1;
        }
    }
    println!("Runs stopped after exactly 40 steps by a switch that turned on before step 41: {stopped_exact} of {n}");
}

pub fn tune() {
    println!("\n### Pruning settings, tuning seeds 101..120 (never used for the report)\n");
    for budget in [3000i64, 6000] {
        println!("budget {budget}:");
        for (window, min_n, threshold) in [(10usize, 6u32, 0.5f64), (10, 6, 0.3), (8, 5, 0.5), (10, 8, 0.5), (16, 8, 0.4), (6, 4, 0.5), (10, 4, 0.5)] {
            let cfg = PruneCfg { window, min_n, threshold, merge_min: 5 };
            prune_scenario("tuning", 101..=120, budget, cfg, usize::MAX, false);
        }
    }
}

pub fn main_b5(args: &[String]) {
    let mode = args.get(2).map(|s| s.as_str()).unwrap_or("b5");
    let cfg = PruneCfg::default();
    let all = mode == "b5";
    if mode == "b5-tune" {
        tune();
        return;
    }
    if all || mode == "b5-compose" {
        for budget in [3000i64, 6000] {
            compose_report("report seeds 1..20", 1..=20, budget);
        }
    }
    if all || mode == "b5-prune" {
        for budget in [3000i64, 6000] {
            prune_scenario("report seeds 1..20", 1..=20, budget, cfg, usize::MAX, true);
            prune_scenario("report seeds 1..20, live-rule cap 8", 1..=20, budget, cfg, 8, true);
        }
    }
    if all || mode == "b5-caps" {
        caps_report(1..=20, 3000);
    }
    if all || mode == "b5-kill" {
        kill_report(1..=20, 3000);
    }
}
