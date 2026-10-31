//! Track B5 measurement: composition of verified building blocks, rule pruning, caps, kill switch.
//! Everything runs on the synthetic `extended2` world (author-defined families, author-defined rewards)
//! without a log (the same engine as the logged loop). Hidden truth is used ONLY here, to score the
//! being; the being never sees it.

use super::controller::Limits;
use super::discover::{self, Rule};
use super::planner::{ModelChooser, DEFAULT_BETA, DEFAULT_CURIOSITY};
use super::policy::{Policy, StopReason};
use super::run_loop::Engine;
use super::solver::ScriptedThinker;
use super::state::{tag, BeingState, Chooser, Genesis};
use super::world::{Domain, Kind, World, SECS_PER_ENERGY};
use std::collections::HashMap;

const ORACLE_OFFSET: u64 = 20_000_000;

/// Families the plain grammar can express (B2), fixed solvers cannot solve.
pub const GRAMMAR: [Kind; 6] = [Kind::ExtAffGeom, Kind::ExtPoly3, Kind::ExtRec2, Kind::ExtTribo, Kind::ExtAlt, Kind::ExtPeriodic];
/// Families outside the plain grammar that composition of verified blocks can express.
pub const COMPOSABLE: [Kind; 4] = [Kind::Ext2SplitQuad, Kind::Ext2ParityCubic, Kind::Ext2CubeGeom, Kind::Ext2AltCube];
/// Negative controls: no rule of this search space is exact for them.
pub const NEGATIVE: [Kind; 4] = [Kind::NegProd, Kind::NegPrimes, Kind::NegLcg, Kind::NegMod7];

#[derive(Debug, Clone, Copy)]
pub struct Use {
    pub step: u64,
    /// World clock at the end of the attempt.
    pub clock: i64,
    pub correct: bool,
}

#[derive(Debug, Clone)]
pub struct B5Out {
    pub reward: i64,
    pub cost: i64,
    pub steps: u64,
    pub stopped: Option<StopReason>,
    /// (step count when accepted, rule)
    pub accepted: Vec<(u64, Rule)>,
    /// (step count, rule id, reason tag)
    pub retired: Vec<(u64, String, String)>,
    /// Live rules after each step.
    pub live: Vec<usize>,
    /// Attempts whose answer came from a rule, with the step and outcome.
    pub uses: Vec<Use>,
    /// Per step: (clock before, reward, cost).
    pub trace: Vec<(i64, i64, i64)>,
    pub refusals: Vec<super::controller::Refusal>,
    pub max_candidates_seen: usize,
    pub state: BeingState,
}

impl B5Out {
    pub fn per_budget(&self) -> f64 {
        if self.cost > 0 { self.reward as f64 / self.cost as f64 } else { 0.0 }
    }
    /// Reward per energy of the steps that started at or after world-clock second `from`.
    pub fn per_budget_after(&self, from: i64) -> f64 {
        let (r, c) = self.trace.iter().filter(|t| t.0 >= from).fold((0i64, 0i64), |a, t| (a.0 + t.1, a.1 + t.2));
        if c > 0 { r as f64 / c as f64 } else { 0.0 }
    }
    pub fn max_live(&self) -> usize {
        self.live.iter().copied().max().unwrap_or(0)
    }
}

/// One run in the extended2 world. `limits`: None = the B2 behaviour (plain grammar search, no caps).
/// `every`: discovery pass every N steps (0 = never).
pub fn run_b5(seed: u64, budget: i64, rule_drift: bool, limits: Option<Limits>, every: u64, policy: &Policy) -> B5Out {
    let world = World::with_ext2(seed, if rule_drift { Some(budget) } else { None });
    let th = ScriptedThinker;
    let mut st = BeingState::default();
    st.start(Genesis { world_seed: seed, loop_seed: 0, budget, explore_c: 0.0, chooser: Chooser::Model, extended: true, ext2: true, rule_drift, drift: false, adapt: Default::default(), advice: Default::default() }, String::new());
    let mut e = Engine::new(world, st, &th).with_limits(limits);
    let mut chooser = ModelChooser { curiosity: DEFAULT_CURIOSITY, ..ModelChooser::new(DEFAULT_BETA) };
    let (mut accepted, mut retired, mut live, mut uses, mut trace) = (vec![], vec![], vec![], vec![], vec![]);
    let (mut done, mut reward, mut cost) = (0u64, 0i64, 0i64);
    let mut stopped = None;
    loop {
        if let Some(r) = policy.gate_cost(done, cost) {
            stopped = Some(r);
            break;
        }
        let clock0 = e.state.clock;
        let Ok(o) = e.step_menu(&mut chooser) else { break };
        let a = o.outcome;
        done += 1;
        reward += a.reward;
        cost += a.cost;
        trace.push((clock0, a.reward, a.cost));
        if a.rule_used.is_some() && !a.skipped {
            uses.push(Use { step: a.step, clock: a.clock, correct: a.correct });
        }
        if every > 0 && e.state.step % every == 0 {
            for nr in e.discover_pass() {
                accepted.push((e.state.step, nr.rule));
            }
        }
        for (ev, _) in e.housekeeping() {
            retired.push((e.state.step, tag(&ev.tags.join(","), "rule_id").unwrap_or("").to_string(), tag(&ev.tags.join(","), "reason").unwrap_or("").to_string()));
        }
        live.push(e.state.live_rules());
    }
    B5Out { reward, cost, steps: done, stopped, accepted, retired, live, uses, trace, refusals: e.refusals_total.clone(), max_candidates_seen: e.max_candidates_seen, state: e.state }
}

