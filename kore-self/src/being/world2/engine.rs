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
use super::rules_use::{self, CExample, ConstRule, UseCfg};
use super::skills::SkillsCfg;
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
use std::borrow::Cow;
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
    /// Track B7 switches (all off by default; see `rules_use.rs`).
    pub use_: UseCfg,
    /// Track B10 switches (adaptation, rule retirement; all off by default; see `skills.rs`).
    pub skills: SkillsCfg,
}

impl Default for LearnerCfg {
    fn default() -> Self {
        let l = Learner2::default();
        LearnerCfg { beta_milli: (l.beta * 1000.0).round() as u32, skip_milli: (l.skip_frac * 1000.0).round() as u32, use_: UseCfg::default(), skills: SkillsCfg::default() }
    }
}

impl LearnerCfg {
    pub fn learner(&self) -> Learner2 {
        Learner2 { beta: self.beta_milli as f64 / 1000.0, skip_frac: self.skip_milli as f64 / 1000.0, use_: self.use_ }
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
    /// The rule that answered (tag `rule_used`), if any.
    pub rule_used: Option<String>,
    /// B7 C: a revealed formula example (only logged with `fit_const` on).
    pub cex: Option<CExample>,
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
    /// B7: reported (attempts, successes) of the attempts each rule answered, per (family, difficulty, rule id).
    pub rule_post: HashMap<(usize, u8, String), (u64, u64)>,
    /// B7 B: examples a robustly accepted rule missed at acceptance (rule id -> count; absent: 0).
    pub rule_missed: HashMap<String, u32>,
    /// B7 C: revealed formula examples and accepted constants.
    pub cexamples: Vec<CExample>,
    pub const_rules: Vec<ConstRule>,
    /// B10 retirement: number of examples (sequence rules) or formula/grid examples (constants) when each rule was accepted.
    pub rule_since: HashMap<String, usize>,
    /// B10 retirement: examples of a signature (sequence) or family (constants) before this index are no longer searched.
    pub sig_floor: HashMap<u8, usize>,
    pub const_floor: HashMap<usize, usize>,
    /// B10: retired constants (ids; never accepted again) and every retirement in order (rule id, step).
    pub retired_consts: Vec<String>,
    pub retirements: Vec<(String, u64)>,
    /// B10 adaptation: every change of the B3b mechanisms with the step and the outcome events it rests on.
    pub changes: Vec<Change2>,
    recent: VecDeque<String>,
    group_recent: HashMap<(usize, usize), VecDeque<String>>,
}

/// `compose` families whose base is `fam` (family types are observable: the type cycle is public).
/// One B3b change in World v2 (B10), with the step and the outcome events it rests on.
#[derive(Debug, Clone, PartialEq)]
pub struct Change2 {
    pub index: usize,
    pub step: u64,
    pub clock: i64,
    pub change: super::model::FamChange,
    pub cites: Vec<String>,
}

pub fn compose_children(fam: usize, families: usize) -> Vec<usize> {
    (0..families).filter(|&i| TYPE_CYCLE[i % TYPE_CYCLE.len()] == FType::Compose && i >= 1 && i - 1 == fam).collect()
}

impl State2 {
    pub fn start(&mut self, cfg: W2Cfg, learner: LearnerCfg, event: String) {
        self.budget_left = cfg.budget;
        self.model = if learner.skills.adapt { FamModel::with_switch(cfg.families, crate::being::adapt_b3b::DEFAULT_SWITCH) } else { FamModel::new(cfg.families) };
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
        if let Some(cx) = &o.cex {
            self.cexamples.push(cx.clone());
        }
        if let Some(r) = &o.rule_used {
            let e = self.rule_post.entry((fam, o.difficulty.clamp(1, 5), r.clone())).or_default();
            e.0 += 1;
            e.1 += (o.result == "correct") as u64;
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
        // B10 adaptation: the changes this outcome caused, citing the latest outcomes of the (family, strategy) group
        while self.changes.len() < self.model.changes.len() {
            let c = self.model.changes[self.changes.len()].clone();
            let cites: Vec<String> = self.group_recent.get(&(fam, s.idx())).map(|q| q.iter().cloned().collect()).unwrap_or_default();
            self.changes.push(Change2 { index: self.changes.len(), step: o.step, clock: o.clock, change: c, cites });
        }
    }

    /// The examples the rule search uses: all of them, except (B10 retirement) those of a signature before its floor.
    pub fn search_examples(&self) -> Cow<'_, [Example]> {
        if self.sig_floor.is_empty() {
            return Cow::Borrowed(&self.examples);
        }
        Cow::Owned(self.examples.iter().enumerate().filter(|(i, e)| *i >= self.sig_floor.get(&e.sig).copied().unwrap_or(0)).map(|(_, e)| e.clone()).collect())
    }

