//! The one choke point for every tool the chat (and so the thinker) can invoke.
//!
//! `authorize(owner, actor, tool, args)` returns Allow, Deny or NeedsApproval and
//! appends an audit event (source `authorize`) for every decision of a
//! non-human actor, and for every refusal of a human-only tool. Default is
//! DENY. The rules, in order:
//!
//! 1. `<owner dir>/KILL` exists: only read-only status tools can be allowed.
//! 2. Human-only tools (chat_say, chat_approve, chat_history, chat_pending) are
//!    allowed for the actor `human` and denied for anyone else, so a thinker
//!    cannot approve its own request or start a turn.
//! 3. A tool that is not in the built-in table `CHAT_TOOLS` is denied, whatever
//!    the policy file says.
//! 4. No policy file: deny. A policy file with any error: deny everything.
//! 5. A tool not listed in the policy: deny.
//! 6. Listed under L1 or L2: allow. Listed under L3: NeedsApproval until a
//!    human approval event exists (`authorize_approved`), never auto.
//!
//! Policy file `<owner dir>/autonomy_policy.txt`, edited by hand (the path is
//! reserved: no tool can read or write it):
//!
//! ```text
//! # comments start with '#'
//! [L1]            # suggest only: read-only tools, no effects
//! self_sql
//! [L2]            # low-risk effects: a note into the being's own log
//! self_event_add
//! [L3]            # always needs a human approval, never automatic
//! ```
//!
//! A tool may be listed under a tier at or above its built-in tier (a read-only
//! tool under L3 means "ask me every time"); listing it below its built-in tier
//! is a policy error.

use crate::redact::Redactor;
use crate::safety;
use crate::versioned::{self as vl, NewEvent};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub const POLICY_FILE: &str = "autonomy_policy.txt";
pub const KILL_FILE: &str = "KILL";
pub const SOURCE: &str = "authorize";
pub const HUMAN: &str = "human";
const MAX_POLICY_BYTES: u64 = 64 * 1024;

/// Tools a thinker may propose in A1, with their built-in tier (1 read-only, 2 note into own log).
pub const CHAT_TOOLS: [(&str, u8); 4] = [("self_sql", 1), ("self_belief_history", 1), ("being_status", 1), ("self_event_add", 2)];
/// Tools only the human actor may use.
pub const HUMAN_ONLY: [&str; 4] = ["chat_say", "chat_approve", "chat_history", "chat_pending"];
/// What still works while the kill switch file exists (read-only).
const KILL_ALLOWED: [&str; 3] = ["being_status", "chat_history", "chat_pending"];

pub fn is_reserved_source(source: &str) -> bool {
    source == SOURCE || source == "thinker" || source.starts_with("chat:")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Allow,
    Deny,
    NeedsApproval,
}

impl Outcome {
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Allow => "allow",
            Outcome::Deny => "deny",
            Outcome::NeedsApproval => "needs_approval",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Decision {
    pub outcome: Outcome,
    pub reason: String,
}

fn decision(outcome: Outcome, reason: impl Into<String>) -> Decision {
    Decision { outcome, reason: reason.into() }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PolicyState {
    Missing,
    Invalid(String),
    Ok(BTreeMap<String, u8>),
}

fn builtin_tier(tool: &str) -> Option<u8> {
    CHAT_TOOLS.iter().find(|(n, _)| *n == tool).map(|(_, t)| *t)
}

/// Parse the policy text. Any problem is an error (the caller then denies everything).
pub fn parse_policy(text: &str) -> Result<BTreeMap<String, u8>, String> {
    let mut map = BTreeMap::new();
    let mut tier: Option<u8> = None;
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let l = raw.trim().trim_start_matches('\u{feff}');
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if l.starts_with('[') {
            tier = Some(match l.to_ascii_uppercase().as_str() {
                "[L1]" => 1,
                "[L2]" => 2,
                "[L3]" => 3,
                _ => return Err(format!("line {n}: unknown section '{l}' (use [L1], [L2] or [L3])")),
            });
            continue;
        }
        let Some(t) = tier else {
            return Err(format!("line {n}: tool '{l}' appears before any [L1]/[L2]/[L3] section"));
        };
        let Some(min) = builtin_tier(l) else {
            return Err(format!("line {n}: '{}' is not a tool the chat can invoke (allowed names: {})", safety::trunc(l, 60), CHAT_TOOLS.map(|c| c.0).join(", ")));
        };
        if t < min {
            return Err(format!("line {n}: {l} needs at least tier L{min} but is listed under L{t}"));
        }
        if map.insert(l.to_string(), t).is_some() {
            return Err(format!("line {n}: tool '{l}' is listed twice"));
        }
    }
    Ok(map)
}

/// Where a hand-edited file is group/world-writable (unix). `None` if fine or not checkable.
pub fn perm_problem(path: &Path) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = std::fs::metadata(path) {
            if m.permissions().mode() & 0o022 != 0 {
                return Some(format!("{} is group- or world-writable; run chmod go-w on it", path.display()));
            }
        }
    }
    let _ = path;
    None
}

