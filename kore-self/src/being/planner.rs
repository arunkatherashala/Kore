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
//! global exploration decay it only keeps exploring cells that are both
//! uncertain and potentially valuable.
//!
//! Skipping costs `SKIP_COST` energy and earns nothing. The planner skips when
//! the best optimistic score is below `skip_frac` times the reward per energy
//! the being has actually earned so far (after `SKIP_MIN_COST` energy spent);
//! `skip_frac = 0` never skips.
//!
//! ## Advice (step B4)
//!
//! When the log holds advice for the step (`st.advice`), the advised (offer, strategy) is a
//! candidate like any other and the planner combines the advice with its own model:
//!   * `blind` (a being created with `trust: false`): the advised option is chosen whenever it can be
//!     paid; the skip rule does not apply;
//!   * `trust`: with the optimistic trust weight `w` of the advisor for the advised option's domain
//!     (`trust.rs`) and `M` the best own score among affordable options, the advised option's score
//!     becomes `s' = s + w * ((1 + LIFT) * M - s)`; all other scores stay. The option with the
//!     highest mixed score is chosen; the skip rule is applied to that mixed score. So `w = 0` is the
//!     being's own decision and `w` near 1 follows the advice unless the own model puts the advised
//!     option far below the best. Advice for the option the model already prefers changes nothing.
//! The decision records what the model alone would have chosen, the reward per energy it predicted
//! for that choice (`ref`, the yardstick of the trust test), the weight and whether advice was
//! followed. Without advice (or with mode `off`) every number is the B1 number bit for bit.
//!
//! Only observable things are used: the offer's domain, difficulty, listed
//! cost and listed reward, and the log-derived state. This file must not read
//! hidden world parameters; a test scans for that.

use super::advice::AdviceTrace;
use super::model::AdaptMode;
use super::state::Genesis;
use super::trust::{DEFAULT_TRUST_BETA, LIFT};
use super::solver::{Strategy, STRATEGIES, STRATEGIES_EXT};
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
    /// What was done with the advice of this step, if there was some.
    pub advice: Option<AdviceTrace>,
}

pub trait MenuChooser {
    fn name(&self) -> String;
    /// `offers` is never empty. Affordability is the chooser's job; return
    /// `None` when nothing can be paid.
    fn choose(&mut self, st: &BeingState, offers: &[Offer]) -> Option<Choice>;
}

/// How the planner treats advice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdviceMode {
    /// Advice is ignored (the B1 learner).
    Off,
    /// Follow advice whenever it can be paid.
    Blind,
    /// Mix advice with the own model by learned trust.
    Trust,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdviceCfg {
    pub mode: AdviceMode,
    /// Optimism of the trust weight, in posterior standard deviations.
    pub trust_beta: f64,
    pub lift: f64,
}

impl Default for AdviceCfg {
    fn default() -> Self {
        AdviceCfg { mode: AdviceMode::Off, trust_beta: DEFAULT_TRUST_BETA, lift: LIFT }
    }
}

impl AdviceCfg {
    pub fn for_genesis(g: &Genesis) -> AdviceCfg {
        let mode = if g.advice.is_none() {
            AdviceMode::Off
        } else if g.advice.trust {
            AdviceMode::Trust
        } else {
            AdviceMode::Blind
        };
        AdviceCfg { mode, ..AdviceCfg::default() }
    }
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
    /// Measurement control: never consider the `discovered` strategy even where it exists.
    pub no_discovered: bool,
    /// Extended pattern world only: extra optimism (in success probability) for a pattern offer whose
    /// signature has fewer than `EXAMPLE_TARGET` revealed examples in the log, because every attempt
    /// at it adds an example the rule search can learn from. 0 = off.
    pub curiosity: f64,
    /// Cold-start bonus for unseen (domain, strategy) pairs; used only when the model is in mode `Full`.
    pub novelty: f64,
    /// How advice found in the log is used (default: ignored).
    pub advice: AdviceCfg,
}

