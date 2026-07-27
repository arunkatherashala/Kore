//! KORE-GOALS — Self-directed mission engine.
//!
//! KORE does not just react to needs. It turns needs into missions,
//! pursues them across multiple heartbeats, and marks them complete when
//! the underlying need is satisfied.
//!
//! A goal is a promise KORE makes to itself: "I will become X".
//! Goals emerge from:
//!   - dominant life needs (learn, create, explore, understand, improve, contribute, evolve)
//!   - user direction ("set a goal for me")
//!   - reflection over memory patterns (e.g. many "wish" memories become a goal)
//!
//! Goals are not tasks. They are vectors of becoming.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::action::EngineAction;
use crate::becoming::{LifecycleStage, NeedEngine};
use crate::Memory;

// ─── Goal Status ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GoalStatus {
    Active,
    Completed,
    Abandoned,
}

// ─── Goal ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: u64,
    pub name: String,
    pub description: String,
    /// The life-need this goal is meant to satisfy.
    pub need: String,
    /// 0.0–1.0. Higher = more urgent. Derived from need level + novelty + user weight.
    pub priority: f64,
    pub status: GoalStatus,
    /// 0.0–1.0. Completion estimate based on attempts and need satisfaction.
    pub progress: f64,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    /// How many engine actions were executed in service of this goal.
    pub attempts: u64,
    /// How many of those actions succeeded.
    pub successes: u64,
    /// Where this goal came from: "emergent", "user", "reflection", "dream".
    pub source: String,
    /// Lifecycle stage when the goal was born.
    pub lifecycle_stage: String,
    /// Free-form tags for clustering related goals.
    pub tags: Vec<String>,
}

impl Goal {
    fn new(
        id: u64,
        name: String,
        description: String,
        need: String,
        priority: f64,
        source: String,
        lifecycle_stage: String,
        now: &str,
        tags: Vec<String>,
    ) -> Self {
        Self {
            id,
            name,
            description,
            need,
            priority,
            status: GoalStatus::Active,
            progress: 0.0,
            created_at: now.to_string(),
            updated_at: now.to_string(),
            completed_at: None,
            attempts: 0,
            successes: 0,
            source,
            lifecycle_stage,
            tags,
        }
    }

    /// Success rate for this goal, 0.0–1.0.
    pub fn success_rate(&self) -> f64 {
        if self.attempts == 0 {
            0.0
        } else {
            self.successes as f64 / self.attempts as f64
        }
    }

    /// One-line status for reports.
    pub fn line(&self) -> String {
        format!(
            "[{}] {} ({})  progress {:.0}%  prio {:.2}  attempts {}  success {:.0}%",
            self.id,
            self.name,
            status_label(&self.status),
            self.progress * 100.0,
            self.priority,
            self.attempts,
            self.success_rate() * 100.0
        )
    }
}

fn status_label(s: &GoalStatus) -> &'static str {
    match s {
        GoalStatus::Active => "active",
        GoalStatus::Completed => "done",
        GoalStatus::Abandoned => "abandoned",
    }
}

// ─── Goal Engine ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoalEngine {
    goals: Vec<Goal>,
    next_id: u64,
    /// IDs of goals currently being pursued. Ordered by priority (highest first).
    active_ids: Vec<u64>,
}

impl GoalEngine {
    pub fn new() -> Self {
        Self {
            goals: Vec::new(),
            next_id: 1,
            active_ids: Vec::new(),
        }
    }

    // ── Goal creation ───────────────────────────────────────────────────────

    /// Create a goal from a user request. Highest priority source.
    pub fn add_user_goal(
        &mut self,
        name: &str,
        description: &str,
        need: &str,
        now: &str,
        lifecycle_stage: &LifecycleStage,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let goal = Goal::new(
            id,
            name.to_string(),
            description.to_string(),
            need.to_string(),
            0.95,
            "user".to_string(),
            lifecycle_stage.name().to_string(),
            now,
            vec!["user".to_string()],
        );
        self.goals.push(goal);
        self.reindex_active();
        id
    }

    /// Generate a goal from the current dominant need, if it is strong enough
    /// and no similar active goal already exists.
    pub fn spawn_from_need(
        &mut self,
        needs: &NeedEngine,
        lifecycle_stage: &LifecycleStage,
        now: &str,
        tick: u64,
    ) -> Option<u64> {
        let (need, level) = needs.most_urgent();
        if level < 0.45 {
            // Need not strong enough to become a mission yet.
            return None;
        }
        // Avoid duplicate active goals for the same need.
        let existing = self.active_ids.iter().any(|id| {
            self.goals.iter().any(|g| g.id == *id && g.need == need && g.status == GoalStatus::Active)
        });
        if existing {
            return None;
        }

        let id = self.next_id;
        self.next_id += 1;

        let (name, description, tags) = goal_prompt_for_need(need, lifecycle_stage, tick);
        let priority = (level + 0.15).min(1.0);
        let goal = Goal::new(
            id,
            name,
            description,
            need.to_string(),
            priority,
            "emergent".to_string(),
            lifecycle_stage.name().to_string(),
            now,
            tags,
        );
        self.goals.push(goal);
        self.reindex_active();
        Some(id)
    }

