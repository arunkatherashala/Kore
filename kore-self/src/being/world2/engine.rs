//! The World v2 loop: observe -> decide -> act -> check -> record, on the being's own log.
//!
//! Same shape as the menu loop of `loop.rs`: the state is folded from the being's log (source
//! `being:world2`), the chooser decides from that state and the observable offers, the solver
//! answers, the world's exact checker decides, and every step appends a decision, an action and an
//! outcome event, one `decision` revision citing its evidence (the genesis event, the latest
//! outcomes, the latest outcomes of the chosen (family, strategy) group) and a prediction with its
//! resolved outcome row. Accepted rules are a note and a revision citing the examples they rest on.
//! Caps of the controller (`Limits`) and refusals work as in B5; the kill switch and the energy cap
//! are checked before every step (`Policy::gate_cost`), and a stop is logged.
//!
//! Only what the being is TOLD goes into the log (the reported result and revealed answer, which
//! feedback noise can make wrong). The checker's true verdict is kept by the engine for the
//! measurement only (`TrueRec`) and is never written.

use super::learner::{Act2, Choice2, Chooser2, Learner2, Offer2};
use super::model::FamModel;
use super::solve::{self, sig2, sig2_split};
use super::world::{FType, Task2, W2Cfg, World2, SKIP_COST, SOURCE2, TYPE_CYCLE};
use crate::being::controller::{Limits, Refusal};
use crate::being::discover::{self, rule_from_event, Example, Found, Rule};
use crate::being::policy::{Policy, StopReason};
use crate::being::solver::Strategy;
use crate::being::state::{tag, EVIDENCE_WINDOW};
use crate::being::world::{iso_at, SECS_PER_ENERGY};
use crate::versioned::{event_id, prediction_id, Batch, EventRow, LogError, NewEvent, NewOutcome, NewPrediction, NewRevision, VersionedLog};
use serde_json::json;
use std::collections::{HashMap, HashSet, VecDeque};

pub const FLUSH_EVERY: usize = 25;
pub const DEFAULT_DISCOVER_EVERY: u64 = 10;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ArmStat {
    pub n: u64,
    pub reward: i64,
    pub cost: i64,
}

/// The learner's constants, fixed at creation and logged in the genesis event (for exact replay).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LearnerCfg {
    pub beta_milli: u32,
    pub skip_milli: u32,
}

impl Default for LearnerCfg {
    fn default() -> Self {
        let l = Learner2::default();
        LearnerCfg { beta_milli: (l.beta * 1000.0).round() as u32, skip_milli: (l.skip_frac * 1000.0).round() as u32 }
    }
}

impl LearnerCfg {
    pub fn learner(&self) -> Learner2 {
        Learner2 { beta: self.beta_milli as f64 / 1000.0, skip_frac: self.skip_milli as f64 / 1000.0 }
    }
}

/// One outcome as the being was told it (what an outcome event carries).
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome2 {
    pub step: u64,
    pub fam: Option<usize>,
    pub difficulty: u8,
    pub strategy: Option<Strategy>,
    /// correct / wrong (reported), skip, unpaid, oracle-correct / oracle-wrong (oracle runs only).
    pub result: String,
    pub reward: i64,
    pub cost: i64,
    pub listed: i64,
    pub budget_left: i64,
    pub clock: i64,
    pub event_id: String,
    pub example: Option<Example>,
}

impl Outcome2 {
    pub fn attempted(&self) -> bool {
        self.strategy.is_some() && (self.result == "correct" || self.result == "wrong")
    }
}

/// The being's state in World v2, folded from its log.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State2 {
    pub cfg: Option<W2Cfg>,
    pub learner: LearnerCfg,
    pub genesis_event: String,
    pub step: u64,
    pub clock: i64,
    pub budget_left: i64,
    /// Reward the being was told it earned (feedback noise included).
    pub total_reward: i64,
    pub total_cost: i64,
    pub skips: u64,
    pub model: FamModel,
    pub arms: Vec<[ArmStat; 4]>,
    pub examples: Vec<Example>,
    pub rules: Vec<Rule>,
    recent: VecDeque<String>,
    group_recent: HashMap<(usize, usize), VecDeque<String>>,
}

