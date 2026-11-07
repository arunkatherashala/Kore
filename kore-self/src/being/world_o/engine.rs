//! The World O loop: observe -> decide -> act -> check -> record, on the being's own log (source
//! `being:worldo`).
//!
//! The same shape as the World v2 loop (`world2/engine.rs`) and the same learner: the being's
//! learning state IS a World v2 `State2` (the B1 world model keyed by family slot, the B2/B5 rule book
//! and its examples), folded from the log; the chooser is `Learner2` (the B1 planner) wrapped in
//! `LearnerO`, which only adds the Track B11 skip test as a switch. What is new is the bookkeeping of
//! an open world, all of it driven by public announcements that are logged before the step that
//! follows them:
//!
//! * a family opens: its slot is recorded (and, for `compose`, its base slot);
//! * a family closes: the evidence of its slot is moved into the model's archive cell (so the
//!   strategy-wide prior keeps everything the being has seen), its examples, rules and rule records
//!   are dropped from the working set (the log keeps them), and the slot is free for a later family.
//!
//! Caps (`Limits`), refusals, the kill switch and the energy cap (`Policy::gate_cost`) work as in
//! World v2. The checker's true verdict is kept for the measurement only (`TrueO`), never logged.

use super::b12::{self, B12State, B12View, L2LCfg};
use super::world::{FeedbackO, Open, TaskO, WOCfg, WorldO, ARCHIVE, SKIP_COST, SOURCE_O};
use crate::being::controller::{Limits, Refusal};
use crate::being::discover::{self, rule_from_event, Example, Found, Rule};
use crate::being::planner::{SkipRule, SKIP_MIN_COST};
use crate::being::policy::{Policy, StopReason};
use crate::being::solver::{Strategy, STRATEGIES_EXT};
use crate::being::state::tag;
use crate::being::world::{iso_at, SECS_PER_ENERGY};
use crate::being::world2::engine::{outcome_from_event, LearnerCfg, Outcome2, State2};
use crate::being::world2::learner::{r3, Act2, Choice2, Chooser2, Learner2, Offer2};
use crate::being::world2::rules_use::UseCfg;
use crate::being::world2::solve::{self, sig2, sig2_split};
use crate::being::world2::world::{FType, W2Cfg};
use crate::versioned::{event_id, prediction_id, Batch, EventRow, LogError, NewEvent, NewOutcome, NewPrediction, NewRevision, VersionedLog};
use serde_json::json;
use std::collections::{BTreeMap, HashSet};

pub const FLUSH_EVERY: usize = 25;
pub const LEARNER_O: &str = "wo-model-ucb";

/// The learner's constants, fixed at creation and logged in the genesis event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LearnerOCfg {
    pub beta_milli: u32,
    pub skip_milli: u32,
    /// Track B11 switch (default: the B1 skip test).
    pub skip_rule: SkipRule,
    /// Track B7 switches A and A2 (rule-record estimate, `discovered` only where a rule covers the offer).
    pub use_: UseCfg,
    /// Track B12 switches (learning to learn; all off by default, see `b12.rs`).
    pub l2l: L2LCfg,
}

impl Default for LearnerOCfg {
    fn default() -> Self {
        let l = LearnerCfg::default();
        LearnerOCfg { beta_milli: l.beta_milli, skip_milli: l.skip_milli, skip_rule: SkipRule::Rate, use_: UseCfg::default(), l2l: L2LCfg::default() }
    }
}

impl LearnerOCfg {
    pub fn learner(&self) -> LearnerO {
        LearnerO { inner: Learner2 { beta: self.beta_milli as f64 / 1000.0, skip_frac: self.skip_milli as f64 / 1000.0, use_: self.use_ }, skip_rule: self.skip_rule, l2l: self.l2l, view: None }
    }
    fn w2(&self) -> LearnerCfg {
        LearnerCfg { beta_milli: self.beta_milli, skip_milli: self.skip_milli, use_: self.use_, skills: Default::default() }
    }
}

/// `Learner2` (the B1 planner over families) with the Track B11 skip test as a switch. With
/// `SkipRule::Rate` it returns exactly what `Learner2` returns (a test checks it).
#[derive(Debug, Clone)]
pub struct LearnerO {
    pub inner: Learner2,
    pub skip_rule: SkipRule,
    /// Track B12 switches; with all off `choose` is exactly the B11 learner.
    pub l2l: L2LCfg,
    /// Track B12: what the learner reads of the World O state (set by `prepare_o` before each choice).
    pub view: Option<B12View>,
}

