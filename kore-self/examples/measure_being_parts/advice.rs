//! Track B4 measurement: advice as an input, simulated advisors of known quality.
//! `cargo run -p kore-self --example measure_being --offline --release -- advice` prints the tables of
//! docs/measured.md. Everything here uses SIMULATED advisors: it measures how a being should use advice of
//! a given quality, NOT how good any real model is.

use super::being::advice::Advisor;
use super::being::measure::*;
use super::being::planner::AdviceMode;
use super::being::solver::{STRATEGIES, STRATEGIES_EXT};
use super::being::world::World;

pub const QS: [f64; 11] = [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0];
pub const NAMED: [f64; 4] = [0.5, 0.7, 0.9, 1.0];

#[derive(Clone, Debug)]
pub struct Met {
    pub rpe: f64,
    pub attempts: usize,
    pub tests: u64,
    pub test_wins: u64,
    pub advice_steps: u64,
    pub followed: u64,
    pub test_energy: i64,
    pub energy: i64,
    pub trust: Option<f64>,
    pub weight: Option<f64>,
    pub thirds: [f64; 3],
    pub to_good: Option<usize>,
    pub alarm_steps: Vec<u64>,
    pub after: f64,
}

fn met(r: &AdvRun, target: f64, spec: &TrustSpec, split: usize) -> Met {
    let n = r.run.recs.len();
    let tests = r.info.iter().filter(|i| i.tested).count() as u64;
    let (mut tw, mut tn, mut wsum) = (0.0, 0.0, 0.0);
    for ((_, kind), c) in r.state.trust.cells() {
        if kind != "rule" && c.tests > 0 {
            tw += c.mean() * c.tests as f64;
            wsum += c.weight(spec.beta) * c.tests as f64;
            tn += c.tests as f64;
        }
    }
    let to_good = (40..=n).find(|&i| r.run.window(i - 40, i) >= 0.9 * target);
    Met {
        rpe: r.run.per_budget(),
        attempts: r.run.strategies.iter().sum::<u64>() as usize,
        tests,
        test_wins: r.info.iter().filter(|i| i.win == Some(true)).count() as u64,
        advice_steps: r.info.iter().filter(|i| i.has_advice).count() as u64,
        followed: r.info.iter().filter(|i| i.followed).count() as u64,
        test_energy: r.info.iter().filter(|i| i.tested).map(|i| i.cost).sum(),
        energy: r.run.cost,
        trust: if tn > 0.0 { Some(tw / tn) } else { None },
        weight: if tn > 0.0 { Some(wsum / tn) } else { None },
        thirds: [r.run.window(0, n / 4), r.run.window(n / 4, n / 2), r.run.window(n / 2, n)],
        to_good,
        alarm_steps: r.alarms.iter().map(|a| a.0).collect(),
        after: r.run.window(split.min(n), n),
    }
}

pub struct SeedRes {
    pub seed: u64,
    pub none: Met,
    pub oracle: Met,
    pub blind: Vec<Met>,
    pub trust: Vec<Met>,
    pub cw_blind: Met,
    pub cw_trust: Met,
    pub deg_blind: Met,
    pub deg_trust: Met,
    pub degrade_at: usize,
}

