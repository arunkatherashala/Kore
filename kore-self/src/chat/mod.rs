//! Talking to the being (A1): a provider-agnostic chat scaffold.
//!
//! One turn (`say`): log the user's message, build a bounded and redacted
//! context from the recent conversation and the current beliefs/habits, ask a
//! `ChatThinker` (built-in rules, or a program the owner configured in
//! `thinker.txt`), and run the tool calls it PROPOSES only after `authorize`
//! allows them. Everything (messages, the exact prompt and reply, every
//! decision, every tool result) is an event in the being's own log, so a
//! conversation survives restarts and can be queried with self_sql.
//!
//! No model is built in and this crate has no network code. The quality of the
//! replies depends entirely on the thinker you plug in and is not measured here.

pub mod advisor_cmd;
pub mod scripted;
pub mod thinker_cmd;

use crate::authorize::{self, Outcome};
use crate::log_tools;
use crate::redact::Redactor;
use crate::safety;
use crate::versioned::{EventRow, RevisionRow};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Mutex;

pub use scripted::ScriptedChatThinker;

pub const SRC_USER: &str = "chat:user";
pub const SRC_BEING: &str = "chat:being";
pub const SRC_TOOL: &str = "chat:tool";
pub const SRC_PENDING: &str = "chat:pending";
pub const SRC_HUMAN: &str = "chat:human";
pub const SRC_THINKER: &str = "thinker";
pub const PROTOCOL: &str = "kore-self-chat-1";
const MAX_TEXT_CHARS: usize = 4000;
const MAX_CONTEXT_BYTES: usize = 12_000;
const CONTEXT_MESSAGES: usize = 12;
const MAX_TOOL_CALLS: usize = 5;
const MAX_ARGS_BYTES: usize = 8192;
const MAX_RESULT_BYTES: usize = 4000;

static CHAT_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub tool: String,
    pub args: Value,
}

#[derive(Debug, Clone, Default)]
pub struct ChatReply {
    pub reply: String,
    pub tool_calls: Vec<ToolCall>,
    /// What the thinker printed, byte for byte (command thinkers only).
    pub raw: Option<String>,
}

/// Everything a thinker sees. Already redacted and size bounded.
#[derive(Debug, Clone)]
pub struct ChatContext {
    pub session: String,
    pub user_text: String,
    /// (role, text), oldest first, ends with the current user message. Roles: user, assistant, tool.
    pub messages: Vec<(String, String)>,
    pub facts: Vec<String>,
    /// (name, description) of the tools the policy lets the thinker propose.
    pub tools: Vec<(String, String)>,
}

impl ChatContext {
    /// The exact request a command thinker receives on stdin (and what is logged).
    pub fn request_json(&self) -> String {
        json!({
            "protocol": PROTOCOL,
            "session": self.session,
            "user": self.user_text,
            "messages": self.messages.iter().map(|(r, t)| json!({"role": r, "text": t})).collect::<Vec<_>>(),
            "facts": self.facts,
            "tools": self.tools.iter().map(|(n, d)| json!({"name": n, "description": d})).collect::<Vec<_>>(),
            "reply_format": "{\"reply\": \"text\", \"tool_calls\": [{\"tool\": \"name\", \"args\": {}}]}; tool calls are proposals, a separate policy decides",
        })
        .to_string()
    }
}

/// The seam. Same idea as `being::Thinker`, for conversation.
pub trait ChatThinker {
    fn id(&self) -> String;
    fn think(&self, ctx: &ChatContext) -> Result<ChatReply, String>;
}

fn tool_desc(name: &str) -> &'static str {
    match name {
        "self_sql" => "args {sql}: one read-only SELECT over events, revisions, beliefs_current",
        "self_belief_history" => "args {key}: every revision of a belief with its cited events",
        "being_status" => "args {}: status of the being loop",
        "self_event_add" => "args {text, tags?}: append a note to the being's own log",
        _ => "",
    }
}

fn valid_session(s: &str) -> Result<(), Value> {
    if (1..=32).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        Ok(())
    } else {
        Err(log_tools::err("'session' must be 1..32 characters of letters, digits, '_' or '-'"))
    }
}

