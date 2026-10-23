//! M2b/M2c tools: self_learn, self_explain, self_evolve_rules,
//! self_ledger_snapshot, self_ledger_outcome.
//!
//! Rule-authored revisions (`author = rule:<id>`) are written only by this
//! code, through the library; no argument of any tool can choose an author or
//! an entity type of `rule`. Nothing here writes outside the log directory and
//! nothing here writes or compiles source code.

use crate::learn::derive::{self, Kinds};
use crate::learn::{evolve, ledger, Params};
use crate::log_tools::{err, log_err, open, req_str, str_list, text};
use crate::safety;
use crate::versioned::{self as vl, CauseRow, EventRow, NewOutcome, PredictionRow, RevisionRow};
use serde_json::{json, Value};
use std::collections::HashMap;

pub const TOOLS: [&str; 5] = ["self_learn", "self_explain", "self_evolve_rules", "self_ledger_snapshot", "self_ledger_outcome"];

pub fn handle(name: &str, args: &Value, owner: &str) -> Result<Value, Value> {
    match name {
        "self_learn" => learn(args, owner),
        "self_explain" => explain(args, owner),
        "self_evolve_rules" => evolve_rules(args, owner),
        "self_ledger_snapshot" => ledger_snapshot(args, owner),
        "self_ledger_outcome" => ledger_outcome(args, owner),
        _ => Err(err("unknown tool")),
    }
}

fn opt_bool(args: &Value, k: &str) -> Result<bool, Value> {
    match args.get(k) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(err(format!("'{k}' must be a boolean"))),
    }
}

fn opt_u32(args: &Value, k: &str) -> Result<Option<u32>, Value> {
    match args.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().and_then(|n| u32::try_from(n).ok()).map(Some).ok_or_else(|| err(format!("'{k}' must be a non-negative integer"))),
    }
}

const MAX_LISTED: usize = 100;

fn learn(args: &Value, owner: &str) -> Result<Value, Value> {
    let kinds = Kinds::parse(&str_list(args, "kinds")?).map_err(err)?;
    let dry = opt_bool(args, "dry_run")?;
    let mut log = open(owner)?;
    let (mut params, rule_rev) = derive::current_params(&log);
    if let Some(n) = opt_u32(args, "min_n")? {
        params.habit_min_n = n;
    }
    params.validate().map_err(err)?;
    let r = derive::learn(&mut log, &params, &kinds, dry).map_err(log_err)?;
    let mut counts = json!({});
    for k in ["habits", "changes", "stances"] {
        let (mut derived, mut new) = (0, 0);
        for o in &r.outcomes {
            if r.derivation.items[o.item].kind == k {
                derived += 1;
                if o.is_new {
                    new += 1;
                }
            }
        }
        counts[k] = json!({ "derived": derived, if dry { "would_add" } else { "added" }: new, "already_present": derived - new });
    }
    let listed: Vec<Value> = r
        .outcomes
        .iter()
        .take(MAX_LISTED)
        .map(|o| {
            let it = &r.derivation.items[o.item];
            json!({ "kind": it.kind, "key": it.key, "stance": it.stance, "confidence": it.confidence, "author": it.author,
                    "rev_id": o.rev_id, "new": o.is_new, "detail": it.detail })
        })
        .collect();
    Ok(text(json!({
        "dry_run": dry,
        "params": params.to_json(),
        "params_source": rule_rev.map(|id| format!("rule revision {id}")).unwrap_or_else(|| "built-in defaults".into()),
        "version_before": r.version_before, "version_after": r.version_after,
        "counts": counts,
        "derived": listed,
        "derived_truncated": r.outcomes.len() > MAX_LISTED,
        "insufficient_data": {
            "habit_contexts": r.derivation.insufficient_habits.iter().map(|i| json!({"context": i.context, "occasions": i.n, "min_n": i.min_n})).collect::<Vec<_>>(),
            "topics": r.derivation.insufficient_topics.iter().map(|i| json!({"topic": i.topic, "mentions": i.total, "min_mentions": i.min_total})).collect::<Vec<_>>(),
        },
        "note": "rule-based, deterministic; accuracy has been measured only on synthetic data (docs/measured.md)",
    })))
}

