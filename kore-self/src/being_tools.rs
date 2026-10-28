//! Tools of the being loop: being_run, being_status, being_explain, being_rules.
//!
//! The loop runs in-process, only against the simulated world and this owner's
//! own log (see `being/policy.rs`); no network, no files other than the log.
//! The owner directory holds the being's log. It is a deterministic simulation
//! with author-defined rewards, measured in docs/measured.md.

use crate::authorize::{self, Outcome, BEING_ACTOR};
use crate::being::advice::{ev_kind, Advisor, AdvisorKind, SimulatedAdvisor, ADVISOR_SOURCE};
use crate::being::replay::{rederive_rules, replay_decisions};
use crate::being::run_loop::{domain_json, run_advised, trust_json, world_for, RunCfg};
use crate::being::solver::{STRATEGIES, STRATEGIES_EXT};
use crate::being::state::{tag, BeingState, Chooser, Genesis, SOURCE};
use crate::being::{self, Policy, ScriptedThinker};
use crate::log_tools::{err, log_err, open, req_str, text};
use crate::versioned::{CauseRow, EventRow, RevisionRow};
use serde_json::{json, Value};
use std::collections::HashMap;

pub const TOOLS: [&str; 4] = ["being_run", "being_status", "being_explain", "being_rules"];
const MAX_STEPS: u64 = 5000;

pub fn handle(name: &str, args: &Value, owner: &str) -> Result<Value, Value> {
    match name {
        "being_run" => run(args, owner),
        "being_status" => status(owner),
        "being_explain" => explain(args, owner),
        "being_rules" => rules(owner),
        _ => Err(err("unknown tool")),
    }
}

fn opt_u64(args: &Value, k: &str) -> Result<Option<u64>, Value> {
    match args.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| err(format!("'{k}' must be a non-negative integer"))),
    }
}

fn run(args: &Value, owner: &str) -> Result<Value, Value> {
    let steps = opt_u64(args, "steps")?.ok_or_else(|| err("'steps' is required (integer 1..5000)"))?;
    if !(1..=MAX_STEPS).contains(&steps) {
        return Err(err(format!("'steps' must be 1..{MAX_STEPS}")));
    }
    let stop_after_budget = match args.get("stop_after_budget") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(err("'stop_after_budget' must be a boolean")),
    };
    let budget = match opt_u64(args, "budget")? {
        None => None,
        Some(b) if (1..=1_000_000).contains(&b) => Some(b as i64),
        Some(_) => return Err(err("'budget' must be 1..1000000 energy units")),
    };
    let chooser = match args.get("chooser") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(Chooser::parse(s).ok_or_else(|| err("'chooser' must be \"ucb\" or \"model\""))?),
        Some(_) => return Err(err("'chooser' must be \"ucb\" or \"model\"")),
    };
    let extended = match args.get("pattern_families") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s == "standard" => Some(false),
        Some(Value::String(s)) if s == "extended" => Some(true),
        Some(_) => return Err(err("'pattern_families' must be \"standard\" or \"extended\"")),
    };
    let discover = match args.get("discover") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(err("'discover' must be a boolean")),
    };
    let every = opt_u64(args, "discover_every")?;
    if every.is_some() && !discover {
        return Err(err("'discover_every' only applies with discover: true"));
    }
    let discover_every = match (discover, every) {
        (false, _) => None,
        (true, None) => Some(10),
        (true, Some(n)) if (1..=1000).contains(&n) => Some(n),
        (true, Some(_)) => return Err(err("'discover_every' must be 1..1000 steps")),
    };
    let flag = |k: &str| -> Result<Option<bool>, Value> {
        match args.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(err(format!("'{k}' must be a boolean"))),
        }
    };
    let advisor_kind = match args.get("advisor") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(AdvisorKind::parse(s).ok_or_else(|| err("'advisor' must be \"none\", \"simulated\" or \"command\""))?),
        Some(_) => return Err(err("'advisor' must be \"none\", \"simulated\" or \"command\"")),
    };
    let advisor_quality = match args.get("advisor_quality") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_f64().filter(|q| (0.0..=1.0).contains(q)).ok_or_else(|| err("'advisor_quality' must be a number within 0..1"))?),
    };
    let cfg = RunCfg {
        advisor: advisor_kind,
        advisor_quality,
        trust: flag("trust")?,
        drift: flag("drift")?,
        adapt: flag("adapt")?,
        extended,
        discover_every,
        chooser,
        world_seed: opt_u64(args, "world_seed")?,
        loop_seed: opt_u64(args, "seed")?,
        budget,
        explore_c: None,
        steps,
        refill_when_exhausted: !stop_after_budget,
    };
    // which advisor will be asked: the one the being already has, else the one requested for a new being
    let existing = {
        let (_, rows) = load(owner)?;
        BeingState::from_events(&rows).genesis
    };
    let kind = existing.as_ref().map_or(cfg.advisor.unwrap_or(AdvisorKind::None), |g| g.advice.kind);
    let advisor: Option<Box<dyn Advisor>> = if kind == AdvisorKind::None {
        None
    } else {
        // every run that asks an advisor is authorized (and audited) first, BEFORE the log is opened: default deny
        let d = authorize::authorize(owner, BEING_ACTOR, "advisor_query", &json!({ "advisor": kind.name(), "steps": steps }));
        if d.outcome != Outcome::Allow {
            return Err(err(format!("advisor_query was not allowed ({}): {}. The being will not ask its advisor. To allow it, list advisor_query under [L1] in autonomy_policy.txt (edit that file by hand)", d.outcome.name(), d.reason)));
        }
        Some(make_advisor(kind, &cfg, existing.as_ref(), owner)?)
    };
    let mut log = open(owner)?;
    let mut policy = Policy::new(steps);
    policy.refill_when_exhausted = cfg.refill_when_exhausted;
    let s = run_advised(&mut log, &cfg, &ScriptedThinker, &policy, advisor.as_deref()).map_err(log_err)?;
    let mut out = s.to_json();
    out["note"] = json!("deterministic simulation loop: a seeded world with author-defined rewards and a scripted solver; the preference comes from those rewards. See docs/measured.md");
    Ok(text(out))
}

