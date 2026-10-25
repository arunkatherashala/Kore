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

use super::chooser::{DomainPicker, Pick, Ucb1, DEFAULT_EXPLORE_C};
use super::discover::{self, terms_tag, Rule};
use super::planner::{Action, MenuChooser, ModelChooser, Offer, DEFAULT_BETA};
use super::policy::{Policy, StopReason};
use super::solver::{Context, Strategy, Thinker};
use super::state::{genesis_event, refill_event, BeingState, Chooser, Genesis, Outcome, SOURCE};
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
}

impl<'a> Engine<'a> {
    pub fn new(world: World, state: BeingState, thinker: &'a dyn Thinker) -> Engine<'a> {
        Engine { world, state, thinker }
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
        let ctx = Context { step, task: v, noise: self.world.noise(step, v.domain), stats: &self.state.stats, rules: &self.state.rules };
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
        Ok(StepOut { outcome, pick: Some(pick), action: None, predicted_p: None, decision_event_id: dev_id, events: vec![dev, aev, oev], revision, prediction: Some(prediction), resolved: Some(resolved) })
    }

    /// Menu mode: one step over `MENU_SIZE` offered tasks. The chooser picks
    /// (offer, strategy) or skips; the thinker answers with that strategy.
    pub fn step_menu(&mut self, chooser: &mut dyn MenuChooser) -> Result<StepOut, StopReason> {
        let step = self.state.step;
        let tasks = self.world.offers(step);
        let offers = offers_of(&tasks);
        let Some(ch) = chooser.choose(&self.state, &offers) else {
            return Err(StopReason::BudgetExhausted);
        };
        let label = ch.action.label();
        let evidence = self.state.evidence_ids();
        let t0 = self.state.clock;
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
                    let ctx = Context { step, task: v, noise: self.world.menu_noise(step, slot), stats: &self.state.stats, rules: &self.state.rules };
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
        self.state.apply(&outcome);
        Ok(StepOut { outcome, pick: None, action: Some(label), predicted_p, decision_event_id: dev_id, events: vec![dev, aev, oev], revision, prediction, resolved })
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
            let mut rule = Rule::new(f.sig, f.form, f.train_ok, f.val_ok, self.state.step);
            if self.state.rules.iter().any(|r| r.id == rule.id) {
                continue;
            }
            let at = iso_at(self.state.clock);
            let (event, ev_id) = rule.event(at.clone());
            rule.event_id = ev_id.clone();
            let mut caused_by = f.cite.clone();
            caused_by.push(ev_id);
            let rationale = json!({
                "rule_id": rule.id, "signature": format!("pattern difficulty {}", rule.sig), "form": rule.form.code(), "dsl": rule.form.dsl(),
                "training_examples_reproduced": f.train_ok, "held_out_examples_reproduced": f.val_ok,
                "new_training_support": f.new_train, "new_held_out_support": f.new_val,
                "min_training_support": discover::K_TRAIN, "min_held_out_support": discover::K_VAL,
                "cited_examples": f.cite.len(), "accepted_at_step": self.state.step,
                "grammar": "author-defined: 14 linear forms over lags 1-3 (optional constant) and polynomials of degree 1-3 in the term index; coefficients fitted per task",
            });
            let revision = NewRevision {
                entity_type: "rule".into(),
                key: format!("being/rule/{}", rule.id),
                stance: format!("pattern difficulty {}: {}", rule.sig, rule.form.dsl()),
                confidence: ((f.train_ok + f.val_ok + 1) as f64 / (f.train_ok + f.val_ok + 2) as f64).clamp(0.0, 1.0),
                rationale: rationale.to_string(),
                supersedes: None,
                valid_from: at,
                author: "rule:being-discover".into(),
                caused_by,
            };
            self.state.apply_rule(rule.clone());
            out.push(NewRule { rule, event, revision });
        }
        out
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
            "rules_total": self.state.rules.len(), "rules_accepted_this_run": self.new_rules.iter().map(|r| json!({ "rule_id": r.id, "signature": format!("pattern difficulty {}", r.sig), "form": r.form.code() })).collect::<Vec<_>>(),
            "new_being": self.created, "version_before": self.version_before, "version_after": self.version_after,
            "domains": domain_json(&self.state),
        })
    }
}

pub fn domain_json(st: &BeingState) -> Value {
    Value::Array(
        DOMAINS
            .iter()
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
    let version_before = log.latest_version();
    let mut state = BeingState::from_snapshot(&log.open_latest());
    let mut created = false;
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
            let g = Genesis { world_seed: cfg.world_seed.unwrap_or(1), loop_seed: cfg.loop_seed.unwrap_or(1), budget, explore_c: c, chooser, extended };
            let (ev, id) = genesis_event(&g);
            log.append_events(&[ev])?;
            state.start(g, id);
            created = true;
        }
    }
    let g = state.genesis.clone().expect("genesis exists now");
    let discover_every = cfg.discover_every.unwrap_or(0);
    if discover_every > 0 && !g.extended {
        return Err(LogError::Invalid("discovery needs a being created with pattern_families extended (only then are revealed examples logged)".into()));
    }
    let mut own = Ucb1::new(g.loop_seed, g.explore_c);
    let picker: &mut dyn DomainPicker = match picker {
        Some(p) => p,
        None => &mut own,
    };

    let menu = g.chooser == Chooser::Model;
    let mut mc = ModelChooser::new(g.explore_c);
    let mut engine = Engine::new(World::with_extended(g.world_seed, g.extended), state, thinker);
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
                pending.predictions.extend(out.prediction);
                pending.outcomes.extend(out.resolved);
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
                    let offers = offers_of(&engine.world.offers(engine.state.step));
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
    if !pending.events.is_empty() {
        log.commit_batch(pending)?;
    }
    Ok(RunSummary { steps_run: done, stopped, reward, cost, state: engine.state, version_before, version_after: log.latest_version(), created, new_rules })
}