impl Chooser2 for LearnerO {
    fn name(&self) -> String {
        LEARNER_O.into()
    }
    fn prepare_o(&mut self, st: &StateO) {
        if self.l2l.chooser_on() {
            b12::prepare(self, st);
        }
    }
    fn choose(&mut self, st: &State2, offers: &[Offer2]) -> Option<Choice2> {
        if self.l2l.chooser_on() {
            return b12::choose(self, st, offers);
        }
        let ch = self.inner.choose(st, offers)?;
        if self.skip_rule == SkipRule::Rate {
            return Some(ch);
        }
        let cands = self.inner.candidates(st, offers);
        let mut bi: Option<usize> = None;
        for (i, c) in cands.iter().enumerate() {
            if c.affordable && bi.map_or(true, |b| c.score > cands[b].score) {
                bi = Some(i);
            }
        }
        let best = &cands[bi?];
        let rate = if st.total_cost > 0 { st.total_reward as f64 / st.total_cost as f64 } else { 0.0 };
        let sf = self.inner.skip_frac;
        let bar = self.skip_rule.bar(sf, rate, best.cost_est, SKIP_COST, best.n);
        let skip = sf > 0.0 && st.total_cost >= SKIP_MIN_COST && best.score < bar && st.budget_left >= SKIP_COST;
        let mut rationale = ch.rationale;
        rationale["skip_rule"] = json!(self.skip_rule.code());
        rationale["skip_bar"] = json!(r3(bar));
        if skip {
            return Some(Choice2 {
                act: Act2::Skip,
                policy: ch.policy,
                why: format!("best optimistic score {:.3} is below the {} skip bar {:.3} (reported rate {:.3}, its energy {:.3}, {} attempts in its cell): skip the menu", best.score, self.skip_rule.code(), bar, rate, best.cost_est, best.n),
                p: 0.0,
                sd: 0.0,
                cost_est: SKIP_COST as f64,
                score: 0.0,
                rationale,
            });
        }
        Some(Choice2 {
            act: Act2::Attempt { slot: best.slot, strategy: best.strategy },
            policy: ch.policy,
            why: "highest optimistic reward per estimated energy (p + beta*sd) among affordable options".into(),
            p: best.p,
            sd: best.sd,
            cost_est: best.cost_est,
            score: best.score,
            rationale,
        })
    }
}

/// The being's state in World O, folded from its log.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StateO {
    pub cfg: Option<WOCfg>,
    pub learner: LearnerOCfg,
    /// The learning state: the World v2 state (model keyed by slot, rule book, examples, totals).
    pub s2: State2,
    /// Open families as announced: slot -> family id.
    pub known: BTreeMap<usize, u64>,
    /// `compose` slots -> the slot whose rules they reuse.
    pub bases: BTreeMap<usize, usize>,
    pub opened: u64,
    pub closed: u64,
    /// Track B12 state (empty with the switches off).
    pub b12: B12State,
}

impl StateO {
    pub fn start(&mut self, cfg: WOCfg, learner: LearnerOCfg, event: String) {
        // families: 0 (the model grows by slot; no World v2 compose relation applies)
        let w2 = W2Cfg { seed: cfg.seed, families: 0, budget: cfg.budget, noise_pm: cfg.noise_pm, drift_per_10k: 0, curve_pct: 0, menu: cfg.menu };
        self.s2.start(w2, learner.w2(), event);
        self.cfg = Some(cfg);
        self.learner = learner;
        self.b12.cfg = learner.l2l;
    }

    pub fn open_family(&mut self, o: &Open) {
        self.known.insert(o.slot, o.fid);
        if let Some(b) = o.base_slot {
            self.bases.insert(o.slot, b);
        }
        self.opened += 1;
        if self.b12.cfg.prior_k > 0 {
            b12::on_open(self, o);
        }
    }

    /// A family closed: archive its slot's evidence, drop its working examples and rules.
    pub fn close_slot(&mut self, slot: usize) {
        if self.b12.cfg.prior_k > 0 {
            b12::on_close(self, slot);
        }
        for d in 1..=5u8 {
            for s in STRATEGIES_EXT {
                let c = self.s2.model.cell(slot, d, s);
                // the archive keeps the exact counts (re-observed one by one: the model only adds)
                for i in 0..c.n {
                    let (listed, spent) = if i == 0 { (c.listed, c.spent) } else { (0, 0) };
                    self.s2.model.observe(ARCHIVE, d, s, i < c.succ, listed, spent);
                }
                self.s2.model.reset_cell(slot, d, s);
            }
        }
        if let Some(a) = self.s2.arms.get_mut(slot) {
            *a = Default::default();
        }
        let in_slot = |sig: u8| sig2_split(sig).0 == slot;
        self.s2.examples.retain(|e| !in_slot(e.sig));
        let gone: Vec<String> = self.s2.rules.iter().filter(|r| in_slot(r.sig)).map(|r| r.id.clone()).collect();
        self.s2.rules.retain(|r| !in_slot(r.sig));
        self.s2.rule_post.retain(|k, _| k.0 != slot);
        for id in gone {
            self.s2.rule_missed.remove(&id);
        }
        self.s2.cexamples.retain(|c| c.fam != slot);
        self.s2.const_rules.retain(|c| c.fam != slot);
        self.known.remove(&slot);
        self.bases.remove(&slot);
        self.closed += 1;
    }