/// The advisor object for a being: simulated (seeded by the being's loop seed, truth supplied by the
/// measurement helpers) or the owner's command.
fn make_advisor(kind: AdvisorKind, cfg: &RunCfg, existing: Option<&Genesis>, owner: &str) -> Result<Box<dyn Advisor>, Value> {
    match kind {
        AdvisorKind::None => Err(err("no advisor")),
        AdvisorKind::Command => {
            let c = crate::chat::advisor_cmd::load_config(owner).map_err(|e| err(format!("advisor.txt problem, nothing was run: {e}")))?;
            let a = crate::chat::advisor_cmd::CommandAdvisor::new(c, owner).map_err(err)?;
            Ok(Box::new(a))
        }
        AdvisorKind::Simulated => {
            let (world_seed, loop_seed, extended, drift, budget, q) = match existing {
                Some(g) => (g.world_seed, g.loop_seed, g.extended, g.drift, g.budget, g.advice.quality),
                None => (cfg.world_seed.unwrap_or(1), cfg.loop_seed.unwrap_or(1), cfg.extended.unwrap_or(false), cfg.drift.unwrap_or(false), cfg.budget.unwrap_or(being::run_loop::DEFAULT_BUDGET), cfg.advisor_quality.unwrap_or(0.0)),
            };
            let g = Genesis { world_seed, loop_seed, budget, explore_c: 0.0, chooser: Chooser::Model, extended, drift, adapt: Default::default(), advice: Default::default() };
            let w = world_for(&g);
            let strategies: Vec<being::solver::Strategy> = if extended { STRATEGIES_EXT.to_vec() } else { STRATEGIES.to_vec() };
            let ranker = being::measure::truth_ranker(being::measure::truth_in(&w, &[]), strategies.iter().copied().filter(|s| *s != being::solver::Strategy::Discovered).collect());
            let mut a = SimulatedAdvisor::new(loop_seed, q, ranker);
            if extended {
                a = a.with_rules(being::measure::rule_table());
            }
            Ok(Box::new(a))
        }
    }
}

