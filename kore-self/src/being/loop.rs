//! The being's loop: observe -> decide -> act -> check -> record.
//!
//! * observe: the state is folded from the being's own log (`BeingState`), not
//!   from anything kept in memory between calls;
//! * decide: a `DomainPicker` (UCB1 for the being) picks the domain among the
//!   affordable ones; the decision is logged with the counts, means, bonuses
//!   and scores of every domain and cites the latest outcome events it used;
//! * act: a `Thinker` proposes an answer for the task on offer;
//! * check: the world's exact checker decides right or wrong;
//! * record: every step appends a decision, an action and an outcome event
//!   (source `being:loop`, `occurred_at` from the world clock), one
//!   `decision` revision citing its evidence events, and a prediction (the
//!   thinker's own confidence) with its resolved outcome row.
//!
//! Rows are committed every `FLUSH_EVERY` steps and at the end of a run, each
//! commit being one atomic version. A crash can lose the last unflushed steps;
//! the log stays valid. Nothing else is written anywhere.

use super::advice::{self, Advice, AdviceRequest, AdviceSetup, Advisor, AdvisorKind, DecisionAdvice, OfferView, RULE_EVERY};
use super::chooser::{DomainPicker, Pick, Ucb1, DEFAULT_EXPLORE_C};
use super::discover::{self, terms_tag, Found, Rule};
use super::planner::{Action, MenuChooser, ModelChooser, Offer, DEFAULT_BETA};
use super::policy::{Policy, StopReason};
use super::solver::{Context, Strategy, Thinker, STRATEGIES, STRATEGIES_EXT};
use super::model::AdaptMode;
use super::state::{genesis_event, refill_event, BeingState, Change, Chooser, Genesis, Outcome, SOURCE};
use super::world::{iso_at, Domain, Task, World, DOMAINS, SECS_PER_ENERGY, SKIP_COST};
use crate::versioned::{event_id, prediction_id, Batch, LogError, NewEvent, NewOutcome, NewPrediction, NewRevision, VersionedLog};
use serde_json::{json, Value};

pub const FLUSH_EVERY: usize = 25;
pub const DEFAULT_BUDGET: i64 = 2000;

/// Everything one step adds to the log.
pub struct StepOut {
    pub outcome: Outcome,
    pub pick: Option<Pick>,
    /// Menu mode: the action taken and the model's predicted success (when it made one).
    pub action: Option<String>,
    pub predicted_p: Option<f64>,
    pub decision_event_id: String,
    pub events: Vec<NewEvent>,
    pub revision: NewRevision,
    /// None for a skipped menu (nothing to predict).
    pub prediction: Option<NewPrediction>,
    pub resolved: Option<NewOutcome>,
    /// Changes the detector flagged on this step: a note event and a revision citing the outcomes they rest on.
    pub changes: Vec<(NewEvent, NewRevision)>,
    /// Rules accepted on an advisor's proposal this step (their events are already in `events`; these carry the revisions).
    pub rules: Vec<NewRule>,
}

/// The world a being lives in, from its genesis event.
pub fn world_for(g: &Genesis) -> World {
    World::with_drift(g.world_seed, g.extended, if g.drift { Some(g.budget) } else { None })
}

/// The log records of one detected change.
pub fn change_records(c: &Change) -> (NewEvent, NewRevision) {
    let a = &c.alarm;
    let dir = if a.drop { "lower" } else { "higher" };
    let at = iso_at(c.clock);
    let text = format!(
        "change {} detected at step {}: the success rate of strategy {} in domain {} looks {dir} than the model predicted (observed {} right of {} attempts, predicted {:.1}; CUSUM statistic {:.2}); older evidence of this group is cut to {:.0}% of its weight",
        c.index, c.step, a.strategy.name(), a.domain.name(), a.run_succ, a.run_n, a.run_pred, a.statistic, 100.0 * super::model::KEEP_AFTER_ALARM
    );
    let key = format!("being/change/{:04}", c.index);
    let ev = NewEvent {
        source: SOURCE.into(),
        occurred_at: at.clone(),
        kind: "note".into(),
        text,
        tags: vec![
            "change".into(),
            format!("change_index:{}", c.index),
            format!("step:{}", c.step),
            format!("domain:{}", a.domain.name()),
            format!("strategy:{}", a.strategy.name()),
            format!("direction:{}", if a.drop { "drop" } else { "rise" }),
            format!("rev_key:{key}"),
        ],
        redacted: false,
    };
    let ev_id = event_id(&ev.source, &ev.occurred_at, &ev.text);
    let mut caused_by = c.cites.clone();
    caused_by.push(ev_id);
    let rationale = json!({
        "detector": "two-sided CUSUM on standardized prediction residuals per (domain, strategy)", "domain": a.domain.name(), "strategy": a.strategy.name(),
        "direction": if a.drop { "drop" } else { "rise" }, "statistic": a.statistic, "threshold": super::model::CUSUM_H, "drift_k": super::model::CUSUM_K,
        "attempts_in_run": a.run_n, "right_in_run": a.run_succ, "predicted_right_in_run": a.run_pred, "cited_outcomes": c.cites.len(),
        "action": "evidence of this (domain, strategy) group cut to a share of its weight", "share_kept": super::model::KEEP_AFTER_ALARM,
    });
    let rev = NewRevision {
        entity_type: "belief".into(),
        key,
        stance: format!("{} with {}: success rate {dir} than predicted", a.domain.name(), a.strategy.name()),
        confidence: (a.statistic / (a.statistic + super::model::CUSUM_H)).clamp(0.0, 1.0),
        rationale: rationale.to_string(),
        supersedes: None,
        valid_from: at,
        author: "rule:being-detector".into(),
        caused_by,
    };
    (ev, rev)
}

