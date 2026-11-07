//! The lookahead planner (Track B8): a bounded search over the being's OWN learned model.
//!
//! Inputs, all learned or observed: per (family, difficulty, strategy, context) cell a Beta posterior of
//! the success probability and an energy ratio (from `model.rs` / `world2/model.rs` / `beingl.rs`, read
//! through `Source`), the menus the being has seen (the last `memory` menus: future menus are sampled
//! from them), a learned rule for tokens (World L only: how often a success at a family has given a
//! token), and the energy left.
//!
//! Method: depth-limited expectimax with sparse sampling (Kearns, Mansour and Ng, 2002), optionally
//! Bayes-adaptive inside the tree (`learn`; off by default, it did not help on the tuning seeds): every simulated attempt updates the simulated posterior of its cell by one success or
//! one failure, so a deeper node sees what the being would have learned on that branch, and the value of
//! a branch weighs both outcomes by the posterior predictive probability. That is how information (a
//! probe, an untried cell) gets value: by changing later simulated choices.
//!
//! * Root: every affordable (offer, strategy) and skip. `Q = p_root * reward + p * V(after success) +
//!   (1 - p) * V(after failure)`, with `p_root = min(1, p + beta_root * sd)` (the B1 optimism, kept for the
//!   immediate term only) and `p` the posterior mean.
//! * Inner node `V(state, d)`: the mean over `samples` menus drawn from the memory (the same draws for
//!   every branch at the same depth: common random numbers) of the best `Q` over the `width` actions with
//!   the highest myopic surplus `p * reward - rho * energy`, plus skip.
//! * Leaf: `rho * energy left + token * tau`, where `rho` is the reward per energy the learned model
//!   predicts for the stationary policy "take the option with the largest `p * reward - rho * energy`"
//!   (a Dinkelbach fixed point over the remembered menus, mean estimates), and `tau` the learned surplus of
//!   using a held token (World L).
//! * Depth 1 is therefore the one-step rule "largest `p_root * reward - rho * energy`" (opportunity cost of
//!   energy), not the B1 ratio rule; depth 2 and more add simulated future steps.
//! * Compute: iterative deepening 1, 2, ... `max_depth`; every Q evaluation counts as one node; when the
//!   count reaches `max_nodes` the running depth is abandoned at once and the deepest completed depth is
//!   used (`truncated`). If not even depth 1 fits, no plan is returned and the caller uses the B1 planner.
//!
//! Deterministic: no clock, no global random state; menu draws are a hash of (step, depth, sample).
//! It reads no hidden world parameter (a test scans this file).

use crate::being::solver::Strategy;
use crate::being::world2::world::mix2;
use serde_json::{json, Value};
use std::collections::HashMap;

/// An offer as the planner sees it (only what the menu shows).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AOffer {
    pub slot: usize,
    pub fam: usize,
    pub diff: u8,
    pub cost: i64,
    pub reward: i64,
    /// World L: the offer can be attempted only while a token is held (shown on the menu).
    pub needs_token: bool,
}

/// One cell of the learned model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key {
    pub fam: u32,
    pub diff: u8,
    pub strat: u8,
    /// 0: no context; World L: 1 + 2 * family + ok of the previous attempt.
    pub ctx: u16,
}

/// A cell's posterior: Beta(a, b) of the success probability, the energy ratio, own attempts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Base {
    pub a: f64,
    pub b: f64,
    pub ratio: f64,
    pub n: u64,
}

impl Base {
    /// From a posterior mean and standard deviation (sd^2 = p (1 - p) / (a + b + 1)).
    pub fn from_mean_sd(p: f64, sd: f64, ratio: f64, n: u64) -> Base {
        let p = p.clamp(1e-9, 1.0 - 1e-9);
        let m = if sd > 1e-12 { (p * (1.0 - p) / (sd * sd) - 1.0).max(1e-6) } else { 1e9 };
        Base { a: p * m, b: (1.0 - p) * m, ratio, n }
    }
}