fn rules(owner: &str) -> Result<Value, Value> {
    let (log, rows) = load(owner)?;
    let st = BeingState::from_events(&rows);
    let Some(g) = st.genesis.clone() else {
        return Ok(text(json!({ "exists": false, "note": "no being loop has run in this owner directory yet; call being_run" })));
    };
    let snap = log.open_latest();
    let revs: HashMap<String, RevisionRow> = snap.rows::<RevisionRow>().into_iter().map(|r| (r.key.clone(), r)).collect();
    let causes: Vec<CauseRow> = snap.rows();
    let events: HashMap<String, &EventRow> = rows.iter().map(|e| (e.event_id.clone(), e)).collect();
    let re = rederive_rules(&rows);
    let list: Vec<Value> = st
        .rules
        .iter()
        .map(|r| {
            let rev = revs.get(&format!("being/rule/{}", r.id));
            let cited: Vec<&EventRow> = rev.map(|v| causes.iter().filter(|c| c.rev_id == v.rev_id).filter_map(|c| events.get(&c.event_id).copied()).filter(|e| e.kind == "outcome").collect()).unwrap_or_default();
            let used = rows.iter().filter(|e| e.source == SOURCE && e.kind == "action" && tag(&e.tags, "rule_used") == Some(r.id.as_str())).count();
            json!({
                "rule_id": r.id, "signature": format!("pattern difficulty {}", r.sig), "form": r.form.code(), "dsl": r.form.dsl(),
                "training_examples_reproduced": r.train_ok, "held_out_examples_reproduced": r.val_ok, "accepted_at_step": r.step,
                "rule_event_id": r.event_id, "revision": rev.map(|v| json!({ "rev_id": v.rev_id, "key": v.key, "confidence": v.confidence, "author": v.author })),
                "cited_example_events": cited.len(),
                "cited_sample": cited.iter().take(3).map(|e| json!({ "event_id": e.event_id, "text": e.text, "terms": tag(&e.tags, "terms"), "revealed_next": tag(&e.tags, "next") })).collect::<Vec<_>>(),
                "times_used_in_attempts": used,
                "rederived_from_log": re.iter().find(|(id, _)| *id == r.id).map(|x| x.1),
            })
        })
        .collect();
    Ok(text(json!({
        "exists": true, "pattern_families": if g.extended { "extended" } else { "standard" }, "revealed_examples_in_log": st.examples.len(),
        "rules": list,
        "note": "rules are found by a search inside a small grammar written by the author (see README, Track B2); the coefficients are fitted per task from its own prefix. 'rederived_from_log' re-runs the search over only the examples logged before the rule and checks that it yields this rule",
    })))
}

fn load(owner: &str) -> Result<(crate::versioned::VersionedLog, Vec<EventRow>), Value> {
    let log = open(owner)?;
    let rows: Vec<EventRow> = log.open_latest().rows();
    Ok((log, rows))
}

fn status(owner: &str) -> Result<Value, Value> {
    let (log, rows) = load(owner)?;
    let st = BeingState::from_events(&rows);
    let Some(g) = st.genesis.clone() else {
        return Ok(text(json!({ "exists": false, "note": "no being loop has run in this owner directory yet; call being_run" })));
    };
    let snap = log.open_latest();
    let revs: HashMap<String, RevisionRow> = snap.rows::<RevisionRow>().into_iter().map(|r| (r.key.clone(), r)).collect();
    let decisions: Vec<&EventRow> = rows.iter().filter(|e| e.source == SOURCE && e.kind == "decision").collect();
    let last: Vec<Value> = decisions
        .iter()
        .rev()
        .take(5)
        .map(|e| {
            let rationale = tag(&e.tags, "rev_key").and_then(|k| revs.get(k)).and_then(|r| serde_json::from_str::<Value>(&r.rationale).ok());
            json!({ "decision_event_id": e.event_id, "occurred_at": e.occurred_at, "step": tag(&e.tags, "step"), "domain": tag(&e.tags, "domain"), "action": tag(&e.tags, "action"), "text": e.text, "reasons": rationale })
        })
        .collect();
    let rep = replay_decisions(&rows);
    let ok = rep.iter().filter(|r| r.matches()).count();
    Ok(text(json!({
        "exists": true,
        "world_seed": g.world_seed, "loop_seed": g.loop_seed, "initial_budget": g.budget, "explore_c": g.explore_c, "chooser": g.chooser.name(),
        "drift": g.drift, "adapt": g.adapt.name(), "changes_detected": changes_json(&rows, &revs, &snap.rows::<CauseRow>()),
        "advisor": g.advice.kind.name(), "advisor_quality": if g.advice.kind == AdvisorKind::Simulated { json!(g.advice.quality) } else { Value::Null },
        "trust_learning": g.advice.kind != AdvisorKind::None && g.advice.trust,
        "advice": advice_status(&st, &rows),
        "model_attempts": st.model.attempts(), "pattern_families": if g.extended { "extended" } else { "standard" }, "rules_discovered": st.rules.len(),
        "steps": st.step, "budget_left": st.budget_left, "refills": st.refills, "world_clock_seconds": st.clock,
        "total_reward": st.total_reward, "total_energy_spent": st.total_cost,
        "reward_per_energy": if st.total_cost > 0 { st.total_reward as f64 / st.total_cost as f64 } else { 0.0 },
        "domains": domain_json(&st),
        "last_decisions": last,
        "replay_check": { "decisions": rep.len(), "re_derived_from_log_and_equal": ok },
        "note": "deterministic simulation; preferences are produced by the defined rewards",
    })))
}

