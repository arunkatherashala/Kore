//! Tools of the chat (A1): chat_say, chat_history, chat_pending, chat_approve.
//!
//! They are for the human (the MCP caller). A thinker can never call them:
//! `authorize` denies them to every actor but `human`.

use crate::chat;
use serde_json::{json, Value};

pub const TOOLS: [&str; 4] = ["chat_say", "chat_history", "chat_pending", "chat_approve"];

/// Handle a chat tool; None for any other name. Takes no global lock itself.
pub fn handle(name: &str, args: &Value, owner: &str) -> Option<Value> {
    if !TOOLS.contains(&name) {
        return None;
    }
    if let Err(e) = crate::safety::validate_owner(owner) {
        return Some(crate::log_tools::err(format!("invalid owner: {e}")));
    }
    let r = match name {
        "chat_say" => chat::say(args, owner),
        "chat_history" => chat::history(args, owner),
        "chat_pending" => chat::pending(args, owner),
        _ => chat::approve(args, owner),
    };
    Some(r.unwrap_or_else(|e| e))
}

pub fn tool_defs() -> Vec<Value> {
    vec![
        json!({ "name": "chat_say",
          "description": "Say something to the being and get its reply. The message and the reply are events in its log (sources chat:user and chat:being). The reply comes from the built-in rule-based thinker, or from the program configured in thinker.txt if you created that file by hand. Tool calls the thinker proposes run only if autonomy_policy.txt allows them; every decision is audited",
          "inputSchema": { "type": "object", "properties": {
            "text": { "type": "string", "description": "Your message (at most 4000 characters)" },
            "session": { "type": "string", "description": "Conversation name, default 'default'" }
          }, "required": ["text"] } }),
        json!({ "name": "chat_history",
          "description": "Read-only: the last messages of a conversation (yours, the being's replies and tool results), from the log",
          "inputSchema": { "type": "object", "properties": {
            "session": { "type": "string", "description": "Conversation name, default 'default'" },
            "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "Default 20" }
          } } }),
        json!({ "name": "chat_pending",
          "description": "Read-only: tool calls the thinker proposed that need your approval (listed under L3 in autonomy_policy.txt) and that you have not decided yet",
          "inputSchema": { "type": "object", "properties": {} } }),
        json!({ "name": "chat_approve",
          "description": "Approve or reject one pending tool call by id (from chat_pending). Your decision is recorded as an event; an approved call runs once, after the policy and the kill switch are checked again. Only you can call this, the thinker cannot",
          "inputSchema": { "type": "object", "properties": {
            "id": { "type": "string", "description": "Pending item id (or a unique prefix of at least 8 characters)" },
            "approve": { "type": "boolean" }
          }, "required": ["id", "approve"] } }),
    ]
}
