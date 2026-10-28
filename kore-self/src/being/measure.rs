//! Measurement helpers (shared by the unit test floors and examples/measure_being.rs).
//! In-memory runs of the same engine the logged loop uses; no log is written.

use super::chooser::{DomainPicker, FixedPicker, RandomPicker, Ucb1};
use super::run_loop::Engine;
use super::solver::{Context, ScriptedThinker, Thinker};
use super::state::{BeingState, Genesis};
use super::world::{Domain, World, DOMAINS};

pub const BUDGET: i64 = 600;
/// Steps offset used to estimate the hidden truth, far from any run.
const ORACLE_OFFSET: u64 = 10_000_000;
const ORACLE_SAMPLES: u64 = 3000;

/// Hidden truth: expected reward per energy of each domain under the scripted thinker.
pub fn true_efficiency(world_seed: u64) -> [f64; 4] {
    let w = World::new(world_seed);
    let th = ScriptedThinker;
    let st = [Default::default(); 5];
    let mut out = [0.0; 4];
    for d in DOMAINS {
        let (mut r, mut c) = (0i64, 0i64);
        for s in 0..ORACLE_SAMPLES {
            let step = ORACLE_OFFSET + s;
            let t = w.task(step, d);
            let ctx = Context { step, task: &t.view, noise: w.noise(step, d), stats: &st, rules: &[], degraded: false };
            if t.check(th.propose(&ctx).answer) {
                r += t.view.reward;
            }
            c += t.view.cost;
        }
        out[d.idx()] = r as f64 / c as f64;
    }
    out
}

pub fn oracle_domain(world_seed: u64) -> Domain {
    let e = true_efficiency(world_seed);
    let mut best = Domain::Arithmetic;
    for d in DOMAINS {
        if e[d.idx()] > e[best.idx()] {
            best = d;
        }
    }
    best
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub reward: i64,
    pub cost: i64,
    pub steps: u64,
    pub picks: Vec<Domain>,
}

impl RunResult {
    pub fn per_budget(&self) -> f64 {
        if self.cost > 0 { self.reward as f64 / self.cost as f64 } else { 0.0 }
    }
}

pub fn run_policy(world_seed: u64, budget: i64, picker: &mut dyn DomainPicker) -> RunResult {
    let th = ScriptedThinker;
    let mut st = BeingState::default();
    st.start(Genesis { world_seed, loop_seed: 0, budget, explore_c: 0.0, chooser: super::state::Chooser::Ucb, extended: false, drift: false, adapt: Default::default(), advice: Default::default() }, String::new());
    let mut e = Engine::new(World::new(world_seed), st, &th);
    let mut picks = vec![];
    while let Ok(o) = e.step(picker) {
        picks.push(o.outcome.domain);
    }
    RunResult { reward: e.state.total_reward, cost: e.state.total_cost, steps: e.state.step, picks }
}

pub fn learned(world_seed: u64, run_seed: u64, c: f64, budget: i64) -> RunResult {
    run_policy(world_seed, budget, &mut Ucb1::new(run_seed, c))
}

/// All six policies for one seed: random, always x4, oracle, learned.
pub struct SeedRow {
    pub seed: u64,
    pub best: Domain,
    pub random: RunResult,
    pub always: [RunResult; 4],
    pub oracle: RunResult,
    pub learned: RunResult,
}

pub fn seed_row(seed: u64, c: f64, budget: i64) -> SeedRow {
    let best = oracle_domain(seed);
    SeedRow {
        seed,
        best,
        random: run_policy(seed, budget, &mut RandomPicker::new(seed)),
        always: [
            run_policy(seed, budget, &mut FixedPicker::always(Domain::Arithmetic)),
            run_policy(seed, budget, &mut FixedPicker::always(Domain::Algebra)),
            run_policy(seed, budget, &mut FixedPicker::always(Domain::Physics)),
            run_policy(seed, budget, &mut FixedPicker::always(Domain::Pattern)),
        ],
        oracle: run_policy(seed, budget, &mut FixedPicker::oracle(best)),
        learned: learned(seed, seed, c, budget),
    }
}

/// Share of the picks in `range` that went to `d`.
pub fn share(picks: &[Domain], d: Domain, from: usize, to: usize) -> f64 {
    let s = &picks[from.min(picks.len())..to.min(picks.len())];
    if s.is_empty() { 0.0 } else { s.iter().filter(|x| **x == d).count() as f64 / s.len() as f64 }
}

/// First step at which the trailing 20-step window has >= 60% picks of `d`.
pub fn concentration_step(picks: &[Domain], d: Domain) -> Option<usize> {
    (20..=picks.len()).find(|&e| share(picks, d, e - 20, e) >= 0.6)
}

pub fn mean_ci(xs: &[f64]) -> (f64, f64, f64) {
    let n = xs.len() as f64;
    let m = xs.iter().sum::<f64>() / n;
    let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
    let h = 2.093 * (var / n).sqrt(); // t(0.975, 19)
    (m, m - h, m + h)
}

