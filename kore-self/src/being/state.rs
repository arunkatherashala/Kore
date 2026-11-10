//! The being's state, derived from its own log.
//!
//! Nothing here is stored anywhere except as events in the versioned log
//! (source `being:loop`). `BeingState::from_snapshot` folds those events, in
//! log order, into counts per domain, the remaining budget and the clock, and
//! the live loop updates the same structure with the same `apply` call. The
//! replay tests check that both routes give the same state and therefore the
//! same next decision.

use super::advice::{decision_from_tags, pending_from_tags, AdviceSetup, AdvisorKind, DecisionAdvice, PendingAdvice, ADVISOR_SOURCE, RULE_KIND};
use super::discover::{example_from_event, rule_from_event, Example, Rule};
use super::trust::{TrustAlarm, TrustTable};
use super::model::{AdaptMode, Alarm, Model, Obs, DEFAULT_HALF_LIFE};
use super::solver::Strategy;
use super::transfer::{self, XferKind, XferMode, XferRec, XferState};
use super::world::{iso_at, Domain, ALL_DOMAINS_X, N_DOM};
use crate::versioned::{event_id, EventRow, LogError, NewEvent, Snapshot, VersionedLog};
use std::collections::{BTreeMap, VecDeque};

pub const SOURCE: &str = "being:loop";
/// A decision cites at most this many of the latest outcome events per domain
/// (plus the genesis event). The counts it used come from all outcomes.
pub const EVIDENCE_WINDOW: usize = 5;
/// Efficiency (reward points per energy unit) is divided by this to land
/// roughly in 0..1 before the exploration bonus is added.
pub const EFF_NORM: f64 = 10.0;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, Default, PartialEq)]
pub struct DomainStat {
    pub n: u64,
    pub correct: u64,
    pub reward: i64,
    pub cost: i64,
}

impl DomainStat {
    /// Reward points per energy unit spent (the quantity the being is paid in).
    pub fn efficiency(&self) -> f64 {
        if self.cost > 0 {
            self.reward as f64 / self.cost as f64
        } else {
            0.0
        }
    }
    pub fn value(&self) -> f64 {
        self.efficiency() / EFF_NORM
    }
}

/// Which chooser the being was created with. `Ucb` is the original
/// single-task world with the UCB1 domain chooser; `Model` is the menu world
/// with the learned world model and planner.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chooser {
    Ucb,
    Model,
}

