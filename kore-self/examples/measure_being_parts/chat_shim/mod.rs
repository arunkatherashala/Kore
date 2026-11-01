//! Stand-in for `src/chat/mod.rs` inside the measurement example: the REAL `thinker_cmd.rs` and
//! `advisor_cmd.rs` are compiled unchanged (so the pilot runs the real `CommandAdvisor`); only the chat
//! types they import are redefined here (they are not used on the advisor path).
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub tool: String,
    pub args: Value,
}

#[derive(Debug, Clone, Default)]
pub struct ChatReply {
    pub reply: String,
    pub tool_calls: Vec<ToolCall>,
    pub raw: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChatContext {
    pub session: String,
    pub user_text: String,
    pub messages: Vec<(String, String)>,
    pub facts: Vec<String>,
    pub tools: Vec<(String, String)>,
}

impl ChatContext {
    pub fn request_json(&self) -> String {
        String::new()
    }
}

pub trait ChatThinker {
    fn id(&self) -> String;
    fn think(&self, ctx: &ChatContext) -> Result<ChatReply, String>;
}

#[path = "../../../src/chat/thinker_cmd.rs"]
pub mod thinker_cmd;
#[path = "../../../src/chat/advisor_cmd.rs"]
pub mod advisor_cmd;
