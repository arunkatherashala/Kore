//! Track B14, part 2: always-on curiosity, and the planner's use of the rule library (first world;
//! opt-in, default off).
//!
//! `Curious` wraps the B1 planner (`ModelChooser`, unchanged). With both parts off it returns the
//! planner's choice untouched (bit for bit).
//!
//! * Library estimate (`rule_library` on): for an offered sequence task whose prefix a live rule
//!   applies to (the being sees the prefix), the success estimate of the `discovered` strategy is
//!   the library's record of that rule (`library::LibIndex::estimate`: its share right on its
//!   applicable revealed examples times the share right of all rule-produced answers), with the
//!   planner's own optimism `beta x sd` and the planner's own skip test. Elsewhere the B1 estimate
//!   is used. The decision (policy `b14-library` when this changed the choice, the planner's policy
//!   otherwise) cites the rule's note and the latest examples of its record.
//! * Curiosity (`curiosity` on, budget `pm` per mille): the energy of curiosity steps may never
//!   exceed `pm / 1000` of the energy spent so far (including the step itself). The budget grows
//!   with the life, so it never runs out and never decays to zero; it is spent where the being
//!   expects to learn most. The learning value of an option is
//!
//!   ```text
//!   value = EIG(prefix) + ei_w * EI / rate
//!   ```
//!
//!   with EIG the expected information gain in bits about the rule of the offer's signature (B9's
//!   live hypotheses: zero where an accepted rule already predicts the prefix, and zero once every
//!   hypothesis of the grammar is refuted, so unlearnable families stop drawing curiosity), and EI
//!   the expected improvement of the option's reward per energy over the best mean of the menu under
//!   the planner's own posterior (normal approximation; a prediction-error term: it is large where
//!   the estimate is uncertain and close to the best, and shrinks as the cell's evidence grows). A
//!   curiosity step takes the option of highest value if that value is at least `v_min` and the
//!   budget allows it, in place of the planner's choice (policy `b14-curious`; the decision records
//!   the value, its parts, the planner's choice and the budget).
//!
//! Only observable things are used (offers, prefixes the being is shown, its own log-derived state).

use super::super::discover::{self, sig_of, Form};
use super::super::experiment::HypCache;
use super::super::planner::{Action, AdviceMode, Choice, MenuChooser, ModelChooser, Offer, SkipRule, SKIP_MIN_COST};
use super::super::solver::Strategy;
use super::super::state::BeingState;
use super::super::world::{Domain, World, SKIP_COST};
use super::library::{LibCfg, LibEst, LibIndex};
use serde_json::json;

pub const CUR_POLICY: &str = "b14-curious";
pub const LIB_POLICY: &str = "b14-library";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurCfg {
    /// Budget: per mille of the energy spent so far.
    pub pm: u32,
    /// Weight of the expected-improvement term (relative to the realized rate).
    pub ei_w: f64,
    /// Smallest value that triggers a curiosity step.
    pub v_min: f64,
}

/// Chosen on tuning seeds 101..120 only (docs/measured.md, Track B14).
pub const DEFAULT_CUR: CurCfg = CurCfg { pm: 20, ei_w: 1.0, v_min: 0.05 };

pub struct Curious {
    pub base: ModelChooser,
    world: World,
    pub lib: Option<(LibCfg, LibIndex)>,
    pub cur: Option<CurCfg>,
    hyp: HypCache,
    last: bool,
    pub lib_choices: u64,
}

/// Standard normal density and distribution function.
fn phi(z: f64) -> f64 {
    (-0.5 * z * z).exp() / (2.0 * std::f64::consts::PI).sqrt()
}
fn big_phi(z: f64) -> f64 {
    // Abramowitz-Stegun 7.1.26 via erf
    let x = z / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let y = 1.0 - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * (-x * x).exp();
    0.5 * (1.0 + if x >= 0.0 { y } else { -y })
}

/// E[max(0, X)] for X ~ N(mu, sd^2).
pub fn expected_improvement(mu: f64, sd: f64) -> f64 {
    if sd <= 1e-12 {
        return mu.max(0.0);
    }
    let z = mu / sd;
    (mu * big_phi(z) + sd * phi(z)).max(0.0)
}

impl Curious {
    pub fn new(base: ModelChooser, world: &World, lib: Option<LibCfg>, cur: Option<CurCfg>) -> Curious {
        Curious { base, world: world.clone(), lib: lib.map(|c| (c, LibIndex::default())), cur, hyp: HypCache::new(), last: false, lib_choices: 0 }
    }
    /// The last decision was a curiosity step.
    pub fn last_was_curious(&self) -> bool {
        self.last
    }

    fn skip_bar(&self, st: &BeingState, cost_est: f64, n: u64) -> (f64, f64) {
        let rate = match st.model.recent_rate() {
            Some(r) => r,
            None => {
                if st.total_cost > 0 {
                    st.total_reward as f64 / st.total_cost as f64
                } else {
                    0.0
                }
            }
        };
        let b = &self.base;
        let bar = if b.skip_rule == SkipRule::Rate { b.skip_frac * rate } else { b.skip_rule.bar(b.skip_frac, rate, cost_est, SKIP_COST, n) };
        (bar, rate)
    }
}

