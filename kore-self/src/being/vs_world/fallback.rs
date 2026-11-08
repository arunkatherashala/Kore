//! Track B13 baseline "rules-or-LinUCB" (World v2 and World O): the being's own rule estimate where an
//! accepted rule covers the offer, the V1-tuned LinUCB everywhere else. The simple alternative to the B13
//! chooser: it uses the same rule book and the same B7 rule-record estimate, but no B1 model and no
//! B13 estimate for the fixed strategies.
//!
//! * Rule options: for an offer an accepted rule (or fitted constant) covers, the option `discovered` at
//!   the listed energy, scored `listed reward x min(1, p + beta x sd) / listed energy` with the B7 A
//!   rule-record posterior (`rules_use::rule_estimate`) and the B1 optimism `beta`.
//! * Every other option: LinUCB's own score (`core::LinUcb::score`); LinUCB learns only from the outcomes
//!   of the options it chose (a rule answer is not a LinUCB arm).
//! * The highest score wins (first on ties); the menu is skipped with LinUCB's tuned skip rule (best
//!   score below `skip_frac` x the reward per energy the being was told, after 40 energy).
//!
//! Only visible offer features, the being's own rule book and its reported outcomes are used.

use super::adapt::{ctx2, opts2};
use super::core::{Agent, Ctx, LinUcb, Opt, SKIP_MIN_COST};
use crate::being::planner::DEFAULT_BETA;
use crate::being::solver::Strategy;
use crate::being::world2::engine::State2;
use crate::being::world2::learner::{r3, Act2, Choice2, Chooser2, Offer2};
use crate::being::world2::rules_use::{covered, rule_estimate, DEFAULT_RULE_K};
use serde_json::json;

pub const FALLBACK_NAME: &str = "rules-or-linucb";

struct Pend {
    cx: Ctx,
    opt: Opt,
    reward0: i64,
    cost0: i64,
}

pub struct RulesOrLin {
    pub lin: LinUcb,
    pub beta: f64,
    pending: Option<Pend>,
    counts: Vec<u64>,
    /// Decisions taken by a rule estimate / by LinUCB / skips.
    pub by_rule: u64,
    pub by_lin: u64,
    pub skips: u64,
}

impl RulesOrLin {
    /// `alpha`, `skip_frac`: LinUCB's tuned constants (V1: 0.5 and 1 in World v2 and World O).
    pub fn new(alpha: f64, skip_frac: f64) -> RulesOrLin {
        RulesOrLin { lin: LinUcb::new(alpha, skip_frac), beta: DEFAULT_BETA, pending: None, counts: vec![], by_rule: 0, by_lin: 0, skips: 0 }
    }
}

impl Chooser2 for RulesOrLin {
    fn name(&self) -> String {
        FALLBACK_NAME.into()
    }
    fn choose(&mut self, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
        let cx = ctx2(st);
        if let Some(p) = self.pending.take() {
            self.lin.observe(&p.cx, &p.opt, (st.total_reward - p.reward0) as f64, (st.total_cost - p.cost0) as f64, &cx);
        }
        for (fam, a) in st.arms.iter().enumerate() {
            let n: u64 = a.iter().map(|x| x.n).sum();
            if fam >= self.counts.len() {
                self.counts.resize(fam + 1, 0);
            }
            if n < self.counts[fam] {
                self.lin.reset_fam(fam);
            }
            self.counts[fam] = n;
        }
        // (score, from rule?, index into opts or offers, p)
        let opts = opts2(offers, st.budget_left);
        let mut best: Option<(f64, bool, usize, f64)> = None;
        for (i, o) in opts.iter().enumerate().filter(|(_, o)| !o.is_skip()) {
            let (s, mean, _) = self.lin.score(&cx, o);
            if best.map_or(true, |b| s > b.0) {
                best = Some((s, false, i, mean));
            }
        }
        let mut rule_info = None;
        for (i, o) in offers.iter().enumerate() {
            if o.listed_cost > st.budget_left || !covered(st, o) {
                continue;
            }
            let cell = st.model.estimate(o.fam, o.difficulty, Strategy::Discovered);
            if let Some(re) = rule_estimate(st, o, DEFAULT_RULE_K, &cell) {
                let s = o.listed_reward as f64 * (re.est.p + self.beta * re.est.sd).min(1.0) / o.listed_cost.max(1) as f64;
                if best.map_or(true, |b| s > b.0) {
                    best = Some((s, true, i, re.est.p));
                    rule_info = Some(re);
                }
            }
        }
        let Some((score, from_rule, i, p)) = best else {
            if st.budget_left >= 1 {
                self.skips += 1;
                return Some(Choice2 { act: Act2::Skip, policy: FALLBACK_NAME.into(), why: "nothing affordable but the skip".into(), p: 0.0, sd: 0.0, cost_est: 1.0, score: 0.0, rationale: json!({ "policy": FALLBACK_NAME }) });
            }
            return None;
        };
        let rate = cx.rate();
        if self.lin.skip_frac > 0.0 && cx.spent >= SKIP_MIN_COST && score < self.lin.skip_frac * rate && st.budget_left >= 1 {
            self.skips += 1;
            return Some(Choice2 { act: Act2::Skip, policy: FALLBACK_NAME.into(), why: format!("best score {score:.3} below {} x reported rate {rate:.3}: skip", self.lin.skip_frac), p: 0.0, sd: 0.0, cost_est: 1.0, score: 0.0, rationale: json!({ "policy": FALLBACK_NAME, "best_score": r3(score), "rate": r3(rate) }) });
        }
        if from_rule {
            self.by_rule += 1;
            let o = &offers[i];
            let re = rule_info.expect("rule estimate");
            return Some(Choice2 {
                act: Act2::Attempt { slot: o.slot, strategy: Strategy::Discovered },
                policy: FALLBACK_NAME.into(),
                why: "an accepted rule covers this offer: its rule-record estimate gives the highest score".into(),
                p,
                sd: re.est.sd,
                cost_est: o.listed_cost as f64,
                score,
                rationale: json!({ "policy": FALLBACK_NAME, "rule_id": re.rule_id, "rule_event": re.event_id, "support": re.support, "missed": re.missed, "p": r3(re.est.p), "score": r3(score) }),
            });
        }
        self.by_lin += 1;
        let o = opts[i];
        self.pending = Some(Pend { cx, opt: o, reward0: st.total_reward, cost0: st.total_cost });
        Some(Choice2 {
            act: Act2::Attempt { slot: o.slot, strategy: o.strategy.expect("not a skip") },
            policy: FALLBACK_NAME.into(),
            why: "no rule option scores higher: LinUCB's choice".into(),
            p,
            sd: 0.0,
            cost_est: o.energy,
            score,
            rationale: json!({ "policy": FALLBACK_NAME, "linucb_score": r3(score) }),
        })
    }
}
