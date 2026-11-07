//! The being in World L: its state folded from its own log, its learned model, the planners compared in
//! Track B8 (the B1 one-step planner, UCB1, the lookahead planner, two oracles), the step engine and the
//! logged run (controller, kill switch, audited refusals).
//!
//! Learned model (`StateL::estimate`), built only from logged outcomes and logged menus:
//! * success per (family, strategy, context) cell, context = (family, result) of the previous attempt
//!   (0 before the first): Beta posterior with the B1 hierarchy and constants (cell prior from the
//!   (family, strategy) group, group prior from the strategy-wide estimate, flat Beta(1,1) at the top,
//!   `PRIOR_K` pseudo-observations per level); the energy ratio shrunk the same way with `COST_K`;
//! * the token rule: per family, how often a success while no token was held was followed by a token
//!   (Beta(0.5, 1.5) prior, so 0.25 before evidence);
//! * the menu memory: the last `MEMORY` menus the being was shown (logged in its decision events).
//! Every planner in World L reads the same model; they differ only in how they plan.

use super::ctl::{node_cap_refusal, LookCtl};
use super::plan::{self, AOffer, Base, Key, LookCfg, Sim, Source};
use super::worldl::{belief_after, OfferL, OracleDp, WLCfg, WorldL, BEL_U, SKIP_COST, SOURCE_L};
use crate::being::controller::{Controller, Refusal};
use crate::being::model::{COST_K, PRIOR_K};
use crate::being::planner::{DEFAULT_BETA, DEFAULT_SKIP_FRAC, SKIP_MIN_COST};
use crate::being::policy::{Policy, StopReason};
use crate::being::solver::{Strategy, STRATEGIES};
use crate::being::state::{tag, EFF_NORM, EVIDENCE_WINDOW};
use crate::being::world::{iso_at, SECS_PER_ENERGY};
use crate::versioned::{event_id, Batch, EventRow, LogError, NewEvent, NewRevision, VersionedLog};
use serde_json::{json, Value};
use std::collections::VecDeque;

/// Remembered menus (the planner samples future menus from them).
pub const MEMORY: usize = 64;
const TOKEN_A: f64 = 0.5;
const TOKEN_B: f64 = 1.5;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Cnt {
    pub n: u64,
    pub succ: u64,
    pub spent: i64,
    pub listed: i64,
}