pub fn seed_res(seed: u64, budget: i64, qs: &[f64], spec: &TrustSpec) -> SeedRes {
    let w = World::new(seed);
    let truth = truth_in(&w, &[]);
    let degrade_at = (budget / 8) as usize;
    let run = |mode: AdviceMode, adv: Option<&dyn Advisor>| run_adv(World::new(seed), budget, mode, spec, adv, 0, false);
    let oracle_run = run_menu(seed, budget, &mut OracleMenu { truth: truth.clone(), strategies: STRATEGIES.to_vec() });
    let target = oracle_run.per_budget();
    let m = |r: &AdvRun| met(r, target, spec, degrade_at);
    let none = m(&run(AdviceMode::Off, None));
    let oracle = Met { rpe: target, attempts: oracle_run.strategies.iter().sum::<u64>() as usize, tests: 0, test_wins: 0, advice_steps: 0, followed: 0, test_energy: 0, energy: oracle_run.cost, trust: None, weight: None, thirds: [oracle_run.window(0, oracle_run.recs.len() / 4), oracle_run.window(oracle_run.recs.len() / 4, oracle_run.recs.len() / 2), oracle_run.window(oracle_run.recs.len() / 2, oracle_run.recs.len())], to_good: None, alarm_steps: vec![], after: oracle_run.window(degrade_at, oracle_run.recs.len()) };
    let (mut blind, mut trust) = (vec![], vec![]);
    for &q in qs {
        let a = sim_advisor(&truth, false, seed, q);
        blind.push(m(&run(AdviceMode::Blind, Some(&a))));
        trust.push(m(&run(AdviceMode::Trust, Some(&a))));
    }
    let mut cw = sim_advisor(&truth, false, seed, 0.2);
    cw.confidently_wrong = true;
    let mut deg = sim_advisor(&truth, false, seed, 0.9);
    deg.degrade = Some((degrade_at as u64, 0.3));
    SeedRes {
        seed,
        none,
        oracle,
        blind,
        trust,
        cw_blind: m(&run(AdviceMode::Blind, Some(&cw))),
        cw_trust: m(&run(AdviceMode::Trust, Some(&cw))),
        deg_blind: m(&run(AdviceMode::Blind, Some(&deg))),
        deg_trust: m(&run(AdviceMode::Trust, Some(&deg))),
        degrade_at,
    }
}

fn ci(xs: &[f64]) -> (f64, f64, f64) {
    mean_ci(xs)
}

fn pm(xs: &[f64]) -> String {
    let (m, lo, hi) = ci(xs);
    format!("{m:.3} [{lo:.3}, {hi:.3}]")
}

fn pd(xs: &[f64]) -> String {
    let (m, lo, hi) = ci(xs);
    format!("{m:+.3} [{lo:+.3}, {hi:+.3}]")
}

fn wlt(a: &[f64], b: &[f64]) -> String {
    let w = a.iter().zip(b).filter(|(x, y)| **x > **y + 1e-9).count();
    let l = a.iter().zip(b).filter(|(x, y)| **x + 1e-9 < **y).count();
    format!("{w} / {l} / {}", a.len() - w - l)
}

fn col(rows: &[SeedRes], f: impl Fn(&SeedRes) -> f64) -> Vec<f64> {
    rows.iter().map(f).collect()
}

fn mean(xs: &[f64]) -> f64 {
    xs.iter().sum::<f64>() / xs.len().max(1) as f64
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v.get(v.len() / 2).copied()
}

/// Mean paired difference of two arms as a function of the first one's quality, crossing zero.
fn crossing(qs: &[f64], diffs: &[f64]) -> String {
    for i in 1..qs.len() {
        if diffs[i - 1] < 0.0 && diffs[i] >= 0.0 {
            let t = -diffs[i - 1] / (diffs[i] - diffs[i - 1]);
            return format!("about q = {:.2}", qs[i - 1] + t * (qs[i] - qs[i - 1]));
        }
    }
    if diffs.iter().all(|d| *d >= 0.0) { "never below zero in the grid (advice never hurt on average)".into() } else if diffs.iter().all(|d| *d < 0.0) { "never above zero in the grid".into() } else { "no single crossing".into() }
}

