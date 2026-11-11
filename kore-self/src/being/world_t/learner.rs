//! The World T learner: which plan works for which kind of task, learned from the being's own log.
//!
//! The state is folded from the log (`engine::StateT`); this file only reads the folded counts and the
//! visible task view. Per (template, context, plan) it keeps a Beta estimate of the chance that the plan's
//! answer is right, starting from a prior pooled over what it has seen elsewhere (the same context and
//! plan in other templates, then the same template and plan in other contexts, then the plan anywhere;
//! `prior = flat` turns this off); the being's own series rules start from their verification record). It picks the plan with the highest optimistic reward per energy,
//! `min(1, p + beta * sd) x reward / energy`, the B1 planner's rule (with a wrong-answer penalty: the
//! optimistic expected reward of the whole outcome, `min(reward, p x reward - p_wrong x penalty + beta x
//! sd of that mean) / energy`, `value_ucb`), and skips a task when even that
//! is below the renewal bar `rate x (E - 1) / E` (the reward per energy it has realized so far, with
//! the one energy unit a skip costs). Nothing here sees how files are generated: a test scans this file.

use super::engine::{Cell, StateT};
use super::tools::ts_sig;
use super::world::{plans_for, Plan, ViewT, SKIP_COST};

use serde_json::json;

pub const LEARNER_T: &str = "wt-model-ucb";
pub const SKIP_MIN_COST: i64 = 40;
/// Pooled counts needed before a pool is used as the prior.
pub const POOL_MIN: u32 = 3;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct LearnerTCfg {
    pub beta_milli: u32,
    /// Prior pseudo-count (per mille: 2000 = two pseudo-attempts).
    pub prior_w_milli: u32,
    /// true: the pooled (hierarchical) prior; false: a flat prior of 0.5.
    pub pooled: bool,
    pub skip: bool,
    /// true: the optimism bonus is on the expected reward of the whole outcome (right: +reward, wrong:
    /// -penalty, no answer: 0; Dirichlet posterior over the three, the bonus is beta x its posterior sd),
    /// so the uncertain wrong-answer chance is explored at the penalty's scale. false (logs written before
    /// this field, tag `wt_ucb` missing): the bonus is on the right-answer chance only and the penalty
    /// term is a point estimate. Identical when the penalty is 0.
    pub value_ucb: bool,
}

/// Default: the constants chosen on the tuning seeds 101..120 (docs/measured.md, Track W1).
impl Default for LearnerTCfg {
    fn default() -> Self {
        LearnerTCfg { beta_milli: 2000, prior_w_milli: 2000, pooled: true, skip: true, value_ucb: true }
    }
}

impl LearnerTCfg {
    pub fn tags(&self) -> Vec<String> {
        vec![format!("wt_beta_milli:{}", self.beta_milli), format!("wt_prior_w_milli:{}", self.prior_w_milli), format!("wt_prior:{}", if self.pooled { "pooled" } else { "flat" }), format!("wt_skip:{}", if self.skip { "on" } else { "off" }), format!("wt_ucb:{}", if self.value_ucb { "value" } else { "p" })]
    }
    pub fn from_tags(tags: &str) -> Option<LearnerTCfg> {
        let t = |k| crate::being::state::tag(tags, k);
        Some(LearnerTCfg {
            beta_milli: t("wt_beta_milli")?.parse().ok()?,
            prior_w_milli: t("wt_prior_w_milli")?.parse().ok()?,
            pooled: match t("wt_prior")? { "pooled" => true, "flat" => false, _ => return None },
            skip: match t("wt_skip")? { "on" => true, "off" => false, _ => return None },
            value_ucb: match t("wt_ucb") { None | Some("p") => false, Some("value") => true, _ => return None },
        })
    }
}

/// One decision: a plan, or a skip (None).
#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceT {
    pub plan: Option<Plan>,
    pub policy: String,
    pub why: String,
    pub p: f64,
    pub sd: f64,
    pub energy: f64,
    pub score: f64,
    pub rationale: serde_json::Value,
    /// Measurement references only (the oracle): an answer to submit instead of running the plan.
    pub oracle_answer: Option<String>,
}

