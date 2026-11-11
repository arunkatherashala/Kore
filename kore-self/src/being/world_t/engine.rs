//! The World T loop: observe -> decide -> act (run a tool on a sandbox file) -> check -> record, on the
//! being's own log (source `being:worldt`).
//!
//! The being's state is folded from its log (`StateT::from_events`); every decision is a `decision`
//! event citing the evidence it used, with a revision whose rationale lists every option and its
//! estimate; the action event says what the tool did (loader, delimiter, the SQL text, the rule or form
//! used); the outcome event carries the checker's verdict, the reward, the energy and, for series tasks,
//! the revealed next value (an example for the B2 rule search). The kill switch, the step cap and the
//! energy cap are checked before every step (`Policy::gate_cost`); the controller's caps on rules and
//! search candidates are obeyed and every refusal is logged. The engine reads files only through the
//! `Sandbox` it is handed (in the tool layer: a directory under the owner's data root, every path through
//! `safe_path`); it has no file, network or process access of its own.

use super::learner::{ChoiceT, LearnerT, LearnerTCfg, LEARNER_T};
use super::tools::{self, run_plan, search_order, ts_sig, ts_terms, SqlLimits, SqlRecord, ToolCtx, ToolOut};
use super::world::*;
use crate::being::controller::{Limits, Refusal};
use crate::being::discover::{self, rule_from_event, Example, Rule};
use crate::being::policy::{Policy, StopReason};
use crate::being::state::tag;
use crate::being::world::{iso_at, SECS_PER_ENERGY};
use crate::versioned::{event_id, prediction_id, Batch, EventRow, LogError, NewEvent, NewOutcome, NewPrediction, NewRevision, VersionedLog};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const FLUSH_EVERY: usize = 25;
pub const DEFAULT_DISCOVER_EVERY: u64 = 10;
pub const CITE_LAST: usize = 5;

pub type ToolCache = HashMap<(usize, Tmpl, String, usize, Plan), ToolOut>;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, Default, PartialEq)]
pub struct Cell {
    pub n: u32,
    pub succ: u32,
    pub spent: i64,
    /// Attempts whose answer the checker rejected (a failure without an answer is not counted here).
    pub wrong: u32,
}

impl Cell {
    fn add(&mut self, ok: bool, wrong: bool, cost: i64) {
        self.n += 1;
        self.succ += ok as u32;
        self.wrong += wrong as u32;
        self.spent += cost;
    }
}

/// One step's outcome as logged (and folded).
#[derive(Debug, Clone, PartialEq)]
pub struct OutcomeT {
    pub step: u64,
    pub tmpl: Option<Tmpl>,
    pub ctx: Option<super::data::Ctx>,
    pub plan: Option<Plan>,
    /// correct | wrong | no_answer | skip | unpaid
    pub result: String,
    pub reward: i64,
    pub cost: i64,
    pub budget_left: i64,
    pub clock: i64,
    pub event_id: String,
    /// A series example (signature, the terms the search sees, the revealed next value).
    pub example: Option<(u8, Vec<i64>, i64)>,
    /// A column-rule form the search accepted on the file.
    pub form: Option<u8>,
    /// Track W2: the task's file, logged on the outcome (tag `file:`) only by a being created with
    /// `wt_file_key` on; None in every other log (whose fold then has no file counts: exactly the being before W2).
    pub file: Option<String>,
}

impl OutcomeT {
    pub fn tags(&self) -> Vec<String> {
        let mut t = vec![format!("step:{}", self.step), format!("result:{}", self.result), format!("reward:{}", self.reward), format!("cost:{}", self.cost), format!("budget_left:{}", self.budget_left), format!("clock:{}", self.clock)];
        if let Some(x) = self.tmpl {
            t.push(format!("tmpl:{}", x.name()));
        }
        if let Some(x) = self.ctx {
            t.push(format!("ctx:{}", x.name()));
        }
        if let Some(p) = self.plan {
            t.push(format!("plan:{}", p.name()));
        }
        if let Some((sig, terms, next)) = &self.example {
            t.push(format!("sig:{sig}"));
            t.push(format!("terms:{}", discover::terms_tag(terms)));
            t.push(format!("next:{next}"));
        }
        if let Some(f) = self.form {
            t.push(format!("form:{f}"));
        }
        if let Some(f) = &self.file {
            t.push(format!("file:{f}"));
        }
        t
    }
    pub fn from_event(e: &EventRow) -> Option<OutcomeT> {
        if e.kind != "outcome" || e.source != SOURCE_T {
            return None;
        }
        let g = |k| tag(&e.tags, k);
        let n = |k| g(k).and_then(|x| x.parse::<i64>().ok());
        let example = match (g("sig").and_then(|x| x.parse().ok()), g("terms"), n("next")) {
            (Some(sig), Some(ts), Some(next)) => Some((sig, ts.split(';').map(|x| x.parse().ok()).collect::<Option<Vec<i64>>>()?, next)),
            _ => None,
        };
        Some(OutcomeT {
            step: n("step")? as u64,
            tmpl: g("tmpl").and_then(Tmpl::parse),
            ctx: g("ctx").and_then(super::data::Ctx::parse),
            plan: g("plan").and_then(Plan::parse),
            result: g("result")?.to_string(),
            reward: n("reward")?,
            cost: n("cost")?,
            budget_left: n("budget_left")?,
            clock: n("clock")?,
            event_id: e.event_id.clone(),
            example,
            form: g("form").and_then(|x| x.parse().ok()),
            file: g("file").map(|x| x.to_string()),
        })
    }
}