pub fn report(label: &str, seeds: std::ops::RangeInclusive<u64>, budget: i64, spec: &TrustSpec) {
    let rows: Vec<SeedRes> = seeds.map(|s| seed_res(s, budget, &QS, spec)).collect();
    let n = rows.len();
    let qi = |q: f64| QS.iter().position(|x| (*x - q).abs() < 1e-9).unwrap();
    println!("\n### Advice, simulated advisor, {label} (budget {budget}, beta_trust {}, n = {n} worlds)\n", spec.beta);
    println!("| arm | mean reward per energy | 95% interval of the mean |");
    println!("|---|---|---|");
    println!("| B1 learner, no advisor | {} |", pm(&col(&rows, |r| r.none.rpe)));
    for &q in &NAMED {
        println!("| follow advice blindly, q = {q} | {} |", pm(&col(&rows, |r| r.blind[qi(q)].rpe)));
    }
    for &q in &NAMED {
        println!("| trust-weighted learner, q = {q} | {} |", pm(&col(&rows, |r| r.trust[qi(q)].rpe)));
    }
    println!("| blind, confidently wrong (q = 0.2, states 0.95) | {} |", pm(&col(&rows, |r| r.cw_blind.rpe)));
    println!("| trust-weighted, confidently wrong | {} |", pm(&col(&rows, |r| r.cw_trust.rpe)));
    println!("| blind, degrades 0.9 -> 0.3 at step {} | {} |", rows[0].degrade_at, pm(&col(&rows, |r| r.deg_blind.rpe)));
    println!("| trust-weighted, degrades 0.9 -> 0.3 at step {} | {} |", rows[0].degrade_at, pm(&col(&rows, |r| r.deg_trust.rpe)));
    println!("| oracle (knows hidden success rates) | {} |", pm(&col(&rows, |r| r.oracle.rpe)));

    println!("\nKey curve: benefit of advice versus advisor quality q (paired difference in reward per energy against the B1 learner without advisor, same worlds; wins / losses / ties out of {n}):\n");
    println!("| q | blind minus none | blind w/l/t | trust minus none | trust w/l/t | trust minus blind |");
    println!("|---|---|---|---|---|---|");
    let (mut db, mut dt) = (vec![], vec![]);
    for (i, &q) in QS.iter().enumerate() {
        let b = col(&rows, |r| r.blind[i].rpe - r.none.rpe);
        let t = col(&rows, |r| r.trust[i].rpe - r.none.rpe);
        db.push(mean(&b));
        dt.push(mean(&t));
        println!(
            "| {q} | {} | {} | {} | {} | {} |",
            pd(&b),
            wlt(&col(&rows, |r| r.blind[i].rpe), &col(&rows, |r| r.none.rpe)),
            pd(&t),
            wlt(&col(&rows, |r| r.trust[i].rpe), &col(&rows, |r| r.none.rpe)),
            pd(&col(&rows, |r| r.trust[i].rpe - r.blind[i].rpe))
        );
    }
    println!("\nBreak-even (mean paired difference against no advisor crosses zero): blind following {}; trust-weighted {}.", crossing(&QS, &db), crossing(&QS, &dt));

    println!("\nCalibration: learned trust against the true quality (trust arms; mean over worlds; trust = posterior mean of the win rate of followed-against-model advice, weight = optimistic mixing weight at the end of life):\n");
    println!("| true q | learned trust | mixing weight at end | tests per world | tests as share of steps | test win rate | energy spent on tests (share) |");
    println!("|---|---|---|---|---|---|---|");
    for (i, &q) in QS.iter().enumerate() {
        let ts: Vec<f64> = rows.iter().filter_map(|r| r.trust[i].trust).collect();
        let ws: Vec<f64> = rows.iter().filter_map(|r| r.trust[i].weight).collect();
        let tests = mean(&col(&rows, |r| r.trust[i].tests as f64));
        let share = mean(&col(&rows, |r| r.trust[i].tests as f64 / r.trust[i].advice_steps.max(1) as f64));
        let (w, t): (u64, u64) = rows.iter().fold((0, 0), |a, r| (a.0 + r.trust[i].test_wins, a.1 + r.trust[i].tests));
        let es = mean(&col(&rows, |r| r.trust[i].test_energy as f64 / r.trust[i].energy.max(1) as f64));
        println!("| {q} | {} | {} | {tests:.1} | {share:.3} | {:.3} | {es:.3} |", if ts.is_empty() { "-".into() } else { format!("{:.3}", mean(&ts)) }, if ws.is_empty() { "-".into() } else { format!("{:.3}", mean(&ws)) }, if t > 0 { w as f64 / t as f64 } else { f64::NAN });
    }
    let q_trust: Vec<(f64, f64)> = QS.iter().enumerate().filter_map(|(i, q)| mean_opt(&col_opt(&rows, |r| r.trust[i].trust)).map(|t| (*q, t))).collect();
    println!("\nRank correlation between true q and learned trust over the grid: {:.3}", spearman(&q_trust));

    println!("\nCost of testing advice that is useless or bad (trust-weighted learner against the B1 learner, and against blind following):\n");
    println!("| q | trust minus none | tests per world | steps lost to tests (share of steps) | blind minus none |");
    println!("|---|---|---|---|---|");
    for &q in &[0.0, 0.2, 0.4, 0.5] {
        let i = qi(q);
        println!("| {q} | {} | {:.1} | {:.3} | {} |", pd(&col(&rows, |r| r.trust[i].rpe - r.none.rpe)), mean(&col(&rows, |r| r.trust[i].tests as f64)), mean(&col(&rows, |r| r.trust[i].tests as f64 / r.trust[i].advice_steps.max(1) as f64)), pd(&col(&rows, |r| r.blind[i].rpe - r.none.rpe)));
    }

    println!("\nLearning curve, reward per energy by quarter of the run (mean over worlds; speed = first step where the trailing 40 steps reach 90% of the oracle's overall rate):\n");
    println!("| arm | first quarter | second quarter | second half | median step of 90% of oracle |");
    println!("|---|---|---|---|---|");
    let curve = |name: &str, f: &dyn Fn(&SeedRes) -> &Met| {
        let reached: Vec<f64> = rows.iter().filter_map(|r| f(r).to_good.map(|x| x as f64)).collect();
        println!("| {name} | {:.3} | {:.3} | {:.3} | {} |", mean(&col(&rows, |r| f(r).thirds[0])), mean(&col(&rows, |r| f(r).thirds[1])), mean(&col(&rows, |r| f(r).thirds[2])), match median(reached.clone()) { Some(m) => format!("{m:.0} ({} of {n} reached)", reached.len()), None => format!("never (0 of {n})") });
    };
    curve("no advisor", &|r| &r.none);
    curve("blind q = 0.9", &|r| &r.blind[qi(0.9)]);
    curve("trust q = 0.9", &|r| &r.trust[qi(0.9)]);
    curve("trust q = 0.7", &|r| &r.trust[qi(0.7)]);
    curve("trust q = 0.5", &|r| &r.trust[qi(0.5)]);
    println!("| oracle | {:.3} | {:.3} | {:.3} | - |", mean(&col(&rows, |r| r.oracle.thirds[0])), mean(&col(&rows, |r| r.oracle.thirds[1])), mean(&col(&rows, |r| r.oracle.thirds[2])));

    println!("\nBad advisors and a degrading advisor (paired differences against the B1 learner without advisor):\n");
    println!("| scenario | blind minus none | blind w/l/t | trust minus none | trust w/l/t |");
    println!("|---|---|---|---|---|");
    println!("| confidently wrong (q = 0.2, states 0.95) | {} | {} | {} | {} |", pd(&col(&rows, |r| r.cw_blind.rpe - r.none.rpe)), wlt(&col(&rows, |r| r.cw_blind.rpe), &col(&rows, |r| r.none.rpe)), pd(&col(&rows, |r| r.cw_trust.rpe - r.none.rpe)), wlt(&col(&rows, |r| r.cw_trust.rpe), &col(&rows, |r| r.none.rpe)));
    println!("| degrades 0.9 -> 0.3 at step {}, whole run | {} | {} | {} | {} |", rows[0].degrade_at, pd(&col(&rows, |r| r.deg_blind.rpe - r.none.rpe)), wlt(&col(&rows, |r| r.deg_blind.rpe), &col(&rows, |r| r.none.rpe)), pd(&col(&rows, |r| r.deg_trust.rpe - r.none.rpe)), wlt(&col(&rows, |r| r.deg_trust.rpe), &col(&rows, |r| r.none.rpe)));
    println!("| degrades 0.9 -> 0.3, steps after the degradation only | {} | {} | {} | {} |", pd(&col(&rows, |r| r.deg_blind.after - r.none.after)), wlt(&col(&rows, |r| r.deg_blind.after), &col(&rows, |r| r.none.after)), pd(&col(&rows, |r| r.deg_trust.after - r.none.after)), wlt(&col(&rows, |r| r.deg_trust.after), &col(&rows, |r| r.none.after)));
    let first_after: Vec<f64> = rows.iter().filter_map(|r| r.deg_trust.alarm_steps.iter().find(|s| **s as usize >= r.degrade_at).map(|s| (*s as usize - r.degrade_at) as f64)).collect();
    let false_alarms: usize = rows.iter().map(|r| r.deg_trust.alarm_steps.iter().filter(|s| (**s as usize) < r.degrade_at).count()).sum();
    println!("\nDegradation detector (trust arm, degrading advisor): flagged after the degradation in {} of {n} worlds, median delay {} steps; alarms before the degradation: {false_alarms} in total; alarms in the constant q = 0.9 trust arm: {}.", first_after.len(), median(first_after).map_or("-".into(), |m| format!("{m:.0}")), rows.iter().map(|r| r.trust[qi(0.9)].alarm_steps.len()).sum::<usize>());
    let att: Vec<f64> = col(&rows, |r| r.none.attempts as f64);
    println!("Attempts per world without advisor: {:.1}", mean(&att));
    println!("Share of steps where advice was followed (trust arm q = 0.9): {:.3}; (blind q = 0.9): {:.3}", mean(&col(&rows, |r| r.trust[qi(0.9)].followed as f64 / r.trust[qi(0.9)].advice_steps.max(1) as f64)), mean(&col(&rows, |r| r.blind[qi(0.9)].followed as f64 / r.blind[qi(0.9)].advice_steps.max(1) as f64)));
}

