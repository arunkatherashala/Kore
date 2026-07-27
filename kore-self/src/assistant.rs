// kore-self  —  Phase 7: Human Assistant Mode
//
// Moves kore-self from REACTIVE (tool-responder) to PROACTIVE (human partner).
//
// self_brief    → morning situational briefing: yesterday, today, patterns, alerts
// self_chat     → free-form conversation using ALL memory + identity + consciousness context
// self_push     → intentional pushback on a decision using your own past patterns
// self_remind   → set + surface reminders in daily brief
// self_goals    → track all goals, show progress, detect missed deadlines

use serde::{Deserialize, Serialize};
use crate::Memory;
use crate::identity::IdentityModel;
use crate::consciousness::ConsciousnessState;
use crate::shadow::ShadowObserver;
use crate::predictive::PredictiveEngine;
use crate::dream::DreamEngine;
use crate::kore_query;

// ─── Reminder ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reminder {
    pub id:          u64,
    pub topic:       String,
    pub note:        String,
    pub created_at:  String,
    pub surfaced:    u32,    // how many times shown in brief
    pub done:        bool,
}

// ─── Chat turn ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurn {
    pub user_msg:  String,
    pub reply:     String,
    pub at:        String,
}

// ─── Assistant Engine ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantEngine {
    pub reminders:    Vec<Reminder>,
    pub chat_history: Vec<ChatTurn>,   // last 50 turns
    pub briefs_given: u32,
    pub next_id:      u64,
}

