//! Track B9 measurement: experiments chosen by expected information gain vs passive discovery and vs
//! random probes with the same budget, in World v2 and in the B2 (extended) and B5 (extended2) menu
//! worlds. In-memory runs of the same engines as the logged loops. Hidden truth is used ONLY here, to
//! score: which signatures have a rule inside the search space, whether an accepted rule is right on
//! fresh tasks, and the oracles.

use super::controller::Limits;
use super::discover::{self, Rule};
use super::experiment::{ExpBook, ExpCfg};
use super::experiment_menu::Exp1;
use super::measure::{self, coverage, OracleMenu, COVER_MIN, COVER_N, EXPRESSIBLE};
use super::planner::{MenuChooser, ModelChooser, DEFAULT_BETA, DEFAULT_CURIOSITY};
use super::run_loop::Engine;
use super::solver::{ScriptedThinker, STRATEGIES_EXT};
use super::state::{BeingState, Chooser, Genesis};
use super::world::{Domain, Kind, World, SECS_PER_ENERGY};
use super::world2::baselines::Oracle2;
use super::world2::experiment::Exp2;
use super::world2::learner::Learner2;
use super::world2::measure::{rule_on_fresh, run_w2, W2Run};
use super::world2::solve::sig2;
use super::world2::world::{FType, W2Cfg, World2};

pub const EVERY: u64 = 10;
/// Experiment budget (per mille of the life budget) used in the report. Chosen on tuning seeds 101..120 only.
pub const DEFAULT_CAP_PM: u32 = 50;
const FRESH_OFFSET: u64 = 30_000_000;

/// How fast and how well rules were learned in one life (scored against hidden truth).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Speed {
    /// Targets: (signature or family) that a rule of the search space can express.
    pub targets: usize,
    /// Targets covered by a right rule by the end of the life.
    pub found: usize,
    /// Mean over targets of the share of the life's energy spent before the target was covered (1.0 if never).
    pub energy_frac: f64,
    /// Mean over targets of the step count at coverage (the life's step count if never).
    pub steps: f64,
    pub rules: usize,
    /// Accepted rules that were applicable-and-wrong on fresh tasks of their signature.
    pub wrong: usize,
}

#[derive(Debug, Clone)]
pub struct Arm {
    pub name: String,
    pub reward: i64,
    pub cost: i64,
    pub steps: u64,
    pub speed: Speed,
    pub exp: ExpBook,
    /// Experiments whose revealed answer rejected at least one live hypothesis.
    pub exp_rejecting: u64,
}

impl Arm {
    pub fn per_energy(&self) -> f64 {
        if self.cost > 0 { self.reward as f64 / self.cost as f64 } else { 0.0 }
    }
    pub fn exp_share(&self) -> f64 {
        if self.cost > 0 { self.exp.energy as f64 / self.cost as f64 } else { 0.0 }
    }
}

// ---- World v2 ----

/// Signatures (family, difficulty) of sequence families whose rule (at the start of the life) the grammar expresses.
pub fn w2_targets(w: &World2) -> Vec<u8> {
    let mut v = vec![];
    for f in 0..w.cfg.families {
        if w.fams[f].ftype == FType::Seq && w.class_of(f, 0).in_grammar() {
            for d in 1..=5u8 {
                v.push(sig2(f, d));
            }
        }
    }
    v
}

pub fn w2_speed(cfg: &W2Cfg, run: &W2Run) -> Speed {
    let w = World2::new(cfg.clone());
    let targets = w2_targets(&w);
    let mut first: Vec<Option<(i64, u64)>> = vec![None; targets.len()];
    for rc in &run.rules {
        if let Some(i) = targets.iter().position(|s| *s == rc.rule.sig) {
            if first[i].is_none() {
                let (app, ok) = rule_on_fresh(&w, &rc.rule, rc.clock, 50);
                if app > 0 && ok * 10 >= 50 * 9 {
                    first[i] = Some((rc.clock / SECS_PER_ENERGY, rc.rule.step));
                }
            }
        }
    }
    let n = targets.len().max(1) as f64;
    let budget = cfg.budget.max(1) as f64;
    Speed {
        targets: targets.len(),
        found: first.iter().filter(|x| x.is_some()).count(),
        energy_frac: first.iter().map(|x| x.map_or(1.0, |(e, _)| (e as f64 / budget).min(1.0))).sum::<f64>() / n,
        steps: first.iter().map(|x| x.map_or(run.steps as f64, |(_, s)| s as f64)).sum::<f64>() / n,
        rules: run.rules.len(),
        wrong: run.rules.iter().filter(|r| r.wrong_at_accept).count(),
    }
}

fn w2_arm(name: &str, cfg: &W2Cfg, run: W2Run, exp: ExpBook, rejecting: u64) -> Arm {
    let speed = w2_speed(cfg, &run);
    Arm { name: name.into(), reward: run.true_reward, cost: run.cost, steps: run.steps, speed, exp, exp_rejecting: rejecting }
}

