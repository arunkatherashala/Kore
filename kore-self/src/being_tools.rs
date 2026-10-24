//! Tools of the being loop: being_run, being_status, being_explain.
//!
//! The loop runs in-process, only against the simulated world and this owner's
//! own log (see `being/policy.rs`); no network, no files other than the log.
//! The owner directory holds the being's log. It is a deterministic simulation
//! with author-defined rewards, measured in docs/measured.md.

use crate::being::replay::replay_decisions;
use crate::being::run_loop::{domain_json, RunCfg};
use crate::being::state::{tag, BeingState, SOURCE};
use crate::being::{self, Policy, ScriptedThinker};
use crate::log_tools::{err, log_err, open, req_str, text};
use crate::versioned::{CauseRow, EventRow, RevisionRow};
use serde_json::{json, Value};
use std::collections::HashMap;

pub const TOOLS: [&str; 3] = ["being_run", "being_status", "being_explain"];
const MAX_STEPS: u64 = 5000;

pub fn handle(name: &str, args: &Value, owner: &str) -> Result<Value, Value> {
    match name {
        "being_run" => run(args, owner),
        "being_status" => status(owner),
        "being_explain" => explain(args, owner),
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
    let cfg = RunCfg {
        world_seed: opt_u64(args, "world_seed")?,
        loop_seed: opt_u64(args, "seed")?,
        budget,
        explore_c: None,
        steps,
        refill_when_exhausted: !stop_after_budget,
    };
    let mut log = open(owner)?;
    let mut policy = Policy::new(steps);
    policy.refill_when_exhausted = cfg.refill_when_exhausted;
    let s = being::run(&mut log, &cfg, &ScriptedThinker, &policy).map_err(log_err)?;
    let mut out = s.to_json();
    out["note"] = json!("deterministic simulation loop: a seeded world with author-defined rewards and a scripted solver; the preference comes from those rewards. See docs/measured.md");
    Ok(text(out))
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
            json!({ "decision_event_id": e.event_id, "occurred_at": e.occurred_at, "step": tag(&e.tags, "step"), "domain": tag(&e.tags, "domain"), "text": e.text, "reasons": rationale })
        })
        .collect();
    let rep = replay_decisions(&rows);
    let ok = rep.iter().filter(|r| r.matches()).count();
    Ok(text(json!({
        "exists": true,
        "world_seed": g.world_seed, "loop_seed": g.loop_seed, "initial_budget": g.budget, "explore_c": g.explore_c,
        "steps": st.step, "budget_left": st.budget_left, "refills": st.refills, "world_clock_seconds": st.clock,
        "total_reward": st.total_reward, "total_energy_spent": st.total_cost,
        "reward_per_energy": if st.total_cost > 0 { st.total_reward as f64 / st.total_cost as f64 } else { 0.0 },
        "domains": domain_json(&st),
        "last_decisions": last,
        "replay_check": { "decisions": rep.len(), "re_derived_from_log_and_equal": ok },
        "note": "deterministic simulation; preferences are produced by the defined rewards",
    })))
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
    let result = rows.iter().find(|e| e.source == SOURCE && e.kind == "outcome" && tag(&e.tags, "step") == step).map(|e| json!({ "event_id": e.event_id, "text": e.text }));
    let rep = replay_decisions(&rows).into_iter().find(|r| r.event_id == d.event_id);
    Ok(text(json!({
        "decision_event_id": d.event_id, "occurred_at": d.occurred_at, "step": step, "domain": tag(&d.tags, "domain"),
        "decision": d.text,
        "revision": rev.as_ref().map(|r| json!({ "rev_id": r.rev_id, "key": r.key, "stance": r.stance, "author": r.author })),
        "reasons_and_alternatives": reasons,
        "evidence_events_cited": cited,
        "evidence_note": "the cited events are the genesis event and the latest outcomes per domain; the counts and means used come from all outcomes before this step",
        "outcome_of_this_step": result,
        "replay_check": rep.map(|r| json!({ "re_derived_domain": r.rederived.name(), "equal_to_recorded": r.matches() })),
    })))
}

pub fn tool_defs() -> Vec<Value> {
    vec![
        json!({ "name": "being_run",
          "description": "Run the being loop for N steps in a deterministic simulated world (arithmetic, algebra, physics formulas, sequences) with a seeded budget; the being picks the domain by learned reward per energy and every decision, action and outcome is appended to its own log. A simulation with author-defined rewards, not a real-world agent",
          "inputSchema": { "type": "object", "properties": {
            "steps": { "type": "integer", "minimum": 1, "maximum": 5000, "description": "Maximum steps to run in this call" },
            "seed": { "type": "integer", "minimum": 0, "description": "Loop seed (order of first exploration). Default 1; fixed once the being exists" },
            "world_seed": { "type": "integer", "minimum": 0, "description": "World seed (hidden rewards and tasks). Default 1; fixed once the being exists" },
            "budget": { "type": "integer", "minimum": 1, "description": "Energy units for a new being. Default 2000" },
            "stop_after_budget": { "type": "boolean", "description": "Default true: stop when no task is affordable. False: the world refills the budget at the next world day (logged) and the run continues" }
          }, "required": ["steps"] } }),
        json!({ "name": "being_status",
          "description": "Read-only: the being's steps, budget, per-domain selections and reward per energy, its last decisions with their reasons, and a check that every decision can be re-derived from the log",
          "inputSchema": { "type": "object", "properties": {} } }),
        json!({ "name": "being_explain",
          "description": "Read-only: for one decision event, show why the domain was chosen (counts, mean reward per energy, exploration bonus and score of every alternative), the events it cited as evidence, the outcome, and a replay that re-derives the choice from the log",
          "inputSchema": { "type": "object", "properties": { "decision_event_id": { "type": "string", "description": "Event id (or unique prefix of at least 8 characters) of a decision event, from self_sql over events or being_status" } }, "required": ["decision_event_id"] } }),
    ]
}