/// The learned model, read-only.
pub trait Source {
    fn base(&self, k: Key) -> Base;
    fn strategies(&self) -> &[Strategy];
    /// Context after an attempt at `fam` with result `ok` (0 when the model keeps no context).
    fn ctx_after(&self, _fam: usize, _ok: bool) -> u16 {
        0
    }
    /// Learned probability that a success at `fam` gives a token (World L; 0 elsewhere).
    fn token_p(&self, _fam: usize) -> f64 {
        0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LookCfg {
    /// Steps simulated, the current one included (1 = one-step opportunity-cost rule).
    pub depth: u32,
    pub max_nodes: u64,
    /// Menus sampled per inner node.
    pub samples: usize,
    /// Attempts kept per inner node (best myopic surplus), plus skip.
    pub width: usize,
    /// Optimism of the immediate term at the root (B1: 1.5).
    pub beta_root: f64,
    /// Remembered menus future menus are drawn from.
    pub memory: usize,
    /// How the plan is turned into a decision.
    pub mode: Mode,
    /// Hybrid only: weight of the simulated future.
    pub lambda: f64,
    /// Simulated outcomes update the simulated posteriors (Bayes-adaptive). Off: the model is held fixed
    /// inside the tree, so only tokens, contexts and energy carry value forward.
    pub learn: bool,
}

/// `Pure`: take the alternative with the highest simulated value. `Hybrid`: keep the B1 score
/// (optimistic reward per estimated energy) and add `lambda * (Q_depth - Q_1) / energy`, the value the
/// simulated future steps add beyond the one-step leaf, per unit of the option's energy; the B1 skip
/// rule then applies to the best hybrid score. At depth 1 the hybrid is the B1 planner exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Pure,
    Hybrid,
}

pub const DEFAULT_LAMBDA: f64 = 1.0;
pub const DEFAULT_LEARN: bool = false;
pub const DEFAULT_MODE: Mode = Mode::Hybrid;

/// The hybrid choice among attempts: B1 scores (`scores`: slot, strategy, score, affordable) plus the
/// simulated future per energy. Returns (slot, strategy, hybrid score, added term).
pub fn hybrid_pick(p: &Plan, scores: &[(usize, Strategy, f64, bool)], lambda: f64) -> Option<(usize, Strategy, f64, f64)> {
    let mut best: Option<(usize, Strategy, f64, f64)> = None;
    for (slot, st, sc, ok) in scores {
        if !ok {
            continue;
        }
        let add = p.alts.iter().find(|a| a.act == Some((*slot, *st))).map_or(0.0, |a| lambda * (a.q - a.q1) / a.energy.max(1.0));
        let h = sc + add;
        if best.map_or(true, |b| h > b.2) {
            best = Some((*slot, *st, h, add));
        }
    }
    best
}

/// Chosen on tuning seeds 101..120 only (see docs/measured.md, Track B8).
pub const DEFAULT_SAMPLES: usize = 2;
pub const DEFAULT_WIDTH: usize = 3;
pub const DEFAULT_BETA_ROOT: f64 = 1.5;
pub const DEFAULT_MEMORY: usize = 64;

impl LookCfg {
    pub fn new(depth: u32, max_nodes: u64) -> LookCfg {
        LookCfg { depth, max_nodes, samples: DEFAULT_SAMPLES, width: DEFAULT_WIDTH, beta_root: DEFAULT_BETA_ROOT, memory: DEFAULT_MEMORY, mode: DEFAULT_MODE, lambda: DEFAULT_LAMBDA, learn: DEFAULT_LEARN }
    }
}

/// The simulated state of the being.
#[derive(Debug, Clone, PartialEq)]
pub struct Sim {
    pub budget: f64,
    pub token: u8,
    pub ctx: u16,
    /// Simulated observations: (cell, successes, failures).
    overlay: Vec<(Key, f64, f64)>,
}

impl Sim {
    pub fn new(budget: i64, token: u8, ctx: u16) -> Sim {
        Sim { budget: budget as f64, token, ctx, overlay: vec![] }
    }
}

/// One root alternative with its simulated values.
#[derive(Debug, Clone, PartialEq)]
pub struct Alt {
    /// None: skip.
    pub act: Option<(usize, Strategy)>,
    pub key: Option<Key>,
    pub n: u64,
    pub p: f64,
    pub sd: f64,
    pub energy: f64,
    pub reward: i64,
    /// Q minus the leaf value of doing nothing (`rho * energy left + token * tau`), at the depth used.
    pub q: f64,
    pub immediate: f64,
    pub v_ok: f64,
    pub v_fail: f64,
    /// Q minus the leaf at depth 1 (the one-step opportunity-cost value); `q - q1` is what the simulated
    /// future steps add.
    pub q1: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// None: skip the menu.
    pub act: Option<(usize, Strategy)>,
    pub alts: Vec<Alt>,
    pub depth_done: u32,
    pub depth_wanted: u32,
    pub nodes: u64,
    pub truncated: bool,
    pub rho: f64,
    pub tau: f64,
    /// Cells the search read, with their posterior and how often.
    pub cells: Vec<(Key, Base, u64)>,
    /// Memory indices (step numbers of the remembered menus) the search drew.
    pub menus_drawn: Vec<u64>,
}

struct Search<'a> {
    src: &'a dyn Source,
    cfg: LookCfg,
    memory: &'a [(u64, Vec<AOffer>)],
    step: u64,
    rho: f64,
    tau: f64,
    nodes: u64,
    abort: bool,
    cache: HashMap<Key, (Base, u64)>,
    drawn: Vec<u64>,
    top: u32,
}