/// `compose` families whose base is `fam` (family types are observable: the type cycle is public).
pub fn compose_children(fam: usize, families: usize) -> Vec<usize> {
    (0..families).filter(|&i| TYPE_CYCLE[i % TYPE_CYCLE.len()] == FType::Compose && i >= 1 && i - 1 == fam).collect()
}

impl State2 {
    pub fn start(&mut self, cfg: W2Cfg, learner: LearnerCfg, event: String) {
        self.budget_left = cfg.budget;
        self.model = FamModel::new(cfg.families);
        self.arms = vec![[ArmStat::default(); 4]; cfg.families];
        self.cfg = Some(cfg);
        self.learner = learner;
        self.genesis_event = event;
    }

    pub fn arm(&self, fam: usize, s: Strategy) -> ArmStat {
        self.arms.get(fam).map(|a| a[s.idx()]).unwrap_or_default()
    }

    pub fn apply(&mut self, o: &Outcome2) {
        self.total_cost += o.cost;
        self.budget_left = o.budget_left;
        self.clock = o.clock;
        self.step = o.step + 1;
        if o.result == "skip" {
            self.skips += 1;
        }
        self.recent.push_back(o.event_id.clone());
        while self.recent.len() > EVIDENCE_WINDOW {
            self.recent.pop_front();
        }
        if !o.attempted() {
            return;
        }
        let (fam, s) = (o.fam.expect("attempted"), o.strategy.expect("attempted"));
        self.total_reward += o.reward;
        if let Some(ex) = &o.example {
            self.examples.push(ex.clone());
        }
        self.model.observe(fam, o.difficulty, s, o.result == "correct", o.listed, o.cost);
        if fam >= self.arms.len() {
            self.arms.resize(fam + 1, [ArmStat::default(); 4]);
        }
        let a = &mut self.arms[fam][s.idx()];
        a.n += 1;
        a.reward += o.reward;
        a.cost += o.cost;
        let q = self.group_recent.entry((fam, s.idx())).or_default();
        q.push_back(o.event_id.clone());
        while q.len() > EVIDENCE_WINDOW {
            q.pop_front();
        }
    }

    /// Take an accepted rule in; the `discovered` cells it changes start over (as `BeingState::apply_rule`),
    /// including the `compose` families that reuse this family's rules.
    pub fn apply_rule(&mut self, r: Rule) -> bool {
        if self.rules.iter().any(|x| x.id == r.id) {
            return false;
        }
        let (fam, d) = sig2_split(r.sig);
        self.model.reset_cell(fam, d, Strategy::Discovered);
        for c in compose_children(fam, self.cfg.as_ref().map_or(0, |c| c.families)) {
            self.model.reset_cell(c, d, Strategy::Discovered);
        }
        self.rules.push(r);
        true
    }

    pub fn live_rules(&self) -> usize {
        self.rules.iter().filter(|r| !r.retired).count()
    }

    pub fn blocks(&self) -> Vec<discover::Form> {
        let mut v: Vec<discover::Form> = vec![];
        for r in self.rules.iter().filter(|r| r.xf.is_id() && !r.retired) {
            if !v.contains(&r.form) {
                v.push(r.form);
            }
        }
        v
    }

    /// The events a decision for (family, strategy) cites: genesis, latest outcomes, latest outcomes of that group.
    pub fn evidence(&self, group: Option<(usize, Strategy)>) -> Vec<String> {
        let mut v = vec![self.genesis_event.clone()];
        v.extend(self.recent.iter().cloned());
        if let Some((f, s)) = group {
            if let Some(q) = self.group_recent.get(&(f, s.idx())) {
                for id in q {
                    if !v.contains(id) {
                        v.push(id.clone());
                    }
                }
            }
        }
        v
    }

    pub fn from_events(rows: &[EventRow]) -> State2 {
        let mut st = State2::default();
        for e in rows.iter().filter(|e| e.source == SOURCE2) {
            st.fold_event(e);
        }
        st
    }