/// The being's state in World T, folded from its log.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq)]
pub struct StateT {
    pub cfg: Option<WTCfg>,
    pub learner: LearnerTCfg,
    pub genesis: String,
    pub step: u64,
    pub clock: i64,
    pub budget_left: i64,
    pub total_reward: i64,
    pub total_cost: i64,
    pub right: u64,
    pub wrong: u64,
    pub no_answer: u64,
    pub skips: u64,
    /// (template, context, plan)
    pub cells: BTreeMap<(u8, u8, u8), Cell>,
    pub by_ctx_plan: BTreeMap<(u8, u8), Cell>,
    pub by_tmpl_plan: BTreeMap<(u8, u8), Cell>,
    pub by_plan: BTreeMap<u8, Cell>,
    /// Track W2: file -> (template, plan) -> counts, from outcomes that carry the file (`wt_file_key` on).
    pub files: BTreeMap<String, BTreeMap<(u8, u8), Cell>>,
    pub examples: Vec<Example>,
    seen_examples: BTreeSet<(u8, Vec<i64>, i64)>,
    pub rules: Vec<Rule>,
    /// Column-rule forms accepted per context (counts).
    pub forms: BTreeMap<u8, [u32; 6]>,
    pub kinds_open: Vec<Kind>,
    pub recent: Vec<String>,
    pub cell_recent: BTreeMap<(u8, u8, u8), Vec<String>>,
}

fn push_last(v: &mut Vec<String>, id: &str) {
    v.push(id.to_string());
    if v.len() > CITE_LAST {
        v.remove(0);
    }
}

impl StateT {
    pub fn start(&mut self, cfg: WTCfg, l: LearnerTCfg, genesis: String) {
        self.budget_left = cfg.budget;
        self.cfg = Some(cfg);
        self.learner = l;
        self.genesis = genesis;
    }

    pub fn apply(&mut self, o: &OutcomeT) {
        self.step = o.step + 1;
        self.clock = o.clock;
        self.budget_left = o.budget_left;
        self.total_reward += o.reward;
        self.total_cost += o.cost;
        match o.result.as_str() {
            "correct" => self.right += 1,
            "wrong" => self.wrong += 1,
            "skip" => self.skips += 1,
            _ => self.no_answer += 1,
        }
        if let (Some(t), Some(c), Some(p)) = (o.tmpl, o.ctx, o.plan) {
            if o.result != "skip" && o.result != "unpaid" {
                let ok = o.result == "correct";
                let bad = o.result == "wrong";
                let k = (t.idx() as u8, c.idx() as u8, p.idx() as u8);
                self.cells.entry(k).or_default().add(ok, bad, o.cost);
                self.by_ctx_plan.entry((k.1, k.2)).or_default().add(ok, bad, o.cost);
                self.by_tmpl_plan.entry((k.0, k.2)).or_default().add(ok, bad, o.cost);
                self.by_plan.entry(k.2).or_default().add(ok, bad, o.cost);
                if let Some(f) = &o.file {
                    self.files.entry(f.clone()).or_default().entry((k.0, k.2)).or_default().add(ok, bad, o.cost);
                }
                push_last(self.cell_recent.entry(k).or_default(), &o.event_id);
            }
            if let Some(f) = o.form {
                self.forms.entry(c.idx() as u8).or_default()[f as usize % 6] += 1;
            }
        }
        if let Some((sig, terms, next)) = &o.example {
            // the same revealed example twice is one example
            if self.seen_examples.insert((*sig, terms.clone(), *next)) {
                self.examples.push(Example { event_id: o.event_id.clone(), sig: *sig, terms: terms.clone(), next: *next });
            }
        }
        push_last(&mut self.recent, &o.event_id);
    }

    pub fn apply_rule(&mut self, r: Rule) -> bool {
        if self.rules.iter().any(|x| x.id == r.id) {
            return false;
        }
        self.rules.push(r);
        true
    }

    /// Track W2: number of (template, file, plan) keys with counts.
    pub fn file_keys(&self) -> usize {
        self.files.values().map(|m| m.len()).sum()
    }

    pub fn live_rules(&self) -> usize {
        self.rules.iter().filter(|r| !r.retired).count()
    }

    pub fn from_events(rows: &[EventRow]) -> StateT {
        let mut st = StateT::default();
        for e in rows {
            st.fold_event(e);
        }
        st
    }

