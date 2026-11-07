//! Track B9 in World v2: the existing learner plus experiments (`being/experiment.rs`), opt-in.
//!
//! `Exp2` wraps the existing learner (`Learner2`, unchanged). Every step it asks the learner for its
//! choice, then asks `experiment::decide` whether one of the offered sequence tasks is worth more as
//! an experiment. An exploit step is EXACTLY the learner's choice (same label, policy `w2-model-ucb`,
//! so the existing replay re-derives it); an experiment step has policy `w2-experiment` and carries
//! in its decision record the live hypotheses, the predicted split, the value of information and the
//! budget. The being sees the prefix of an offered sequence task (its prompt); `Exp2` rebuilds the
//! offers from the logged world config (as the replay does) and reads ONLY the visible task fields
//! `ftype`, `fam`, `difficulty`, `reward` and `params` (a test scans this file).
//!
//! The logged run (`run_v2_exp`) is `engine::run_v2` plus: an `experiment_cfg` note at the start of the
//! run, an `experiment` note and revision after every experiment (hypotheses kept and rejected by the
//! revealed answer, citing the decision and outcome events), and for every accepted rule whose cited
//! examples include an experiment's outcome an `experiment_credit` revision citing those experiments.
//! The kill switch and the energy cap are checked before every step as in `run_v2`.

use super::engine::{genesis_event2, refusal_event2, stop_event2, Engine2, Run2Cfg, Run2Summary, State2, FLUSH_EVERY};
use super::learner::{Act2, Choice2, Chooser2, Learner2, Offer2, LEARNER_NAME};
use super::solve::sig2;
use super::world::{FType, World2, SOURCE2};
use crate::being::controller::Refusal;
use crate::being::experiment::{decide, pick_json, Decision, ExpBook, ExpCfg, HypCache, Opt, Pick, Probe};
use crate::being::policy::{Policy, StopReason};
use crate::being::state::tag;
use crate::being::world::iso_at;
use crate::versioned::{event_id, Batch, EventRow, LogError, NewEvent, NewRevision, VersionedLog};
use serde_json::json;
use std::collections::HashMap;

pub const EXP_POLICY: &str = "w2-experiment";

/// Record of one experiment (for the log and the measurement).
#[derive(Debug, Clone, PartialEq)]
pub struct ExpRec2 {
    pub step: u64,
    pub fam: usize,
    pub difficulty: u8,
    pub pick: Pick,
}

pub struct Exp2 {
    pub base: Learner2,
    pub cfg: ExpCfg,
    world: World2,
    pub cache: HypCache,
    pub book: ExpBook,
    pub life_budget: i64,
    /// Experiments of this chooser, in order.
    pub recs: Vec<ExpRec2>,
    /// Refusals by the experiment budget not yet handed out (each kind once per chooser).
    pub refusals: Vec<Refusal>,
    refused_kinds: Vec<&'static str>,
    pub compose_blocks: bool,
    /// B10: the exploit choice comes from the lookahead planner (over the same learner) instead of the learner itself.
    pub look: Option<crate::being::lookahead::adapt::Look2>,
    /// B10: retirements seen by the hypothesis cache (a retirement removes examples from the search set: the cache starts over).
    seen_retirements: usize,
}

impl Exp2 {
    /// `world` must be built from the being's logged config (`World2::new(state.cfg)`): only offers are read from it.
    pub fn new(base: Learner2, cfg: ExpCfg, world: World2, life_budget: i64) -> Exp2 {
        Exp2 { base, cfg, world, cache: HypCache::new(), book: ExpBook::default(), life_budget, recs: vec![], refusals: vec![], refused_kinds: vec![], compose_blocks: false, look: None, seen_retirements: 0 }
    }

    fn probes(&self, st: &State2, offers: &[Offer2]) -> Vec<Probe> {
        let tasks = self.world.offers(st.step, st.clock);
        let mut v = vec![];
        for o in offers {
            let Some(t) = tasks.get(o.slot) else { continue };
            let view = &t.view;
            if view.ftype == FType::Seq && view.fam == o.fam && view.difficulty == o.difficulty {
                v.push(Probe { slot: o.slot, sig: sig2(o.fam, o.difficulty), prefix: view.params.clone(), listed_reward: view.reward });
            }
        }
        v
    }
}

