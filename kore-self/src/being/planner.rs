//! The planner (Track B, step B1): which offered task to attempt, with which
//! strategy, or to skip the menu.
//!
//! Method: UCB on the learned model. For every (offered task, strategy) the
//! score is
//!
//!   `listed_reward * min(1, p + beta * sd) / estimated_energy`
//!
//! with `p`, `sd` and the cost ratio from `model.rs`. Why UCB and not Thompson
//! sampling: it needs no random numbers, so the decision is a pure function of
//! the log (replay is exact without seeding a generator from the log), and the
//! optimism term is printed with every alternative. The bonus `beta * sd`
//! decays with evidence because `sd` shrinks as cells fill up; unlike a
//! global exploration schedule it only keeps exploring cells that are both
//! uncertain and potentially valuable.
//!
//! Skipping costs `SKIP_COST` energy and earns nothing. The planner skips when
//! the best optimistic score is below `skip_frac` times the reward per energy
//! the being has actually earned so far (after `SKIP_MIN_COST` energy spent);
//! `skip_frac = 0` never skips.
//!
//! Only observable things are used: the offer's domain, difficulty, listed
//! cost and listed reward, and the log-derived state. This file must not read
//! hidden world parameters; a test scans for that.

use super::solver::{Strategy, STRATEGIES};
use super::state::BeingState;
use super::world::Domain;
use serde_json::{json, Value};

/// What the world shows about one candidate task.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Offer {
    pub slot: usize,
    pub domain: Domain,
    pub difficulty: u8,
    pub listed_cost: i64,
    pub listed_reward: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Attempt { slot: usize, strategy: Strategy },
    Skip,
}

impl Action {
    /// Stable text form used in logs and replay comparison.
    pub fn label(&self) -> String {
        match self {
            Action::Attempt { slot, strategy } => format!("{slot}:{}", strategy.name()),
            Action::Skip => "skip".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub action: Action,
    pub policy: String,
    pub why: String,
    /// Model estimate of the chosen option (zero for baselines and skips).
    pub p: f64,
    pub sd: f64,
    pub cost_est: f64,
    pub score: f64,
    /// Full reasons (all alternatives) as JSON, stored in the decision revision.
    pub rationale: Value,
}

pub trait MenuChooser {
    fn name(&self) -> String;
    /// `offers` is never empty. Affordability is the chooser's job; return
    /// `None` when nothing can be paid.
    fn choose(&mut self, st: &BeingState, offers: &[Offer]) -> Option<Choice>;
}

/// Chosen on tuning seeds 101..120 only (see docs/measured.md).
pub const DEFAULT_BETA: f64 = 1.5;
pub const DEFAULT_SKIP_FRAC: f64 = 1.0;
pub const SKIP_MIN_COST: i64 = 40;

#[derive(Debug, Clone)]
pub struct Cand {
    pub slot: usize,
    pub strategy: Strategy,
    pub n: u64,
    pub p: f64,
    pub sd: f64,
    pub cost_est: f64,
    pub score: f64,
    pub mean_score: f64,
    pub affordable: bool,
}

#[derive(Debug, Clone)]
pub struct ModelChooser {
    pub beta: f64,
    pub skip_frac: f64,
}

impl ModelChooser {
    pub fn new(beta: f64) -> ModelChooser {
        ModelChooser { beta, skip_frac: DEFAULT_SKIP_FRAC }
    }

    pub fn candidates(&self, st: &BeingState, offers: &[Offer]) -> Vec<Cand> {
        let mut v = vec![];
        for o in offers {
            for s in STRATEGIES {
                let e = st.model.estimate(o.domain, o.difficulty, s);
                let cost_est = (o.listed_cost as f64 * e.cost_ratio).max(1.0);
                let pu = (e.p + self.beta * e.sd).min(1.0);
                v.push(Cand {
                    slot: o.slot,
                    strategy: s,
                    n: e.n,
                    p: e.p,
                    sd: e.sd,
                    cost_est,
                    score: o.listed_reward as f64 * pu / cost_est,
                    mean_score: o.listed_reward as f64 * e.p / cost_est,
                    affordable: cost_est.round() as i64 <= st.budget_left,
                });
            }
        }
        v
    }
}

impl MenuChooser for ModelChooser {
    fn name(&self) -> String {
        "model-ucb".into()
    }

    fn choose(&mut self, st: &BeingState, offers: &[Offer]) -> Option<Choice> {
        let cands = self.candidates(st, offers);
        // first best in (slot, strategy) order wins ties
        let mut best: Option<&Cand> = None;
        for c in cands.iter().filter(|c| c.affordable) {
            if best.map_or(true, |b| c.score > b.score) {
                best = Some(c);
            }
        }
        let best = best?;
        let rate = if st.total_cost > 0 { st.total_reward as f64 / st.total_cost as f64 } else { 0.0 };
        let skip = self.skip_frac > 0.0 && st.total_cost >= SKIP_MIN_COST && best.score < self.skip_frac * rate && st.budget_left >= super::world::SKIP_COST;
        let alts: Vec<Value> = cands
            .iter()
            .map(|c| {
                let o = &offers[c.slot];
                json!({
                    "slot": c.slot, "domain": o.domain.name(), "difficulty": o.difficulty, "strategy": c.strategy.name(),
                    "listed_cost": o.listed_cost, "listed_reward": o.listed_reward,
                    "n_cell": c.n, "p_success": round3(c.p), "uncertainty_sd": round3(c.sd), "est_energy": round3(c.cost_est),
                    "mean_reward_per_energy": round3(c.mean_score), "score": round3(c.score), "affordable": c.affordable,
                })
            })
            .collect();
        let rationale = json!({
            "policy": self.name(), "beta": self.beta, "skip_frac": self.skip_frac, "realized_reward_per_energy": round3(rate),
            "attempts_in_model": st.model.attempts(), "alternatives": alts,
        });
        if skip {
            return Some(Choice {
                action: Action::Skip,
                policy: self.name(),
                why: format!("best optimistic score {:.3} is below {} x realized rate {:.3}: skip the menu", best.score, self.skip_frac, rate),
                p: 0.0,
                sd: 0.0,
                cost_est: super::world::SKIP_COST as f64,
                score: 0.0,
                rationale,
            });
        }
        Some(Choice {
            action: Action::Attempt { slot: best.slot, strategy: best.strategy },
            policy: self.name(),
            why: "highest optimistic reward per estimated energy (p + beta*sd) among affordable options".into(),
            p: best.p,
            sd: best.sd,
            cost_est: best.cost_est,
            score: best.score,
            rationale,
        })
    }
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}