fn key_of(o: &AOffer, s: Strategy, ctx: u16) -> Key {
    Key { fam: o.fam as u32, diff: o.diff, strat: s.idx() as u8, ctx }
}

impl<'a> Search<'a> {
    fn base(&mut self, k: Key) -> Base {
        if let Some(e) = self.cache.get_mut(&k) {
            e.1 += 1;
            return e.0;
        }
        let b = self.src.base(k);
        self.cache.insert(k, (b, 1));
        b
    }

    /// Posterior mean and sd of a cell in the simulated state (overlay included).
    fn post(&mut self, s: &Sim, k: Key) -> (f64, f64, Base) {
        let b = self.base(k);
        let (mut a, mut bb) = (b.a, b.b);
        for (kk, ds, df) in &s.overlay {
            if *kk == k {
                a += ds;
                bb += df;
            }
        }
        let p = a / (a + bb);
        (p, (p * (1.0 - p) / (a + bb + 1.0)).sqrt(), b)
    }

    fn after(&self, s: &Sim, o: &AOffer, k: Key, energy: f64, ok: bool) -> Sim {
        let mut n = s.clone();
        n.budget -= energy;
        if !self.cfg.learn {
            // the model is held fixed inside the tree
        } else if let Some(e) = n.overlay.iter_mut().find(|e| e.0 == k) {
            if ok { e.1 += 1.0 } else { e.2 += 1.0 }
        } else {
            n.overlay.push((k, ok as u8 as f64, (!ok) as u8 as f64));
        }
        if o.needs_token {
            n.token = n.token.saturating_sub(1);
        }
        let c = self.src.ctx_after(o.fam, ok);
        if c != 0 {
            n.ctx = c;
        }
        n
    }

    fn leaf(&self, s: &Sim) -> f64 {
        self.rho * s.budget.max(0.0) + s.token as f64 * self.tau
    }

    fn actions(&mut self, s: &Sim, menu: &[AOffer]) -> Vec<(AOffer, Strategy, Key, f64, f64, f64, Base)> {
        let mut v = vec![];
        let strats: Vec<Strategy> = self.src.strategies().to_vec();
        for o in menu {
            if o.needs_token && s.token == 0 {
                continue;
            }
            for &st in &strats {
                let k = key_of(o, st, s.ctx);
                let (p, sd, b) = self.post(s, k);
                let e = (o.cost as f64 * b.ratio).max(1.0);
                if e.round() > s.budget {
                    continue;
                }
                v.push((*o, st, k, p, sd, e, b));
            }
        }
        v
    }