fn session_arg(args: &Value) -> Result<String, Value> {
    let s = match args.get("session") {
        None | Some(Value::Null) => "default".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(_) => return Err(log_tools::err("'session' must be a string")),
    };
    valid_session(&s)?;
    Ok(s)
}

fn read_events(owner: &str) -> Result<(Vec<EventRow>, Vec<RevisionRow>), Value> {
    let _g = log_tools::LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let log = log_tools::open(owner)?;
    let snap = log.open_latest();
    Ok((snap.rows(), snap.rows()))
}

fn tag_of<'a>(tags: &'a str, key: &str) -> Option<&'a str> {
    tags.split(',').find_map(|t| t.strip_prefix(key).and_then(|r| r.strip_prefix(':')))
}

fn is_chat_message(e: &EventRow) -> bool {
    e.source == SRC_USER || e.source == SRC_BEING || e.source == SRC_TOOL
}

fn role_of(source: &str) -> &'static str {
    match source {
        SRC_USER => "user",
        SRC_BEING => "assistant",
        _ => "tool",
    }
}

fn append(owner: &str, source: &str, kind: &str, text: &str, session: &str, extra: &[String]) -> Result<String, Value> {
    let mut tags = vec![format!("session:{session}")];
    tags.extend(extra.iter().cloned());
    authorize::append_event(owner, source, kind, text, tags).map(|(id, _)| id).map_err(|e| log_tools::err(format!("cannot write to the log: {e}")))
}

/// Build the bounded, redacted context for one turn (the user's message is already logged).
pub fn build_context(owner: &str, session: &str, user_text: &str) -> Result<ChatContext, Value> {
    let (events, revs) = read_events(owner)?;
    let red = Redactor::load(&safety::owner_dir(owner));
    let r = |s: &str| red.redact(safety::trunc(s, 1500)).text;
    let mut msgs: Vec<(String, String)> = events
        .iter()
        .filter(|e| is_chat_message(e) && tag_of(&e.tags, "session") == Some(session))
        .map(|e| (role_of(&e.source).to_string(), r(&e.text)))
        .collect();
    let skip = msgs.len().saturating_sub(CONTEXT_MESSAGES);
    msgs.drain(..skip);
    // latest revision per (type, key) among beliefs and habits
    let mut latest: BTreeMap<(String, String), &RevisionRow> = BTreeMap::new();
    for rv in revs.iter().filter(|x| x.entity_type == "belief" || x.entity_type == "habit") {
        latest.insert((rv.entity_type.clone(), rv.key.clone()), rv);
    }
    let facts: Vec<String> = latest
        .values()
        .take(10)
        .map(|v| format!("{} {}: {} (confidence {:.2})", v.entity_type, r(&v.key), r(&v.stance), v.confidence))
        .collect();
    let tools = match authorize::load_policy(owner) {
        authorize::PolicyState::Ok(m) => m.keys().filter(|n| authorize::CHAT_TOOLS.iter().any(|(c, _)| c == n)).map(|n| (n.clone(), tool_desc(n).to_string())).collect(),
        _ => vec![],
    };
    let mut ctx = ChatContext { session: session.into(), user_text: r(user_text), messages: msgs, facts, tools };
    while ctx.request_json().len() > MAX_CONTEXT_BYTES && ctx.messages.len() > 1 {
        ctx.messages.remove(0);
    }
    while ctx.request_json().len() > MAX_CONTEXT_BYTES && !ctx.facts.is_empty() {
        ctx.facts.pop();
    }
    Ok(ctx)
}

fn result_text(v: &Value) -> (bool, String) {
    let t = v["content"][0]["text"].as_str().unwrap_or("").to_string();
    (v["isError"].as_bool().unwrap_or(false), t)
}

