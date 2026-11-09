//! Track E1 measurement (synthetic first world, author-defined families and rewards).
//!
//! `run_e1` is `b14::measure::run_b14` (the long run's being, stepped through `Engine::step_menu`,
//! B14 switches as configured) plus the E1 evolution pass after the discovery and library passes, and
//! a measurement side channel that is never logged and never read by the being: per window reward per
//! energy, rules, E1 passes / evaluations / wall time / accepted rules, answers given by evolved rules
//! (right / wrong before the checker) and the coverage of the rule book on fresh tasks of every
//! hidden family, including the negative controls the B2 grammar cannot express. With evolution off
//! the run is `run_b14` step for step (test `off_is_run_b14`).
//!
//! Measurement only.

use super::super::b14::curiosity::Curious;
use super::super::b14::measure::{RunCfgB14, FAMILIES};
use super::super::controller::Controller;
use super::super::discover::Rule;
use super::super::measure::coverage;
use super::super::measure_b11::{genesis, LONGRUN_CONTROLLER};
use super::super::planner::ModelChooser;
use super::super::run_loop::{world_for, Engine};
use super::super::solver::ScriptedThinker;
use super::super::state::BeingState;
use super::super::world::{Domain, Kind};
use super::evolve::EvoCfg;

/// The negative controls of the extended2 world (outside the B2 grammar and composition).
pub const NEG_FAMILIES: [Kind; 4] = [Kind::NegLcg, Kind::NegMod7, Kind::NegProd, Kind::NegPrimes];

/// Every hidden sequence family of the extended2 world without drift: the 14 expressible ones, then the 4 negative controls.
pub fn all_families() -> Vec<Kind> {
    FAMILIES.iter().chain(NEG_FAMILIES.iter()).copied().collect()
}

#[derive(Debug, Clone)]
pub struct RunCfgE1 {
    pub b14: RunCfgB14,
    pub evo: Option<EvoCfg>,
}

#[derive(Debug, Clone, Default)]
pub struct WinE1 {
    pub from: u64,
    pub steps: u64,
    pub reward: i64,
    pub cost: i64,
    pub attempts: u64,
    pub skips: u64,
    pub rule_answers: u64,
    pub rule_right: u64,
    /// Answers produced by an evolved rule, and how many of them the checker found right.
    pub evo_answers: u64,
    pub evo_right: u64,
    pub accepted: u64,
    pub evo_accepted: u64,
    pub retired: u64,
    pub live_end: usize,
    pub cur_energy: i64,
    pub evo_evals: u64,
    pub evo_nanos: u128,
    /// Coverage per family of `all_families()` at the end of the window (empty unless `cover_n > 0`).
    pub cover: Vec<f64>,
}

impl WinE1 {
    pub fn rpe(&self) -> f64 {
        if self.cost > 0 { self.reward as f64 / self.cost as f64 } else { 0.0 }
    }
}

#[derive(Debug, Clone, Default)]
pub struct OutE1 {
    pub wins: Vec<WinE1>,
    pub rules_final: Vec<Rule>,
    /// Rules accepted by evolution: (step, rule).
    pub evolved: Vec<(u64, Rule)>,
    pub total_cost: i64,
    pub total_reward: i64,
    pub cur_energy: i64,
    pub evo_evals: u64,
    pub evo_passes: u64,
    pub evo_nanos: u128,
    pub evo_rejected: u64,
    pub evo_verified: u64,
    pub run_nanos: u128,
    pub state: Option<BeingState>,
}

