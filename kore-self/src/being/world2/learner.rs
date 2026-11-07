//! The decision types of World v2 and the existing learner (the B1 planner on the family-keyed B1 model).
//!
//! `Learner2` is `planner::ModelChooser` (mode `Off`, no advisor, curiosity 0) restated over
//! families: for every (offer, strategy) the score is
//! `listed_reward * min(1, p + beta * sd) / estimated_energy`, the highest affordable score wins
//! (first in (slot, strategy) order on ties), and the menu is skipped when that score is below
//! `skip_frac` times the reward per energy the being has been told it earned (after 40 energy).
//! A test checks that it chooses exactly what `ModelChooser` chooses on the same observations.
//! It uses only observable offer features and the log-derived state.

use super::engine::State2;
use super::rules_use::{covered, rule_estimate, RuleEst, UseCfg};
use super::world::FType;
use crate::being::planner::{DEFAULT_BETA, DEFAULT_SKIP_FRAC, SKIP_MIN_COST};
use crate::being::solver::{Strategy, STRATEGIES_EXT};
use serde_json::{json, Value};

/// What the being sees of one offer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Offer2 {
    pub slot: usize,
    pub fam: usize,
    pub ftype: FType,
    pub difficulty: u8,
    pub listed_cost: i64,
    pub listed_reward: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Act2 {
    Attempt { slot: usize, strategy: Strategy },
    /// Oracle only: answer with the checker's own answer (at the listed energy, careful slips).
    Oracle { slot: usize },
    Skip,
}