/// The learner with experiments (or the random-probe control) in World v2.
pub fn w2_exp_run(cfg: &W2Cfg, exp: ExpCfg, limits: Option<Limits>) -> (W2Run, ExpBook, u64) {
    let mut ch = Exp2::new(Learner2::default(), exp, World2::new(cfg.clone()), cfg.budget);
    ch.compose_blocks = limits.as_ref().map_or(false, |l| l.compose);
    let run = run_w2(cfg, &mut ch, EVERY, limits);
    // an experiment "rejected" something if its revealed example was wrong for a hypothesis live just before it
    let exps: Vec<(u8, Vec<i64>)> = ch.recs.iter().map(|r| (r.pick.sig, r.pick.prefix.clone())).collect();
    let rejecting = super::experiment::count_rejecting(&run.state.examples, &exps);
    (run, ch.book.clone(), rejecting)
}

/// All arms of one World v2 world. `cap`: experiment budget (per mille of the life), `kappa`.
pub fn w2_world(cfg: &W2Cfg, kappa: f64, cap_pm: u32, max_n: u64, with_random: bool) -> Vec<Arm> {
    let passive = run_w2(cfg, &mut Learner2::default(), EVERY, None);
    let mut out = vec![w2_arm("passive", cfg, passive, ExpBook::default(), 0)];
    let (run, book, rej) = w2_exp_run(cfg, ExpCfg::eig(kappa, cap_pm, max_n), None);
    let matched_pm = if run.cost > 0 { ((book.energy * 1000 + run.cost - 1) / run.cost).clamp(0, 1000) as u32 } else { 0 };
    out.push(w2_arm("experiments", cfg, run, book, rej));
    if with_random {
        let (run, book, rej) = w2_exp_run(cfg, ExpCfg::random(cfg.seed ^ 0xB9, matched_pm, max_n), None);
        out.push(w2_arm("random-matched", cfg, run, book, rej));
        let (run, book, rej) = w2_exp_run(cfg, ExpCfg::random(cfg.seed ^ 0xB9, cap_pm, max_n), None);
        out.push(w2_arm("random-cap", cfg, run, book, rej));
        for (name, parts) in [("split-only", 1u8), ("support-only", 2u8)] {
            let (run, book, rej) = w2_exp_run(cfg, ExpCfg { parts, ..ExpCfg::eig(kappa, cap_pm, max_n) }, None);
            out.push(w2_arm(name, cfg, run, book, rej));
        }
        let orc = run_w2(cfg, &mut Oracle2::new(World2::new(cfg.clone()), true), 0, None);
        out.push(w2_arm("oracle-knows-rules", cfg, orc, ExpBook::default(), 0));
    }
    out
}

// ---- the first (menu) world: B2 extended, B5 extended2 ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuWorld {
    /// B2: `World::with_extended(seed, true)`, plain grammar.
    B2,
    /// B5: `World::with_ext2(seed, None)`, composition on.
    B5,
}

