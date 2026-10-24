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
use super::policy::{Policy, StopReason};
use super::solver::{Context, Thinker};
use super::state::{genesis_event, refill_event, BeingState, Genesis, Outcome, SOURCE};
use super::world::{iso_at, Domain, Task, World, DOMAINS, SECS_PER_ENERGY};
use crate::versioned::{event_id, prediction_id, Batch, LogError, NewEvent, NewOutcome, NewPrediction, NewRevision, VersionedLog};
use serde_json::{json, Value};

pub const FLUSH_EVERY: usize = 25;
pub const DEFAULT_BUDGET: i64 = 2000;

/// Everything one step adds to the log.
pub struct StepOut {
    pub outcome: Outcome,
    pub pick: Pick,
    pub decision_event_id: String,
    pub events: Vec<NewEvent>,
    pub revision: NewRevision,
    pub prediction: NewPrediction,
    pub resolved: NewOutcome,
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
        let ctx = Context { step, task: v, noise: self.world.noise(step, v.domain), stats: &self.state.stats };
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
        };
        self.state.apply(&outcome);
        Ok(StepOut { outcome, pick, decision_event_id: dev_id, events: vec![dev, aev, oev], revision, prediction, resolved })
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunCfg {
    pub world_seed: Option<u64>,
    pub loop_seed: Option<u64>,
    pub budget: Option<i64>,
    pub explore_c: Option<f64>,
    pub steps: u64,
    pub refill_when_exhausted: bool,
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
        }
        None => {
            let budget = cfg.budget.unwrap_or(DEFAULT_BUDGET);
            if !(1..=1_000_000).contains(&budget) {
                return Err(LogError::Invalid("budget must be 1..=1000000 energy units".into()));
            }
            let c = cfg.explore_c.unwrap_or(DEFAULT_EXPLORE_C);
            if !c.is_finite() || !(0.0..=10.0).contains(&c) {
                return Err(LogError::Invalid("explore_c must be within 0..=10".into()));
            }
            let g = Genesis { world_seed: cfg.world_seed.unwrap_or(1), loop_seed: cfg.loop_seed.unwrap_or(1), budget, explore_c: c };
            let (ev, id) = genesis_event(&g);
            log.append_events(&[ev])?;
            state.start(g, id);
            created = true;
        }
    }
    let g = state.genesis.clone().expect("genesis exists now");
    let mut own = Ucb1::new(g.loop_seed, g.explore_c);
    let picker: &mut dyn DomainPicker = match picker {
        Some(p) => p,
        None => &mut own,
    };

    let mut engine = Engine::new(World::new(g.world_seed), state, thinker);
    let mut pending = Batch::default();
    let mut pending_steps = 0usize;
    let (mut done, mut reward, mut cost) = (0u64, 0i64, 0i64);
    let stopped;
    loop {
        if let Some(r) = policy.gate(done) {
            stopped = r;
            break;
        }
        match engine.step(picker) {
            Ok(out) => {
                done += 1;
                reward += out.outcome.reward;
                cost += out.outcome.cost;
                pending.events.extend(out.events);
                pending.revisions.push(out.revision);
                pending.predictions.push(out.prediction);
                pending.outcomes.push(out.resolved);
                pending_steps += 1;
                if pending_steps >= FLUSH_EVERY {
                    log.commit_batch(std::mem::take(&mut pending))?;
                    pending_steps = 0;
                }
            }
            Err(StopReason::BudgetExhausted) if policy.refill_when_exhausted => {
                let (ev, clock) = refill_event(g.budget, engine.state.clock, engine.state.refills);
                engine.state.apply_refill(g.budget, clock);
                pending.events.push(ev);
                let menu = engine.world.menu(engine.state.step);
                if engine.affordable(&menu).is_empty() {
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
    Ok(RunSummary { steps_run: done, stopped, reward, cost, state: engine.state, version_before, version_after: log.latest_version(), created })
}
