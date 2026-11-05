//! Track B3b: measurement helpers for the second attempt at adaptation (synthetic world,
//! author-defined schedule). In-memory runs of the same engine the logged loop uses; no log is written.
//!
//! Adds to the B3 helpers in `measure.rs` (which stay unchanged):
//!   * `run_drift_hook`: a run with an optional measurement-only hook that may change the model at the
//!     true change times (the "told when" diagnostic arms; no being under test is ever given this);
//!   * per-window regret against the schedule oracle and against a skip-aware oracle;
//!   * diagnostics of the attempt-1 failure (what each learner does in the new domain and with
//!     `fast` after it degrades).

use super::measure::{schedule_energy, truth_flag, AdaptSpec, AlarmAt, DRec, DriftRun, ScheduleOracle, Truth};
use super::adapt_b3b::{SwitchCfg, DEFAULT_SWITCH};
use super::model::AdaptMode;
use super::planner::{Action, Choice, MenuChooser, ModelChooser, Offer, DEFAULT_BETA};
use super::run_loop::Engine;
use super::solver::{ScriptedThinker, Strategy, STRATEGIES};
use super::state::{BeingState, Chooser, Genesis};
use super::world::{Domain, World, SECS_PER_ENERGY};

/// A measurement-only hook called before each step with the state and the world clock.
pub type Hook<'a> = &'a mut dyn FnMut(&mut BeingState, i64);

/// As `measure::run_drift`, plus an optional hook and the B3b model settings.
pub fn run_drift_hook(seed: u64, budget: i64, drift: bool, spec: &AdaptSpec, chooser: &mut dyn MenuChooser, hook: Option<Hook>) -> DriftRun {
    run_x(seed, budget, drift, spec, chooser, hook).run
}

/// A learner under test: the B3 settings plus the B3b switch settings (used in mode `Switch` only).
#[derive(Debug, Clone, Copy)]
pub struct Arm {
    pub spec: AdaptSpec,
    pub sw: SwitchCfg,
}

impl Arm {
    pub fn b1() -> Arm {
        Arm { spec: AdaptSpec::off(), sw: DEFAULT_SWITCH }
    }
    pub fn attempt1() -> Arm {
        Arm { spec: AdaptSpec::full(), sw: DEFAULT_SWITCH }
    }
    /// Attempt-1 cold-start bonus only (no forgetting, detector never fires).
    pub fn cold_start() -> Arm {
        Arm { spec: AdaptSpec { mode: AdaptMode::Full, half_life: 1e12, h: 1e12, ..AdaptSpec::full() }, sw: DEFAULT_SWITCH }
    }
    /// B3b with the given switch settings and cold-start bonus.
    pub fn switch(sw: SwitchCfg, novelty: f64) -> Arm {
        Arm { spec: AdaptSpec { mode: AdaptMode::Switch, novelty, ..AdaptSpec::off() }, sw }
    }
}

/// One run of a learner arm (the model planner with the arm's settings).
pub fn run_arm(seed: u64, budget: i64, drift: bool, arm: &Arm) -> XRun {
    let mut ch = model_chooser(&arm.spec);
    run_xs(seed, budget, drift, &arm.spec, Some(&arm.sw), &mut ch, None)
}

/// A run plus the difficulty of every step (0 for a skip), parallel to `run.recs`.
#[derive(Debug, Clone)]
pub struct XRun {
    pub run: DriftRun,
    pub diff: Vec<u8>,
    /// Every flagged change: (energy spent at the step, kind, domain, strategy, difficulty, drop).
    pub flags: Vec<(i64, super::model::Detector, Domain, Strategy, Option<u8>, bool)>,
}

impl XRun {
    pub fn per_budget(&self) -> f64 {
        self.run.per_budget()
    }
}

/// As `run_drift_hook`, keeping the difficulty of every attempt.
pub fn run_x(seed: u64, budget: i64, drift: bool, spec: &AdaptSpec, chooser: &mut dyn MenuChooser, hook: Option<Hook>) -> XRun {
    run_xs(seed, budget, drift, spec, None, chooser, hook)
}