impl Chooser {
    pub fn name(self) -> &'static str {
        match self {
            Chooser::Ucb => "ucb",
            Chooser::Model => "model",
        }
    }
    pub fn parse(s: &str) -> Option<Chooser> {
        match s {
            "ucb" => Some(Chooser::Ucb),
            "model" => Some(Chooser::Model),
            _ => None,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct Genesis {
    pub world_seed: u64,
    pub loop_seed: u64,
    pub budget: i64,
    /// UCB1 exploration constant, or beta of the model planner.
    pub explore_c: f64,
    pub chooser: Chooser,
    /// Pattern tasks come from the extended hidden families and the `discovered` strategy exists (menu world only).
    pub extended: bool,
    /// B5: pattern tasks come from the `extended2` families (implies `extended`).
    pub ext2: bool,
    /// B5: the sequence rule of one family changes halfway through the budget (extended2 only).
    pub rule_drift: bool,
    /// The world has scheduled hidden changes (menu world only). Fixed once the being exists.
    pub drift: bool,
    /// How the world model treats old evidence. Fixed once the being exists.
    pub adapt: AdaptMode,
    /// Which advisor (if any) the being takes advice from and whether it learns trust. Fixed once the being exists.
    pub advice: AdviceSetup,
    /// B6: the world has two phases and the domains `pattern_b` and `control` (needs `ext2`). Fixed once the being exists.
    pub transfer: bool,
    /// B6: the model links new domains to learned ones (transfer). Fixed once the being exists.
    pub learn_transfer: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub step: u64,
    pub domain: Domain,
    pub task_id: String,
    pub difficulty: u8,
    pub correct: bool,
    pub reward: i64,
    pub cost: i64,
    pub budget_left: i64,
    pub clock: i64,
    pub event_id: String,
    /// Menu mode: the strategy used (None in single-task mode).
    pub strategy: Option<Strategy>,
    /// Cost printed on the task (equals `cost` in single-task mode).
    pub listed_cost: i64,
    /// No attempt was run (menu skipped, or the energy ran out before the attempt):
    /// only the energy is accounted for; no domain statistics, no model update.
    pub skipped: bool,
    /// Sequence attempts of the extended world: the prefix and the revealed next term.
    pub example: Option<Example>,
    /// Id of the discovered rule that produced the answer of this attempt (B5 pruning statistics).
    pub rule_used: Option<String>,
}

/// How often a rule was used and how it fared (folded from outcomes that carry `rule_used`).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq)]
pub struct RuleUse {
    pub uses: u32,
    pub right: u32,
    /// Latest uses, oldest first: (outcome event id, correct). At most `RULE_USE_KEEP`.
    pub recent: VecDeque<(String, bool)>,
}

pub const RULE_USE_KEEP: usize = 24;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct BeingState {
    pub genesis: Option<Genesis>,
    pub genesis_event: String,
    /// Completed steps (outcome events).
    pub step: u64,
    pub budget_left: i64,
    /// World seconds since the origin.
    pub clock: i64,
    pub stats: [DomainStat; N_DOM],
    /// Latest outcome event ids per domain, oldest first.
    pub recent: [VecDeque<String>; N_DOM],
    pub total_reward: i64,
    pub total_cost: i64,
    pub refills: u64,
    /// Learned world model, folded from the attempts in the log.
    pub model: Model,
    /// Revealed sequence examples read from the log (extended world only), in log order.
    pub examples: Vec<Example>,
    /// Rules accepted into the log, in acceptance order.
    pub rules: Vec<Rule>,
    /// Use statistics per rule id (B5).
    pub rule_use: BTreeMap<String, RuleUse>,
    /// Changes the model's detector has flagged, in order (adaptive learner only).
    pub changes: Vec<Change>,
    /// Advice read from the log for the step in progress (set by the advice event, consumed by the outcome).
    pub advice: Option<PendingAdvice>,
    /// What the decision did with that advice (set by the decision event, consumed by the outcome).
    pub decision_advice: Option<DecisionAdvice>,
    /// Learned reliability of each advisor, folded from advice, decision and outcome events.
    pub trust: TrustTable,
    /// Degradations of an advisor flagged so far: (step, alarm).
    pub advice_alarms: Vec<(u64, TrustAlarm)>,
    /// Latest outcome event ids per (domain, strategy), oldest first: the evidence a detected change cites.
    group_recent: [[VecDeque<String>; 4]; N_DOM],
    /// B6: transfer settings, links made and decisions not yet logged.
    pub xfer: XferState,
    /// Latest outcome event ids per (domain, difficulty), oldest first (evidence of a transfer link).
    cell_recent: BTreeMap<(usize, u8), VecDeque<String>>,
    /// Track B14: the switches of this being (from its `b14_setup` note; None: off, the being is as before).
    pub b14: Option<super::b14::ctl::B14Setup>,
    /// Track B14: library entries carried over from an earlier life (`rule_lib_import` notes), in log order.
    pub b14_imports: Vec<super::b14::library::LibEntry>,
    /// Track B14: energy spent on curiosity steps (decisions of policy `b14-curious`), folded from the log.
    pub b14_cur_energy: i64,
    b14_cur_pending: bool,
    /// Track E1: the evolution setup, passes, evaluations spent and latest elites (folded from `e1_setup` / `e1_pass` notes).
    pub e1: super::e1::evolve::E1State,
}

/// A change flagged by the detector, with the logged outcomes it rests on.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct Change {
    pub index: usize,
    pub step: u64,
    pub clock: i64,
    pub alarm: Alarm,
    pub cites: Vec<String>,
}

/// Outcome events a detected change cites at most.
pub const CHANGE_CITES: usize = 12;

/// Value of `key:` among comma-joined tags.
pub fn tag<'a>(tags: &'a str, key: &str) -> Option<&'a str> {
    tags.split(',').find_map(|t| t.strip_prefix(key).and_then(|r| r.strip_prefix(':')))
}

fn has_tag(tags: &str, key: &str) -> bool {
    tags.split(',').any(|t| t == key)
}

impl BeingState {
    pub fn total_pulls(&self) -> u64 {
        self.stats.iter().map(|s| s.n).sum()
    }