impl ChoiceT {
    pub fn label(&self) -> String {
        self.plan.map_or("skip".to_string(), |p| p.name().to_string())
    }
    pub fn skip(policy: &str, why: impl Into<String>) -> ChoiceT {
        ChoiceT { plan: None, policy: policy.into(), why: why.into(), p: 0.0, sd: 0.0, energy: SKIP_COST as f64, score: 0.0, rationale: json!({}), oracle_answer: None }
    }
    pub fn plain(policy: &str, plan: Plan, energy: f64, why: impl Into<String>) -> ChoiceT {
        ChoiceT { plan: Some(plan), policy: policy.into(), why: why.into(), p: f64::NAN, sd: f64::NAN, energy, score: f64::NAN, rationale: json!({}), oracle_answer: None }
    }
}

/// Plans the learner may use on a view: the template's plans; `TsRule` only when the being holds a live
/// rule for the series' signature.
pub fn options(st: &StateT, v: &ViewT) -> Vec<Plan> {
    let sig = ts_sig(v);
    plans_for(v.tmpl).iter().copied().filter(|p| *p != Plan::TsRule || st.rules.iter().any(|r| r.sig == sig && !r.retired)).collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Est {
    pub plan: Plan,
    pub n: u32,
    pub succ: u32,
    pub prior: f64,
    pub prior_from: &'static str,
    /// Estimated chance that the plan's answer is wrong (matters only when wrong answers are penalized).
    pub p_wrong: f64,
    pub p: f64,
    pub sd: f64,
    pub energy: f64,
    pub score: f64,
}

pub struct LearnerT {
    pub cfg: LearnerTCfg,
}

impl LearnerT {
    pub fn prior(&self, st: &StateT, v: &ViewT, plan: Plan) -> (f64, &'static str) {
        let (m, _, from) = self.prior2(st, v, plan);
        (m, from)
    }

    /// Prior chance of a right answer and of a wrong answer, and where they come from.
    pub fn prior2(&self, st: &StateT, v: &ViewT, plan: Plan) -> (f64, f64, &'static str) {
        if !self.cfg.pooled {
            return (0.5, 0.25, "flat");
        }
        // a series rule starts from its own verification record (the B7 rule-record idea): its training and
        // held-out examples reproduced, best live rule of the signature
        if plan == Plan::TsRule {
            let sig = ts_sig(v);
            let best = st.rules.iter().filter(|r| r.sig == sig && !r.retired).map(|r| { let k = (r.train_ok + r.val_ok) as f64; (k + 1.0) / (k + 2.0) }).fold(0.0, f64::max);
            if best > 0.0 {
                return (best, (1.0 - best) / 2.0, "rule record");
            }
        }
        let rate = |c: Option<&Cell>| c.filter(|c| c.n >= POOL_MIN).map(|c| ((c.succ as f64 + 0.5) / (c.n as f64 + 1.0), (c.wrong as f64 + 0.25) / (c.n as f64 + 1.0)));
        if let Some((r, w)) = rate(st.by_ctx_plan.get(&(v.ctx.idx() as u8, plan.idx() as u8))) {
            return (r, w, "context+plan");
        }
        if let Some((r, w)) = rate(st.by_tmpl_plan.get(&(v.tmpl.idx() as u8, plan.idx() as u8))) {
            return (r, w, "template+plan");
        }
        if let Some((r, w)) = rate(st.by_plan.get(&(plan.idx() as u8))) {
            return (r, w, "plan");
        }
        (0.5, 0.25, "none")
    }

    pub fn estimate(&self, st: &StateT, v: &ViewT, plan: Plan) -> Est {
        let c = st.cells.get(&(v.tmpl.idx() as u8, v.ctx.idx() as u8, plan.idx() as u8)).copied().unwrap_or_default();
        // energy: listed, except a search whose cost the being has observed
        let energy = if plan == Plan::ColSearch && c.n > 0 { c.spent as f64 / c.n as f64 } else { plan.listed(v) as f64 };
        self.estimate_from(v, plan, c, self.prior2(st, v, plan), energy)
    }

    /// The Beta estimate of one key from its counts `c`, a prior (chance right, chance wrong, source) and the
    /// plan's energy. `estimate` uses the (template, context, plan) cell with the pooled prior; measurement
    /// baselines (the file-keyed table of Track N1 Gate 0) may pass a finer key's counts with the cell's
    /// estimate as the prior.
    pub fn estimate_from(&self, v: &ViewT, plan: Plan, c: Cell, prior: (f64, f64, &'static str), energy: f64) -> Est {
        let (m, mw, from) = prior;
        let w = self.cfg.prior_w_milli as f64 / 1000.0;
        let p_wrong = (c.wrong as f64 + mw * w) / (c.n as f64 + w).max(1e-9);
        let a = m * w + c.succ as f64;
        let b = (1.0 - m) * w + (c.n - c.succ) as f64;
        let p = a / (a + b).max(1e-9);
        let sd = (p * (1.0 - p) / (a + b + 1.0)).sqrt();
        let beta = self.cfg.beta_milli as f64 / 1000.0;
        let (r, pen) = (v.reward as f64, v.penalty());
        let value = if self.cfg.value_ucb && pen > 0.0 {
            // optimistic expected reward of one attempt: the outcome is right (+r), wrong (-pen) or no answer (0)
            // with Dirichlet posterior means p, p_wrong (prior weight w plus the attempts); the posterior variance
            // of the mean reward is Var(outcome) / (w + n + 1); capped at r, the best outcome
            let q = p_wrong.clamp(0.0, 1.0 - p);
            let var = (p * (1.0 - p) * r * r + q * (1.0 - q) * pen * pen + 2.0 * p * q * r * pen) / (a + b + 1.0);
            (p * r - q * pen + beta * var.max(0.0).sqrt()).min(r)
        } else {
            // optimistic chance of a right answer x reward, minus the expected penalty of a wrong one
            (p + beta * sd).min(1.0) * r - p_wrong * pen
        };
        Est { plan, n: c.n, succ: c.succ, prior: m, prior_from: from, p_wrong, p, sd, energy, score: value / energy.max(1.0) }
    }

    pub fn choose(&self, st: &StateT, v: &ViewT) -> ChoiceT {
        self.choose_with(st, v, |p| self.estimate(st, v, p))
    }

    /// The chooser (score, optimism, skip test) over the estimates `est` gives for the plans on offer.
    pub fn choose_with(&self, st: &StateT, v: &ViewT, est: impl Fn(Plan) -> Est) -> ChoiceT {
        let opts = options(st, v);
        let ests: Vec<Est> = opts.iter().map(|p| est(*p)).collect();
        let mut bi = 0;
        for (i, e) in ests.iter().enumerate() {
            if e.score > ests[bi].score {
                bi = i;
            }
        }
        let rate = if st.total_cost > 0 { st.total_reward as f64 / st.total_cost as f64 } else { 0.0 };
        let best = ests[bi];
        let bar = rate * (best.energy - SKIP_COST as f64).max(0.0) / best.energy.max(1.0);
        let r3 = |x: f64| (x * 1000.0).round() / 1000.0;
        let rationale = json!({
            "template": v.tmpl.name(), "context": v.ctx.name(), "rows": v.rows, "listed_reward": v.reward,
            "options": ests.iter().map(|e| json!({ "plan": e.plan.name(), "attempts": e.n, "right": e.succ, "prior": r3(e.prior), "prior_from": e.prior_from, "p": r3(e.p), "sd": r3(e.sd), "energy": r3(e.energy), "score": r3(e.score) })).collect::<Vec<_>>(),
            "realized_rate": r3(rate), "skip_bar": r3(bar), "beta": self.cfg.beta_milli as f64 / 1000.0,
        });
        if self.cfg.skip && st.total_cost >= SKIP_MIN_COST && best.score < bar && st.budget_left >= SKIP_COST {
            return ChoiceT {
                plan: None,
                policy: LEARNER_T.into(),
                why: format!("best optimistic score {:.3} ({}) is below the skip bar {:.3} (realized rate {:.3} x (energy {:.2} - 1) / energy): skip", best.score, best.plan.name(), bar, rate, best.energy),
                p: 0.0,
                sd: 0.0,
                energy: SKIP_COST as f64,
                score: 0.0,
                rationale,
                oracle_answer: None,
            };
        }
        ChoiceT {
            plan: Some(best.plan),
            policy: LEARNER_T.into(),
            why: format!("highest optimistic reward per energy: p {:.3} + beta x sd {:.3} (from {} attempts, prior {:.3} from {}) x reward {} / energy {:.2}", best.p, best.sd, best.n, best.prior, best.prior_from, v.reward, best.energy),
            p: best.p,
            sd: best.sd,
            energy: best.energy,
            score: best.score,
            rationale,
            oracle_answer: None,
        }
    }
}