    pub fn fold_event(&mut self, e: &EventRow) {
        if e.source != SOURCE_T {
            return;
        }
        let has = |t: &str| e.tags.split(',').any(|x| x == t);
        if e.kind == "note" && has("wt_genesis") && self.cfg.is_none() {
            if let (Some(c), Some(l)) = (WTCfg::from_tags(&e.tags), LearnerTCfg::from_tags(&e.tags)) {
                self.start(c, l, e.event_id.clone());
            }
        } else if e.kind == "note" && has("wt_kind") {
            if let Some(k) = tag(&e.tags, "kind").and_then(|x| KINDS.iter().copied().find(|k| k.name() == x)) {
                if !self.kinds_open.contains(&k) {
                    self.kinds_open.push(k);
                }
            }
        } else if e.kind == "outcome" {
            if let Some(o) = OutcomeT::from_event(e) {
                self.apply(&o);
            }
        } else if e.kind == "note" && has("rule") && has("worldt") {
            if let Some(r) = rule_from_event(e) {
                self.apply_rule(r);
            }
        }
    }

    /// Events cited by a decision: the genesis, the last outcomes, the last outcomes of the cell.
    pub fn evidence(&self, cell: Option<(u8, u8, u8)>) -> Vec<String> {
        let mut v = vec![self.genesis.clone()];
        v.extend(self.recent.iter().cloned());
        if let Some(c) = cell.and_then(|c| self.cell_recent.get(&c)) {
            for id in c {
                if !v.contains(id) {
                    v.push(id.clone());
                }
            }
        }
        v
    }
}

/// Where the engine reads files. The tool layer's implementation reads a sandbox directory through
/// `safe_path`; measurements and the replay may use `MemSandbox` (the generator's bytes, identical).
pub trait Sandbox {
    fn read(&self, name: &str) -> Result<Vec<u8>, String>;
}

pub struct MemSandbox<'a>(pub &'a WorldT);

impl Sandbox for MemSandbox<'_> {
    fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        self.0.file(name).map(|f| f.bytes.clone()).ok_or_else(|| format!("no file {name} in the sandbox"))
    }
}

/// A chooser: the being's learner, or a reference policy of the measurement.
pub trait ChooserT {
    fn name(&self) -> String;
    /// Measurement references that see hidden information (the oracle) get the true result of every plan.
    fn peek(&mut self, _task: &TaskT, _results: &[PlanResult]) {}
    fn choose(&mut self, st: &StateT, v: &ViewT) -> ChoiceT;
    /// What the reference policies learn from: the told reward and the energy of their last action.
    fn observe(&mut self, _v: &ViewT, _plan: Option<Plan>, _reward: i64, _cost: i64) {}
}

impl ChooserT for LearnerT {
    fn name(&self) -> String {
        LEARNER_T.into()
    }
    fn choose(&mut self, st: &StateT, v: &ViewT) -> ChoiceT {
        LearnerT::choose(self, st, v)
    }
}

/// The true result of a plan on a task (measurement only).
#[derive(Debug, Clone, PartialEq)]
pub struct PlanResult {
    pub plan: Plan,
    pub correct: bool,
    pub energy: i64,
    pub answer: Option<String>,
}

pub fn genesis_event_t(cfg: &WTCfg, l: &LearnerTCfg) -> (NewEvent, String) {
    let mut tags = vec!["wt_genesis".to_string()];
    tags.extend(cfg.tags());
    tags.extend(l.tags());
    let text = format!(
        "world T being created: seed {}, budget {} energy units, {} task kinds open at the start and one more every {} energy units, {} files per context and pool in the sandbox; learner beta {:.3}, prior weight {:.3}, {} prior, skip {}{}. Simulated data-work world with author-defined data, quirks and rewards",
        cfg.seed, cfg.budget, cfg.start_kinds, cfg.intro_every, cfg.files, l.beta_milli as f64 / 1000.0, l.prior_w_milli as f64 / 1000.0, if l.pooled { "pooled" } else { "flat" }, if l.skip { "on" } else { "off" },
        // only when on (Track W2), so that a genesis with it off is the one written before
        if l.file_key { ", file-keyed estimate on (counts per template, file and plan; prior = the cell estimate; fixed for this being's life)" } else { "" }
    );
    // only when on (Track W2b), so that every other genesis is the one written before
    let text = if l.file_key && l.file_beta_table {
        format!("{text}; beta per reward design from the Track W2b table (nearest entry in log space of wrong-answer penalty and energy weight; the table is in the tag wt_file_beta_table; fixed for this being's life)")
    } else {
        text
    };
    let ev = NewEvent { source: SOURCE_T.into(), occurred_at: iso_at(0), kind: "note".into(), text, tags, redacted: false };
    let id = event_id(&ev.source, &ev.occurred_at, &ev.text);
    (ev, id)
}

fn kind_event(k: Kind, step: u64, clock: i64) -> NewEvent {
    let tm: Vec<&str> = k.tmpls().iter().map(|t| t.name()).collect();
    NewEvent {
        source: SOURCE_T.into(),
        occurred_at: iso_at(clock),
        kind: "note".into(),
        text: format!("world T opens task kind {} (templates {}) at step {step}", k.name(), tm.join(" ")),
        tags: vec!["wt_kind".into(), format!("kind:{}", k.name()), format!("at_step:{step}")],
        redacted: false,
    }
}

