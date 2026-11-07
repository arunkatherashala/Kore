//! The lookahead planner on the existing worlds (controls of Track B8): the first menu world
//! (`planner::MenuChooser`, model `model.rs`) and World v2 (`world2::learner::Chooser2`, model
//! `world2/model.rs`). Off (`cfg: None`) both are the existing B1 planner, called as is, so every
//! decision, rationale and number is the existing one; a test compares them decision by decision.
//!
//! On, the planner reads the existing model's estimates (posterior mean, sd, energy ratio of every
//! cell, turned into Beta pseudo-counts by `Base::from_mean_sd`) and samples future menus from the
//! menus this chooser has been shown in the current life (kept here, oldest first; they can be rebuilt
//! from the logged world config, which makes the offers a pure function of step and clock). No token
//! and no context exist in these worlds. Advice (B4) is not used by the lookahead planner.

use super::plan::{self, AOffer, Base, Key, LookCfg, Plan, Sim, Source};
use crate::being::controller::Refusal;
use crate::being::planner::{Action, Choice, MenuChooser, ModelChooser, Offer, SKIP_MIN_COST};
use crate::being::solver::{Strategy, STRATEGIES, STRATEGIES_EXT};
use crate::being::state::BeingState;
use crate::being::world::ALL_DOMAINS_X;
use crate::being::world2::engine::State2;
use crate::being::world2::learner::{Act2, Choice2, Chooser2, Learner2, Offer2};
use serde_json::json;
use std::collections::VecDeque;

fn strat(i: u8) -> Strategy {
    STRATEGIES_EXT[(i as usize).min(3)]
}

fn cell_name(k: &Key) -> String {
    format!("f{} d{} {}", k.fam, k.diff, strat(k.strat).name())
}

/// Memory of shown menus with a cap (oldest dropped).
#[derive(Debug, Clone, Default)]
pub struct Memory {
    pub menus: VecDeque<(u64, Vec<AOffer>)>,
}

impl Memory {
    fn note(&mut self, step: u64, m: Vec<AOffer>, cap: usize) {
        if self.menus.back().map_or(false, |x| x.0 == step) {
            return;
        }
        self.menus.push_back((step, m));
        while self.menus.len() > cap {
            self.menus.pop_front();
        }
    }
    fn before(&self, step: u64) -> Vec<(u64, Vec<AOffer>)> {
        self.menus.iter().filter(|m| m.0 < step).cloned().collect()
    }
}

/// Counters of one life.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LookStats {
    pub decisions: u64,
    pub nodes: u64,
    pub max_nodes: u64,
    pub truncations: u64,
    pub fallbacks: u64,
}

impl LookStats {
    fn note(&mut self, p: &Plan) {
        self.decisions += 1;
        self.nodes += p.nodes;
        self.max_nodes = self.max_nodes.max(p.nodes);
        self.truncations += p.truncated as u64;
    }
}

// ---- World v2 ----

struct Src2<'a> {
    st: &'a State2,
}

impl<'a> Source for Src2<'a> {
    fn base(&self, k: Key) -> Base {
        let e = self.st.model.estimate(k.fam as usize, k.diff, strat(k.strat));
        Base::from_mean_sd(e.p, e.sd, e.cost_ratio, e.n)
    }
    fn strategies(&self) -> &[Strategy] {
        &STRATEGIES_EXT
    }
}

pub struct Look2 {
    pub cfg: Option<LookCfg>,
    pub inner: Learner2,
    pub memory: Memory,
    pub stats: LookStats,
    refusals: Vec<Refusal>,
}

impl Look2 {
    pub fn new(cfg: Option<LookCfg>) -> Look2 {
        Look2 { cfg, inner: Learner2::default(), memory: Memory::default(), stats: LookStats::default(), refusals: vec![] }
    }
    pub fn take_refusals(&mut self) -> Vec<Refusal> {
        std::mem::take(&mut self.refusals)
    }
}