    /// Take an accepted rule in: the `discovered` cells of its slot and difficulty start over, and those
    /// of the open `compose` families that reuse that slot's rules.
    pub fn apply_rule(&mut self, r: Rule) -> bool {
        let (fam, d) = sig2_split(r.sig);
        let rid = r.clone();
        if !self.s2.apply_rule(r) {
            return false;
        }
        if self.b12.cfg.form_prior > 0 {
            b12::on_rule(self, &rid);
        }
        let kids: Vec<usize> = self.bases.iter().filter(|(_, b)| **b == fam).map(|(k, _)| *k).collect();
        for k in kids {
            self.s2.model.reset_cell(k, d, Strategy::Discovered);
        }
        true
    }

    /// Track B12 counters (exploration energy, exploration attempts, rules accepted with reduced support).
    pub fn b12_counters(&self) -> (i64, u64, u64) {
        (self.b12.explore_energy, self.b12.explore_steps, self.b12.reduced_rules)
    }

    pub fn from_events(rows: &[EventRow]) -> StateO {
        let mut st = StateO::default();
        for e in rows {
            st.fold_event(e);
        }
        st
    }

    pub fn fold_event(&mut self, e: &EventRow) {
        if e.source != SOURCE_O {
            return;
        }
        let has = |t: &str| e.tags.split(',').any(|x| x == t);
        if e.kind == "note" && has("wo_genesis") && self.cfg.is_none() {
            if let (Some(c), Some(l)) = (WOCfg::from_tags(&e.tags), learner_from_tags(&e.tags)) {
                self.start(c, l, e.event_id.clone());
            }
        } else if e.kind == "note" && has("wo_family") {
            let slot: Option<usize> = tag(&e.tags, "slot").and_then(|x| x.parse().ok());
            let fid: Option<u64> = tag(&e.tags, "fid").and_then(|x| x.parse().ok());
            let ftype = tag(&e.tags, "type").and_then(FType::parse);
            match (has("open"), has("close"), slot, fid, ftype) {
                (true, _, Some(slot), Some(fid), Some(ftype)) => {
                    let base_slot = tag(&e.tags, "base_slot").and_then(|x| x.parse().ok());
                    self.open_family(&Open { fid, slot, ftype, base_slot });
                }
                (_, true, Some(slot), _, _) => self.close_slot(slot),
                _ => {}
            }
        } else if e.kind == "outcome" {
            if let Some(o) = outcome_from_event(e) {
                self.s2.apply(&o);
                if has("b12_explore") {
                    self.b12.note_explore(o.cost);
                }
            }
        } else if e.kind == "note" && has("rule") && has("worldo") {
            if let Some(r) = rule_from_event(e) {
                if self.apply_rule(r) && has("b12_reduced") {
                    self.b12.reduced_rules += 1;
                }
            }
        } else if e.kind == "note" && has("controller_refusal") && tag(&e.tags, "cap") == Some(b12::CAP_EXPLORE) {
            self.b12.explore_cap_pm = tag(&e.tags, "limit").and_then(|x| x.parse().ok());
        }
    }
}

fn learner_from_tags(tags: &str) -> Option<LearnerOCfg> {
    let beta_milli = tag(tags, "wo_beta_milli")?.parse().ok()?;
    let skip_milli = tag(tags, "wo_skip_milli")?.parse().ok()?;
    let skip_rule = match tag(tags, "wo_skip_rule") {
        Some(x) => SkipRule::parse(x)?,
        None => SkipRule::Rate,
    };
    let use_ = tag(tags, "w2_use").and_then(UseCfg::parse).unwrap_or_default();
    let l2l = match tag(tags, "wo_l2l") {
        Some(x) => L2LCfg::parse(x)?,
        None => L2LCfg::default(),
    };
    Some(LearnerOCfg { beta_milli, skip_milli, skip_rule, use_, l2l })
}

pub fn genesis_event_o(cfg: &WOCfg, l: &LearnerOCfg) -> (NewEvent, String) {
    let mut tags = vec!["wo_genesis".to_string()];
    tags.extend(cfg.tags());
    tags.push(format!("wo_beta_milli:{}", l.beta_milli));
    tags.push(format!("wo_skip_milli:{}", l.skip_milli));
    if l.skip_rule != SkipRule::Rate {
        tags.push(format!("wo_skip_rule:{}", l.skip_rule.code()));
    }
    if l.use_.any() {
        tags.push(l.use_.tag());
    }
    if l.l2l.any() {
        tags.push(l.l2l.tag());
    }
    let text = format!(
        "world O being created: seed {}, budget {} energy units, menu of {}, {} families open at the start, a new family every {} energy units, each living {} x that, {} families per tier, {} per mille of families change their hidden rule once, feedback noise {} per mille; learner beta {:.3}, skip fraction {:.3}, skip test {}{}. Simulated open-ended world with author-defined families and rewards",
        cfg.seed, cfg.budget, cfg.menu, cfg.start, cfg.intro_every, cfg.life_intros, cfg.tier_every, cfg.drift_pm, cfg.noise_pm, l.beta_milli as f64 / 1000.0, l.skip_milli as f64 / 1000.0, l.skip_rule.code(),
        if l.l2l.any() { l.l2l.words() } else { String::new() }
    );
    let ev = NewEvent { source: SOURCE_O.into(), occurred_at: iso_at(0), kind: "note".into(), text, tags, redacted: false };
    let id = event_id(&ev.source, &ev.occurred_at, &ev.text);
    (ev, id)
}