    pub fn start(&mut self, g: Genesis, event: String) {
        self.budget_left = g.budget;
        self.model.set_mode(g.adapt, DEFAULT_HALF_LIFE);
        self.xfer.cfg.mode = if g.learn_transfer { XferMode::On } else { XferMode::Off };
        self.genesis = Some(g);
        self.genesis_event = event;
    }

    /// Advice for the step in progress (from its advice event, live or replayed).
    pub fn note_advice(&mut self, a: PendingAdvice) {
        self.advice = Some(a);
    }

    /// What the planner did with it (from the decision event, live or replayed).
    pub fn note_decision_advice(&mut self, d: DecisionAdvice) {
        self.decision_advice = Some(d);
    }

    /// Verdict of the verified search on a proposed rule (factual advice: accepted = right).
    pub fn note_rule_verdict(&mut self, advisor: &str, accepted: bool) {
        self.trust.note_given(advisor, RULE_KIND);
        if let Some(al) = self.trust.observe(advisor, RULE_KIND, accepted, false) {
            self.advice_alarms.push((self.step, al));
        }
    }

    /// Judge the advice of the step that just ended against its outcome and update trust.
    /// A test happens only when the advice was followed although the model alone preferred something
    /// else; it is won when the realized reward per energy reaches what the model alone predicted for
    /// its own choice (see `trust.rs`).
    fn settle_advice(&mut self, o: &Outcome) {
        let (adv, dec) = (self.advice.take(), self.decision_advice.take());
        let (Some(a), Some(d)) = (adv, dec) else {
            return;
        };
        let Some(m) = &a.menu else {
            return;
        };
        if a.step != o.step || d.id != a.id {
            return;
        }
        self.trust.note_given(&a.advisor, m.domain.name());
        if o.skipped || !d.followed || d.agree {
            return;
        }
        let x = o.reward as f64 / o.cost.max(1) as f64;
        if let Some(al) = self.trust.observe(&a.advisor, m.domain.name(), x >= d.ref_score, true) {
            self.advice_alarms.push((o.step, al));
        }
    }

    pub fn apply(&mut self, o: &Outcome) {
        self.settle_advice(o);
        self.model.note_step(o.reward, o.cost);
        if o.skipped {
            self.total_cost += o.cost;
            self.budget_left = o.budget_left;
            self.clock = o.clock;
            self.step += 1;
            return;
        }
        if let Some(ex) = &o.example {
            self.examples.push(ex.clone());
        }
        if let Some(id) = &o.rule_used {
            let u = self.rule_use.entry(id.clone()).or_default();
            u.uses += 1;
            u.right += o.correct as u32;
            u.recent.push_back((o.event_id.clone(), o.correct));
            while u.recent.len() > RULE_USE_KEEP {
                u.recent.pop_front();
            }
        }
        if let Some(strategy) = o.strategy {
            let gq = &mut self.group_recent[o.domain.idx()][strategy.idx()];
            gq.push_back(o.event_id.clone());
            while gq.len() > CHANGE_CITES {
                gq.pop_front();
            }
            let cq = self.cell_recent.entry((o.domain.idx(), o.difficulty)).or_default();
            cq.push_back(o.event_id.clone());
            while cq.len() > CHANGE_CITES {
                cq.pop_front();
            }
            if let Some(alarm) = self.model.observe(&Obs { domain: o.domain, difficulty: o.difficulty, strategy, correct: o.correct, listed_cost: o.listed_cost, spent: o.cost }) {
                let cites: Vec<String> = self.group_recent[o.domain.idx()][strategy.idx()].iter().cloned().collect();
                self.changes.push(Change { index: self.changes.len(), step: o.step, clock: o.clock, alarm, cites });
            }
            for alarm in self.model.take_extra_alarms() {
                let cites: Vec<String> = self.group_recent[o.domain.idx()][strategy.idx()].iter().cloned().collect();
                self.changes.push(Change { index: self.changes.len(), step: o.step, clock: o.clock, alarm, cites });
            }
            for rv in self.model.take_revoked() {
                let src = self.model.link(rv.domain, rv.difficulty).map(|l| l.src);
                let cites: Vec<String> = self.cell_recent.get(&(rv.domain.idx(), rv.difficulty)).map(|q| q.iter().cloned().collect()).unwrap_or_default();
                let index = self.xfer.next_index;
                self.xfer.next_index += 1;
                let rec = XferRec {
                    kind: XferKind::Revoke { delta: rv.delta, n: rv.n },
                    index,
                    step: o.step + 1,
                    clock: o.clock,
                    target: rv.domain,
                    difficulty: rv.difficulty,
                    src,
                    src_k: rv.difficulty,
                    evidence: serde_json::json!({ "log_likelihood_difference": rv.delta, "attempts_influenced": rv.n, "threshold_nats": self.model.guard_nats }),
                    cites,
                };
                self.xfer.pending.push(rec);
            }
        }
        let s = &mut self.stats[o.domain.idx()];
        s.n += 1;
        s.correct += o.correct as u64;
        s.reward += o.reward;
        s.cost += o.cost;
        self.total_reward += o.reward;
        self.total_cost += o.cost;
        self.budget_left = o.budget_left;
        self.clock = o.clock;
        self.step += 1;
        let q = &mut self.recent[o.domain.idx()];
        q.push_back(o.event_id.clone());
        while q.len() > EVIDENCE_WINDOW {
            q.pop_front();
        }
    }

