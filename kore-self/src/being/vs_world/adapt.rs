//! Adapters: a world-independent `Agent` (core.rs) as a chooser of each world, seeing exactly what the
//! being's own planner sees there (the offers' visible features and the log-derived state), and learning
//! from what the being is told (the reported reward, read as the change of the told totals between two
//! decisions). Only the fixed strategies (fast, careful, deep) are offered to a method: `discovered` is
//! the being's own rule book, which no method here has.
//!
//! * World v2 and World O: `Chooser2` over `State2` / `Offer2` (World O re-uses a family slot for a new
//!   family; the being's state then clears that slot's attempt counts, and the adapter tells the method).
//! * World L: `ChooserL` over `StateL` / `OfferL` (visible state: token held, previous family and result).
//! * The first menu world (Tracks B1/B2): `MenuChooser` over `BeingState` / `Offer`.

use super::core::{Agent, Ctx, Opt};
use crate::being::lookahead::beingl::{ActL, ChoiceL, ChooserL, StateL};
use crate::being::lookahead::worldl::OfferL;
use crate::being::planner::{Action, Choice, MenuChooser, Offer};
use crate::being::solver::{Strategy, STRATEGIES};
use crate::being::state::BeingState;
use crate::being::world2::engine::State2;
use crate::being::world2::learner::{plain, Act2, Choice2, Chooser2, Offer2};
use serde_json::json;

const SKIP: i64 = 1;

struct Pending {
    cx: Ctx,
    opt: Opt,
    reward0: i64,
    cost0: i64,
}

/// A method playing World v2 or World O.
pub struct Player2 {
    pub agent: Box<dyn Agent>,
    pending: Option<Pending>,
    /// Attempts per family slot as the player counted them (World O slot re-use detection).
    counts: Vec<u64>,
    pub decisions: u64,
}

impl Player2 {
    pub fn new(agent: Box<dyn Agent>) -> Player2 {
        Player2 { agent, pending: None, counts: vec![], decisions: 0 }
    }
}

pub fn ctx2(st: &State2) -> Ctx {
    Ctx { step: st.step, budget_left: st.budget_left, told_reward: st.total_reward as f64, spent: st.total_cost as f64, state: 0, token: false }
}

/// The options a method may take in World v2 / World O: every affordable (offer, fixed strategy), then the skip.
pub fn opts2(offers: &[Offer2], budget_left: i64) -> Vec<Opt> {
    let mut v = vec![];
    for o in offers {
        for s in STRATEGIES {
            let e = s.energy(o.listed_cost);
            if e <= budget_left {
                v.push(Opt { slot: o.slot, strategy: Some(s), fam: o.fam, ftype: o.ftype as u8, diff: o.difficulty, listed_reward: o.listed_reward as f64, energy: e as f64, needs_token: false });
            }
        }
    }
    if budget_left >= SKIP {
        v.push(Opt::skip(SKIP as f64));
    }
    v
}

impl Chooser2 for Player2 {
    fn name(&self) -> String {
        self.agent.name()
    }
    fn choose(&mut self, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
        let cx = ctx2(st);
        if let Some(p) = self.pending.take() {
            self.agent.observe(&p.cx, &p.opt, (st.total_reward - p.reward0) as f64, (st.total_cost - p.cost0) as f64, &cx);
        }
        // World O: a slot whose attempt counts went back (the family closed) is a new family
        for (fam, a) in st.arms.iter().enumerate() {
            let n: u64 = a.iter().map(|x| x.n).sum();
            if fam >= self.counts.len() {
                self.counts.resize(fam + 1, 0);
            }
            if n < self.counts[fam] {
                self.agent.reset_fam(fam);
            }
            self.counts[fam] = n;
        }
        let opts = opts2(offers, st.budget_left);
        if opts.is_empty() {
            return None;
        }
        let i = self.agent.choose(&cx, &opts);
        let o = opts[i];
        self.decisions += 1;
        self.pending = Some(Pending { cx, opt: o, reward0: st.total_reward, cost0: st.total_cost });
        let name = self.agent.name();
        Some(match o.strategy {
            None => plain(&name, "standard method: skip", Act2::Skip, 0.0, 0.0),
            Some(s) => plain(&name, "standard method", Act2::Attempt { slot: o.slot, strategy: s }, 0.0, 0.0),
        })
    }
}

// ---- World L ----

pub struct PlayerL {
    pub agent: Box<dyn Agent>,
    pending: Option<Pending>,
}