impl Cnt {
    fn add(&mut self, ok: bool, spent: i64, listed: i64) {
        self.n += 1;
        self.succ += ok as u64;
        self.spent += spent;
        self.listed += listed;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Arm {
    pub n: u64,
    pub reward: i64,
    pub cost: i64,
}

/// One outcome as the being was told it.
#[derive(Debug, Clone, PartialEq)]
pub struct OutcomeL {
    pub step: u64,
    pub fam: Option<usize>,
    pub strategy: Option<Strategy>,
    /// correct, wrong, skip, unpaid
    pub result: String,
    pub reward: i64,
    pub cost: i64,
    pub listed: i64,
    pub budget_left: i64,
    pub clock: i64,
    /// Tokens held after the step (the world says so).
    pub token: u8,
    pub token_gained: bool,
    pub event_id: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StateL {
    pub cfg: Option<WLCfg>,
    pub genesis_event: String,
    pub families: usize,
    pub step: u64,
    pub clock: i64,
    pub budget_left: i64,
    pub total_reward: i64,
    pub total_cost: i64,
    pub skips: u64,
    pub attempts: u64,
    pub token: u8,
    pub ctx: u16,
    cells: Vec<Cnt>,
    groups: Vec<Cnt>,
    tok_rule: Vec<(u64, u64)>,
    pub arms: Vec<[Arm; 3]>,
    pub menus: VecDeque<(u64, Vec<OfferL>)>,
    /// Step of the latest attempt (right or wrong) per family.
    pub last_at: Vec<Option<u64>>,
    recent: VecDeque<String>,
}

pub fn nctx(families: usize) -> usize {
    1 + 2 * families
}

pub fn ctx_of(fam: usize, ok: bool) -> u16 {
    (1 + 2 * fam + ok as usize) as u16
}

pub fn ctx_name(c: u16) -> String {
    if c == 0 { "start".into() } else { format!("after f{}:{}", (c - 1) / 2, if (c - 1) % 2 == 1 { "ok" } else { "fail" }) }
}

pub fn menu_tag(m: &[OfferL]) -> String {
    m.iter().map(|o| format!("{}.{}.{}.{}", o.fam, o.cost, o.reward, o.needs_token as u8)).collect::<Vec<_>>().join("/")
}

pub fn menu_from_tag(t: &str) -> Option<Vec<OfferL>> {
    t.split('/')
        .enumerate()
        .map(|(slot, x)| {
            let p: Vec<i64> = x.split('.').map(|y| y.parse().ok()).collect::<Option<Vec<i64>>>()?;
            if p.len() != 4 {
                return None;
            }
            Some(OfferL { slot, fam: p[0] as usize, cost: p[1], reward: p[2], needs_token: p[3] == 1 })
        })
        .collect()
}

fn shrink(s: f64, n: f64, prior: f64, k: f64) -> f64 {
    (s + k * prior) / (n + k)
}

impl StateL {
    pub fn start(&mut self, cfg: WLCfg, families: usize, event: String) {
        self.budget_left = cfg.budget;
        self.families = families;
        self.cells = vec![Cnt::default(); families * 3 * nctx(families)];
        self.groups = vec![Cnt::default(); families * 3];
        self.tok_rule = vec![(0, 0); families];
        self.arms = vec![[Arm::default(); 3]; families];
        self.last_at = vec![None; families];
        self.cfg = Some(cfg);
        self.genesis_event = event;
    }

    fn ci(&self, fam: usize, s: usize, ctx: u16) -> usize {
        (fam * 3 + s) * nctx(self.families) + (ctx as usize).min(nctx(self.families) - 1)
    }

    /// Remember a menu the being was shown.
    pub fn note_menu(&mut self, step: u64, m: &[OfferL]) {
        if self.menus.back().map_or(false, |x| x.0 == step) {
            return;
        }
        self.menus.push_back((step, m.to_vec()));
        while self.menus.len() > MEMORY {
            self.menus.pop_front();
        }
    }

    pub fn apply(&mut self, o: &OutcomeL) {
        self.total_cost += o.cost;
        self.budget_left = o.budget_left;
        self.clock = o.clock;
        self.step = o.step + 1;
        self.recent.push_back(o.event_id.clone());
        while self.recent.len() > EVIDENCE_WINDOW {
            self.recent.pop_front();
        }
        if o.result == "skip" {
            self.skips += 1;
        }
        let token_before = self.token;
        self.token = o.token;
        let (Some(fam), Some(s)) = (o.fam, o.strategy) else { return };
        if !(o.result == "correct" || o.result == "wrong") || fam >= self.families {
            return;
        }
        let ok = o.result == "correct";
        self.attempts += 1;
        self.total_reward += o.reward;
        let si = s.idx().min(2);
        let i = self.ci(fam, si, self.ctx);
        self.cells[i].add(ok, o.cost, o.listed);
        self.groups[fam * 3 + si].add(ok, o.cost, o.listed);
        if ok && token_before == 0 {
            let t = &mut self.tok_rule[fam];
            t.0 += 1;
            t.1 += o.token_gained as u64;
        }
        let a = &mut self.arms[fam][si];
        a.n += 1;
        a.reward += o.reward;
        a.cost += o.cost;
        self.ctx = ctx_of(fam, ok);
        self.last_at[fam] = Some(o.step);
    }

    /// The learned estimate of a cell: (p, sd, energy ratio, own attempts).
    pub fn estimate(&self, fam: usize, s: Strategy, ctx: u16) -> (f64, f64, f64, u64) {
        let si = s.idx().min(2);
        if fam >= self.families {
            return (0.5, 0.5f64.sqrt() / 2.0, 1.0, 0);
        }
        let mut st = Cnt::default();
        for f in 0..self.families {
            let g = &self.groups[f * 3 + si];
            st.n += g.n;
            st.succ += g.succ;
            st.spent += g.spent;
            st.listed += g.listed;
        }
        let g = &self.groups[fam * 3 + si];
        let c = &self.cells[self.ci(fam, si, ctx)];
        let p_s = (st.succ as f64 + 1.0) / (st.n as f64 + 2.0);
        let p_g = shrink(g.succ as f64, g.n as f64, p_s, PRIOR_K);
        let a = c.succ as f64 + PRIOR_K * p_g;
        let b = (c.n - c.succ) as f64 + PRIOR_K * (1.0 - p_g);
        let p = a / (a + b);
        let sd = (p * (1.0 - p) / (a + b + 1.0)).sqrt();
        let r_s = shrink(st.spent as f64, st.listed as f64, 1.0, COST_K);
        let r_g = shrink(g.spent as f64, g.listed as f64, r_s, COST_K);
        (p, sd, r_g, c.n)
    }

    pub fn base(&self, fam: usize, s: Strategy, ctx: u16) -> Base {
        let si = s.idx().min(2);
        let (p, _, ratio, n) = self.estimate(fam, s, ctx);
        // exact pseudo-counts of the cell posterior (same numbers as `estimate`)
        let mut st = (0u64, 0u64);
        for f in 0..self.families {
            let g = &self.groups[f * 3 + si];
            st.0 += g.n;
            st.1 += g.succ;
        }
        let g = &self.groups[fam * 3 + si];
        let c = &self.cells[self.ci(fam, si, ctx)];
        let p_s = (st.1 as f64 + 1.0) / (st.0 as f64 + 2.0);
        let p_g = shrink(g.succ as f64, g.n as f64, p_s, PRIOR_K);
        let a = c.succ as f64 + PRIOR_K * p_g;
        let b = (c.n - c.succ) as f64 + PRIOR_K * (1.0 - p_g);
        debug_assert!((a / (a + b) - p).abs() < 1e-12);
        Base { a, b, ratio, n }
    }

    /// Learned chance that a success at `fam` gives a token.
    pub fn token_p(&self, fam: usize) -> f64 {
        self.tok_rule.get(fam).map_or(0.0, |t| (t.1 as f64 + TOKEN_A) / (t.0 as f64 + TOKEN_A + TOKEN_B))
    }

    pub fn token_rule(&self, fam: usize) -> (u64, u64) {
        self.tok_rule.get(fam).copied().unwrap_or_default()
    }

    pub fn evidence(&self) -> Vec<String> {
        let mut v = vec![self.genesis_event.clone()];
        v.extend(self.recent.iter().cloned());
        v
    }

    pub fn rate(&self) -> f64 {
        if self.total_cost > 0 { self.total_reward as f64 / self.total_cost as f64 } else { 0.0 }
    }

    pub fn from_events(rows: &[EventRow]) -> StateL {
        let mut st = StateL::default();
        for e in rows.iter().filter(|e| e.source == SOURCE_L) {
            st.fold_event(e);
        }
        st
    }

    pub fn fold_event(&mut self, e: &EventRow) {
        let has = |t: &str| e.tags.split(',').any(|x| x == t);
        if e.kind == "note" && has("wl_genesis") && self.cfg.is_none() {
            let t = |k: &str| tag(&e.tags, k);
            if let (Some(seed), Some(setting), Some(budget), Some(f)) = (t("wl_seed").and_then(|x| x.parse().ok()), t("wl_setting").and_then(super::worldl::Setting::parse), t("wl_budget").and_then(|x| x.parse().ok()), t("wl_families").and_then(|x| x.parse().ok())) {
                self.start(WLCfg { seed, setting, budget }, f, e.event_id.clone());
            }
        } else if e.kind == "decision" {
            if let (Some(step), Some(m)) = (tag(&e.tags, "step").and_then(|x| x.parse().ok()), tag(&e.tags, "menu").and_then(menu_from_tag)) {
                self.note_menu(step, &m);
            }
        } else if e.kind == "outcome" {
            if let Some(o) = outcome_from_event(e) {
                self.apply(&o);
            }
        }
    }
}

pub fn outcome_from_event(e: &EventRow) -> Option<OutcomeL> {
    let t = |k: &str| tag(&e.tags, k);
    Some(OutcomeL {
        step: t("step")?.parse().ok()?,
        fam: t("fam").and_then(|x| x.parse().ok()),
        strategy: t("strategy").and_then(Strategy::parse),
        result: t("result")?.to_string(),
        reward: t("reward")?.parse().ok()?,
        cost: t("cost")?.parse().ok()?,
        listed: t("listed")?.parse().ok()?,
        budget_left: t("budget_left")?.parse().ok()?,
        clock: t("clock")?.parse().ok()?,
        token: t("token")?.parse().ok()?,
        token_gained: t("token_gained") == Some("1"),
        event_id: e.event_id.clone(),
    })
}

// ---- choices ----

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActL {
    Attempt { slot: usize, strategy: Strategy },
    Skip,
}

impl ActL {
    pub fn label(&self) -> String {
        match self {
            ActL::Attempt { slot, strategy } => format!("{slot}:{}", strategy.name()),
            ActL::Skip => "skip".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceL {
    pub act: ActL,
    pub policy: String,
    pub why: String,
    pub p: f64,
    pub score: f64,
    pub rationale: Value,
    /// Simulated evaluations used for this decision (0 for one-step planners).
    pub nodes: u64,
}

pub trait ChooserL {
    fn name(&self) -> String;
    /// `offers` is never empty. None when nothing can be paid.
    fn choose(&mut self, st: &StateL, offers: &[OfferL]) -> Option<ChoiceL>;
    /// Refusals to log (node cap reached).
    fn take_refusals(&mut self) -> Vec<Refusal> {
        vec![]
    }
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// The B1 planner restated for World L (same score, same skip rule, same constants).
#[derive(Debug, Clone)]
pub struct GreedyL {
    pub beta: f64,
    pub skip_frac: f64,
}

impl Default for GreedyL {
    fn default() -> Self {
        GreedyL { beta: DEFAULT_BETA, skip_frac: DEFAULT_SKIP_FRAC }
    }
}

impl GreedyL {
    /// The B1 score of every (offer, strategy): (slot, strategy, score, affordable).
    pub fn scores(&self, st: &StateL, offers: &[OfferL]) -> Vec<(usize, Strategy, f64, bool)> {
        let mut v = vec![];
        for o in offers {
            if o.needs_token && st.token == 0 {
                continue;
            }
            for s in STRATEGIES {
                let (p, sd, ratio, _) = st.estimate(o.fam, s, st.ctx);
                let e = (o.cost as f64 * ratio).max(1.0);
                v.push((o.slot, s, o.reward as f64 * (p + self.beta * sd).min(1.0) / e, e.round() as i64 <= st.budget_left));
            }
        }
        v
    }
}

impl ChooserL for GreedyL {
    fn name(&self) -> String {
        "wl-model-ucb".into()
    }
    fn choose(&mut self, st: &StateL, offers: &[OfferL]) -> Option<ChoiceL> {
        let mut best: Option<(f64, usize, Strategy, f64)> = None;
        let mut alts = vec![];
        for o in offers {
            if o.needs_token && st.token == 0 {
                continue;
            }
            for s in STRATEGIES {
                let (p, sd, ratio, n) = st.estimate(o.fam, s, st.ctx);
                let e = (o.cost as f64 * ratio).max(1.0);
                let score = o.reward as f64 * (p + self.beta * sd).min(1.0) / e;
                let ok = e.round() as i64 <= st.budget_left;
                alts.push(json!({ "action": format!("{}:{}", o.slot, s.name()), "family": o.fam, "context": ctx_name(st.ctx), "n_cell": n, "p_success": r3(p), "uncertainty_sd": r3(sd), "est_energy": r3(e), "score": r3(score), "affordable": ok }));
                if ok && best.map_or(true, |b| score > b.0) {
                    best = Some((score, o.slot, s, p));
                }
            }
        }
        let (score, slot, s, p) = best?;
        let rate = st.rate();
        let skip = self.skip_frac > 0.0 && st.total_cost >= SKIP_MIN_COST && score < self.skip_frac * rate && st.budget_left >= SKIP_COST;
        let rationale = json!({ "policy": self.name(), "planner": "B1 one-step: listed_reward * min(1, p + beta*sd) / estimated energy", "beta": self.beta, "skip_frac": self.skip_frac, "realized_reward_per_energy": r3(rate), "alternatives": alts });
        if skip {
            return Some(ChoiceL { act: ActL::Skip, policy: self.name(), why: format!("best optimistic score {score:.3} below {} x realized rate {rate:.3}: skip", self.skip_frac), p: 0.0, score: 0.0, rationale, nodes: 0 });
        }
        Some(ChoiceL { act: ActL::Attempt { slot, strategy: s }, policy: self.name(), why: "highest optimistic reward per estimated energy".into(), p, score, rationale, nodes: 0 })
    }
}

/// UCB1 over (family, strategy) arms on reward per energy (`EFF_NORM`), untried first (as `Ucb2`).
#[derive(Debug, Clone)]
pub struct Ucb1L {
    pub c: f64,
}

impl ChooserL for Ucb1L {
    fn name(&self) -> String {
        "ucb1-family-strategy".into()
    }
    fn choose(&mut self, st: &StateL, offers: &[OfferL]) -> Option<ChoiceL> {
        let total = st.arms.iter().flatten().map(|a| a.n).sum::<u64>().max(1) as f64;
        let mut best: Option<(f64, f64, usize, Strategy)> = None;
        for o in offers {
            if o.needs_token && st.token == 0 {
                continue;
            }
            for s in STRATEGIES {
                if s.energy(o.cost) > st.budget_left {
                    continue;
                }
                let a = st.arms.get(o.fam).map(|x| x[s.idx()]).unwrap_or_default();
                let score = if a.n == 0 { f64::INFINITY } else { (if a.cost > 0 { a.reward as f64 / a.cost as f64 } else { 0.0 }) / EFF_NORM + self.c * (2.0 * total.ln() / a.n as f64).sqrt() };
                let ratio = o.reward as f64 / s.energy(o.cost) as f64;
                if best.map_or(true, |b| score > b.0 || (score == b.0 && ratio > b.1)) {
                    best = Some((score, ratio, o.slot, s));
                }
            }
        }
        let (score, _, slot, strategy) = best?;
        Some(ChoiceL { act: ActL::Attempt { slot, strategy }, policy: self.name(), why: "baseline: UCB1 over (family, strategy)".into(), p: 0.0, score: if score.is_finite() { score } else { 0.0 }, rationale: json!({ "policy": self.name() }), nodes: 0 })
    }
}

/// The learned model of World L as the lookahead planner reads it.
pub struct SrcL<'a> {
    pub st: &'a StateL,
}

impl<'a> Source for SrcL<'a> {
    fn base(&self, k: Key) -> Base {
        let s = STRATEGIES[(k.strat as usize).min(2)];
        self.st.base(k.fam as usize, s, k.ctx)
    }
    fn strategies(&self) -> &[Strategy] {
        &STRATEGIES
    }
    fn ctx_after(&self, fam: usize, ok: bool) -> u16 {
        ctx_of(fam, ok)
    }
    fn token_p(&self, fam: usize) -> f64 {
        self.st.token_p(fam)
    }
}

pub fn aoffers(m: &[OfferL]) -> Vec<AOffer> {
    m.iter().map(|o| AOffer { slot: o.slot, fam: o.fam, diff: 1, cost: o.cost, reward: o.reward, needs_token: o.needs_token }).collect()
}

/// The lookahead planner in World L; `cfg: None` (lookahead off or not allowed) is exactly `GreedyL`.
pub struct LookL {
    pub cfg: Option<LookCfg>,
    pub fallback: GreedyL,
    refusals: Vec<Refusal>,
    pub truncations: u64,
    pub fallbacks: u64,
}

impl LookL {
    pub fn new(cfg: Option<LookCfg>) -> LookL {
        LookL { cfg, fallback: GreedyL::default(), refusals: vec![], truncations: 0, fallbacks: 0 }
    }
}

pub const LOOK_NAME: &str = "wl-lookahead";

impl ChooserL for LookL {
    fn name(&self) -> String {
        match &self.cfg {
            Some(c) => format!("{LOOK_NAME}-d{}", c.depth),
            None => self.fallback.name(),
        }
    }
    fn choose(&mut self, st: &StateL, offers: &[OfferL]) -> Option<ChoiceL> {
        let Some(cfg) = self.cfg else { return self.fallback.choose(st, offers) };
        let past: Vec<&(u64, Vec<OfferL>)> = st.menus.iter().filter(|m| m.0 < st.step).collect();
        let memory: Vec<(u64, Vec<AOffer>)> = past[past.len().saturating_sub(cfg.memory)..].iter().map(|(s, m)| (*s, aoffers(m))).collect();
        let src = SrcL { st };
        let sim = Sim::new(st.budget_left, st.token, st.ctx);
        let rho0 = if st.total_cost >= SKIP_MIN_COST { st.rate() } else { 0.0 };
        let Some(p) = plan::plan(&src, &cfg, &aoffers(offers), &memory, &sim, st.step, rho0) else {
            self.fallbacks += 1;
            let mut c = self.fallback.choose(st, offers)?;
            c.rationale["lookahead"] = json!("no plan fitted the node cap (or nothing was affordable): the one-step planner decided");
            return Some(c);
        };
        if p.truncated {
            self.truncations += 1;
            self.refusals.push(node_cap_refusal(cfg.max_nodes, cfg.depth));
        }
        let names = |k: &Key| format!("f{} {} {}", k.fam, STRATEGIES[(k.strat as usize).min(2)].name(), ctx_name(k.ctx));
        let mut rationale = p.to_json(&cfg, &names);
        rationale["policy"] = json!(self.name());
        rationale["token_rule"] = json!((0..st.families).map(|f| { let (s, g) = st.token_rule(f); json!({ "family": f, "successes_without_token": s, "tokens_after": g, "p_token": r3(st.token_p(f)) }) }).collect::<Vec<_>>());
        let scores = self.fallback.scores(st, offers);
        let (rate, sf) = (st.rate(), self.fallback.skip_frac);
        let skip_ok = |h: f64| sf > 0.0 && st.total_cost >= SKIP_MIN_COST && h < sf * rate && st.budget_left >= SKIP_COST;
        let Some((dec, score, add)) = plan::decide(&p, &cfg, &scores, &skip_ok) else { return self.fallback.choose(st, offers) };
        rationale["decided"] = json!({ "action": match dec { Some((sl, s)) => format!("{sl}:{}", s.name()), None => "skip".into() }, "score": r3(score), "future_term_per_energy": r3(add) });
        let ch = p.alts.iter().find(|a| a.act == dec).cloned().unwrap_or_else(|| p.chosen().clone());
        let act = match dec {
            Some((slot, strategy)) => ActL::Attempt { slot, strategy },
            None => ActL::Skip,
        };
        let why = format!("lookahead ({:?}) depth {} of {} ({} nodes{}): {} chosen on score {:.3} (future term {:+.3} per energy); simulated value {:+.3} over the leaf (immediate {:.3}, if success {:+.3}, if failure {:+.3}); learned rate {:.3}", cfg.mode, p.depth_done, p.depth_wanted, p.nodes, if p.truncated { ", node cap reached" } else { "" }, ActL::label(&act), score, add, ch.q, ch.immediate, ch.v_ok, ch.v_fail, p.rho);
        let pp = match dec {
            Some((sl, s)) => offers.iter().find(|o| o.slot == sl).map_or(ch.p, |o| st.estimate(o.fam, s, st.ctx).0),
            None => 0.0,
        };
        Some(ChoiceL { act, policy: self.name(), why, p: pp, score, rationale, nodes: p.nodes })
    }
    fn take_refusals(&mut self) -> Vec<Refusal> {
        std::mem::take(&mut self.refusals)
    }
}

pub struct OracleL {
    pub dp: OracleDp,
    bel: usize,
    seen: u64,
}

/// Oracle: knows every hidden parameter (not the phase) and plans exactly (`OracleDp`).
impl OracleL {
    pub fn new(world: WorldL) -> OracleL {
        OracleL { dp: OracleDp::new(world), bel: BEL_U, seen: 0 }
    }
}

/// Tracks the phase belief from the being's observed outcomes (last attempt = the context).
fn track(world: &WorldL, st: &StateL, bel: &mut usize, seen: &mut u64) {
    if st.attempts > *seen && st.ctx > 0 {
        let fam = ((st.ctx - 1) / 2) as usize;
        *bel = belief_after(world, fam, (st.ctx - 1) % 2 == 1, *bel);
    }
    *seen = st.attempts;
}

impl ChooserL for OracleL {
    fn name(&self) -> String {
        "oracle-dp".into()
    }
    fn choose(&mut self, st: &StateL, offers: &[OfferL]) -> Option<ChoiceL> {
        track(&self.dp.world, st, &mut self.bel, &mut self.seen);
        let (v, a) = self.dp.best(offers, st.budget_left, st.token as usize, self.bel);
        if st.budget_left <= 0 {
            return None;
        }
        let act = match a {
            Some((slot, strategy)) => ActL::Attempt { slot, strategy },
            None => ActL::Skip,
        };
        Some(ChoiceL { act, policy: self.name(), why: "knows the hidden parameters; exact dynamic programme".into(), p: 0.0, score: v, rationale: json!({ "policy": "oracle-dp" }), nodes: 0 })
    }
}

/// Oracle without planning: knows the hidden probabilities (with the same phase belief), takes the
/// best expected reward per energy now, never skips (as the B1 menu oracle).
pub struct OracleMyopicL {
    pub world: WorldL,
    bel: usize,
    seen: u64,
}

impl OracleMyopicL {
    pub fn new(world: WorldL) -> OracleMyopicL {
        OracleMyopicL { world, bel: BEL_U, seen: 0 }
    }
}

impl ChooserL for OracleMyopicL {
    fn name(&self) -> String {
        "oracle-one-step".into()
    }
    fn choose(&mut self, st: &StateL, offers: &[OfferL]) -> Option<ChoiceL> {
        track(&self.world, st, &mut self.bel, &mut self.seen);
        let mut best: Option<(f64, usize, Strategy)> = None;
        for o in offers {
            if o.needs_token && st.token == 0 {
                continue;
            }
            for s in STRATEGIES {
                let e = s.energy(o.cost);
                if e > st.budget_left {
                    continue;
                }
                let p = match self.bel {
                    BEL_U => 0.5 * (self.world.p_true(o.fam, s, 0) + self.world.p_true(o.fam, s, 1)),
                    super::worldl::BEL_0 => self.world.p_true(o.fam, s, 0),
                    _ => self.world.p_true(o.fam, s, 1),
                };
                let v = o.reward as f64 * p / e as f64;
                if best.map_or(true, |b| v > b.0) {
                    best = Some((v, o.slot, s));
                }
            }
        }
        let (v, slot, strategy) = match best {
            Some(b) => b,
            None if st.budget_left >= SKIP_COST => return Some(ChoiceL { act: ActL::Skip, policy: self.name(), why: "nothing affordable".into(), p: 0.0, score: 0.0, rationale: json!({}), nodes: 0 }),
            None => return None,
        };
        Some(ChoiceL { act: ActL::Attempt { slot, strategy }, policy: self.name(), why: "knows the hidden probabilities; best expected reward per energy now".into(), p: 0.0, score: v, rationale: json!({ "policy": "oracle-one-step" }), nodes: 0 })
    }
}

// ---- the engine ----

pub struct StepL {
    pub outcome: OutcomeL,
    pub true_ok: bool,
    pub label: String,
    pub nodes: u64,
    pub events: Vec<NewEvent>,
    pub revision: NewRevision,
}

pub struct EngineL {
    pub world: WorldL,
    pub state: StateL,
    /// Hidden phase of the probe setting (never shown to the being).
    phase: u8,
}

pub fn genesis_event_l(cfg: &WLCfg, families: usize) -> (NewEvent, String) {
    let mut tags = vec!["wl_genesis".to_string()];
    tags.extend(cfg.tags());
    tags.push(format!("wl_families:{families}"));
    let text = format!("world L being created: seed {}, setting {}, budget {} energy units, {} families. Simulated world with author-defined rewards (Track B8)", cfg.seed, cfg.setting.name(), cfg.budget, families);
    let ev = NewEvent { source: SOURCE_L.into(), occurred_at: iso_at(0), kind: "note".into(), text, tags, redacted: false };
    let id = event_id(&ev.source, &ev.occurred_at, &ev.text);
    (ev, id)
}

impl EngineL {
    pub fn new(state: StateL) -> EngineL {
        let world = WorldL::new(state.cfg.clone().expect("a started state"));
        let phase = world.first_phase();
        let mut e = EngineL { world, state, phase };
        e.phase = e.replay_phase();
        e
    }

    /// The hidden phase after the steps already in the state (a pure function of the world and the job attempts).
    fn replay_phase(&self) -> u8 {
        let job = self.world.fams.iter().find(|f| f.kind == super::worldl::FKind::Job).map(|f| f.idx);
        match job.and_then(|j| self.state.last_at.get(j).copied().flatten()) {
            Some(t) => self.world.next_phase(t),
            None => self.world.first_phase(),
        }
    }

    pub fn step(&mut self, chooser: &mut dyn ChooserL) -> Result<StepL, StopReason> {
        let step = self.state.step;
        let t0 = self.state.clock;
        let offers = self.world.offers(step);
        let ch = chooser.choose(&self.state, &offers).ok_or(StopReason::BudgetExhausted)?;
        self.state.note_menu(step, &offers);
        let label = ch.act.label();
        let b0 = self.state.budget_left;
        let mut true_ok = false;
        let (fam, strategy, result, reward, cost, listed, mut token, mut gained) = match ch.act {
            ActL::Skip => (None, None, "skip".to_string(), 0, SKIP_COST.min(b0), SKIP_COST.min(b0), self.state.token, false),
            ActL::Attempt { slot, strategy } => {
                let o = offers[slot];
                let e = strategy.energy(o.cost);
                if e > b0 || (o.needs_token && self.state.token == 0) {
                    (Some(o.fam), Some(strategy), "unpaid".to_string(), 0, b0.min(e).max(1).min(b0), o.cost, self.state.token, false)
                } else {
                    let ok = self.world.success(step, slot, o.fam, strategy, self.phase);
                    true_ok = ok;
                    let mut tok = self.state.token;
                    let mut g = false;
                    if o.needs_token {
                        tok = 0;
                    }
                    if ok && self.world.fams[o.fam].kind == super::worldl::FKind::Key && tok == 0 {
                        tok = 1;
                        g = true;
                    }
                    if self.world.fams[o.fam].kind == super::worldl::FKind::Job {
                        self.phase = self.world.next_phase(step);
                    }
                    (Some(o.fam), Some(strategy), if ok { "correct" } else { "wrong" }.to_string(), if ok { o.reward } else { 0 }, e, o.cost, tok, g)
                }
            }
        };
        if result == "unpaid" {
            token = self.state.token;
            gained = false;
        }
        let t1 = t0 + cost * SECS_PER_ENERGY;
        let budget_left = b0 - cost;
        let evidence = self.state.evidence();
        let key = format!("being/worldl/choice/{step:06}");
        let dtext = format!("step {step}: chose {label} by {} ({}); cites {} events", ch.policy, ch.why, evidence.len());
        let dtags = vec![format!("step:{step}"), format!("action:{label}"), format!("policy:{}", ch.policy), format!("rev_key:{key}"), format!("menu:{}", menu_tag(&offers)), format!("nodes:{}", ch.nodes)];
        let dev = NewEvent { source: SOURCE_L.into(), occurred_at: iso_at(t0), kind: "decision".into(), text: dtext, tags: dtags, redacted: false };
        let mut tags = vec![format!("step:{step}")];
        if let Some(f) = fam {
            tags.push(format!("fam:{f}"));
        }
        if let Some(s) = strategy {
            tags.push(format!("strategy:{}", s.name()));
        }
        tags.extend([format!("result:{result}"), format!("reward:{reward}"), format!("cost:{cost}"), format!("listed:{listed}"), format!("budget_left:{budget_left}"), format!("clock:{t1}"), format!("token:{token}"), format!("token_gained:{}", gained as u8)]);
        let oev = NewEvent { source: SOURCE_L.into(), occurred_at: iso_at(t1), kind: "outcome".into(), text: format!("step {step}: {label} is {result}; reward {reward} cost {cost} budget left {budget_left}; tokens held {token}{}", if gained { " (a token was gained)" } else { "" }), tags, redacted: false };
        let oev_id = event_id(&oev.source, &oev.occurred_at, &oev.text);
        let revision = NewRevision { entity_type: "decision".into(), key, stance: format!("choose {label}"), confidence: if ch.p.is_finite() && ch.p > 0.0 { ch.p.clamp(0.0, 1.0) } else { 0.5 }, rationale: ch.rationale.to_string(), supersedes: None, valid_from: iso_at(t0), author: format!("rule:being-{}", ch.policy), caused_by: evidence };
        let outcome = OutcomeL { step, fam, strategy, result, reward, cost, listed, budget_left, clock: t1, token, token_gained: gained, event_id: oev_id };
        self.state.apply(&outcome);
        Ok(StepL { outcome, true_ok, label, nodes: ch.nodes, events: vec![dev, oev], revision })
    }
}

// ---- measurement (no log) ----

#[derive(Debug, Clone, Default)]
pub struct RunL {
    pub reward: i64,
    pub cost: i64,
    pub steps: u64,
    pub skips: u64,
    pub nodes: u64,
    pub max_nodes: u64,
    pub truncations: u64,
    /// Energy spent per family kind: normal, key, vault, probe, job.
    pub kind_energy: [i64; 5],
}

impl RunL {
    pub fn per_energy(&self) -> f64 {
        if self.cost > 0 { self.reward as f64 / self.cost as f64 } else { 0.0 }
    }
}

pub fn kind_idx(k: super::worldl::FKind) -> usize {
    use super::worldl::FKind::*;
    match k {
        Normal => 0,
        Key => 1,
        Vault => 2,
        Probe => 3,
        Job => 4,
    }
}

pub fn fresh_state(cfg: &WLCfg) -> StateL {
    let w = WorldL::new(cfg.clone());
    let mut st = StateL::default();
    st.start(cfg.clone(), w.fams.len(), "genesis".into());
    st
}

/// One life without a log.
pub fn run_wl(cfg: &WLCfg, chooser: &mut dyn ChooserL) -> RunL {
    let mut e = EngineL::new(fresh_state(cfg));
    let mut r = RunL::default();
    while let Ok(o) = e.step(chooser) {
        r.steps += 1;
        r.reward += o.outcome.reward;
        r.cost += o.outcome.cost;
        r.skips += (o.outcome.result == "skip") as u64;
        r.nodes += o.nodes;
        r.max_nodes = r.max_nodes.max(o.nodes);
        if let Some(f) = o.outcome.fam {
            r.kind_energy[kind_idx(e.world.fams[f].kind)] += o.outcome.cost;
        }
        r.truncations += chooser.take_refusals().len() as u64;
    }
    r
}

// ---- the logged run ----

#[derive(Debug, Clone, Default)]
pub struct RunLCfg {
    /// Creation only: the world (must match when the being exists).
    pub world: Option<WLCfg>,
    /// Ask for planning ahead (granted only if controller.txt allows it).
    pub lookahead: bool,
    /// Depth asked for (capped by the controller's `max_lookahead_depth`).
    pub depth: u32,
    /// The text of the owner's controller.txt (None: missing, fail closed).
    pub controller_text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RunLSummary {
    pub steps_run: u64,
    pub stopped: StopReason,
    pub reward: i64,
    pub cost: i64,
    pub state: StateL,
    pub refusals: Vec<Refusal>,
    pub lookahead: Option<(u64, u32)>,
    pub nodes_max: u64,
}

fn refusal_event_l(r: &Refusal, step: u64, clock: i64) -> NewEvent {
    let mut ev = r.event(step, clock);
    ev.source = SOURCE_L.into();
    ev
}

/// Run the World L being on its own log. Gates: controller.txt (missing or unparseable: the run is
/// refused and the refusal logged), the kill switch and the energy cap before every step, the step cap,
/// and the lookahead caps (`ctl.rs`). Every decision is a decision event (with the menu) plus a
/// `decision` revision whose rationale is the plan with its evidence.
pub fn run_l(log: &mut VersionedLog, cfg: &RunLCfg, policy: &Policy) -> Result<RunLSummary, LogError> {
    let rows: Vec<EventRow> = log.open_latest().rows();
    let mut state = StateL::from_events(&rows);
    let ctl = Controller::parse_or_closed(cfg.controller_text.as_deref().unwrap_or(""));
    let ctl = if cfg.controller_text.is_none() { Controller::fail_closed("controller.txt is missing") } else { ctl };
    let look = LookCtl::from_text(cfg.controller_text.as_deref());
    let mut refusals: Vec<Refusal> = vec![];
    if ctl.is_closed() {
        let why = match &ctl.origin {
            crate::being::controller::Origin::FailClosed(r) => r.clone(),
            _ => String::new(),
        };
        let r = Refusal::new("controller_closed", 0, policy.max_steps, format!("the World L run was refused: {why}"));
        log.append_events(&[refusal_event_l(&r, state.step, state.clock)])?;
        return Ok(RunLSummary { steps_run: 0, stopped: StopReason::MaxSteps, reward: 0, cost: 0, state, refusals: vec![r], lookahead: None, nodes_max: 0 });
    }
    let mut pending = Batch::default();
    match &state.cfg {
        Some(c) => {
            if let Some(w) = &cfg.world {
                if w != c {
                    return Err(LogError::Invalid(format!("this World L being already lives in world {:?}; the request says {:?}", c, w)));
                }
            }
        }
        None => {
            let w = cfg.world.clone().ok_or_else(|| LogError::Invalid("no World L being exists yet; a world config is needed".into()))?;
            w.validate().map_err(LogError::Invalid)?;
            let fams = WorldL::new(w.clone()).fams.len();
            let (ev, id) = genesis_event_l(&w, fams);
            log.append_events(&[ev])?;
            state.start(w, fams, id);
        }
    }
    let (grant, mut rf) = look.grant(cfg.lookahead);
    refusals.append(&mut rf);
    let look_cfg = grant.map(|(nodes, depth)| LookCfg::new(cfg.depth.max(1).min(depth), nodes));
    if let (Some(lc), true) = (&look_cfg, cfg.depth > grant.map_or(0, |g| g.1)) {
        refusals.push(Refusal::new(super::ctl::KEY_DEPTH, lc.depth as u64, cfg.depth as u64, format!("depth {} was requested, the controller allows {}; the search is cut to the cap", cfg.depth, lc.depth)));
    }
    let mut chooser = LookL::new(look_cfg);
    let max_steps = policy.max_steps.min(ctl.max_steps_per_run);
    if policy.max_steps > ctl.max_steps_per_run {
        refusals.push(Refusal::new("max_steps_per_run", ctl.max_steps_per_run, policy.max_steps, format!("{} steps were requested, the controller allows {}", policy.max_steps, ctl.max_steps_per_run)));
    }
    let mut pol = policy.clone();
    pol.max_steps = max_steps;
    pol.max_energy = Some(policy.max_energy.unwrap_or(i64::MAX).min(ctl.max_energy_per_run));
    for r in &refusals {
        pending.events.push(refusal_event_l(r, state.step, state.clock));
    }
    let mut engine = EngineL::new(state);
    let (mut done, mut reward, mut cost, mut nodes_max) = (0u64, 0i64, 0i64, 0u64);
    let mut seen_caps: Vec<String> = vec![];
    let stopped;
    loop {
        if let Some(r) = pol.gate_cost(done, cost) {
            stopped = r;
            if matches!(r, StopReason::Killed | StopReason::EnergyCap) {
                let mut ev = crate::being::controller::stop_event(r.as_str(), engine.state.step, engine.state.clock);
                ev.source = SOURCE_L.into();
                pending.events.push(ev);
            }
            break;
        }
        match engine.step(&mut chooser) {
            Ok(out) => {
                done += 1;
                reward += out.outcome.reward;
                cost += out.outcome.cost;
                nodes_max = nodes_max.max(out.nodes);
                pending.events.extend(out.events);
                pending.revisions.push(out.revision);
                for r in chooser.take_refusals() {
                    let k = format!("{}|{}", r.cap, r.what);
                    if !seen_caps.contains(&k) {
                        seen_caps.push(k);
                        pending.events.push(refusal_event_l(&r, engine.state.step, engine.state.clock));
                        refusals.push(r);
                    }
                }
                if pending.revisions.len() >= 25 {
                    log.commit_batch(std::mem::take(&mut pending))?;
                }
            }
            Err(r) => {
                stopped = r;
                break;
            }
        }
    }
    if !pending.events.is_empty() {
        log.commit_batch(pending)?;
    }
    Ok(RunLSummary { steps_run: done, stopped, reward, cost, state: engine.state, refusals, lookahead: grant, nodes_max })
}