fn col_opt(rows: &[SeedRes], f: impl Fn(&SeedRes) -> Option<f64>) -> Vec<f64> {
    rows.iter().filter_map(f).collect()
}

fn mean_opt(v: &[f64]) -> Option<f64> {
    if v.is_empty() { None } else { Some(mean(v)) }
}

fn spearman(pts: &[(f64, f64)]) -> f64 {
    let rank = |v: Vec<f64>| -> Vec<f64> {
        let mut idx: Vec<usize> = (0..v.len()).collect();
        idx.sort_by(|a, b| v[*a].partial_cmp(&v[*b]).unwrap());
        let mut r = vec![0.0; v.len()];
        for (k, i) in idx.iter().enumerate() {
            r[*i] = k as f64;
        }
        r
    };
    let (a, b) = (rank(pts.iter().map(|p| p.0).collect()), rank(pts.iter().map(|p| p.1).collect()));
    let (ma, mb) = (mean(&a), mean(&b));
    let (mut num, mut da, mut db) = (0.0, 0.0, 0.0);
    for i in 0..a.len() {
        num += (a[i] - ma) * (b[i] - mb);
        da += (a[i] - ma).powi(2);
        db += (b[i] - mb).powi(2);
    }
    if da * db > 0.0 { num / (da * db).sqrt() } else { f64::NAN }
}

