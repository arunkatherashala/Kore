//! S4 tools: predict_make, predict_resolve, predict_score.
//!
//! Same safety as the other log tools: they run under the process log lock
//! through `log_tools::handle`, open the owner's log through `safe_path`, and
//! write only prediction, outcome and refusal rows through the versioned
//! library. `predict_score` only reads. None of them is on the chat thinker's
//! tool list (`authorize` default-denies anything not listed in `CHAT_TOOLS`).

use crate::log_tools::{err, log_err, open, req_str, str_list, text};
use crate::predict::baselines::Config;
use crate::predict::ledger::{self, MakeReq, ResolveReq};
use serde_json::{json, Value};

pub const TOOLS: [&str; 3] = ["predict_make", "predict_resolve", "predict_score"];

pub fn handle(name: &str, args: &Value, owner: &str) -> Result<Value, Value> {
    match name {
        "predict_make" => make(args, owner),
        "predict_resolve" => resolve(args, owner),
        "predict_score" => score(owner),
        _ => Err(err("unknown tool")),
    }
}

fn opt_str(args: &Value, k: &str) -> Result<Option<String>, Value> {
    match args.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(err(format!("'{k}' must be a string"))),
    }
}

fn make(args: &Value, owner: &str) -> Result<Value, Value> {
    let context = req_str(args, "context")?.to_string();
    let predictors = str_list(args, "predictors")?;
    let mut cfg = Config::default();
    match args.get("min_n") {
        None | Some(Value::Null) => {}
        Some(v) => {
            let n = v.as_u64().filter(|n| (1..=100_000).contains(n)).ok_or_else(|| err("'min_n' must be an integer 1..100000"))? as usize;
            cfg.min_n = n;
            cfg.min_ctx_n = n;
        }
    }
    let req = MakeReq { context, predictors, made_at: opt_str(args, "made_at")?, question_id: opt_str(args, "question_id")?, cfg };
    let mut log = open(owner)?;
    let r = ledger::make(&mut log, &req).map_err(log_err)?;
    Ok(text(json!({
        "question_id": r.question_id, "made_at": r.made_at, "cutoff_version": r.cutoff_version, "history_observations": r.history_n,
        "written": r.written, "already_present": r.already_present, "refused_insufficient_data": r.refused, "version": r.version,
        "note": "forecasts written before any outcome, from events dated at or before made_at; a refusal is logged, never guessed; resolve with predict_resolve, score with predict_score",
    })))
}

fn resolve(args: &Value, owner: &str) -> Result<Value, Value> {
    let actual = req_str(args, "actual")?.to_string();
    let resolved_at = req_str(args, "resolved_at")?.to_string();
    let question_id = opt_str(args, "question_id")?;
    let pred_id = opt_str(args, "pred_id")?;
    if question_id.is_none() && pred_id.is_none() {
        return Err(err("give 'question_id' (resolves every forecast of the question) or 'pred_id'"));
    }
    let req = ResolveReq { question_id, pred_id, actual, resolved_at, outcome_event_id: opt_str(args, "outcome_event_id")? };
    let mut log = open(owner)?;
    let r = ledger::resolve(&mut log, &req).map_err(log_err)?;
    Ok(text(json!({
        "resolved": r.resolved, "already_resolved": r.already_resolved, "actual_recorded": r.actual_recorded, "version": r.version,
        "note": "outcome attached; an outcome dated before its prediction is refused; no score is computed here",
    })))
}

fn score(owner: &str) -> Result<Value, Value> {
    let log = open(owner)?;
    Ok(text(ledger::score_report(&log)))
}

pub fn tool_defs() -> Vec<Value> {
    vec![
        json!({ "name": "predict_make",
          "description": "Forecast the next choice in a context with the baseline predictors (uniform, frequency, last-choice, recency, context-count), using only events dated at or before made_at, and write the forecasts to the ledger before any outcome exists. A predictor with too little data is refused and the refusal is logged, never guessed. Refuses a made_at earlier than events already in the log",
          "inputSchema": { "type": "object", "properties": {
            "context": { "type": "string", "description": "The ctx: tag value the question is about" },
            "predictors": { "type": "array", "items": { "type": "string", "enum": ["uniform", "frequency", "last-choice", "recency", "context-count"] }, "description": "Default: all five" },
            "made_at": { "type": "string", "description": "ISO-8601 UTC. Default: now" },
            "question_id": { "type": "string", "description": "Default: next-choice:<context>@<made_at>" },
            "min_n": { "type": "integer", "minimum": 1, "description": "Override the minimum number of observations (in the context, for context-count)" }
          }, "required": ["context"] } }),
        json!({ "name": "predict_resolve",
          "description": "Attach the choice that actually happened to the forecasts of a question (or one forecast by pred_id). Refused if resolved_at is before the forecast was made or the cited outcome event is not after it. A choice not among the options is recorded as (other)",
          "inputSchema": { "type": "object", "properties": {
            "question_id": { "type": "string" }, "pred_id": { "type": "string" },
            "actual": { "type": "string", "description": "The choice that happened" },
            "resolved_at": { "type": "string", "description": "ISO-8601 UTC time the outcome became known. Required" },
            "outcome_event_id": { "type": "string" }
          }, "required": ["actual", "resolved_at"] } }),
        json!({ "name": "predict_score",
          "description": "Read-only scoring report of resolved forecasts: Brier score, log loss, accuracy, calibration bins and ECE with bootstrap 95% intervals per predictor, paired comparison against each baseline, forecasts excluded for breaking the time order, pending counts and logged refusals. Says so when there is too little data for a verdict",
          "inputSchema": { "type": "object", "properties": {} } }),
    ]
}
