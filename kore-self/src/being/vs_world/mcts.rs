//! MCTS with access to the TRUE simulator: an upper-bound reference ("cheater"), NOT a method on the
//! being's footing. It knows the hidden success probability of every fixed strategy in the current
//! regime, the future menus, and (World L) the token and phase rules and the current hidden phase. It
//! does not see the world's own random draws (outcomes are sampled from the true probabilities).
//!
//! Method: UCT (Kocsis, Szepesvari 2006) over a depth-limited tree with chance nodes for success and
//! failure (the immediate term of a visit is its expected reward; the outcome is sampled to pick the
//! child state), in the opportunity-cost form of the reward-per-energy objective: an action is worth
//! `reward - rho x energy` plus what follows, with `rho` the expected reward per energy this player has
//! achieved so far (the renewal yardstick the oracles use; before 40 energy, 0.75 x the best expected
//! reward per energy of the menu). Skip = one more action (0 - rho x 1).

use super::core::Rng;
use crate::being::lookahead::beingl::{ActL, ChoiceL, ChooserL, StateL};
use crate::being::lookahead::worldl::{FKind, WorldL};
use crate::being::solver::{Strategy, STRATEGIES};
use crate::being::world::SECS_PER_ENERGY;
use crate::being::world2::baselines::Oracle2;
use crate::being::world2::engine::State2;
use crate::being::world2::learner::{plain, Act2, Choice2, Chooser2, Offer2};
use crate::being::world2::world::World2;
use serde_json::json;

pub const MCTS_ITERS: usize = 200;
pub const MCTS_DEPTH_V2: usize = 3;
pub const MCTS_DEPTH_L: usize = 4;
pub const MCTS_C: f64 = 0.5;

#[derive(Debug, Clone)]
struct ActN {
    slot: usize,
    strat: Option<Strategy>,
    reward: f64,
    energy: i64,
    p: f64,
    n: f64,
    sum: f64,
    child: [Option<usize>; 2],
}

#[derive(Debug, Clone, Default)]
struct Node {
    n: f64,
    acts: Option<Vec<ActN>>,
}

/// The simulator interface the tree needs: actions of a state and the state after an outcome.
trait Sim {
    type S: Clone;
    fn actions(&mut self, s: &Self::S) -> Vec<ActN>;
    fn next(&mut self, s: &Self::S, a: &ActN, ok: bool, rng: &mut Rng) -> Self::S;
    fn budget(&self, s: &Self::S) -> i64;
}

fn uct<M: Sim>(sim: &mut M, root: M::S, iters: usize, depth: usize, c: f64, rho: f64, rng: &mut Rng) -> Option<(usize, Option<Strategy>, f64)> {
    let mut nodes: Vec<Node> = vec![Node::default()];
    for _ in 0..iters {
        walk(sim, &mut nodes, 0, root.clone(), depth, c, rho, rng);
    }
    let acts = nodes[0].acts.as_ref()?;
    let mut best: Option<(f64, usize)> = None;
    for (i, a) in acts.iter().enumerate() {
        if a.n > 0.0 {
            let m = a.sum / a.n;
            if best.map_or(true, |b| m > b.0) {
                best = Some((m, i));
            }
        }
    }
    let (m, i) = best?;
    Some((acts[i].slot, acts[i].strat, m))
}

#[allow(clippy::too_many_arguments)]
fn walk<M: Sim>(sim: &mut M, nodes: &mut Vec<Node>, id: usize, s: M::S, depth: usize, c: f64, rho: f64, rng: &mut Rng) -> f64 {
    if depth == 0 || sim.budget(&s) <= 0 {
        return 0.0;
    }
    if nodes[id].acts.is_none() {
        let mut a = sim.actions(&s);
        // untried actions are tried in the order of their one-step value
        a.sort_by(|x, y| (y.p * y.reward - rho * y.energy as f64).partial_cmp(&(x.p * x.reward - rho * x.energy as f64)).unwrap_or(std::cmp::Ordering::Equal));
        nodes[id].acts = Some(a);
    }
    let n = nodes[id].n;
    let (pick, scale) = {
        let acts = nodes[id].acts.as_ref().expect("expanded");
        if acts.is_empty() {
            return 0.0;
        }
        let scale = acts.iter().map(|a| a.reward).fold(1.0, f64::max);
        let mut pick = 0;
        let mut bv = f64::MIN;
        for (i, a) in acts.iter().enumerate() {
            let v = if a.n == 0.0 { 1e18 - i as f64 } else { a.sum / a.n + c * scale * ((n.max(1.0)).ln() / a.n).sqrt() };
            if v > bv {
                bv = v;
                pick = i;
            }
        }
        (pick, scale)
    };
    let _ = scale;
    let a = nodes[id].acts.as_ref().expect("expanded")[pick].clone();
    // the immediate term is the expectation (lower variance); the outcome is sampled for the next state
    let ok = rng.f64() < a.p;
    let r = a.p * a.reward - rho * a.energy as f64;
    let ns = sim.next(&s, &a, ok, rng);
    let child = match a.child[ok as usize] {
        Some(c) => c,
        None => {
            nodes.push(Node::default());
            let k = nodes.len() - 1;
            nodes[id].acts.as_mut().expect("expanded")[pick].child[ok as usize] = Some(k);
            k
        }
    };
    let v = r + walk(sim, nodes, child, ns, depth - 1, c, rho, rng);
    nodes[id].n += 1.0;
    let an = &mut nodes[id].acts.as_mut().expect("expanded")[pick];
    an.n += 1.0;
    an.sum += v;
    v
}