    /// Take an accepted rule into the state. False if a rule with this id is already there
    /// (discovering twice changes nothing). The `discovered` estimates of that signature start over,
    /// because the strategy now does something different there.
    pub fn apply_rule(&mut self, r: Rule) -> bool {
        if self.rules.iter().any(|x| x.id == r.id) {
            return false;
        }
        let (dom, diff) = super::discover::sig_split(r.sig);
        self.model.reset_cell(dom, diff, Strategy::Discovered);
        if dom == Domain::PatternB {
            self.model.set_disc_ready(dom, diff, true);
        }
        self.rules.push(r);
        true
    }

    /// Retire a rule (logged by a `rule_retired` note): it stays in the book but is never applied again.
    /// False if unknown or already retired. The `discovered` estimates of its signature start over.
    pub fn apply_retire(&mut self, id: &str) -> bool {
        let Some(r) = self.rules.iter_mut().find(|r| r.id == id && !r.retired) else {
            return false;
        };
        r.retired = true;
        let sig = r.sig;
        let (dom, diff) = super::discover::sig_split(sig);
        self.model.reset_cell(dom, diff, Strategy::Discovered);
        if dom == Domain::PatternB && !self.rules.iter().any(|x| !x.retired && x.sig == sig) {
            self.model.set_disc_ready(dom, diff, false);
        }
        true
    }

    /// Take a transfer link into the model (the engine decided it, or a replayed `transfer_link` note says so).
    pub fn apply_link(&mut self, target: Domain, difficulty: u8, src: Domain, src_k: u8, guard: bool) -> bool {
        let made = self.model.set_link(target, difficulty, src, src_k, guard);
        if made {
            self.xfer.made += 1;
        }
        made
    }

    /// Latest outcome event ids at (domain, difficulty), oldest first.
    pub fn cell_recent(&self, d: Domain, difficulty: u8) -> Vec<String> {
        self.cell_recent.get(&(d.idx(), difficulty)).map(|q| q.iter().cloned().collect()).unwrap_or_default()
    }

    /// The building blocks: forms of the plain rules the being holds and has not retired, in acceptance order.
    pub fn blocks(&self) -> Vec<super::discover::Form> {
        let mut v: Vec<super::discover::Form> = vec![];
        for r in self.rules.iter().filter(|r| r.xf.is_id() && !r.retired && !r.form.is_gp()) {
            if !v.contains(&r.form) {
                v.push(r.form);
            }
        }
        v
    }

    /// Rules that are still applied.
    pub fn live_rules(&self) -> usize {
        self.rules.iter().filter(|r| !r.retired).count()
    }

    pub fn apply_refill(&mut self, budget_left: i64, clock: i64) {
        self.budget_left = budget_left;
        self.clock = clock;
        self.refills += 1;
    }

    /// The events a decision made now would cite.
    pub fn evidence_ids(&self) -> Vec<String> {
        let mut v = vec![self.genesis_event.clone()];
        for d in ALL_DOMAINS_X {
            v.extend(self.recent[d.idx()].iter().cloned());
        }
        v
    }

    /// Fold the being's events (log order) into a state.
    pub fn from_snapshot(snap: &Snapshot) -> BeingState {
        Self::from_events(&snap.rows::<EventRow>())
    }