fn family_event(o: &Open, open: bool, step: u64, clock: i64, name: &str) -> NewEvent {
    let mut tags = vec!["wo_family".to_string(), if open { "open" } else { "close" }.into(), format!("fid:{}", o.fid), format!("slot:{}", o.slot), format!("type:{}", o.ftype.name()), format!("at_step:{step}")];
    if let Some(b) = o.base_slot {
        tags.push(format!("base_slot:{b}"));
    }
    let text = if open {
        format!("world O announces family {name} (slot {}, type {}{}) open at step {step}", o.slot, o.ftype.name(), o.base_slot.map_or(String::new(), |b| format!(", reusing the rule of slot {b}")))
    } else {
        format!("world O announces family {name} (slot {}) closed at step {step}; its evidence moves to the archive, its working examples and rules are set aside (the log keeps them)", o.slot)
    };
    NewEvent { source: SOURCE_O.into(), occurred_at: iso_at(clock), kind: "note".into(), text, tags, redacted: false }
}

pub fn refusal_event_o(r: &Refusal, step: u64, clock: i64) -> NewEvent {
    let mut ev = r.event(step, clock);
    ev.source = SOURCE_O.into();
    ev
}

pub fn stop_event_o(why: &str, step: u64, clock: i64) -> NewEvent {
    let mut ev = crate::being::controller::stop_event(why, step, clock);
    ev.source = SOURCE_O.into();
    ev
}

/// Truth of one step, for the measurement only; never written to the log.
#[derive(Debug, Clone, PartialEq)]
pub struct TrueO {
    pub step: u64,
    pub clock: i64,
    pub fid: Option<u64>,
    pub tier: u32,
    pub ftype: Option<FType>,
    pub difficulty: u8,
    pub strategy: Option<Strategy>,
    pub oracle: bool,
    pub attempted: bool,
    pub true_correct: bool,
    pub reported_correct: bool,
    pub true_reward: i64,
    pub cost: i64,
    pub rule_used: bool,
    pub predicted_p: Option<f64>,
}

pub struct StepO {
    pub outcome: Outcome2,
    pub truth: TrueO,
    pub label: String,
    /// The task attempted (measurement only).
    pub task: Option<TaskO>,
    pub events: Vec<NewEvent>,
    pub revision: NewRevision,
    pub prediction: Option<NewPrediction>,
    pub resolved: Option<NewOutcome>,
}

pub struct NewRuleO {
    pub rule: Rule,
    pub event: NewEvent,
    pub revision: NewRevision,
}

pub fn offers_of_o(tasks: &[TaskO]) -> Vec<Offer2> {
    tasks.iter().map(|t| Offer2 { slot: t.view.slot, fam: t.view.fam, ftype: t.view.ftype, difficulty: t.view.difficulty, listed_cost: t.view.cost, listed_reward: t.view.reward }).collect()
}

fn fin(x: f64) -> String {
    if x.is_finite() { format!("{x:.3}") } else { "n/a".into() }
}

pub struct EngineO {
    pub world: WorldO,
    pub state: StateO,
    pub limits: Option<Limits>,
    pub refusals: Vec<Refusal>,
    pub refusals_total: Vec<Refusal>,
    refused: HashSet<String>,
    last_blocks: usize,
    pub max_candidates_seen: usize,
    pub scan: crate::being::discover_scan::ScanCache,
    /// Family notes of the next step not yet handed out (when the step then found nothing to do).
    carry: Vec<NewEvent>,
    /// Track B12 M4: per slot, the (examples, live rules, form-prior version) of its last family pass (no work twice).
    pub fam_scan: BTreeMap<usize, (usize, usize, u64)>,
}

impl EngineO {
    pub fn new(state: StateO, limits: Option<Limits>) -> EngineO {
        let world = WorldO::new(state.cfg.clone().expect("a started state"));
        EngineO { world, state, limits, refusals: vec![], refusals_total: vec![], refused: HashSet::new(), last_blocks: 0, max_candidates_seen: 0, scan: Default::default(), carry: vec![], fam_scan: BTreeMap::new() }
    }

    pub fn refuse(&mut self, r: Refusal) {
        if self.refused.insert(format!("{}|{}", r.cap, r.what)) {
            self.refusals_total.push(r.clone());
            self.refusals.push(r);
        }
    }

    /// Track B12: the controller's cap on the exploration budget (per mille of the energy). A learner asking for
    /// more gets the cap, and the refusal (returned, to be logged before the first step) records it.
    pub fn cap_explore(&mut self, cap_pm: Option<u32>) -> Option<Refusal> {
        let cap = cap_pm?;
        let want = self.state.b12.cfg.explore_pm;
        if self.state.b12.cfg.explore_n == 0 || want <= cap || self.state.b12.explore_cap_pm == Some(cap) {
            return None;
        }
        self.state.b12.explore_cap_pm = Some(cap);
        let r = Refusal::new(b12::CAP_EXPLORE, cap as u64, want as u64, format!("the learner asks for an exploration budget of {want} per mille of its energy; the controller allows {cap}"));
        self.refused.insert(format!("{}|{}", r.cap, r.what));
        self.refusals_total.push(r.clone());
        Some(r)
    }

