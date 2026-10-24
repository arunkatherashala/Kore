//! MCP tools over the versioned log (`versioned/`): event_add, belief_revise,
//! belief_history, sql, versions, diff, verify, migrate_v1.
//!
//! Every call opens the log fresh from disk (`<data root>/<owner>/log`), does
//! its work, and drops it, so no stale in-memory copy can diverge from the
//! files. A process-wide mutex serialises log access between the MCP loop, the
//! HTTP/TCP servers and the heartbeat thread; a second *process* writing the
//! same log is caught by the library (`Conflict`) and reported as a tool error.
//! None of these tools needs the shared `KoreSelf` state, so the MCP loop calls
//! them without holding the global state lock (except `self_migrate_v1`, which
//! reads the old beliefs).

use crate::safety;
use crate::versioned::{self as vl, AsOf, Batch, EventRow, LogError, NewEvent, NewRevision, RevisionRow, VersionedLog};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;

static LOG_LOCK: Mutex<()> = Mutex::new(());

/// Tools that only need the owner name (no `KoreSelf` state).
pub const STATELESS_TOOLS: [&str; 18] = [
    "self_event_add",
    "self_belief_revise",
    "self_belief_history",
    "self_sql",
    "self_versions",
    "self_diff",
    "self_verify",
    "self_ingest_folder",
    "self_ingest_csv",
    "self_ingest_kore",
    "self_forget",
    "self_export",
    "self_learn",
    "self_explain",
    "self_evolve_rules",
    "self_ledger_snapshot",
    "self_ledger_outcome",
    "self_tag_rules_check",
];

pub fn is_log_tool(name: &str) -> bool {
    STATELESS_TOOLS.contains(&name) || name == "self_migrate_v1"
}

pub(crate) fn text(v: Value) -> Value {
    json!({ "content": [{ "type": "text", "text": v.to_string() }] })
}
pub(crate) fn plain(s: String) -> Value {
    json!({ "content": [{ "type": "text", "text": s }] })
}
pub(crate) fn err(s: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": s.into() }], "isError": true })
}
pub(crate) fn log_err(e: LogError) -> Value {
    match e {
        LogError::Conflict(d) => err(format!(
            "Conflict: the log changed underneath this call ({d}). Another kore-self process may be using this owner. Nothing was written; retry."
        )),
        other => err(other.to_string()),
    }
}

pub(crate) fn open(owner: &str) -> Result<VersionedLog, Value> {
    safety::validate_owner(owner).map_err(|e| err(format!("invalid owner: {e}")))?;
    // Routes through safe_path: the owner dir must resolve inside the data root.
    let base = safety::safe_path(owner, "log").map_err(|e| err(format!("log path rejected: {e}")))?;
    let root = base.parent().and_then(|p| p.parent()).map(|p| p.to_path_buf()).unwrap_or_else(safety::data_root);
    // `safe_path` canonicalises; open with the canonical root so the log lives at <root>/<owner>/log.
    VersionedLog::open_or_create(&root, owner).map_err(log_err)
}

pub(crate) fn req_str<'a>(args: &'a Value, k: &str) -> Result<&'a str, Value> {
    match args.get(k).and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => Ok(s),
        Some(_) => Err(err(format!("'{k}' must not be empty"))),
        None => Err(err(format!("'{k}' is required (string)"))),
    }
}

pub(crate) fn str_list(args: &Value, k: &str) -> Result<Vec<String>, Value> {
    match args.get(k) {
        None | Some(Value::Null) => Ok(vec![]),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| x.as_str().map(|s| s.to_string()).ok_or_else(|| err(format!("'{k}' must be an array of strings"))))
            .collect(),
        Some(_) => Err(err(format!("'{k}' must be an array of strings"))),
    }
}

/// Handle a stateless log tool. Returns None for any other tool name.
pub fn handle(name: &str, args: &Value, owner: &str) -> Option<Value> {
    if !STATELESS_TOOLS.contains(&name) {
        return None;
    }
    let _g = LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let r = match name {
        "self_event_add" => event_add(args, owner),
        "self_belief_revise" => belief_revise(args, owner),
        "self_belief_history" => belief_history(args, owner),
        "self_sql" => sql(args, owner),
        "self_versions" => versions(owner),
        "self_diff" => diff(args, owner),
        "self_verify" => verify(owner),
        "self_ingest_folder" => crate::ingest_tools::ingest_folder(args, owner),
        "self_ingest_csv" => crate::ingest_tools::ingest_csv(args, owner),
        "self_ingest_kore" => crate::ingest_tools::ingest_kore(args, owner),
        "self_forget" => crate::ingest_tools::forget(args, owner),
        "self_export" => crate::ingest_tools::export(args, owner),
        "self_learn" | "self_explain" | "self_evolve_rules" | "self_ledger_snapshot" | "self_ledger_outcome" | "self_tag_rules_check" => crate::learn_tools::handle(name, args, owner),
        _ => unreachable!(),
    };
    Some(r.unwrap_or_else(|e| e))
}

