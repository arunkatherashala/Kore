//! Measures the learned chooser against baselines in the simulated world and
//! prints the tables recorded in docs/measured.md (synthetic world, author-defined rewards).
//!   cargo run -p kore-self --example measure_being --offline --release
//! Report seeds 1..=20; tuning seeds 101..=120 (used only to choose the exploration constant).

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;

use being::chooser::DEFAULT_EXPLORE_C;
use being::measure::*;
use being::world::{Domain, DOMAINS};

fn col(rows: &[SeedRow], f: impl Fn(&SeedRow) -> f64) -> (f64, f64, f64) {
    mean_ci(&rows.iter().map(f).collect::<Vec<_>>())
}

fn line(name: &str, rows: &[SeedRow], f: impl Fn(&SeedRow) -> f64) {
    let (m, lo, hi) = col(rows, &f);
    let min = rows.iter().map(&f).fold(f64::MAX, f64::min);
    let max = rows.iter().map(&f).fold(f64::MIN, f64::max);
    println!("| {name} | {m:.3} | [{lo:.3}, {hi:.3}] | {min:.3}-{max:.3} |");
}

fn report(label: &str, seeds: std::ops::RangeInclusive<u64>, c: f64) {
    let rows: Vec<SeedRow> = seeds.map(|s| seed_row(s, c, BUDGET)).collect();
    println!("\n### {label} (c = {c}, budget {BUDGET}, n = {} worlds)\n", rows.len());
    let mut best = [0; 4];
    for r in &rows {
        best[r.best.idx()] += 1;
    }
    println!("Hidden best domain per world: arithmetic {} / algebra {} / physics {} / pattern {}\n", best[0], best[1], best[2], best[3]);
    println!("| policy | mean reward per energy unit | 95% interval of the mean | per-world range |");
    println!("|---|---|---|---|");
    line("random domain", &rows, |r| r.random.per_budget());
    for (i, d) in DOMAINS.iter().enumerate() {
        line(&format!("always {}", d.name()), &rows, |r| r.always[i].per_budget());
    }
    line("oracle (hidden best domain)", &rows, |r| r.oracle.per_budget());
    line("learned chooser (UCB1)", &rows, |r| r.learned.per_budget());
    println!("\n| quantity | mean | 95% interval of the mean | range |");
    println!("|---|---|---|---|");
    line("regret per energy, learned vs oracle", &rows, |r| r.oracle.per_budget() - r.learned.per_budget());
    line("regret per energy, random vs oracle", &rows, |r| r.oracle.per_budget() - r.random.per_budget());
    line("total reward, learned (points)", &rows, |r| r.learned.reward as f64);
    line("total reward, random (points)", &rows, |r| r.random.reward as f64);
    line("share of picks on the best domain, steps 1-30", &rows, |r| share(&r.learned.picks, r.best, 0, 30));
    line("share of picks on the best domain, steps 31-80", &rows, |r| share(&r.learned.picks, r.best, 30, 80));
    line("share of picks on the best domain, step 81 on", &rows, |r| share(&r.learned.picks, r.best, 80, usize::MAX));
    line("share of picks on the best domain, random policy", &rows, |r| share(&r.random.picks, r.best, 0, usize::MAX));
    let conc: Vec<f64> = rows.iter().filter_map(|r| concentration_step(&r.learned.picks, r.best)).map(|x| x as f64).collect();
    let mut s = conc.clone();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("\nConcentration step (first step where the trailing 20 picks are >= 60% best domain): reached in {} of {} worlds, median {:?}", conc.len(), rows.len(), s.get(s.len() / 2));
    let beats = rows.iter().filter(|r| r.learned.per_budget() > r.random.per_budget()).count();
    println!("Learned beats random in {beats} of {} worlds.", rows.len());
    for (i, d) in DOMAINS.iter().enumerate() {
        let w = rows.iter().filter(|r| r.learned.per_budget() > r.always[i].per_budget()).count();
        println!("Learned beats always-{} in {w} of {} worlds.", d.name(), rows.len());
    }
    let steps: Vec<f64> = rows.iter().map(|r| r.learned.steps as f64).collect();
    println!("Steps per budget (learned): mean {:.1}", steps.iter().sum::<f64>() / steps.len() as f64);
}

fn mrow(rows: &[MenuRow], f: impl Fn(&MenuRow) -> f64) -> (f64, f64, f64) {
    mean_ci(&rows.iter().map(f).collect::<Vec<_>>())
}

