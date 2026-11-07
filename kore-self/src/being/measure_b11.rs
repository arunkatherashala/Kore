//! Track B11, part 1: the slow decline of reward per energy in a stationary world (docs/longrun.md, bug 4).
//!
//! An in-process copy of the long run's being (world `world_seed` with the `extended2` pattern
//! families, chooser `model`, discovery every 10 steps, composition and pruning under the long-run
//! controller, budget refills), stepped through the normal `Engine::step_menu`, with a measurement
//! side channel that is never logged and never read by the being:
//!
//! * per step, the decision a skip-aware oracle would make on the same offers (it knows the hidden
//!   success rate of every strategy and the real energy, and skips below `lambda`);
//! * the value of both decisions in renewal-reward terms with the oracle's long-run rate `rho`
//!   (attempt: `reward x p - rho x energy`; skip: `-rho x SKIP_COST`), so the loss of a step is
//!   `oracle value - being value` and splits into skip errors (one skips, the other attempts) and
//!   choice errors (both attempt, different options);
//! * the optimism bonus of the being's best option (`score - mean score`), its skip bar, and the error
//!   of its success estimate on the cells it attempted.
//!
//! Measurement only; the being under test is the unchanged loop (with the `SkipRule` switch of the planner).

use super::controller::Controller;
use super::measure::{truth_in, Truth};
use super::planner::{Action, MenuChooser, ModelChooser, Offer, SkipRule};
use super::run_loop::{offers_of, world_for, Engine};
use super::solver::{ScriptedThinker, Strategy, STRATEGIES, STRATEGIES_EXT};
use super::state::{BeingState, Chooser, Genesis};
use super::world::SKIP_COST;

/// The controller of the long run (docs/longrun.md).
pub const LONGRUN_CONTROLLER: &str = "max_live_rules = 16\nmax_building_blocks = 8\nmax_search_candidates = 2000\nmax_steps_per_run = 5000\nmax_energy_per_run = 100000\ncomposition = on\npruning = on\n";

#[derive(Debug, Clone, Copy)]
pub struct DeclineCfg {
    pub world_seed: u64,
    pub steps: u64,
    pub window: u64,
    pub budget: i64,
    pub skip_rule: SkipRule,
    /// Skip threshold of the oracle (reward per energy).
    pub lambda: f64,
    /// Long-run rate of the oracle, the price of energy in the step values.
    pub rho: f64,
    /// Discovery pass every N steps (0 = never; the long run used 10).
    pub discover_every: u64,
}

#[derive(Debug, Clone, Default)]
pub struct DeclineWin {
    pub from: u64,
    pub steps: u64,
    pub reward: i64,
    pub cost: i64,
    pub attempts: u64,
    pub skips: u64,
    /// The being skipped where the oracle attempts / attempted where the oracle skips.
    pub over_skip: u64,
    pub under_skip: u64,
    /// Summed step losses (oracle value minus being value, true expectations), by kind.
    pub loss_skip: f64,
    pub loss_choice: f64,
    /// Summed optimism bonus (score - mean score) of the being's best option, and its skip bar.
    pub bonus: f64,
    pub bar: f64,
    /// Summed |estimated p - true p| over attempts.
    pub est_err: f64,
    /// Attempts the oracle would make on the same offers.
    pub oracle_attempts: u64,
    /// Summed optimism bonus (score - mean score) of the option attempted, over attempts.
    pub bonus_attempted: f64,
    /// Steps whose best option's mean score (no bonus) lies in [bar - 0.25, bar): the margin the bonus decides.
    pub near_bar: u64,
    /// Live rules at the end of the window.
    pub rules_end: usize,
}

impl DeclineWin {
    pub fn rpe(&self) -> f64 {
        if self.cost > 0 { self.reward as f64 / self.cost as f64 } else { 0.0 }
    }
}

fn truth_p(t: &Truth, o: &Offer, s: Strategy) -> f64 {
    t.p[o.domain.idx()][o.difficulty as usize - 1][s.idx()]
}

/// The oracle's best (value per energy, slot, strategy) on these offers.
pub fn oracle_best(t: &Truth, strategies: &[Strategy], offers: &[Offer], budget_left: i64) -> Option<(f64, usize, Strategy)> {
    let mut best: Option<(f64, usize, Strategy)> = None;
    for o in offers {
        for &s in strategies {
            let e = s.energy(o.listed_cost);
            if e > budget_left {
                continue;
            }
            let v = o.listed_reward as f64 * truth_p(t, o, s) / e as f64;
            if best.map_or(true, |b| v > b.0) {
                best = Some((v, o.slot, s));
            }
        }
    }
    best
}

pub fn genesis(world_seed: u64, budget: i64) -> Genesis {
    Genesis { world_seed, loop_seed: 1, budget, explore_c: super::planner::DEFAULT_BETA, chooser: Chooser::Model, extended: true, ext2: true, rule_drift: false, drift: false, adapt: Default::default(), advice: Default::default(), transfer: false, learn_transfer: false }
}