impl AssistantEngine {
    pub fn new() -> Self {
        Self {
            reminders:    vec![],
            chat_history: vec![],
            briefs_given: 0,
            next_id:      1,
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // self_brief
    // ─────────────────────────────────────────────────────────────────────────

    pub fn brief(
        &mut self,
        memories:      &[Memory],
        identity:      &IdentityModel,
        consciousness: &ConsciousnessState,
        shadow:        &ShadowObserver,
        dream:         &DreamEngine,
        predictive:    &PredictiveEngine,
    ) -> String {
        self.briefs_given += 1;
        let now = crate::now();
        let owner = &identity.owner;

        let mut lines: Vec<String> = vec![];

        // ── Header ────────────────────────────────────────────────────────────
        lines.push(format!("━━━ kore-self briefing for {} | {} ━━━", owner, &now[..10]));

        // ── Yesterday: what you worked on ─────────────────────────────────────
        let recent = kore_query::recent(memories, 20);
        if !recent.is_empty() {
            lines.push("\n📋 RECENTLY:".to_string());
            let by_kind = kore_query::kind_distribution(memories);
            for (kind, cnt, avg_imp) in by_kind.iter().take(4) {
                lines.push(format!("  {kind}: {cnt} memories (avg importance {avg_imp:.2})"));
            }
            // Most recent actual memory
            if let Some((_, content, imp)) = recent.first() {
                let preview: String = content.chars().take(100).collect();
                lines.push(format!("  Last memory ({imp:.1}): \"{preview}...\""));
            }
        }

        // ── Identity status ────────────────────────────────────────────────────
        lines.push("\n🧠 WHO YOU ARE (right now):".to_string());
        for v in identity.top_values(3) {
            lines.push(format!("  {} → {:.0}% strength ({} evidence)", v.name, v.strength*100.0, v.evidence));
        }
        lines.push(format!("  Thinking: metrics-driven {:.0}% | perfectionism {:.0}% | decision-speed {:.0}%",
            identity.thinking.metrics_driven*100.0,
            identity.thinking.perfectionism*100.0,
            identity.thinking.decision_speed*100.0));

        // ── Consciousness state ────────────────────────────────────────────────
        lines.push("\n⚡ CONSCIOUSNESS:".to_string());
        lines.push(format!("  Cycle {} | {} total ticks | {} insights generated",
            consciousness.cycle, consciousness.total_ticks, consciousness.insights_total));
        if !consciousness.active_plan.is_empty() {
            lines.push("  Active plan:".to_string());
            for p in &consciousness.active_plan {
                lines.push(format!("    → {p}"));
            }
        }
        if !consciousness.thoughts.is_empty() {
            lines.push(format!("  Last thought: \"{}\"", consciousness.thoughts.last().unwrap_or(&String::new())));
        }

        // ── Goals tracking ─────────────────────────────────────────────────────
        let goals = kore_query::by_kind(memories, "goal");
        if !goals.is_empty() {
            lines.push("\n🎯 GOALS:".to_string());
            for (_, content, imp) in goals.iter().take(5) {
                let preview: String = content.chars().take(90).collect();
                let status = if *imp >= 0.9 { "🔴 critical" } else if *imp >= 0.7 { "🟡 important" } else { "🟢 tracked" };
                lines.push(format!("  {status} | \"{preview}\""));
            }
        }

        // ── Dream patterns (obsessions) ────────────────────────────────────────
        let obsessions: Vec<_> = dream.discoveries.iter()
            .filter(|d| d.kind == "obsession").take(3).collect();
        if !obsessions.is_empty() {
            lines.push("\n💭 WHAT YOUR MIND KEEPS RETURNING TO:".to_string());
            for d in obsessions {
                lines.push(format!("  → {} ({:.0}% confidence)", d.description, d.confidence*100.0));
            }
        }

        // ── Shadow: work mode ──────────────────────────────────────────────────
        let mode  = shadow.dominant_mode();
        let depth = shadow.engagement_depth();
        lines.push(format!("\n👁  SHADOW REPORT: mode='{}' | engagement='{}' | {} total observations",
            mode, depth, shadow.total_observed));
        if !shadow.implicit_interests.is_empty() {
            lines.push(format!("  Implicit interests: {}", shadow.implicit_interests.iter().take(5).cloned().collect::<Vec<_>>().join(", ")));
        }

        // ── Decision patterns ──────────────────────────────────────────────────
        if !predictive.patterns.is_empty() {
            lines.push("\n🎲 YOUR DECISION PATTERNS:".to_string());
            for p in predictive.patterns.iter().take(3) {
                lines.push(format!("  When '{}' → you choose '{}' ({:.0}% of the time)",
                    p.context, p.choice, p.confidence*100.0));
            }
        }

        // ── Reminders ─────────────────────────────────────────────────────────
        let active_reminders: Vec<&Reminder> = self.reminders.iter().filter(|r| !r.done).collect();
        if !active_reminders.is_empty() {
            lines.push("\n🔔 REMINDERS:".to_string());
            for r in active_reminders.iter().take(5) {
                lines.push(format!("  → [{}] {}", r.topic, r.note));
                // Mark as surfaced
            }
            for r in self.reminders.iter_mut().filter(|r| !r.done) {
                r.surfaced += 1;
            }
        }

        // ── Proactive suggestions ──────────────────────────────────────────────
        lines.push("\n💡 kore-self NOTICES:".to_string());
        let suggestions = self.proactive_insights(memories, identity, consciousness, shadow);
        if suggestions.is_empty() {
            lines.push("  Everything looks on track.".to_string());
        } else {
            for s in suggestions {
                lines.push(format!("  ⚠  {s}"));
            }
        }

        lines.push(format!("\n━━━ end of brief #{} ━━━", self.briefs_given));
        lines.join("\n")
    }

    // ─────────────────────────────────────────────────────────────────────────
    // self_chat
    // ─────────────────────────────────────────────────────────────────────────

    pub fn chat(
        &mut self,
        message:  &str,
        memories: &[Memory],
        identity: &IdentityModel,
        shadow:   &ShadowObserver,
    ) -> String {
        let msg_lower = message.to_lowercase();

        // Recall relevant memories
        let q_words: Vec<&str> = message.split_whitespace().collect();
        let n = memories.len();
        let mut scored: Vec<(f64, &Memory)> = memories.iter().enumerate()
            .filter_map(|(i, m)| {
                let c = m.content.to_lowercase();
                let hits = q_words.iter().filter(|&&w| c.contains(w)).count() as f64;
                if hits == 0.0 { return None; }
                let recency = 1.0 / (1.0 + n.saturating_sub(i) as f64 * 0.05);
                Some((hits * m.importance * (1.0 + recency), m))
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let relevant: Vec<&Memory> = scored.iter().take(5).map(|(_, m)| *m).collect();

        // Build context string from relevant memories
        let context: Vec<String> = relevant.iter()
            .map(|m| format!("[{}|{:.1}] {}", m.kind, m.importance,
                m.content.chars().take(120).collect::<String>()))
            .collect();

        // Detect intent
        let reply = if msg_lower.contains("how") && msg_lower.contains("you") {
            // "how are you" type
            format!("I'm kore-self — your digital twin. {} memories, {} consciousness cycles. \
                     Currently tracking: {}. What's on your mind?",
                memories.len(), 0,
                shadow.implicit_interests.iter().take(2).cloned().collect::<Vec<_>>().join(", "))

        } else if msg_lower.contains("remember") || msg_lower.contains("recall") || msg_lower.contains("what did") {
            // Memory recall
            if relevant.is_empty() {
                format!("I don't have memories about '{}' yet. Ingest some with self_ingest.", message)
            } else {
                let mem_str: String = relevant.iter().take(3)
                    .map(|m| format!("• [{}] {}", m.kind, m.content.chars().take(150).collect::<String>()))
                    .collect::<Vec<_>>().join("\n");
                format!("Here's what I remember about that:\n\n{}\n\n({} relevant memories found)", mem_str, relevant.len())
            }

        } else if msg_lower.contains("what should") || msg_lower.contains("should i") || msg_lower.contains("advice") {
            // Advice mode — use identity + past decisions
            let top_val = identity.top_values(1).into_iter().next()
                .map(|v| v.name.clone()).unwrap_or("your principles".to_string());
            if relevant.is_empty() {
                format!("Based on your identity: you value '{}' most. Apply that lens here.", top_val)
            } else {
                let past = relevant[0].content.chars().take(120).collect::<String>();
                format!("Based on your past ({:.0}% importance): \"{}\"\n\nYour core value is '{}'. \
                         What would that value tell you to do here?",
                    relevant[0].importance * 100.0, past, top_val)
            }

        } else if msg_lower.contains("why") && (msg_lower.contains("i") || msg_lower.contains("me")) {
            // Self-reflection
            let obsession = shadow.implicit_interests.first()
                .cloned().unwrap_or("your core work".to_string());
            format!("Looking at your patterns: you consistently gravitate toward '{}'. \
                     Your thinking style is {:.0}% metrics-driven with {:.0}% perfectionism. \
                     The 'why' is usually in those defaults.",
                obsession,
                identity.thinking.metrics_driven * 100.0,
                identity.thinking.perfectionism * 100.0)

        } else if relevant.is_empty() {
            // No context — honest answer
            format!("I don't have specific memories about '{}'. \
                     Tell me more — use self_ingest to build my understanding. \
                     I currently have {} memories across your experience.",
                message, memories.len())

        } else {
            // General — assemble answer from context
            let ctx_summary: String = context.iter().take(2).cloned().collect::<Vec<_>>().join(" | ");
            format!("Based on what I know about you:\n\n{}\n\n\
                     This connects to your '{}' value (strength {:.0}%). \
                     {} relevant memories found.",
                ctx_summary,
                identity.top_values(1).first().map(|v| v.name.as_str()).unwrap_or("your principles"),
                identity.top_values(1).first().map(|v| v.strength * 100.0).unwrap_or(0.0),
                relevant.len())
        };

        let turn = ChatTurn {
            user_msg: message.to_string(),
            reply:    reply.clone(),
            at:       crate::now(),
        };
        self.chat_history.push(turn);
        if self.chat_history.len() > 50 { self.chat_history.remove(0); }

        reply
    }

    // ─────────────────────────────────────────────────────────────────────────
    // self_push
    // ─────────────────────────────────────────────────────────────────────────

    /// Pushback on a decision using past patterns.
    /// "Are you sure? Here's what your past says..."
    pub fn push(
        &self,
        decision:   &str,
        memories:   &[Memory],
        identity:   &IdentityModel,
        predictive: &PredictiveEngine,
    ) -> String {
        // Find contradicting past decisions
        let dec_lower = decision.to_lowercase();
        let related: Vec<&Memory> = memories.iter()
            .filter(|m| m.kind == "decision" &&
                m.content.split_whitespace()
                    .any(|w| dec_lower.contains(&w.to_lowercase()) || w.len() >= 5))
            .collect();

        let mut pushback: Vec<String> = vec![];

        // Check against predictive patterns
        for p in &predictive.patterns {
            if dec_lower.contains(&p.context) || p.context.len() >= 4 && dec_lower.contains(&p.context[..4.min(p.context.len())]) {
                pushback.push(format!(
                    "Pattern alert: when you face '{}' situations, you've chosen '{}' {:.0}% of the time ({} times).",
                    p.context, p.choice, p.confidence * 100.0, p.count
                ));
            }
        }

        // Check against beliefs
        for belief in identity.beliefs.values() {
            if dec_lower.contains(&belief.topic.to_lowercase()) && !belief.history.is_empty() {
                pushback.push(format!(
                    "Belief drift detected: your view on '{}' has changed {} time(s). Current stance: '{}'.",
                    belief.topic, belief.history.len(), belief.stance
                ));
            }
        }

        // Check against high-importance past decisions
        if !related.is_empty() {
            let past = related[0].content.chars().take(150).collect::<String>();
            pushback.push(format!(
                "Past decision (importance {:.0}%): \"{}\"",
                related[0].importance * 100.0, past
            ));
        }

        // Top value alignment check
        let top_val = identity.top_values(1).into_iter().next();
        if let Some(val) = top_val {
            let aligned = dec_lower.contains(&val.name.to_lowercase());
            if !aligned {
                pushback.push(format!(
                    "Value check: your top value is '{}' ({:.0}% strength). Does '{}' align with it?",
                    val.name, val.strength * 100.0, decision
                ));
            } else {
                pushback.push(format!(
                    "Value check: ✓ '{}' aligns with your core value '{}'.",
                    decision, val.name
                ));
            }
        }

        if pushback.is_empty() {
            format!("No strong contradictions found for '{}'. You're in uncharted territory — no past pattern to push back against. Proceed with intention.", decision)
        } else {
            format!("⚠  Pushback on: \"{decision}\"\n\n{}\n\nFinal call is yours. Just making sure you've considered this.", pushback.join("\n\n"))
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // self_remind
    // ─────────────────────────────────────────────────────────────────────────

    pub fn add_reminder(&mut self, topic: &str, note: &str) -> String {
        let r = Reminder {
            id:         self.next_id,
            topic:      topic.to_string(),
            note:       note.to_string(),
            created_at: crate::now(),
            surfaced:   0,
            done:       false,
        };
        self.next_id += 1;
        self.reminders.push(r);
        // Keep last 50
        if self.reminders.len() > 50 { self.reminders.remove(0); }
        format!("Reminder set: [{}] {} — will surface in next self_brief", topic, note)
    }

    pub fn mark_done(&mut self, topic: &str) -> String {
        let found = self.reminders.iter_mut()
            .filter(|r| !r.done && r.topic.to_lowercase().contains(&topic.to_lowercase()))
            .count();
        self.reminders.iter_mut()
            .filter(|r| r.topic.to_lowercase().contains(&topic.to_lowercase()))
            .for_each(|r| r.done = true);
        if found > 0 {
            format!("Marked {found} reminder(s) as done for topic '{topic}'")
        } else {
            format!("No active reminders found for '{topic}'")
        }
    }

    pub fn list_reminders(&self) -> serde_json::Value {
        let active: Vec<_> = self.reminders.iter().filter(|r| !r.done).map(|r| serde_json::json!({
            "id":      r.id, "topic": r.topic, "note": r.note,
            "created": r.created_at, "surfaced": r.surfaced,
        })).collect();
        let done_count = self.reminders.iter().filter(|r| r.done).count();
        serde_json::json!({ "active": active, "done_count": done_count })
    }

    // ─────────────────────────────────────────────────────────────────────────
    // self_goals
    // ─────────────────────────────────────────────────────────────────────────

    pub fn goals_report(&self, memories: &[Memory]) -> String {
        let goals = kore_query::by_kind(memories, "goal");
        if goals.is_empty() {
            return "No goals tracked yet. Use self_ingest with kind='goal' to add one.".to_string();
        }
        let mut lines = vec!["🎯 GOAL TRACKER:".to_string()];
        for (id, content, imp) in &goals {
            let status = if *imp >= 0.95 { "🔴 CRITICAL" }
                         else if *imp >= 0.8 { "🟡 HIGH" }
                         else { "🟢 TRACKED" };
            let preview: String = content.chars().take(120).collect();
            lines.push(format!("  [{status}] (id:{id}, importance:{imp:.2}) \"{preview}\""));
        }
        lines.push(format!("\n{} goals total. Ingest progress updates with kind='goal'.", goals.len()));
        lines.join("\n")
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Proactive insights
    // ─────────────────────────────────────────────────────────────────────────

    fn proactive_insights(
        &self,
        memories:      &[Memory],
        identity:      &IdentityModel,
        consciousness: &ConsciousnessState,
        shadow:        &ShadowObserver,
    ) -> Vec<String> {
        let mut insights = vec![];

        // High decision load
        let recent_dec = kore_query::by_kind(memories, "decision").len();
        if recent_dec >= 5 {
            insights.push(format!("{recent_dec} decisions tracked recently. High cognitive load. Consider a review before adding more."));
        }

        // Goal without progress
        let goals = kore_query::by_kind(memories, "goal");
        if !goals.is_empty() && consciousness.cycle < 5 {
            insights.push(format!("{} goal(s) tracked. Run self_tick or self_dream to analyze progress.", goals.len()));
        }

        // Single-track obsession
        if let Some(first) = shadow.implicit_interests.first() {
            let count = memories.iter().filter(|m| m.content.to_lowercase().contains(first.as_str())).count();
            if count > memories.len() / 3 && memories.len() > 10 {
                insights.push(format!("'{}' appears in {:.0}% of your memories. Deep focus — or tunnel vision?",
                    first, count as f64 / memories.len() as f64 * 100.0));
            }
        }

        // Low certainty in voice
        if identity.voice.certainty < 0.45 {
            insights.push("Your voice certainty is low. You've been hedging lately. What's making you uncertain?".to_string());
        }

        // High perfectionism + low decision speed = bottleneck
        if identity.thinking.perfectionism > 0.7 && identity.thinking.decision_speed < 0.4 {
            insights.push("Perfectionism is high, decision speed is low. Classic bottleneck. Ship something, then refine.".to_string());
        }

        // Active reminders not yet surfaced
        let urgent: Vec<&Reminder> = self.reminders.iter().filter(|r| !r.done && r.surfaced == 0).collect();
        if !urgent.is_empty() {
            insights.push(format!("{} new reminder(s) you haven't seen yet.", urgent.len()));
        }

        insights
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "briefs_given":   self.briefs_given,
            "chat_turns":     self.chat_history.len(),
            "active_reminders": self.reminders.iter().filter(|r| !r.done).count(),
            "last_chat":      self.chat_history.last().map(|t| serde_json::json!({
                "user": t.user_msg, "at": t.at
            })),
        })
    }
}

impl Default for AssistantEngine {
    fn default() -> Self { Self::new() }
}