impl Chooser2 for Exp2 {
    fn name(&self) -> String {
        EXP_POLICY.into()
    }
    fn choose(&mut self, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
        let base = match self.look.as_mut() {
            Some(l) => l.choose(st, offers)?,
            None => self.base.choose(st, offers)?,
        };
        let blocks = if self.compose_blocks { st.blocks() } else { vec![] };
        if st.retirements.len() != self.seen_retirements {
            self.cache = HypCache::new();
            self.seen_retirements = st.retirements.len();
        }
        self.cache.update(&st.search_examples(), &blocks);
        let probes = self.probes(st, offers);
        let opts: Vec<Opt> = self
            .base
            .candidates(st, offers)
            .iter()
            .map(|c| {
                let listed = offers.iter().find(|o| o.slot == c.slot).map_or(0, |o| o.listed_cost);
                Opt { slot: c.slot, strategy: c.strategy, p: c.p, cost_est: c.cost_est, mean_score: c.mean_score, affordable: c.affordable && c.strategy.energy(listed) <= st.budget_left, energy: c.strategy.energy(listed) }
            })
            .collect();
        let d = decide(&self.cfg, &self.book, &self.cache, &st.rules, &probes, &opts, self.life_budget, st.total_cost, st.step);
        match d {
            Decision::Exploit { considered, refused } => {
                let mut ch = base;
                if let Some(kind) = refused {
                    match kind {
                        "max_experiments_per_run" => self.book.refused_count += 1,
                        _ => self.book.refused_energy += 1,
                    }
                    if !self.refused_kinds.contains(&kind) {
                        self.refused_kinds.push(kind);
                        let (limit, req) = match kind {
                            "max_experiments_per_run" => (self.cfg.max_experiments, self.book.n + 1),
                            _ => (self.cfg.energy_pm as u64, if self.life_budget > 0 { ((self.book.energy + considered.as_ref().map_or(0, |p| p.energy)) * 1000 / self.life_budget).max(0) as u64 } else { 0 }),
                        };
                        self.refusals.push(Refusal::new(kind, limit, req, format!("an experiment was wanted at step {} but the experiment budget is used up ({} experiments, {} energy units so far); the planner's choice was kept", st.step, self.book.n, self.book.energy)));
                    }
                }
                if let (Some(p), Some(obj)) = (considered, ch.rationale.as_object_mut()) {
                    obj.insert("experiment_considered".into(), pick_json(&p, false, &self.book, &self.cfg, refused));
                }
                Some(ch)
            }
            Decision::Experiment(p) => {
                let o = offers.iter().find(|o| o.slot == p.slot).expect("probe of an offer");
                let c = self.base.candidates(st, offers).into_iter().find(|c| c.slot == p.slot && c.strategy == p.strategy).expect("candidate");
                self.book.take(&p);
                let mut rationale = json!({
                    "policy": EXP_POLICY, "learner": "B1 world model and planner (unchanged) plus experiments (being/experiment.rs)",
                    "planner_choice": base.act.label(), "planner_score": crate::being::world2::learner::r3(base.score),
                });
                rationale["experiment"] = pick_json(&p, true, &self.book, &self.cfg, None);
                self.recs.push(ExpRec2 { step: st.step, fam: o.fam, difficulty: o.difficulty, pick: p.clone() });
                Some(Choice2 { act: Act2::Attempt { slot: p.slot, strategy: p.strategy }, policy: EXP_POLICY.into(), why: p.why.clone(), p: c.p, sd: c.sd, cost_est: c.cost_est, score: p.score, rationale })
            }
        }
    }
}

// ---- the logged run ----