fn explain(args: &Value, owner: &str) -> Result<Value, Value> {
    let id = req_str(args, "rev_id")?;
    if id.len() < 8 {
        return Err(err("'rev_id' must be at least 8 characters"));
    }
    let log = open(owner)?;
    let snap = log.open_latest();
    let revs: Vec<RevisionRow> = snap.rows();
    let found: Vec<&RevisionRow> = revs.iter().filter(|r| r.rev_id.starts_with(id)).collect();
    let r = match found.as_slice() {
        [] => return Err(err(format!("not found: revision {id}"))),
        [one] => *one,
        _ => return Err(err(format!("'{id}' matches {} revisions; give more characters", found.len()))),
    };
    let events: HashMap<String, EventRow> = snap.rows::<EventRow>().into_iter().map(|e| (e.event_id.clone(), e)).collect();
    let mut cited: Vec<&EventRow> = snap.rows::<CauseRow>().iter().filter(|c| c.rev_id == r.rev_id).filter_map(|c| events.get(&c.event_id)).collect::<Vec<_>>();
    cited.sort_by(|a, b| (&a.occurred_at, &a.event_id).cmp(&(&b.occurred_at, &b.event_id)));
    let shown: Vec<Value> = cited
        .iter()
        .take(50)
        .map(|e| json!({"event_id": e.event_id, "occurred_at": e.occurred_at, "source": e.source, "kind": e.kind, "tags": e.tags, "text": safety::trunc(&e.text, 300)}))
        .collect();
    let derived_by_rule = r.author.starts_with("rule:");
    Ok(text(json!({
        "rev_id": r.rev_id, "entity_type": r.entity_type, "key": r.key, "stance": r.stance, "confidence": r.confidence,
        "rationale": r.rationale, "valid_from": r.valid_from, "author": r.author, "recorded_at": r.recorded_at,
        "supersedes": r.supersedes,
        "unsupported": cited.is_empty(),
        "cited_event_count": cited.len(),
        "cited_events": shown,
        "cited_truncated": cited.len() > 50,
        "provenance": if derived_by_rule { "derived by a deterministic rule from the cited events; re-running the rule on a log without them does not reproduce this row" } else { "recorded by the user" },
    })))
}

fn evolve_rules(args: &Value, owner: &str) -> Result<Value, Value> {
    let dry = opt_bool(args, "dry_run")?;
    let mut log = open(owner)?;
    let (d, rec) = evolve::run(&mut log, &evolve::EvolveCfg::default(), dry).map_err(log_err)?;
    let mut out = d.to_json();
    out["dry_run"] = json!(dry);
    out["recorded"] = match rec {
        Some(r) => json!({"event_id": r.event_id, "rev_id": r.rev_id, "version": r.version}),
        None => Value::Null,
    };
    out["note"] = json!("changes rule parameters only (never source code); rollback is time travel: read an earlier rule set with self_sql as_of_version");
    Ok(text(out))
}

fn ledger_snapshot(args: &Value, owner: &str) -> Result<Value, Value> {
    let dry = opt_bool(args, "dry_run")?;
    let window_start = args.get("window_start").and_then(|v| v.as_str());
    let days = match opt_u32(args, "window_days")? {
        None => 7,
        Some(n) if (1..=60).contains(&n) => n as i64,
        Some(_) => return Err(err("'window_days' must be 1..60")),
    };
    let mut log = open(owner)?;
    let (mut params, _) = derive::current_params(&log);
    if let Some(n) = opt_u32(args, "min_n")? {
        params.habit_min_n = n;
    }
    params.validate().map_err(err)?;
    let r = ledger::snapshot(&mut log, &params, window_start, days, dry).map_err(log_err)?;
    Ok(text(json!({
        "dry_run": dry, "window_start": r.window_start, "resolves_by": r.resolves_by, "cutoff": r.cutoff,
        "model": format!("{} {}", ledger::MODEL_ID, ledger::MODEL_VERSION),
        "written": r.written, "already_present": r.already_present, "insufficient_data": r.insufficient,
        "ignored_events_at_or_after_window_start": r.ignored_future_events, "version": r.version,
        "note": "frequency baseline written before any outcome; nothing is scored and no accuracy is claimed",
    })))
}