/// As `run_x` with B3b switch settings.
pub fn run_xs(seed: u64, budget: i64, drift: bool, spec: &AdaptSpec, sw: Option<&SwitchCfg>, chooser: &mut dyn MenuChooser, mut hook: Option<Hook>) -> XRun {
    let th = ScriptedThinker;
    let mut st = BeingState::default();
    st.start(Genesis { world_seed: seed, loop_seed: 0, budget, explore_c: 0.0, chooser: Chooser::Model, extended: false, drift, adapt: spec.mode, advice: Default::default(), ext2: false, rule_drift: false, transfer: false, learn_transfer: false }, String::new());
    st.model.set_mode(spec.mode, spec.half_life);
    st.model.cusum_k = spec.k;
    st.model.cusum_h = spec.h;
    st.model.keep = spec.keep;
    if let Some(c) = sw {
        st.model.sw_cfg = *c;
    }
    let world = World::with_drift(seed, false, if drift { Some(budget) } else { None });
    let mut e = Engine::new(world, st, &th);
    let mut recs = vec![];
    let mut alarms = vec![];
    let mut diff = vec![];
    let mut flags = vec![];
    loop {
        if let Some(h) = hook.as_mut() {
            let clock = e.state.clock;
            h(&mut e.state, clock);
        }
        let at = e.state.total_cost;
        let n_alarms = e.state.changes.len();
        let Ok(o) = e.step_menu(chooser) else { break };
        let a = o.outcome;
        recs.push(DRec { at, cost: a.cost, reward: a.reward, domain: if a.skipped { None } else { Some(a.domain) }, strategy: a.strategy, attempted: !a.skipped, correct: a.correct });
        diff.push(if a.skipped { 0 } else { a.difficulty });
        for c in &e.state.changes[n_alarms..] {
            flags.push((at, c.alarm.kind, c.alarm.domain, c.alarm.strategy, c.alarm.cell, c.alarm.drop));
            alarms.push(AlarmAt { energy: c.clock / SECS_PER_ENERGY, step: c.step, domain: c.alarm.domain, strategy: c.alarm.strategy, drop: c.alarm.drop });
        }
    }
    XRun { run: DriftRun { recs, reward: e.state.total_reward, cost: e.state.total_cost, alarms }, diff, flags }
}

pub fn model_chooser(spec: &AdaptSpec) -> ModelChooser {
    ModelChooser { novelty: spec.novelty, ..ModelChooser::new(DEFAULT_BETA) }
}

/// The schedule oracle of B3 (knows the hidden success rates and the time of the degradation; never skips).
pub fn schedule_oracle(seed: u64, budget: i64, drift: bool) -> ScheduleOracle {
    let world = World::with_drift(seed, false, if drift { Some(budget) } else { None });
    let degrade_at = world.drift.as_ref().map_or(i64::MAX, |d| d.at[2]);
    ScheduleOracle { before: truth_flag(&world, &[], false), after: if drift { truth_flag(&world, &[], true) } else { truth_flag(&world, &[], false) }, degrade_at }
}

/// Oracle that knows the hidden success rates and the schedule and also skips a menu when its best
/// expected reward per energy is below `skip_frac` times its own realized rate (the same skip rule
/// the model planner uses). Diagnostic: the B3 oracle never skips, so a learner that skips can beat it.
pub struct SkipOracle {
    pub inner: ScheduleOracle,
    pub skip_frac: f64,
}

impl MenuChooser for SkipOracle {
    fn name(&self) -> String {
        "oracle-schedule-skip".into()
    }
    fn choose(&mut self, st: &BeingState, offers: &[Offer]) -> Option<Choice> {
        let t: &Truth = if st.clock >= self.inner.degrade_at { &self.inner.after } else { &self.inner.before };
        let mut best: Option<(f64, usize, Strategy)> = None;
        for o in offers {
            for &s in &STRATEGIES {
                let e = s.energy(o.listed_cost);
                if e > st.budget_left {
                    continue;
                }
                let v = o.listed_reward as f64 * t.p[o.domain.idx()][o.difficulty as usize - 1][s.idx()] / e as f64;
                if best.map_or(true, |b| v > b.0) {
                    best = Some((v, o.slot, s));
                }
            }
        }
        let (v, slot, strategy) = best?;
        let rate = if st.total_cost > 0 { st.total_reward as f64 / st.total_cost as f64 } else { 0.0 };
        let action = if st.total_cost >= super::planner::SKIP_MIN_COST && v < self.skip_frac * rate && st.budget_left >= super::world::SKIP_COST { Action::Skip } else { Action::Attempt { slot, strategy } };
        Some(Choice { action, policy: self.name(), why: "knows hidden truth".into(), p: 0.0, sd: 0.0, cost_est: 0.0, score: v, rationale: serde_json::json!({}), advice: None })
    }
}