/// Tuning on seeds 101..120 only: trust optimism and detector settings.
pub fn tune() {
    for budget in [600i64, 3000] {
        println!("budget {budget} (tuning seeds 101..120): paired mean difference of the trust arm against no advisor, averaged over q in {{0.0, 0.3, 0.5, 0.7, 0.9, 1.0}}, and the degrading scenario");
        for beta in [0.0, 0.5, 1.0, 1.5, 2.0] {
            let spec = TrustSpec { beta, ..TrustSpec::default() };
            let qs = [0.0, 0.3, 0.5, 0.7, 0.9, 1.0];
            let rows: Vec<SeedRes> = (101..=120).map(|s| seed_res(s, budget, &qs, &spec)).collect();
            let per_q: Vec<String> = (0..qs.len()).map(|i| format!("q{}:{:+.3}", qs[i], mean(&col(&rows, |r| r.trust[i].rpe - r.none.rpe)))).collect();
            let avg = mean(&(0..qs.len()).map(|i| mean(&col(&rows, |r| r.trust[i].rpe - r.none.rpe))).collect::<Vec<_>>());
            println!("  beta {beta}: mean over q {avg:+.4} | {} | degrade {:+.3} cw {:+.3}", per_q.join(" "), mean(&col(&rows, |r| r.deg_trust.rpe - r.none.rpe)), mean(&col(&rows, |r| r.cw_trust.rpe - r.none.rpe)));
        }
    }
}