/// What the log says about advice: trust per (advisor, kind), counts, degradations and the latest advice records.
fn advice_status(st: &BeingState, rows: &[EventRow]) -> Value {
    let ev: Vec<&EventRow> = rows.iter().filter(|e| e.source == ADVISOR_SOURCE).collect();
    if ev.is_empty() {
        return json!({ "advice_events": 0 });
    }
    let n = |k: &str| ev.iter().filter(|e| ev_kind(&e.tags) == Some(k)).count();
    let latest: Vec<Value> = ev.iter().rev().filter(|e| matches!(ev_kind(&e.tags), Some("result" | "verdict" | "alarm"))).take(3).map(|e| json!({ "event_id": e.event_id, "kind": ev_kind(&e.tags), "text": e.text })).collect();
    json!({
        "advice_events": n("advice"), "results_logged": n("result"), "rule_verdicts": n("verdict"), "degradations_flagged": n("alarm"),
        "failed_requests": ev.iter().filter(|e| ev_kind(&e.tags) == Some("advice") && tag(&e.tags, "error").is_some()).count(),
        "trust": trust_json(st),
        "trust_note": "trust = Beta posterior of 'advice followed against the being's own model beat what the model alone predicted'; it is folded from the log, never stored elsewhere",
        "latest": latest,
    })
}

/// The changes the being's detector flagged: the logged note, its revision and how many outcome events it cites.
fn changes_json(rows: &[EventRow], revs: &HashMap<String, RevisionRow>, causes: &[CauseRow]) -> Vec<Value> {
    rows.iter()
        .filter(|e| e.source == SOURCE && e.kind == "note" && e.tags.split(',').any(|t| t == "change"))
        .map(|e| {
            let rev = tag(&e.tags, "rev_key").and_then(|k| revs.get(k));
            json!({
                "change_event_id": e.event_id, "occurred_at": e.occurred_at, "step": tag(&e.tags, "step"), "domain": tag(&e.tags, "domain"),
                "strategy": tag(&e.tags, "strategy"), "direction": tag(&e.tags, "direction"), "text": e.text,
                "revision": rev.map(|r| json!({ "rev_id": r.rev_id, "key": r.key, "stance": r.stance, "author": r.author, "confidence": r.confidence })),
                "evidence_events_cited": rev.map(|r| causes.iter().filter(|c| c.rev_id == r.rev_id).count().saturating_sub(1)),
            })
        })
        .collect()
}

/// The advisor events of one step: the advice (request and reply), any verdict on a proposed rule, and what happened afterwards.
fn advice_for_step(rows: &[EventRow], step: Option<&str>) -> Value {
    let evs: Vec<Value> = rows
        .iter()
        .filter(|e| e.source == ADVISOR_SOURCE && tag(&e.tags, "step") == step && step.is_some())
        .map(|e| json!({ "event_id": e.event_id, "kind": ev_kind(&e.tags), "text": crate::safety::trunc(&e.text, 1500) }))
        .collect();
    json!(evs)
}