    /// Value of a simulated state with `d` more steps to expand.
    fn v(&mut self, s: &Sim, d: u32) -> f64 {
        if d == 0 || s.budget < 1.0 || self.memory.is_empty() || self.abort {
            return self.leaf(s);
        }
        let level = self.top - d;
        let mut total = 0.0;
        for j in 0..self.cfg.samples {
            let mi = (mix2(&[self.step, level as u64, j as u64, 0x100C_A4EA]) % self.memory.len() as u64) as usize;
            let (mstep, menu) = (&self.memory[mi].0, &self.memory[mi].1);
            if !self.drawn.contains(mstep) {
                self.drawn.push(*mstep);
            }
            let mut acts = self.actions(s, menu);
            // keep the `width` best by myopic surplus (stable: ties keep menu order)
            let rho = self.rho;
            let mut idx: Vec<usize> = (0..acts.len()).collect();
            idx.sort_by(|&x, &y| {
                let sx = acts[x].3 * acts[x].0.reward as f64 - rho * acts[x].5;
                let sy = acts[y].3 * acts[y].0.reward as f64 - rho * acts[y].5;
                sy.partial_cmp(&sx).unwrap_or(std::cmp::Ordering::Equal)
            });
            idx.truncate(self.cfg.width);
            idx.sort();
            acts = idx.into_iter().map(|i| acts[i]).collect();
            let mut best = f64::NEG_INFINITY;
            for (o, _st, k, p, _sd, e, _b) in acts {
                let q = self.q_attempt(s, &o, k, p, p, e, d);
                if q > best {
                    best = q;
                }
            }
            if s.budget >= 1.0 {
                let q = self.q_skip(s, d);
                if q > best {
                    best = q;
                }
            }
            if best == f64::NEG_INFINITY {
                best = self.leaf(s);
            }
            total += best;
            if self.abort {
                return 0.0;
            }
        }
        total / self.cfg.samples as f64
    }

    fn count(&mut self) -> bool {
        self.nodes += 1;
        if self.nodes > self.cfg.max_nodes {
            self.abort = true;
        }
        !self.abort
    }

    /// Q of an attempt with `d` steps of tree left (this one included); returns (Q, immediate, V ok, V fail) through `q_parts`.
    fn q_attempt(&mut self, s: &Sim, o: &AOffer, k: Key, p_imm: f64, p: f64, e: f64, d: u32) -> f64 {
        self.q_parts(s, o, k, p_imm, p, e, d).0
    }

    fn q_parts(&mut self, s: &Sim, o: &AOffer, k: Key, p_imm: f64, p: f64, e: f64, d: u32) -> (f64, f64, f64, f64) {
        if !self.count() {
            return (0.0, 0.0, 0.0, 0.0);
        }
        let imm = p_imm * o.reward as f64;
        let s_ok = self.after(s, o, k, e, true);
        let s_fail = self.after(s, o, k, e, false);
        let tp = if s.token == 0 && !o.needs_token { self.src.token_p(o.fam) } else { 0.0 };
        let v_ok = if tp > 0.02 {
            let mut with = s_ok.clone();
            with.token = 1;
            tp * self.v(&with, d - 1) + (1.0 - tp) * self.v(&s_ok, d - 1)
        } else {
            self.v(&s_ok, d - 1)
        };
        let v_fail = self.v(&s_fail, d - 1);
        (imm + p * v_ok + (1.0 - p) * v_fail, imm, v_ok, v_fail)
    }

    fn q_skip(&mut self, s: &Sim, d: u32) -> f64 {
        if !self.count() {
            return 0.0;
        }
        let mut n = s.clone();
        n.budget -= 1.0;
        self.v(&n, d - 1)
    }
}