/// Run the long-run being in-process with the diagnostic side channel. Returns one row per window.
pub fn decline_run(cfg: &DeclineCfg) -> Vec<DeclineWin> {
    let g = genesis(cfg.world_seed, cfg.budget);
    let mut st = BeingState::default();
    st.start(g.clone(), String::new());
    let (limits, _) = Controller::parse(LONGRUN_CONTROLLER).expect("controller").limits(true, true);
    let th = ScriptedThinker;
    let world = world_for(&g);
    let mut truth = truth_in(&world, &[]);
    let mut truth_rules = 0usize;
    let mut e = Engine::new(world, st, &th).with_limits(Some(limits));
    let mut mc = ModelChooser { skip_rule: cfg.skip_rule, ..ModelChooser::for_genesis(&g) };
    let mut out: Vec<DeclineWin> = vec![];
    let mut w = DeclineWin::default();
    let mut n = 0u64;
    while n < cfg.steps {
        if e.state.rules.len() != truth_rules {
            truth_rules = e.state.rules.len();
            truth = truth_in(&e.world, &e.state.rules);
        }
        let strategies: &[Strategy] = if e.state.rules.is_empty() { &STRATEGIES } else { &STRATEGIES_EXT };
        let offers = offers_of(&e.world.offers_at(e.state.step, e.state.clock));
        // the being's own view (same function the planner calls)
        let cands = mc.candidates(&e.state, &offers);
        let bi = cands.iter().enumerate().filter(|(_, c)| c.affordable).fold(None, |b: Option<usize>, (i, c)| if b.map_or(true, |b| c.score > cands[b].score) { Some(i) } else { b });
        let ob = oracle_best(&truth, strategies, &offers, e.state.budget_left);
        let res = e.step_menu(&mut mc);
        let o = match res {
            Ok(o) => o,
            Err(super::StopReason::BudgetExhausted) => {
                let c = e.state.clock;
                e.state.apply_refill(g.budget, c + 86_400);
                continue;
            }
            Err(_) => break,
        };
        n += 1;
        if cfg.discover_every > 0 && e.state.step % cfg.discover_every == 0 {
            let _ = e.discover_pass();
        }
        let _ = e.housekeeping();
        let _ = e.take_refusals();
        let a = &o.outcome;
        w.steps += 1;
        w.reward += a.reward;
        w.cost += a.cost;
        if a.skipped { w.skips += 1 } else { w.attempts += 1 }
        let rate = if e.state.total_cost > 0 { e.state.total_reward as f64 / e.state.total_cost as f64 } else { 0.0 };
        if let Some(bi) = bi {
            let c = &cands[bi];
            w.bonus += c.score - c.mean_score;
            w.bar += cfg.skip_rule.bar(mc.skip_frac, rate, c.cost_est, SKIP_COST, c.n);
        }
        let skip_v = -cfg.rho * SKIP_COST as f64;
        let val = |slot: usize, s: Strategy| {
            let off = &offers[slot];
            off.listed_reward as f64 * truth_p(&truth, off, s) - cfg.rho * s.energy(off.listed_cost) as f64
        };
        // the reference decision: the best option by renewal value (reward x p - rho x energy), or skip
        let o_act: Option<(usize, Strategy)> = if cfg.lambda < 0.0 {
            let _ = ob;
            renewal_best(&truth, strategies, &offers, e.state.budget_left, cfg.rho).filter(|x| x.0 > skip_v).map(|x| (x.1, x.2))
        } else {
            match ob {
                Some((v, slot, s)) if v >= cfg.lambda => Some((slot, s)),
                _ => None,
            }
        };
        if o_act.is_some() {
            w.oracle_attempts += 1;
        }
        // the oracle's value is the best of skip and its best attempt
        let o_val = o_act.map_or(skip_v, |(sl, s)| val(sl, s).max(skip_v));
        let b_act: Option<(usize, Strategy)> = match o.action.as_deref().and_then(|l| ch_parse(l)) {
            Some(x) if !a.skipped => Some(x),
            _ => None,
        };
        let b_val = b_act.map_or(skip_v, |(sl, s)| val(sl, s));
        let loss = (o_val - b_val).max(0.0);
        match (b_act.is_some(), o_act.is_some()) {
            (false, true) => {
                w.over_skip += 1;
                w.loss_skip += loss;
            }
            (true, false) => {
                w.under_skip += 1;
                w.loss_skip += loss;
            }
            (true, true) => w.loss_choice += loss,
            _ => {}
        }
        if let (Some((sl, s)), Some(p)) = (b_act, o.predicted_p) {
            w.est_err += (p - truth_p(&truth, &offers[sl], s)).abs();
            if let Some(c) = cands.iter().find(|c| c.slot == sl && c.strategy == s) {
                w.bonus_attempted += c.score - c.mean_score;
            }
        }
        if let Some(bi) = bi {
            let c = &cands[bi];
            let bar = cfg.skip_rule.bar(mc.skip_frac, rate, c.cost_est, SKIP_COST, c.n);
            if c.mean_score < bar && c.mean_score >= bar - 0.25 {
                w.near_bar += 1;
            }
        }
        if n % cfg.window == 0 {
            w.from = n - cfg.window;
            w.rules_end = e.state.live_rules();
            out.push(std::mem::take(&mut w));
        }
    }
    if w.steps > 0 {
        w.from = n - w.steps;
        out.push(w);
    }
    out
}