fn explain(args: &Value, owner: &str) -> Result<Value, Value> {
    let id = req_str(args, "decision_event_id")?;
    if id.len() < 8 {
        return Err(err("'decision_event_id' must be at least 8 characters"));
    }
    let (log, rows) = load(owner)?;
    let found: Vec<&EventRow> = rows.iter().filter(|e| e.source == SOURCE && e.kind == "decision" && e.event_id.starts_with(id)).collect();
    let d = match found.as_slice() {
        [] => return Err(err(format!("not found: decision event {id}"))),
        [one] => *one,
        _ => return Err(err(format!("'{id}' matches {} decisions; give more characters", found.len()))),
    };
    let snap = log.open_latest();
    let key = tag(&d.tags, "rev_key").unwrap_or("");
    let rev = snap.rows::<RevisionRow>().into_iter().find(|r| r.key == key);
    let events: HashMap<String, EventRow> = rows.iter().map(|e| (e.event_id.clone(), e.clone())).collect();
    let (mut cited, mut reasons) = (Vec::new(), Value::Null);
    if let Some(r) = &rev {
        reasons = serde_json::from_str(&r.rationale).unwrap_or(Value::Null);
        let mut c: Vec<&EventRow> = snap.rows::<CauseRow>().iter().filter(|c| c.rev_id == r.rev_id).filter_map(|c| events.get(&c.event_id)).collect();
        c.sort_by(|a, b| (&a.occurred_at, &a.event_id).cmp(&(&b.occurred_at, &b.event_id)));
        cited = c.iter().take(40).map(|e| json!({ "event_id": e.event_id, "occurred_at": e.occurred_at, "kind": e.kind, "text": e.text })).collect();
    }
    let step = tag(&d.tags, "step");
    let reasons = reasons.clone();
    let result = rows.iter().find(|e| e.source == SOURCE && e.kind == "outcome" && tag(&e.tags, "step") == step).map(|e| json!({ "event_id": e.event_id, "text": e.text }));
    let rep = replay_decisions(&rows).into_iter().find(|r| r.event_id == d.event_id);
    // the discovered rule that produced the answer of this step, if one did
    let rule_used = rows.iter().find(|e| e.source == SOURCE && e.kind == "action" && tag(&e.tags, "step") == step).and_then(|a| tag(&a.tags, "rule_used")).map(|id| {
        let rev = snap.rows::<RevisionRow>().into_iter().find(|r| r.key == format!("being/rule/{id}"));
        json!({
            "rule_id": id,
            "stance": rev.as_ref().map(|r| r.stance.clone()),
            "rationale": rev.as_ref().and_then(|r| serde_json::from_str::<Value>(&r.rationale).ok()),
            "note": "the answer of this step was computed by this rule from the log; the rule's cited example events are listed by being_rules",
        })
    });
    let changes_before = {
        let revs: HashMap<String, RevisionRow> = snap.rows::<RevisionRow>().into_iter().map(|r| (r.key.clone(), r)).collect();
        let upto: u64 = step.and_then(|s| s.parse().ok()).unwrap_or(0);
        let all = changes_json(&rows, &revs, &snap.rows::<CauseRow>());
        let before: Vec<Value> = all.into_iter().filter(|c| c["step"].as_str().and_then(|s| s.parse::<u64>().ok()).map_or(false, |s| s < upto)).collect();
        json!({ "count": before.len(), "latest": before.iter().rev().take(3).collect::<Vec<_>>() })
    };
    Ok(text(json!({
        "decision_event_id": d.event_id, "occurred_at": d.occurred_at, "step": step, "domain": tag(&d.tags, "domain"), "action": tag(&d.tags, "action"),
        "decision": d.text,
        "revision": rev.as_ref().map(|r| json!({ "rev_id": r.rev_id, "key": r.key, "stance": r.stance, "author": r.author })),
        "reasons_and_alternatives": reasons,
        "evidence_events_cited": cited,
        "evidence_note": "the cited events are the genesis event and the latest outcomes per domain; the counts and means used come from all outcomes before this step",
        "outcome_of_this_step": result,
        "discovered_rule_used": rule_used,
        "advice_for_this_step": advice_for_step(&rows, step),
        "how_advice_changed_the_decision": reasons.get("advice").cloned(),
        "changes_flagged_before_this_decision": changes_before,
        "replay_check": rep.map(|r| json!({ "re_derived_domain": r.rederived, "equal_to_recorded": r.matches() })),
    })))
}