/// The learned reward per energy of the stationary policy "largest `p * reward - rho * energy`, or skip",
/// over the remembered menus (Dinkelbach iteration from `rho0`), and the learned surplus of a token.
pub fn rate_and_token(src: &dyn Source, memory: &[(u64, Vec<AOffer>)], rho0: f64) -> (f64, f64) {
    if memory.is_empty() {
        return (rho0.max(0.0), 0.0);
    }
    let strats: Vec<Strategy> = src.strategies().to_vec();
    // (p * reward, energy) per menu option; tokens are not assumed (offers that need one are left out)
    let opts: Vec<Vec<(f64, f64)>> = memory
        .iter()
        .map(|(_, m)| {
            let mut v = vec![];
            for o in m.iter().filter(|o| !o.needs_token) {
                for &st in &strats {
                    let b = src.base(key_of(o, st, 0));
                    v.push((o.reward as f64 * b.a / (b.a + b.b), (o.cost as f64 * b.ratio).max(1.0)));
                }
            }
            v
        })
        .collect();
    let mut rho = rho0.max(0.0);
    for _ in 0..30 {
        let (mut r, mut c) = (0.0, 0.0);
        for m in &opts {
            let mut best = (0.0, 1.0, -rho); // skip
            for &(pr, e) in m {
                let sur = pr - rho * e;
                if sur > best.2 {
                    best = (pr, e, sur);
                }
            }
            r += best.0;
            c += best.1;
        }
        let nr = if c > 0.0 { r / c } else { 0.0 };
        if (nr - rho).abs() < 1e-12 {
            rho = nr;
            break;
        }
        rho = nr;
    }
    // token: mean over remembered menus that show a token-only offer of the best surplus of using it
    let (mut tsum, mut tn) = (0.0, 0u32);
    for (_, m) in memory {
        let mut best: Option<f64> = None;
        for o in m.iter().filter(|o| o.needs_token) {
            for &st in &strats {
                let b = src.base(key_of(o, st, 0));
                let sur = o.reward as f64 * b.a / (b.a + b.b) - rho * (o.cost as f64 * b.ratio).max(1.0);
                best = Some(best.map_or(sur, |x: f64| x.max(sur)));
            }
        }
        if let Some(b) = best {
            tsum += b.max(0.0);
            tn += 1;
        }
    }
    (rho, if tn > 0 { tsum / tn as f64 } else { 0.0 })
}