/// Run one allowed tool call. Only the four chat tools exist; arguments are
/// narrowed to the documented fields before the normal tool validation runs.
fn execute(owner: &str, tool: &str, args: &Value) -> Result<String, String> {
    let get = |k: &str| args.get(k).cloned();
    let (name, a) = match tool {
        "self_sql" => ("self_sql", json!({"sql": get("sql"), "as_of_version": get("as_of_version"), "as_of_time": get("as_of_time")})),
        "self_belief_history" => ("self_belief_history", json!({"key": get("key"), "entity_type": get("entity_type")})),
        "being_status" => ("being_status", json!({})),
        "self_event_add" => {
            let mut tags = vec![json!("via_chat")];
            if let Some(Value::Array(t)) = args.get("tags") {
                tags.extend(t.iter().take(8).cloned());
            }
            // source, kind and time are fixed here: the thinker cannot forge them
            ("self_event_add", json!({"text": get("text"), "occurred_at": crate::versioned::now_iso(), "source": "chat_note", "kind": "note", "tags": tags}))
        }
        _ => return Err("not a chat tool".into()),
    };
    let a = Value::Object(a.as_object().unwrap().iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k.clone(), v.clone())).collect());
    let r = log_tools::handle(name, &a, owner).ok_or("tool is not available")?;
    let (is_err, t) = result_text(&r);
    let t = safety::trunc(&t, MAX_RESULT_BYTES).to_string();
    if is_err { Err(t) } else { Ok(t) }
}

fn tool_row(owner: &str, session: &str, tool: &str, status: &str, detail_key: &str, detail: &str, extra: &[String]) -> Result<(), Value> {
    let body = json!({"tool": tool, "status": status, detail_key: detail}).to_string();
    let mut tags = vec![format!("status:{status}"), format!("tool:{}", authorize::tag_safe(tool))];
    tags.extend(extra.iter().cloned());
    append(owner, SRC_TOOL, "outcome", &body, session, &tags).map(|_| ())
}

/// Validate, authorize and (if allowed) run one proposed call. Returns the JSON summary for the caller.
fn handle_proposal(owner: &str, session: &str, call: &ToolCall) -> Result<Value, Value> {
    let tool = safety::trunc(&call.tool, 60);
    if !call.args.is_object() || call.args.to_string().len() > MAX_ARGS_BYTES {
        // audited as a denial: a non-object value is always refused by authorize
        let d = authorize::authorize(owner, "thinker", tool, &json!("invalid arguments (not an object, or over 8192 bytes)"));
        tool_row(owner, session, tool, "denied", "reason", &d.reason, &[])?;
        return Ok(json!({"tool": tool, "status": "denied", "reason": d.reason}));
    }
    let d = authorize::authorize(owner, "thinker", tool, &call.args);
    match d.outcome {
        Outcome::Deny => {
            tool_row(owner, session, tool, "denied", "reason", &d.reason, &[])?;
            Ok(json!({"tool": tool, "status": "denied", "reason": d.reason}))
        }
        Outcome::NeedsApproval => {
            let body = json!({"tool": tool, "args": call.args, "session": session, "reason": d.reason, "nonce": authorize::nonce()}).to_string();
            let id = append(owner, SRC_PENDING, "decision", &body, session, &[format!("tool:{}", authorize::tag_safe(tool))]) ?;
            tool_row(owner, session, tool, "needs_approval", "pending_id", &id, &[])?;
            Ok(json!({"tool": tool, "status": "needs_approval", "pending_id": id, "reason": d.reason}))
        }
        Outcome::Allow => match execute(owner, tool, &call.args) {
            Ok(t) => {
                tool_row(owner, session, tool, "ran", "result", &t, &[])?;
                Ok(json!({"tool": tool, "status": "ran", "result": t}))
            }
            Err(e) => {
                tool_row(owner, session, tool, "failed", "error", &e, &[])?;
                Ok(json!({"tool": tool, "status": "failed", "error": e}))
            }
        },
    }
}

fn human_gate(owner: &str, tool: &str) -> Result<(), Value> {
    let d = authorize::authorize(owner, authorize::HUMAN, tool, &json!({}));
    if d.outcome == Outcome::Allow { Ok(()) } else { Err(log_tools::err(format!("{tool} refused: {}", d.reason))) }
}