pub fn load_policy(owner: &str) -> PolicyState {
    let p = safety::owner_dir(owner).join(POLICY_FILE);
    let meta = match std::fs::metadata(&p) {
        Ok(m) => m,
        Err(_) => return PolicyState::Missing,
    };
    if meta.len() > MAX_POLICY_BYTES {
        return PolicyState::Invalid(format!("{POLICY_FILE} is larger than {MAX_POLICY_BYTES} bytes"));
    }
    if let Some(e) = perm_problem(&p) {
        return PolicyState::Invalid(e);
    }
    match std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| String::from_utf8(b).map_err(|_| "not valid UTF-8".to_string())) {
        Ok(t) => match parse_policy(&t) {
            Ok(m) => PolicyState::Ok(m),
            Err(e) => PolicyState::Invalid(e),
        },
        Err(e) => PolicyState::Invalid(format!("cannot read {POLICY_FILE}: {e}")),
    }
}

pub fn kill_active(owner: &str) -> bool {
    std::fs::symlink_metadata(safety::owner_dir(owner).join(KILL_FILE)).is_ok()
}

fn decide(owner: &str, actor: &str, tool: &str, args: &serde_json::Value, approved: bool) -> Decision {
    if !args.is_object() {
        return decision(Outcome::Deny, "arguments must be a JSON object");
    }
    let kill = kill_active(owner);
    if HUMAN_ONLY.contains(&tool) {
        if actor != HUMAN {
            return decision(Outcome::Deny, format!("{tool} is a human-only tool; actor '{actor}' may not use it"));
        }
        if kill && !KILL_ALLOWED.contains(&tool) {
            return decision(Outcome::Deny, "kill switch: the KILL file exists, only read-only status is allowed");
        }
        return decision(Outcome::Allow, "human actor");
    }
    if kill && !KILL_ALLOWED.contains(&tool) {
        return decision(Outcome::Deny, "kill switch: the KILL file exists, only read-only status is allowed");
    }
    if builtin_tier(tool).is_none() {
        return decision(Outcome::Deny, format!("'{}' is not a tool the chat can invoke (default deny)", safety::trunc(tool, 60)));
    }
    match load_policy(owner) {
        PolicyState::Missing => decision(Outcome::Deny, format!("no {POLICY_FILE}: default deny")),
        PolicyState::Invalid(e) => decision(Outcome::Deny, format!("policy error, everything denied: {e}")),
        PolicyState::Ok(m) => match m.get(tool) {
            None => decision(Outcome::Deny, format!("{tool} is not listed in {POLICY_FILE}")),
            Some(3) if approved => decision(Outcome::Allow, "listed under L3 and approved by the human"),
            Some(3) => decision(Outcome::NeedsApproval, "listed under L3: needs the human's approval (chat_approve)"),
            Some(t) => decision(Outcome::Allow, format!("listed under L{t}")),
        },
    }
}

static NONCE: AtomicU64 = AtomicU64::new(0);

/// A process-unique token so that two otherwise identical events are both stored.
pub fn nonce() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:x}-{}", t, NONCE.fetch_add(1, Ordering::Relaxed))
}

/// Keep only `[a-z0-9_:-]`, so a value is safe as a tag.
pub fn tag_safe(s: &str) -> String {
    let t: String = s.chars().take(40).map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c.to_ascii_lowercase() } else { '_' }).collect();
    if t.is_empty() { "_".into() } else { t }
}

/// Append one event (text is redacted unless `raw`). Takes the process log lock.
/// Returns (event_id, created).
pub fn append_event(owner: &str, source: &str, kind: &str, text: &str, tags: Vec<String>) -> Result<(String, bool), String> {
    let _g = crate::log_tools::LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut log = crate::log_tools::open(owner).map_err(|v| v["content"][0]["text"].as_str().unwrap_or("log error").to_string())?;
    let red = Redactor::load(&safety::owner_dir(owner)).redact(text);
    let ev = NewEvent { source: source.into(), occurred_at: vl::now_iso(), kind: kind.into(), text: red.text.clone(), tags, redacted: red.changed() };
    let info = log.append_events(&[ev]).map_err(|e| e.to_string())?;
    Ok((info.event_ids.first().cloned().unwrap_or_default(), info.created))
}