pub fn refusal_event_t(r: &Refusal, step: u64, clock: i64) -> NewEvent {
    let mut ev = r.event(step, clock);
    ev.source = SOURCE_T.into();
    ev
}

pub fn stop_event_t(why: &str, step: u64, clock: i64) -> NewEvent {
    let mut ev = crate::being::controller::stop_event(why, step, clock);
    ev.source = SOURCE_T.into();
    ev
}

/// Truth of one step, for the measurement only; never written to the log.
#[derive(Debug, Clone, PartialEq)]
pub struct TrueT {
    pub step: u64,
    pub energy_before: i64,
    pub kind: Kind,
    pub tmpl: Tmpl,
    pub ctx: super::data::Ctx,
    pub plan: Option<Plan>,
    pub answered: bool,
    pub correct: bool,
    pub reward: i64,
    pub cost: i64,
    /// A reward was paid for an answer the checker rejects (must never happen).
    pub wrong_accepted: bool,
    /// Best true reward per energy any plan (or a perfect series rule) gets on this task (0: none).
    pub best_rpe: f64,
    pub rule_used: bool,
    /// An answer was submitted and the checker rejected it.
    pub wrong: bool,
}

pub struct StepT {
    pub outcome: OutcomeT,
    pub truth: TrueT,
    pub events: Vec<NewEvent>,
    pub revision: NewRevision,
    pub prediction: Option<NewPrediction>,
    pub resolved: Option<NewOutcome>,
}

pub struct NewRuleT {
    pub rule: Rule,
    pub event: NewEvent,
    pub revision: NewRevision,
}

fn fin(x: f64) -> String {
    if x.is_finite() { format!("{x:.3}") } else { "n/a".into() }
}

/// Is the hidden rule of an exact series one the B2 grammar describes (measurement only)?
fn in_grammar(w: &WorldT, t: &TaskT) -> bool {
    matches!(&w.files[t.file_idx].logical, Logical::Exact(s) if !matches!(s.rule, Some(super::data::SRule::Weekly)))
}

pub struct EngineT {
    pub world: WorldT,
    pub state: StateT,
    pub limits: Option<Limits>,
    pub refusals: Vec<Refusal>,
    pub refusals_total: Vec<Refusal>,
    refused: BTreeSet<String>,
    /// Measurement: evaluate every plan on every task (for the oracle and the regret); off in logged runs.
    pub measure: bool,
    /// Measurement: results of the deterministic plans per (file, template, parameter, prefix, plan).
    pub cache: ToolCache,
    carry: Vec<NewEvent>,
    pub max_candidates_seen: usize,
    /// Incremental B2 scan (exactly the full search, `discover_scan.rs`), and the example count of the last pass.
    pub scan: crate::being::discover_scan::ScanCache,
    last_pass_examples: Option<usize>,
    /// Timeout and row limit of every query (from the controller).
    pub sql: SqlLimits,
}