/// The best (renewal value, slot, strategy): value = reward x p - rho x energy.
pub fn renewal_best(t: &Truth, strategies: &[Strategy], offers: &[Offer], budget_left: i64, rho: f64) -> Option<(f64, usize, Strategy)> {
    let mut best: Option<(f64, usize, Strategy)> = None;
    for o in offers {
        for &s in strategies {
            let e = s.energy(o.listed_cost);
            if e > budget_left {
                continue;
            }
            let v = o.listed_reward as f64 * truth_p(t, o, s) - rho * e as f64;
            if best.map_or(true, |b| v > b.0) {
                best = Some((v, o.slot, s));
            }
        }
    }
    best
}

/// The renewal oracle: knows the hidden success rates and real energy, takes the option of highest
/// `reward x p - rho x energy` and skips when that is below `-rho x SKIP_COST`; `rho` is found by
/// iterating `rho <- the rate this policy earns` (Dinkelbach) from `rho0`. Returns (rate, rho).
pub fn renewal_oracle(world_seed: u64, budget: i64, rho0: f64, iters: usize) -> (f64, f64) {
    struct O {
        t: Truth,
        rho: f64,
    }
    impl MenuChooser for O {
        fn name(&self) -> String {
            "oracle-renewal".into()
        }
        fn choose(&mut self, st: &BeingState, offers: &[Offer]) -> Option<super::planner::Choice> {
            let (v, slot, strategy) = renewal_best(&self.t, &STRATEGIES, offers, st.budget_left, self.rho)?;
            let action = if v < -self.rho * SKIP_COST as f64 && st.budget_left >= SKIP_COST { Action::Skip } else { Action::Attempt { slot, strategy } };
            Some(super::planner::Choice { action, policy: "oracle-renewal".into(), why: String::new(), p: 0.0, sd: 0.0, cost_est: 0.0, score: v, rationale: serde_json::json!({}), advice: None })
        }
    }
    let g = genesis(world_seed, budget);
    let t = truth_in(&world_for(&g), &[]);
    let mut rho = rho0;
    let mut rate = 0.0;
    for _ in 0..iters.max(1) {
        let r = super::measure::run_menu_ext(world_for(&g), budget, &mut O { t: Truth { p: t.p }, rho }, 0, &[]);
        rate = r.run.per_budget();
        rho = rate;
    }
    (rate, rho)
}

fn ch_parse(label: &str) -> Option<(usize, Strategy)> {
    let (a, b) = label.split_once(':')?;
    Some((a.parse().ok()?, Strategy::parse(b)?))
}

/// The skip-aware oracle on its own (for `rho` and `lambda`): reward per energy over `budget` energy
/// with skip threshold `lambda`.
pub fn oracle_rate(world_seed: u64, lambda: f64, budget: i64) -> f64 {
    struct O {
        t: Truth,
        lambda: f64,
    }
    impl MenuChooser for O {
        fn name(&self) -> String {
            "oracle-skip".into()
        }
        fn choose(&mut self, st: &BeingState, offers: &[Offer]) -> Option<super::planner::Choice> {
            let (v, slot, strategy) = oracle_best(&self.t, &STRATEGIES, offers, st.budget_left)?;
            let action = if v < self.lambda && st.budget_left >= SKIP_COST { Action::Skip } else { Action::Attempt { slot, strategy } };
            Some(super::planner::Choice { action, policy: "oracle-skip".into(), why: String::new(), p: 0.0, sd: 0.0, cost_est: 0.0, score: v, rationale: serde_json::json!({}), advice: None })
        }
    }
    let g = genesis(world_seed, budget);
    let world = world_for(&g);
    let t = truth_in(&world, &[]);
    let r = super::measure::run_menu_ext(world, budget, &mut O { t, lambda }, 0, &[]);
    r.run.per_budget()
}

/// Best skip threshold of the oracle on a grid 0, 0.25, .., 8 and its rate.
pub fn oracle_best_lambda(world_seed: u64, budget: i64) -> (f64, f64) {
    let mut best = (oracle_rate(world_seed, 0.0, budget), 0.0);
    for i in 1..=32 {
        let l = i as f64 * 0.25;
        let r = oracle_rate(world_seed, l, budget);
        if r > best.0 {
            best = (r, l);
        }
    }
    best
}