pub fn exp_cfg_event(cfg: &ExpCfg, step: u64, clock: i64) -> NewEvent {
    let mut tags = vec!["experiment_cfg".to_string(), format!("step:{step}")];
    tags.extend(cfg.tags());
    NewEvent {
        source: SOURCE2.into(),
        occurred_at: iso_at(clock),
        kind: "note".into(),
        text: format!(
            "experiments on for this run from step {step}: value of information kappa {:.3}, at most {} per mille of the life budget and {} experiments{}; rule acceptance is unchanged",
            cfg.kappa, cfg.energy_pm, cfg.max_experiments, if cfg.random.is_some() { " (random-probe control)" } else { "" }
        ),
        tags,
        redacted: false,
    }
}

/// Experiment energy of the life so far, folded from the log (decisions with policy `w2-experiment` and their outcomes).
pub fn experiment_energy_from_log(rows: &[EventRow]) -> i64 {
    let mut exp_steps: HashMap<String, ()> = HashMap::new();
    let mut e = 0;
    for r in rows.iter().filter(|r| r.source == SOURCE2) {
        if r.kind == "decision" && tag(&r.tags, "policy") == Some(EXP_POLICY) {
            if let Some(s) = tag(&r.tags, "step") {
                exp_steps.insert(s.to_string(), ());
            }
        } else if r.kind == "outcome" {
            if let Some(s) = tag(&r.tags, "step") {
                if exp_steps.contains_key(s) {
                    e += tag(&r.tags, "cost").and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);
                }
            }
        }
    }
    e
}