    pub fn fold_event(&mut self, e: &EventRow) {
        if e.source != SOURCE2 {
            return;
        }
        let has = |t: &str| e.tags.split(',').any(|x| x == t);
        if e.kind == "note" && has("w2_genesis") && self.cfg.is_none() {
            if let (Some(c), Some(b), Some(s)) = (W2Cfg::from_tags(&e.tags), tag(&e.tags, "w2_beta_milli").and_then(|x| x.parse().ok()), tag(&e.tags, "w2_skip_milli").and_then(|x| x.parse().ok())) {
                self.start(c, LearnerCfg { beta_milli: b, skip_milli: s }, e.event_id.clone());
            }
        } else if e.kind == "outcome" {
            if let Some(o) = outcome_from_event(e) {
                self.apply(&o);
            }
        } else if e.kind == "note" && has("rule") && has("world2") {
            if let Some(r) = rule_from_event(e) {
                self.apply_rule(r);
            }
        }
    }
}

pub fn outcome_from_event(e: &EventRow) -> Option<Outcome2> {
    let t = |k: &str| tag(&e.tags, k);
    let step: u64 = t("step")?.parse().ok()?;
    let fam: Option<usize> = t("fam").and_then(|x| x.parse().ok());
    let difficulty: u8 = t("difficulty").and_then(|x| x.parse().ok()).unwrap_or(1);
    let strategy = t("strategy").and_then(Strategy::parse);
    let example = match (t("terms"), t("next"), fam) {
        (Some(ts), Some(nx), Some(f)) => {
            let terms: Option<Vec<i64>> = ts.split(';').map(|x| x.parse().ok()).collect();
            Some(Example { event_id: e.event_id.clone(), sig: sig2(f, difficulty), terms: terms?, next: nx.parse().ok()? })
        }
        _ => None,
    };
    Some(Outcome2 {
        step,
        fam,
        difficulty,
        strategy,
        result: t("result")?.to_string(),
        reward: t("reward")?.parse().ok()?,
        cost: t("cost")?.parse().ok()?,
        listed: t("listed")?.parse().ok()?,
        budget_left: t("budget_left")?.parse().ok()?,
        clock: t("clock")?.parse().ok()?,
        event_id: e.event_id.clone(),
        example,
    })
}

pub fn genesis_event2(cfg: &W2Cfg, l: &LearnerCfg) -> (NewEvent, String) {
    let mut tags = vec!["w2_genesis".to_string()];
    tags.extend(cfg.tags());
    tags.push(format!("w2_beta_milli:{}", l.beta_milli));
    tags.push(format!("w2_skip_milli:{}", l.skip_milli));
    let text = format!(
        "world v2 being created: seed {}, {} task families, budget {} energy units, feedback noise {} per mille, drift {} changes per 10000 energy, difficulty curve {}%, menu of {}; learner beta {:.3}, skip fraction {:.3}. Simulated world with author-defined families and rewards",
        cfg.seed, cfg.families, cfg.budget, cfg.noise_pm, cfg.drift_per_10k, cfg.curve_pct, cfg.menu, l.beta_milli as f64 / 1000.0, l.skip_milli as f64 / 1000.0
    );
    let ev = NewEvent { source: SOURCE2.into(), occurred_at: iso_at(0), kind: "note".into(), text, tags, redacted: false };
    let id = event_id(&ev.source, &ev.occurred_at, &ev.text);
    (ev, id)
}

/// The log record of a refusal (same text and tags as `Refusal::event`, source `being:world2`).
pub fn refusal_event2(r: &Refusal, step: u64, clock: i64) -> NewEvent {
    let mut ev = r.event(step, clock);
    ev.source = SOURCE2.into();
    ev
}

/// The log record of a run stopped by the kill switch or the energy cap.
pub fn stop_event2(why: &str, step: u64, clock: i64) -> NewEvent {
    let mut ev = crate::being::controller::stop_event(why, step, clock);
    ev.source = SOURCE2.into();
    ev
}