/// `chat_say`.
pub fn say(args: &Value, owner: &str) -> Result<Value, Value> {
    let text = log_tools::req_str(args, "text")?;
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(log_tools::err(format!("'text' is longer than {MAX_TEXT_CHARS} characters")));
    }
    let session = session_arg(args)?;
    human_gate(owner, "chat_say")?;
    let _g = CHAT_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    // Choose the thinker first: a broken thinker.txt refuses the turn, it never falls back silently.
    let cfg = thinker_cmd::load_config(owner).map_err(|e| log_tools::err(format!("thinker.txt problem, nothing was run: {e}")))?;
    let thinker: Box<dyn ChatThinker> = match cfg {
        Some(c) => Box::new(thinker_cmd::CommandThinker::new(c, owner).map_err(log_tools::err)?),
        None => Box::new(ScriptedChatThinker),
    };
    let tid = thinker.id();
    let user_id = {
        let mut t = text.to_string();
        let mut n = 1;
        loop {
            let (id, created) = authorize::append_event(owner, SRC_USER, "statement", &t, vec![format!("session:{session}")]).map_err(|e| log_tools::err(format!("cannot write to the log: {e}")))?;
            if created || n >= 5 {
                break id;
            }
            n += 1;
            t = format!("{text} (repeat {n})");
        }
    };
    let ctx = build_context(owner, &session, text)?;
    let prompt = ctx.request_json();
    let tag = [format!("thinker:{}", authorize::tag_safe(&tid))];
    let pbody = json!({"thinker": tid, "direction": "prompt", "user_event": user_id, "body": prompt, "nonce": authorize::nonce()}).to_string();
    append(owner, SRC_THINKER, "action", &pbody, &session, &[tag[0].clone(), "direction:prompt".into()])?;
    let out = thinker.think(&ctx);
    let reply = match out {
        Ok(r) => r,
        Err(e) => {
            let b = json!({"thinker": tid, "direction": "error", "error": safety::trunc(&e, 1000), "nonce": authorize::nonce()}).to_string();
            append(owner, SRC_THINKER, "outcome", &b, &session, &[tag[0].clone(), "direction:error".into()])?;
            return Err(log_tools::err(format!("the thinker failed (logged): {e}")));
        }
    };
    let rbody = json!({"thinker": tid, "direction": "reply", "user_event": user_id, "body": reply.raw.clone().unwrap_or_else(|| json!({"reply": reply.reply, "tool_calls": reply.tool_calls.iter().map(|c| json!({"tool": c.tool, "args": c.args})).collect::<Vec<_>>()}).to_string()), "nonce": authorize::nonce()}).to_string();
    append(owner, SRC_THINKER, "outcome", &rbody, &session, &[tag[0].clone(), "direction:reply".into()])?;
    let mut tools = Vec::new();
    for c in reply.tool_calls.iter().take(MAX_TOOL_CALLS) {
        tools.push(handle_proposal(owner, &session, c)?);
    }
    if reply.tool_calls.len() > MAX_TOOL_CALLS {
        tools.push(json!({"status": "dropped", "reason": format!("only {MAX_TOOL_CALLS} proposed calls are considered per turn; {} dropped", reply.tool_calls.len() - MAX_TOOL_CALLS)}));
    }
    let shown = if reply.reply.trim().is_empty() { "(no text; see the proposed tool calls)".to_string() } else { reply.reply.clone() };
    let being_id = append(owner, SRC_BEING, "statement", &shown, &session, &[])?;
    Ok(log_tools::text(json!({
        "session": session, "thinker": tid, "reply": shown, "tools": tools,
        "user_event_id": user_id, "reply_event_id": being_id,
    })))
}

/// `chat_history`.
pub fn history(args: &Value, owner: &str) -> Result<Value, Value> {
    let session = session_arg(args)?;
    let limit = match args.get("limit") {
        None | Some(Value::Null) => 20,
        Some(v) => v.as_u64().filter(|l| (1..=200).contains(l)).ok_or_else(|| log_tools::err("'limit' must be 1..200"))? as usize,
    };
    human_gate(owner, "chat_history")?;
    let (events, _) = read_events(owner)?;
    let mut rows: Vec<Value> = events
        .iter()
        .filter(|e| is_chat_message(e) && tag_of(&e.tags, "session") == Some(session.as_str()))
        .map(|e| json!({"event_id": e.event_id, "occurred_at": e.occurred_at, "role": role_of(&e.source), "text": e.text}))
        .collect();
    let skip = rows.len().saturating_sub(limit);
    rows.drain(..skip);
    Ok(log_tools::text(json!({"session": session, "messages": rows.len(), "history": rows})))
}