fn event_add(args: &Value, owner: &str) -> Result<Value, Value> {
    let t = req_str(args, "text")?;
    let occurred = args
        .get("occurred_at")
        .and_then(|v| v.as_str())
        .ok_or_else(|| err("'occurred_at' is required (ISO-8601 UTC, e.g. 2025-03-14 or 2025-03-14T09:30:00Z); it is never defaulted to now"))?;
    let occurred = vl::normalize_ts(occurred).map_err(|e| err(e))?;
    let source = args.get("source").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()).unwrap_or("mcp");
    let kind = args.get("kind").and_then(|v| v.as_str()).unwrap_or("note");
    let tags = str_list(args, "tags")?;
    let mut log = open(owner)?;
    let red = crate::redact::Redactor::load(&safety::owner_dir(owner)).redact(t);
    let ev = NewEvent { source: source.into(), occurred_at: occurred, kind: kind.into(), text: red.text.clone(), tags, redacted: red.changed() };
    let info = log.append_events(&[ev]).map_err(log_err)?;
    let mut out = json!({
        "event_id": info.event_ids.first(),
        "duplicate": !info.created,
        "version": info.version,
    });
    if info.suppressed > 0 {
        out["note"] = json!("not stored: this event was forgotten earlier (self_forget) and is not re-added");
    }
    if red.changed() {
        out["redacted"] = json!(red.kinds);
        out["redaction_note"] = json!("best-effort redaction replaced matches with [REDACTED:kind] before storing");
    }
    Ok(text(out))
}

fn belief_revise(args: &Value, owner: &str) -> Result<Value, Value> {
    let key = req_str(args, "key")?;
    let stance = req_str(args, "stance")?;
    let confidence = args
        .get("confidence")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| err("'confidence' is required (number 0..1)"))?;
    if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
        return Err(err("'confidence' must be within 0..1"));
    }
    let rationale = args.get("rationale").and_then(|v| v.as_str()).unwrap_or("");
    let caused_by = str_list(args, "caused_by")?;
    let entity = args.get("entity_type").and_then(|v| v.as_str()).unwrap_or("belief");
    if entity == "rule" {
        return Err(err("entity_type 'rule' is written only by the learning and evolution code, not by tools"));
    }
    let mut log = open(owner)?;
    let valid_from = match args.get("valid_from").and_then(|v| v.as_str()) {
        Some(v) => vl::normalize_ts(v).map_err(|e| err(e))?,
        None => {
            let evs: Vec<EventRow> = log.open_latest().rows();
            caused_by
                .iter()
                .filter_map(|id| evs.iter().find(|e| &e.event_id == id))
                .map(|e| e.occurred_at.clone())
                .max()
                .unwrap_or_else(vl::now_iso)
        }
    };
    let before = log.latest_version();
    let rev = NewRevision {
        entity_type: entity.into(),
        key: key.into(),
        stance: stance.into(),
        confidence,
        rationale: rationale.into(),
        supersedes: None,
        valid_from: valid_from.clone(),
        author: "user".into(), // rule authors are not reachable through tools
        caused_by: caused_by.clone(),
    };
    let rev_id = log.revise(rev).map_err(log_err)?;
    let version = log.latest_version();
    let mut out = json!({
        "rev_id": rev_id,
        "version": version,
        "duplicate": version == before,
        "valid_from": valid_from,
        "cited_events": caused_by.len(),
    });
    if caused_by.is_empty() {
        out["note"] = json!("unsupported: this revision cites no events (caused_by is empty); it is recorded with unsupported=1");
    }
    Ok(text(out))
}