    /// Fold the being's events straight from the log: an events-only read, one chunk of segments at
    /// a time (no revision text is decoded, no row is kept). The same state as `from_snapshot`.
    pub fn from_log(log: &VersionedLog) -> Result<BeingState, LogError> {
        let mut st = BeingState::default();
        log.for_each_event(|e| {
            if e.source == SOURCE || e.source == ADVISOR_SOURCE {
                st.fold_event(&e);
            }
            true
        })?;
        Ok(st)
    }

    /// The being's genesis (the first logged genesis note that parses), reading events only until it.
    pub fn genesis_of(log: &VersionedLog) -> Result<Option<Genesis>, LogError> {
        let mut st = BeingState::default();
        log.for_each_event(|e| {
            if e.source == SOURCE || e.source == ADVISOR_SOURCE {
                st.fold_event(&e);
            }
            st.genesis.is_none()
        })?;
        Ok(st.genesis)
    }

    /// The genesis of the being in the log at `<root>/<owner>/log` without opening the log (see
    /// `VersionedLog::peek_events`): only the events up to the genesis are read. No log yet: `None`.
    pub fn genesis_peek(root: &std::path::Path, owner: &str) -> Result<Option<Genesis>, LogError> {
        if !root.join(owner).join("log").join("MANIFEST").exists() {
            return Ok(None);
        }
        let mut st = BeingState::default();
        VersionedLog::peek_events(root, owner, |e| {
            if e.source == SOURCE || e.source == ADVISOR_SOURCE {
                st.fold_event(&e);
            }
            st.genesis.is_none()
        })?;
        Ok(st.genesis)
    }

    pub fn from_events(rows: &[EventRow]) -> BeingState {
        let mut st = BeingState::default();
        for e in rows.iter().filter(|e| e.source == SOURCE || e.source == ADVISOR_SOURCE) {
            st.fold_event(e);
        }
        st
    }

    /// One event of source `advisor`: advice and verdicts feed the state; results and alarms are derived records and are ignored.
    pub fn fold_advisor(&mut self, kind: &str, tags: &str, event_id: &str) {
        match kind {
            "advice" => {
                if let Some(p) = pending_from_tags(event_id, tags) {
                    self.note_advice(p);
                }
            }
            "verdict" => {
                let accepted = match tag(tags, "verdict") {
                    Some("accepted") => true,
                    Some("rejected") => false,
                    _ => return,
                };
                if let Some(who) = tag(tags, "advisor") {
                    self.note_rule_verdict(who, accepted);
                }
            }
            _ => {}
        }
    }

    /// Apply one logged event (genesis, refill or outcome); others are ignored.
    pub fn fold_event(&mut self, e: &EventRow) {
        if e.source == ADVISOR_SOURCE {
            self.fold_advisor(tag(&e.tags, "ev").unwrap_or(""), &e.tags, &e.event_id);
            return;
        }
        if e.source != SOURCE {
            return;
        }
        if e.kind == "decision" {
            if let Some(d) = decision_from_tags(&e.tags) {
                self.note_decision_advice(d);
            }
            self.b14_cur_pending = tag(&e.tags, "policy") == Some(super::b14::curiosity::CUR_POLICY);
        } else if e.kind == "note" && has_tag(&e.tags, super::b14::ctl::SETUP_TAG) {
            if self.b14.is_none() {
                self.b14 = super::b14::ctl::B14Setup::from_tags(&e.tags);
            }
        } else if e.kind == "note" && has_tag(&e.tags, super::e1::ctl::SETUP_TAG) {
            self.e1.fold_setup(&e.tags);
        } else if e.kind == "note" && has_tag(&e.tags, super::e1::evolve::PASS_TAG) {
            self.e1.fold_pass(&e.tags);
        } else if e.kind == "note" && has_tag(&e.tags, super::b14::library::IMPORT_TAG) {
            if let Some(x) = super::b14::library::LibEntry::from_event(e) {
                if !self.b14_imports.iter().any(|y| y.id() == x.id()) {
                    self.b14_imports.push(x);
                }
            }
        } else if e.kind == "note" && has_tag(&e.tags, "genesis") {
            if let Some(g) = genesis_from_tags(&e.tags) {
                if self.genesis.is_none() {
                    self.start(g, e.event_id.clone());
                }
            }
        } else if e.kind == "note" && has_tag(&e.tags, "refill") {
            let left = tag(&e.tags, "budget_left").and_then(|v| v.parse().ok()).unwrap_or(0);
            let clock = tag(&e.tags, "clock").and_then(|v| v.parse().ok()).unwrap_or(self.clock);
            self.apply_refill(left, clock);
        } else if e.kind == "note" && has_tag(&e.tags, "transfer_link") {
            if let Some((t, k, s, sk, g)) = transfer::link_from_event(e) {
                self.apply_link(t, k, s, sk, g);
            }
        } else if e.kind == "note" && has_tag(&e.tags, "rule_retired") {
            if let Some(id) = tag(&e.tags, "rule_id") {
                self.apply_retire(id);
            }
        } else if e.kind == "note" && has_tag(&e.tags, "rule") {
            if let Some(r) = rule_from_event(e) {
                self.apply_rule(r);
            }
        } else if e.kind == "outcome" {
            if let Some(o) = outcome_from_event(e) {
                if std::mem::take(&mut self.b14_cur_pending) {
                    self.b14_cur_energy += o.cost;
                }
                self.apply(&o);
            }
        }
    }
}

