//! Track B14 measurement (synthetic first world, author-defined families and rewards).
//!
//! An in-process copy of the long run's being (`measure_b11::genesis`: extended2 families, chooser
//! `model`, discovery every 10 steps, composition and pruning under the long-run controller, budget
//! 3000 with refills), stepped through the normal `Engine::step_menu`, with the B14 switches
//! (rule library, curiosity) on or off, and a measurement side channel that is never logged and never
//! read by the being:
//!
//! * the reference rate: a renewal oracle (knows the hidden success rates and real energy) that holds
//!   an oracle rule book (`oracle_book`: the search run by the author on many sampled tasks of every
//!   hidden family); the regret of a window is that rate minus the window's reward per energy;
//! * per window: reward per energy, attempts and skips, rules accepted / retired / live, rule
//!   answers, B14 curiosity steps and their energy, and the coverage of the being's rule book on
//!   fresh tasks of every expressible hidden family.
//!
//! Measurement only.

use super::super::controller::Controller;
use super::super::discover::{self, Example, Found, Rule};
use super::super::measure::{coverage, run_menu_ext, truth_in, Truth};
use super::super::measure_b11::{genesis, renewal_best, LONGRUN_CONTROLLER};
use super::super::planner::{Action, Choice, MenuChooser, ModelChooser, Offer, SkipRule};
use super::super::run_loop::{world_for, Engine};
use super::super::solver::{ScriptedThinker, STRATEGIES_EXT};
use super::super::state::BeingState;
use super::super::world::{Domain, Kind, World, SKIP_COST};
use super::curiosity::{CurCfg, Curious};
use super::library::{LibCfg, LibEntry};

/// Hidden families of the extended2 world the grammar plus composition can express (no negative
/// control, no drift variant): what the oracle book and the coverage are computed over.
pub const FAMILIES: [Kind; 14] = [
    Kind::PatArith,
    Kind::PatGeom,
    Kind::ExtAffGeom,
    Kind::Ext2SplitQuad,
    Kind::PatQuad,
    Kind::ExtPoly3,
    Kind::Ext2ParityCubic,
    Kind::PatFib,
    Kind::ExtRec2,
    Kind::ExtTribo,
    Kind::Ext2CubeGeom,
    Kind::ExtAlt,
    Kind::ExtPeriodic,
    Kind::Ext2AltCube,
];

const BOOK_OFFSET: u64 = 20_000_000;

/// The oracle's rule book for the extended2 world: the being's own search (plain grammar, then
/// composition over the plain forms found) run on `n` sampled tasks of every hidden family. The
/// families do not depend on the world seed; the tasks do.
pub fn oracle_book(world: &World, n: u64) -> Vec<Rule> {
    let mut ex: Vec<Example> = vec![];
    for i in 0..n {
        for &k in FAMILIES.iter() {
            let t = world.task_of_kind(BOOK_OFFSET + i, k);
            ex.push(Example { event_id: format!("book-{i}-{}", k as u64), sig: t.view.difficulty, terms: t.view.params.clone(), next: t.expected() });
        }
    }
    let plain = discover::search(&ex);
    let mut blocks = vec![];
    for f in &plain {
        if !blocks.contains(&f.form) {
            blocks.push(f.form);
        }
    }
    let all = discover::search_composed(&ex, &blocks, usize::MAX).found;
    let to_rule = |f: &Found| Rule::new_x(f.sig, f.xf, f.form, f.train_ok, f.val_ok, 0, blocks.len() as u32);
    all.iter().map(to_rule).collect()
}

/// The renewal oracle's rate with a given rule book (Dinkelbach iterations, as `measure_b11::renewal_oracle`).
pub fn renewal_rate(world_seed: u64, budget: i64, book: &[Rule], iters: usize) -> f64 {
    struct O {
        t: Truth,
        rho: f64,
    }
    impl MenuChooser for O {
        fn name(&self) -> String {
            "oracle-renewal-book".into()
        }
        fn choose(&mut self, st: &BeingState, offers: &[Offer]) -> Option<Choice> {
            let (v, slot, strategy) = renewal_best(&self.t, &STRATEGIES_EXT, offers, st.budget_left, self.rho)?;
            let action = if v < -self.rho * SKIP_COST as f64 && st.budget_left >= SKIP_COST { Action::Skip } else { Action::Attempt { slot, strategy } };
            Some(Choice { action, policy: "oracle".into(), why: String::new(), p: 0.0, sd: 0.0, cost_est: 0.0, score: v, rationale: serde_json::json!({}), advice: None })
        }
    }
    let g = genesis(world_seed, budget);
    let t = truth_in(&world_for(&g), book);
    let mut rho = 3.0;
    let mut rate = 0.0;
    for _ in 0..iters.max(1) {
        let r = run_menu_ext(world_for(&g), budget, &mut O { t: Truth { p: t.p }, rho }, 0, book);
        rate = r.run.per_budget();
        rho = rate;
    }
    rate
}

#[derive(Debug, Clone)]
pub struct RunCfgB14 {
    pub world_seed: u64,
    pub steps: u64,
    pub window: u64,
    pub budget: i64,
    pub skip_rule: SkipRule,
    pub discover_every: u64,
    pub curiosity: Option<CurCfg>,
    pub library: Option<LibCfg>,
    /// Library entries carried over from an earlier life (transfer); empty = none.
    pub import: Vec<LibEntry>,
    /// The extended2 rule-drift world (one family's rule changes at half the first budget).
    pub rule_drift: bool,
    /// Rules in the state from the start (the "already solved" condition: the oracle book).
    pub preload: Vec<Rule>,
    /// Refill the budget when it runs out (the long run); false: one budget, the life ends.
    pub refill: bool,
    /// Compute the coverage of the rule book at every window end (fresh tasks per family).
    pub cover_n: u64,
}