/// Plan one step. `menu` is the current menu; `memory` the remembered menus (oldest first, as (step, menu));
/// `rho0` the starting point of the rate estimate. None: nothing fits the budget, or not even depth 1
/// fits the node cap (the caller then uses the B1 planner).
pub fn plan(src: &dyn Source, cfg: &LookCfg, menu: &[AOffer], memory: &[(u64, Vec<AOffer>)], s0: &Sim, step: u64, rho0: f64) -> Option<Plan> {
    let (rho, tau) = rate_and_token(src, memory, rho0);
    let mut se = Search { src, cfg: *cfg, memory, step, rho, tau, nodes: 0, abort: false, cache: HashMap::new(), drawn: vec![], top: 1 };
    let leaf0 = se.leaf(s0);
    let mut best: Option<(Option<(usize, Strategy)>, Vec<Alt>, u32)> = None;
    let mut first: Vec<Alt> = vec![];
    let mut truncated = false;
    for depth in 1..=cfg.depth.max(1) {
        se.top = depth;
        se.abort = false;
        let mut alts = vec![];
        for (o, st, k, p, sd, e, b) in se.actions(s0, menu) {
            let p_imm = (p + cfg.beta_root * sd).min(1.0);
            let (q, imm, vo, vf) = se.q_parts(s0, &o, k, p_imm, p, e, depth);
            alts.push(Alt { act: Some((o.slot, st)), key: Some(k), n: b.n, p, sd, energy: e, reward: o.reward, q: q - leaf0, immediate: imm, v_ok: vo - leaf0, v_fail: vf - leaf0, q1: 0.0 });
            if se.abort {
                break;
            }
        }
        if s0.budget >= 1.0 && !se.abort {
            let q = se.q_skip(s0, depth);
            alts.push(Alt { act: None, key: None, n: 0, p: 0.0, sd: 0.0, energy: 1.0, reward: 0, q: q - leaf0, immediate: 0.0, v_ok: q - leaf0, v_fail: q - leaf0, q1: 0.0 });
        }
        if se.abort {
            truncated = true;
            break;
        }
        if alts.is_empty() {
            return None;
        }
        // first best in (slot, strategy) order wins ties; skip only if strictly better
        let mut bi = 0;
        for (i, a) in alts.iter().enumerate() {
            if a.q > alts[bi].q {
                bi = i;
            }
        }
        if depth == 1 {
            first = alts.clone();
        }
        best = Some((alts[bi].act, alts, depth));
    }
    let (act, mut alts, depth_done) = best?;
    for a in alts.iter_mut() {
        a.q1 = first.iter().find(|f| f.act == a.act).map_or(a.q, |f| f.q);
    }
    let mut cells: Vec<(Key, Base, u64)> = se.cache.iter().map(|(k, (b, c))| (*k, *b, *c)).collect();
    cells.sort_by(|x, y| x.0.cmp(&y.0));
    let mut drawn = se.drawn.clone();
    drawn.sort();
    Some(Plan { act, alts, depth_done, depth_wanted: cfg.depth.max(1), nodes: se.nodes.min(cfg.max_nodes), truncated, rho, tau, cells, menus_drawn: drawn })
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// The decision of a plan under `cfg.mode`: (action, None = skip; the score it was chosen on; the added
/// future term for the hybrid). `scores` are the B1 scores, `skip_ok(score)` the B1 skip rule.
/// None (hybrid only): no attempt is affordable by the B1 estimate; the caller lets the B1 planner answer.
pub fn decide(p: &Plan, cfg: &LookCfg, scores: &[(usize, Strategy, f64, bool)], skip_ok: &dyn Fn(f64) -> bool) -> Option<(Option<(usize, Strategy)>, f64, f64)> {
    match cfg.mode {
        Mode::Pure => Some((p.act, p.chosen().q, 0.0)),
        Mode::Hybrid => match hybrid_pick(p, scores, cfg.lambda) {
            Some((sl, st, h, add)) if !skip_ok(h) => Some((Some((sl, st)), h, add)),
            Some((_, _, h, add)) => Some((None, h, add)),
            None => None,
        },
    }
}

impl Plan {
    pub fn label(&self) -> String {
        match self.act {
            Some((slot, s)) => format!("{slot}:{}", s.name()),
            None => "skip".into(),
        }
    }

    /// The chosen alternative.
    pub fn chosen(&self) -> &Alt {
        self.alts.iter().find(|a| a.act == self.act).expect("the chosen action is an alternative")
    }

    /// Evidence for the log: every root alternative with its simulated values, the cells read, the menus drawn.
    pub fn to_json(&self, cfg: &LookCfg, cell_names: &dyn Fn(&Key) -> String) -> Value {
        let alts: Vec<Value> = self
            .alts
            .iter()
            .map(|a| {
                json!({
                    "action": match a.act { Some((sl, s)) => format!("{sl}:{}", s.name()), None => "skip".into() },
                    "cell": a.key.as_ref().map(|k| cell_names(k)), "n_cell": a.n, "p_success": r3(a.p), "uncertainty_sd": r3(a.sd), "est_energy": r3(a.energy), "listed_reward": a.reward,
                    "immediate": r3(a.immediate), "value_if_success": r3(a.v_ok), "value_if_failure": r3(a.v_fail), "q_minus_leaf": r3(a.q), "q1_minus_leaf": r3(a.q1),
                })
            })
            .collect();
        let max_cells = 64;
        let cells: Vec<Value> = self.cells.iter().take(max_cells).map(|(k, b, c)| json!({ "cell": cell_names(k), "n": b.n, "p": r3(b.a / (b.a + b.b)), "pseudo_a": r3(b.a), "pseudo_b": r3(b.b), "energy_ratio": r3(b.ratio), "reads": c })).collect();
        json!({
            "planner": "lookahead (B8): depth-limited expectimax with sparse menu sampling over the learned model, Bayes-adaptive (simulated outcomes update simulated posteriors)",
            "depth_wanted": self.depth_wanted, "depth_done": self.depth_done, "nodes": self.nodes, "node_cap": cfg.max_nodes, "truncated": self.truncated,
            "mode": format!("{:?}", cfg.mode), "lambda": cfg.lambda, "simulated_learning": cfg.learn, "samples_per_node": cfg.samples, "width": cfg.width, "beta_root": cfg.beta_root, "memory_menus": cfg.memory,
            "rho_learned_reward_per_energy": r3(self.rho), "tau_learned_token_value": r3(self.tau),
            "chosen": self.label(), "alternatives": alts, "cells_read": cells, "cells_read_total": self.cells.len(), "menus_drawn_from_steps": self.menus_drawn,
        })
    }
}