/// `run_v2` with experiments (`exp`), on the being's own log.
pub fn run_v2_exp(log: &mut VersionedLog, cfg: &Run2Cfg, policy: &Policy, exp: ExpCfg) -> Result<(Run2Summary, Vec<ExpRec2>), LogError> {
    let version_before = log.latest_version();
    let rows: Vec<EventRow> = log.open_latest().rows();
    let mut state = State2::from_events(&rows);
    let mut created = false;
    match &state.cfg {
        Some(c) => {
            if let Some(w) = &cfg.world {
                if w != c {
                    return Err(LogError::Invalid(format!("this World v2 being already lives in world {:?}; the request says {:?}. A being keeps one world", c, w)));
                }
            }
        }
        None => {
            let w = cfg.world.clone().ok_or_else(|| LogError::Invalid("no World v2 being exists yet; a world config is needed to create one".into()))?;
            w.validate().map_err(LogError::Invalid)?;
            let l = cfg.learner.unwrap_or_default();
            if l.beta_milli > 10_000 || l.skip_milli > 10_000 {
                return Err(LogError::Invalid("learner constants must be within 0..=10".into()));
            }
            let (ev, id) = genesis_event2(&w, &l);
            log.append_events(&[ev])?;
            state.start(w, l, id);
            created = true;
        }
    }
    let wcfg = state.cfg.clone().expect("started");
    let mut chooser = Exp2::new(state.learner.learner(), exp, World2::new(wcfg.clone()), wcfg.budget);
    chooser.compose_blocks = cfg.ctl.as_ref().map_or(false, |l| l.compose);
    chooser.book.energy = experiment_energy_from_log(&rows);
    let mut engine = Engine2::new(state, cfg.ctl.clone());
    let mut pending = Batch::default();
    pending.events.push(exp_cfg_event(&exp, engine.state.step, engine.state.clock));
    let (mut new_rules, mut truths) = (vec![], vec![]);
    let mut pending_steps = 0usize;
    let (mut done, mut rep, mut tru, mut cost) = (0u64, 0i64, 0i64, 0i64);
    // outcome event id of an experiment -> (experiment note event id, step)
    let mut exp_outcomes: HashMap<String, (String, u64)> = HashMap::new();
    let stopped;
    loop {
        if let Some(r) = policy.gate_cost(done, cost) {
            stopped = r;
            if matches!(r, StopReason::Killed | StopReason::EnergyCap) {
                pending.events.push(stop_event2(r.as_str(), engine.state.step, engine.state.clock));
                if r == StopReason::EnergyCap {
                    engine.refuse(Refusal::new("max_energy_per_run", policy.max_energy.unwrap_or(0).max(0) as u64, cost.max(0) as u64, format!("the run spent {cost} energy units and may not start another step")));
                }
            }
            break;
        }
        let n_before = chooser.recs.len();
        match engine.step(&mut chooser) {
            Ok(out) => {
                done += 1;
                rep += out.outcome.reward;
                tru += out.truth.true_reward;
                cost += out.outcome.cost;
                truths.push(out.truth.clone());
                let dev_id = event_id(&out.events[0].source, &out.events[0].occurred_at, &out.events[0].text);
                pending.events.extend(out.events);
                pending.revisions.push(out.revision);
                pending.predictions.extend(out.prediction);
                pending.outcomes.extend(out.resolved);
                if chooser.recs.len() > n_before {
                    let rec = chooser.recs.last().expect("experiment").clone();
                    // the chooser's cache took in the examples before this step's answer (in `choose`): the verdict is against it
                    let (ev, rv) = experiment_records(&rec, &out.outcome, &chooser.cache, &dev_id);
                    let nid = event_id(&ev.source, &ev.occurred_at, &ev.text);
                    exp_outcomes.insert(out.outcome.event_id.clone(), (nid, rec.step));
                    pending.events.push(ev);
                    pending.revisions.push(rv);
                }
                pending_steps += 1;
                if cfg.discover_every > 0 && engine.state.step % cfg.discover_every == 0 {
                    for nr in engine.discover_pass() {
                        let credit: Vec<(String, u64)> = nr.revision.caused_by.iter().filter_map(|c| exp_outcomes.get(c).cloned()).collect();
                        let rule_ev = nr.rule.event_id.clone();
                        pending.events.push(nr.event);
                        pending.revisions.push(nr.revision);
                        if !credit.is_empty() {
                            pending.revisions.push(credit_revision(&nr.rule, &rule_ev, &credit, engine.state.clock));
                        }
                        new_rules.push(nr.rule);
                    }
                }
                for r in engine.take_refusals().into_iter().chain(std::mem::take(&mut chooser.refusals)) {
                    pending.events.push(refusal_event2(&r, engine.state.step, engine.state.clock));
                    engine.refusals_total.push(r);
                }
                if pending_steps >= FLUSH_EVERY {
                    log.commit_batch(std::mem::take(&mut pending))?;
                    pending_steps = 0;
                }
            }
            Err(r) => {
                stopped = r;
                break;
            }
        }
    }
    for r in engine.take_refusals().into_iter().chain(std::mem::take(&mut chooser.refusals)) {
        pending.events.push(refusal_event2(&r, engine.state.step, engine.state.clock));
        engine.refusals_total.push(r);
    }
    if !pending.events.is_empty() {
        log.commit_batch(pending)?;
    }
    let recs = chooser.recs.clone();
    Ok((Run2Summary { steps_run: done, stopped, reported_reward: rep, true_reward: tru, cost, state: engine.state, created, new_rules, refusals: engine.refusals_total, truths, version_before, version_after: log.latest_version() }, recs))
}