fn menu_tune() {
    for budget in [BUDGET, LONG_BUDGET] {
        let base: Vec<MenuRow> = (101..=120).map(|s| menu_row(s, budget, 0.0, 0.0)).collect();
        println!("budget {budget}: random {:.3} greedy {:.3} ucb {:.3} oracle {:.3} (tuning seeds 101..120)", mrow(&base, |r| r.random.per_budget()).0, mrow(&base, |r| r.greedy.per_budget()).0, mrow(&base, |r| r.ucb.per_budget()).0, mrow(&base, |r| r.oracle.per_budget()).0);
        for beta in [1.0, 1.5, 2.0, 3.0] {
            for skip in [0.0, 0.6, 0.8, 1.0, 1.2] {
                let rows: Vec<MenuRow> = (101..=120).map(|s| menu_row(s, budget, beta, skip)).collect();
                println!("  beta {beta} skip_frac {skip}: model {:.3}", mrow(&rows, |r| r.model.per_budget()).0);
            }
        }
    }
}

fn mline(name: &str, rows: &[MenuRow], f: impl Fn(&MenuRow) -> f64) {
    let (m, lo, hi) = mrow(rows, &f);
    println!("| {name} | {m:.3} | [{lo:.3}, {hi:.3}] |");
}

fn menu_report(label: &str, seeds: std::ops::RangeInclusive<u64>, budget: i64, beta: f64, skip: f64) {
    let rows: Vec<MenuRow> = seeds.map(|s| menu_row(s, budget, beta, skip)).collect();
    println!("
### Menu world, {label} (budget {budget}, beta {beta}, skip_frac {skip}, n = {} worlds)
", rows.len());
    println!("| policy | mean reward per energy unit | 95% interval of the mean |");
    println!("|---|---|---|");
    mline("random (offer and strategy)", &rows, |r| r.random.per_budget());
    mline("greedy by listed reward per cost (careful)", &rows, |r| r.greedy.per_budget());
    mline("greedy by listed reward per cost (fast always)", &rows, |r| r.greedy_fast.per_budget());
    mline("greedy by listed reward per cost (deep always)", &rows, |r| r.greedy_deep.per_budget());
    mline("UCB1 domain chooser (careful)", &rows, |r| r.ucb.per_budget());
    mline("model learner, never skips", &rows, |r| r.model_noskip.per_budget());
    mline("model learner (with skipping)", &rows, |r| r.model.per_budget());
    mline("oracle (knows hidden success rates and energy)", &rows, |r| r.oracle.per_budget());
    println!("
| regret per energy vs oracle | mean | 95% interval of the mean |");
    println!("|---|---|---|");
    mline("random", &rows, |r| r.oracle.per_budget() - r.random.per_budget());
    mline("greedy", &rows, |r| r.oracle.per_budget() - r.greedy.per_budget());
    mline("greedy fast", &rows, |r| r.oracle.per_budget() - r.greedy_fast.per_budget());
    mline("greedy deep", &rows, |r| r.oracle.per_budget() - r.greedy_deep.per_budget());
    mline("UCB1", &rows, |r| r.oracle.per_budget() - r.ucb.per_budget());
    mline("model learner, never skips", &rows, |r| r.oracle.per_budget() - r.model_noskip.per_budget());
    mline("model learner (with skipping)", &rows, |r| r.oracle.per_budget() - r.model.per_budget());
    println!("
| paired difference (model minus other), reward per energy | mean | 95% interval of the mean | model wins |");
    println!("|---|---|---|---|");
    for (name, f) in [("model never skips", (|r: &MenuRow| r.model_noskip.per_budget()) as fn(&MenuRow) -> f64), ("random", (|r: &MenuRow| r.random.per_budget()) as fn(&MenuRow) -> f64), ("greedy", |r| r.greedy.per_budget()), ("greedy fast", |r| r.greedy_fast.per_budget()), ("greedy deep", |r| r.greedy_deep.per_budget()), ("UCB1", |r| r.ucb.per_budget())] {
        let (m, lo, hi) = mrow(&rows, |r| r.model.per_budget() - f(r));
        let w = rows.iter().filter(|r| r.model.per_budget() > f(r)).count();
        println!("| {name} | {m:+.3} | [{lo:+.3}, {hi:+.3}] | {w} of {} |", rows.len());
    }
    println!("
Learning curve, reward per energy by step window (mean over worlds):
");
    println!("| steps | random | greedy | UCB1 | model | oracle |");
    println!("|---|---|---|---|---|---|");
    let wins: Vec<(usize, usize)> = if budget > 1000 { vec![(0, 25), (25, 50), (50, 100), (100, 200), (200, 400), (400, 100000)] } else { vec![(0, 15), (15, 30), (30, 50), (50, 100000)] };
    for (a, b) in wins {
        let w = |f: fn(&MenuRow) -> &MenuRun| mrow(&rows, |r| f(r).window(a, b)).0;
        let b_txt = if b >= 100000 { "end".to_string() } else { b.to_string() };
        println!("| {}-{} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} |", a + 1, b_txt, w(|r| &r.random), w(|r| &r.greedy), w(|r| &r.ucb), w(|r| &r.model), w(|r| &r.oracle));
    }
    let n = rows.len() as f64;
    let st: Vec<f64> = (0..3).map(|i| rows.iter().map(|r| r.model.strategies[i] as f64).sum::<f64>() / n).collect();
    println!("
Model learner per world: {:.1} fast / {:.1} careful / {:.1} deep attempts, {:.1} skips, {:.1} attempts in total", st[0], st[1], st[2], rows.iter().map(|r| r.model.skips as f64).sum::<f64>() / n, st.iter().sum::<f64>());
    let runs: Vec<&MenuRun> = rows.iter().map(|r| &r.model).collect();
    let cal = calibration(&runs, 10);
    println!("
Calibration of the model's predicted success (all attempts after the 10th of each run; each prediction is made before its outcome is seen, so every point is out of sample), n = {} attempts:
", cal.n);
    println!("| predicted bin | attempts | mean predicted | observed rate |");
    println!("|---|---|---|---|");
    for (lo, c, p, o) in cal.bins.iter().filter(|b| b.1 > 0) {
        println!("| {:.1}-{:.1} | {c} | {p:.3} | {o:.3} |", lo, lo + 0.1);
    }
    println!("
Brier score of the model: {:.4}; Brier of predicting the running overall success rate: {:.4} (lower is better)", cal.brier, cal.brier_base_rate);
}

fn drow(rows: &[DiscRow], f: impl Fn(&DiscRow) -> f64) -> (f64, f64, f64) {
    mean_ci(&rows.iter().map(f).collect::<Vec<_>>())
}

fn dline(name: &str, rows: &[DiscRow], f: impl Fn(&DiscRow) -> f64) {
    let (m, lo, hi) = drow(rows, f);
    println!("| {name} | {m:.3} | [{lo:.3}, {hi:.3}] |");
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(v[v.len() / 2])
}

fn discover_report(label: &str, seeds: std::ops::RangeInclusive<u64>, budget: i64, beta: f64, every: u64, bonus: i64, curiosity: f64) {
    use being::world::Kind;
    let rows: Vec<DiscRow> = seeds.clone().map(|s| disc_row(s, budget, beta, every, bonus, curiosity)).collect();
    let n = rows.len();
    println!("\n### Extended pattern world (pattern reward x{bonus}), {label} (budget {budget}, beta {beta}, discovery pass every {every} steps, curiosity {curiosity}, n = {n} worlds)\n");
    println!("| policy | mean reward per energy unit | 95% interval of the mean |");
    println!("|---|---|---|");
    dline("random (offer and fixed strategy)", &rows, |r| r.random.run.per_budget());
    dline("model learner, fixed solvers only (no discovery)", &rows, |r| r.nodisc.run.per_budget());
    dline("model learner + discovery", &rows, |r| r.disc.run.per_budget());
    dline("oracle, fixed solvers only (knows hidden success rates)", &rows, |r| r.oracle_fixed.run.per_budget());
    dline("oracle with perfect rules (knows rules and success rates)", &rows, |r| r.oracle_rules.run.per_budget());
    println!("\n| paired difference, reward per energy | mean | 95% interval of the mean | first minus second wins |");
    println!("|---|---|---|---|");
    type Pick = fn(&DiscRow) -> f64;
    let pairs: [(&str, Pick, Pick); 4] = [
        ("discovery minus no discovery", |r| r.disc.run.per_budget(), |r| r.nodisc.run.per_budget()),
        ("discovery minus random", |r| r.disc.run.per_budget(), |r| r.random.run.per_budget()),
        ("no discovery minus random", |r| r.nodisc.run.per_budget(), |r| r.random.run.per_budget()),
        ("oracle with rules minus discovery (regret)", |r| r.oracle_rules.run.per_budget(), |r| r.disc.run.per_budget()),
    ];
    for (name, a, b) in pairs {
        let (m, lo, hi) = drow(&rows, |r| a(r) - b(r));
        let (w, l) = (rows.iter().filter(|r| a(r) > b(r)).count(), rows.iter().filter(|r| a(r) < b(r)).count());
        println!("| {name} | {m:+.3} | [{lo:+.3}, {hi:+.3}] | {w} wins, {l} losses, {} ties of {n} |", n - w - l);
    }
    println!("\nLearning curve, reward per energy by step window (mean over worlds):\n");
    println!("| steps | random | no discovery | discovery | oracle with rules |");
    println!("|---|---|---|---|---|");
    let wins: Vec<(usize, usize)> = if budget > 1000 { vec![(0, 50), (50, 100), (100, 200), (200, 400), (400, 100000)] } else { vec![(0, 30), (30, 60), (60, 100), (100, 100000)] };
    for (a, b) in wins {
        let w = |f: fn(&DiscRow) -> &ExtRun| drow(&rows, |r| f(r).run.window(a, b)).0;
        let b_txt = if b >= 100000 { "end".to_string() } else { b.to_string() };
        println!("| {}-{} | {:.3} | {:.3} | {:.3} | {:.3} |", a + 1, b_txt, w(|r| &r.random), w(|r| &r.nodisc), w(|r| &r.disc), w(|r| &r.oracle_rules));
    }
    println!("\nRule discovery by the model learner (families found are checked against hidden truth on fresh tasks; the being never sees this):\n");
    println!("| hidden family (expressible) | worlds where found | median step count at first coverage |");
    println!("|---|---|---|");
    let names = ["affine geometric (a*t+c)", "cubic in n", "second-order recurrence", "tribonacci-like", "alternating add", "period-3 cycle"];
    let mut found_total = 0;
    for (k, nm) in names.iter().enumerate() {
        let xs: Vec<f64> = rows.iter().filter_map(|r| r.analysis.first[k]).map(|x| x as f64).collect();
        found_total += xs.len();
        println!("| {nm} | {} of {n} | {} |", xs.len(), median(xs.clone()).map_or("-".to_string(), |m| format!("{m:.0}")));
    }
    println!("\nRecall: {found_total} of {} (family, world) pairs found by the end = {:.1}%", n * 6, 100.0 * found_total as f64 / (n * 6) as f64);
    let (acc, ex, wr): (usize, usize, usize) = rows.iter().fold((0, 0, 0), |a, r| (a.0 + r.analysis.rules, a.1 + r.analysis.exact, a.2 + r.analysis.wrong));
    println!("Rules accepted in total: {acc}; exactly correct on 300 fresh tasks of their signature: {ex}; wrong or overfit: {wr}; precision {:.1}%", 100.0 * ex as f64 / acc.max(1) as f64);
    for r in &rows {
        for w in &r.analysis.wrong_rules {
            println!("  wrong rule in world {}: signature {} form {} support train {} held-out {}; applicable-and-wrong on {} of 300 fresh tasks", r.seed, w.0, w.1, w.2, w.3, w.4);
        }
    }
    println!("Worlds with at least one wrong rule: {} of {n}; mean rules per world {:.2}", rows.iter().filter(|r| r.analysis.wrong > 0).count(), acc as f64 / n as f64);
    println!("\nNegative controls inside the world (families outside the grammar; fresh tasks, final rule book):\n");
    println!("| family | share of tasks where some rule is applied | wrong share of those applications |");
    println!("|---|---|---|");
    for (i, k) in [Kind::NegProd, Kind::NegPrimes].iter().enumerate() {
        let (a, wr) = (drow(&rows, |r| r.analysis.neg[i].0).0, drow(&rows, |r| r.analysis.neg[i].1).0);
        println!("| {k:?} | {:.3} | {:.3} |", a, wr);
    }
    let (d, od) = (drow(&rows, |r| r.disc.run.strategies.iter().sum::<u64>() as f64).0, drow(&rows, |r| r.oracle_rules.run.strategies.iter().sum::<u64>() as f64).0);
    println!("\nAttempts per world: discovery {d:.1}, oracle with rules {od:.1}");
    let used: f64 = rows.iter().map(|r| r.disc.run.strategies[3] as f64).sum::<f64>() / n as f64;
    println!("Attempts with the `discovered` strategy per world (discovery run): {used:.1}");
}

fn control_report(seeds: std::ops::RangeInclusive<u64>) {
    use being::discover::search;
    println!("\n### Controls on revealed examples only (seeds {}..{})\n", seeds.start(), seeds.end());
    for n in [30u64, 100, 300] {
        let (mut accepted, mut bad_seeds) = (0usize, 0usize);
        for s in seeds.clone() {
            let ex = control_examples(s, &UNEXPRESSIBLE, n, 5_000_000);
            let f = search(&ex);
            accepted += f.len();
            bad_seeds += (!f.is_empty()) as usize;
        }
        println!("Unexpressible families only ({} examples each of product recurrence and consecutive primes): {accepted} rules accepted in total, {bad_seeds} of {} worlds accepted anything", n, seeds.clone().count());
    }
    for n in [8u64, 16, 30, 60] {
        let mut found = [0usize; 6];
        for s in seeds.clone() {
            for (k, f) in EXPRESSIBLE.iter().enumerate() {
                let ex = control_examples(s, &[*f], n, 5_000_000);
                let w = being::world::World::with_extended(s, true);
                let book: Vec<being::discover::Rule> = search(&ex).into_iter().map(|f| being::discover::Rule::new(f.sig, f.form, f.train_ok, f.val_ok, 0)).collect();
                found[k] += (coverage(&w, &book, *f, COVER_N) >= COVER_MIN) as usize;
            }
        }
        println!("Each expressible family alone, {n} examples: found in {:?} of {} worlds (order: affine geometric, cubic, recurrence, tribonacci, alternating add, period-3)", found, seeds.clone().count());
    }
}

fn discover_tune(bonus: i64) {
    for budget in [BUDGET, LONG_BUDGET] {
        for every in [10u64] {
            for (beta, cur) in [(1.5, 0.0), (1.5, 0.2), (1.5, 0.4), (1.5, 0.8), (3.0, 0.0), (3.0, 0.4)] {
                let rows: Vec<DiscRow> = (101..=120).map(|s| disc_row(s, budget, beta, every, bonus, cur)).collect();
                println!("budget {budget} every {every} beta {beta} curiosity {cur}: discovery {:.3} no-discovery {:.3} oracle-rules {:.3} (tuning seeds 101..120)", drow(&rows, |r| r.disc.run.per_budget()).0, drow(&rows, |r| r.nodisc.run.per_budget()).0, drow(&rows, |r| r.oracle_rules.run.per_budget()).0);
            }
        }
    }
}


// ---- Track B3: drifting world ----

use being::measure::{run_drift, AdaptSpec, DriftRow, DriftRun};
use being::planner::ModelChooser;

fn dcol(rows: &[DriftRow], f: impl Fn(&DriftRow) -> f64) -> (f64, f64, f64) {
    mean_ci(&rows.iter().map(f).collect::<Vec<_>>())
}

fn dl(name: &str, rows: &[DriftRow], f: impl Fn(&DriftRow) -> f64) {
    let (m, lo, hi) = dcol(rows, f);
    println!("| {name} | {m:.3} | [{lo:.3}, {hi:.3}] |");
}

fn med_txt(v: &mut Vec<f64>, total: usize) -> String {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if v.is_empty() {
        format!("never (0 of {total})")
    } else {
        format!("{:.0} ({} of {total} reached)", v[v.len() / 2], v.len())
    }
}

type P = fn(&DriftRow) -> &DriftRun;

fn drift_report(label: &str, seeds: std::ops::RangeInclusive<u64>, budget: i64, forget: &AdaptSpec, full: &AdaptSpec) {
    use being::measure::{first_units_attempt, recovery, units_competence};
    let rows: Vec<DriftRow> = seeds.clone().map(|s| drift_row(s, budget, true, forget, full)).collect();
    let n = rows.len();
    println!("\n### Drifting world, {label} (budget {budget}, half-life {} attempts, CUSUM k {} h {}, keep {}, novelty {}, n = {n} worlds)\n", full.half_life, full.k, full.h, full.keep, full.novelty);
    let mean_at: Vec<f64> = (0..3).map(|i| rows.iter().map(|r| r.at[i] as f64).sum::<f64>() / n as f64).collect();
    println!("Changes happen after about {:.0} (new domain), {:.0} (profile shift) and {:.0} (fast degrades) energy units of {budget}.\n", mean_at[0], mean_at[1], mean_at[2]);
    println!("| policy | mean reward per energy unit | 95% interval of the mean |");
    println!("|---|---|---|");
    let pols: [(&str, P); 7] = [
        ("random", |r| &r.random),
        ("greedy by listed reward per cost", |r| &r.greedy),
        ("UCB1 domain chooser", |r| &r.ucb),
        ("B1 model learner (no adaptation)", |r| &r.off),
        ("B1 + forgetting only", |r| &r.forget),
        ("adaptive learner (forgetting + change detection + cold start)", |r| &r.full),
        ("oracle that knows the schedule", |r| &r.oracle),
    ];
    for (nm, f) in pols {
        dl(nm, &rows, |r| f(r).per_budget());
    }
    println!("\nReward per energy by regime (mean over worlds; regime 0 = before any change):\n");
    println!("| policy | regime 0 | regime 1 (new domain) | regime 2 (shift) | regime 3 (fast degraded) |");
    println!("|---|---|---|---|---|");
    let bounds = |r: &DriftRow| [0, r.at[0], r.at[1], r.at[2], i64::MAX];
    for (nm, f) in pols {
        let v: Vec<f64> = (0..4)
            .map(|i| {
                dcol(&rows, |r| {
                    let b = bounds(r);
                    f(r).window_e(b[i], b[i + 1])
                })
                .0
            })
            .collect();
        println!("| {nm} | {:.3} | {:.3} | {:.3} | {:.3} |", v[0], v[1], v[2], v[3]);
    }
    println!("\nRegret per energy unit against the oracle, and paired comparisons (reward per energy):\n");
    println!("| quantity | mean | 95% interval of the mean | first minus second: wins / losses / ties |");
    println!("|---|---|---|---|");
    for (nm, f) in &pols[..6] {
        let (m, lo, hi) = dcol(&rows, |r| r.oracle.per_budget() - f(r).per_budget());
        println!("| regret of {nm} | {m:.3} | [{lo:.3}, {hi:.3}] | |");
    }
    let pairs: [(&str, P, P); 7] = [
        ("adaptive minus B1", |r| &r.full, |r| &r.off),
        ("adaptive minus forgetting only", |r| &r.full, |r| &r.forget),
        ("forgetting only minus B1", |r| &r.forget, |r| &r.off),
        ("adaptive minus UCB1", |r| &r.full, |r| &r.ucb),
        ("adaptive minus greedy", |r| &r.full, |r| &r.greedy),
        ("B1 minus UCB1", |r| &r.off, |r| &r.ucb),
        ("adaptive minus random", |r| &r.full, |r| &r.random),
    ];
    for (nm, a, b) in pairs {
        let (m, lo, hi) = dcol(&rows, |r| a(r).per_budget() - b(r).per_budget());
        let (w, l) = (rows.iter().filter(|r| a(r).per_budget() > b(r).per_budget() + 1e-9).count(), rows.iter().filter(|r| a(r).per_budget() < b(r).per_budget() - 1e-9).count());
        println!("| {nm} | {m:+.3} | [{lo:+.3}, {hi:+.3}] | {w} / {l} / {} |", n - w - l);
    }
    println!("\nPaired difference adaptive minus B1 by regime (reward per energy, mean over worlds):\n");
    println!("| regime | difference | 95% interval |");
    println!("|---|---|---|");
    for i in 0..4 {
        let (m, lo, hi) = dcol(&rows, |r| {
            let b = bounds(r);
            r.full.window_e(b[i], b[i + 1]) - r.off.window_e(b[i], b[i + 1])
        });
        println!("| {i} | {m:+.3} | [{lo:+.3}, {hi:+.3}] |");
    }
    println!("\nRecovery: energy units after each change until the trailing 100-unit reward per energy reaches 90% of the oracle's mean in that regime (median over worlds that reached it):\n");
    println!("| change | UCB1 | B1 | forgetting only | adaptive |");
    println!("|---|---|---|---|---|");
    let names = ["new domain appears", "profile shift", "fast degrades"];
    for i in 0..3 {
        let mut cells = vec![];
        for f in [(|r: &DriftRow| &r.ucb) as P, |r| &r.off, |r| &r.forget, |r| &r.full] {
            let mut v: Vec<f64> = rows
                .iter()
                .filter_map(|r| {
                    let to = if i < 2 { r.at[i + 1] } else { budget - 60 };
                    recovery(f(r), &r.oracle, r.at[i], to, 100, 0.9)
                })
                .map(|x| x as f64)
                .collect();
            cells.push(med_txt(&mut v, n));
        }
        println!("| {} | {} | {} | {} | {} |", names[i], cells[0], cells[1], cells[2], cells[3]);
    }
    println!("\nNew domain (units), measured from the moment it appears:\n");
    println!("| learner | energy until first attempt in units (median) | energy until trailing 10 units attempts earn 80% of the oracle's rate (median) |");
    println!("|---|---|---|");
    for (nm, f) in [("UCB1", (|r: &DriftRow| &r.ucb) as P), ("B1", |r| &r.off), ("forgetting only", |r| &r.forget), ("adaptive", |r| &r.full)] {
        let mut a: Vec<f64> = rows.iter().filter_map(|r| first_units_attempt(f(r), r.at[0])).map(|x| x as f64).collect();
        let mut c: Vec<f64> = rows.iter().filter_map(|r| units_competence(f(r), &r.oracle, r.at[0], 0.8)).map(|x| x as f64).collect();
        println!("| {nm} | {} | {} |", med_txt(&mut a, n), med_txt(&mut c, n));
    }
    println!("\nChange detector of the adaptive learner (alarms are flagged changes of a (domain, strategy) group):\n");
    let (mut pre, mut mid, mut deg_fast, mut deg_other) = (0, 0, 0, 0);
    let mut delays = vec![];
    for r in &rows {
        for a in &r.full.alarms {
            if a.energy < r.at[0] {
                pre += 1
            } else if a.energy < r.at[2] {
                mid += 1
            } else if a.strategy == being::solver::Strategy::Fast && a.drop {
                deg_fast += 1
            } else {
                deg_other += 1
            }
        }
        if let Some(a) = r.full.alarms.iter().find(|a| a.energy >= r.at[2] && a.strategy == being::solver::Strategy::Fast && a.drop) {
            delays.push((a.energy - r.at[2]) as f64);
        }
    }
    println!(
        "Alarms per world: before any change {:.2}; between the first change and the degradation {:.2}; after the degradation on `fast` (drop) {:.2}, other {:.2}",
        pre as f64 / n as f64,
        mid as f64 / n as f64,
        deg_fast as f64 / n as f64,
        deg_other as f64 / n as f64
    );
    println!("Degradation of `fast` detected (first drop alarm on fast after the change): delay in energy units: {}", med_txt(&mut delays.clone(), n));
}

fn stationary_report(label: &str, seeds: std::ops::RangeInclusive<u64>, budget: i64, forget: &AdaptSpec, full: &AdaptSpec) {
    let rows: Vec<DriftRow> = seeds.map(|s| drift_row(s, budget, false, forget, full)).collect();
    let n = rows.len();
    println!("\n### Stationary world (no drift), {label} (budget {budget}, n = {n} worlds): what adaptation costs\n");
    println!("| policy | mean reward per energy unit | 95% interval of the mean |");
    println!("|---|---|---|");
    dl("UCB1 domain chooser", &rows, |r| r.ucb.per_budget());
    dl("B1 model learner (no adaptation)", &rows, |r| r.off.per_budget());
    dl("B1 + forgetting only", &rows, |r| r.forget.per_budget());
    dl("adaptive learner", &rows, |r| r.full.per_budget());
    dl("oracle", &rows, |r| r.oracle.per_budget());
    println!("\n| paired difference | mean | 95% interval of the mean | wins / losses / ties |");
    println!("|---|---|---|---|");
    let pairs: [(&str, fn(&DriftRow) -> f64, fn(&DriftRow) -> f64); 2] = [
        ("forgetting only minus B1", |r| r.forget.per_budget(), |r| r.off.per_budget()),
        ("adaptive minus B1", |r| r.full.per_budget(), |r| r.off.per_budget()),
    ];
    for (nm, a, b) in pairs {
        let (m, lo, hi) = dcol(&rows, |r| a(r) - b(r));
        let (w, l) = (rows.iter().filter(|r| a(r) > b(r) + 1e-9).count(), rows.iter().filter(|r| a(r) < b(r) - 1e-9).count());
        println!("| {nm} | {m:+.3} | [{lo:+.3}, {hi:+.3}] | {w} / {l} / {} |", n - w - l);
    }
    let total: usize = rows.iter().map(|r| r.full.alarms.len()).sum();
    let with: usize = rows.iter().filter(|r| !r.full.alarms.is_empty()).count();
    let energy: i64 = rows.iter().map(|r| r.full.cost).sum();
    println!(
        "\nFalse alarms of the adaptive learner (no change exists): {total} alarms in {n} worlds ({:.2} per world, {:.2} per 1000 energy units); {with} of {n} worlds had at least one.",
        total as f64 / n as f64,
        1000.0 * total as f64 / energy as f64
    );
}

fn drift_tune(second: bool) {
    let budget = 6000;
    let seeds = 101..=120u64;
    let eval = |spec: &AdaptSpec| {
        let (mut d, mut s, mut alarms_s, mut alarms_d) = (0.0, 0.0, 0usize, 0usize);
        for seed in seeds.clone() {
            let mk = |sp: &AdaptSpec| ModelChooser { novelty: sp.novelty, ..ModelChooser::new(being::planner::DEFAULT_BETA) };
            let rd = run_drift(seed, budget, true, spec, &mut mk(spec));
            let rs = run_drift(seed, budget, false, spec, &mut mk(spec));
            d += rd.per_budget();
            s += rs.per_budget();
            alarms_d += rd.alarms.len();
            alarms_s += rs.alarms.len();
        }
        (d / 20.0, s / 20.0, alarms_d as f64 / 20.0, alarms_s as f64 / 20.0)
    };
    let off = eval(&AdaptSpec::off());
    println!("tuning seeds 101..120, budget {budget}: B1 (off): drift {:.3} stationary {:.3}", off.0, off.1);
    for hl in if second { vec![] } else { vec![50.0, 100.0, 150.0, 300.0, 600.0, 1200.0] } {
        let r = eval(&AdaptSpec { half_life: hl, ..AdaptSpec::forget() });
        println!("forget half-life {hl}: drift {:.3} stationary {:.3}", r.0, r.1);
    }
    for hl in if second { vec![600.0, 1200.0, 5000.0, 1e9] } else { vec![100.0, 150.0, 300.0, 600.0] } {
        for (k, h) in if second { vec![(0.75, 6.0), (1.0, 5.0), (1.0, 6.0), (1.0, 8.0)] } else { vec![(0.5, 3.0), (0.5, 4.0), (0.5, 6.0), (0.75, 4.0), (0.75, 6.0), (1.0, 5.0)] } {
            for keep in if second { vec![0.1, 0.3] } else { vec![0.1, 0.3] } {
                for novelty in if second { vec![0.25, 0.5] } else { vec![0.0, 0.25] } {
                    let r = eval(&AdaptSpec { half_life: hl, k, h, keep, novelty, ..AdaptSpec::full() });
                    println!("full hl {hl} k {k} h {h} keep {keep} novelty {novelty}: drift {:.3} stationary {:.3} alarms/run drift {:.2} stationary {:.2}", r.0, r.1, r.2, r.3);
                }
            }
        }
    }
}

fn drift_main() {
    let (forget, full) = (AdaptSpec::forget(), AdaptSpec::full());
    for budget in [6000i64] {
        drift_report("tuning seeds 101..120", 101..=120, budget, &forget, &full);
        drift_report("report seeds 1..20", 1..=20, budget, &forget, &full);
    }
    for budget in [600i64, 3000, 6000] {
        stationary_report("tuning seeds 101..120", 101..=120, budget, &forget, &full);
        stationary_report("report seeds 1..20", 1..=20, budget, &forget, &full);
    }
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|a| a == "drift-tune").unwrap_or(false) {
        drift_tune(args.get(2).map_or(false, |a| a == "2"));
        return;
    }
    if args.get(1).map(|a| a == "drift").unwrap_or(false) {
        drift_main();
        return;
    }
    if args.get(1).map(|a| a == "discover-tune").unwrap_or(false) {
        discover_tune(args.get(2).and_then(|x| x.parse().ok()).unwrap_or(2));
        return;
    }
    if args.get(1).map(|a| a == "discover-one").unwrap_or(false) {
        let g = |i: usize, d: i64| args.get(i).and_then(|x| x.parse::<i64>().ok()).unwrap_or(d);
        discover_report("single", (g(4, 101) as u64)..=(g(5, 120) as u64), g(2, 3000), being::planner::DEFAULT_BETA, DISCOVER_EVERY, g(3, 2), being::planner::DEFAULT_CURIOSITY);
        return;
    }
    if args.get(1).map(|a| a == "discover").unwrap_or(false) {
        use being::planner::{DEFAULT_BETA, DEFAULT_CURIOSITY};
        for budget in [BUDGET, LONG_BUDGET] {
            for bonus in [being::world::EXT_PATTERN_BONUS, 1] {
                discover_report("tuning seeds 101..120", 101..=120, budget, DEFAULT_BETA, DISCOVER_EVERY, bonus, DEFAULT_CURIOSITY);
                discover_report("report seeds 1..20", 1..=20, budget, DEFAULT_BETA, DISCOVER_EVERY, bonus, DEFAULT_CURIOSITY);
            }
        }
        control_report(101..=120);
        control_report(1..=20);
        return;
    }
    if args.get(1).map(|a| a == "tune-menu").unwrap_or(false) {
        menu_tune();
        return;
    }
    if args.get(1).map(|a| a == "menu").unwrap_or(false) {
        use being::planner::{DEFAULT_BETA, DEFAULT_SKIP_FRAC};
        for budget in [BUDGET, LONG_BUDGET] {
            menu_report("tuning seeds 101..120", 101..=120, budget, DEFAULT_BETA, DEFAULT_SKIP_FRAC);
            menu_report("report seeds 1..20", 1..=20, budget, DEFAULT_BETA, DEFAULT_SKIP_FRAC);
        }
        return;
    }
    if args.get(1).map(|a| a == "tune").unwrap_or(false) {
        for c in [0.0, 0.01, 0.02, 0.05, 0.1, 0.2, 0.3, 0.5, 0.8, 1.0, 1.5] {
            let rows: Vec<SeedRow> = (101..=120).map(|s| seed_row(s, c, BUDGET)).collect();
            let (m, _, _) = col(&rows, |r| r.learned.per_budget());
            let (o, _, _) = col(&rows, |r| r.oracle.per_budget());
            println!("tuning seeds 101..120, c = {c}: learned {m:.3} of oracle {o:.3}");
        }
        return;
    }
    report("Tuning seeds 101..120", 101..=120, DEFAULT_EXPLORE_C);
    report("Report seeds 1..20", 1..=20, DEFAULT_EXPLORE_C);
    let _ = Domain::Arithmetic;
}