    /// The formula and grid examples the constant search uses (B10: those of a family before its floor are left out).
    pub fn search_cexamples(&self) -> Cow<'_, [CExample]> {
        if self.const_floor.is_empty() {
            return Cow::Borrowed(&self.cexamples);
        }
        Cow::Owned(self.cexamples.iter().enumerate().filter(|(i, e)| *i >= self.const_floor.get(&e.fam).copied().unwrap_or(0)).map(|(_, e)| e.clone()).collect())
    }

    /// B10: retire rule (or constant) `id` at `step`: it is never applied again, its `discovered` cells start over and
    /// the examples of its signature (family) logged so far are no longer searched. False if no live rule has this id.
    pub fn retire_rule(&mut self, id: &str, step: u64) -> bool {
        let families = self.cfg.as_ref().map_or(0, |c| c.families);
        if let Some(r) = self.rules.iter_mut().find(|r| r.id == id && !r.retired) {
            r.retired = true;
            let sig = r.sig;
            let (fam, d) = sig2_split(sig);
            self.model.reset_cell(fam, d, Strategy::Discovered);
            for c in compose_children(fam, families) {
                self.model.reset_cell(c, d, Strategy::Discovered);
            }
            self.sig_floor.insert(sig, self.examples.len());
            self.retirements.push((id.to_string(), step));
            return true;
        }
        if let Some(i) = self.const_rules.iter().position(|c| c.id == id) {
            let c = self.const_rules.remove(i);
            for d in 1..=5 {
                self.model.reset_cell(c.fam, d, Strategy::Discovered);
            }
            self.const_floor.insert(c.fam, self.cexamples.len());
            self.retired_consts.push(c.id);
            self.retirements.push((id.to_string(), step));
            return true;
        }
        false
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
        if self.learner.skills.retire {
            self.rule_since.insert(r.id.clone(), self.examples.len());
        }
        self.rules.push(r);
        true
    }

    /// B7 C: take an accepted constant in; the `discovered` cells of its family start over.
    pub fn apply_const(&mut self, c: ConstRule) -> bool {
        if self.const_rules.iter().any(|x| x.id == c.id) || self.retired_consts.contains(&c.id) {
            return false;
        }
        for d in 1..=5 {
            self.model.reset_cell(c.fam, d, Strategy::Discovered);
        }
        if self.learner.skills.retire {
            self.rule_since.insert(c.id.clone(), self.cexamples.len());
        }
        self.const_rules.push(c);
        true
    }

    pub fn live_rules(&self) -> usize {
        self.rules.iter().filter(|r| !r.retired).count() + self.const_rules.len()
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
                let use_ = tag(&e.tags, "w2_use").and_then(UseCfg::parse).unwrap_or_default();
                let skills = tag(&e.tags, "w2_skills").and_then(SkillsCfg::parse).unwrap_or_default();
                self.start(c, LearnerCfg { beta_milli: b, skip_milli: s, use_, skills }, e.event_id.clone());
            }
        } else if e.kind == "outcome" {
            if let Some(o) = outcome_from_event(e) {
                self.apply(&o);
            }
        } else if e.kind == "note" && has("rule") && has("world2") {
            if let Some(r) = rule_from_event(e) {
                if let Some(m) = tag(&e.tags, "missed").and_then(|x| x.parse::<u32>().ok()) {
                    self.rule_missed.insert(r.id.clone(), m);
                }
                self.apply_rule(r);
            }
        } else if e.kind == "note" && has("const_rule") && has("world2") {
            if let Some(c) = const_from_event(e) {
                self.apply_const(c);
            }
        } else if e.kind == "note" && has("rule_retired") && has("world2") {
            if let (Some(id), Some(step)) = (tag(&e.tags, "rule_id"), tag(&e.tags, "at_step").and_then(|x| x.parse().ok())) {
                self.retire_rule(id, step);
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
    let cex = match (t("x").and_then(|x| x.split(';').map(|y| y.parse().ok()).collect::<Option<Vec<i64>>>()), t("fkind"), t("revealed").and_then(|x| x.parse().ok()), fam) {
        (Some(x), Some(k), Some(rv), Some(f)) => Some(CExample { event_id: e.event_id.clone(), fam: f, kind: k.to_string(), x, revealed: rv }),
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
        rule_used: t("rule_used").map(|x| x.to_string()),
        cex,
    })
}