// ---- Track B1: menu world ----

use super::chooser::{GreedyMenu, RandomMenu, UcbMenu};
use super::planner::{Action, Choice, MenuChooser, ModelChooser, Offer, DEFAULT_SKIP_FRAC};
use super::discover::Rule;
use super::solver::{Strategy, STRATEGIES, STRATEGIES_EXT};
use super::state::Chooser;

/// Budget of the longer menu runs.
pub const LONG_BUDGET: i64 = 3000;
const TRUTH_SAMPLES: u64 = 400;

/// Hidden truth: probability that each strategy answers right, per (domain, difficulty).
/// Uses the generator and the noise source; only the oracle and the tests touch it.
#[derive(Debug, Clone)]
pub struct Truth {
    pub p: [[[f64; 4]; 5]; 5],
}

pub fn truth(world_seed: u64) -> Truth {
    truth_in(&World::new(world_seed), &[])
}

/// As `truth`, in any world, for a being whose accepted rules are `rules` (fourth strategy included).
pub fn truth_in(w: &World, rules: &[Rule]) -> Truth {
    truth_flag(w, rules, false)
}

/// As `truth_in`, with the `fast` strategy degraded or not (drifting worlds).
pub fn truth_flag(w: &World, rules: &[Rule], degraded: bool) -> Truth {
    let th = ScriptedThinker;
    let st = [Default::default(); 5];
    let mut t = Truth { p: [[[0.0; 4]; 5]; 5] };
    let doms: Vec<Domain> = if w.drift.is_some() { super::world::ALL_DOMAINS.to_vec() } else { DOMAINS.to_vec() };
    for d in doms {
        for k in 1..=5u8 {
            for s in if w.extended { STRATEGIES_EXT.to_vec() } else { STRATEGIES.to_vec() } {
                let mut ok = 0u64;
                for i in 0..TRUTH_SAMPLES {
                    let step = ORACLE_OFFSET + i;
                    let task = w.task_forced(step, d, k);
                    let ctx = Context { step, task: &task.view, noise: w.menu_noise(step, (i % 4) as usize), stats: &st, rules, degraded };
                    ok += task.check(th.propose_with(&ctx, s).answer) as u64;
                }
                t.p[d.idx()][k as usize - 1][s.idx()] = ok as f64 / TRUTH_SAMPLES as f64;
            }
        }
    }
    t
}

/// Knows the hidden success probabilities and the real energy of each strategy.
pub struct OracleMenu {
    pub truth: Truth,
    /// Strategies the oracle may use (three, or four in the extended world).
    pub strategies: Vec<Strategy>,
}

impl MenuChooser for OracleMenu {
    fn name(&self) -> String {
        "oracle".into()
    }
    fn choose(&mut self, st: &BeingState, offers: &[Offer]) -> Option<Choice> {
        oracle_choice(&self.truth, &self.strategies, st, offers, "oracle")
    }
}

fn oracle_choice(truth: &Truth, strategies: &[Strategy], st: &BeingState, offers: &[Offer], name: &str) -> Option<Choice> {
    let mut best: Option<(f64, usize, Strategy)> = None;
    for o in offers {
        for &s in strategies {
            let e = s.energy(o.listed_cost);
            if e > st.budget_left {
                continue;
            }
            let v = o.listed_reward as f64 * truth.p[o.domain.idx()][o.difficulty as usize - 1][s.idx()] / e as f64;
            if best.map_or(true, |b| v > b.0) {
                best = Some((v, o.slot, s));
            }
        }
    }
    let (v, slot, strategy) = best?;
    Some(Choice { action: Action::Attempt { slot, strategy }, policy: name.into(), why: "knows hidden truth".into(), p: 0.0, sd: 0.0, cost_est: 0.0, score: v, rationale: serde_json::json!({}), advice: None })
}

/// Oracle for a drifting world: knows the hidden success rates before and after the degradation of
/// `fast` and the time of that change, and so re-optimises at the change. (The offers it chooses
/// from already show the new rewards and costs after the other scheduled changes.)
pub struct ScheduleOracle {
    pub before: Truth,
    pub after: Truth,
    pub degrade_at: i64,
}