fn audit(owner: &str, actor: &str, tool: &str, args: &serde_json::Value, d: &Decision) -> Result<(), String> {
    let a = safety::trunc(&args.to_string(), 300).to_string();
    let body = serde_json::json!({
        "actor": safety::trunc(actor, 40), "tool": safety::trunc(tool, 60), "decision": d.outcome.name(),
        "reason": safety::trunc(&d.reason, 300), "args": a, "nonce": nonce(),
    });
    let tags = vec![format!("decision:{}", d.outcome.name()), format!("tool:{}", tag_safe(tool)), format!("actor:{}", tag_safe(actor))];
    append_event(owner, SOURCE, "decision", &body.to_string(), tags).map(|_| ())
}

fn finish(owner: &str, actor: &str, tool: &str, args: &serde_json::Value, d: Decision) -> Decision {
    let read_only_human = actor == HUMAN && matches!(tool, "chat_history" | "chat_pending") && d.outcome == Outcome::Allow;
    if read_only_human {
        return d;
    }
    match audit(owner, actor, tool, args, &d) {
        Ok(()) => d,
        // no audit row, no action
        Err(e) => decision(Outcome::Deny, format!("audit event could not be written ({e}): denied")),
    }
}

/// Decide, and write the audit row. Fails closed if the row cannot be written.
pub fn authorize(owner: &str, actor: &str, tool: &str, args: &serde_json::Value) -> Decision {
    let d = decide(owner, actor, tool, args, false);
    finish(owner, actor, tool, args, d)
}