impl Chooser2 for Look2 {
    fn name(&self) -> String {
        match &self.cfg {
            Some(c) => format!("w2-lookahead-d{}", c.depth),
            None => self.inner.name(),
        }
    }
    fn choose(&mut self, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
        let Some(cfg) = self.cfg else { return self.inner.choose(st, offers) };
        let menu: Vec<AOffer> = offers.iter().map(|o| AOffer { slot: o.slot, fam: o.fam, diff: o.difficulty, cost: o.listed_cost, reward: o.listed_reward, needs_token: false }).collect();
        let mem = self.memory.before(st.step);
        self.memory.note(st.step, menu.clone(), cfg.memory);
        let rho0 = if st.total_cost >= SKIP_MIN_COST { st.total_reward as f64 / st.total_cost as f64 } else { 0.0 };
        let Some(p) = plan::plan(&Src2 { st }, &cfg, &menu, &mem, &Sim::new(st.budget_left, 0, 0), st.step, rho0) else {
            self.stats.fallbacks += 1;
            return self.inner.choose(st, offers);
        };
        self.stats.note(&p);
        if p.truncated {
            self.refusals.push(super::ctl::node_cap_refusal(cfg.max_nodes, cfg.depth));
        }
        let mut rationale = p.to_json(&cfg, &cell_name);
        rationale["policy"] = json!(self.name());
        let cands = self.inner.candidates(st, offers);
        let scores: Vec<(usize, Strategy, f64, bool)> = cands.iter().map(|c| (c.slot, c.strategy, c.score, c.affordable)).collect();
        let rate = if st.total_cost > 0 { st.total_reward as f64 / st.total_cost as f64 } else { 0.0 };
        let sf = self.inner.skip_frac;
        let skip_ok = |h: f64| sf > 0.0 && st.total_cost >= SKIP_MIN_COST && h < sf * rate && st.budget_left >= crate::being::world2::world::SKIP_COST;
        let Some((dec, score, add)) = plan::decide(&p, &cfg, &scores, &skip_ok) else { return self.inner.choose(st, offers) };
        rationale["decided"] = json!({ "score": score, "future_term_per_energy": add });
        let ch = p.alts.iter().find(|a| a.act == dec).cloned().unwrap_or_else(|| p.chosen().clone());
        let act = match dec {
            Some((slot, strategy)) => Act2::Attempt { slot, strategy },
            None => Act2::Skip,
        };
        let why = format!("lookahead ({:?}) depth {} of {} ({} nodes): score {:.3}, future term {:+.3} per energy, simulated value {:+.3} over the leaf, learned rate {:.3}", cfg.mode, p.depth_done, p.depth_wanted, p.nodes, score, add, ch.q, p.rho);
        // the model's own estimate of the chosen option (exact, for the prediction it is logged with)
        let (pp, sd, ce) = match dec.and_then(|(sl, s)| cands.iter().find(|c| c.slot == sl && c.strategy == s)) {
            Some(c) => (c.p, c.sd, c.cost_est),
            None if dec.is_none() => (0.0, 0.0, crate::being::world2::world::SKIP_COST as f64),
            None => (ch.p, ch.sd, ch.energy),
        };
        let score = if dec.is_none() && cfg.mode == plan::Mode::Hybrid { 0.0 } else { score };
        Some(Choice2 { act, policy: self.name(), why, p: pp, sd, cost_est: ce, score, rationale })
    }
}

// ---- the first menu world ----

struct Src1<'a> {
    st: &'a BeingState,
    strategies: &'static [Strategy],
}

impl<'a> Source for Src1<'a> {
    fn base(&self, k: Key) -> Base {
        let e = self.st.model.estimate(ALL_DOMAINS_X[(k.fam as usize).min(ALL_DOMAINS_X.len() - 1)], k.diff, strat(k.strat));
        Base::from_mean_sd(e.p, e.sd, e.cost_ratio, e.n)
    }
    fn strategies(&self) -> &[Strategy] {
        self.strategies
    }
}

pub struct LookMenu {
    pub cfg: Option<LookCfg>,
    pub inner: ModelChooser,
    pub memory: Memory,
    pub stats: LookStats,
}

