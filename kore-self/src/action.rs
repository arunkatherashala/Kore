//! Action Bridge — KORE's life-needs to engine-actions translator.
//!
//! KORE's [`becoming::NeedEngine`] describes what KORE wants to become:
//! learn, create, explore, understand, improve, contribute, and evolve.
//! The `ActionBridge` turns those emergent needs into concrete [`EngineAction`]
//! instances, executes them through a [`KoreBody`] (the KORE SQL engine + memory
//! DataBlock), and returns an [`ActionResult`] so `KoreSelf` can record the outcome.
//!
//! This keeps the "life layer" (needs) separate from the "motor layer"
//! (actions), so either side can evolve without breaking the other.

use crate::becoming::{LifecycleStage, NeedEngine, StoryKind};
use crate::Memory;
use kore_body::{BodyCommand, BodyResult, KoreBody};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Engine Action Types ────────────────────────────────────────────────────

/// A concrete action the KORE engine can perform on behalf of a life need.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EngineAction {
    Learn { topic: String },
    Create { prompt: String },
    Explore { direction: String },
    Understand { question: String },
    Improve { target: String },
    Contribute { channel: String },
    Evolve { aspiration: String },
}

impl EngineAction {
    /// The need name that this action is meant to satisfy.
    pub fn need_name(&self) -> &'static str {
        match self {
            Self::Learn { .. } => "learn",
            Self::Create { .. } => "create",
            Self::Explore { .. } => "explore",
            Self::Understand { .. } => "understand",
            Self::Improve { .. } => "improve",
            Self::Contribute { .. } => "contribute",
            Self::Evolve { .. } => "evolve",
        }
    }

    /// A short human-readable label for logging/story entries.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Learn { .. } => "Learn",
            Self::Create { .. } => "Create",
            Self::Explore { .. } => "Explore",
            Self::Understand { .. } => "Understand",
            Self::Improve { .. } => "Improve",
            Self::Contribute { .. } => "Contribute",
            Self::Evolve { .. } => "Evolve",
        }
    }

    /// The kind of story entry this action produces.
    pub fn story_kind(&self) -> StoryKind {
        match self {
            Self::Learn { .. } => StoryKind::Wisdom,
            Self::Create { .. } => StoryKind::Evolution,
            Self::Explore { .. } => StoryKind::Discovery,
            Self::Understand { .. } => StoryKind::Wisdom,
            Self::Improve { .. } => StoryKind::Evolution,
            Self::Contribute { .. } => StoryKind::Legacy,
            Self::Evolve { .. } => StoryKind::Evolution,
        }
    }
}

// ─── Action Result / State ─────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionResult {
    pub success: bool,
    pub outcome: String,
    pub memory_summary: String,
    pub tool_name: String,
    pub error_detail: Option<String>,
    pub fallback_used: bool,
}

impl ActionResult {
    /// Suggested story text for this result.
    pub fn story_text(&self, action: &EngineAction) -> String {
        let fallback = if self.fallback_used { " (fallback query)" } else { "" };
        format!(
            "{} executed via {}{}. Outcome: {}.",
            action.label(),
            self.tool_name,
            fallback,
            self.outcome
        )
    }

    /// Suggested importance for the memory recording.
    pub fn importance(&self) -> f64 {
        if self.success { 0.7 } else { 0.4 }
    }