// ---- World v2 ----

struct SimV2<'a> {
    w: &'a World2,
    o: &'a mut Oracle2,
}

#[derive(Clone)]
struct SV2 {
    step: u64,
    clock: i64,
    budget: i64,
}

impl<'a> Sim for SimV2<'a> {
    type S = SV2;
    fn actions(&mut self, s: &SV2) -> Vec<ActN> {
        let mut v = vec![];
        for t in self.w.offers(s.step, s.clock) {
            let tv = &t.view;
            for st in STRATEGIES {
                let e = st.energy(tv.cost);
                if e <= s.budget {
                    let p = self.o.truth(tv.fam, tv.difficulty, st, s.clock);
                    v.push(ActN { slot: tv.slot, strat: Some(st), reward: tv.reward as f64, energy: e, p, n: 0.0, sum: 0.0, child: [None, None] });
                }
            }
        }
        if s.budget >= 1 {
            v.push(ActN { slot: usize::MAX, strat: None, reward: 0.0, energy: 1, p: 1.0, n: 0.0, sum: 0.0, child: [None, None] });
        }
        v
    }
    fn next(&mut self, s: &SV2, a: &ActN, _ok: bool, _rng: &mut Rng) -> SV2 {
        SV2 { step: s.step + 1, clock: s.clock + a.energy * SECS_PER_ENERGY, budget: s.budget - a.energy }
    }
    fn budget(&self, s: &SV2) -> i64 {
        s.budget
    }
}

/// MCTS cheater in World v2 (fixed strategies only; it does not answer with the checker's answer).
pub struct Mcts2 {
    pub world: World2,
    oracle: Oracle2,
    pub iters: usize,
    pub depth: usize,
    pub c: f64,
    rng: Rng,
    exp_reward: f64,
    spent: f64,
}

impl Mcts2 {
    pub fn new(world: World2, seed: u64) -> Mcts2 {
        let oracle = Oracle2::new(world.clone(), false);
        Mcts2 { world, oracle, iters: MCTS_ITERS, depth: MCTS_DEPTH_V2, c: MCTS_C, rng: Rng::new(seed ^ 0x3C75), exp_reward: 0.0, spent: 0.0 }
    }
}

impl Chooser2 for Mcts2 {
    fn name(&self) -> String {
        "mcts-true-simulator".into()
    }
    fn choose(&mut self, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
        let root = SV2 { step: st.step, clock: st.clock, budget: st.budget_left };
        let rho = if self.spent >= 40.0 {
            self.exp_reward / self.spent
        } else {
            let mut b = 0.0f64;
            for o in offers {
                for s in STRATEGIES {
                    let e = s.energy(o.listed_cost);
                    if e <= st.budget_left {
                        b = b.max(o.listed_reward as f64 * self.oracle.truth(o.fam, o.difficulty, s, st.clock) / e as f64);
                    }
                }
            }
            0.75 * b
        };
        let mut sim = SimV2 { w: &self.world, o: &mut self.oracle };
        let (slot, strat, v) = uct(&mut sim, root, self.iters, self.depth, self.c, rho, &mut self.rng)?;
        match strat {
            None => {
                self.spent += 1.0;
                Some(plain("mcts-true-simulator", "UCT on the true simulator: skip", Act2::Skip, 0.0, v))
            }
            Some(s) => {
                let o = offers.iter().find(|o| o.slot == slot)?;
                let p = self.oracle.truth(o.fam, o.difficulty, s, st.clock);
                self.exp_reward += p * o.listed_reward as f64;
                self.spent += s.energy(o.listed_cost) as f64;
                Some(plain("mcts-true-simulator", "UCT on the true simulator", Act2::Attempt { slot, strategy: s }, p, v))
            }
        }
    }
}