fn genesis_from_tags(tags: &str) -> Option<Genesis> {
    Some(Genesis {
        world_seed: tag(tags, "world_seed")?.parse().ok()?,
        loop_seed: tag(tags, "loop_seed")?.parse().ok()?,
        budget: tag(tags, "budget")?.parse().ok()?,
        explore_c: tag(tags, "explore_c")?.parse().ok()?,
        chooser: match tag(tags, "chooser") {
            Some(c) => Chooser::parse(c)?,
            None => Chooser::Ucb,
        },
        extended: matches!(tag(tags, "pattern_families"), Some("extended") | Some("extended2")),
        ext2: tag(tags, "pattern_families") == Some("extended2"),
        rule_drift: tag(tags, "rule_drift") == Some("on"),
        drift: tag(tags, "drift") == Some("on"),
        adapt: tag(tags, "adapt").and_then(AdaptMode::parse).unwrap_or(AdaptMode::Off),
        advice: AdviceSetup {
            kind: tag(tags, "advisor").and_then(AdvisorKind::parse).unwrap_or(AdvisorKind::None),
            quality: tag(tags, "advisor_quality").and_then(|q| q.parse().ok()).unwrap_or(0.0),
            trust: tag(tags, "trust") != Some("off"),
        },
        transfer: tag(tags, "transfer") == Some("on"),
        learn_transfer: tag(tags, "learn_transfer") == Some("on"),
    })
}

pub fn outcome_from_event(e: &EventRow) -> Option<Outcome> {
    let t = &e.tags;
    let result = tag(t, "result")?;
    let skipped = result == "skip" || result == "unpaid";
    let cost: i64 = tag(t, "cost")?.parse().ok()?;
    Some(Outcome {
        step: tag(t, "step")?.parse().ok()?,
        domain: match tag(t, "domain").and_then(Domain::parse) {
            Some(d) => d,
            None if skipped => Domain::Arithmetic,
            None => return None,
        },
        task_id: tag(t, "task").unwrap_or("none").to_string(),
        difficulty: tag(t, "difficulty").and_then(|v| v.parse().ok()).unwrap_or(1),
        correct: result == "correct",
        reward: tag(t, "reward")?.parse().ok()?,
        cost,
        budget_left: tag(t, "budget_left")?.parse().ok()?,
        clock: tag(t, "clock")?.parse().ok()?,
        event_id: e.event_id.clone(),
        strategy: tag(t, "strategy").and_then(Strategy::parse),
        listed_cost: tag(t, "listed").and_then(|v| v.parse().ok()).unwrap_or(cost),
        skipped,
        example: if skipped { None } else { example_from_event(e) },
        rule_used: tag(t, "rule_used").map(|s| s.to_string()),
    })
}