/// Revealed examples per signature after which the curiosity bonus is gone.
pub const EXAMPLE_TARGET: u64 = 24;
/// A (domain, strategy) group with less effective evidence than this counts as unseen.
pub const NOVEL_BELOW: f64 = 1.0;
/// Chosen on tuning seeds 101..120 only (see docs/measured.md).
pub const DEFAULT_CURIOSITY: f64 = 0.0;
/// Cold start (adaptive learner only): extra optimism, in success probability, for a (domain, strategy)
/// pair the model has (almost) no evidence for, so a domain that appears for the first time is tried.
/// Chosen on tuning seeds 101..120 only (see docs/measured.md).
pub const DEFAULT_NOVELTY: f64 = 0.25;

impl ModelChooser {
    pub fn new(beta: f64) -> ModelChooser {
        ModelChooser { beta, skip_frac: DEFAULT_SKIP_FRAC, no_discovered: false, curiosity: DEFAULT_CURIOSITY, novelty: DEFAULT_NOVELTY, advice: AdviceCfg::default() }
    }

    /// The planner of a logged being: its optimism from the genesis event and its advice mode.
    pub fn for_genesis(g: &Genesis) -> ModelChooser {
        ModelChooser { advice: AdviceCfg::for_genesis(g), ..ModelChooser::new(g.explore_c) }
    }