fn belief_history(args: &Value, owner: &str) -> Result<Value, Value> {
    let key = req_str(args, "key")?;
    let entity = args.get("entity_type").and_then(|v| v.as_str());
    let log = open(owner)?;
    let snap = log.open_latest();
    let revs: Vec<RevisionRow> = snap.rows();
    let events: HashMap<String, EventRow> = snap.rows::<EventRow>().into_iter().map(|e| (e.event_id.clone(), e)).collect();
    let mut causes: HashMap<String, Vec<String>> = HashMap::new();
    for c in snap.rows::<vl::CauseRow>() {
        causes.entry(c.rev_id).or_default().push(c.event_id);
    }
    let rows: Vec<Value> = revs
        .iter()
        .filter(|r| r.key == key && entity.map_or(true, |e| r.entity_type == e))
        .map(|r| {
            let cited: Vec<Value> = causes
                .get(&r.rev_id)
                .map(|ids| {
                    ids.iter()
                        .map(|id| match events.get(id) {
                            Some(e) => json!({"event_id": id, "occurred_at": e.occurred_at, "source": e.source, "text": e.text}),
                            None => json!({"event_id": id, "missing": true}),
                        })
                        .collect()
                })
                .unwrap_or_default();
            json!({
                "rev_id": r.rev_id, "entity_type": r.entity_type, "stance": r.stance, "confidence": r.confidence,
                "rationale": r.rationale, "valid_from": r.valid_from, "author": r.author, "recorded_at": r.recorded_at,
                "supersedes": r.supersedes, "unsupported": cited.is_empty(), "caused_by": cited,
            })
        })
        .collect();
    Ok(text(json!({ "key": key, "version": snap.version, "revisions": rows.len(), "history": rows })))
}

const MAX_SQL_ROWS: usize = 200;

fn sql(args: &Value, owner: &str) -> Result<Value, Value> {
    let q = req_str(args, "sql")?;
    let log = open(owner)?;
    let snap = match (args.get("as_of_version"), args.get("as_of_time")) {
        (Some(_), Some(_)) => return Err(err("pass only one of as_of_version and as_of_time")),
        (Some(v), None) => {
            let n = v.as_u64().ok_or_else(|| err("'as_of_version' must be a non-negative integer"))?;
            log.open_at(AsOf::Version(n)).map_err(log_err)?
        }
        (None, Some(t)) => {
            let t = t.as_str().ok_or_else(|| err("'as_of_time' must be an ISO-8601 string"))?;
            // transaction time picks the version; the same instant is the valid-time cut for beliefs_asof
            let s = log.open_at(AsOf::Time(t.to_string())).map_err(log_err)?;
            s.with_valid_at(t).map_err(log_err)?
        }
        (None, None) => log.open_latest(),
    };
    let block = vl::run_readonly_sql(&snap, q).map_err(log_err)?;
    let mut rendered = vl::sql::render(&block, 60);
    let lines: Vec<&str> = rendered.lines().collect();
    if block.num_rows > MAX_SQL_ROWS {
        let keep = MAX_SQL_ROWS + 2;
        let mut cut = lines[..keep.min(lines.len())].join("\n");
        cut.push_str(&format!("\n... output cut at {MAX_SQL_ROWS} of {} rows; add LIMIT or WHERE", block.num_rows));
        rendered = cut;
    }
    Ok(plain(format!("version {} (recorded {})\n{}", snap.version, snap.recorded_at, rendered)))
}

fn versions(owner: &str) -> Result<Value, Value> {
    let log = open(owner)?;
    let list: Vec<Value> = log
        .versions()
        .iter()
        .map(|v| {
            json!({
                "version": v.v, "recorded_at": v.recorded_at, "hash": safety::trunc(&v.manifest_hash, 12),
                "rows": v.rows,
            })
        })
        .collect();
    Ok(text(json!({ "latest": log.latest_version(), "versions": list })))
}

fn diff(args: &Value, owner: &str) -> Result<Value, Value> {
    let v1 = args.get("v1").and_then(|v| v.as_u64()).ok_or_else(|| err("'v1' is required (non-negative integer)"))?;
    let v2 = args.get("v2").and_then(|v| v.as_u64()).ok_or_else(|| err("'v2' is required (non-negative integer)"))?;
    let log = open(owner)?;
    let d = log.diff(v1, v2).map_err(log_err)?;
    Ok(text(json!({
        "from": d.from, "to": d.to,
        "counts": { "events": d.events.len(), "revisions": d.revisions.len(), "revision_causes": d.causes.len(),
                    "predictions": d.predictions.len(), "outcomes": d.outcomes.len() },
        "events": d.events.iter().map(|e| json!({"event_id": e.event_id, "occurred_at": e.occurred_at, "source": e.source, "kind": e.kind, "text": safety::trunc(&e.text, 200)})).collect::<Vec<_>>(),
        "revisions": d.revisions.iter().map(|r| json!({"rev_id": r.rev_id, "key": r.key, "stance": safety::trunc(&r.stance, 200), "confidence": r.confidence, "author": r.author})).collect::<Vec<_>>(),
    })))
}