/// Truth of one step, kept by the engine for the measurement; never written to the log.
#[derive(Debug, Clone, PartialEq)]
pub struct TrueRec {
    pub step: u64,
    pub clock: i64,
    pub fam: Option<usize>,
    pub ftype: Option<FType>,
    pub difficulty: u8,
    pub strategy: Option<Strategy>,
    pub oracle: bool,
    pub attempted: bool,
    pub true_correct: bool,
    pub reported_correct: bool,
    pub flipped: bool,
    pub true_reward: i64,
    pub reported_reward: i64,
    pub cost: i64,
    pub rule_used: bool,
    pub predicted_p: Option<f64>,
}

pub struct Step2Out {
    pub outcome: Outcome2,
    pub truth: TrueRec,
    pub label: String,
    pub events: Vec<NewEvent>,
    pub revision: NewRevision,
    pub prediction: Option<NewPrediction>,
    pub resolved: Option<NewOutcome>,
}

pub struct NewRule2 {
    pub rule: Rule,
    pub event: NewEvent,
    pub revision: NewRevision,
}

pub struct Engine2 {
    pub world: World2,
    pub state: State2,
    pub limits: Option<Limits>,
    pub refusals: Vec<Refusal>,
    pub refusals_total: Vec<Refusal>,
    refused: HashSet<String>,
    last_blocks: usize,
    pub max_candidates_seen: usize,
}

pub fn offers_of2(tasks: &[Task2]) -> Vec<Offer2> {
    tasks.iter().map(|t| Offer2 { slot: t.view.slot, fam: t.view.fam, ftype: t.view.ftype, difficulty: t.view.difficulty, listed_cost: t.view.cost, listed_reward: t.view.reward }).collect()
}

fn fin(x: f64) -> String {
    if x.is_finite() { format!("{x:.3}") } else { "n/a".into() }
}

impl Engine2 {
    pub fn new(state: State2, limits: Option<Limits>) -> Engine2 {
        let world = World2::new(state.cfg.clone().expect("a started state"));
        Engine2 { world, state, limits, refusals: vec![], refusals_total: vec![], refused: HashSet::new(), last_blocks: 0, max_candidates_seen: 0 }
    }

    pub fn refuse(&mut self, r: Refusal) {
        if self.refused.insert(format!("{}|{}", r.cap, r.what)) {
            self.refusals_total.push(r.clone());
            self.refusals.push(r);
        }
    }

    pub fn take_refusals(&mut self) -> Vec<Refusal> {
        std::mem::take(&mut self.refusals)
    }