struct Pending {
    id: String,
    session: String,
    tool: String,
    args: Value,
    at: String,
}

fn pending_items(events: &[EventRow]) -> (Vec<Pending>, std::collections::HashSet<String>) {
    let resolved: std::collections::HashSet<String> = events
        .iter()
        .filter(|e| e.source == SRC_HUMAN)
        .filter_map(|e| serde_json::from_str::<Value>(&e.text).ok())
        .filter_map(|v| v["pending_id"].as_str().map(|s| s.to_string()))
        .collect();
    let items = events
        .iter()
        .filter(|e| e.source == SRC_PENDING)
        .filter_map(|e| {
            let v: Value = serde_json::from_str(&e.text).ok()?;
            Some(Pending { id: e.event_id.clone(), session: v["session"].as_str()?.to_string(), tool: v["tool"].as_str()?.to_string(), args: v["args"].clone(), at: e.occurred_at.clone() })
        })
        .collect();
    (items, resolved)
}

/// `chat_pending`.
pub fn pending(_args: &Value, owner: &str) -> Result<Value, Value> {
    human_gate(owner, "chat_pending")?;
    let (events, _) = read_events(owner)?;
    let (items, resolved) = pending_items(&events);
    let open: Vec<Value> = items.iter().filter(|p| !resolved.contains(&p.id)).map(|p| json!({"id": p.id, "session": p.session, "tool": p.tool, "args": p.args, "proposed_at": p.at})).collect();
    Ok(log_tools::text(json!({"pending": open.len(), "items": open})))
}

/// `chat_approve`: only the human actor can reach this (authorize denies it to the thinker).
pub fn approve(args: &Value, owner: &str) -> Result<Value, Value> {
    let id = log_tools::req_str(args, "id")?;
    let yes = match args.get("approve") {
        Some(Value::Bool(b)) => *b,
        _ => return Err(log_tools::err("'approve' is required (boolean)")),
    };
    if id.len() < 8 {
        return Err(log_tools::err("'id' must be at least 8 characters"));
    }
    human_gate(owner, "chat_approve")?;
    let _g = CHAT_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (events, _) = read_events(owner)?;
    let (items, resolved) = pending_items(&events);
    let found: Vec<&Pending> = items.iter().filter(|p| p.id.starts_with(id)).collect();
    let p = match found.as_slice() {
        [] => return Err(log_tools::err(format!("no pending item '{id}'"))),
        [one] => *one,
        _ => return Err(log_tools::err(format!("'{id}' matches {} pending items; give more characters", found.len()))),
    };
    if resolved.contains(&p.id) {
        return Err(log_tools::err("that item was already decided"));
    }
    let body = json!({"pending_id": p.id, "approve": yes, "tool": p.tool, "nonce": authorize::nonce()}).to_string();
    append(owner, SRC_HUMAN, "decision", &body, &p.session, &[format!("approve:{yes}")])?;
    let pid = [format!("pending:{}", safety::trunc(&p.id, 16))];
    if !yes {
        tool_row(owner, &p.session, &p.tool, "rejected", "pending_id", &p.id, &pid)?;
        return Ok(log_tools::text(json!({"id": p.id, "approved": false, "tool": p.tool})));
    }
    let d = authorize::authorize_approved(owner, "thinker", &p.tool, &p.args);
    if d.outcome != Outcome::Allow {
        tool_row(owner, &p.session, &p.tool, "denied", "reason", &d.reason, &pid)?;
        return Ok(log_tools::text(json!({"id": p.id, "approved": true, "ran": false, "reason": d.reason})));
    }
    match execute(owner, &p.tool, &p.args) {
        Ok(t) => {
            tool_row(owner, &p.session, &p.tool, "ran", "result", &t, &pid)?;
            Ok(log_tools::text(json!({"id": p.id, "approved": true, "ran": true, "result": t})))
        }
        Err(e) => {
            tool_row(owner, &p.session, &p.tool, "failed", "error", &e, &pid)?;
            Ok(log_tools::text(json!({"id": p.id, "approved": true, "ran": false, "error": e})))
        }
    }
}