pub fn genesis_event(g: &Genesis) -> (NewEvent, String) {
    let policy = match g.chooser {
        Chooser::Ucb => format!("choice policy ucb1 with exploration constant {}", g.explore_c),
        Chooser::Model => format!("menu world, choice policy model-ucb (learned world model, optimism beta {})", g.explore_c),
    };
    let ext = if g.ext2 {
        "; pattern families extended2 (author-defined hidden sequence families, some only expressible by composing verified building blocks, plus negative controls; strategy discovered added)"
    } else if g.extended {
        "; pattern families extended (author-defined hidden sequence families, strategy discovered added)"
    } else {
        ""
    };
    let ext = format!("{ext}{}", if g.rule_drift { "; the sequence rule of one family changes halfway through the budget and the being is not told" } else { "" });
    let ext = format!("{ext}{}", if g.transfer { "; two-phase life: from a hidden time the domains pattern_b (sequence families seen through a hidden map of the values) and control (unit conversions) are offered as well" } else { "" });
    let ext = format!("{ext}{}", if g.learn_transfer { "; the model may start a new domain from the posterior of a similar learned domain (transfer, with a guard)" } else { "" });
    let drift = if g.drift { "; the world changes at scheduled times that the being is not told about" } else { "" };
    let adapt = match g.adapt { AdaptMode::Off => "", AdaptMode::Forget => "; the model forgets old evidence exponentially", AdaptMode::Full => "; the model forgets old evidence exponentially and flags changes with a CUSUM detector", AdaptMode::Switch => "; the model replaces a cell's old evidence by its recent window when they disagree and drops a prior its outcomes reject" };
    let advice = match g.advice.kind {
        AdvisorKind::None => String::new(),
        AdvisorKind::Simulated => format!("; takes advice from a simulated advisor of quality {} and {}", g.advice.quality, if g.advice.trust { "learns how far to trust it" } else { "follows its advice blindly" }),
        AdvisorKind::Command => format!("; takes advice from the owner's advisor command and {}", if g.advice.trust { "learns how far to trust it" } else { "follows its advice blindly" }),
    };
    let text = format!(
        "being genesis: a deterministic simulation world and loop start; world_seed {} loop_seed {} budget {} energy units; {policy}{ext}{drift}{adapt}{advice}",
        g.world_seed, g.loop_seed, g.budget
    );
    let at = iso_at(0);
    let id = event_id(SOURCE, &at, &text);
    let mut tags = vec![
        "genesis".to_string(),
        format!("world_seed:{}", g.world_seed),
        format!("loop_seed:{}", g.loop_seed),
        format!("budget:{}", g.budget),
        format!("explore_c:{}", g.explore_c),
    ];
    match g.chooser {
        Chooser::Ucb => tags.push("policy:ucb1".into()),
        Chooser::Model => {
            tags.push("policy:model-ucb".into());
            tags.push("chooser:model".into());
        }
    }
    if g.ext2 {
        tags.push("pattern_families:extended2".into());
    } else if g.extended {
        tags.push("pattern_families:extended".into());
    }
    if g.rule_drift {
        tags.push("rule_drift:on".into());
    }
    if g.drift {
        tags.push("drift:on".into());
    }
    if g.transfer {
        tags.push("transfer:on".into());
    }
    if g.learn_transfer {
        tags.push("learn_transfer:on".into());
    }
    if g.adapt != AdaptMode::Off {
        tags.push(format!("adapt:{}", g.adapt.name()));
    }
    if g.advice.kind != AdvisorKind::None {
        tags.push(format!("advisor:{}", g.advice.kind.name()));
        if g.advice.kind == AdvisorKind::Simulated {
            tags.push(format!("advisor_quality:{}", g.advice.quality));
        }
        tags.push(format!("trust:{}", if g.advice.trust { "on" } else { "off" }));
    }
    let ev = NewEvent { source: SOURCE.into(), occurred_at: at, kind: "note".into(), text, tags, redacted: false };
    (ev, id)
}

/// Refill: only when the caller allows it (`stop_after_budget: false`). Logged.
pub fn refill_event(budget: i64, clock_before: i64, refills_so_far: u64) -> (NewEvent, i64) {
    let clock = ((clock_before / 86_400) + 1) * 86_400;
    let text = format!("budget refilled to {budget} energy units at the start of a new world day (refill {})", refills_so_far + 1);
    let ev = NewEvent {
        source: SOURCE.into(),
        occurred_at: iso_at(clock),
        kind: "note".into(),
        text,
        tags: vec!["refill".into(), format!("budget_left:{budget}"), format!("clock:{clock}")],
        redacted: false,
    };
    (ev, clock)
}