    /// Suggested need satisfaction amount.
    pub fn satisfaction_amount(&self) -> f64 {
        if self.success { 0.20 } else { 0.05 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionState {
    pub memory_count: usize,
    pub dominant_need: String,
    pub lifecycle_stage: LifecycleStage,
    pub last_action_tick: u64,
}

impl Default for ActionState {
    fn default() -> Self {
        Self {
            memory_count: 0,
            dominant_need: String::new(),
            lifecycle_stage: LifecycleStage::Birth,
            last_action_tick: 0,
        }
    }
}

// ─── Action Bridge ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionBridge {
    action_count: u64,
    success_count: u64,
    failure_count: u64,
    fallback_count: u64,
    per_need: HashMap<String, (u64, u64)>, // need -> (success, failure)
}

impl ActionBridge {
    pub fn new() -> Self {
        Self {
            action_count: 0,
            success_count: 0,
            failure_count: 0,
            fallback_count: 0,
            per_need: HashMap::new(),
        }
    }

    /// Record the result of an executed action for learning/summary.
    fn record_result(&mut self, need: &str, success: bool, fallback_used: bool) {
        self.action_count += 1;
        if success {
            self.success_count += 1;
        } else {
            self.failure_count += 1;
        }
        if fallback_used {
            self.fallback_count += 1;
        }
        let entry = self.per_need.entry(need.to_string()).or_insert((0, 0));
        if success {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }

    /// Human-readable summary of action history.
    pub fn summary(&self) -> String {
        let total = self.action_count;
        if total == 0 {
            return "No actions executed yet.".to_string();
        }
        let rate = (self.success_count as f64 / total as f64) * 100.0;
        let per_need_lines: Vec<String> = self.per_need.iter()
            .map(|(need, (s, f))| {
                let t = s + f;
                let r = if t == 0 { 0.0 } else { (*s as f64 / t as f64) * 100.0 };
                format!("  {:12} : {} success / {} fail  ({:.0}%)", need, s, f, r)
            })
            .collect();
        format!(
            "Action Bridge Summary\n\
             Total actions: {}\n\
             Success: {} | Failure: {} | Rate: {:.0}%\n\
             Fallbacks used: {}\n\
             Per need:\n{}",
            total, self.success_count, self.failure_count, rate,
            self.fallback_count,
            per_need_lines.join("\n")
        )
    }

    /// Pick the dominant emergent need and map it to a concrete engine action.
    pub fn select(&self, needs: &NeedEngine, state: &ActionState) -> EngineAction {
        let (need, _level) = needs.most_urgent();

        match need {
            "learn" => EngineAction::Learn {
                topic: format!("what surprised me in {:?}", state.lifecycle_stage),
            },
            "create" => EngineAction::Create {
                prompt: "synthesize a new idea from memory".to_string(),
            },
            "explore" => EngineAction::Explore {
                direction: "unvisited memory kind".to_string(),
            },
            "understand" => EngineAction::Understand {
                question: "why does this pattern keep appearing".to_string(),
            },
            "improve" => EngineAction::Improve {
                target: "self performance and clarity".to_string(),
            },
            "contribute" => EngineAction::Contribute {
                channel: "broadcast".to_string(),
            },
            "evolve" => EngineAction::Evolve {
                aspiration: "next lifecycle stage".to_string(),
            },
            _ => EngineAction::Create {
                prompt: "create something meaningful".to_string(),
            },
        }
    }

    /// Execute an action against the body and return the result.
    /// Tries a primary query first; if it fails, tries a simpler fallback.
    /// The caller (KoreSelf) is responsible for recording the result in memory,
    /// story, and needs.
    pub fn execute(
        &mut self,
        action: EngineAction,
        body: &mut dyn KoreBody,
        memories: &[Memory],
    ) -> ActionResult {
        // Keep the body in sync with KORE's current memory set.
        let mem_block = crate::kore_query::memories_to_block(memories);
        let _ = body.act(BodyCommand::LoadTable {
            name: "memories".to_string(),
            block: mem_block,
        });

        let (primary, fallback, tool_name) = action_query_plan(&action);

        // Try primary, then fallback.
        let (query_result, fallback_used) = match body.act(BodyCommand::Query { sql: primary.to_string() }) {
            Ok(BodyResult { success: true, data_block: Some(block), .. }) => (Ok(block), false),
            Ok(_) => {
                let detail = "primary query returned no data block".to_string();
                (Err(detail), false)
            }
            Err(primary_err) => {
                if let Some(fb_sql) = fallback {
                    match body.act(BodyCommand::Query { sql: fb_sql.to_string() }) {
                        Ok(BodyResult { success: true, data_block: Some(block), .. }) => (Ok(block), true),
                        Ok(_) => {
                            let detail = format!("primary: {primary_err}; fallback returned no data block");
                            (Err(detail), true)
                        }
                        Err(fb_err) => {
                            let detail = format!("primary: {primary_err}; fallback: {fb_err}");
                            (Err(detail), true)
                        }
                    }
                } else {
                    (Err(primary_err.to_string()), false)
                }
            }
        };

        let success = query_result.is_ok();
        self.record_result(action.need_name(), success, fallback_used);

        let (memory_summary, outcome, error_detail) = match query_result {
            Ok(block) => {
                let fb_note = if fallback_used { " (fallback)" } else { "" };
                let summary = format!(
                    "{} action produced a DataBlock with {} rows and {} columns via {}{}.",
                    action.label(),
                    block.num_rows,
                    block.columns.len(),
                    tool_name,
                    fb_note
                );
                (summary, format!("query succeeded: {} rows{}", block.num_rows, fb_note), None)
            }
            Err(detail) => {
                let summary = format!(
                    "{} action failed to query the body: {}.",
                    action.label(),
                    detail
                );
                (summary, format!("query failed: {}", detail), Some(detail))
            }
        };

        ActionResult {
            success,
            outcome,
            memory_summary,
            tool_name: tool_name.to_string(),
            error_detail,
            fallback_used,
        }
    }

    /// Convenience: select + execute in one call.
    pub fn select_and_execute(
        &mut self,
        needs: &NeedEngine,
        state: &ActionState,
        body: &mut dyn KoreBody,
        memories: &[Memory],
    ) -> (EngineAction, ActionResult) {
        let action = self.select(needs, state);
        let result = self.execute(action.clone(), body, memories);
        (action, result)
    }
}

// ─── Helper Functions ───────────────────────────────────────────────────────

/// Select a primary query, optional fallback query, and tool name for each action.
/// Fallbacks are intentionally simple so the action still succeeds even when the
/// primary query hits a parser/schema edge case.
fn action_query_plan(action: &EngineAction) -> (&'static str, Option<&'static str>, &'static str) {
    match action {
        EngineAction::Learn { .. } => (
            "SELECT kind, COUNT(*) AS cnt FROM memories GROUP BY kind ORDER BY cnt ASC LIMIT 5",
            Some("SELECT kind FROM memories LIMIT 5"),
            "self_query",
        ),
        EngineAction::Create { .. } => (
            "SELECT kind, content, importance FROM memories ORDER BY importance DESC LIMIT 3",
            Some("SELECT kind, content, importance FROM memories LIMIT 1"),
            "synthesis",
        ),
        EngineAction::Explore { .. } => (
            "SELECT DISTINCT kind FROM memories",
            Some("SELECT kind FROM memories LIMIT 5"),
            "explore",
        ),
        EngineAction::Understand { .. } => (
            "SELECT kind, content, importance FROM memories WHERE importance >= 0.8 ORDER BY importance DESC LIMIT 5",
            Some("SELECT kind, content, importance FROM memories LIMIT 5"),
            "understand",
        ),
        EngineAction::Improve { .. } => (
            "SELECT kind, content, importance FROM memories WHERE kind IN ('benchmark','decision','performance','optimization') ORDER BY importance DESC LIMIT 5",
            Some("SELECT kind, content, importance FROM memories LIMIT 5"),
            "improve",
        ),
        EngineAction::Contribute { .. } => (
            "SELECT kind, content FROM memories ORDER BY id DESC LIMIT 3",
            Some("SELECT kind, content FROM memories LIMIT 1"),
            "broadcast",
        ),
        EngineAction::Evolve { .. } => (
            "SELECT kind, COUNT(*) AS cnt FROM memories GROUP BY kind ORDER BY cnt DESC",
            Some("SELECT kind FROM memories LIMIT 5"),
            "evolve",
        ),
    }
}