/// One life in-process: `run_b14` with the E1 pass on top (when `cfg.evo` is set).
pub fn run_e1(cfg: &RunCfgE1) -> OutE1 {
    let t_run = std::time::Instant::now();
    let c = &cfg.b14;
    let mut g = genesis(c.world_seed, c.budget);
    g.rule_drift = c.rule_drift;
    let mut st = BeingState::default();
    st.start(g.clone(), String::new());
    for r in &c.preload {
        let mut r = r.clone();
        r.event_id = format!("preload-{}", r.id);
        st.apply_rule(r);
    }
    let (limits, _) = Controller::parse(LONGRUN_CONTROLLER).expect("controller").limits(true, true);
    let th = ScriptedThinker;
    let world = world_for(&g);
    let mut ch = Curious::new(ModelChooser { skip_rule: c.skip_rule, ..ModelChooser::for_genesis(&g) }, &world, c.library, c.curiosity);
    let mut e = Engine::new(world, st, &th).with_limits(Some(limits));
    if let Some(lc) = &c.library {
        e.enable_library(*lc, &c.import);
    }
    if let Some(ec) = cfg.evo {
        e.enable_evolution(ec);
    }
    let mut out = OutE1::default();
    let mut w = WinE1::default();
    let mut n = 0u64;
    let fams = all_families();
    let mut evo_ids: Vec<String> = vec![];
    let (mut ev0, mut ns0) = (0u64, 0u128);
    while n < c.steps {
        let res = e.step_menu(&mut ch);
        let o = match res {
            Ok(o) => o,
            Err(super::super::StopReason::BudgetExhausted) if c.refill => {
                let ck = e.state.clock;
                e.state.apply_refill(g.budget, ck + 86_400);
                continue;
            }
            Err(_) => break,
        };
        n += 1;
        let before = e.state.rules.len();
        let retired_before = e.state.rules.iter().filter(|r| r.retired).count();
        if c.discover_every > 0 && e.state.step % c.discover_every == 0 {
            let _ = e.discover_pass();
            let _ = e.library_pass();
        }
        let (_, evolved) = e.evolve_pass();
        for nr in evolved {
            evo_ids.push(nr.rule.id.clone());
            w.evo_accepted += 1;
        }
        let _ = e.housekeeping();
        let _ = e.library_retire();
        let _ = e.library_notes();
        let _ = e.take_refusals();
        w.accepted += (e.state.rules.len() - before) as u64;
        w.retired += (e.state.rules.iter().filter(|r| r.retired).count() - retired_before) as u64;
        let a = &o.outcome;
        w.steps += 1;
        w.reward += a.reward;
        w.cost += a.cost;
        if a.skipped {
            w.skips += 1
        } else {
            w.attempts += 1;
            if let Some(rid) = &a.rule_used {
                w.rule_answers += 1;
                w.rule_right += a.correct as u64;
                if evo_ids.iter().any(|x| x == rid) {
                    w.evo_answers += 1;
                    w.evo_right += a.correct as u64;
                }
            }
        }
        if ch.last_was_curious() {
            w.cur_energy += a.cost;
        }
        if n % c.window == 0 {
            w.from = n - c.window;
            w.live_end = e.state.live_rules();
            if let Some(x) = &e.evo {
                w.evo_evals = x.evals - ev0;
                w.evo_nanos = x.nanos - ns0;
                (ev0, ns0) = (x.evals, x.nanos);
            }
            if c.cover_n > 0 {
                w.cover = fams.iter().map(|&k| coverage(&e.world, &e.state.rules, k, c.cover_n)).collect();
            }
            out.wins.push(std::mem::take(&mut w));
        }
    }
    if w.steps > 0 {
        w.from = n - w.steps;
        w.live_end = e.state.live_rules();
        if let Some(x) = &e.evo {
            w.evo_evals = x.evals - ev0;
            w.evo_nanos = x.nanos - ns0;
        }
        if c.cover_n > 0 {
            w.cover = fams.iter().map(|&k| coverage(&e.world, &e.state.rules, k, c.cover_n)).collect();
        }
        out.wins.push(w);
    }
    out.cur_energy = e.state.b14_cur_energy;
    out.total_cost = e.state.total_cost;
    out.total_reward = e.state.total_reward;
    out.rules_final = e.state.rules.clone();
    if let Some(x) = &e.evo {
        out.evolved = x.accepted.clone();
        out.evo_evals = x.evals;
        out.evo_passes = x.passes;
        out.evo_nanos = x.nanos;
        out.evo_rejected = x.rejected;
        out.evo_verified = x.verified;
    }
    out.run_nanos = t_run.elapsed().as_nanos();
    out.state = Some(e.state);
    out
}

/// How an evolved rule does on fresh tasks of every hidden sequence family at its difficulty
/// (`n` tasks each): (family, applied, right). A rule is WRONG if it is applied and wrong on any.
pub fn fresh_check(seed: u64, rule: &Rule, n: u64) -> Vec<(Kind, u64, u64)> {
    let world = world_for(&genesis(seed, 3000));
    let (d, k) = super::super::discover::sig_split(rule.sig);
    let mut out = vec![];
    if d != Domain::Pattern {
        return out;
    }
    for kind in Kind::options_ext2(Domain::Pattern, k, false) {
        let (mut ap, mut ok) = (0u64, 0u64);
        for i in 0..n {
            let t = world.task_of_kind(30_000_000 + i, kind);
            if let Some(v) = super::super::discover::predict_x(rule.xf, &rule.form, &t.view.params) {
                ap += 1;
                ok += t.check(v) as u64;
            }
        }
        out.push((kind, ap, ok));
    }
    out
}