pub fn tool_defs() -> Vec<Value> {
    vec![
        json!({ "name": "being_run",
          "description": "Run the being loop for N steps in a deterministic simulated world (arithmetic, algebra, physics formulas, sequences) with a seeded budget; the being picks the domain by learned reward per energy (or, with chooser model, picks one of 4 offered tasks and a solver strategy from a world model learned from its own log) and every decision, action and outcome is appended to its own log. A simulation with author-defined rewards, not a real-world agent",
          "inputSchema": { "type": "object", "properties": {
            "steps": { "type": "integer", "minimum": 1, "maximum": 5000, "description": "Maximum steps to run in this call" },
            "seed": { "type": "integer", "minimum": 0, "description": "Loop seed (order of first exploration). Default 1; fixed once the being exists" },
            "world_seed": { "type": "integer", "minimum": 0, "description": "World seed (hidden rewards and tasks). Default 1; fixed once the being exists" },
            "budget": { "type": "integer", "minimum": 1, "description": "Energy units for a new being. Default 2000" },
            "chooser": { "type": "string", "enum": ["ucb", "model"], "description": "Default ucb (single task per step, UCB1 over domains). model: a menu of 4 tasks per step; a world model learned from the log picks task and solver strategy. Fixed once the being exists" },
            "pattern_families": { "type": "string", "enum": ["standard", "extended"], "description": "Default standard. extended (needs chooser model): sequence tasks also come from extra hidden families the fixed solvers cannot solve, revealed answers are logged and a fourth strategy, discovered, applies rules the being found itself. Fixed once the being exists" },
            "drift": { "type": "boolean", "description": "Default false. true (needs chooser model): the world changes at scheduled times the being is not told about (a new domain, units, appears in the menu; one domain's rewards and costs shift; the fast strategy gets much less reliable). The times scale with the initial budget. Fixed once the being exists" },
            "adapt": { "type": "boolean", "description": "Default false. true (needs chooser model): the world model discounts old evidence exponentially and a detector flags changes in the success rate of a (domain, strategy) group; every flagged change is logged with the outcomes it rests on and shown by being_status. Fixed once the being exists" },
            "discover": { "type": "boolean", "description": "Run rule discovery during this run (needs a being created with pattern_families extended): every discover_every steps the being searches a small author-defined grammar of sequence rules over its own revealed examples and logs accepted rules" },
            "discover_every": { "type": "integer", "minimum": 1, "maximum": 1000, "description": "With discover: steps between discovery passes. Default 10" },
            "advisor": { "type": "string", "enum": ["none", "simulated", "command"], "description": "Default none (needs chooser model). simulated: a deterministic simulated advisor of quality advisor_quality advises each step; command: the program in advisor.txt (hand-edited by the owner) is asked. Advice is ONE input to the planner, never executed blindly; proposed rules must pass the verified search. Every run that asks an advisor is authorized (tool advisor_query, default deny, list it under [L1]) and audited. Fixed once the being exists" },
            "advisor_quality": { "type": "number", "minimum": 0, "maximum": 1, "description": "Simulated advisor only, required with it: probability that its suggestion is the oracle-correct one. A property of the simulation, not of any real model. Fixed once the being exists" },
            "trust": { "type": "boolean", "description": "With an advisor, default true: the being learns from logged outcomes how far to trust the advisor per domain and mixes advice with its own model by that trust. false: it follows advice blindly. Fixed once the being exists" },
            "stop_after_budget": { "type": "boolean", "description": "Default true: stop when no task is affordable. False: the world refills the budget at the next world day (logged) and the run continues" }
          }, "required": ["steps"] } }),
        json!({ "name": "being_status",
          "description": "Read-only: the being's steps, budget, per-domain selections and reward per energy, its last decisions with their reasons, and a check that every decision can be re-derived from the log",
          "inputSchema": { "type": "object", "properties": {} } }),
        json!({ "name": "being_explain",
          "description": "Read-only: for one decision event, show why the domain was chosen (counts, mean reward per energy, exploration bonus and score of every alternative), the events it cited as evidence, the outcome, and a replay that re-derives the choice from the log",
          "inputSchema": { "type": "object", "properties": { "decision_event_id": { "type": "string", "description": "Event id (or unique prefix of at least 8 characters) of a decision event, from self_sql over events or being_status" } }, "required": ["decision_event_id"] } }),
        json!({ "name": "being_rules",
          "description": "Read-only: the rules the being has discovered by search over its own logged examples (author-defined grammar): form, signature, training and held-out support, the cited example events, how often each was used, and a check that each rule is re-derived from the log alone",
          "inputSchema": { "type": "object", "properties": {} } }),
    ]
}