pub fn tune_detector() {
    let budget = 3000i64;
    println!("detector settings on the degrading scenario (tuning seeds 101..120, budget {budget}); beta 1.0");
    for (k, h, keep) in [(0.5, 4.0, 0.2), (0.5, 3.0, 0.2), (0.5, 6.0, 0.2), (0.25, 4.0, 0.2), (1.0, 4.0, 0.2), (0.5, 4.0, 0.1), (0.5, 4.0, 0.4), (0.5, 100.0, 0.2)] {
        let spec = TrustSpec { k, h, keep, ..TrustSpec::default() };
        let rows: Vec<SeedRes> = (101..=120).map(|s| seed_res(s, budget, &[0.9], &spec)).collect();
        let det = rows.iter().filter(|r| r.deg_trust.alarm_steps.iter().any(|s| *s as usize >= r.degrade_at)).count();
        let fa: usize = rows.iter().map(|r| r.deg_trust.alarm_steps.iter().filter(|s| (**s as usize) < r.degrade_at).count() + r.trust[0].alarm_steps.len()).sum();
        println!("  k {k} h {h} keep {keep}: after-degradation rate trust minus none {:+.3}, whole run {:+.3}, detected {det}/20, false alarms {fa}, constant-q rate minus none {:+.3}", mean(&col(&rows, |r| r.deg_trust.after - r.none.after)), mean(&col(&rows, |r| r.deg_trust.rpe - r.none.rpe)), mean(&col(&rows, |r| r.trust[0].rpe - r.none.rpe)));
    }
}

// ---- extended pattern world ----

struct ExtMet {
    rpe: f64,
    rules: usize,
    first_rule: Option<u64>,
    covered: usize,
    wrong_rules: usize,
    prop_acc: u64,
    prop_rej: u64,
    thirds: [f64; 3],
}

fn ext_met(r: &AdvRun, w: &World) -> ExtMet {
    use super::being::discover;
    let book = &r.state.rules;
    let covered = EXPRESSIBLE.iter().filter(|k| coverage(w, book, **k, COVER_N) >= COVER_MIN).count();
    let mut wrong = 0;
    for ru in book {
        let mut bad = 0;
        for j in 0..200u64 {
            let t = w.task_forced(10_000_000 + j, super::being::world::Domain::Pattern, ru.sig);
            if let Some(v) = discover::predict(&ru.form, &t.view.params) {
                bad += (!t.check(v)) as u32;
            }
        }
        wrong += (bad > 0) as usize;
    }
    let n = r.run.recs.len();
    let c = r.state.trust.cell(&r.state.trust.cells().find(|((_, k), _)| k == "rule").map(|((a, _), _)| a.clone()).unwrap_or_default(), "rule");
    ExtMet { rpe: r.run.per_budget(), rules: book.len(), first_rule: r.accepted.first().map(|a| a.0), covered, wrong_rules: wrong, prop_acc: c.test_wins, prop_rej: c.tests - c.test_wins, thirds: [r.run.window(0, n / 4), r.run.window(n / 4, n / 2), r.run.window(n / 2, n)] }
}