impl MenuWorld {
    pub fn world(self, seed: u64) -> World {
        match self {
            MenuWorld::B2 => World::with_extended(seed, true),
            MenuWorld::B5 => World::with_ext2(seed, None),
        }
    }
    pub fn limits(self) -> Option<Limits> {
        match self {
            MenuWorld::B2 => None,
            MenuWorld::B5 => Some(Limits { compose: true, ..Default::default() }),
        }
    }
    pub fn targets(self) -> Vec<Kind> {
        match self {
            MenuWorld::B2 => EXPRESSIBLE.to_vec(),
            MenuWorld::B5 => super::measure_b5::GRAMMAR.iter().chain(super::measure_b5::COMPOSABLE.iter()).copied().collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MenuOut {
    pub reward: i64,
    pub cost: i64,
    pub steps: u64,
    /// (step, energy spent so far, rule) at acceptance.
    pub accepted: Vec<(u64, i64, Rule)>,
    pub state: BeingState,
}

pub fn run_menu_b9(mw: MenuWorld, seed: u64, budget: i64, chooser: &mut dyn MenuChooser, every: u64, preload: &[Rule]) -> MenuOut {
    let th = ScriptedThinker;
    let mut st = BeingState::default();
    let ext2 = mw == MenuWorld::B5;
    st.start(Genesis { world_seed: seed, loop_seed: 0, budget, explore_c: 0.0, chooser: Chooser::Model, extended: true, ext2, rule_drift: false, drift: false, adapt: Default::default(), advice: Default::default(), transfer: false, learn_transfer: false }, String::new());
    for r in preload {
        st.apply_rule(r.clone());
    }
    let mut e = Engine::new(mw.world(seed), st, &th).with_limits(mw.limits());
    let mut accepted = vec![];
    let mut steps = 0;
    while e.step_menu(chooser).is_ok() {
        steps += 1;
        if every > 0 && e.state.step % every == 0 {
            for nr in e.discover_pass() {
                accepted.push((e.state.step, e.state.total_cost, nr.rule));
            }
        }
    }
    MenuOut { reward: e.state.total_reward, cost: e.state.total_cost, steps, accepted, state: e.state }
}

pub fn menu_speed(mw: MenuWorld, seed: u64, budget: i64, out: &MenuOut) -> Speed {
    let w = mw.world(seed);
    let targets = mw.targets();
    let mut first: Vec<Option<(i64, u64)>> = vec![None; targets.len()];
    let mut book: Vec<Rule> = vec![];
    let mut i = 0;
    while i < out.accepted.len() {
        let (at, en) = (out.accepted[i].0, out.accepted[i].1);
        while i < out.accepted.len() && out.accepted[i].0 == at {
            book.push(out.accepted[i].2.clone());
            i += 1;
        }
        for (k, kind) in targets.iter().enumerate() {
            if first[k].is_none() && coverage(&w, &book, *kind, COVER_N) >= COVER_MIN {
                first[k] = Some((en, at));
            }
        }
    }
    let mut wrong = 0;
    for (_, _, r) in &out.accepted {
        let (mut ok, mut bad) = (0u32, 0u32);
        for j in 0..300u64 {
            let t = w.task_forced(FRESH_OFFSET + j, Domain::Pattern, r.sig);
            match discover::predict_x(r.xf, &r.form, &t.view.params) {
                Some(v) if t.check(v) => ok += 1,
                Some(_) => bad += 1,
                None => {}
            }
        }
        if !(bad == 0 && ok > 0) {
            wrong += 1;
        }
    }
    let n = targets.len().max(1) as f64;
    Speed {
        targets: targets.len(),
        found: first.iter().filter(|x| x.is_some()).count(),
        energy_frac: first.iter().map(|x| x.map_or(1.0, |(e, _)| (e as f64 / budget as f64).min(1.0))).sum::<f64>() / n,
        steps: first.iter().map(|x| x.map_or(out.steps as f64, |(_, s)| s as f64)).sum::<f64>() / n,
        rules: out.accepted.len(),
        wrong,
    }
}

fn learner1() -> ModelChooser {
    ModelChooser { curiosity: DEFAULT_CURIOSITY, ..ModelChooser::new(DEFAULT_BETA) }
}

fn menu_arm(name: &str, mw: MenuWorld, seed: u64, budget: i64, out: MenuOut, exp: (ExpBook, u64)) -> Arm {
    let speed = menu_speed(mw, seed, budget, &out);
    Arm { name: name.into(), reward: out.reward, cost: out.cost, steps: out.steps, speed, exp: exp.0, exp_rejecting: exp.1 }
}

pub fn menu_exp_run(mw: MenuWorld, seed: u64, budget: i64, exp: ExpCfg) -> (MenuOut, (ExpBook, u64)) {
    let mut ch = Exp1::new(learner1(), exp, mw.world(seed), budget);
    ch.compose_blocks = mw == MenuWorld::B5;
    let out = run_menu_b9(mw, seed, budget, &mut ch, EVERY, &[]);
    let exps: Vec<(u8, Vec<i64>)> = ch.recs.iter().map(|r| (r.pick.sig, r.pick.prefix.clone())).collect();
    let rej = super::experiment::count_rejecting(&out.state.examples, &exps);
    (out, (ch.book.clone(), rej))
}

/// All arms of one menu world. The oracle with perfect rules exists for B2 only.
pub fn menu_world(mw: MenuWorld, seed: u64, budget: i64, kappa: f64, cap_pm: u32, max_n: u64, with_random: bool) -> Vec<Arm> {
    let passive = run_menu_b9(mw, seed, budget, &mut learner1(), EVERY, &[]);
    let mut out = vec![menu_arm("passive", mw, seed, budget, passive, (ExpBook::default(), 0))];
    let (run, book) = menu_exp_run(mw, seed, budget, ExpCfg::eig(kappa, cap_pm, max_n));
    let matched_pm = if run.cost > 0 { ((book.0.energy * 1000 + run.cost - 1) / run.cost).clamp(0, 1000) as u32 } else { 0 };
    out.push(menu_arm("experiments", mw, seed, budget, run, book));
    if with_random {
        let (run, book) = menu_exp_run(mw, seed, budget, ExpCfg::random(seed ^ 0xB9, matched_pm, max_n));
        out.push(menu_arm("random-matched", mw, seed, budget, run, book));
        let (run, book) = menu_exp_run(mw, seed, budget, ExpCfg::random(seed ^ 0xB9, cap_pm, max_n));
        out.push(menu_arm("random-cap", mw, seed, budget, run, book));
        for (name, parts) in [("split-only", 1u8), ("support-only", 2u8)] {
            let (run, book) = menu_exp_run(mw, seed, budget, ExpCfg { parts, ..ExpCfg::eig(kappa, cap_pm, max_n) });
            out.push(menu_arm(name, mw, seed, budget, run, book));
        }
        if mw == MenuWorld::B2 {
            let w = mw.world(seed);
            let perfect = measure::perfect_rules();
            let mut orc = OracleMenu { truth: measure::truth_in(&w, &perfect), strategies: STRATEGIES_EXT.to_vec() };
            let o = run_menu_b9(mw, seed, budget, &mut orc, 0, &perfect);
            out.push(Arm { name: "oracle-knows-rules".into(), reward: o.reward, cost: o.cost, steps: o.steps, speed: Speed::default(), exp: ExpBook::default(), exp_rejecting: 0 });
        }
    }
    out
}