/// World-clock second at which the rule drift of a world with this budget happens.
pub fn drift_clock(budget: i64) -> i64 {
    budget / 2 * SECS_PER_ENERGY
}

/// Fraction of fresh tasks of one hidden family on which the book applies a rule and gets the exact answer.
pub fn coverage(world: &World, rules: &[Rule], kind: Kind, n: u64) -> f64 {
    let ok = (0..n)
        .filter(|i| {
            let t = world.task_of_kind(ORACLE_OFFSET + i, kind);
            discover::apply(rules, t.view.difficulty, &t.view.params).map_or(false, |(v, _)| t.check(v))
        })
        .count();
    ok as f64 / n as f64
}

/// (share of tasks where some rule is applied, share of those applications that are wrong).
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
pub struct Found {
    /// Per family of GRAMMAR then COMPOSABLE: covered (>= 90% exact on 100 fresh tasks) by the final book.
    pub grammar: [bool; 6],
    pub composable: [bool; 4],
    /// Per negative control: (share of tasks where a rule is applied, wrong share of applications).
    pub negative: [(f64, f64); 4],
    pub rules: usize,
    pub composed_rules: usize,
    /// Accepted rules that are never applicable-and-wrong on 300 fresh tasks of their signature and right at least once, and the rest.
    pub exact: usize,
    pub wrong: usize,
    /// (signature, transform+form, applicable-and-wrong count of 300) for the wrong ones.
    pub wrong_rules: Vec<(u8, String, u32)>,
}

pub fn analyze(seed: u64, out: &B5Out) -> Found {
    let w = World::with_ext2(seed, None);
    let book: Vec<Rule> = out.accepted.iter().map(|a| a.1.clone()).collect();
    let mut grammar = [false; 6];
    for (i, k) in GRAMMAR.iter().enumerate() {
        grammar[i] = coverage(&w, &book, *k, COVER_N) >= COVER_MIN;
    }
    let mut composable = [false; 4];
    for (i, k) in COMPOSABLE.iter().enumerate() {
        composable[i] = coverage(&w, &book, *k, COVER_N) >= COVER_MIN;
    }
    let mut negative = [(0.0, 0.0); 4];
    for (i, k) in NEGATIVE.iter().enumerate() {
        negative[i] = applied_and_wrong(&w, &book, *k, 300);
    }
    let (mut exact, mut wrong) = (0, 0);
    let mut wrong_rules = vec![];
    for r in &book {
        let (mut ok, mut bad) = (0u32, 0u32);
        for j in 0..300u64 {
            let t = w.task_forced(ORACLE_OFFSET + j, Domain::Pattern, r.sig);
            match discover::predict_x(r.xf, &r.form, &t.view.params) {
                Some(v) if t.check(v) => ok += 1,
                Some(_) => bad += 1,
                None => {}
            }
        }
        if bad == 0 && ok > 0 {
            exact += 1;
        } else {
            wrong += 1;
            wrong_rules.push((r.sig, format!("{}+{}", r.xf.code(), r.form.code()), bad));
        }
    }
    Found { grammar, composable, negative, rules: book.len(), composed_rules: book.iter().filter(|r| !r.xf.is_id()).count(), exact, wrong, wrong_rules }
}

/// Count of outcomes by the cap that produced each refusal.
pub fn refusals_by_cap(out: &B5Out) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    for r in &out.refusals {
        *m.entry(r.cap.clone()).or_insert(0) += 1;
    }
    m
}

/// Wrong answers produced by a rule in attempts that ended at or after world-clock second `from`: (uses, wrong).
pub fn wrong_uses_after(out: &B5Out, from: i64) -> (usize, usize) {
    let u: Vec<&Use> = out.uses.iter().filter(|u| u.clock >= from).collect();
    (u.len(), u.iter().filter(|u| !u.correct).count())
}

/// Live rules at the given step counts (the last value is repeated when a run is shorter).
pub fn live_at(out: &B5Out, steps: &[usize]) -> Vec<usize> {
    steps.iter().map(|s| if out.live.is_empty() { 0 } else { out.live[(*s).min(out.live.len()) .saturating_sub(1)] }).collect()
}