pub fn ext_report(label: &str, seeds: std::ops::RangeInclusive<u64>, budget: i64, spec: &TrustSpec) {
    let seeds: Vec<u64> = seeds.collect();
    let n = seeds.len();
    let names = ["B2 learner: discovery pass every 10 steps, no advisor", "no discovery, no advisor", "trust, q = 0.5 (rule proposals + menu advice, no discovery pass)", "trust, q = 0.9 (no discovery pass)", "trust, q = 1.0 (no discovery pass)", "blind, q = 0.9 (no discovery pass)", "trust, q = 0.9 + discovery pass every 10 steps", "oracle with perfect rules (knows rules and success rates)"];
    let mut res: Vec<Vec<ExtMet>> = names.iter().map(|_| vec![]).collect();
    for &seed in &seeds {
        let w = World::with_extended(seed, true);
        let truth = truth_in(&w, &[]);
        let perfect = perfect_rules();
        let run = |mode: AdviceMode, adv: Option<&dyn Advisor>, every: u64, nodisc: bool| run_adv(World::with_extended(seed, true), budget, mode, spec, adv, every, nodisc);
        let a5 = sim_advisor(&truth, true, seed, 0.5);
        let a9 = sim_advisor(&truth, true, seed, 0.9);
        let a10 = sim_advisor(&truth, true, seed, 1.0);
        res[0].push(ext_met(&run(AdviceMode::Off, None, 10, false), &w));
        res[1].push(ext_met(&run(AdviceMode::Off, None, 0, true), &w));
        res[2].push(ext_met(&run(AdviceMode::Trust, Some(&a5), 0, false), &w));
        res[3].push(ext_met(&run(AdviceMode::Trust, Some(&a9), 0, false), &w));
        res[4].push(ext_met(&run(AdviceMode::Trust, Some(&a10), 0, false), &w));
        res[5].push(ext_met(&run(AdviceMode::Blind, Some(&a9), 0, false), &w));
        res[6].push(ext_met(&run(AdviceMode::Trust, Some(&a9), 10, false), &w));
        let o = run_menu_ext(World::with_extended(seed, true), budget, &mut OracleMenu { truth: truth_in(&w, &perfect), strategies: STRATEGIES_EXT.to_vec() }, 0, &perfect);
        let m = o.run.recs.len();
        res[7].push(ExtMet { rpe: o.run.per_budget(), rules: perfect.len(), first_rule: None, covered: 6, wrong_rules: 0, prop_acc: 0, prop_rej: 0, thirds: [o.run.window(0, m / 4), o.run.window(m / 4, m / 2), o.run.window(m / 2, m)] });
    }
    let c = |i: usize, f: &dyn Fn(&ExtMet) -> f64| res[i].iter().map(f).collect::<Vec<f64>>();
    println!("\n### Advice in the extended pattern world, simulated advisor, {label} (budget {budget}, n = {n} worlds)\n");
    println!("| arm | mean reward per energy [95% interval of the mean] | rules held at end | expressible families covered (of 6) | wrong rules | proposals accepted / rejected per world | median first accepted rule (step) |");
    println!("|---|---|---|---|---|---|---|");
    for (i, nm) in names.iter().enumerate() {
        let firsts: Vec<f64> = res[i].iter().filter_map(|m| m.first_rule.map(|x| x as f64)).collect();
        println!(
            "| {nm} | {} | {:.2} | {:.2} | {} | {:.1} / {:.1} | {} |",
            pm(&c(i, &|m| m.rpe)),
            mean(&c(i, &|m| m.rules as f64)),
            mean(&c(i, &|m| m.covered as f64)),
            res[i].iter().map(|m| m.wrong_rules).sum::<usize>(),
            mean(&c(i, &|m| m.prop_acc as f64)),
            mean(&c(i, &|m| m.prop_rej as f64)),
            median(firsts.clone()).map_or("-".into(), |m| format!("{m:.0} ({} of {n})", firsts.len()))
        );
    }
    println!("\nPaired differences in reward per energy (wins / losses / ties out of {n}):\n");
    println!("| comparison | mean difference | 95% interval | wins / losses / ties |");
    println!("|---|---|---|---|");
    for (a, b, nm) in [(3, 0, "trust q = 0.9 minus B2 discovery"), (2, 1, "trust q = 0.5 minus no discovery"), (3, 1, "trust q = 0.9 minus no discovery"), (4, 1, "trust q = 1.0 minus no discovery"), (5, 1, "blind q = 0.9 minus no discovery"), (6, 0, "trust q = 0.9 + discovery minus B2 discovery"), (7, 3, "oracle minus trust q = 0.9")] {
        println!("| {nm} | {} | {} |", pd(&(0..n).map(|i| res[a][i].rpe - res[b][i].rpe).collect::<Vec<_>>()), wlt(&c(a, &|m| m.rpe), &c(b, &|m| m.rpe)));
    }
    println!("\nLearning curve by quarter (reward per energy):\n");
    println!("| arm | first quarter | second quarter | second half |");
    println!("|---|---|---|---|");
    for (i, nm) in names.iter().enumerate() {
        println!("| {nm} | {:.3} | {:.3} | {:.3} |", mean(&c(i, &|m| m.thirds[0])), mean(&c(i, &|m| m.thirds[1])), mean(&c(i, &|m| m.thirds[2])));
    }
}

pub fn main_advice(args: &[String]) {
    let spec = TrustSpec::default();
    match args.get(2).map(|s| s.as_str()) {
        Some("tune") => tune(),
        Some("tune-detector") => tune_detector(),
        Some("quick") => {
            report("quick check, seeds 1..4", 1..=4, 600, &spec);
        }
        Some("ext-quick") => ext_report("quick check, seeds 1..4", 1..=4, 600, &spec),
        Some("ext") => {
            for budget in [600i64, 3000] {
                ext_report("tuning seeds 101..120", 101..=120, budget, &spec);
                ext_report("report seeds 1..20", 1..=20, budget, &spec);
            }
        }
        _ => {
            for budget in [600i64, 3000] {
                report("tuning seeds 101..120", 101..=120, budget, &spec);
                report("report seeds 1..20", 1..=20, budget, &spec);
            }
        }
    }
    let _ = STRATEGIES;
}