fn ledger_outcome(args: &Value, owner: &str) -> Result<Value, Value> {
    let pred_id = req_str(args, "pred_id")?;
    let actual = req_str(args, "actual")?;
    let resolved_at = req_str(args, "resolved_at")?;
    let resolved_at = vl::normalize_ts(resolved_at).map_err(err)?;
    let outcome_event_id = args.get("outcome_event_id").and_then(|v| v.as_str()).map(|s| s.to_string());
    let mut log = open(owner)?;
    let exists = log.open_latest().rows::<PredictionRow>().iter().any(|p| p.pred_id == pred_id);
    if !exists {
        return Err(err(format!("not found: prediction {pred_id}")));
    }
    let info = log.record_outcome(NewOutcome { pred_id: pred_id.into(), resolved_at, actual: actual.into(), outcome_event_id }).map_err(log_err)?;
    Ok(text(json!({"pred_id": pred_id, "version": info.version, "duplicate": !info.created, "note": "outcome attached; no score is computed"})))
}

pub fn tool_defs() -> Vec<Value> {
    vec![
        json!({ "name": "self_learn",
          "description": "Derive habits, topic change points and explicit stance statements from the event log with deterministic rules; each derived revision is authored by a rule and cites its events (idempotent, incremental). Measured only on synthetic data",
          "inputSchema": { "type": "object", "properties": {
            "kinds": { "type": "array", "items": { "type": "string", "enum": ["habits", "changes", "stances"] }, "description": "Default: all three" },
            "dry_run": { "type": "boolean", "description": "Report what would be derived without writing" },
            "min_n": { "type": "integer", "minimum": 1, "description": "Override the minimum number of occasions of a context for a habit (default: current rule set). Contexts below it are reported as insufficient data" }
          } } }),
        json!({ "name": "self_explain",
          "description": "Show one revision with the events it cites (the provenance); rev_id may be a unique prefix of at least 8 characters",
          "inputSchema": { "type": "object", "properties": { "rev_id": { "type": "string" } }, "required": ["rev_id"] } }),
        json!({ "name": "self_evolve_rules",
          "description": "Propose a neighbouring set of learner parameters, backtest it on the past of the log (rolling origin) against the current set and a trivial baseline, and promote it only if it wins; every promotion or rejection is logged with its scores. Changes rule parameters only, never source code",
          "inputSchema": { "type": "object", "properties": { "dry_run": { "type": "boolean", "description": "Report the decision without writing" } } } }),
        json!({ "name": "self_ledger_snapshot",
          "description": "Record frequency-baseline predictions (action in context within the next 7 days) for each habit with enough data, using only events before the window start; one row per habit and window, never scored here",
          "inputSchema": { "type": "object", "properties": {
            "window_start": { "type": "string", "description": "ISO date the window starts. Default: the day after the newest event" },
            "window_days": { "type": "integer", "minimum": 1, "maximum": 60, "description": "Default 7" },
            "min_n": { "type": "integer", "minimum": 1 },
            "dry_run": { "type": "boolean" }
          } } }),
        json!({ "name": "self_ledger_outcome",
          "description": "Attach an outcome (yes or no) to a stored prediction, optionally citing the event that shows it; no score is computed",
          "inputSchema": { "type": "object", "properties": {
            "pred_id": { "type": "string" }, "actual": { "type": "string", "enum": ["yes", "no"] },
            "resolved_at": { "type": "string", "description": "ISO-8601 UTC time the outcome became known. Required" },
            "outcome_event_id": { "type": "string" }
          }, "required": ["pred_id", "actual", "resolved_at"] } }),
    ]
}

#[allow(dead_code)]
fn _unused(_: Params) {}