impl RunCfgB14 {
    pub fn base(world_seed: u64, steps: u64, window: u64) -> RunCfgB14 {
        RunCfgB14 { world_seed, steps, window, budget: 3000, skip_rule: SkipRule::Rate, discover_every: 10, curiosity: None, library: None, import: vec![], rule_drift: false, preload: vec![], refill: true, cover_n: 0 }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WinB14 {
    pub from: u64,
    pub steps: u64,
    pub reward: i64,
    pub cost: i64,
    pub attempts: u64,
    pub skips: u64,
    pub pattern_attempts: u64,
    pub rule_answers: u64,
    pub rule_right: u64,
    pub accepted: u64,
    pub retired: u64,
    pub live_end: usize,
    pub cur_steps: u64,
    pub cur_energy: i64,
    pub lib_reused: u64,
    /// Coverage per family of `FAMILIES` at the end of the window (empty unless `cover_n > 0`).
    pub cover: Vec<f64>,
}

impl WinB14 {
    pub fn rpe(&self) -> f64 {
        if self.cost > 0 { self.reward as f64 / self.cost as f64 } else { 0.0 }
    }
}

#[derive(Debug, Clone, Default)]
pub struct OutB14 {
    pub wins: Vec<WinB14>,
    /// Step (count) at which each rule was accepted: (step, sig, rule id, via library).
    pub accepted: Vec<(u64, u8, String, bool)>,
    /// Library entries at the end of the life (for transfer into the next one).
    pub library: Vec<LibEntry>,
    pub cur_energy: i64,
    pub total_cost: i64,
    pub rules_final: Vec<Rule>,
    /// Revealed examples per difficulty (index 1..5) at the end.
    pub ex_per_sig: [u64; 6],
    /// Pattern attempts per (difficulty, strategy) over the life.
    pub pat_strat: [[u64; 4]; 6],
}

/// One life in-process. The being under test is the normal engine; the B14 switches are those of `cfg`.
pub fn run_b14(cfg: &RunCfgB14) -> OutB14 {
    let mut g = genesis(cfg.world_seed, cfg.budget);
    g.rule_drift = cfg.rule_drift;
    let mut st = BeingState::default();
    st.start(g.clone(), String::new());
    for r in &cfg.preload {
        let mut r = r.clone();
        r.event_id = format!("preload-{}", r.id);
        st.apply_rule(r);
    }
    let (limits, _) = Controller::parse(LONGRUN_CONTROLLER).expect("controller").limits(true, true);
    let th = ScriptedThinker;
    let world = world_for(&g);
    let mut ch = Curious::new(ModelChooser { skip_rule: cfg.skip_rule, ..ModelChooser::for_genesis(&g) }, &world, cfg.library, cfg.curiosity);
    let mut e = Engine::new(world, st, &th).with_limits(Some(limits));
    if let Some(lc) = &cfg.library {
        e.enable_library(*lc, &cfg.import);
    }
    let mut out = OutB14::default();
    let mut w = WinB14::default();
    let mut n = 0u64;
    let fams: Vec<Kind> = FAMILIES.to_vec();
    while n < cfg.steps {
        let res = e.step_menu(&mut ch);
        let o = match res {
            Ok(o) => o,
            Err(super::super::StopReason::BudgetExhausted) if cfg.refill => {
                let c = e.state.clock;
                e.state.apply_refill(g.budget, c + 86_400);
                continue;
            }
            Err(_) => break,
        };
        n += 1;
        let before = e.state.rules.len();
        let retired_before = e.state.rules.iter().filter(|r| r.retired).count();
        if cfg.discover_every > 0 && e.state.step % cfg.discover_every == 0 {
            for nr in e.discover_pass() {
                out.accepted.push((n, nr.rule.sig, nr.rule.id.clone(), false));
            }
            for nr in e.library_pass() {
                out.accepted.push((n, nr.rule.sig, nr.rule.id.clone(), true));
                w.lib_reused += 1;
            }
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
            if a.domain == Domain::Pattern {
                w.pattern_attempts += 1;
                if let Some(s) = a.strategy {
                    out.pat_strat[(a.difficulty as usize).min(5)][s.idx()] += 1;
                }
            }
            if a.rule_used.is_some() {
                w.rule_answers += 1;
                w.rule_right += a.correct as u64;
            }
        }
        if ch.last_was_curious() {
            w.cur_steps += 1;
            w.cur_energy += a.cost;
        }
        if n % cfg.window == 0 {
            w.from = n - cfg.window;
            w.live_end = e.state.live_rules();
            if cfg.cover_n > 0 {
                w.cover = fams.iter().map(|&k| coverage(&e.world, &e.state.rules, k, cfg.cover_n)).collect();
            }
            out.wins.push(std::mem::take(&mut w));
        }
    }
    if w.steps > 0 {
        w.from = n - w.steps;
        w.live_end = e.state.live_rules();
        out.wins.push(w);
    }
    out.cur_energy = e.state.b14_cur_energy;
    out.total_cost = e.state.total_cost;
    out.library = e.library_entries();
    out.rules_final = e.state.rules.clone();
    for x in &e.state.examples {
        out.ex_per_sig[(x.sig as usize).min(5)] += 1;
    }
    out
}