impl MenuChooser for ScheduleOracle {
    fn name(&self) -> String {
        "oracle-schedule".into()
    }
    fn choose(&mut self, st: &BeingState, offers: &[Offer]) -> Option<Choice> {
        let t = if st.clock >= self.degrade_at { &self.after } else { &self.before };
        oracle_choice(t, &STRATEGIES, st, offers, "oracle-schedule")
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Rec {
    pub reward: i64,
    pub cost: i64,
    pub attempted: bool,
    pub correct: bool,
    /// Predicted success probability made before the attempt (None: no prediction).
    pub pred: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct MenuRun {
    pub reward: i64,
    pub cost: i64,
    pub recs: Vec<Rec>,
    pub skips: u64,
    /// Strategy counts [fast, careful, deep].
    pub strategies: [u64; 4],
}

impl MenuRun {
    pub fn per_budget(&self) -> f64 {
        if self.cost > 0 { self.reward as f64 / self.cost as f64 } else { 0.0 }
    }
    /// Reward per energy over steps `from..to` (0-based, by step index).
    pub fn window(&self, from: usize, to: usize) -> f64 {
        let s = &self.recs[from.min(self.recs.len())..to.min(self.recs.len())];
        let (r, c) = s.iter().fold((0i64, 0i64), |a, x| (a.0 + x.reward, a.1 + x.cost));
        if c > 0 { r as f64 / c as f64 } else { 0.0 }
    }
}

pub fn run_menu(world_seed: u64, budget: i64, chooser: &mut dyn MenuChooser) -> MenuRun {
    let th = ScriptedThinker;
    let mut st = BeingState::default();
    st.start(Genesis { world_seed, loop_seed: 0, budget, explore_c: 0.0, chooser: Chooser::Model, extended: false, drift: false, adapt: Default::default(), advice: Default::default() }, String::new());
    let mut e = Engine::new(World::new(world_seed), st, &th);
    let (mut recs, mut skips, mut strategies) = (vec![], 0, [0u64; 4]);
    while let Ok(o) = e.step_menu(chooser) {
        let a = o.outcome;
        if !(o.predicted_p.is_some() || a.skipped) {
            continue;
        }
        if a.skipped {
            skips += 1;
        } else if let Some(s) = a.strategy {
            strategies[s.idx()] += 1;
        }
        recs.push(Rec { reward: a.reward, cost: a.cost, attempted: !a.skipped, correct: a.correct, pred: o.predicted_p });
    }
    MenuRun { reward: e.state.total_reward, cost: e.state.total_cost, recs, skips, strategies }
}

pub struct MenuRow {
    pub seed: u64,
    pub random: MenuRun,
    pub greedy: MenuRun,
    pub greedy_fast: MenuRun,
    pub greedy_deep: MenuRun,
    pub ucb: MenuRun,
    pub model: MenuRun,
    pub model_noskip: MenuRun,
    pub oracle: MenuRun,
}

pub fn menu_row(seed: u64, budget: i64, beta: f64, skip_frac: f64) -> MenuRow {
    let mut m = ModelChooser::new(beta);
    m.skip_frac = skip_frac;
    MenuRow {
        seed,
        random: run_menu(seed, budget, &mut RandomMenu::new(seed)),
        greedy: run_menu(seed, budget, &mut GreedyMenu { strategy: Strategy::Careful }),
        greedy_fast: run_menu(seed, budget, &mut GreedyMenu { strategy: Strategy::Fast }),
        greedy_deep: run_menu(seed, budget, &mut GreedyMenu { strategy: Strategy::Deep }),
        ucb: run_menu(seed, budget, &mut UcbMenu::new(seed, super::chooser::DEFAULT_EXPLORE_C)),
        model: run_menu(seed, budget, &mut m),
        model_noskip: run_menu(seed, budget, &mut ModelChooser { beta, skip_frac: 0.0, no_discovered: false, curiosity: 0.0, novelty: super::planner::DEFAULT_NOVELTY, advice: Default::default() }),
        oracle: run_menu(seed, budget, &mut OracleMenu { truth: truth(seed), strategies: STRATEGIES.to_vec() }),
    }
}

/// Reliability bins (10 equal-width bins of predicted success) and Brier scores.
/// `skip_first` attempts of each run are left out (the model starts from its prior).
pub struct Calibration {
    /// (bin low, count, mean predicted, observed rate)
    pub bins: Vec<(f64, u64, f64, f64)>,
    pub brier: f64,
    /// Brier of "predict the running success rate so far" on the same attempts.
    pub brier_base_rate: f64,
    pub n: u64,
}

pub fn calibration(runs: &[&MenuRun], skip_first: usize) -> Calibration {
    let mut acc = [(0u64, 0.0f64, 0u64); 10];
    let (mut b, mut bb, mut n) = (0.0, 0.0, 0u64);
    for r in runs {
        let (mut seen, mut hits) = (0u64, 0u64);
        let mut k = 0usize;
        for x in r.recs.iter().filter(|x| x.attempted) {
            let base = (hits as f64 + 1.0) / (seen as f64 + 2.0);
            if let (true, Some(p)) = (k >= skip_first, x.pred) {
                let y = x.correct as u8 as f64;
                b += (p - y).powi(2);
                bb += (base - y).powi(2);
                n += 1;
                let i = ((p * 10.0) as usize).min(9);
                acc[i].0 += 1;
                acc[i].1 += p;
                acc[i].2 += x.correct as u64;
            }
            seen += 1;
            hits += x.correct as u64;
            k += 1;
        }
    }
    let bins = acc.iter().enumerate().map(|(i, a)| (i as f64 / 10.0, a.0, if a.0 > 0 { a.1 / a.0 as f64 } else { 0.0 }, if a.0 > 0 { a.2 as f64 / a.0 as f64 } else { 0.0 })).collect();
    Calibration { bins, brier: b / n.max(1) as f64, brier_base_rate: bb / n.max(1) as f64, n }
}

// ---- Track B2: extended pattern world, rule discovery ----

use super::discover::{self, Example, Form};
use super::world::Kind;
use std::collections::HashMap;

/// The six extended families the grammar of `discover.rs` can express and the fixed solvers cannot solve.
pub const EXPRESSIBLE: [Kind; 6] = [Kind::ExtAffGeom, Kind::ExtPoly3, Kind::ExtRec2, Kind::ExtTribo, Kind::ExtAlt, Kind::ExtPeriodic];
/// Negative controls: outside the grammar.
pub const UNEXPRESSIBLE: [Kind; 2] = [Kind::NegProd, Kind::NegPrimes];

/// Rules a perfect discoverer would hold (the author knows the families): used by the oracle only.
pub fn perfect_rules() -> Vec<Rule> {
    let lin = |mask: u8, constant: bool| Form::Lin { mask, constant };
    [(1, Form::Poly(1)), (2, lin(1, false)), (2, lin(1, true)), (3, Form::Poly(3)), (4, lin(1, false)), (4, lin(3, false)), (4, lin(7, false)), (5, lin(2, true)), (5, lin(4, false))]
        .into_iter()
        .map(|(sig, f)| Rule::new(sig, f, 0, 0, 0))
        .collect()
}

pub struct ExtRun {
    pub run: MenuRun,
    /// (step count when accepted, rule)
    pub accepted: Vec<(u64, Rule)>,
    /// Outcome event id -> hidden family of the attempted task (measurement only).
    pub kinds: HashMap<String, Kind>,
    pub state: BeingState,
}

/// Menu run in the extended world. `discover_every` 0 = no discovery pass; `preload` rules are
/// in the state from the start (oracle with perfect rules).
pub fn run_menu_ext(world: World, budget: i64, chooser: &mut dyn MenuChooser, discover_every: u64, preload: &[Rule]) -> ExtRun {
    let world_seed = world.seed;
    let th = ScriptedThinker;
    let mut st = BeingState::default();
    st.start(Genesis { world_seed, loop_seed: 0, budget, explore_c: 0.0, chooser: Chooser::Model, extended: true, drift: false, adapt: Default::default(), advice: Default::default() }, String::new());
    for r in preload {
        st.apply_rule(r.clone());
    }
    let mut e = Engine::new(world, st, &th);
    let (mut recs, mut skips, mut strategies) = (vec![], 0, [0u64; 4]);
    let (mut accepted, mut kinds) = (vec![], HashMap::new());
    loop {
        let tasks = e.world.offers(e.state.step);
        let Ok(o) = e.step_menu(chooser) else { break };
        let a = o.outcome;
        if let Some(slot) = o.action.as_deref().and_then(|l| l.split(':').next()).and_then(|x| x.parse::<usize>().ok()) {
            kinds.insert(a.event_id.clone(), tasks[slot].view.kind);
        }
        if o.predicted_p.is_some() || a.skipped {
            if a.skipped {
                skips += 1;
            } else if let Some(s) = a.strategy {
                strategies[s.idx()] += 1;
            }
            recs.push(Rec { reward: a.reward, cost: a.cost, attempted: !a.skipped, correct: a.correct, pred: o.predicted_p });
        }
        if discover_every > 0 && e.state.step % discover_every == 0 {
            for nr in e.discover_pass() {
                accepted.push((e.state.step, nr.rule));
            }
        }
    }
    ExtRun { run: MenuRun { reward: e.state.total_reward, cost: e.state.total_cost, recs, skips, strategies }, accepted, kinds, state: e.state }
}

/// Fraction of fresh tasks of one hidden family on which the rule book applies a rule and gets the exact answer.
pub fn coverage(world: &World, rules: &[Rule], kind: Kind, n: u64) -> f64 {
    let ok = (0..n)
        .filter(|i| {
            let t = world.task_of_kind(ORACLE_OFFSET + i, kind);
            discover::apply(rules, t.view.difficulty, &t.view.params).map_or(false, |(v, _)| t.check(v))
        })
        .count();
    ok as f64 / n as f64
}

/// Share of the family's tasks where the book applies SOME rule, and share of those answers that are wrong.
pub fn applied_and_wrong(world: &World, rules: &[Rule], kind: Kind, n: u64) -> (f64, f64) {
    let (mut applied, mut wrong) = (0u64, 0u64);
    for i in 0..n {
        let t = world.task_of_kind(ORACLE_OFFSET + i, kind);
        if let Some((v, _)) = discover::apply(rules, t.view.difficulty, &t.view.params) {
            applied += 1;
            wrong += !t.check(v) as u64;
        }
    }
    (applied as f64 / n as f64, if applied > 0 { wrong as f64 / applied as f64 } else { 0.0 })
}

pub const COVER_N: u64 = 100;
pub const COVER_MIN: f64 = 0.9;

#[derive(Debug, Clone)]
pub struct Analysis {
    /// Step count at which each expressible family was first covered (>= 90% exact on fresh tasks), per `EXPRESSIBLE`.
    pub first: [Option<u64>; 6],
    pub rules: usize,
    /// Accepted rules that are never applicable-and-wrong on 300 fresh tasks of their signature (and right at least once).
    pub exact: usize,
    pub wrong: usize,
    /// The wrong ones: (signature, form code, training support, held-out support, fresh applicable-and-wrong count of 300).
    pub wrong_rules: Vec<(u8, String, u32, u32, u32)>,
    /// Over the unexpressible families: (share of tasks where a rule was applied, share of those applications that are wrong), per `UNEXPRESSIBLE`.
    pub neg: [(f64, f64); 2],
}

pub fn analyze(world_seed: u64, run: &ExtRun) -> Analysis {
    let w = World::with_extended(world_seed, true);
    let mut first = [None; 6];
    let mut book: Vec<Rule> = vec![];
    let mut i = 0;
    while i < run.accepted.len() {
        let at = run.accepted[i].0;
        while i < run.accepted.len() && run.accepted[i].0 == at {
            book.push(run.accepted[i].1.clone());
            i += 1;
        }
        for (k, f) in EXPRESSIBLE.iter().enumerate() {
            if first[k].is_none() && coverage(&w, &book, *f, COVER_N) >= COVER_MIN {
                first[k] = Some(at);
            }
        }
    }
    let (mut exact, mut wrong) = (0, 0);
    let mut wrong_rules = vec![];
    for (_, r) in &run.accepted {
        let (mut ok, mut bad) = (0, 0);
        for j in 0..300u64 {
            let t = w.task_forced(ORACLE_OFFSET + j, Domain::Pattern, r.sig);
            match discover::predict(&r.form, &t.view.params) {
                Some(v) if t.check(v) => ok += 1,
                Some(_) => bad += 1,
                None => {}
            }
        }
        if bad == 0 && ok > 0 {
            exact += 1;
        } else {
            wrong += 1;
            wrong_rules.push((r.sig, r.form.code(), r.train_ok, r.val_ok, bad));
        }
    }
    let neg = [applied_and_wrong(&w, &book, UNEXPRESSIBLE[0], 300), applied_and_wrong(&w, &book, UNEXPRESSIBLE[1], 300)];
    Analysis { first, rules: run.accepted.len(), exact, wrong, wrong_rules, neg }
}

/// Revealed examples of the given hidden families only, interleaved, `n` per family (measurement and tests).
pub fn control_examples(world_seed: u64, kinds: &[Kind], n: u64, offset: u64) -> Vec<Example> {
    let w = World::with_extended(world_seed, true);
    let mut v = vec![];
    for i in 0..n {
        for k in kinds {
            let t = w.task_of_kind(offset + i, *k);
            v.push(Example { event_id: format!("ctl-{}-{i}", *k as u64), sig: t.view.difficulty, terms: t.view.params.clone(), next: t.expected() });
        }
    }
    v
}

pub struct DiscRow {
    pub seed: u64,
    pub random: ExtRun,
    pub nodisc: ExtRun,
    pub disc: ExtRun,
    pub oracle_rules: ExtRun,
    pub oracle_fixed: ExtRun,
    pub analysis: Analysis,
}

pub const DISCOVER_EVERY: u64 = 10;

/// `bonus`: reward multiplier of pattern tasks (EXT_PATTERN_BONUS in the logged extended world; 1 = no bonus control).
pub fn disc_row(seed: u64, budget: i64, beta: f64, every: u64, bonus: i64, curiosity: f64) -> DiscRow {
    let mk = || {
        let mut w = World::with_extended(seed, true);
        w.pattern_bonus = bonus;
        w
    };
    let w = mk();
    let perfect = perfect_rules();
    let disc = run_menu_ext(mk(), budget, &mut ModelChooser { curiosity, ..ModelChooser::new(beta) }, every, &[]);
    let analysis = analyze(seed, &disc);
    DiscRow {
        seed,
        random: run_menu_ext(mk(), budget, &mut RandomMenu::new(seed), 0, &[]),
        nodisc: run_menu_ext(mk(), budget, &mut ModelChooser { beta, skip_frac: DEFAULT_SKIP_FRAC, no_discovered: true, curiosity: 0.0, novelty: super::planner::DEFAULT_NOVELTY, advice: Default::default() }, 0, &[]),
        oracle_rules: run_menu_ext(mk(), budget, &mut OracleMenu { truth: truth_in(&w, &perfect), strategies: STRATEGIES_EXT.to_vec() }, 0, &perfect),
        oracle_fixed: run_menu_ext(mk(), budget, &mut OracleMenu { truth: truth_in(&w, &[]), strategies: STRATEGIES.to_vec() }, 0, &[]),
        disc,
        analysis,
    }
}

// ---- Track B3: drifting world, adaptive learner ----

use super::model::{AdaptMode, CUSUM_H, CUSUM_K, DEFAULT_HALF_LIFE, KEEP_AFTER_ALARM};
use super::planner::DEFAULT_NOVELTY;

/// Settings of the learner under test (the logged being uses the defaults of `AdaptSpec::full`).
#[derive(Debug, Clone, Copy)]
pub struct AdaptSpec {
    pub mode: AdaptMode,
    pub half_life: f64,
    pub k: f64,
    pub h: f64,
    pub keep: f64,
    pub novelty: f64,
}

impl AdaptSpec {
    pub fn off() -> AdaptSpec {
        AdaptSpec { mode: AdaptMode::Off, half_life: DEFAULT_HALF_LIFE, k: CUSUM_K, h: CUSUM_H, keep: KEEP_AFTER_ALARM, novelty: DEFAULT_NOVELTY }
    }
    pub fn forget() -> AdaptSpec {
        AdaptSpec { mode: AdaptMode::Forget, ..AdaptSpec::off() }
    }
    pub fn full() -> AdaptSpec {
        AdaptSpec { mode: AdaptMode::Full, ..AdaptSpec::off() }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DRec {
    /// Energy spent before this step.
    pub at: i64,
    pub cost: i64,
    pub reward: i64,
    pub domain: Option<Domain>,
    pub strategy: Option<Strategy>,
    pub attempted: bool,
    pub correct: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct AlarmAt {
    pub energy: i64,
    pub step: u64,
    pub domain: Domain,
    pub strategy: Strategy,
    pub drop: bool,
}

#[derive(Debug, Clone)]
pub struct DriftRun {
    pub recs: Vec<DRec>,
    pub reward: i64,
    pub cost: i64,
    pub alarms: Vec<AlarmAt>,
}

impl DriftRun {
    pub fn per_budget(&self) -> f64 {
        if self.cost > 0 { self.reward as f64 / self.cost as f64 } else { 0.0 }
    }
    /// Reward per energy over the steps that started with `from <= energy spent < to`.
    pub fn window_e(&self, from: i64, to: i64) -> f64 {
        let (r, c) = self.recs.iter().filter(|x| x.at >= from && x.at < to).fold((0i64, 0i64), |a, x| (a.0 + x.reward, a.1 + x.cost));
        if c > 0 { r as f64 / c as f64 } else { 0.0 }
    }
}

/// One in-memory run in a (possibly drifting) menu world with the given learner settings.
pub fn run_drift(seed: u64, budget: i64, drift: bool, spec: &AdaptSpec, chooser: &mut dyn MenuChooser) -> DriftRun {
    let th = ScriptedThinker;
    let mut st = BeingState::default();
    st.start(Genesis { world_seed: seed, loop_seed: 0, budget, explore_c: 0.0, chooser: Chooser::Model, extended: false, drift, adapt: spec.mode, advice: Default::default() }, String::new());
    st.model.set_mode(spec.mode, spec.half_life);
    st.model.cusum_k = spec.k;
    st.model.cusum_h = spec.h;
    st.model.keep = spec.keep;
    let world = World::with_drift(seed, false, if drift { Some(budget) } else { None });
    let mut e = Engine::new(world, st, &th);
    let mut recs = vec![];
    let mut alarms = vec![];
    loop {
        let at = e.state.total_cost;
        let n_alarms = e.state.changes.len();
        let Ok(o) = e.step_menu(chooser) else { break };
        let a = o.outcome;
        recs.push(DRec { at, cost: a.cost, reward: a.reward, domain: if a.skipped { None } else { Some(a.domain) }, strategy: a.strategy, attempted: !a.skipped, correct: a.correct });
        for c in &e.state.changes[n_alarms..] {
            alarms.push(AlarmAt { energy: c.clock / super::world::SECS_PER_ENERGY, step: c.step, domain: c.alarm.domain, strategy: c.alarm.strategy, drop: c.alarm.drop });
        }
    }
    DriftRun { recs, reward: e.state.total_reward, cost: e.state.total_cost, alarms }
}

/// Energy at which each scheduled change takes effect.
pub fn schedule_energy(seed: u64, budget: i64) -> [i64; 3] {
    let w = World::with_drift(seed, false, Some(budget));
    let d = w.drift.expect("drift schedule");
    [d.at[0] / super::world::SECS_PER_ENERGY, d.at[1] / super::world::SECS_PER_ENERGY, d.at[2] / super::world::SECS_PER_ENERGY]
}

fn model_chooser(spec: &AdaptSpec) -> ModelChooser {
    ModelChooser { novelty: spec.novelty, ..ModelChooser::new(super::planner::DEFAULT_BETA) }
}

pub struct DriftRow {
    pub seed: u64,
    pub budget: i64,
    pub drift: bool,
    /// Energy of the three changes (empty schedule values when `drift` is false).
    pub at: [i64; 3],
    pub random: DriftRun,
    pub greedy: DriftRun,
    pub ucb: DriftRun,
    pub off: DriftRun,
    pub forget: DriftRun,
    pub full: DriftRun,
    /// Attribution control: cold-start bonus only (no forgetting, detector never fires).
    pub novel: DriftRun,
    pub oracle: DriftRun,
}

pub fn drift_row(seed: u64, budget: i64, drift: bool, forget: &AdaptSpec, full: &AdaptSpec) -> DriftRow {
    let world = World::with_drift(seed, false, if drift { Some(budget) } else { None });
    let off = AdaptSpec::off();
    let novel_spec = AdaptSpec { mode: AdaptMode::Full, half_life: 1e12, h: 1e12, ..*full };
    let degrade_at = world.drift.as_ref().map_or(i64::MAX, |d| d.at[2]);
    let mut oracle = ScheduleOracle { before: truth_flag(&world, &[], false), after: if drift { truth_flag(&world, &[], true) } else { truth_flag(&world, &[], false) }, degrade_at };
    DriftRow {
        seed,
        budget,
        drift,
        at: if drift { schedule_energy(seed, budget) } else { [i64::MAX; 3] },
        random: run_drift(seed, budget, drift, &off, &mut RandomMenu::new(seed)),
        greedy: run_drift(seed, budget, drift, &off, &mut GreedyMenu { strategy: Strategy::Careful }),
        ucb: run_drift(seed, budget, drift, &off, &mut UcbMenu::new(seed, super::chooser::DEFAULT_EXPLORE_C)),
        off: run_drift(seed, budget, drift, &off, &mut model_chooser(&off)),
        forget: run_drift(seed, budget, drift, forget, &mut model_chooser(forget)),
        full: run_drift(seed, budget, drift, full, &mut model_chooser(full)),
        novel: run_drift(seed, budget, drift, &novel_spec, &mut model_chooser(&novel_spec)),
        oracle: run_drift(seed, budget, drift, &off, &mut oracle),
    }
}

/// Energy after the start of a regime until the trailing `w`-energy reward per energy of `run` first
/// reaches `frac` of the oracle's mean over the regime `[from, to)`. The trailing window lies fully
/// inside the regime, so the shortest possible answer is `w`. None: never within the regime.
pub fn recovery(run: &DriftRun, oracle: &DriftRun, from: i64, to: i64, w: i64, frac: f64) -> Option<i64> {
    let level = oracle.window_e(from, to);
    let mut t = from + w;
    while t <= to {
        if run.window_e(t - w, t) >= frac * level {
            return Some(t - from);
        }
        t += 10;
    }
    None
}

/// Energy after `from` until the first attempt in the new domain.
pub fn first_units_attempt(run: &DriftRun, from: i64) -> Option<i64> {
    run.recs.iter().find(|r| r.at >= from && r.domain == Some(Domain::Units)).map(|r| r.at - from)
}

/// Energy after `from` until the trailing 10 attempts in the new domain earn at least `frac` of the oracle's mean reward per energy there.
pub fn units_competence(run: &DriftRun, oracle: &DriftRun, from: i64, frac: f64) -> Option<i64> {
    let rate = |r: &DriftRun| {
        let (a, b) = r.recs.iter().filter(|x| x.at >= from && x.domain == Some(Domain::Units)).fold((0i64, 0i64), |s, x| (s.0 + x.reward, s.1 + x.cost));
        if b > 0 { a as f64 / b as f64 } else { 0.0 }
    };
    let level = rate(oracle);
    let u: Vec<&DRec> = run.recs.iter().filter(|x| x.at >= from && x.domain == Some(Domain::Units)).collect();
    for j in 9..u.len() {
        let (a, b) = u[j - 9..=j].iter().fold((0i64, 0i64), |s, x| (s.0 + x.reward, s.1 + x.cost));
        if b > 0 && a as f64 / b as f64 >= frac * level {
            return Some(u[j].at - from);
        }
    }
    None
}

// ---- Track B4: advice ----

use super::advice::{AdviceRequest, Ranked, Ranker};

/// A ranker for the simulated advisor: every (offer, strategy) of a request ordered best first by true
/// reward per energy. Uses the hidden truth, as the simulated advisor is supposed to.
pub fn truth_ranker(truth: Truth, strategies: Vec<Strategy>) -> Ranker {
    Box::new(move |req: &AdviceRequest| {
        let mut v = vec![];
        for o in &req.offers {
            let Some(d) = Domain::parse(&o.domain) else { continue };
            for &s in &strategies {
                let p = truth.p[d.idx()][(o.difficulty.clamp(1, 5) - 1) as usize][s.idx()];
                v.push(Ranked { slot: o.slot, strategy: s, value: o.listed_reward as f64 * p / s.energy(o.listed_cost) as f64, p });
            }
        }
        v.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap().then(a.slot.cmp(&b.slot)).then(a.strategy.cmp(&b.strategy)));
        v
    })
}

/// (signature, form code) of the rules a perfect discoverer would hold; what a simulated advisor knows about rules.
pub fn rule_table() -> Vec<(u8, String)> {
    perfect_rules().iter().map(|r| (r.sig, r.form.code())).collect()
}

#[derive(Debug, Clone, Copy)]
pub struct StepInfo {
    pub has_advice: bool,
    /// Advice was followed although the model alone preferred something else.
    pub tested: bool,
    pub followed: bool,
    /// Test outcome (Some only for a test).
    pub win: Option<bool>,
    pub cost: i64,
}

pub struct AdvRun {
    pub run: MenuRun,
    pub state: BeingState,
    pub info: Vec<StepInfo>,
    /// (step count when accepted, rule), rules from discovery passes and from advisor proposals alike.
    pub accepted: Vec<(u64, Rule)>,
    pub alarms: Vec<(u64, super::trust::TrustAlarm)>,
}

/// Settings of the trust learner under test (the logged being uses the defaults).
#[derive(Debug, Clone, Copy)]
pub struct TrustSpec {
    pub beta: f64,
    pub k: f64,
    pub h: f64,
    pub keep: f64,
}

impl Default for TrustSpec {
    fn default() -> TrustSpec {
        TrustSpec { beta: super::trust::DEFAULT_TRUST_BETA, k: super::trust::CUSUM_K, h: super::trust::CUSUM_H, keep: super::trust::KEEP_AFTER_ALARM }
    }
}

/// A simulated advisor of quality `q` for a world, consulting the hidden truth (`truth` from `truth_in(world, &[])`).
pub fn sim_advisor(truth: &Truth, extended: bool, seed: u64, q: f64) -> super::advice::SimulatedAdvisor {
    let a = super::advice::SimulatedAdvisor::new(seed, q, truth_ranker(truth.clone(), STRATEGIES.to_vec()));
    if extended { a.with_rules(rule_table()) } else { a }
}

/// One in-memory menu run with an advisor (None: no advice), `mode` deciding how advice is used.
/// `discover_every` 0: no discovery pass.
pub fn run_adv(world: World, budget: i64, mode: super::planner::AdviceMode, spec: &TrustSpec, advisor: Option<&dyn super::advice::Advisor>, discover_every: u64, no_discovered: bool) -> AdvRun {
    let extended = world.extended;
    let world_seed = world.seed;
    let th = ScriptedThinker;
    let mut st = BeingState::default();
    st.start(Genesis { world_seed, loop_seed: 0, budget, explore_c: 0.0, chooser: Chooser::Model, extended, drift: false, adapt: Default::default(), advice: Default::default() }, String::new());
    st.trust.cusum_k = spec.k;
    st.trust.cusum_h = spec.h;
    st.trust.keep = spec.keep;
    let mut chooser = ModelChooser { advice: super::planner::AdviceCfg { mode, trust_beta: spec.beta, lift: super::trust::LIFT }, no_discovered, ..ModelChooser::new(super::planner::DEFAULT_BETA) };
    let mut e = Engine::new(world, st, &th).with_advisor(advisor);
    let (mut recs, mut skips, mut strategies, mut info, mut accepted) = (vec![], 0, [0u64; 4], vec![], vec![]);
    while let Ok(o) = e.step_menu(&mut chooser) {
        let a = o.outcome;
        for nr in &o.rules {
            accepted.push((e.state.step, nr.rule.clone()));
        }
        let (has_advice, followed, tested, win) = match &o.advice {
            Some(t) => {
                let tested = t.followed && !t.agree;
                let win = if tested && !a.skipped { Some(a.reward as f64 / a.cost.max(1) as f64 >= t.ref_score) } else { None };
                (true, t.followed, tested, win)
            }
            None => (false, false, false, None),
        };
        info.push(StepInfo { has_advice, tested, followed, win, cost: a.cost });
        if o.predicted_p.is_some() || a.skipped {
            if a.skipped {
                skips += 1;
            } else if let Some(s) = a.strategy {
                strategies[s.idx()] += 1;
            }
            recs.push(Rec { reward: a.reward, cost: a.cost, attempted: !a.skipped, correct: a.correct, pred: o.predicted_p });
        }
        if discover_every > 0 && e.state.step % discover_every == 0 {
            for nr in e.discover_pass() {
                accepted.push((e.state.step, nr.rule));
            }
        }
    }
    let alarms = e.state.advice_alarms.clone();
    AdvRun { run: MenuRun { reward: e.state.total_reward, cost: e.state.total_cost, recs, skips, strategies }, state: e.state, info, accepted, alarms }
}