// ---- World L ----

struct SimL<'a> {
    w: &'a WorldL,
}

#[derive(Clone)]
struct SL {
    step: u64,
    budget: i64,
    token: u8,
    phase: u8,
}

impl<'a> Sim for SimL<'a> {
    type S = SL;
    fn actions(&mut self, s: &SL) -> Vec<ActN> {
        let mut v = vec![];
        for o in self.w.offers(s.step) {
            if o.needs_token && s.token == 0 {
                continue;
            }
            for st in STRATEGIES {
                let e = st.energy(o.cost);
                if e <= s.budget {
                    v.push(ActN { slot: o.slot, strat: Some(st), reward: o.reward as f64, energy: e, p: self.w.p_true(o.fam, st, s.phase), n: 0.0, sum: 0.0, child: [None, None] });
                }
            }
        }
        if s.budget >= 1 {
            v.push(ActN { slot: usize::MAX, strat: None, reward: 0.0, energy: 1, p: 1.0, n: 0.0, sum: 0.0, child: [None, None] });
        }
        v
    }
    fn next(&mut self, s: &SL, a: &ActN, ok: bool, rng: &mut Rng) -> SL {
        let mut n = SL { step: s.step + 1, budget: s.budget - a.energy, token: s.token, phase: s.phase };
        if a.strat.is_some() {
            let o = self.w.offers(s.step)[a.slot];
            let kind = self.w.fams[o.fam].kind;
            if o.needs_token {
                n.token = 0;
            }
            if ok && kind == FKind::Key && n.token == 0 {
                n.token = 1;
            }
            if kind == FKind::Job {
                n.phase = (rng.f64() < 0.5) as u8;
            }
        }
        n
    }
    fn budget(&self, s: &SL) -> i64 {
        s.budget
    }
}

/// MCTS cheater in World L (knows the hidden probabilities, the token and phase rules, the current phase).
pub struct MctsL {
    pub world: WorldL,
    pub iters: usize,
    pub depth: usize,
    pub c: f64,
    rng: Rng,
    exp_reward: f64,
    spent: f64,
}

impl MctsL {
    pub fn new(world: WorldL, seed: u64) -> MctsL {
        MctsL { world, iters: MCTS_ITERS, depth: MCTS_DEPTH_L, c: MCTS_C, rng: Rng::new(seed ^ 0x3C7A), exp_reward: 0.0, spent: 0.0 }
    }
    /// The hidden phase now (what the engine holds): from the last job attempt in the being's state.
    fn phase(&self, st: &StateL) -> u8 {
        let job = self.world.fams.iter().find(|f| f.kind == FKind::Job).map(|f| f.idx);
        match job.and_then(|j| st.last_at.get(j).copied().flatten()) {
            Some(t) => self.world.next_phase(t),
            None => self.world.first_phase(),
        }
    }
}

impl ChooserL for MctsL {
    fn name(&self) -> String {
        "mcts-true-simulator".into()
    }
    fn choose(&mut self, st: &StateL, offers: &[crate::being::lookahead::worldl::OfferL]) -> Option<ChoiceL> {
        let phase = self.phase(st);
        let root = SL { step: st.step, budget: st.budget_left, token: st.token, phase };
        let rho = if self.spent >= 40.0 {
            self.exp_reward / self.spent
        } else {
            let mut b = 0.0f64;
            for o in offers {
                if o.needs_token && st.token == 0 {
                    continue;
                }
                for s in STRATEGIES {
                    let e = s.energy(o.cost);
                    if e <= st.budget_left {
                        b = b.max(o.reward as f64 * self.world.p_true(o.fam, s, phase) / e as f64);
                    }
                }
            }
            0.75 * b
        };
        let mut sim = SimL { w: &self.world };
        let (slot, strat, v) = uct(&mut sim, root, self.iters, self.depth, self.c, rho, &mut self.rng)?;
        let (act, p) = match strat {
            None => {
                self.spent += 1.0;
                (ActL::Skip, 0.0)
            }
            Some(s) => {
                let o = offers[slot];
                let p = self.world.p_true(o.fam, s, phase);
                self.exp_reward += p * o.reward as f64;
                self.spent += s.energy(o.cost) as f64;
                (ActL::Attempt { slot, strategy: s }, p)
            }
        };
        Some(ChoiceL { act, policy: "mcts-true-simulator".into(), why: "UCT on the true simulator".into(), p, score: v, rationale: json!({ "policy": "mcts-true-simulator" }), nodes: self.iters as u64 })
    }
}