    pub fn take_refusals(&mut self) -> Vec<Refusal> {
        std::mem::take(&mut self.refusals)
    }

    /// Bring the known families up to the world's announcement at the current clock (closes first).
    fn sync_families(&mut self) -> Vec<NewEvent> {
        let (step, clock) = (self.state.s2.step, self.state.s2.clock);
        let open: Vec<u64> = self.world.open(clock);
        let mut evs = vec![];
        let closing: Vec<(usize, u64)> = self.state.known.iter().filter(|(_, j)| !open.contains(j)).map(|(s, j)| (*s, *j)).collect();
        for (slot, j) in closing {
            let a = self.world.announce(j);
            evs.push(family_event(&a, false, step, clock, &self.world.fams[j as usize].name()));
            self.state.close_slot(slot);
            self.scan = Default::default();
            self.fam_scan.clear();
        }
        for j in open {
            if self.state.known.get(&self.world.fams[j as usize].slot) != Some(&j) {
                let a = self.world.announce(j);
                evs.push(family_event(&a, true, step, clock, &self.world.fams[j as usize].name()));
                self.state.open_family(&a);
            }
        }
        evs
    }

    pub fn step(&mut self, chooser: &mut dyn Chooser2) -> Result<StepO, StopReason> {
        let mut fam_events = std::mem::take(&mut self.carry);
        fam_events.extend(self.sync_families());
        let step = self.state.s2.step;
        let t0 = self.state.s2.clock;
        let tasks = self.world.offers(step, t0);
        let offers = offers_of_o(&tasks);
        if !offers.is_empty() {
            chooser.prepare_o(&self.state);
        }
        let Some(ch) = (if offers.is_empty() { None } else { chooser.choose(&self.state.s2, &offers) }) else {
            self.carry = fam_events;
            return Err(StopReason::BudgetExhausted);
        };
        let label = ch.act.label();
        let budget0 = self.state.s2.budget_left;
        let rules = self.state.s2.rules.clone();
        let (slot, strategy, oracle) = match ch.act {
            Act2::Attempt { slot, strategy } => (Some(slot), Some(strategy), false),
            Act2::Oracle { slot } => (Some(slot), None, true),
            Act2::Skip => (None, None, false),
        };
        let mut truth = TrueO { step, clock: t0, fid: None, tier: 0, ftype: None, difficulty: 1, strategy, oracle, attempted: false, true_correct: false, reported_correct: false, true_reward: 0, cost: 0, rule_used: false, predicted_p: None };
        let mut attempted_task = None;
        let (cost, result, answer_text, mut otags, example, rule_used): (i64, String, String, Vec<String>, Option<Example>, Option<String>) = match slot {
            None => {
                let c = SKIP_COST.min(budget0);
                (c, "skip".into(), format!("skip the menu at step {step} (energy {c})"), vec![], None, None)
            }
            Some(m) => {
                let task = &tasks[m];
                let v = &task.view;
                let fam = &self.world.fams[task.fid as usize];
                truth.fid = Some(task.fid);
                truth.tier = fam.tier;
                truth.ftype = Some(v.ftype);
                truth.difficulty = v.difficulty;
                let energy = strategy.map_or(v.cost, |s| s.energy(v.cost));
                let mut tg = vec![format!("fam:{}", v.fam), format!("fid:{}", task.fid), format!("type:{}", v.ftype.name()), format!("task:{}", v.id), format!("difficulty:{}", v.difficulty), format!("slot:{m}")];
                if let Some(s) = strategy {
                    tg.push(format!("strategy:{}", s.name()));
                }
                if energy > budget0 {
                    (budget0, "unpaid".into(), format!("attempt task {} with {label}: energy ran out before the answer ({budget0} left, {energy} needed)", v.id), tg, None, None)
                } else {
                    let prop = attempt_o(&self.world, task, strategy, &rules);
                    let fb: FeedbackO = self.world.feedback(task, prop.answer);
                    let true_correct = task.check(prop.answer);
                    truth.attempted = true;
                    truth.true_correct = true_correct;
                    truth.reported_correct = fb.reported_correct;
                    truth.true_reward = if true_correct { v.reward } else { 0 };
                    truth.rule_used = prop.rule_id.is_some();
                    attempted_task = Some(task.clone());
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
        let reported_reward = if truth.reported_correct { slot.map_or(0, |m| tasks[m].view.reward) } else { 0 };
        let key = format!("being/worldo/choice/{step:07}");
        let group = match (slot, strategy) {
            (Some(m), Some(s)) => Some((tasks[m].view.fam, s)),
            _ => None,
        };
        let evidence = self.state.s2.evidence(group);
        let chosen_txt = match slot {
            Some(m) => {
                let v = &tasks[m].view;
                format!("slot {m} (family {} in slot {} difficulty {}, listed cost {} reward {}) with {}", self.world.fams[tasks[m].fid as usize].name(), v.fam, v.difficulty, v.cost, v.reward, strategy.map_or("the ground truth (oracle)", |s| s.name()))
            }
            None => "skip".into(),
        };
        let dtext = format!("step {step}: chose {chosen_txt} by {} ({}); p_success={} sd={} est_energy={} score={}; cites {} events", ch.policy, ch.why, fin(ch.p), fin(ch.sd), fin(ch.cost_est), fin(ch.score), evidence.len());
        let mut dtags = vec![format!("step:{step}"), format!("action:{label}"), format!("policy:{}", ch.policy), format!("rev_key:{key}")];
        if let Some(m) = slot {
            dtags.push(format!("fam:{}", tasks[m].view.fam));
        }
        let dev = NewEvent { source: SOURCE_O.into(), occurred_at: iso_at(t0), kind: "decision".into(), text: dtext, tags: dtags, redacted: false };
        let dev_id = event_id(&dev.source, &dev.occurred_at, &dev.text);
        let mut atags = vec![format!("step:{step}"), format!("action:{label}")];
        if let Some(r) = &rule_used {
            atags.push(format!("rule_used:{r}"));
        }
        let aev = NewEvent { source: SOURCE_O.into(), occurred_at: iso_at(t0), kind: "action".into(), text: format!("step {step}: {answer_text}"), tags: atags, redacted: false };
        let listed = slot.map_or(cost, |m| tasks[m].view.cost);
        let mut tags = vec![format!("step:{step}")];
        tags.append(&mut otags);
        let explore = ch.policy == b12::POLICY_EXPLORE;
        if explore {
            tags.push("b12_explore".into());
        }
        tags.extend([format!("result:{result}"), format!("reward:{reported_reward}"), format!("cost:{cost}"), format!("listed:{listed}"), format!("budget_left:{budget_left}"), format!("clock:{t1}")]);
        let what = slot.map_or("menu".to_string(), |m| format!("task {}", tasks[m].view.id));
        let oev = NewEvent { source: SOURCE_O.into(), occurred_at: iso_at(t1), kind: "outcome".into(), text: format!("step {step}: {what} is reported {result}; reward {reported_reward} cost {cost} budget left {budget_left}"), tags, redacted: false };
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
        let (prediction, resolved) = if truth.attempted {
            let pp = if ch.p > 0.0 { ch.p } else { 0.5 }.clamp(0.001, 0.999);
            truth.predicted_p = Some(pp);
            let pr = NewPrediction {
                made_at: iso_at(t0),
                question_id: format!("being:worldo:step:{step}"),
                target: format!("task:{}", slot.map_or("none".to_string(), |m| tasks[m].view.id.clone())),
                options: vec!["correct".into(), "wrong".into()],
                probs: vec![pp, 1.0 - pp],
                model_id: format!("being:worldo:{}", ch.policy),
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
            fam: slot.map(|m| tasks[m].view.fam),
            difficulty: truth.difficulty,
            strategy: if result == "correct" || result == "wrong" || result == "unpaid" { strategy } else { None },
            result,
            reward: reported_reward,
            cost,
            listed,
            budget_left,
            clock: t1,
            event_id: oev_id.clone(),
            example: example.map(|mut e| {
                e.event_id = oev_id.clone();
                e
            }),
            rule_used,
            cex: None,
        };
        self.state.s2.apply(&outcome);
        if explore {
            self.state.b12.note_explore(outcome.cost);
        }
        let mut events = fam_events;
        events.extend([dev, aev, oev]);
        Ok(StepO { outcome, truth, label, task: attempted_task, events, revision, prediction, resolved })
    }

    /// One discovery pass over the being's working examples (as World v2, plain B2 guard, with the caps).
    pub fn discover_pass(&mut self) -> Vec<NewRuleO> {
        let found: Vec<Found> = match self.limits.clone() {
            None => self.scan.search(&self.state.s2.examples, discover::K_TRAIN, discover::K_VAL, &[], usize::MAX, &[]).found,
            Some(l) => {
                let all = self.state.s2.blocks();
                let blocks: Vec<discover::Form> = if l.compose { all.iter().take(l.max_blocks).cloned().collect() } else { vec![] };
                if l.compose && all.len() > l.max_blocks {
                    self.refuse(Refusal::new("max_building_blocks", l.max_blocks as u64, all.len() as u64, format!("{} verified building blocks exist; only the first {} are used for composition", all.len(), l.max_blocks)));
                }
                let o = self.scan.search(&self.state.s2.examples, discover::K_TRAIN, discover::K_VAL, &blocks, l.max_candidates, &[]);
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
            if let Some(nr) = self.accept_found(&f, None) {
                out.push(nr);
            }
        }
        if self.state.b12.cfg.form_prior > 0 {
            for (f, how) in b12::family_pass(self) {
                if let Some(nr) = self.accept_found(&f, Some(how)) {
                    out.push(nr);
                }
            }
        }
        out
    }

    /// `b12`: a family-wide rule of Track B12 M4 (with how it was accepted).
    fn accept_found(&mut self, f: &Found, b12: Option<b12::FamilyHow>) -> Option<NewRuleO> {
        let nblocks = if f.xf.is_id() { 0 } else { self.last_blocks as u32 };
        let mut rule = Rule::new_x(f.sig, f.xf, f.form, f.train_ok, f.val_ok, self.state.s2.step, nblocks);
        if self.state.s2.rules.iter().any(|r| r.id == rule.id) {
            return None;
        }
        let (slot, d) = sig2_split(f.sig);
        let fid = self.state.known.get(&slot).copied();
        if let Some(l) = &self.limits {
            let live = self.state.s2.live_rules();
            if live >= l.max_live_rules {
                let cap = l.max_live_rules as u64;
                self.refuse(Refusal::new("max_live_rules", cap, live as u64 + 1, format!("rule {} ({} {}) for slot {slot} difficulty {d} is verified but was not added: {live} rules are live", rule.id, f.xf.code(), f.form.code())));
                return None;
            }
        }
        let at = iso_at(self.state.s2.clock);
        let name = fid.map_or(format!("slot {slot}"), |j| self.world.fams[j as usize].name());
        let text = format!(
            "world O rule {} for family {name} (slot {slot}) difficulty {d}: {}{}; coefficients fitted per task from its own prefix; reproduced the revealed next term exactly on {} training and {} held-out logged examples and was never applicable-and-wrong on them (search inside the author-defined grammar of discover.rs)",
            rule.id, if f.xf.is_id() { String::new() } else { format!("take {} of the prefix, then ", f.xf.words()) }, f.form.dsl(), f.train_ok, f.val_ok
        );
        let mut tags = vec!["rule".to_string(), "worldo".into(), format!("rule_id:{}", rule.id), format!("sig:{}", rule.sig), format!("form:{}", rule.form.code()), format!("train:{}", rule.train_ok), format!("val:{}", rule.val_ok), format!("at_step:{}", rule.step), format!("fam:{slot}"), format!("difficulty:{d}")];
        if let Some(j) = fid {
            tags.push(format!("fid:{j}"));
        }
        if !f.xf.is_id() {
            tags.push(format!("xf:{}", f.xf.code()));
            tags.push(format!("blocks:{nblocks}"));
        }
        let mut text = text;
        if let Some(h) = &b12 {
            tags.push("b12_family".into());
            if h.reduced {
                tags.push("b12_reduced".into());
            }
            text = h.text(&text);
        }
        let event = NewEvent { source: SOURCE_O.into(), occurred_at: at.clone(), kind: "note".into(), text, tags, redacted: false };
        let ev_id = event_id(&event.source, &event.occurred_at, &event.text);
        rule.event_id = ev_id.clone();
        let mut caused_by = f.cite.clone();
        caused_by.push(ev_id);
        let rationale = json!({
            "rule_id": rule.id, "slot": slot, "family_id": fid, "difficulty": d, "form": rule.form.code(), "dsl": rule.form.dsl(), "transform": f.xf.code(),
            "training_examples_reproduced": f.train_ok, "held_out_examples_reproduced": f.val_ok, "cited_examples": f.cite.len(), "accepted_at_step": self.state.s2.step,
        });
        let mut rationale = rationale;
        if let Some(h) = &b12 {
            rationale["b12_family_rule"] = h.json();
        }
        let revision = NewRevision {
            entity_type: "rule".into(),
            key: format!("being/worldo/rule/{}", rule.id),
            stance: format!("family {name} difficulty {d}: {}", rule.form.dsl()),
            confidence: ((f.train_ok + f.val_ok + 1) as f64 / (f.train_ok + f.val_ok + 2) as f64).clamp(0.0, 1.0),
            rationale: rationale.to_string(),
            supersedes: None,
            valid_from: at,
            author: "rule:being-discover".into(),
            caused_by,
        };
        if self.state.apply_rule(rule.clone()) && b12.as_ref().map_or(false, |h| h.reduced) {
            self.state.b12.reduced_rules += 1;
        }
        Some(NewRuleO { rule, event, revision })
    }
}

/// Answer a World O task: a strategy of the being (World v2 solvers, slips with the family's slip
/// rate), or (`None`, oracle runs only) the checker's ground truth with careful slips.
pub fn attempt_o(w: &WorldO, task: &TaskO, strategy: Option<Strategy>, rules: &[Rule]) -> solve::Prop2 {
    let v = &task.view;
    let slip = w.fams[task.fid as usize].slip_pm;
    match strategy {
        Some(s) => {
            let mut p = solve::solve(v, s, rules);
            if p.exact && w.slip_noise(v.step, v.slot) % 1000 < solve::slip_pm(s, slip) {
                p.answer += 1;
                p.method.push_str("+slip");
            }
            p
        }
        None => {
            let mut a = task.expected();
            let mut method = "oracle:ground-truth".to_string();
            if w.slip_noise(v.step, v.slot) % 1000 < solve::slip_pm(Strategy::Careful, slip) {
                a += 1;
                method.push_str("+slip");
            }
            solve::Prop2 { answer: a, exact: true, method, rule_id: None }
        }
    }
}

// ---- the logged run ----

#[derive(Debug, Clone, Default)]
pub struct RunOCfg {
    /// Creation only: the world (must match when the being exists).
    pub world: Option<WOCfg>,
    /// Creation only: the learner's constants.
    pub learner: Option<LearnerOCfg>,
    /// Discovery pass after every N-th step (0: never).
    pub discover_every: u64,
    pub ctl: Option<Limits>,
    /// Track B12: the controller's cap on the exploration budget, per mille of the energy (None: no cap).
    pub max_explore_pm: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct RunOSummary {
    pub steps_run: u64,
    pub stopped: StopReason,
    pub reported_reward: i64,
    pub true_reward: i64,
    pub cost: i64,
    pub state: StateO,
    pub created: bool,
    pub new_rules: Vec<Rule>,
    pub refusals: Vec<Refusal>,
    pub truths: Vec<TrueO>,
    pub version_before: u64,
    pub version_after: u64,
}

/// Run the World O being on its own log with its own learner (or `chooser`, for references and tests).
pub fn run_o(log: &mut VersionedLog, cfg: &RunOCfg, policy: &Policy, chooser: Option<&mut dyn Chooser2>) -> Result<RunOSummary, LogError> {
    let version_before = log.latest_version();
    let rows: Vec<EventRow> = log.open_latest().rows();
    let mut state = StateO::from_events(&rows);
    let mut created = false;
    match &state.cfg {
        Some(c) => {
            if let Some(w) = &cfg.world {
                if w != c {
                    return Err(LogError::Invalid(format!("this World O being already lives in world {:?}; the request says {:?}. A being keeps one world", c, w)));
                }
            }
        }
        None => {
            let w = cfg.world.clone().ok_or_else(|| LogError::Invalid("no World O being exists yet; a world config is needed to create one".into()))?;
            w.validate().map_err(LogError::Invalid)?;
            let l = cfg.learner.unwrap_or_default();
            if l.beta_milli > 10_000 || l.skip_milli > 10_000 {
                return Err(LogError::Invalid("learner constants must be within 0..=10".into()));
            }
            let (ev, id) = genesis_event_o(&w, &l);
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
    let mut engine = EngineO::new(state, cfg.ctl.clone());
    let (mut new_rules, mut truths) = (vec![], vec![]);
    let mut pending = Batch::default();
    if let Some(r) = engine.cap_explore(cfg.max_explore_pm) {
        // logged before any step, so the fold (and the replay) apply the cap from the first decision on
        log.commit_batch(Batch { events: vec![refusal_event_o(&r, engine.state.s2.step, engine.state.s2.clock)], ..Default::default() })?;
    }
    let mut pending_steps = 0usize;
    let (mut done, mut rep, mut tru, mut cost) = (0u64, 0i64, 0i64, 0i64);
    let stopped;
    loop {
        if let Some(r) = policy.gate_cost(done, cost) {
            stopped = r;
            if matches!(r, StopReason::Killed | StopReason::EnergyCap) {
                pending.events.push(stop_event_o(r.as_str(), engine.state.s2.step, engine.state.s2.clock));
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
                if cfg.discover_every > 0 && engine.state.s2.step % cfg.discover_every == 0 {
                    for nr in engine.discover_pass() {
                        pending.events.push(nr.event);
                        pending.revisions.push(nr.revision);
                        new_rules.push(nr.rule);
                    }
                }
                for r in engine.take_refusals() {
                    pending.events.push(refusal_event_o(&r, engine.state.s2.step, engine.state.s2.clock));
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
        pending.events.push(refusal_event_o(&r, engine.state.s2.step, engine.state.s2.clock));
    }
    pending.events.extend(std::mem::take(&mut engine.carry));
    if !pending.events.is_empty() {
        log.commit_batch(pending)?;
    }
    Ok(RunOSummary { steps_run: done, stopped, reported_reward: rep, true_reward: tru, cost, state: engine.state, created, new_rules, refusals: engine.refusals_total, truths, version_before, version_after: log.latest_version() })
}

/// Re-derive every learner decision of a World O log from the events before it (and the world rebuilt
/// from the logged config). Returns (decisions checked, mismatches).
pub fn replay_o(rows: &[EventRow]) -> (u64, u64) {
    let mut st = StateO::default();
    let mut world: Option<WorldO> = None;
    let (mut n, mut bad) = (0u64, 0u64);
    for e in rows.iter().filter(|e| e.source == SOURCE_O) {
        let pol = tag(&e.tags, "policy");
        if e.kind == "decision" && (pol == Some(crate::being::world2::learner::LEARNER_NAME) || pol == Some(b12::POLICY_EXPLORE)) {
            if world.is_none() {
                world = st.cfg.clone().map(WorldO::new);
            }
            if let Some(w) = &world {
                let offers = offers_of_o(&w.offers(st.s2.step, st.s2.clock));
                let mut l = st.learner.learner();
                if !offers.is_empty() {
                    l.prepare_o(&st);
                }
                let got = l.choose(&st.s2, &offers).map(|c| c.act.label());
                n += 1;
                if got.as_deref() != tag(&e.tags, "action") {
                    bad += 1;
                }
            }
        }
        st.fold_event(e);
    }
    (n, bad)
}