impl EngineT {
    pub fn new(state: StateT, limits: Option<Limits>) -> EngineT {
        let world = WorldT::new(state.cfg.clone().expect("a started state"));
        EngineT { world, state, limits, refusals: vec![], refusals_total: vec![], refused: BTreeSet::new(), measure: false, cache: HashMap::new(), carry: vec![], max_candidates_seen: 0, scan: Default::default(), last_pass_examples: None, sql: SqlLimits::default() }
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

    fn sync_kinds(&mut self) -> Vec<NewEvent> {
        let mut evs = vec![];
        for k in self.world.open_kinds(self.state.clock) {
            if !self.state.kinds_open.contains(&k) {
                evs.push(kind_event(k, self.state.step, self.state.clock));
                self.state.kinds_open.push(k);
            }
        }
        evs
    }

    fn tool(&mut self, plan: Plan, task: &TaskT, bytes: &[u8], order: &[u8]) -> ToolOut {
        let v = &task.view;
        let cacheable = self.measure && plan != Plan::TsRule && plan != Plan::ColSearch;
        let key = (task.file_idx, v.tmpl, v.param.clone(), v.prefix, plan);
        if cacheable {
            if let Some(o) = self.cache.get(&key) {
                return o.clone();
            }
        }
        let o = run_plan(plan, v, bytes, &ToolCtx { rules: &self.state.rules, order: order.to_vec(), sql: self.sql });
        if cacheable {
            self.cache.insert(key, o.clone());
        }
        o
    }

    fn evaluate_all(&mut self, task: &TaskT, bytes: &[u8]) -> Vec<PlanResult> {
        let mut out = vec![];
        let order: Vec<u8> = (0..6).collect();
        for p in plans_for(task.view.tmpl) {
            if *p == Plan::TsRule {
                if task.view.tmpl == Tmpl::TsNext {
                    let ok = in_grammar(&self.world, task);
                    let ans = if let Truth::Exact(s) = &task.truth { Some(s.clone()) } else { None };
                    out.push(PlanResult { plan: *p, correct: ok, energy: p.listed(&task.view), answer: if ok { ans } else { None } });
                }
                continue;
            }
            let o = self.tool(*p, task, bytes, &order);
            out.push(PlanResult { plan: *p, correct: o.answer.as_deref().map_or(false, |a| task.check(a)), energy: scale_energy(o.energy, task.view.cost_pm), answer: o.answer });
        }
        out
    }

    pub fn step(&mut self, chooser: &mut dyn ChooserT, sb: &dyn Sandbox) -> Result<StepT, StopReason> {
        let mut pre = std::mem::take(&mut self.carry);
        pre.extend(self.sync_kinds());
        let step = self.state.step;
        let t0 = self.state.clock;
        let budget0 = self.state.budget_left;
        if budget0 < SKIP_COST {
            self.carry = pre;
            return Err(StopReason::BudgetExhausted);
        }
        let task = self.world.task(step, t0);
        let v = task.view.clone();
        let file = sb.read(&v.file);
        let bytes_full = file.clone().unwrap_or_default();
        let bytes = self.world.view_bytes(&v, &bytes_full).to_vec();
        let results = if self.measure { self.evaluate_all(&task, &bytes) } else { vec![] };
        chooser.peek(&task, &results);
        let ch = chooser.choose(&self.state, &v);
        let label = ch.label();
        let cellk = ch.plan.map(|p| (v.tmpl.idx() as u8, v.ctx.idx() as u8, p.idx() as u8));
        let evidence = self.state.evidence(cellk);
        let best_rpe = results.iter().filter(|r| r.correct).map(|r| v.reward as f64 / r.energy.max(1) as f64).fold(0.0, f64::max);
        let order = search_order(&self.state.forms.get(&(v.ctx.idx() as u8)).copied().unwrap_or_default());
        // act
        let mut sqls: Vec<SqlRecord> = vec![];
        let (cost, result, answer, detail, form, rule_id): (i64, String, Option<String>, String, Option<u8>, Option<String>) = match ch.plan {
            None => (SKIP_COST.min(budget0), "skip".into(), None, format!("skip task {} (energy {})", v.id, SKIP_COST.min(budget0)), None, None),
            Some(p) => {
                let listed = p.listed(&v);
                if listed > budget0 {
                    (budget0, "unpaid".into(), None, format!("plan {} needs {listed} energy, {budget0} left: not run", p.name()), None, None)
                } else if let Err(e) = &file {
                    (listed, "no_answer".into(), None, format!("the sandbox could not give {}: {e}", v.file), None, None)
                } else if let Some(a) = &ch.oracle_answer {
                    let e = results.iter().find(|r| r.plan == p).map_or(listed, |r| r.energy);
                    (e.min(budget0), String::new(), Some(a.clone()), format!("oracle reference answers {a} with {}", p.name()), None, None)
                } else {
                    let o = self.tool(p, &task, &bytes, &order);
                    sqls = o.sql.clone();
                    let e = scale_energy(o.energy, v.cost_pm).min(budget0);
                    (e, String::new(), o.answer.clone(), o.detail.clone(), o.form, o.rule_id.clone())
                }
            }
        };
        let correct = answer.as_deref().map_or(false, |a| task.check(a));
        let result = if result.is_empty() { if answer.is_none() { "no_answer".into() } else if correct { "correct".into() } else { "wrong".into() } } else { result };
        // a wrong answer costs the world's penalty (0 by default); no answer costs nothing beyond its energy
        let reward = if correct { v.reward } else if result == "wrong" { -((v.reward * v.penalty_pm as i64 + 500) / 1000) } else { 0 };
        let t1 = t0 + cost * SECS_PER_ENERGY;
        let budget_left = budget0 - cost;
        let attempted = ch.plan.is_some() && result != "unpaid";
        let example = if attempted && matches!(v.tmpl, Tmpl::TsNext | Tmpl::TsNear) && file.is_ok() {
            task.revealed().map(|next| (ts_sig(&v), ts_terms(&tools::series_values(&bytes)), next))
        } else {
            None
        };
        let key = format!("being/worldt/choice/{step:07}");
        let dtext = format!("step {step}: task {} ({}); chose {label} by {} ({}); p_right={} sd={} est_energy={} score={}; cites {} events", v.id, v.prompt, ch.policy, ch.why, fin(ch.p), fin(ch.sd), fin(ch.energy), fin(ch.score), evidence.len());
        let dtags = vec![format!("step:{step}"), format!("action:{label}"), format!("policy:{}", ch.policy), format!("rev_key:{key}"), format!("tmpl:{}", v.tmpl.name()), format!("ctx:{}", v.ctx.name())];
        let dev = NewEvent { source: SOURCE_T.into(), occurred_at: iso_at(t0), kind: "decision".into(), text: dtext, tags: dtags, redacted: false };
        let dev_id = event_id(&dev.source, &dev.occurred_at, &dev.text);
        let mut atags = vec![format!("step:{step}"), format!("action:{label}"), format!("file:{}", v.file)];
        if let Some(r) = &rule_id {
            atags.push(format!("rule_used:{r}"));
        }
        let mut atext = format!("step {step}: {label} on {}: {}; answer {}", v.file, detail, answer.as_deref().unwrap_or("(none)"));
        for (i, q) in sqls.iter().enumerate() {
            atext.push_str(&format!("; SQL #{i} [read-only, timeout {} ms, row limit {}]: {} -> {}{}", q.limits.timeout_ms, q.limits.max_rows, q.sql, q.why, if q.digest.is_empty() { String::new() } else { format!(", result digest {}", q.digest) }));
            atags.push(format!("sql_timeout_ms:{}", q.limits.timeout_ms));
            atags.push(format!("sql_max_rows:{}", q.limits.max_rows));
            atags.push(format!("sql_status:{}", q.cap.as_deref().unwrap_or("ok")));
            if !q.digest.is_empty() {
                atags.push(format!("sql_digest:{}", q.digest));
            }
            if let Some(r) = q.refusal(step, &format!("{label} on {}", v.file)) {
                self.refuse(r);
            }
        }
        let aev = NewEvent { source: SOURCE_T.into(), occurred_at: iso_at(t0), kind: "action".into(), text: atext, tags: atags, redacted: false };
        let file_tag = self.state.learner.file_key.then(|| v.file.clone());
        let mut out = OutcomeT { step, tmpl: Some(v.tmpl), ctx: Some(v.ctx), plan: ch.plan, result: result.clone(), reward, cost, budget_left, clock: t1, event_id: String::new(), example, form, file: file_tag };
        let mut otags = out.tags();
        if let Some(a) = &answer {
            otags.push(format!("answer:{}", a.replace(',', ";")));
        }
        for q in &sqls {
            // the checker's outcome of a generated query is this event's result tag
            otags.push(format!("sql_status:{}", q.cap.as_deref().unwrap_or("ok")));
            if !q.digest.is_empty() {
                otags.push(format!("sql_digest:{}", q.digest));
            }
        }
        let rev_txt = out.example.as_ref().map_or(String::new(), |e| format!("; the next value was {}", e.2));
        let oev = NewEvent { source: SOURCE_T.into(), occurred_at: iso_at(t1), kind: "outcome".into(), text: format!("step {step}: task {} is {result}; reward {reward} energy {cost} budget left {budget_left}{rev_txt}", v.id), tags: otags, redacted: false };
        let oev_id = event_id(&oev.source, &oev.occurred_at, &oev.text);
        out.event_id = oev_id.clone();
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
        let (prediction, resolved) = if attempted {
            let pp = if ch.p.is_finite() && ch.p > 0.0 { ch.p } else { 0.5 }.clamp(0.001, 0.999);
            let pr = NewPrediction {
                made_at: iso_at(t0),
                question_id: format!("being:worldt:step:{step}"),
                target: format!("task:{}", v.id),
                options: vec!["correct".into(), "wrong".into()],
                probs: vec![pp, 1.0 - pp],
                model_id: format!("being:worldt:{}", ch.policy),
                model_version: "1".into(),
                features_cite: vec![dev_id.clone()],
                resolves_by: None,
            };
            let pred_id = prediction_id(&pr).map_err(|_| StopReason::StopFlag)?;
            let actual = if correct { "correct" } else { "wrong" };
            (Some(pr), Some(NewOutcome { pred_id, resolved_at: iso_at(t1), actual: actual.into(), outcome_event_id: Some(oev_id.clone()) }))
        } else {
            (None, None)
        };
        self.state.apply(&out);
        chooser.observe(&v, ch.plan, reward, cost);
        let truth = TrueT { step, energy_before: t0 / SECS_PER_ENERGY, kind: v.tmpl.kind(), tmpl: v.tmpl, ctx: v.ctx, plan: ch.plan, answered: answer.is_some(), correct, reward, cost, wrong_accepted: reward > 0 && !answer.as_deref().map_or(false, |a| task.check(a)), best_rpe, rule_used: rule_id.is_some(), wrong: result == "wrong" };
        let mut events = pre;
        events.extend([dev, aev, oev]);
        Ok(StepT { outcome: out, truth, events, revision, prediction, resolved })
    }

    /// One B2 discovery pass over the being's revealed series examples, with the controller's caps.
    pub fn discover_pass(&mut self) -> Vec<NewRuleT> {
        let max_c = self.limits.as_ref().map_or(usize::MAX, |l| l.max_candidates);
        // no new example since the last pass: the search would return the same rules, all already taken
        if self.last_pass_examples == Some(self.state.examples.len()) {
            return vec![];
        }
        self.last_pass_examples = Some(self.state.examples.len());
        let o = self.scan.search(&self.state.examples, discover::K_TRAIN, discover::K_VAL, &[], max_c, &[]);
        if o.truncated {
            self.refuse(Refusal::new("max_search_candidates", max_c as u64, o.wanted as u64, format!("a discovery pass would try {} candidates; it stopped after {}", o.wanted, o.candidates)));
        }
        self.max_candidates_seen = self.max_candidates_seen.max(o.candidates);
        let mut out = vec![];
        for f in o.found {
            let mut rule = Rule::new(f.sig, f.form, f.train_ok, f.val_ok, self.state.step);
            if self.state.rules.iter().any(|r| r.id == rule.id) {
                continue;
            }
            if let Some(l) = &self.limits {
                let live = self.state.live_rules();
                if live >= l.max_live_rules {
                    self.refuse(Refusal::new("max_live_rules", l.max_live_rules as u64, live as u64 + 1, format!("rule {} ({}) for signature {} is verified but was not added: {live} rules are live", rule.id, f.form.code(), f.sig)));
                    continue;
                }
            }
            let at = iso_at(self.state.clock);
            let ctx = super::data::CTXS.get((f.sig as usize).saturating_sub(100) % 20).map_or("?", |c| c.name());
            let text = format!(
                "world T rule {} for {ctx} series (signature {}): {}; coefficients fitted per task from its own prefix; reproduced the revealed next value exactly on {} training and {} held-out logged examples and was never applicable-and-wrong on them (search inside the author-defined grammar of discover.rs)",
                rule.id, f.sig, f.form.dsl(), f.train_ok, f.val_ok
            );
            let tags = vec!["rule".to_string(), "worldt".into(), format!("rule_id:{}", rule.id), format!("sig:{}", rule.sig), format!("form:{}", rule.form.code()), format!("train:{}", rule.train_ok), format!("val:{}", rule.val_ok), format!("at_step:{}", rule.step)];
            let event = NewEvent { source: SOURCE_T.into(), occurred_at: at.clone(), kind: "note".into(), text, tags, redacted: false };
            let ev_id = event_id(&event.source, &event.occurred_at, &event.text);
            rule.event_id = ev_id.clone();
            let mut caused_by = f.cite.clone();
            caused_by.push(ev_id);
            let revision = NewRevision {
                entity_type: "rule".into(),
                key: format!("being/worldt/rule/{}", rule.id),
                stance: format!("{ctx} series: {}", rule.form.dsl()),
                confidence: ((f.train_ok + f.val_ok + 1) as f64 / (f.train_ok + f.val_ok + 2) as f64).clamp(0.0, 1.0),
                rationale: json!({ "rule_id": rule.id, "signature": f.sig, "form": rule.form.code(), "dsl": rule.form.dsl(), "training_examples_reproduced": f.train_ok, "held_out_examples_reproduced": f.val_ok, "cited_examples": f.cite.len(), "accepted_at_step": self.state.step }).to_string(),
                supersedes: None,
                valid_from: at,
                author: "rule:being-discover".into(),
                caused_by,
            };
            self.state.apply_rule(rule.clone());
            out.push(NewRuleT { rule, event, revision });
        }
        out
    }
}

// ---- the logged run ----

#[derive(Debug, Clone, Default)]
pub struct RunTCfg {
    pub world: Option<WTCfg>,
    pub learner: Option<LearnerTCfg>,
    pub discover_every: u64,
    pub ctl: Option<Limits>,
    /// Query limits (None: the defaults).
    pub sql: Option<SqlLimits>,
}

#[derive(Debug, Clone)]
pub struct RunTSummary {
    pub steps_run: u64,
    pub stopped: StopReason,
    pub reward: i64,
    pub cost: i64,
    pub state: StateT,
    pub created: bool,
    pub new_rules: Vec<Rule>,
    pub refusals: Vec<Refusal>,
    pub truths: Vec<TrueT>,
    pub version_before: u64,
    pub version_after: u64,
    /// Track D1b: checkpoint restored at the start and written at the end (all empty when off).
    pub ckpt: crate::being::ckpt::CkptReport,
}

/// Run the World T being on its own log (or `chooser`, for references and tests), reading files from `sb`.
pub fn run_t(log: &mut VersionedLog, cfg: &RunTCfg, policy: &Policy, chooser: Option<&mut dyn ChooserT>, sb: &dyn Sandbox) -> Result<RunTSummary, LogError> {
    run_t_key(log, cfg, None, policy, chooser, sb)
}

/// As `run_t`, with the Track W2 controller key `wt_file_key` (None: the key is not in controller.txt).
/// A new being is created with it (None: `cfg.learner`'s setting, off by default) and the setting is recorded
/// at its genesis. An existing being keeps the setting of its genesis (like every learner constant,
/// `cfg.learner` is used only at creation); a run that asks for the other setting is refused with an error,
/// never silently changed.
pub fn run_t_key(log: &mut VersionedLog, cfg: &RunTCfg, file_key: Option<bool>, policy: &Policy, chooser: Option<&mut dyn ChooserT>, sb: &dyn Sandbox) -> Result<RunTSummary, LogError> {
    run_t_keys(log, cfg, file_key, None, policy, chooser, sb)
}

/// As `run_t_key`, with the Track W2b controller key `wt_file_beta` (Some(true) = `table`, Some(false) = `fixed`,
/// None: no request; the tool passes it only when `wt_file_key = on`). A new being is created with it (only
/// together with the file key; without it the request has no effect) and it is recorded at its genesis with the
/// whole table. An existing being keeps its genesis setting; a run asking for the other one is refused.
pub fn run_t_keys(log: &mut VersionedLog, cfg: &RunTCfg, file_key: Option<bool>, file_beta: Option<bool>, policy: &Policy, chooser: Option<&mut dyn ChooserT>, sb: &dyn Sandbox) -> Result<RunTSummary, LogError> {
    let version_before = log.latest_version();
    // Track D1b: with checkpoints off this is exactly the old full fold of the snapshot
    let (mut state, restored) = if policy.checkpoint_every == 0 {
        let rows: Vec<EventRow> = log.open_latest().rows();
        (StateT::from_events(&rows), Default::default())
    } else {
        crate::being::ckpt::restore::<StateT>(log)?
    };
    let ckpt_base = (policy.checkpoint_every > 0).then(|| (state.clone(), version_before));
    let mut created = false;
    match &state.cfg {
        Some(c) => {
            if let Some(w) = &cfg.world {
                if w != c {
                    return Err(LogError::Invalid(format!("this World T being already lives in world {c:?}; the request says {w:?}. A being keeps one world")));
                }
            }
            if let Some(fk) = file_key {
                if fk != state.learner.file_key {
                    let on = |b: bool| if b { "on" } else { "off" };
                    return Err(LogError::Invalid(format!("this World T being was created with wt_file_key = {} (recorded at its genesis); the request says {}. The setting is fixed for the being's life", on(state.learner.file_key), on(fk))));
                }
            }
            if let Some(fb) = file_beta {
                if fb != state.learner.file_beta_table {
                    let nm = |b: bool| if b { "table" } else { "fixed" };
                    return Err(LogError::Invalid(format!("this World T being was created with wt_file_beta = {} (recorded at its genesis); the request says {}. The setting is fixed for the being's life", nm(state.learner.file_beta_table), nm(fb))));
                }
            }
        }
        None => {
            let w = cfg.world.clone().ok_or_else(|| LogError::Invalid("no World T being exists yet; a world config is needed to create one".into()))?;
            w.validate().map_err(LogError::Invalid)?;
            let mut l = cfg.learner.unwrap_or_default();
            if let Some(fk) = file_key {
                l.file_key = fk;
            }
            if let Some(fb) = file_beta {
                l.file_beta_table = fb;
            }
            // the beta table exists only with the file key
            l.file_beta_table &= l.file_key;
            if l.beta_milli > 10_000 || l.prior_w_milli > 100_000 {
                return Err(LogError::Invalid("learner constants out of range".into()));
            }
            let (ev, id) = genesis_event_t(&w, &l);
            log.append_events(&[ev])?;
            state.start(w, l, id);
            created = true;
        }
    }
    let mut own = LearnerT { cfg: state.learner };
    let chooser: &mut dyn ChooserT = match chooser {
        Some(c) => c,
        None => &mut own,
    };
    let mut engine = EngineT::new(state, cfg.ctl.clone());
    engine.sql = cfg.sql.unwrap_or_default();
    let (mut new_rules, mut truths) = (vec![], vec![]);
    let mut pending = Batch::default();
    let mut pending_steps = 0usize;
    let (mut done, mut rew, mut cost) = (0u64, 0i64, 0i64);
    let stopped;
    loop {
        if let Some(r) = policy.gate_cost(done, cost) {
            stopped = r;
            if matches!(r, StopReason::Killed | StopReason::EnergyCap) {
                pending.events.push(stop_event_t(r.as_str(), engine.state.step, engine.state.clock));
                if r == StopReason::EnergyCap {
                    engine.refuse(Refusal::new("max_energy_per_run", policy.max_energy.unwrap_or(0).max(0) as u64, cost.max(0) as u64, format!("the run spent {cost} energy units and may not start another step")));
                }
            }
            break;
        }
        match engine.step(chooser, sb) {
            Ok(out) => {
                done += 1;
                rew += out.outcome.reward;
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
                    pending.events.push(refusal_event_t(&r, engine.state.step, engine.state.clock));
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
        pending.events.push(refusal_event_t(&r, engine.state.step, engine.state.clock));
    }
    pending.events.extend(std::mem::take(&mut engine.carry));
    if !pending.events.is_empty() {
        log.commit_batch(pending)?;
    }
    let ckpt = crate::being::ckpt::write_after_run(log, policy.checkpoint_every, ckpt_base, restored);
    Ok(RunTSummary { steps_run: done, stopped, reward: rew, cost, state: engine.state, created, new_rules, refusals: engine.refusals_total, truths, version_before, version_after: log.latest_version(), ckpt })
}

/// Re-derive every learner decision of a World T log from the events before it and the world rebuilt
/// from the logged config. Returns (decisions checked, mismatches).
pub fn replay_t(rows: &[EventRow]) -> (u64, u64) {
    let mut st = StateT::default();
    let mut world: Option<WorldT> = None;
    let (mut n, mut bad) = (0u64, 0u64);
    for e in rows.iter().filter(|e| e.source == SOURCE_T) {
        if e.kind == "decision" && tag(&e.tags, "policy") == Some(LEARNER_T) {
            if world.is_none() {
                world = st.cfg.clone().map(WorldT::new);
            }
            if let Some(w) = &world {
                let v = w.task(st.step, st.clock).view;
                let got = LearnerT { cfg: st.learner }.choose(&st, &v).label();
                n += 1;
                if Some(got.as_str()) != tag(&e.tags, "action") {
                    bad += 1;
                }
            }
        }
        st.fold_event(e);
    }
    (n, bad)
}