fn verify(owner: &str) -> Result<Value, Value> {
    safety::validate_owner(owner).map_err(|e| err(format!("invalid owner: {e}")))?;
    let base = safety::safe_path(owner, "log").map_err(|e| err(format!("log path rejected: {e}")))?;
    // Finish a forget that was interrupted by a crash before checking anything.
    vl::recover_pending(&base).map_err(log_err)?;
    // Verify straight from disk, before opening (opening a damaged log would fail first).
    match vl::verify_dir(&base) {
        Ok(()) => {
            let log = VersionedLog::open(base.parent().and_then(|p| p.parent()).unwrap_or(&safety::data_root()), owner).map_err(log_err)?;
            let v = log.latest_version();
            let head = log.snapshot_hash(v).unwrap_or_default();
            Ok(plain(format!("OK: {} version(s), latest {}, head hash {}", v + 1, v, head)))
        }
        Err(vl::VerifyError::ManifestMissing) if !base.exists() || std::fs::read_dir(&base).map(|d| d.count() == 0).unwrap_or(true) => {
            Ok(plain("OK: no log exists yet (nothing to verify)".into()))
        }
        Err(e) => Ok(err(format!("VERIFY FAILED: {e}"))),
    }
}

/// `self_migrate_v1`: import old identity beliefs as `author=legacy`
/// revisions. No causes are invented, so every imported row is unsupported.
/// Idempotent: a topic that already has any belief revision is skipped.
pub fn migrate_v1(owner: &str, beliefs: &[(String, String, f64, String, Vec<String>)]) -> Value {
    let _g = LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let run = || -> Result<Value, Value> {
        let mut log = open(owner)?;
        let existing: std::collections::HashSet<String> = log
            .open_latest()
            .rows::<RevisionRow>()
            .into_iter()
            .filter(|r| r.entity_type == "belief")
            .map(|r| r.key)
            .collect();
        let mut sorted: Vec<&(String, String, f64, String, Vec<String>)> = beliefs.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        let (mut skipped_existing, mut skipped_invalid) = (0, 0);
        let mut batch = Batch::default();
        for (topic, stance, conf, formed_at, history) in sorted {
            if topic.trim().is_empty() || stance.trim().is_empty() {
                skipped_invalid += 1;
                continue;
            }
            if existing.contains(topic) {
                skipped_existing += 1;
                continue;
            }
            let valid_from = vl::normalize_ts(safety::date_part(formed_at)).unwrap_or_else(|_| "1970-01-01T00:00:00Z".into());
            let mut rationale = String::from("imported from the pre-log identity state; no cited events");
            if !history.is_empty() {
                rationale.push_str("; earlier stances recorded there: ");
                rationale.push_str(&history.join(" / "));
            }
            batch.revisions.push(NewRevision {
                entity_type: "belief".into(),
                key: topic.clone(),
                stance: stance.clone(),
                confidence: if conf.is_finite() { conf.clamp(0.0, 1.0) } else { 0.5 },
                rationale: safety::trunc(&rationale, 4000).to_string(),
                supersedes: None,
                valid_from,
                author: "legacy".into(),
                caused_by: vec![],
            });
        }
        let n = batch.revisions.len();
        let version = if n == 0 { log.latest_version() } else { log.commit_batch(batch).map_err(log_err)?.version };
        Ok(text(json!({
            "imported": n, "skipped_existing": skipped_existing, "skipped_invalid": skipped_invalid, "version": version,
            "note": "imported rows have author=legacy and no cited events (unsupported=1)",
        })))
    };
    run().unwrap_or_else(|e| e)
}

// ─── Tool definitions ────────────────────────────────────────────────────────