/// The observable part of the tasks on offer.
pub fn offers_of(tasks: &[Task]) -> Vec<Offer> {
    tasks.iter().enumerate().map(|(i, t)| Offer { slot: i, domain: t.view.domain, difficulty: t.view.difficulty, listed_cost: t.view.cost, listed_reward: t.view.reward }).collect()
}

/// World + state + thinker; one `step` per call. Used by the logged loop and,
/// without a log, by the measurement.
pub struct Engine<'a> {
    pub world: World,
    pub state: BeingState,
    pub thinker: &'a dyn Thinker,
    /// An outside advisor, asked once per menu step (None: no advice).
    pub advisor: Option<&'a dyn Advisor>,
    /// Records of a step that ended without a decision (nothing affordable) after advice was already taken into the state.
    carry: Vec<NewEvent>,
    carry_rules: Vec<NewRule>,
}

impl<'a> Engine<'a> {
    pub fn new(world: World, state: BeingState, thinker: &'a dyn Thinker) -> Engine<'a> {
        Engine { world, state, thinker, advisor: None, carry: vec![], carry_rules: vec![] }
    }

    pub fn with_advisor(mut self, advisor: Option<&'a dyn Advisor>) -> Engine<'a> {
        self.advisor = advisor;
        self
    }

    /// Records that were produced but not yet returned by a step (the run loop writes them at the end).
    pub fn take_carry(&mut self) -> (Vec<NewEvent>, Vec<NewRule>) {
        (std::mem::take(&mut self.carry), std::mem::take(&mut self.carry_rules))
    }

    /// What the being tells an advisor about the offers of one step (nothing else leaves the being).
    pub fn advice_request(&self, step: u64, tasks: &[Task]) -> AdviceRequest {
        let ext = self.state.genesis.as_ref().map_or(false, |g| g.extended);
        let offers: Vec<OfferView> = tasks
            .iter()
            .enumerate()
            .map(|(i, t)| OfferView {
                slot: i,
                domain: t.view.domain.name().to_string(),
                difficulty: t.view.difficulty,
                listed_cost: t.view.cost,
                listed_reward: t.view.reward,
                terms: if ext && t.view.domain == Domain::Pattern { Some(t.view.params.clone()) } else { None },
            })
            .collect();
        let mut sigs: Vec<u8> = if ext && step % RULE_EVERY == 0 { offers.iter().filter(|o| o.terms.is_some()).map(|o| o.difficulty).collect() } else { vec![] };
        sigs.sort_unstable();
        sigs.dedup();
        let strategies = if ext { STRATEGIES_EXT.iter() } else { STRATEGIES.iter() }.map(|s| s.name().to_string()).collect();
        AdviceRequest { step, offers, strategies, rule_sigs_wanted: sigs }
    }

    /// Domains whose task fits in the remaining budget at the current step.
    pub fn affordable(&self, menu: &[Task]) -> Vec<Domain> {
        menu.iter().filter(|t| t.view.cost <= self.state.budget_left).map(|t| t.view.domain).collect()
    }

    pub fn step(&mut self, picker: &mut dyn DomainPicker) -> Result<StepOut, StopReason> {
        let step = self.state.step;
        let menu = self.world.menu(step);
        let affordable = self.affordable(&menu);
        if affordable.is_empty() {
            return Err(StopReason::BudgetExhausted);
        }
        let mut pick = picker.pick(&self.state, &affordable);
        if !affordable.contains(&pick.domain) {
            pick.domain = affordable[0];
        }
        let task = &menu[pick.domain.idx()];
        let v = &task.view;
        let ctx = Context { step, task: v, noise: self.world.noise(step, v.domain), stats: &self.state.stats, rules: &self.state.rules, degraded: false };
        let prop = self.thinker.propose(&ctx);
        let correct = task.check(prop.answer);
        let reward = if correct { v.reward } else { 0 };

        let evidence = self.state.evidence_ids();
        let t0 = self.state.clock;
        let t1 = t0 + v.cost * SECS_PER_ENERGY;
        let chosen = pick.chosen().cloned();
        let (mean, bonus, score) = chosen.as_ref().map(|r| (r.mean, r.bonus, r.score)).unwrap_or((0.0, 0.0, 0.0));
        let fin = |x: f64| if x.is_finite() { format!("{x:.3}") } else { "n/a".to_string() };
        let n_chosen = chosen.as_ref().map(|r| r.n).unwrap_or(0);

        let dtext = format!(
            "step {step}: chose {} by {} ({}); n={n_chosen} mean={} reward/energy bonus={} score={}; task {} difficulty {} cost {} reward {}; cites {} events",
            v.domain.name(), pick.policy, pick.why, fin(mean), fin(bonus), fin(score), v.id, v.difficulty, v.cost, v.reward, evidence.len()
        );
        let key = format!("being/choice/{step:06}");
        let dev = NewEvent {
            source: SOURCE.into(),
            occurred_at: iso_at(t0),
            kind: "decision".into(),
            text: dtext,
            tags: vec![format!("step:{step}"), format!("domain:{}", v.domain.name()), format!("policy:{}", pick.policy), format!("rev_key:{key}")],
            redacted: false,
        };
        let dev_id = event_id(&dev.source, &dev.occurred_at, &dev.text);

        let aev = NewEvent {
            source: SOURCE.into(),
            occurred_at: iso_at(t0),
            kind: "action".into(),
            text: format!(
                "step {step}: attempt task {} ({}) with thinker {} using {}: answer {} (own confidence {:.2})",
                v.id, v.prompt, self.thinker.id(), prop.method, prop.answer, prop.confidence
            ),
            tags: vec![format!("step:{step}"), format!("domain:{}", v.domain.name()), format!("method:{}", prop.method)],
            redacted: false,
        };

        let budget_left = self.state.budget_left - v.cost;
        let oev = NewEvent {
            source: SOURCE.into(),
            occurred_at: iso_at(t1),
            kind: "outcome".into(),
            text: format!(
                "step {step}: task {} answer {} is {} (expected {}); reward {reward} cost {} budget left {budget_left}",
                v.id, prop.answer, if correct { "correct" } else { "wrong" }, task.expected(), v.cost
            ),
            tags: vec![
                format!("step:{step}"),
                format!("domain:{}", v.domain.name()),
                format!("task:{}", v.id),
                format!("difficulty:{}", v.difficulty),
                format!("result:{}", if correct { "correct" } else { "wrong" }),
                format!("reward:{reward}"),
                format!("cost:{}", v.cost),
                format!("budget_left:{budget_left}"),
                format!("clock:{t1}"),
            ],
            redacted: false,
        };
        let oev_id = event_id(&oev.source, &oev.occurred_at, &oev.text);

        let revision = NewRevision {
            entity_type: "decision".into(),
            key,
            stance: format!("choose {}", v.domain.name()),
            confidence: if score.is_finite() { score.clamp(0.0, 1.0) } else { 1.0 },
            rationale: pick.to_json().to_string(),
            supersedes: None,
            valid_from: iso_at(t0),
            author: format!("rule:being-{}", pick.policy),
            caused_by: evidence,
        };
        let prediction = NewPrediction {
            made_at: iso_at(t0),
            question_id: format!("being:step:{step}"),
            target: format!("task:{}", v.id),
            options: vec!["correct".into(), "wrong".into()],
            probs: vec![prop.confidence, 1.0 - prop.confidence],
            model_id: format!("being:loop:{}", self.thinker.id()),
            model_version: "1".into(),
            features_cite: vec![dev_id.clone()],
            resolves_by: None,
        };
        let pred_id = prediction_id(&prediction).map_err(|_| StopReason::StopFlag)?;
        let resolved = NewOutcome {
            pred_id,
            resolved_at: iso_at(t1),
            actual: if correct { "correct" } else { "wrong" }.into(),
            outcome_event_id: Some(oev_id.clone()),
        };
        let outcome = Outcome {
            step,
            domain: v.domain,
            task_id: v.id.clone(),
            difficulty: v.difficulty,
            correct,
            reward,
            cost: v.cost,
            budget_left,
            clock: t1,
            event_id: oev_id,
            strategy: None,
            listed_cost: v.cost,
            skipped: false,
            example: None,
        };
        self.state.apply(&outcome);
        Ok(StepOut { outcome, pick: Some(pick), action: None, predicted_p: None, decision_event_id: dev_id, events: vec![dev, aev, oev], revision, prediction: Some(prediction), resolved: Some(resolved), changes: vec![], rules: vec![] })
    }

    /// Menu mode: one step over `MENU_SIZE` offered tasks. The chooser picks
    /// (offer, strategy) or skips; the thinker answers with that strategy.
    pub fn step_menu(&mut self, chooser: &mut dyn MenuChooser) -> Result<StepOut, StopReason> {
        let step = self.state.step;
        let tasks = self.world.offers_at(step, self.state.clock);
        let offers = offers_of(&tasks);
        let t0 = self.state.clock;
        let alarms_before = self.state.advice_alarms.len();
        // advice first: one advice event (request and advice), then the verdict on a proposed rule
        let mut pre_events: Vec<NewEvent> = std::mem::take(&mut self.carry);
        let mut new_rules: Vec<NewRule> = std::mem::take(&mut self.carry_rules);
        if let Some(advisor) = self.advisor {
            let req = self.advice_request(step, &tasks);
            let mut adv = advisor.advise(&req);
            if let Some(m) = &adv.menu {
                if m.slot >= req.offers.len() || !req.strategies.iter().any(|s| s == m.strategy.name()) {
                    adv.menu = None;
                    adv.error = Some("the advisor named an option that is not on offer".into());
                }
            }
            let who = advisor.id();
            let task_ids: Vec<String> = tasks.iter().map(|t| t.view.id.clone()).collect();
            let (ev, id, pending) = advice::advice_event(&who, iso_at(t0), &req, &adv, &task_ids);
            pre_events.push(ev);
            self.state.note_advice(pending);
            if let Some(r) = &adv.rule {
                let (evs, nr) = self.judge_rule(&id, &who, r.sig, r.form, t0);
                pre_events.extend(evs);
                new_rules.extend(nr);
            }
        }
        let Some(ch) = chooser.choose(&self.state, &offers) else {
            self.state.advice = None;
            self.carry = pre_events;
            self.carry_rules = new_rules;
            return Err(StopReason::BudgetExhausted);
        };
        let label = ch.action.label();
        let evidence = self.state.evidence_ids();
        let budget0 = self.state.budget_left;
        let fin = |x: f64| if x.is_finite() { format!("{x:.3}") } else { "n/a".to_string() };

        // act and check
        struct Done {
            correct: bool,
            reward: i64,
            cost: i64,
            result: &'static str,
            answer_text: String,
            view: Option<(usize, Strategy)>,
            conf: f64,
            rule: Option<String>,
        }
        let done = match ch.action {
            Action::Skip => {
                let cost = SKIP_COST.min(budget0);
                Done { correct: false, reward: 0, cost, result: "skip", answer_text: format!("skip the menu at step {step} (energy {cost})"), view: None, conf: 0.0, rule: None }
            }
            Action::Attempt { slot, strategy } => {
                let task = &tasks[slot];
                let v = &task.view;
                let energy = strategy.energy(v.cost);
                if energy > budget0 {
                    Done {
                        correct: false,
                        reward: 0,
                        cost: budget0,
                        result: "unpaid",
                        answer_text: format!("attempt task {} with {}: energy ran out before the answer ({budget0} left, {energy} needed)", v.id, strategy.name()),
                        view: Some((slot, strategy)),
                        conf: 0.0,
                        rule: None,
                    }
                } else {
                    let ctx = Context { step, task: v, noise: self.world.menu_noise(step, slot), stats: &self.state.stats, rules: &self.state.rules, degraded: self.world.degraded(t0) };
                    let prop = self.thinker.propose_with(&ctx, strategy);
                    let correct = task.check(prop.answer);
                    Done {
                        correct,
                        reward: if correct { v.reward } else { 0 },
                        cost: energy,
                        result: if correct { "correct" } else { "wrong" },
                        answer_text: format!("attempt task {} ({}) with thinker {} using {}: answer {} (own confidence {:.2})", v.id, v.prompt, self.thinker.id(), prop.method, prop.answer, prop.confidence),
                        view: Some((slot, strategy)),
                        conf: prop.confidence,
                        rule: prop.rule_id.clone(),
                    }
                }
            }
        };
        let t1 = t0 + done.cost * SECS_PER_ENERGY;
        let budget_left = budget0 - done.cost;
        let key = format!("being/choice/{step:06}");
        let (chosen_txt, mut dtags) = match done.view {
            Some((slot, strategy)) => {
                let v = &tasks[slot].view;
                (
                    format!("slot {slot} ({} difficulty {}, listed cost {} reward {}) with strategy {}", v.domain.name(), v.difficulty, v.cost, v.reward, strategy.name()),
                    vec![format!("domain:{}", v.domain.name()), format!("strategy:{}", strategy.name())],
                )
            }
            None => ("skip".to_string(), vec![]),
        };
        let dtext = format!(
            "step {step}: chose {chosen_txt} by {} ({}); p_success={} sd={} est_energy={} score={}; cites {} events",
            ch.policy, ch.why, fin(ch.p), fin(ch.sd), fin(ch.cost_est), fin(ch.score), evidence.len()
        );
        dtags.extend([format!("step:{step}"), format!("action:{label}"), format!("policy:{}", ch.policy), format!("rev_key:{key}")]);
        if let Some(tr) = &ch.advice {
            dtags.extend(tr.tags());
            if let Some(d) = advice::decision_from_tags(&dtags.join(",")) {
                self.state.note_decision_advice(d);
            }
        }
        let dev = NewEvent { source: SOURCE.into(), occurred_at: iso_at(t0), kind: "decision".into(), text: dtext, tags: dtags, redacted: false };
        let dev_id = event_id(&dev.source, &dev.occurred_at, &dev.text);
        let aev = NewEvent {
            source: SOURCE.into(),
            occurred_at: iso_at(t0),
            kind: "action".into(),
            text: format!("step {step}: {}", done.answer_text),
            tags: {
                let mut t = vec![format!("step:{step}"), format!("action:{label}")];
                if let Some(r) = &done.rule {
                    t.push(format!("rule_used:{r}"));
                }
                t
            },
            redacted: false,
        };
        let mut otags = vec![format!("step:{step}")];
        let (domain, task_id, difficulty, listed) = match done.view {
            Some((slot, strategy)) => {
                let v = &tasks[slot].view;
                otags.extend([
                    format!("domain:{}", v.domain.name()),
                    format!("task:{}", v.id),
                    format!("difficulty:{}", v.difficulty),
                    format!("strategy:{}", strategy.name()),
                    format!("slot:{slot}"),
                ]);
                if self.world.extended && v.domain == Domain::Pattern && (done.result == "correct" || done.result == "wrong") {
                    // the checker's answer is revealed after the attempt and recorded: one example for the being's own search
                    otags.extend([format!("terms:{}", terms_tag(&v.params)), format!("next:{}", tasks[slot].expected())]);
                }
                (v.domain, v.id.clone(), v.difficulty, v.cost)
            }
            None => (Domain::Arithmetic, "none".to_string(), 1, done.cost),
        };
        otags.extend([
            format!("result:{}", done.result),
            format!("reward:{}", done.reward),
            format!("cost:{}", done.cost),
            format!("listed:{listed}"),
            format!("budget_left:{budget_left}"),
            format!("clock:{t1}"),
        ]);
        let what = if done.view.is_some() { format!("task {task_id}") } else { "menu".to_string() };
        let oev = NewEvent {
            source: SOURCE.into(),
            occurred_at: iso_at(t1),
            kind: "outcome".into(),
            text: format!("step {step}: {what} is {}; reward {} cost {} budget left {budget_left}", done.result, done.reward, done.cost),
            tags: otags,
            redacted: false,
        };
        let oev_id = event_id(&oev.source, &oev.occurred_at, &oev.text);
        let revision = NewRevision {
            entity_type: "decision".into(),
            key,
            stance: format!("choose {label}"),
            confidence: if ch.p.is_finite() && ch.p > 0.0 { ch.p.clamp(0.0, 1.0) } else { 0.5 },
            rationale: ch.rationale.to_string(),
            supersedes: None,
            valid_from: iso_at(t0),
            author: format!("rule:being-{}", ch.policy),
            caused_by: evidence,
        };
        let attempted = done.result == "correct" || done.result == "wrong";
        let predicted_p = if attempted { Some(if ch.p > 0.0 { ch.p } else { done.conf }.clamp(0.001, 0.999)) } else { None };
        let (prediction, resolved) = match predicted_p {
            Some(pp) => {
                let pr = NewPrediction {
                    made_at: iso_at(t0),
                    question_id: format!("being:step:{step}"),
                    target: format!("task:{task_id}"),
                    options: vec!["correct".into(), "wrong".into()],
                    probs: vec![pp, 1.0 - pp],
                    model_id: format!("being:loop:{}", ch.policy),
                    model_version: "1".into(),
                    features_cite: vec![dev_id.clone()],
                    resolves_by: None,
                };
                let pred_id = prediction_id(&pr).map_err(|_| StopReason::StopFlag)?;
                let rs = NewOutcome { pred_id, resolved_at: iso_at(t1), actual: done.result.into(), outcome_event_id: Some(oev_id.clone()) };
                (Some(pr), Some(rs))
            }
            None => (None, None),
        };
        let example = if oev.tags.iter().any(|t| t.starts_with("terms:")) {
            done.view.map(|(slot, _)| discover::Example { event_id: oev_id.clone(), sig: tasks[slot].view.difficulty, terms: tasks[slot].view.params.clone(), next: tasks[slot].expected() })
        } else {
            None
        };
        let outcome = Outcome {
            step,
            domain,
            task_id,
            difficulty,
            correct: done.correct,
            reward: done.reward,
            cost: done.cost,
            budget_left,
            clock: t1,
            event_id: oev_id,
            strategy: done.view.map(|x| x.1),
            listed_cost: listed,
            skipped: !attempted,
            example,
        };
        let before = self.state.changes.len();
        let pending_advice = self.state.advice.clone();
        self.state.apply(&outcome);
        let changes: Vec<(NewEvent, NewRevision)> = self.state.changes[before..].iter().map(change_records).collect();
        let mut events = pre_events;
        events.extend([dev, aev, oev]);
        if let (Some(tr), Some(pa)) = (&ch.advice, &pending_advice) {
            let (text, result) = self.advice_result_text(tr, pa, &tasks, &outcome, &label);
            events.push(advice::result_event(iso_at(t1), tr, step, text, result));
        }
        for (_, al) in &self.state.advice_alarms[alarms_before..] {
            events.push(advice::alarm_event(iso_at(t1), step, al));
        }
        Ok(StepOut { outcome, pick: None, action: Some(label), predicted_p, decision_event_id: dev_id, events, revision, prediction, resolved, changes, rules: new_rules })
    }

    /// In words, with the numbers the being has now: what happened to the advice of one step.
    fn advice_result_text(&self, tr: &advice::AdviceTrace, pa: &advice::PendingAdvice, tasks: &[Task], o: &Outcome, label: &str) -> (String, &'static str) {
        let head = format!("advice {} of {} at step {}", &tr.id[..tr.id.len().min(12)], tr.advisor, o.step);
        let Some(m) = &pa.menu else {
            return (format!("{head}: the advisor named no usable option, nothing to follow or ignore"), "none");
        };
        if !tr.valid {
            return (format!("{head}: the advised option {} could not be attempted (not on offer or not affordable); the being went on with its own choice {label}", tr.advised), "invalid");
        }
        let got = format!("{} (reward {} for energy {})", if o.skipped { "no attempt" } else if o.correct { "correct" } else { "wrong" }, o.reward, o.cost);
        let cell = self.state.trust.cell(&tr.advisor, m.domain.name());
        let (mean, sd) = (cell.mean(), cell.sd());
        if tr.followed && tr.agree {
            return (format!("{head}: the advised option {} is also what the model alone chose; both agree; outcome {got}. Nothing to learn about trust from an agreement", tr.advised), "agree");
        }
        if tr.followed {
            let x = o.reward as f64 / o.cost.max(1) as f64;
            let win = x >= tr.ref_score;
            return (
                format!(
                    "{head}: followed advice {} against the model's own choice {} (the model alone predicted {:.3} reward per energy for it); realized {:.3} reward per energy, outcome {got}; the advice {} the model's prediction; trust in {} for {} advice is now {:.3} +/- {:.3} after {} tests ({} wins), weight {:.2}",
                    tr.advised, tr.alone, tr.ref_score, x, if win { "beat or matched" } else { "did not reach" }, tr.advisor, m.domain.name(), mean, sd, cell.tests, cell.test_wins, tr.w
                ),
                if win { "win" } else { "lose" },
            );
        }
        let v = &tasks[m.slot].view;
        let e = self.state.model.estimate(v.domain, v.difficulty, m.strategy);
        let est = v.reward as f64 * e.p / (v.cost as f64 * e.cost_ratio).max(1.0);
        (
            format!(
                "{head}: ignored; the being chose {label} instead of the advised {} (weight {:.2}); outcome {got}. What the advised option would have earned is not observable; the model's estimate for it is now success {:.2}, about {:.3} reward per energy, from {} attempts of that kind",
                tr.advised, tr.w, e.p, est, e.n
            ),
            "ignored",
        )
    }

    /// An advisor proposed a rule: only the verified search can accept it, on the being's own logged
    /// examples. Returns the verdict event (and the rule note and records when accepted).
    fn judge_rule(&mut self, advice_id: &str, who: &str, sig: u8, form: discover::Form, t0: i64) -> (Vec<NewEvent>, Vec<NewRule>) {
        let at = iso_at(t0);
        if self.state.rules.iter().any(|r| r.id == discover::rule_id(sig, &form)) {
            return (vec![advice::verdict_event(who, at, advice_id, sig, &form, "held", "The rule is already in the rule book; nothing to do.")], vec![]);
        }
        match discover::verify_proposal(&self.state.examples, sig, &form) {
            Some(f) => {
                let nr = self.accept_found(&f, Some((advice_id, who)));
                self.state.note_rule_verdict(who, true);
                let detail = format!("Accepted: it reproduced the revealed next term exactly on {} training and {} held-out logged examples and was never applicable-and-wrong on them.", f.train_ok, f.val_ok);
                let mut evs = vec![advice::verdict_event(who, at, advice_id, sig, &form, "accepted", &detail)];
                let mut out = vec![];
                if let Some(nr) = nr {
                    evs.push(nr.event.clone());
                    out.push(nr);
                }
                (evs, out)
            }
            None => {
                let ex: Vec<discover::Example> = self.state.examples.iter().filter(|e| e.sig == sig).cloned().collect();
                let (r, w, n) = discover::evaluate(&form, &ex);
                self.state.note_rule_verdict(who, false);
                let detail = format!(
                    "Rejected: on the {} logged examples of this signature it is right on {r}, wrong on {w} and not applicable on {n}; acceptance needs no wrong application and at least {} training and {} held-out exact reproductions. The proposal was not used.",
                    ex.len(), discover::K_TRAIN, discover::K_VAL
                );
                (vec![advice::verdict_event(who, at, advice_id, sig, &form, "rejected", &detail)], vec![])
            }
        }
    }
}

/// A rule accepted by one discovery pass, with what has to go into the log.
pub struct NewRule {
    pub rule: Rule,
    pub event: NewEvent,
    pub revision: NewRevision,
}

impl<'a> Engine<'a> {
    /// One discovery pass: search the being's own revealed examples and take every newly accepted
    /// rule into the state. Pure function of the state; a second pass with no new examples adds nothing.
    pub fn discover_pass(&mut self) -> Vec<NewRule> {
        let mut out = vec![];
        for f in discover::search(&self.state.examples) {
            if let Some(nr) = self.accept_found(&f, None) {
                out.push(nr);
            }
        }
        out
    }

    /// Take an accepted form into the rule book and build its records. `via`: (advice id, advisor) when an advisor proposed it.
    /// None if the rule is already held.
    pub fn accept_found(&mut self, f: &Found, via: Option<(&str, &str)>) -> Option<NewRule> {
        let mut rule = Rule::new(f.sig, f.form, f.train_ok, f.val_ok, self.state.step);
        if self.state.rules.iter().any(|r| r.id == rule.id) {
            return None;
        }
        let at = iso_at(self.state.clock);
        let (mut event, ev_id) = rule.event(at.clone());
        if let Some((aid, who)) = via {
            event.tags.push(format!("via_advice:{aid}"));
            event.tags.push(format!("via_advisor:{who}"));
        }
        rule.event_id = ev_id.clone();
        let mut caused_by = f.cite.clone();
        caused_by.push(ev_id);
        if let Some((aid, _)) = via {
            caused_by.push(aid.to_string());
        }
        let mut rationale = json!({
            "rule_id": rule.id, "signature": format!("pattern difficulty {}", rule.sig), "form": rule.form.code(), "dsl": rule.form.dsl(),
            "training_examples_reproduced": f.train_ok, "held_out_examples_reproduced": f.val_ok,
            "new_training_support": f.new_train, "new_held_out_support": f.new_val,
            "min_training_support": discover::K_TRAIN, "min_held_out_support": discover::K_VAL,
            "cited_examples": f.cite.len(), "accepted_at_step": self.state.step,
            "grammar": "author-defined: 14 linear forms over lags 1-3 (optional constant) and polynomials of degree 1-3 in the term index; coefficients fitted per task",
        });
        if let Some((aid, who)) = via {
            rationale["proposed_by_advisor"] = json!(who);
            rationale["advice_id"] = json!(aid);
            rationale["acceptance"] = json!("the advisor only proposed the form; it was accepted by the same verified search (training and held-out exact reproduction on the being's own logged examples) as a discovered rule");
        }
        let revision = NewRevision {
            entity_type: "rule".into(),
            key: format!("being/rule/{}", rule.id),
            stance: format!("pattern difficulty {}: {}", rule.sig, rule.form.dsl()),
            confidence: ((f.train_ok + f.val_ok + 1) as f64 / (f.train_ok + f.val_ok + 2) as f64).clamp(0.0, 1.0),
            rationale: rationale.to_string(),
            supersedes: None,
            valid_from: at,
            author: if via.is_some() { "rule:being-advice-verified".into() } else { "rule:being-discover".into() },
            caused_by,
        };
        self.state.apply_rule(rule.clone());
        Some(NewRule { rule, event, revision })
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunCfg {
    pub world_seed: Option<u64>,
    pub loop_seed: Option<u64>,
    pub budget: Option<i64>,
    pub explore_c: Option<f64>,
    /// `Some(Chooser::Model)` creates a menu-world being with the learned planner; fixed once created.
    pub chooser: Option<Chooser>,
    pub steps: u64,
    pub refill_when_exhausted: bool,
    /// `Some(true)` creates a being in the extended pattern world (menu world only); fixed once created.
    pub extended: Option<bool>,
    /// Run a discovery pass after every N-th step of the being (None or 0: never).
    pub discover_every: Option<u64>,
    /// `Some(true)` creates a being in a world with scheduled hidden changes (menu world only); fixed once created.
    pub drift: Option<bool>,
    /// `Some(true)` creates a being whose model forgets old evidence and flags changes (menu world only); fixed once created.
    pub adapt: Option<bool>,
    /// Creation-time: where advice comes from (menu world only); fixed once the being exists.
    pub advisor: Option<AdvisorKind>,
    /// Creation-time, simulated advisor only: its quality in 0..1.
    pub advisor_quality: Option<f64>,
    /// Creation-time: true (default with an advisor) learns trust, false follows advice blindly.
    pub trust: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub steps_run: u64,
    pub stopped: StopReason,
    pub reward: i64,
    pub cost: i64,
    pub state: BeingState,
    pub version_before: u64,
    pub version_after: u64,
    pub created: bool,
    /// Rules accepted during this run.
    pub new_rules: Vec<Rule>,
}

impl RunSummary {
    pub fn to_json(&self) -> Value {
        let g = self.state.genesis.clone();
        json!({
            "steps_run": self.steps_run, "stopped": self.stopped.as_str(),
            "reward_this_run": self.reward, "energy_spent_this_run": self.cost,
            "reward_per_energy_this_run": if self.cost > 0 { self.reward as f64 / self.cost as f64 } else { 0.0 },
            "total_steps": self.state.step, "total_reward": self.state.total_reward, "total_energy_spent": self.state.total_cost,
            "budget_left": self.state.budget_left, "refills": self.state.refills,
            "world_seed": g.as_ref().map(|g| g.world_seed), "loop_seed": g.as_ref().map(|g| g.loop_seed),
            "chooser": g.as_ref().map(|g| g.chooser.name()),
            "pattern_families": if g.as_ref().map_or(false, |g| g.extended) { "extended" } else { "standard" },
            "drift": g.as_ref().map_or(false, |g| g.drift), "adapt": g.as_ref().map_or("off", |g| g.adapt.name()), "changes_flagged_total": self.state.changes.len(),
            "advisor": g.as_ref().map_or("none", |g| g.advice.kind.name()),
            "advisor_quality": g.as_ref().and_then(|g| if g.advice.kind == AdvisorKind::Simulated { Some(g.advice.quality) } else { None }),
            "trust_learning": g.as_ref().map_or(false, |g| g.advice.kind != AdvisorKind::None && g.advice.trust),
            "advice": trust_json(&self.state),
            "rules_total": self.state.rules.len(), "rules_accepted_this_run": self.new_rules.iter().map(|r| json!({ "rule_id": r.id, "signature": format!("pattern difficulty {}", r.sig), "form": r.form.code() })).collect::<Vec<_>>(),
            "new_being": self.created, "version_before": self.version_before, "version_after": self.version_after,
            "domains": domain_json(&self.state),
        })
    }
}

/// Trust per (advisor, kind) as the log has taught it, for status output.
pub fn trust_json(st: &BeingState) -> Value {
    Value::Array(
        st.trust
            .cells()
            .map(|((who, kind), c)| {
                json!({
                    "advisor": who, "kind": kind, "advice_given": c.given, "tests_followed_against_model": c.tests, "tests_won": c.test_wins,
                    "trust_mean": (c.mean() * 1000.0).round() / 1000.0, "trust_sd": (c.sd() * 1000.0).round() / 1000.0,
                    "mixing_weight_now": (c.weight(super::trust::DEFAULT_TRUST_BETA) * 1000.0).round() / 1000.0,
                    "degradations_flagged": c.alarms,
                })
            })
            .collect(),
    )
}

pub fn domain_json(st: &BeingState) -> Value {
    Value::Array(
        super::world::ALL_DOMAINS
            .iter()
            .filter(|d| d.idx() < 4 || st.stats[d.idx()].n > 0)
            .map(|d| {
                let s = &st.stats[d.idx()];
                json!({
                    "domain": d.name(), "selected": s.n, "correct": s.correct, "reward": s.reward, "energy": s.cost,
                    "reward_per_energy": (s.efficiency() * 1000.0).round() / 1000.0,
                })
            })
            .collect(),
    )
}

/// Run the loop with the being's own chooser (UCB1, constant and seed from its genesis event).
pub fn run(log: &mut VersionedLog, cfg: &RunCfg, thinker: &dyn Thinker, policy: &Policy) -> Result<RunSummary, LogError> {
    run_with(log, cfg, thinker, policy, None)
}

/// As `run`, with another picker (baselines written to a log, tests).
pub fn run_with(log: &mut VersionedLog, cfg: &RunCfg, thinker: &dyn Thinker, policy: &Policy, picker: Option<&mut dyn DomainPicker>) -> Result<RunSummary, LogError> {
    run_full(log, cfg, thinker, policy, picker, None)
}

/// As `run`, taking advice from `advisor` (which must match the advisor the being was created with).
pub fn run_advised(log: &mut VersionedLog, cfg: &RunCfg, thinker: &dyn Thinker, policy: &Policy, advisor: Option<&dyn Advisor>) -> Result<RunSummary, LogError> {
    run_full(log, cfg, thinker, policy, None, advisor)
}

pub fn run_full(log: &mut VersionedLog, cfg: &RunCfg, thinker: &dyn Thinker, policy: &Policy, picker: Option<&mut dyn DomainPicker>, advisor: Option<&dyn Advisor>) -> Result<RunSummary, LogError> {
    let version_before = log.latest_version();
    let mut state = BeingState::from_snapshot(&log.open_latest());
    let mut created = false;
    // validated before anything is written, so a refused request never creates a being
    if cfg.discover_every.unwrap_or(0) > 0 && !state.genesis.as_ref().map_or(cfg.extended == Some(true), |g| g.extended) {
        return Err(LogError::Invalid("discovery needs a being created with pattern_families extended (only then are revealed examples logged)".into()));
    }
    match &state.genesis {
        Some(g) => {
            let bad = |what: &str, a: String, b: String| Err(LogError::Invalid(format!("this being already has {what} {a}; the request says {b}. A being keeps one world and one seed")));
            if let Some(w) = cfg.world_seed {
                if w != g.world_seed {
                    return bad("world_seed", g.world_seed.to_string(), w.to_string());
                }
            }
            if let Some(s) = cfg.loop_seed {
                if s != g.loop_seed {
                    return bad("loop seed", g.loop_seed.to_string(), s.to_string());
                }
            }
            if let Some(e) = cfg.extended {
                if e != g.extended {
                    return bad("pattern_families", if g.extended { "extended" } else { "standard" }.into(), if e { "extended" } else { "standard" }.into());
                }
            }
            if let Some(e) = cfg.drift {
                if e != g.drift {
                    return bad("drift", if g.drift { "on" } else { "off" }.into(), if e { "on" } else { "off" }.into());
                }
            }
            if let Some(e) = cfg.adapt {
                if e != (g.adapt != AdaptMode::Off) {
                    return bad("adapt", if g.adapt != AdaptMode::Off { "on" } else { "off" }.into(), if e { "on" } else { "off" }.into());
                }
            }
            if let Some(k) = cfg.advisor {
                if k != g.advice.kind {
                    return bad("advisor", g.advice.kind.name().into(), k.name().into());
                }
            }
            if let Some(q) = cfg.advisor_quality {
                if g.advice.kind != AdvisorKind::Simulated || q != g.advice.quality {
                    return bad("advisor_quality", g.advice.quality.to_string(), q.to_string());
                }
            }
            if let Some(t) = cfg.trust {
                if g.advice.kind == AdvisorKind::None || t != g.advice.trust {
                    return bad("trust setting", if g.advice.trust { "on" } else { "off" }.into(), if t { "on" } else { "off" }.into());
                }
            }
            if let Some(c) = cfg.chooser {
                if c != g.chooser {
                    return bad("chooser", g.chooser.name().to_string(), c.name().to_string());
                }
            }
        }
        None => {
            let budget = cfg.budget.unwrap_or(DEFAULT_BUDGET);
            if !(1..=1_000_000).contains(&budget) {
                return Err(LogError::Invalid("budget must be 1..=1000000 energy units".into()));
            }
            let chooser = cfg.chooser.unwrap_or(Chooser::Ucb);
            let c = cfg.explore_c.unwrap_or(if chooser == Chooser::Model { DEFAULT_BETA } else { DEFAULT_EXPLORE_C });
            if !c.is_finite() || !(0.0..=10.0).contains(&c) {
                return Err(LogError::Invalid("explore_c must be within 0..=10".into()));
            }
            let extended = cfg.extended.unwrap_or(false);
            if extended && chooser != Chooser::Model {
                return Err(LogError::Invalid("pattern_families extended needs chooser model (the menu world)".into()));
            }
            let (drift, adapt) = (cfg.drift.unwrap_or(false), cfg.adapt.unwrap_or(false));
            if (drift || adapt) && chooser != Chooser::Model {
                return Err(LogError::Invalid("drift and adapt need chooser model (the menu world)".into()));
            }
            let kind = cfg.advisor.unwrap_or(AdvisorKind::None);
            if kind != AdvisorKind::None && chooser != Chooser::Model {
                return Err(LogError::Invalid("an advisor needs chooser model (the menu world)".into()));
            }
            if kind == AdvisorKind::None && (cfg.advisor_quality.is_some() || cfg.trust.is_some()) {
                return Err(LogError::Invalid("advisor_quality and trust only apply with an advisor".into()));
            }
            let quality = match (kind, cfg.advisor_quality) {
                (AdvisorKind::Simulated, Some(q)) if q.is_finite() && (0.0..=1.0).contains(&q) => q,
                (AdvisorKind::Simulated, _) => return Err(LogError::Invalid("advisor simulated needs advisor_quality within 0..=1".into())),
                (_, Some(_)) => return Err(LogError::Invalid("advisor_quality only applies with advisor simulated".into())),
                (_, None) => 0.0,
            };
            let advice = AdviceSetup { kind, quality, trust: cfg.trust.unwrap_or(true) };
            let g = Genesis { world_seed: cfg.world_seed.unwrap_or(1), loop_seed: cfg.loop_seed.unwrap_or(1), budget, explore_c: c, chooser, extended, drift, adapt: if adapt { AdaptMode::Full } else { AdaptMode::Off }, advice };
            let (ev, id) = genesis_event(&g);
            log.append_events(&[ev])?;
            state.start(g, id);
            created = true;
        }
    }
    let g = state.genesis.clone().expect("genesis exists now");
    if advisor.is_some() && g.advice.kind == AdvisorKind::None {
        return Err(LogError::Invalid("this being was created without an advisor; the advisor is fixed at creation".into()));
    }
    let discover_every = cfg.discover_every.unwrap_or(0);
    let mut own = Ucb1::new(g.loop_seed, g.explore_c);
    let picker: &mut dyn DomainPicker = match picker {
        Some(p) => p,
        None => &mut own,
    };

    let menu = g.chooser == Chooser::Model;
    let mut mc = ModelChooser::for_genesis(&g);
    let mut engine = Engine::new(world_for(&g), state, thinker).with_advisor(advisor);
    let mut new_rules: Vec<Rule> = vec![];
    let mut pending = Batch::default();
    let mut pending_steps = 0usize;
    let (mut done, mut reward, mut cost) = (0u64, 0i64, 0i64);
    let stopped;
    loop {
        if let Some(r) = policy.gate(done) {
            stopped = r;
            break;
        }
        let stepped = if menu { engine.step_menu(&mut mc) } else { engine.step(picker) };
        match stepped {
            Ok(out) => {
                done += 1;
                reward += out.outcome.reward;
                cost += out.outcome.cost;
                pending.events.extend(out.events);
                pending.revisions.push(out.revision);
                for (ev, rev) in out.changes {
                    pending.events.push(ev);
                    pending.revisions.push(rev);
                }
                pending.predictions.extend(out.prediction);
                pending.outcomes.extend(out.resolved);
                for nr in out.rules {
                    pending.revisions.push(nr.revision);
                    new_rules.push(nr.rule);
                }
                pending_steps += 1;
                if discover_every > 0 && engine.state.step % discover_every == 0 {
                    for nr in engine.discover_pass() {
                        pending.events.push(nr.event);
                        pending.revisions.push(nr.revision);
                        new_rules.push(nr.rule);
                    }
                }
                if pending_steps >= FLUSH_EVERY {
                    log.commit_batch(std::mem::take(&mut pending))?;
                    pending_steps = 0;
                }
            }
            Err(StopReason::BudgetExhausted) if policy.refill_when_exhausted => {
                let (ev, clock) = refill_event(g.budget, engine.state.clock, engine.state.refills);
                engine.state.apply_refill(g.budget, clock);
                pending.events.push(ev);
                let none_left = if menu {
                    let offers = offers_of(&engine.world.offers_at(engine.state.step, engine.state.clock));
                    mc.choose(&engine.state, &offers).is_none()
                } else {
                    engine.affordable(&engine.world.menu(engine.state.step)).is_empty()
                };
                if none_left {
                    stopped = StopReason::BudgetExhausted;
                    break;
                }
            }
            Err(r) => {
                stopped = r;
                break;
            }
        }
    }
    let (carry_events, carry_rules) = engine.take_carry();
    pending.events.extend(carry_events);
    for nr in carry_rules {
        pending.revisions.push(nr.revision);
        new_rules.push(nr.rule);
    }
    if !pending.events.is_empty() {
        log.commit_batch(pending)?;
    }
    Ok(RunSummary { steps_run: done, stopped, reward, cost, state: engine.state, version_before, version_after: log.latest_version(), created, new_rules })
}