    pub fn step(&mut self, chooser: &mut dyn Chooser2) -> Result<Step2Out, StopReason> {
        let step = self.state.step;
        let t0 = self.state.clock;
        let tasks = self.world.offers(step, t0);
        let offers = offers_of2(&tasks);
        let ch: Choice2 = chooser.choose(&self.state, &offers).ok_or(StopReason::BudgetExhausted)?;
        let label = ch.act.label();
        let budget0 = self.state.budget_left;
        let rules = self.state.rules.clone();
        // act and check
        let (slot, strategy, oracle) = match ch.act {
            Act2::Attempt { slot, strategy } => (Some(slot), Some(strategy), false),
            Act2::Oracle { slot } => (Some(slot), None, true),
            Act2::Skip => (None, None, false),
        };
        let mut truth = TrueRec { step, clock: t0, fam: None, ftype: None, difficulty: 1, strategy, oracle, attempted: false, true_correct: false, reported_correct: false, flipped: false, true_reward: 0, reported_reward: 0, cost: 0, rule_used: false, predicted_p: None };
        let (cost, result, answer_text, mut otags, example, rule_used): (i64, String, String, Vec<String>, Option<Example>, Option<String>) = match slot {
            None => {
                let c = SKIP_COST.min(budget0);
                (c, "skip".into(), format!("skip the menu at step {step} (energy {c})"), vec![], None, None)
            }
            Some(slot) => {
                let task = &tasks[slot];
                let v = &task.view;
                truth.fam = Some(v.fam);
                truth.ftype = Some(v.ftype);
                truth.difficulty = v.difficulty;
                let energy = match strategy {
                    Some(s) => s.energy(v.cost),
                    None => v.cost,
                };
                let mut tg = vec![format!("fam:{}", v.fam), format!("type:{}", v.ftype.name()), format!("task:{}", v.id), format!("difficulty:{}", v.difficulty), format!("slot:{slot}")];
                if let Some(s) = strategy {
                    tg.push(format!("strategy:{}", s.name()));
                }
                if energy > budget0 {
                    (budget0, "unpaid".into(), format!("attempt task {} with {}: energy ran out before the answer ({budget0} left, {energy} needed)", v.id, label), tg, None, None)
                } else {
                    let prop = match strategy {
                        Some(s) => solve::attempt(&self.world, task, s, &rules),
                        None => solve::attempt_oracle(&self.world, task),
                    };
                    let fb = self.world.feedback(task, prop.answer);
                    let true_correct = task.check(prop.answer);
                    truth.attempted = true;
                    truth.true_correct = true_correct;
                    truth.reported_correct = fb.reported_correct;
                    truth.flipped = fb.flipped;
                    truth.true_reward = if true_correct { v.reward } else { 0 };
                    truth.reported_reward = if fb.reported_correct { v.reward } else { 0 };
                    truth.rule_used = prop.rule_id.is_some();
                    let res = match (oracle, fb.reported_correct) {
                        (false, true) => "correct",
                        (false, false) => "wrong",
                        (true, true) => "oracle-correct",
                        (true, false) => "oracle-wrong",
                    };
                    let mut ex = None;
                    if v.ftype == FType::Seq {
                        tg.push(format!("terms:{}", discover::terms_tag(&v.params)));
                        tg.push(format!("next:{}", fb.revealed));
                        if !oracle {
                            ex = Some(Example { event_id: String::new(), sig: sig2(v.fam, v.difficulty), terms: v.params.clone(), next: fb.revealed });
                        }
                    } else {
                        tg.push(format!("revealed:{}", fb.revealed));
                    }
                    if let Some(r) = &prop.rule_id {
                        tg.push(format!("rule_used:{r}"));
                    }
                    let txt = format!("attempt task {} ({}) using {}: answer {}", v.id, v.prompt, prop.method, prop.answer);
                    (energy, res.into(), txt, tg, ex, prop.rule_id.clone())
                }
            }
        };
        truth.cost = cost;
        let t1 = t0 + cost * SECS_PER_ENERGY;
        let budget_left = budget0 - cost;
        let reported_reward = truth.reported_reward;
        let key = format!("being/world2/choice/{step:06}");
        let group = match (truth.fam, strategy) {
            (Some(f), Some(s)) => Some((f, s)),
            _ => None,
        };
        let evidence = self.state.evidence(group);
        let chosen_txt = match (slot, truth.fam) {
            (Some(sl), Some(f)) => {
                let v = &tasks[sl].view;
                format!("slot {sl} (family f{f:02} {} difficulty {}, listed cost {} reward {}) with {}", v.ftype.name(), v.difficulty, v.cost, v.reward, strategy.map_or("the ground truth (oracle)", |s| s.name()))
            }
            _ => "skip".into(),
        };
        let dtext = format!("step {step}: chose {chosen_txt} by {} ({}); p_success={} sd={} est_energy={} score={}; cites {} events", ch.policy, ch.why, fin(ch.p), fin(ch.sd), fin(ch.cost_est), fin(ch.score), evidence.len());
        let mut dtags = vec![format!("step:{step}"), format!("action:{label}"), format!("policy:{}", ch.policy), format!("rev_key:{key}")];
        if let Some(f) = truth.fam {
            dtags.push(format!("fam:{f}"));
        }
        let dev = NewEvent { source: SOURCE2.into(), occurred_at: iso_at(t0), kind: "decision".into(), text: dtext, tags: dtags, redacted: false };
        let dev_id = event_id(&dev.source, &dev.occurred_at, &dev.text);
        let mut atags = vec![format!("step:{step}"), format!("action:{label}")];
        if let Some(r) = &rule_used {
            atags.push(format!("rule_used:{r}"));
        }
        let aev = NewEvent { source: SOURCE2.into(), occurred_at: iso_at(t0), kind: "action".into(), text: format!("step {step}: {answer_text}"), tags: atags, redacted: false };
        let mut tags = vec![format!("step:{step}")];
        tags.append(&mut otags);
        tags.extend([format!("result:{result}"), format!("reward:{reported_reward}"), format!("cost:{cost}"), format!("listed:{}", slot.map_or(cost, |s| tasks[s].view.cost)), format!("budget_left:{budget_left}"), format!("clock:{t1}")]);
        let what = match slot {
            Some(s) => format!("task {}", tasks[s].view.id),
            None => "menu".into(),
        };
        let oev = NewEvent {
            source: SOURCE2.into(),
            occurred_at: iso_at(t1),
            kind: "outcome".into(),
            text: format!("step {step}: {what} is reported {result}; reward {reported_reward} cost {cost} budget left {budget_left}"),
            tags,
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
        let attempted = truth.attempted;
        let (prediction, resolved) = if attempted {
            let pp = if ch.p > 0.0 { ch.p } else { 0.5 }.clamp(0.001, 0.999);
            truth.predicted_p = Some(pp);
            let pr = NewPrediction {
                made_at: iso_at(t0),
                question_id: format!("being:world2:step:{step}"),
                target: format!("task:{}", slot.map_or("none".to_string(), |s| tasks[s].view.id.clone())),
                options: vec!["correct".into(), "wrong".into()],
                probs: vec![pp, 1.0 - pp],
                model_id: format!("being:world2:{}", ch.policy),
                model_version: "1".into(),
                features_cite: vec![dev_id.clone()],
                resolves_by: None,
            };
            let pred_id = prediction_id(&pr).map_err(|_| StopReason::StopFlag)?;
            let actual = if truth.reported_correct { "correct" } else { "wrong" };
            (Some(pr), Some(NewOutcome { pred_id, resolved_at: iso_at(t1), actual: actual.into(), outcome_event_id: Some(oev_id.clone()) }))
        } else {
            (None, None)
        };
        let outcome = Outcome2 {
            step,
            fam: truth.fam,
            difficulty: truth.difficulty,
            strategy: if result == "correct" || result == "wrong" || result == "unpaid" { strategy } else { None },
            result,
            reward: reported_reward,
            cost,
            listed: slot.map_or(cost, |s| tasks[s].view.cost),
            budget_left,
            clock: t1,
            event_id: oev_id.clone(),
            example: example.map(|mut e| {
                e.event_id = oev_id.clone();
                e
            }),
        };
        self.state.apply(&outcome);
        Ok(Step2Out { outcome, truth, label, events: vec![dev, aev, oev], revision, prediction, resolved })
    }

    /// One discovery pass over the being's own revealed examples (as `Engine::discover_pass`, with the caps).
    pub fn discover_pass(&mut self) -> Vec<NewRule2> {
        let found: Vec<Found> = match self.limits.clone() {
            None => discover::search(&self.state.examples),
            Some(l) => {
                let all = self.state.blocks();
                let blocks: Vec<discover::Form> = if l.compose { all.iter().take(l.max_blocks).cloned().collect() } else { vec![] };
                if l.compose && all.len() > l.max_blocks {
                    self.refuse(Refusal::new("max_building_blocks", l.max_blocks as u64, all.len() as u64, format!("{} verified building blocks exist; only the first {} are used for composition", all.len(), l.max_blocks)));
                }
                let o = discover::search_composed(&self.state.examples, &blocks, l.max_candidates);
                if o.truncated {
                    self.refuse(Refusal::new("max_search_candidates", l.max_candidates as u64, o.wanted as u64, format!("a discovery pass would try {} candidates; it stopped after {}", o.wanted, o.candidates)));
                }
                self.last_blocks = blocks.len();
                self.max_candidates_seen = self.max_candidates_seen.max(o.candidates);
                o.found
            }
        };
        let mut out = vec![];
        for f in found {
            if let Some(nr) = self.accept_found(&f) {
                out.push(nr);
            }
        }
        out
    }

    fn accept_found(&mut self, f: &Found) -> Option<NewRule2> {
        let nblocks = if f.xf.is_id() { 0 } else { self.last_blocks as u32 };
        let mut rule = Rule::new_x(f.sig, f.xf, f.form, f.train_ok, f.val_ok, self.state.step, nblocks);
        if self.state.rules.iter().any(|r| r.id == rule.id) {
            return None;
        }
        let (fam, d) = sig2_split(f.sig);
        if let Some(l) = &self.limits {
            let live = self.state.live_rules();
            if live >= l.max_live_rules {
                let cap = l.max_live_rules as u64;
                self.refuse(Refusal::new("max_live_rules", cap, live as u64 + 1, format!("rule {} ({} {}) for family f{fam:02} difficulty {d} is verified but was not added: {live} rules are live", rule.id, f.xf.code(), f.form.code())));
                return None;
            }
        }
        let at = iso_at(self.state.clock);
        let text = format!(
            "world v2 rule {} for family f{fam:02} difficulty {d}: {}{}; coefficients fitted per task from its own prefix; reproduced the revealed next term exactly on {} training and {} held-out logged examples and was never applicable-and-wrong on them (search inside the author-defined grammar of discover.rs)",
            rule.id, if f.xf.is_id() { String::new() } else { format!("take {} of the prefix, then ", f.xf.words()) }, f.form.dsl(), f.train_ok, f.val_ok
        );
        let mut tags = vec!["rule".to_string(), "world2".into(), format!("rule_id:{}", rule.id), format!("sig:{}", rule.sig), format!("form:{}", rule.form.code()), format!("train:{}", rule.train_ok), format!("val:{}", rule.val_ok), format!("at_step:{}", rule.step), format!("fam:{fam}"), format!("difficulty:{d}")];
        if !f.xf.is_id() {
            tags.push(format!("xf:{}", f.xf.code()));
            tags.push(format!("blocks:{nblocks}"));
        }
        let event = NewEvent { source: SOURCE2.into(), occurred_at: at.clone(), kind: "note".into(), text, tags, redacted: false };
        let ev_id = event_id(&event.source, &event.occurred_at, &event.text);
        rule.event_id = ev_id.clone();
        let mut caused_by = f.cite.clone();
        caused_by.push(ev_id);
        let rationale = json!({
            "rule_id": rule.id, "family": fam, "difficulty": d, "form": rule.form.code(), "dsl": rule.form.dsl(), "transform": f.xf.code(),
            "training_examples_reproduced": f.train_ok, "held_out_examples_reproduced": f.val_ok, "min_training_support": discover::K_TRAIN, "min_held_out_support": discover::K_VAL,
            "cited_examples": f.cite.len(), "accepted_at_step": self.state.step,
            "note": "examples are the answers the being was TOLD; with feedback noise some are wrong",
        });
        let revision = NewRevision {
            entity_type: "rule".into(),
            key: format!("being/world2/rule/{}", rule.id),
            stance: format!("family f{fam:02} difficulty {d}: {}", rule.form.dsl()),
            confidence: ((f.train_ok + f.val_ok + 1) as f64 / (f.train_ok + f.val_ok + 2) as f64).clamp(0.0, 1.0),
            rationale: rationale.to_string(),
            supersedes: None,
            valid_from: at,
            author: "rule:being-discover".into(),
            caused_by,
        };
        self.state.apply_rule(rule.clone());
        Some(NewRule2 { rule, event, revision })
    }
}

// ---- the logged run ----

#[derive(Debug, Clone, Default)]
pub struct Run2Cfg {
    /// Creation only: the world. Ignored (must match) when the being exists.
    pub world: Option<W2Cfg>,
    /// Creation only: the learner's constants (default: the B1 defaults).
    pub learner: Option<LearnerCfg>,
    /// Discovery pass after every N-th step (0: never).
    pub discover_every: u64,
    /// Caps and switches of the controller (None: no controller).
    pub ctl: Option<Limits>,
}

#[derive(Debug, Clone)]
pub struct Run2Summary {
    pub steps_run: u64,
    pub stopped: StopReason,
    pub reported_reward: i64,
    pub true_reward: i64,
    pub cost: i64,
    pub state: State2,
    pub created: bool,
    pub new_rules: Vec<Rule>,
    pub refusals: Vec<Refusal>,
    pub truths: Vec<TrueRec>,
    pub version_before: u64,
    pub version_after: u64,
}

impl Run2Summary {
    pub fn to_json(&self) -> serde_json::Value {
        let c = self.state.cfg.clone();
        json!({
            "steps_run": self.steps_run, "stopped": self.stopped.as_str(), "reported_reward_this_run": self.reported_reward, "energy_spent_this_run": self.cost,
            "reported_reward_per_energy_this_run": if self.cost > 0 { self.reported_reward as f64 / self.cost as f64 } else { 0.0 },
            "total_steps": self.state.step, "budget_left": self.state.budget_left, "skips": self.state.skips,
            "world": c.as_ref().map(|c| json!({ "seed": c.seed, "families": c.families, "budget": c.budget, "noise_pm": c.noise_pm, "drift_per_10k": c.drift_per_10k, "curve_pct": c.curve_pct, "menu": c.menu })),
            "rules_total": self.state.rules.len(), "rules_accepted_this_run": self.new_rules.iter().map(|r| json!({ "rule_id": r.id, "family": sig2_split(r.sig).0, "difficulty": sig2_split(r.sig).1, "form": r.form.code(), "transform": r.xf.code() })).collect::<Vec<_>>(),
            "controller_refusals_this_run": self.refusals.iter().map(|r| json!({ "cap": r.cap, "limit": r.limit, "requested": r.requested, "what": r.what })).collect::<Vec<_>>(),
            "new_being": self.created, "version_before": self.version_before, "version_after": self.version_after,
            "note": "World v2: a deterministic simulated world with author-defined task families; the reward shown is the reward the being was told (feedback noise can make it wrong). See docs/world_v2.md",
        })
    }
}

/// Run the World v2 being on its own log with its own learner (or `chooser`, for baselines and tests).
pub fn run_v2(log: &mut VersionedLog, cfg: &Run2Cfg, policy: &Policy, chooser: Option<&mut dyn Chooser2>) -> Result<Run2Summary, LogError> {
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
    let mut own = state.learner.learner();
    let chooser: &mut dyn Chooser2 = match chooser {
        Some(c) => c,
        None => &mut own,
    };
    let mut engine = Engine2::new(state, cfg.ctl.clone());
    let mut new_rules = vec![];
    let mut truths = vec![];
    let mut pending = Batch::default();
    let mut pending_steps = 0usize;
    let (mut done, mut rep, mut tru, mut cost) = (0u64, 0i64, 0i64, 0i64);
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
        match engine.step(chooser) {
            Ok(out) => {
                done += 1;
                rep += out.outcome.reward;
                tru += out.truth.true_reward;
                cost += out.outcome.cost;
                truths.push(out.truth);
                pending.events.extend(out.events);
                pending.revisions.push(out.revision);
                pending.predictions.extend(out.prediction);
                pending.outcomes.extend(out.resolved);
                pending_steps += 1;
                if cfg.discover_every > 0 && engine.state.step % cfg.discover_every == 0 {
                    for nr in engine.discover_pass() {
                        pending.events.push(nr.event);
                        pending.revisions.push(nr.revision);
                        new_rules.push(nr.rule);
                    }
                }
                for r in engine.take_refusals() {
                    pending.events.push(refusal_event2(&r, engine.state.step, engine.state.clock));
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
    for r in engine.take_refusals() {
        pending.events.push(refusal_event2(&r, engine.state.step, engine.state.clock));
    }
    if !pending.events.is_empty() {
        log.commit_batch(pending)?;
    }
    Ok(Run2Summary { steps_run: done, stopped, reported_reward: rep, true_reward: tru, cost, state: engine.state, created, new_rules, refusals: engine.refusals_total, truths, version_before, version_after: log.latest_version() })
}