impl LookMenu {
    pub fn new(cfg: Option<LookCfg>, inner: ModelChooser) -> LookMenu {
        LookMenu { cfg, inner, memory: Memory::default(), stats: LookStats::default() }
    }
}

impl MenuChooser for LookMenu {
    fn name(&self) -> String {
        match &self.cfg {
            Some(c) => format!("lookahead-d{}", c.depth),
            None => self.inner.name(),
        }
    }
    fn choose(&mut self, st: &BeingState, offers: &[Offer]) -> Option<Choice> {
        let Some(cfg) = self.cfg else { return self.inner.choose(st, offers) };
        let strategies: &'static [Strategy] = if !self.inner.no_discovered && st.genesis.as_ref().map_or(false, |g| g.extended) { &STRATEGIES_EXT } else { &STRATEGIES };
        let menu: Vec<AOffer> = offers.iter().map(|o| AOffer { slot: o.slot, fam: o.domain.idx(), diff: o.difficulty, cost: o.listed_cost, reward: o.listed_reward, needs_token: false }).collect();
        let mem = self.memory.before(st.step);
        self.memory.note(st.step, menu.clone(), cfg.memory);
        let rho0 = if st.total_cost >= SKIP_MIN_COST { st.total_reward as f64 / st.total_cost as f64 } else { 0.0 };
        let Some(p) = plan::plan(&Src1 { st, strategies }, &cfg, &menu, &mem, &Sim::new(st.budget_left, 0, 0), st.step, rho0) else {
            self.stats.fallbacks += 1;
            return self.inner.choose(st, offers);
        };
        self.stats.note(&p);
        let names = |k: &Key| format!("{} d{} {}", ALL_DOMAINS_X[(k.fam as usize).min(ALL_DOMAINS_X.len() - 1)].name(), k.diff, strat(k.strat).name());
        let mut rationale = p.to_json(&cfg, &names);
        rationale["policy"] = json!(self.name());
        let cands = self.inner.candidates(st, offers);
        let scores: Vec<(usize, Strategy, f64, bool)> = cands.iter().map(|c| (c.slot, c.strategy, c.score, c.affordable)).collect();
        let rate = match st.model.recent_rate() {
            Some(r) => r,
            None => if st.total_cost > 0 { st.total_reward as f64 / st.total_cost as f64 } else { 0.0 },
        };
        let sf = self.inner.skip_frac;
        let skip_ok = |h: f64| sf > 0.0 && st.total_cost >= SKIP_MIN_COST && h < sf * rate && st.budget_left >= crate::being::world::SKIP_COST;
        let Some((dec, score, add)) = plan::decide(&p, &cfg, &scores, &skip_ok) else { return self.inner.choose(st, offers) };
        rationale["decided"] = json!({ "score": score, "future_term_per_energy": add });
        let ch = p.alts.iter().find(|a| a.act == dec).cloned().unwrap_or_else(|| p.chosen().clone());
        let action = match dec {
            Some((slot, strategy)) => Action::Attempt { slot, strategy },
            None => Action::Skip,
        };
        let why = format!("lookahead ({:?}) depth {} of {} ({} nodes): score {:.3}, future term {:+.3} per energy, simulated value {:+.3} over the leaf, learned rate {:.3}", cfg.mode, p.depth_done, p.depth_wanted, p.nodes, score, add, ch.q, p.rho);
        let (pp, sd, ce) = match dec.and_then(|(sl, s)| cands.iter().find(|c| c.slot == sl && c.strategy == s)) {
            Some(c) => (c.p, c.sd, c.cost_est),
            None if dec.is_none() => (0.0, 0.0, crate::being::world::SKIP_COST as f64),
            None => (ch.p, ch.sd, ch.energy),
        };
        let score = if dec.is_none() && cfg.mode == plan::Mode::Hybrid { 0.0 } else { score };
        Some(Choice { action, policy: self.name(), why, p: pp, sd, cost_est: ce, score, rationale, advice: None })
    }
}