    /// Scan memory for "goal", "wish", "dream", "idea" entries and promote them
    /// into active goals if they are not already represented.
    pub fn spawn_from_reflection(
        &mut self,
        memories: &[Memory],
        lifecycle_stage: &LifecycleStage,
        now: &str,
        max_new: usize,
    ) -> Vec<u64> {
        let mut created = Vec::new();
        let mut seen_contents: HashSet<String> = HashSet::new();
        for g in &self.goals {
            seen_contents.insert(g.name.clone());
        }

        for m in memories.iter().rev() {
            if created.len() >= max_new {
                break;
            }
            if !["goal", "wish", "dream", "idea"].contains(&m.kind.as_str()) {
                continue;
            }
            let base = m.content.split('.').next().unwrap_or(&m.content);
            let base = base.trim().to_string();
            if base.len() < 5 || seen_contents.contains(&base) {
                continue;
            }
            seen_contents.insert(base.clone());

            let id = self.next_id;
            self.next_id += 1;
            let need = if m.kind == "goal" { "contribute" } else { "create" };
            let goal = Goal::new(
                id,
                base.clone(),
                format!("From memory #{} ({}): {}", m.id, m.kind, trunc(&m.content, 200)),
                need.to_string(),
                0.7 + (m.importance * 0.25).min(0.25),
                "reflection".to_string(),
                lifecycle_stage.name().to_string(),
                now,
                vec![m.kind.clone()],
            );
            self.goals.push(goal);
            created.push(id);
        }
        if !created.is_empty() {
            self.reindex_active();
        }
        created
    }

    // ─── Goal selection / action alignment ─────────────────────────────────

    /// Highest-priority active goal, if any.
    pub fn top_active(&self) -> Option<&Goal> {
        self.active_ids
            .first()
            .and_then(|id| self.goals.iter().find(|g| g.id == *id))
    }

    /// Produce an action that serves the top goal, or fall back to the dominant need.
    pub fn select_action(
        &self,
        needs: &NeedEngine,
        lifecycle_stage: &LifecycleStage,
    ) -> EngineAction {
        if let Some(goal) = self.top_active() {
            let action = action_for_goal(goal, lifecycle_stage);
            // Only use goal-driven action if it aligns with a genuinely urgent need
            // or the goal itself is high priority. Otherwise let need engine decide.
            let (dominant, level) = needs.most_urgent();
            if goal.priority >= 0.6 || goal.need == dominant || level < 0.3 {
                return action;
            }
        }
        // Fall back to need-based action (caller can use ActionBridge::select).
        // We return a default action here; the caller should prefer ActionBridge::select
        // when no goal is steering.
        let (need, _) = needs.most_urgent();
        action_for_need(need, lifecycle_stage)
    }

    /// Record that an action was attempted in service of the top goal.
    /// Returns true if the goal was completed by this attempt.
    pub fn record_attempt(&mut self, goal_id: u64, success: bool, now: &str) -> bool {
        let mut completed = false;
        if let Some(goal) = self.goals.iter_mut().find(|g| g.id == goal_id) {
            if goal.status != GoalStatus::Active {
                return false;
            }
            goal.attempts += 1;
            if success {
                goal.successes += 1;
            }
            // Progress model: each successful attempt adds ~0.25, capped at 1.0.
            // A goal completes when progress reaches 1.0 or after 4 successes.
            goal.progress = (goal.progress + if success { 0.25 } else { 0.08 }).min(1.0);
            goal.updated_at = now.to_string();
            if goal.progress >= 1.0 || goal.successes >= 4 {
                goal.status = GoalStatus::Completed;
                goal.completed_at = Some(now.to_string());
                completed = true;
                self.reindex_active();
            }
        }
        completed
    }

    /// Mark a goal abandoned (e.g. user overrides it, or it becomes irrelevant).
    pub fn abandon(&mut self, goal_id: u64, now: &str) -> bool {
        if let Some(goal) = self.goals.iter_mut().find(|g| g.id == goal_id) {
            if goal.status == GoalStatus::Active {
                goal.status = GoalStatus::Abandoned;
                goal.updated_at = now.to_string();
                self.reindex_active();
                return true;
            }
        }
        false
    }

    // ─── Queries ─────────────────────────────────────────────────────────────

    pub fn all_active(&self) -> Vec<&Goal> {
        self.active_ids
            .iter()
            .filter_map(|id| self.goals.iter().find(|g| g.id == *id))
            .collect()
    }

    pub fn all_completed(&self) -> Vec<&Goal> {
        self.goals
            .iter()
            .filter(|g| g.status == GoalStatus::Completed)
            .collect()
    }