/// Same, for a tool call whose NeedsApproval item the human has approved (checked by the caller).
/// The kill switch and the policy are checked again.
pub fn authorize_approved(owner: &str, actor: &str, tool: &str, args: &serde_json::Value) -> Decision {
    let d = decide(owner, actor, tool, args, true);
    finish(owner, actor, tool, args, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn setup(name: &str) -> String {
        safety::test_support::init();
        let dir = safety::owner_dir(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        name.to_string()
    }
    fn write_policy(o: &str, t: &str) {
        std::fs::write(safety::owner_dir(o).join(POLICY_FILE), t).unwrap();
    }
    fn audit_rows(o: &str) -> Vec<vl::EventRow> {
        let log = crate::log_tools::open(o).unwrap();
        log.open_latest().rows::<vl::EventRow>().into_iter().filter(|e| e.source == SOURCE).collect()
    }

    #[test]
    fn default_deny_without_policy() {
        let o = setup("auth-default");
        for (t, _) in CHAT_TOOLS {
            let d = authorize(&o, "thinker", t, &json!({}));
            assert_eq!(d.outcome, Outcome::Deny, "{t}");
            assert!(d.reason.contains("default deny"));
        }
        assert_eq!(authorize(&o, "thinker", "self_forget", &json!({})).outcome, Outcome::Deny);
        assert_eq!(audit_rows(&o).len(), CHAT_TOOLS.len() + 1);
    }

    #[test]
    fn policy_parse_errors_fail_closed() {
        for bad in [
            "self_sql\n",
            "[L9]\nself_sql\n",
            "[L1]\nself_forget\n",
            "[L1]\nself_event_add\n",
            "[L1]\nself_sql\n[L2]\nself_sql\n",
            "[L1]\nself_sql extra\n",
            "[L1]\nchat_approve\n",
        ] {
            assert!(parse_policy(bad).is_err(), "{bad:?}");
        }
        let o = setup("auth-badpolicy");
        write_policy(&o, "[L1]\nself_sql\nnot_a_tool\n");
        let d = authorize(&o, "thinker", "self_sql", &json!({}));
        assert_eq!(d.outcome, Outcome::Deny);
        assert!(d.reason.contains("policy error"), "{}", d.reason);
        write_policy(&o, "[L1]\n\u{0}\u{0}garbage");
        assert_eq!(authorize(&o, "thinker", "self_sql", &json!({})).outcome, Outcome::Deny);
    }

    #[test]
    fn tiers_behave() {
        let o = setup("auth-tiers");
        write_policy(&o, "# mine\n[L1]\nself_sql\nbeing_status\n[L2]\nself_event_add\n[L3]\nself_belief_history\n");
        assert_eq!(authorize(&o, "thinker", "self_sql", &json!({})).outcome, Outcome::Allow);
        assert_eq!(authorize(&o, "thinker", "self_event_add", &json!({})).outcome, Outcome::Allow);
        assert_eq!(authorize(&o, "thinker", "self_belief_history", &json!({})).outcome, Outcome::NeedsApproval);
        assert_eq!(authorize_approved(&o, "thinker", "self_belief_history", &json!({})).outcome, Outcome::Allow);
        // approval does not widen the policy
        assert_eq!(authorize_approved(&o, "thinker", "self_forget", &json!({})).outcome, Outcome::Deny);
        let o2 = "auth-tiers".to_string();
        write_policy(&o2, "[L1]\nbeing_status\n");
        assert_eq!(authorize_approved(&o2, "thinker", "self_belief_history", &json!({})).outcome, Outcome::Deny);
        assert_eq!(authorize(&o, "thinker", "self_sql", &json!("not an object")).outcome, Outcome::Deny);
    }

    #[test]
    fn thinker_cannot_use_human_tools_and_kill_switch_works() {
        let o = setup("auth-kill");
        write_policy(&o, "[L1]\nself_sql\nbeing_status\n[L2]\nself_event_add\n");
        assert_eq!(authorize(&o, "thinker", "chat_approve", &json!({})).outcome, Outcome::Deny);
        assert_eq!(authorize(&o, "thinker", "chat_say", &json!({})).outcome, Outcome::Deny);
        assert_eq!(authorize(&o, HUMAN, "chat_say", &json!({})).outcome, Outcome::Allow);
        std::fs::write(safety::owner_dir(&o).join(KILL_FILE), "").unwrap();
        assert_eq!(authorize(&o, "thinker", "self_sql", &json!({})).outcome, Outcome::Deny);
        assert_eq!(authorize(&o, "thinker", "self_event_add", &json!({})).outcome, Outcome::Deny);
        assert_eq!(authorize(&o, HUMAN, "chat_say", &json!({})).outcome, Outcome::Deny);
        assert_eq!(authorize(&o, HUMAN, "chat_approve", &json!({})).outcome, Outcome::Deny);
        assert_eq!(authorize(&o, "thinker", "being_status", &json!({})).outcome, Outcome::Allow);
        assert_eq!(authorize(&o, HUMAN, "chat_history", &json!({})).outcome, Outcome::Allow);
        // a read-only status tool still needs the policy to list it
        write_policy(&o, "[L1]\nself_sql\n");
        assert_eq!(authorize(&o, "thinker", "being_status", &json!({})).outcome, Outcome::Deny);
        std::fs::remove_file(safety::owner_dir(&o).join(KILL_FILE)).unwrap();
        assert_eq!(authorize(&o, "thinker", "self_sql", &json!({})).outcome, Outcome::Allow);
    }

    #[test]
    fn every_decision_is_audited() {
        let o = setup("auth-audit");
        write_policy(&o, "[L1]\nself_sql\n[L3]\nself_event_add\n");
        authorize(&o, "thinker", "self_sql", &json!({"sql": "SELECT 1"}));
        authorize(&o, "thinker", "self_event_add", &json!({"text": "mail me at a.b@example.com"}));
        authorize(&o, "thinker", "self_forget", &json!({}));
        authorize(&o, "thinker", "self_sql", &json!({"sql": "SELECT 1"})); // identical call: still its own row
        let rows = audit_rows(&o);
        assert_eq!(rows.len(), 4);
        let kinds: Vec<&str> = rows.iter().map(|r| r.tags.split(',').next().unwrap()).collect();
        assert!(kinds.contains(&"decision:allow") && kinds.contains(&"decision:needs_approval") && kinds.contains(&"decision:deny"));
        assert!(rows.iter().all(|r| !r.text.contains("a.b@example.com")), "audit args are redacted");
    }

    #[test]
    fn reserved_names_and_sources() {
        for n in ["kill", "kill.", "kill:x", "autonomy_policy.txt", "autonomy_policy.txt.bak", "thinker.txt", "thinker.txt.new"] {
            assert!(safety::is_hand_edited_name(n), "{n}");
        }
        assert!(!safety::is_hand_edited_name("killer.txt") && !safety::is_hand_edited_name("notes.txt"));
        assert!(is_reserved_source("authorize") && is_reserved_source("thinker") && is_reserved_source("chat:user"));
        assert!(!is_reserved_source("mcp"));
    }
}