/// A B7 constant from its note event (tags `const_rule`, `world2`, `rule_id`, `fam`, `g`, `train`, `val`, `missed`, `at_step`).
pub fn const_from_event(e: &EventRow) -> Option<ConstRule> {
    let t = |k: &str| tag(&e.tags, k);
    let fam: usize = t("fam")?.parse().ok()?;
    let g: i64 = t("g")?.parse().ok()?;
    let id = t("rule_id")?.to_string();
    if id != rules_use::const_rule_id(fam, g) {
        return None;
    }
    Some(ConstRule { id, fam, g, train_ok: t("train")?.parse().ok()?, val_ok: t("val")?.parse().ok()?, wrong: t("missed")?.parse().ok()?, step: t("at_step")?.parse().ok()?, event_id: e.event_id.clone() })
}

pub fn genesis_event2(cfg: &W2Cfg, l: &LearnerCfg) -> (NewEvent, String) {
    let mut tags = vec!["w2_genesis".to_string()];
    tags.extend(cfg.tags());
    tags.push(format!("w2_beta_milli:{}", l.beta_milli));
    tags.push(format!("w2_skip_milli:{}", l.skip_milli));
    if l.use_.any() {
        tags.push(l.use_.tag());
    }
    if l.skills.any() {
        tags.push(l.skills.tag());
    }
    let mut text = format!(
        "world v2 being created: seed {}, {} task families, budget {} energy units, feedback noise {} per mille, drift {} changes per 10000 energy, difficulty curve {}%, menu of {}; learner beta {:.3}, skip fraction {:.3}. Simulated world with author-defined families and rewards",
        cfg.seed, cfg.families, cfg.budget, cfg.noise_pm, cfg.drift_per_10k, cfg.curve_pct, cfg.menu, l.beta_milli as f64 / 1000.0, l.skip_milli as f64 / 1000.0
    );
    if l.use_.any() {
        text.push_str(&l.use_.words());
    }
    if l.skills.any() {
        text.push_str(&l.skills.words());
    }
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

/// B7 C: an accepted constant with its note and revision.
pub struct NewConst2 {
    pub rule: ConstRule,
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
    /// Incremental scan of the revealed examples (same answers as a full rescan; see `discover_scan`).
    pub scan: crate::being::discover_scan::ScanCache,
    /// true: rescan every example on every pass (the code before the rule scan fix; tests compare both).
    pub reference_scan: bool,
    /// B7 B: incremental state of the robust search.
    pub rscan: rules_use::RobustCache,
    /// B10 adaptation: changes of the state already handed out for the log.
    pub changes_logged: usize,
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
        let changes_logged = state.changes.len();
        Engine2 { changes_logged, world, state, limits, refusals: vec![], refusals_total: vec![], refused: HashSet::new(), last_blocks: 0, max_candidates_seen: 0, scan: Default::default(), reference_scan: crate::being::discover_scan::reference_default(), rscan: Default::default() }
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
        let use_ = self.state.learner.use_;
        let consts = self.state.const_rules.clone();
        let mut rule_cite: Option<String> = None;
        let mut cex: Option<CExample> = None;
        let mut rule_record: Option<String> = None;
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
                        Some(s) => solve::attempt_c(&self.world, task, s, &rules, &consts),
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
                        let fit = match v.ftype {
                            FType::Formula => use_.fit_const,
                            FType::Grid => use_.fit_grid,
                            _ => false,
                        };
                        if let (true, false, Some(kind)) = (fit, oracle, rules_use::kind_of(v)) {
                            tg.push(format!("x:{}", discover::terms_tag(&v.params)));
                            tg.push(format!("fkind:{kind}"));
                            cex = Some(CExample { event_id: String::new(), fam: v.fam, kind: kind.to_string(), x: v.params.clone(), revealed: fb.revealed });
                        }
                    }
                    if let Some(r) = &prop.rule_id {
                        tg.push(format!("rule_used:{r}"));
                        if use_.any() {
                            // the record the rule use rests on, and the rule's own note as evidence of the decision
                            let (ev, sup, miss) = match self.state.rules.iter().find(|x| &x.id == r) {
                                Some(x) => (x.event_id.clone(), x.train_ok + x.val_ok, self.state.rule_missed.get(r).copied().unwrap_or(0)),
                                None => consts.iter().find(|c| &c.id == r).map_or((String::new(), 0, 0), |c| (c.event_id.clone(), c.train_ok + c.val_ok, c.wrong)),
                            };
                            let (pn, ps) = self.state.rule_post.get(&(v.fam, v.difficulty.clamp(1, 5), r.clone())).copied().unwrap_or((0, 0));
                            rule_record = Some(format!("rule_record:support{sup}/missed{miss}/used{pn}/reported_right{ps}"));
                            if !ev.is_empty() {
                                rule_cite = Some(ev);
                            }
                        }
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
        let mut evidence = self.state.evidence(group);
        if let Some(ev) = &rule_cite {
            if !evidence.contains(ev) {
                evidence.push(ev.clone());
            }
        }
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
        if let Some(rr) = &rule_record {
            atags.push(rr.clone());
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
            rule_used: rule_used.clone(),
            cex: cex.map(|mut c| {
                c.event_id = oev_id.clone();
                c
            }),
        };
        self.state.apply(&outcome);
        Ok(Step2Out { outcome, truth, label, events: vec![dev, aev, oev], revision, prediction, resolved })
    }

    /// `discover::search_composed` over the being's examples: incremental, or the full rescan with `reference_scan`.
    pub fn search_examples(&mut self, blocks: &[discover::Form], max_candidates: usize) -> discover::SearchOut {
        let ex = self.state.search_examples();
        if self.reference_scan {
            discover::search_composed(&ex, blocks, max_candidates)
        } else {
            self.scan.search(&ex, discover::K_TRAIN, discover::K_VAL, blocks, max_candidates, &[])
        }
    }

    /// One discovery pass over the being's own revealed examples (as `Engine::discover_pass`, with the caps).
    pub fn discover_pass(&mut self) -> Vec<NewRule2> {
        let theta = self.state.learner.use_.robust_milli;
        let mut allowed_sigs: Option<usize> = None;
        let found: Vec<Found> = match self.limits.clone() {
            None => self.search_examples(&[], usize::MAX).found,
            Some(l) => {
                let all = self.state.blocks();
                let blocks: Vec<discover::Form> = if l.compose { all.iter().take(l.max_blocks).cloned().collect() } else { vec![] };
                if l.compose && all.len() > l.max_blocks {
                    self.refuse(Refusal::new("max_building_blocks", l.max_blocks as u64, all.len() as u64, format!("{} verified building blocks exist; only the first {} are used for composition", all.len(), l.max_blocks)));
                }
                let o = self.search_examples(&blocks, l.max_candidates);
                if o.truncated {
                    self.refuse(Refusal::new("max_search_candidates", l.max_candidates as u64, o.wanted as u64, format!("a discovery pass would try {} candidates; it stopped after {}", o.wanted, o.candidates)));
                    // B7 B keeps to the signatures the capped search covered completely
                    allowed_sigs = Some(o.candidates / discover::candidates_for(&blocks).len().max(1));
                }
                self.last_blocks = blocks.len();
                self.max_candidates_seen = self.max_candidates_seen.max(o.candidates);
                o.found
            }
        };
        if theta > 0 {
            // B7 B: plain grammar forms under the robust guard; composed candidates as found above
            let ex = self.state.search_examples();
            let mut sigs: Vec<u8> = ex.iter().map(|e| e.sig).collect();
            sigs.sort_unstable();
            sigs.dedup();
            let keep: Vec<u8> = sigs.into_iter().take(allowed_sigs.unwrap_or(usize::MAX)).collect();
            let mut all: Vec<(Found, u32)> = self.rscan.search(&ex, discover::K_TRAIN, discover::K_VAL, theta).into_iter().filter(|r| keep.contains(&r.found.sig)).map(|r| (r.found, r.wrong)).collect();
            all.extend(found.into_iter().filter(|f| !f.xf.is_id()).map(|f| (f, 0)));
            all.sort_by_key(|(f, _)| f.sig);
            let mut out = vec![];
            for (f, missed) in all {
                if let Some(nr) = self.accept_found_m(&f, missed) {
                    out.push(nr);
                }
            }
            return out;
        }
        let mut out = vec![];
        for f in found {
            if let Some(nr) = self.accept_found(&f) {
                out.push(nr);
            }
        }
        out
    }

    /// B7 C: one pass of the constant search over the being's revealed formula examples.
    pub fn const_pass(&mut self) -> Vec<NewConst2> {
        if !self.state.learner.use_.fit_const && !self.state.learner.use_.fit_grid {
            return vec![];
        }
        let theta = self.state.learner.use_.robust_milli;
        let mut out = vec![];
        let found = rules_use::search_const(&self.state.search_cexamples(), discover::K_TRAIN, discover::K_VAL, theta);
        for f in found {
            let id = rules_use::const_rule_id(f.fam, f.g);
            if self.state.const_rules.iter().any(|c| c.id == id) || self.state.retired_consts.contains(&id) {
                continue;
            }
            if let Some(l) = &self.limits {
                let live = self.state.live_rules();
                if live >= l.max_live_rules {
                    let cap = l.max_live_rules as u64;
                    self.refuse(Refusal::new("max_live_rules", cap, live as u64 + 1, format!("constant {id} ({} = {}) for family f{:02} is verified but was not added: {live} rules are live", if TYPE_CYCLE[f.fam % TYPE_CYCLE.len()] == FType::Grid { "move rule" } else { "g" }, f.g, f.fam)));
                    continue;
                }
            }
            let at = iso_at(self.state.clock);
            let grid = TYPE_CYCLE[f.fam % TYPE_CYCLE.len()] == FType::Grid;
            let what = if grid { format!("grid family f{:02}: moves to {} neighbours (shortest path by breadth-first search under that move rule)", f.fam, f.g) } else { format!("formula family f{:02}: g = {}", f.fam, f.g) };
            let text = format!(
                "world v2 constant {id} for {what}; reproduced the revealed answer exactly on {} training and {} held-out logged examples{} (one integer fitted inside the author-defined task statement)", f.train_ok, f.val_ok, if f.wrong == 0 { " and missed none".to_string() } else { format!(" and missed {} (robust acceptance, Wilson lower bound {:.3})", f.wrong, rules_use::wilson_lb(f.train_ok + f.val_ok, f.train_ok + f.val_ok + f.wrong)) }
            );
            let tags = vec!["const_rule".to_string(), "world2".into(), format!("rule_id:{id}"), format!("fam:{}", f.fam), format!("g:{}", f.g), format!("train:{}", f.train_ok), format!("val:{}", f.val_ok), format!("missed:{}", f.wrong), format!("at_step:{}", self.state.step)];
            let event = NewEvent { source: SOURCE2.into(), occurred_at: at.clone(), kind: "note".into(), text, tags, redacted: false };
            let ev_id = event_id(&event.source, &event.occurred_at, &event.text);
            let rule = ConstRule { id: id.clone(), fam: f.fam, g: f.g, train_ok: f.train_ok, val_ok: f.val_ok, wrong: f.wrong, step: self.state.step, event_id: ev_id.clone() };
            let mut caused_by = f.cite.clone();
            caused_by.push(ev_id);
            let rationale = json!({
                "rule_id": id, "family": f.fam, "constant_g": f.g, "training_examples_reproduced": f.train_ok, "held_out_examples_reproduced": f.val_ok, "examples_missed": f.wrong,
                "robust_threshold_milli": theta, "cited_examples": f.cite.len(), "accepted_at_step": self.state.step,
                "note": "examples are the answers the being was TOLD; with feedback noise some are wrong",
            });
            let n = (f.train_ok + f.val_ok + f.wrong) as f64;
            let revision = NewRevision {
                entity_type: "rule".into(),
                key: format!("being/world2/rule/{id}"),
                stance: if grid { format!("family f{:02}: move rule {} neighbours", f.fam, f.g) } else { format!("family f{:02}: g = {}", f.fam, f.g) },
                confidence: (((f.train_ok + f.val_ok) as f64 + 1.0) / (n + 2.0)).clamp(0.0, 1.0),
                rationale: rationale.to_string(),
                supersedes: None,
                valid_from: at,
                author: "rule:being-discover".into(),
                caused_by,
            };
            self.state.apply_const(rule.clone());
            out.push(NewConst2 { rule, event, revision });
        }
        out
    }

    fn accept_found(&mut self, f: &Found) -> Option<NewRule2> {
        self.accept_found_m(f, 0)
    }

    /// `missed` > 0 only for B7 B (robust acceptance): the examples the form missed.
    fn accept_found_m(&mut self, f: &Found, missed: u32) -> Option<NewRule2> {
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
        let mut text = format!(
            "world v2 rule {} for family f{fam:02} difficulty {d}: {}{}; coefficients fitted per task from its own prefix; reproduced the revealed next term exactly on {} training and {} held-out logged examples and was never applicable-and-wrong on them (search inside the author-defined grammar of discover.rs)",
            rule.id, if f.xf.is_id() { String::new() } else { format!("take {} of the prefix, then ", f.xf.words()) }, f.form.dsl(), f.train_ok, f.val_ok
        );
        let theta = self.state.learner.use_.robust_milli;
        if missed > 0 {
            text = format!(
                "world v2 rule {} for family f{fam:02} difficulty {d}: {}; coefficients fitted per task from its own prefix; reproduced the revealed next term exactly on {} training and {} held-out logged examples and missed it on {} (robust acceptance: Wilson lower bound {:.3} of the share right, threshold {:.3}; search inside the author-defined grammar of discover.rs)",
                rule.id, f.form.dsl(), f.train_ok, f.val_ok, missed, rules_use::wilson_lb(f.train_ok + f.val_ok, f.train_ok + f.val_ok + missed), theta as f64 / 1000.0
            );
        }
        let mut tags = vec!["rule".to_string(), "world2".into(), format!("rule_id:{}", rule.id), format!("sig:{}", rule.sig), format!("form:{}", rule.form.code()), format!("train:{}", rule.train_ok), format!("val:{}", rule.val_ok), format!("at_step:{}", rule.step), format!("fam:{fam}"), format!("difficulty:{d}")];
        if !f.xf.is_id() {
            tags.push(format!("xf:{}", f.xf.code()));
            tags.push(format!("blocks:{nblocks}"));
        }
        if missed > 0 {
            tags.push(format!("missed:{missed}"));
            tags.push(format!("robust_milli:{theta}"));
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
        if missed > 0 {
            self.state.rule_missed.insert(rule.id.clone(), missed);
        }
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
                // B10 (only when switched on at creation): changes of the adaptive model, then retirements before discovery
                for (ev, rv) in engine.take_change_records() {
                    pending.events.push(ev);
                    pending.revisions.push(rv);
                }
                if cfg.discover_every > 0 && engine.state.step % cfg.discover_every == 0 {
                    for nr in engine.retire_pass() {
                        pending.events.push(nr.event);
                        pending.revisions.push(nr.revision);
                    }
                    for nr in engine.discover_pass() {
                        pending.events.push(nr.event);
                        pending.revisions.push(nr.revision);
                        new_rules.push(nr.rule);
                    }
                    for nc in engine.const_pass() {
                        pending.events.push(nc.event);
                        pending.revisions.push(nc.revision);
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