impl Act2 {
    pub fn label(&self) -> String {
        match self {
            Act2::Attempt { slot, strategy } => format!("{slot}:{}", strategy.name()),
            Act2::Oracle { slot } => format!("{slot}:oracle"),
            Act2::Skip => "skip".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Choice2 {
    pub act: Act2,
    pub policy: String,
    pub why: String,
    pub p: f64,
    pub sd: f64,
    pub cost_est: f64,
    pub score: f64,
    pub rationale: Value,
}

pub trait Chooser2 {
    fn name(&self) -> String;
    /// `offers` is never empty; None when nothing can be paid.
    fn choose(&mut self, st: &State2, offers: &[Offer2]) -> Option<Choice2>;
    /// World O only (Track B12): called with the World O state before `choose`; a no-op for every other chooser.
    fn prepare_o(&mut self, _st: &crate::being::world_o::engine::StateO) {}
}

pub fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

pub fn plain(name: &str, why: &str, act: Act2, p: f64, score: f64) -> Choice2 {
    Choice2 { act, policy: name.into(), why: why.into(), p, sd: 0.0, cost_est: 0.0, score, rationale: json!({ "policy": name }) }
}

// ---- the existing learner ----

#[derive(Debug, Clone)]
pub struct Learner2 {
    pub beta: f64,
    pub skip_frac: f64,
    /// Track B7 switches (`rules_use.rs`); off by default. Only `rule_est` changes what the planner computes.
    pub use_: UseCfg,
}

impl Default for Learner2 {
    fn default() -> Self {
        Learner2 { beta: DEFAULT_BETA, skip_frac: DEFAULT_SKIP_FRAC, use_: UseCfg::default() }
    }
}

pub const LEARNER_NAME: &str = "w2-model-ucb";

#[derive(Debug, Clone)]
pub struct Cand2 {
    pub slot: usize,
    pub strategy: Strategy,
    pub n: u64,
    pub p: f64,
    pub sd: f64,
    pub cost_est: f64,
    pub score: f64,
    pub mean_score: f64,
    pub affordable: bool,
    /// B7 A: the rule record the estimate of `discovered` came from.
    pub rule: Option<RuleEst>,
}

impl Learner2 {
    pub fn candidates(&self, st: &State2, offers: &[Offer2]) -> Vec<Cand2> {
        let mut v = vec![];
        for o in offers {
            for &s in STRATEGIES_EXT.iter() {
                if self.use_.rule_only && s == Strategy::Discovered && !covered(st, o) {
                    continue;
                }
                let mut e = st.model.estimate(o.fam, o.difficulty, s);
                let mut rule = None;
                if self.use_.rule_est && s == Strategy::Discovered {
                    if let Some(re) = rule_estimate(st, o, self.use_.rule_k, &e) {
                        e = re.est;
                        rule = Some(re);
                    }
                }
                let cost_est = (o.listed_cost as f64 * e.cost_ratio).max(1.0);
                let pu = (e.p + self.beta * e.sd).min(1.0);
                v.push(Cand2 {
                    slot: o.slot,
                    strategy: s,
                    n: e.n,
                    p: e.p,
                    sd: e.sd,
                    cost_est,
                    score: o.listed_reward as f64 * pu / cost_est,
                    mean_score: o.listed_reward as f64 * e.p / cost_est,
                    affordable: cost_est.round() as i64 <= st.budget_left,
                    rule,
                });
            }
        }
        v
    }
}

impl Chooser2 for Learner2 {
    fn name(&self) -> String {
        LEARNER_NAME.into()
    }
    fn choose(&mut self, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
        let cands = self.candidates(st, offers);
        let mut best_i: Option<usize> = None;
        for (i, c) in cands.iter().enumerate() {
            if c.affordable && best_i.map_or(true, |b| c.score > cands[b].score) {
                best_i = Some(i);
            }
        }
        let best = &cands[best_i?];
        let rate = if st.total_cost > 0 { st.total_reward as f64 / st.total_cost as f64 } else { 0.0 };
        let skip = self.skip_frac > 0.0 && st.total_cost >= SKIP_MIN_COST && best.score < self.skip_frac * rate && st.budget_left >= super::world::SKIP_COST;
        let alts: Vec<Value> = cands
            .iter()
            .map(|c| {
                let o = offers.iter().find(|o| o.slot == c.slot).expect("candidate of an offer");
                let mut a = json!({
                    "slot": c.slot, "family": o.fam, "type": o.ftype.name(), "difficulty": o.difficulty, "strategy": c.strategy.name(),
                    "listed_cost": o.listed_cost, "listed_reward": o.listed_reward, "n_cell": c.n, "p_success": r3(c.p), "uncertainty_sd": r3(c.sd),
                    "est_energy": r3(c.cost_est), "mean_reward_per_energy": r3(c.mean_score), "score": r3(c.score), "affordable": c.affordable,
                });
                if let Some(r) = &c.rule {
                    a["estimate_from_rule"] = json!({
                        "rule_id": r.rule_id, "rule_event": r.event_id, "support": r.support, "missed": r.missed, "answered_since": r.post_n, "reported_right_since": r.post_succ,
                        "prior_p": r3(r.prior_p), "prior_weight": r3(r.prior_k),
                    });
                }
                a
            })
            .collect();
        let rationale = json!({
            "policy": LEARNER_NAME, "learner": "B1 world model (model.rs, mode off) keyed by family, B1 planner (planner.rs)", "beta": self.beta, "skip_frac": self.skip_frac,
            "reported_reward_per_energy": r3(rate), "attempts_in_model": st.model.attempts(), "alternatives": alts,
        });
        let mut rationale = rationale;
        if self.use_.any() {
            rationale["rules_in_use"] = json!({ "rule_estimate": self.use_.rule_est, "robust_milli": self.use_.robust_milli, "fitted_constants": self.use_.fit_const, "rule_prior_weight_cap": self.use_.rule_k });
        }
        if skip {
            return Some(Choice2 {
                act: Act2::Skip,
                policy: LEARNER_NAME.into(),
                why: format!("best optimistic score {:.3} is below {} x reported rate {:.3}: skip the menu", best.score, self.skip_frac, rate),
                p: 0.0,
                sd: 0.0,
                cost_est: super::world::SKIP_COST as f64,
                score: 0.0,
                rationale,
            });
        }
        Some(Choice2 {
            act: Act2::Attempt { slot: best.slot, strategy: best.strategy },
            policy: LEARNER_NAME.into(),
            why: "highest optimistic reward per estimated energy (p + beta*sd) among affordable options".into(),
            p: best.p,
            sd: best.sd,
            cost_est: best.cost_est,
            score: best.score,
            rationale,
        })
    }
}