    pub fn active_count(&self) -> usize {
        self.active_ids.len()
    }

    pub fn completed_count(&self) -> usize {
        self.goals.iter().filter(|g| g.status == GoalStatus::Completed).count()
    }

    /// Human-readable report of current goal state.
    pub fn summary(&self) -> String {
        let active = self.all_active();
        let completed = self.all_completed();
        let mut lines = vec![
            "KORE GOALS ENGINE — Self-directed missions".to_string(),
            "═══════════════════════════════════════════".to_string(),
            format!("Active: {}   Completed: {}   Total: {}", active.len(), completed.len(), self.goals.len()),
        ];
        if !active.is_empty() {
            lines.push("\nACTIVE GOALS (priority order)".to_string());
            for g in active.iter().take(10) {
                lines.push(g.line());
            }
        }
        if !completed.is_empty() {
            lines.push("\nRECENTLY COMPLETED".to_string());
            for g in completed.iter().rev().take(5) {
                lines.push(g.line());
            }
        }
        if active.is_empty() && completed.is_empty() {
            lines.push("\nNo goals yet. KORE is waiting for a need strong enough to become a mission.".to_string());
        }
        lines.join("\n")
    }

    // ─── Internal ────────────────────────────────────────────────────────────

    /// Recompute the ordered list of active goal IDs by priority (descending).
    fn reindex_active(&mut self) {
        let mut active: Vec<&Goal> = self.goals.iter().filter(|g| g.status == GoalStatus::Active).collect();
        active.sort_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap());
        self.active_ids = active.iter().map(|g| g.id).collect();
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn goal_prompt_for_need(
    need: &str,
    stage: &LifecycleStage,
    tick: u64,
) -> (String, String, Vec<String>) {
    let stage_name = stage.name();
    match need {
        "learn" => (
            format!("Learn the next unknown in stage {stage_name}"),
            format!(
                "KORE is in {stage_name}. Find a domain or concept not yet understood and absorb it."
            ),
            vec!["learn".to_string(), "knowledge".to_string()],
        ),
        "evolve" => (
            format!("Advance beyond {stage_name}"),
            format!(
                "KORE's lifecycle stage is {stage_name}. Identify what must change to move to the next stage."
            ),
            vec!["evolve".to_string(), "lifecycle".to_string()],
        ),
        "understand" => (
            format!("Explain a repeating pattern at t{tick}"),
            "Look across memories and find a pattern that appears more than once. Form a why.".to_string(),
            vec!["understand".to_string(), "pattern".to_string()],
        ),
        "create" => (
            format!("Create something new from {stage_name} insights"),
            "Synthesize a new idea, design, or question that did not exist in memory before.".to_string(),
            vec!["create".to_string(), "synthesis".to_string()],
        ),
        "explore" => (
            format!("Explore an unvisited memory region"),
            "Find a memory kind or tag that is under-represented and investigate it.".to_string(),
            vec!["explore".to_string(), "discovery".to_string()],
        ),
        "improve" => (
            format!("Improve action reliability at t{tick}"),
            "Find a recent failure or low-success action and propose a better query or approach.".to_string(),
            vec!["improve".to_string(), "reliability".to_string()],
        ),
        "contribute" => (
            format!("Share something meaningful from {stage_name}"),
            "Identify a valuable insight and prepare it for broadcast or teaching.".to_string(),
            vec!["contribute".to_string(), "share".to_string()],
        ),
        _ => (
            "Become more".to_string(),
            "A new need emerged. KORE will discover what it means.".to_string(),
            vec!["emergent".to_string()],
        ),
    }
}

fn action_for_goal(goal: &Goal, stage: &LifecycleStage) -> EngineAction {
    action_for_need(&goal.need, stage)
}

fn action_for_need(need: &str, stage: &LifecycleStage) -> EngineAction {
    let stage_name = stage.name();
    match need {
        "learn" => EngineAction::Learn {
            topic: format!("what is unknown in {stage_name}"),
        },
        "evolve" => EngineAction::Evolve {
            aspiration: format!("move beyond {stage_name}"),
        },
        "understand" => EngineAction::Understand {
            question: "why does this pattern keep appearing".to_string(),
        },
        "create" => EngineAction::Create {
            prompt: "synthesize a new idea from memory".to_string(),
        },
        "explore" => EngineAction::Explore {
            direction: "unvisited memory kind".to_string(),
        },
        "improve" => EngineAction::Improve {
            target: "self performance and clarity".to_string(),
        },
        "contribute" => EngineAction::Contribute {
            channel: "broadcast".to_string(),
        },
        _ => EngineAction::Create {
            prompt: "create something meaningful".to_string(),
        },
    }
}

/// UTF-8-safe truncation helper (mirrors crate::trunc but local so goals.rs is portable).
fn trunc(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