fn blocks_of(st: &BeingState) -> Vec<Form> {
    let mut v = vec![];
    for r in st.rules.iter().filter(|r| r.xf.is_id() && !r.retired) {
        if !v.contains(&r.form) {
            v.push(r.form);
        }
    }
    v
}

fn r4(x: f64) -> f64 {
    (x * 10000.0).round() / 10000.0
}

impl MenuChooser for Curious {
    fn name(&self) -> String {
        self.base.name()
    }

    fn choose(&mut self, st: &BeingState, offers: &[Offer]) -> Option<Choice> {
        self.last = false;
        let lib_on = self.lib.is_some() && self.base.advice.mode == AdviceMode::Off && !st.model.adaptive();
        if !lib_on && self.cur.is_none() {
            return self.base.choose(st, offers);
        }
        let base_choice = self.base.choose(st, offers)?;
        let tasks = self.world.offers_at(st.step, st.clock);
        let prefix = |o: &Offer| -> Option<Vec<i64>> {
            if !matches!(o.domain, Domain::Pattern | Domain::PatternB) {
                return None;
            }
            tasks.get(o.slot).filter(|t| t.view.domain == o.domain && t.view.difficulty == o.difficulty).map(|t| t.view.params.clone())
        };
        let mut cands = self.base.candidates(st, offers);
        let mut ests: Vec<Option<LibEst>> = vec![None; cands.len()];
        if lib_on {
            let (lc, idx) = self.lib.as_mut().unwrap();
            if lc.estimate {
                idx.update(st);
                for (i, c) in cands.iter_mut().enumerate() {
                    if c.strategy != Strategy::Discovered {
                        continue;
                    }
                    let Some(o) = offers.iter().find(|o| o.slot == c.slot) else { continue };
                    let Some(pre) = prefix(o) else { continue };
                    let Some((_, r)) = discover::apply(&st.rules, sig_of(o.domain, o.difficulty), &pre) else { continue };
                    let e = idx.estimate(st, r);
                    c.p = e.p;
                    c.sd = e.sd;
                    let pu = (e.p + self.base.beta * e.sd).min(1.0);
                    c.score = o.listed_reward as f64 * pu / c.cost_est;
                    c.mean_score = o.listed_reward as f64 * e.p / c.cost_est;
                    ests[i] = Some(e);
                }
            }
        }
        // the planner's decision on these estimates (the planner's own rule: highest optimistic score, its skip test)
        let mut chosen: Option<usize> = None;
        for (i, c) in cands.iter().enumerate() {
            if c.affordable && chosen.map_or(true, |b| c.score > cands[b].score) {
                chosen = Some(i);
            }
        }
        let gi = chosen?;
        let any_lib = ests.iter().any(|e| e.is_some());
        let (mut action, mut policy) = (base_choice.action, base_choice.policy.clone());
        let mut rationale = base_choice.rationale.clone();
        let (mut p, mut sd, mut cost_est, mut score, mut why) = (base_choice.p, base_choice.sd, base_choice.cost_est, base_choice.score, base_choice.why.clone());
        let mut cites: Vec<String> = vec![];
        if any_lib {
            let g = &cands[gi];
            let (bar, rate) = self.skip_bar(st, g.cost_est, g.n);
            let skip = self.base.skip_frac > 0.0 && st.total_cost >= SKIP_MIN_COST && g.score < bar && st.budget_left >= SKIP_COST;
            let a = if skip { Action::Skip } else { Action::Attempt { slot: g.slot, strategy: g.strategy } };
            let shown: Vec<serde_json::Value> = cands
                .iter()
                .zip(&ests)
                .filter_map(|(c, e)| e.as_ref().map(|e| json!({ "slot": c.slot, "rule_id": e.rule_id, "rule_event": e.rule_event, "applicable_examples": e.app, "rule_share_right": r4(e.acc), "rule_answers_share_right": r4(e.exec), "rule_answers": e.uses, "p_success": r4(e.p), "uncertainty_sd": r4(e.sd), "score": r4(c.score) })))
                .collect();
            rationale["rule_library"] = json!({ "track": "B14", "what": "where a live rule applies to the offered prefix, the discovered strategy is estimated from the rule's record on the being's revealed examples times the record of all rule-produced answers", "estimates": shown, "skip_bar": r4(bar), "realized_rate": r4(rate) });
            if a != base_choice.action {
                action = a;
                policy = LIB_POLICY.into();
                self.lib_choices += 1;
                match a {
                    Action::Skip => {
                        (p, sd, cost_est, score) = (0.0, 0.0, SKIP_COST as f64, 0.0);
                        why = format!("best optimistic score {:.3} (with the library estimates) is below the skip bar {:.3}: skip the menu", g.score, bar);
                    }
                    Action::Attempt { .. } => {
                        (p, sd, cost_est, score) = (g.p, g.sd, g.cost_est, g.score);
                        why = match &ests[gi] {
                            Some(e) => format!("library rule {} applies to the prefix; its record ({} of {} applicable revealed examples right, rule answers right {:.3}) gives the highest optimistic score {:.3}", e.rule_id, (e.acc * (e.app as f64 + 2.0) - 1.0).round(), e.app, e.exec, g.score),
                            None => format!("highest optimistic reward per estimated energy {:.3} with the library estimates", g.score),
                        };
                    }
                }
            }
            if let (Action::Attempt { slot, strategy: Strategy::Discovered }, Some(e)) = (action, ests.get(gi).and_then(|x| x.as_ref())) {
                if cands[gi].slot == slot {
                    cites.push(e.rule_event.clone());
                    cites.extend(e.recent.iter().cloned());
                    rationale["rule_library_used"] = json!({ "rule_id": e.rule_id, "rule_event": e.rule_event, "examples_cited": e.recent });
                }
            }
        }
        // curiosity
        if let Some(cc) = self.cur {
            let spent = st.b14_cur_energy;
            let blocks = blocks_of(st);
            self.hyp.update(&st.examples, &blocks);
            let greedy_mean = cands.iter().filter(|c| c.affordable).map(|c| c.mean_score).fold(f64::NEG_INFINITY, f64::max);
            let (_, rate) = self.skip_bar(st, 1.0, 0);
            let live: Vec<discover::Rule> = st.rules.iter().filter(|r| !r.retired).cloned().collect();
            let mut info_of: Vec<Option<(f64, usize)>> = vec![None; offers.len().max(4)];
            let mut best: Option<(f64, usize, f64, f64)> = None;
            for (i, c) in cands.iter().enumerate() {
                if !c.affordable {
                    continue;
                }
                let Some(o) = offers.iter().find(|o| o.slot == c.slot) else { continue };
                let energy = c.strategy.energy(o.listed_cost);
                if energy > st.budget_left || (spent + energy) * 1000 > cc.pm as i64 * (st.total_cost + energy) {
                    continue;
                }
                let info = match prefix(o) {
                    Some(pre) => {
                        if info_of.len() <= o.slot {
                            info_of.resize(o.slot + 1, None);
                        }
                        if info_of[o.slot].is_none() {
                            let a = self.hyp.assess(sig_of(o.domain, o.difficulty), &pre, &live);
                            info_of[o.slot] = Some((a.value, a.live));
                        }
                        info_of[o.slot].map_or(0.0, |x| x.0)
                    }
                    None => 0.0,
                };
                let sd_score = o.listed_reward as f64 * c.sd / c.cost_est;
                let ei = expected_improvement(c.mean_score - greedy_mean, sd_score) / rate.max(0.1);
                let v = info + cc.ei_w * ei;
                if best.map_or(true, |b| v > b.0 || (v == b.0 && c.mean_score > cands[b.1].mean_score)) {
                    best = Some((v, i, info, ei));
                }
            }
            if let Some((v, i, info, ei)) = best {
                let c = &cands[i];
                let a = Action::Attempt { slot: c.slot, strategy: c.strategy };
                if v >= cc.v_min && a != action {
                    rationale["curiosity"] = json!({
                        "track": "B14", "value": r4(v), "information_bits": r4(info), "expected_improvement_over_rate": r4(ei), "ei_weight": cc.ei_w, "min_value": cc.v_min,
                        "planner_choice": action.label(), "budget_per_mille": cc.pm, "curiosity_energy_so_far": spent, "energy_so_far": st.total_cost,
                        "what": "value = expected information gain about the offer's sequence rule (live hypotheses of the being's grammar) + weight x expected improvement of the option over the best mean of the menu / realized rate; budget: curiosity energy at most pm per mille of the energy spent so far",
                    });
                    why = format!("curiosity: learning value {:.3} ({:.3} bits about the rule, expected improvement {:.3} of the rate) within the budget of {} per mille; the planner would have chosen {}", v, info, ei, cc.pm, action.label());
                    action = a;
                    policy = CUR_POLICY.into();
                    (p, sd, cost_est, score) = (c.p, c.sd, c.cost_est, c.score);
                    cites.clear();
                    if let Some(obj) = rationale.as_object_mut() {
                        obj.remove("rule_library_used");
                    }
                    if let (Strategy::Discovered, Some(e)) = (c.strategy, ests[i].as_ref()) {
                        cites.push(e.rule_event.clone());
                        cites.extend(e.recent.iter().cloned());
                        rationale["rule_library_used"] = json!({ "rule_id": e.rule_id, "rule_event": e.rule_event, "examples_cited": e.recent });
                    }
                    self.last = true;
                }
            }
        }
        if !cites.is_empty() {
            rationale[super::CITES_KEY] = json!(cites);
        }
        Some(Choice { action, policy, why, p, sd, cost_est, score, rationale, advice: base_choice.advice })
    }
}