pub fn tool_defs() -> Vec<Value> {
    let sql_desc = "Run one read-only SELECT/WITH over a snapshot of your versioned log (latest, or as_of_version / as_of_time). \
Tables: events(event_id, occurred_at, recorded_at, source, kind, text, tags, redacted); \
revisions(rev_id, entity_type, key, stance, confidence, rationale, supersedes, valid_from, author, recorded_at, unsupported); \
revision_causes(rev_id, event_id); predictions(pred_id, made_at, question_id, target, options, probs, model_id, model_version, features_cite, resolves_by); \
outcomes(pred_id, resolved_at, actual, outcome_event_id); schema_meta. \
Views: beliefs_current (latest revision per entity_type+key) and beliefs_asof (latest revision with valid_from <= the snapshot time). \
Joins are equi-joins; output columns of a join keep their alias prefix (r.key, e.text). \
Example 1: SELECT key, stance, confidence FROM beliefs_current WHERE entity_type = 'belief'. \
Example 2: SELECT r.key, r.stance, e.occurred_at, e.text FROM revisions r JOIN revision_causes c ON r.rev_id = c.rev_id JOIN events e ON c.event_id = e.event_id";
    vec![
        json!({ "name": "self_event_add",
          "description": "Append a dated event to the versioned log (idempotent: the same source, time and text is stored once). occurred_at is required and never defaulted.",
          "inputSchema": { "type": "object", "properties": {
            "text": { "type": "string" },
            "occurred_at": { "type": "string", "description": "ISO-8601 UTC, YYYY-MM-DD or YYYY-MM-DDTHH:MM:SSZ. Required." },
            "source": { "type": "string", "description": "Where the event came from. Default: mcp" },
            "kind": { "type": "string", "enum": ["note","action","decision","outcome","statement"] },
            "tags": { "type": "array", "items": { "type": "string" } }
          }, "required": ["text", "occurred_at"] } }),
        json!({ "name": "self_belief_revise",
          "description": "Record a new revision of a belief, habit or decision, citing the events that caused it (caused_by). A revision with no citations is stored but marked unsupported.",
          "inputSchema": { "type": "object", "properties": {
            "key": { "type": "string" },
            "stance": { "type": "string" },
            "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
            "rationale": { "type": "string" },
            "caused_by": { "type": "array", "items": { "type": "string" }, "description": "event_ids from self_event_add; each must exist" },
            "valid_from": { "type": "string", "description": "ISO date. Default: occurred_at of the newest cited event, else now." },
            "entity_type": { "type": "string", "enum": ["belief","habit","decision"] }
          }, "required": ["key", "stance", "confidence", "caused_by"] } }),
        json!({ "name": "self_belief_history",
          "description": "Show every revision of a key in order, with the cited events expanded (time, source, text) and the supersedes chain.",
          "inputSchema": { "type": "object", "properties": {
            "key": { "type": "string" },
            "entity_type": { "type": "string", "enum": ["belief","habit","decision"] }
          }, "required": ["key"] } }),
        json!({ "name": "self_sql",
          "description": sql_desc,
          "inputSchema": { "type": "object", "properties": {
            "sql": { "type": "string" },
            "as_of_version": { "type": "integer", "minimum": 0, "description": "Query the log as it was at this version (see self_versions)" },
            "as_of_time": { "type": "string", "description": "Query the log as it was at this ISO-8601 UTC time (latest version recorded by then)" }
          }, "required": ["sql"] } }),
        json!({ "name": "self_versions",
          "description": "List the commits of the versioned log: version, recorded_at, cumulative row counts, short hash.",
          "inputSchema": { "type": "object", "properties": {} } }),
        json!({ "name": "self_diff",
          "description": "Show the rows added between two versions of the versioned log (v1 exclusive, v2 inclusive).",
          "inputSchema": { "type": "object", "properties": {
            "v1": { "type": "integer", "minimum": 0 }, "v2": { "type": "integer", "minimum": 0 }
          }, "required": ["v1", "v2"] } }),
        json!({ "name": "self_verify",
          "description": "Recompute every checksum and the hash chain of the versioned log from disk; reports OK or the exact failure and file.",
          "inputSchema": { "type": "object", "properties": {} } }),
        json!({ "name": "self_migrate_v1",
          "description": "Import beliefs from the pre-log identity state as author=legacy revisions marked unsupported (no causes are invented). Safe to repeat: topics already in the log are skipped.",
          "inputSchema": { "type": "object", "properties": {} } }),
    ]
    .into_iter()
    .chain(crate::ingest_tools::tool_defs())
    .chain(crate::learn_tools::tool_defs())
    .collect()
}

/// Used by `self_ingest`: append the memory as a `note` event stamped now.
/// Failures are returned, not hidden, so the caller can mention them.
pub fn record_ingest(owner: &str, content: &str, kind: &str) -> Result<(String, bool), String> {
    let _g = LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut log = open(owner).map_err(|v| v["content"][0]["text"].as_str().unwrap_or("log error").to_string())?;
    let red = crate::redact::Redactor::load(&safety::owner_dir(owner)).redact(content);
    let ev = NewEvent {
        source: "self_ingest".into(),
        occurred_at: vl::now_iso(),
        kind: "note".into(),
        text: red.text.clone(),
        tags: vec![kind.to_string()],
        redacted: red.changed(),
    };
    let info = log.append_events(&[ev]).map_err(|e| e.to_string())?;
    Ok((info.event_ids.first().cloned().unwrap_or_default(), !info.created))
}

#[cfg(test)]
#[path = "log_tools_tests.rs"]
mod tests;