impl PlayerL {
    pub fn new(agent: Box<dyn Agent>) -> PlayerL {
        PlayerL { agent, pending: None }
    }
}

pub fn ctxl(st: &StateL) -> Ctx {
    Ctx { step: st.step, budget_left: st.budget_left, told_reward: st.total_reward as f64, spent: st.total_cost as f64, state: 1 + st.token as u32 * 1000 + st.ctx as u32, token: st.token > 0 }
}

pub fn optsl(offers: &[OfferL], st: &StateL) -> Vec<Opt> {
    let mut v = vec![];
    for o in offers {
        if o.needs_token && st.token == 0 {
            continue;
        }
        for s in STRATEGIES {
            let e = s.energy(o.cost);
            if e <= st.budget_left {
                v.push(Opt { slot: o.slot, strategy: Some(s), fam: o.fam, ftype: 0, diff: 1, listed_reward: o.reward as f64, energy: e as f64, needs_token: o.needs_token });
            }
        }
    }
    if st.budget_left >= SKIP {
        v.push(Opt::skip(SKIP as f64));
    }
    v
}

impl ChooserL for PlayerL {
    fn name(&self) -> String {
        self.agent.name()
    }
    fn choose(&mut self, st: &StateL, offers: &[OfferL]) -> Option<ChoiceL> {
        let cx = ctxl(st);
        if let Some(p) = self.pending.take() {
            self.agent.observe(&p.cx, &p.opt, (st.total_reward - p.reward0) as f64, (st.total_cost - p.cost0) as f64, &cx);
        }
        let opts = optsl(offers, st);
        if opts.is_empty() {
            return None;
        }
        let i = self.agent.choose(&cx, &opts);
        let o = opts[i];
        self.pending = Some(Pending { cx, opt: o, reward0: st.total_reward, cost0: st.total_cost });
        let act = match o.strategy {
            None => ActL::Skip,
            Some(s) => ActL::Attempt { slot: o.slot, strategy: s },
        };
        Some(ChoiceL { act, policy: self.agent.name(), why: "standard method".into(), p: 0.0, score: 0.0, rationale: json!({ "policy": self.agent.name() }), nodes: 0 })
    }
}

// ---- the first menu world (B1 / B2) ----

pub struct PlayerM {
    pub agent: Box<dyn Agent>,
    pending: Option<Pending>,
}

impl PlayerM {
    pub fn new(agent: Box<dyn Agent>) -> PlayerM {
        PlayerM { agent, pending: None }
    }
}

pub fn ctxm(st: &BeingState) -> Ctx {
    Ctx { step: st.step, budget_left: st.budget_left, told_reward: st.total_reward as f64, spent: st.total_cost as f64, state: 0, token: false }
}

pub fn optsm(offers: &[Offer], budget_left: i64) -> Vec<Opt> {
    let mut v = vec![];
    for o in offers {
        for s in STRATEGIES {
            let e = s.energy(o.listed_cost);
            if e <= budget_left {
                let d = o.domain.idx();
                v.push(Opt { slot: o.slot, strategy: Some(s), fam: d, ftype: d as u8, diff: o.difficulty, listed_reward: o.listed_reward as f64, energy: e as f64, needs_token: false });
            }
        }
    }
    if budget_left >= SKIP {
        v.push(Opt::skip(SKIP as f64));
    }
    v
}

impl MenuChooser for PlayerM {
    fn name(&self) -> String {
        self.agent.name()
    }
    fn choose(&mut self, st: &BeingState, offers: &[Offer]) -> Option<Choice> {
        let cx = ctxm(st);
        if let Some(p) = self.pending.take() {
            self.agent.observe(&p.cx, &p.opt, (st.total_reward - p.reward0) as f64, (st.total_cost - p.cost0) as f64, &cx);
        }
        let opts = optsm(offers, st.budget_left);
        if opts.is_empty() {
            return None;
        }
        let i = self.agent.choose(&cx, &opts);
        let o = opts[i];
        self.pending = Some(Pending { cx, opt: o, reward0: st.total_reward, cost0: st.total_cost });
        let action = match o.strategy {
            None => Action::Skip,
            Some(s) => Action::Attempt { slot: o.slot, strategy: s },
        };
        Some(Choice { action, policy: self.agent.name(), why: "standard method".into(), p: 0.0, sd: 0.0, cost_est: 0.0, score: 0.0, rationale: json!({}), advice: None })
    }
}

/// The strategies a method may use (for the tests).
pub fn method_strategies() -> [Strategy; 3] {
    STRATEGIES
}