    /// Strategies on offer: three in the standard menu, four (with `discovered`) when the being was created with extended pattern families.
    fn strategies(&self, st: &BeingState) -> &'static [Strategy] {
        if !self.no_discovered && st.genesis.as_ref().map_or(false, |g| g.extended) {
            &STRATEGIES_EXT
        } else {
            &STRATEGIES
        }
    }

    pub fn candidates(&self, st: &BeingState, offers: &[Offer]) -> Vec<Cand> {
        let mut v = vec![];
        let full = st.model.mode() == AdaptMode::Full;
        let discovering = !self.no_discovered && st.genesis.as_ref().map_or(false, |g| g.extended);
        let mut seen = [0u64; 6];
        if discovering {
            for e in &st.examples {
                seen[(e.sig as usize).min(5)] += 1;
            }
        }
        for o in offers {
            let cur = if discovering && o.domain == Domain::Pattern && self.curiosity > 0.0 {
                self.curiosity * (1.0 - seen[(o.difficulty as usize).min(5)] as f64 / EXAMPLE_TARGET as f64).max(0.0)
            } else {
                0.0
            };
            for &s in self.strategies(st) {
                let e = st.model.estimate(o.domain, o.difficulty, s);
                let cost_est = (o.listed_cost as f64 * e.cost_ratio).max(1.0);
                let novel = if full && e.group_n < NOVEL_BELOW { self.novelty } else { 0.0 };
                let pu = (e.p + self.beta * e.sd).min(1.0) + cur + novel;
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
        let mut best_i: Option<usize> = None;
        for (i, c) in cands.iter().enumerate() {
            if c.affordable && best_i.map_or(true, |b| c.score > cands[b].score) {
                best_i = Some(i);
            }
        }
        let best_i = best_i?;
        let best = &cands[best_i];
        let rate = match st.model.recent_rate() {
            Some(r) => r,
            None => if st.total_cost > 0 { st.total_reward as f64 / st.total_cost as f64 } else { 0.0 },
        };
        let skip_ok = |score: f64| self.skip_frac > 0.0 && st.total_cost >= SKIP_MIN_COST && score < self.skip_frac * rate && st.budget_left >= super::world::SKIP_COST;
        let label = |c: &Cand| format!("{}:{}", c.slot, c.strategy.name());
        let alone_skip = skip_ok(best.score);
        let (mut chosen, mut skip, mut shown_score) = (best_i, alone_skip, best.score);
        let mut trace: Option<AdviceTrace> = None;
        if self.advice.mode != AdviceMode::Off {
            if let Some(adv) = st.advice.as_ref() {
                let mut tr = AdviceTrace {
                    id: adv.id.clone(),
                    advisor: adv.advisor.clone(),
                    mode: if self.advice.mode == AdviceMode::Blind { "blind" } else { "trust" },
                    advised: "-".into(),
                    valid: false,
                    alone: if alone_skip { "skip".into() } else { label(best) },
                    ref_score: AdviceTrace::round6(best.mean_score),
                    agree: true,
                    followed: false,
                    w: 0.0,
                    trust_mean: 0.0,
                    trust_sd: 0.0,
                };
                let mut advised_i = None;
                if let Some(m) = &adv.menu {
                    tr.advised = format!("{}:{}", m.slot, m.strategy.name());
                    advised_i = cands.iter().position(|c| c.slot == m.slot && c.strategy == m.strategy && c.affordable);
                    if let Some(ai) = advised_i {
                        tr.valid = true;
                        if self.advice.mode == AdviceMode::Blind {
                            chosen = ai;
                            skip = false;
                            shown_score = cands[ai].score;
                            tr.w = 1.0;
                        } else {
                            let (w, mean, sd) = st.trust.weight(&adv.advisor, m.domain.name(), self.advice.trust_beta);
                            (tr.w, tr.trust_mean, tr.trust_sd) = (w, mean, sd);
                            if w > 0.0 {
                                let top = best.score;
                                let mixed = |i: usize, c: &Cand| if i == ai { c.score + w * ((1.0 + self.advice.lift) * top - c.score) } else { c.score };
                                let mut bi = best_i;
                                let mut bs = mixed(best_i, best);
                                for (i, c) in cands.iter().enumerate() {
                                    if c.affordable && mixed(i, c) > bs {
                                        bi = i;
                                        bs = mixed(i, c);
                                    }
                                }
                                chosen = bi;
                                shown_score = bs;
                                skip = skip_ok(bs);
                            }
                        }
                    }
                }
                tr.followed = advised_i == Some(chosen) && !skip;
                let final_label = if skip { "skip".to_string() } else { label(&cands[chosen]) };
                tr.agree = final_label == tr.alone;
                trace = Some(tr);
            }
        }
        let best = &cands[chosen];
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
        let mut rationale = json!({
            "policy": self.name(), "beta": self.beta, "skip_frac": self.skip_frac, "curiosity": self.curiosity, "realized_reward_per_energy": round3(rate),
            "attempts_in_model": st.model.attempts(), "alternatives": alts,
        });
        if st.model.adaptive() {
            rationale["adapt"] = json!(st.model.mode().name());
            rationale["novelty_bonus"] = json!(if st.model.mode() == AdaptMode::Full { self.novelty } else { 0.0 });
            rationale["changes_flagged_so_far"] = json!(st.changes.len());
        }
        if let Some(t) = &trace {
            rationale["advice"] = t.to_json();
            rationale["advice_mixing"] = json!(if self.advice.mode == AdviceMode::Blind {
                "blind: the advised option is chosen whenever it can be paid".to_string()
            } else {
                format!("trust: advised score s' = s + w * ((1 + {}) * M - s), M = best own score, w = clamp((mean + {} * sd - 0.5) / 0.5, 0, 1)", self.advice.lift, self.advice.trust_beta)
            });
        }
        let changed = trace.as_ref().map_or(false, |t| !t.agree);
        if skip {
            return Some(Choice {
                action: Action::Skip,
                policy: self.name(),
                why: format!("best optimistic score {:.3} is below {} x realized rate {:.3}: skip the menu", shown_score, self.skip_frac, rate),
                p: 0.0,
                sd: 0.0,
                cost_est: super::world::SKIP_COST as f64,
                score: 0.0,
                rationale,
                advice: trace,
            });
        }
        let why = match (&trace, changed) {
            (Some(t), true) => format!("advice {} moved the choice from {} to {} (mode {}, weight {:.2}); own optimistic score of the chosen option {:.3}", &t.id[..t.id.len().min(10)], t.alone, label(best), t.mode, t.w, best.score),
            _ => "highest optimistic reward per estimated energy (p + beta*sd) among affordable options".into(),
        };
        Some(Choice {
            action: Action::Attempt { slot: best.slot, strategy: best.strategy },
            policy: self.name(),
            why,
            p: best.p,
            sd: best.sd,
            cost_est: best.cost_est,
            score: best.score,
            rationale,
            advice: trace,
        })
    }
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}