/// The note and revision of one experiment: what was predicted, what was revealed, which hypotheses it kept or rejected.
pub fn experiment_records(rec: &ExpRec2, o: &super::engine::Outcome2, before: &HypCache, decision_event: &str) -> (NewEvent, NewRevision) {
    let p = &rec.pick;
    let (kept, rejected, revealed) = match &o.example {
        Some(ex) => {
            let (k, r) = before.verdict(ex.sig, &ex.terms, ex.next);
            (k, r, Some(ex.next))
        }
        None => (vec![], vec![], None),
    };
    let text = format!(
        "experiment at step {}: family f{:02} difficulty {} with {}; {} live hypotheses split into {} predicted outcomes ({:.3} bits expected, support gain {:.3}); revealed next term {}; kept {} [{}], rejected {} [{}]",
        rec.step, rec.fam, rec.difficulty, p.strategy.name(), p.assess.live, p.assess.groups.len(), p.assess.eig_bits, p.assess.support,
        revealed.map_or("none (not attempted)".to_string(), |v| v.to_string()), kept.len(), kept.join(" "), rejected.len(), rejected.join(" ")
    );
    let mut tags = vec!["experiment".to_string(), format!("step:{}", rec.step), format!("fam:{}", rec.fam), format!("difficulty:{}", rec.difficulty), format!("sig:{}", p.sig), format!("kept:{}", kept.len()), format!("rejected:{}", rejected.len())];
    if let Some(v) = revealed {
        tags.push(format!("revealed:{v}"));
    }
    let ev = NewEvent { source: SOURCE2.into(), occurred_at: iso_at(o.clock), kind: "note".into(), text, tags, redacted: false };
    let ev_id = event_id(&ev.source, &ev.occurred_at, &ev.text);
    let rv = NewRevision {
        entity_type: "decision".into(),
        key: format!("being/world2/experiment/{:06}", rec.step),
        stance: format!("family f{:02} difficulty {}: {} hypotheses rejected, {} kept", rec.fam, rec.difficulty, rejected.len(), kept.len()),
        confidence: 1.0,
        rationale: json!({ "prediction": p.assess.to_json(), "revealed_next": revealed, "kept": kept, "rejected": rejected, "note": "the revealed answer is what the being was TOLD; with feedback noise it can be wrong" }).to_string(),
        supersedes: None,
        valid_from: iso_at(o.clock),
        author: format!("rule:being-{EXP_POLICY}"),
        caused_by: vec![decision_event.to_string(), o.event_id.clone(), ev_id],
    };
    (ev, rv)
}

/// A rule whose cited examples include experiments: the revision that credits them.
pub fn credit_revision(rule: &crate::being::discover::Rule, rule_event: &str, credit: &[(String, u64)], clock: i64) -> NewRevision {
    let mut caused_by = vec![rule_event.to_string()];
    caused_by.extend(credit.iter().map(|c| c.0.clone()));
    NewRevision {
        entity_type: "decision".into(),
        key: format!("being/world2/experiment_credit/{}", rule.id),
        stance: format!("rule {} rests on {} examples revealed by experiments", rule.id, credit.len()),
        confidence: 1.0,
        rationale: json!({ "rule_id": rule.id, "experiment_steps": credit.iter().map(|c| c.1).collect::<Vec<_>>() }).to_string(),
        supersedes: None,
        valid_from: iso_at(clock),
        author: format!("rule:being-{EXP_POLICY}"),
        caused_by,
    }
}

/// Replay of the experimenter from the log alone: every decision (exploit and experiment) is re-derived
/// with a fresh `Exp2` that sees the state folded from the events before it. Returns (step, recorded, re-derived).
pub fn replay_experiments2(rows: &[EventRow]) -> Vec<(u64, String, String)> {
    let mut st = State2::default();
    let mut chooser: Option<Exp2> = None;
    let mut out = vec![];
    for e in rows.iter().filter(|e| e.source == SOURCE2) {
        if e.kind == "note" && e.tags.split(',').any(|t| t == "experiment_cfg") {
            if let (Some(c), Some(w)) = (ExpCfg::from_tags(&e.tags), st.cfg.clone()) {
                let mut x = Exp2::new(st.learner.learner(), c, World2::new(w.clone()), w.budget);
                x.book.energy = chooser.as_ref().map_or(0, |p| p.book.energy);
                x.cache = chooser.take().map(|p| p.cache).unwrap_or_default();
                chooser = Some(x);
            }
        }
        if e.kind == "decision" {
            let pol = tag(&e.tags, "policy");
            if let (Some(x), Some(rec), Some(step), true) = (chooser.as_mut(), tag(&e.tags, "action"), tag(&e.tags, "step").and_then(|s| s.parse().ok()), pol == Some(EXP_POLICY) || pol == Some(LEARNER_NAME)) {
                let offers = super::engine::offers_of2(&x.world.offers(st.step, st.clock));
                let again = x.choose(&st, &offers).map(|c| format!("{}|{}", c.policy, c.act.label())).unwrap_or_else(|| "none".into());
                out.push((step, format!("{}|{rec}", pol.unwrap_or("")), again));
            }
        }
        st.fold_event(e);
    }
    out
}