/// Reward per energy over steps whose start lies in `[from, to)` (energy).
pub fn window(run: &DriftRun, from: i64, to: i64) -> f64 {
    run.window_e(from, to)
}

/// (attempts, successes) of `run` in steps starting in `[from, to)`, filtered.
pub fn count(run: &DriftRun, from: i64, to: i64, f: impl Fn(&DRec) -> bool) -> (u64, u64) {
    run.recs.iter().filter(|r| r.at >= from && r.at < to && r.attempted && f(r)).fold((0, 0), |a, r| (a.0 + 1, a.1 + r.correct as u64))
}

/// Reward and energy of `run` in steps starting in `[from, to)`, filtered.
pub fn reward_cost(run: &DriftRun, from: i64, to: i64, f: impl Fn(&DRec) -> bool) -> (i64, i64) {
    run.recs.iter().filter(|r| r.at >= from && r.at < to && f(r)).fold((0, 0), |a, r| (a.0 + r.reward, a.1 + r.cost))
}

/// Schedule (energy) of a drifting world.
pub fn at(seed: u64, budget: i64) -> [i64; 3] {
    schedule_energy(seed, budget)
}

/// A told-when hook: at the true time of the degradation it clears every `fast` cell (all domains), as an
/// upper bound on what a perfect change detector for that change could do with a reset. Measurement only.
pub fn told_reset_fast(degrade_clock: i64) -> impl FnMut(&mut BeingState, i64) {
    let mut done = false;
    move |st: &mut BeingState, clock: i64| {
        if !done && clock >= degrade_clock {
            done = true;
            for d in super::world::ALL_DOMAINS {
                for k in 1..=5u8 {
                    st.model.reset_cell(d, k, Strategy::Fast);
                }
            }
        }
    }
}

/// Clock (world seconds) of the three changes.
pub fn change_clocks(seed: u64, budget: i64) -> [i64; 3] {
    World::with_drift(seed, false, Some(budget)).drift.expect("drift").at
}


/// Measurement-only upper bound ("told what"): at the true time of change `i` (0 new domain, 1 shift,
/// 2 degradation) the cells the change concerns are given `n_fake` pseudo-attempts at their true success
/// rate (after clearing them for the degradation). No learner under test gets this; it bounds what a
/// perfect detector plus perfect prior could recover.
pub fn told_what(seed: u64, budget: i64, which: [bool; 3], n_fake: u32) -> impl FnMut(&mut BeingState, i64) {
    told(seed, budget, which, n_fake, false)
}

/// As `told_what` for the new domain only, but the pseudo-attempts are at the MEAN true rate of the four
/// old domains at the same (difficulty, strategy): the best case of any prior taken from similar domains.
pub fn told_similar(seed: u64, budget: i64, n_fake: u32) -> impl FnMut(&mut BeingState, i64) {
    told(seed, budget, [true, false, false], n_fake, true)
}

fn told(seed: u64, budget: i64, which: [bool; 3], n_fake: u32, similar: bool) -> impl FnMut(&mut BeingState, i64) {
    use super::model::Obs;
    let w = World::with_drift(seed, false, Some(budget));
    let clocks = w.drift.as_ref().expect("drift").at;
    let before = truth_flag(&w, &[], false);
    let after = truth_flag(&w, &[], true);
    let mut done = [false; 3];
    move |st: &mut BeingState, clock: i64| {
        for i in 0..3 {
            if !which[i] || done[i] || clock < clocks[i] {
                continue;
            }
            done[i] = true;
            let (doms, strats, t): (Vec<Domain>, Vec<Strategy>, &Truth) = match i {
                0 => (vec![Domain::Units], STRATEGIES.to_vec(), &before),
                1 => (vec![Domain::Physics, Domain::Arithmetic], STRATEGIES.to_vec(), &before),
                _ => (super::world::ALL_DOMAINS.to_vec(), vec![Strategy::Fast], &after),
            };
            for d in doms {
                for k in 1..=5u8 {
                    for &s in &strats {
                        if i == 2 {
                            st.model.reset_cell(d, k, s);
                        }
                        let p = if similar { (0..4).map(|o| t.p[o][k as usize - 1][s.idx()]).sum::<f64>() / 4.0 } else { t.p[d.idx()][k as usize - 1][s.idx()] };
                        let succ = (p * n_fake as f64).round() as u32;
                        for j in 0..n_fake {
                            st.model.observe(&Obs { domain: d, difficulty: k, strategy: s, correct: j < succ, listed_cost: 4, spent: s.energy(4) });
                        }
                    }
                }
            }
        }
    }
}
