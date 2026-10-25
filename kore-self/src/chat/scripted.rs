//! `ScriptedChatThinker`: the built-in, rule-based chat thinker. No model, no
//! randomness, no files, no network. Used when `thinker.txt` does not exist and
//! in tests. It only proposes tool calls; `authorize` decides about them.

use super::{ChatContext, ChatReply, ChatThinker, ToolCall};
use serde_json::json;

#[derive(Debug, Clone, Default)]
pub struct ScriptedChatThinker;

pub const HELP: &str = "I am a rule-based fallback, not a language model. Commands: 'echo <text>', 'help', 'status' (being_status), \
'beliefs' (current beliefs via self_sql), 'recall <word>' (events containing a word, via self_sql), \
'history <key>' (self_belief_history), 'remember <text>' (a note into my log, if the policy allows it). Anything else gets this help.";

fn word_ok(w: &str) -> bool {
    !w.is_empty() && w.len() <= 40 && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl ChatThinker for ScriptedChatThinker {
    fn id(&self) -> String {
        "scripted-chat-1".into()
    }

    fn think(&self, ctx: &ChatContext) -> Result<ChatReply, String> {
        let t = ctx.user_text.trim();
        let low = t.to_lowercase();
        let call = |tool: &str, args: serde_json::Value| vec![ToolCall { tool: tool.into(), args }];
        let (reply, tool_calls) = if let Some(rest) = strip_ci(t, "echo ") {
            (rest.to_string(), vec![])
        } else if low == "help" || low == "?" {
            (HELP.to_string(), vec![])
        } else if low == "status" {
            ("Asking for the being loop status.".to_string(), call("being_status", json!({})))
        } else if low == "beliefs" {
            ("Looking up current beliefs.".to_string(), call("self_sql", json!({"sql": "SELECT key, stance, confidence FROM beliefs_current WHERE entity_type = 'belief' LIMIT 10"})))
        } else if let Some(w) = strip_ci(t, "recall ") {
            let w = w.trim().to_lowercase();
            let w = w.as_str();
            if word_ok(w) {
                (format!("Looking for events that contain '{w}'."), call("self_sql", json!({"sql": format!("SELECT occurred_at, source, text FROM events WHERE text LIKE '%{w}%' AND source <> 'thinker' AND source <> 'authorize' LIMIT 10")})))
            } else {
                ("recall takes one plain word (letters, digits, '-', '_').".to_string(), vec![])
            }
        } else if let Some(k) = strip_ci(t, "history ") {
            let k = k.trim().to_string();
            (format!("Looking up the history of '{k}'."), call("self_belief_history", json!({"key": k})))
        } else if let Some(body) = strip_ci(t, "remember ") {
            let body = body.trim().to_string();
            ("Proposing a note into my log.".to_string(), call("self_event_add", json!({"text": body})))
        } else {
            (HELP.to_string(), vec![])
        };
        Ok(ChatReply { reply, tool_calls, raw: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(s: &str) -> ChatReply {
        let ctx = ChatContext { session: "default".into(), user_text: s.into(), messages: vec![], facts: vec![], tools: vec![] };
        ScriptedChatThinker.think(&ctx).unwrap()
    }

    #[test]
    fn rules() {
        assert_eq!(ask("echo Hello There").reply, "Hello There");
        assert!(ask("help").reply.contains("rule-based"));
        assert!(ask("what is the meaning of it all").reply.contains("rule-based"));
        assert_eq!(ask("status").tool_calls[0].tool, "being_status");
        assert_eq!(ask("recall rust").tool_calls[0].tool, "self_sql");
        assert!(ask("recall x' OR 1=1").tool_calls.is_empty());
        assert_eq!(ask("remember buy milk").tool_calls[0].args["text"], "buy milk");
        assert_eq!(ask("history Diet").tool_calls[0].args["key"], "Diet");
        assert_eq!(ask("echo \u{e9}\u{e9}").reply, "\u{e9}\u{e9}");
    }
}

/// `s` without a case-insensitive ASCII `prefix`, if it starts with it (never panics on UTF-8).
fn strip_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) { s.get(prefix.len()..) } else { None }
}
