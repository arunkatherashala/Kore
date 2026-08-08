// consciousness.rs — The Consciousness Loop.
// OBSERVE → THINK → REFLECT → PLAN → ACT → DREAM → REPEAT
// Runs autonomously. Never waits. Generates insights without being asked.
// OBSERVE + THINK phases now powered by KORE SQL engine.

use serde::{Serialize, Deserialize};
use crate::Memory;
use crate::identity::IdentityModel;
use crate::kore_query;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Phase {
    Observe,
    Think,
    Reflect,
    Plan,
    Act,
    Dream,
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Phase::Observe => write!(f, "OBSERVE"),
            Phase::Think   => write!(f, "THINK"),
            Phase::Reflect => write!(f, "REFLECT"),
            Phase::Plan    => write!(f, "PLAN"),
            Phase::Act     => write!(f, "ACT"),
            Phase::Dream   => write!(f, "DREAM"),
        }
    }
}

/// The live state of the consciousness loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsciousnessState {
    pub cycle:           u64,
    pub phase:           Phase,
    pub observations:    Vec<String>,
    pub thoughts:        Vec<String>,
    pub active_plan:     Vec<String>,
    pub dream_log:       Vec<String>,   // last 50 dream insights
    pub last_tick_ts:    String,
    pub total_ticks:     u64,
    pub insights_total:  u64,
}

impl ConsciousnessState {
    pub fn new() -> Self {
        Self {
            cycle:          0,
            phase:          Phase::Observe,
            observations:   vec![],
            thoughts:       vec![],
            active_plan:    vec![],
            dream_log:      vec![],
            last_tick_ts:   crate::now(),
            total_ticks:    0,
            insights_total: 0,
        }
    }

    /// Run one full consciousness cycle.
    /// Returns: (new_memories_to_store, tick_log_lines)
    pub fn tick(
        &mut self,
        memories: &[Memory],
        identity: &mut IdentityModel,
    ) -> (Vec<(String, String, f64)>, Vec<String>) {
        self.cycle          += 1;
        self.total_ticks    += 1;
        self.last_tick_ts    = crate::now();

        let mut new_mems: Vec<(String, String, f64)> = vec![];
        let mut log:      Vec<String>                = vec![];

        // ── OBSERVE: What happened recently? ─────────────── (KORE SQL) ──
        self.phase = Phase::Observe;
        self.observations.clear();

        // Real KQL query on last 30 memories by id DESC
        let kind_dist = kore_query::kind_distribution(memories);
        let recent_30 = kore_query::recent(memories, 30);

        let dominant = kind_dist.first().map(|(k, _, _)| k.as_str()).unwrap_or("none");
        let obs = format!(
            "{} memories total | kind distribution: {} | dominant: '{}' | KQL-powered",
            memories.len(),
            kind_dist.iter().map(|(k, c, _)| format!("{k}:{c}")).collect::<Vec<_>>().join(", "),
            dominant
        );
        self.observations.push(obs.clone());
        log.push(format!("[OBSERVE] {}", obs));

        // ── THINK: What does this mean? ───────────────────── (KORE SQL) ──
        self.phase = Phase::Think;
        self.thoughts.clear();

        // Use KQL avg importance per kind
        let imp_by_kind = kore_query::importance_by_kind(memories);
        let overall_avg: f64 = if recent_30.is_empty() { 0.0 } else {
            recent_30.iter().map(|(_, _, imp)| imp).sum::<f64>() / recent_30.len() as f64
        };
        if overall_avg > 0.0 {
            let thought = if overall_avg >= 0.85 {
                format!("Critical-work period active — KQL avg importance {overall_avg:.2}. Peak building time.")
            } else if overall_avg <= 0.40 {
                format!("Maintenance mode — KQL avg importance {overall_avg:.2}. Consolidation phase.")
            } else {
                format!("Steady progress — KQL avg importance {overall_avg:.2}.")
            };
            self.thoughts.push(thought.clone());
            log.push(format!("[THINK] {}", thought));
        }

        // Decision load from KQL
        let decisions = kind_dist.iter().find(|(k, _, _)| k == "decision").map(|(_, c, _)| *c).unwrap_or(0);
        let recent_decisions = recent_30.iter().filter(|(k, _, _)| k == "decision").count() as i64;
        if recent_decisions >= 3 {
            let t = format!("{recent_decisions} decisions in recent 30 — high cognitive load.");
            self.thoughts.push(t.clone());
            log.push(format!("[THINK] {}", t));
        }

        // Deep focus detection from kind with highest recent count
        if let Some((top_kind, avg_imp)) = imp_by_kind.first() {
            if *avg_imp >= 0.85 {
                let t = format!("Deep focus: '{}' memories averaging {avg_imp:.2} importance — core active area.", top_kind);
                self.thoughts.push(t.clone());
                log.push(format!("[THINK] {}", t));
            }
        }

        // Stagnation: lots of total decisions but none in recent 30
        if self.cycle > 5 && decisions > 5 && recent_decisions == 0 {
            let t = format!("Stagnation signal: {decisions} total decisions but 0 recent — may need a new challenge.");
            self.thoughts.push(t.clone());
            log.push(format!("[THINK] {}", t));
        }

        // topic_map for backward compat with PLAN phase
        let topic_map = topic_frequency(&memories.iter().rev().take(30).collect::<Vec<_>>());
        let mut top_topics: Vec<(&String, &usize)> = topic_map.iter().collect();
        top_topics.sort_by(|a, b| b.1.cmp(a.1));

        // ── REFLECT: What changed? What did I get wrong? ───────────────────
        self.phase = Phase::Reflect;

        // Contradiction Engine input: beliefs that evolved
        let evolved: Vec<String> = identity.beliefs.values()
            .filter(|b| !b.history.is_empty())
            .map(|b| format!("'{}' evolved {} time(s)", b.topic, b.history.len()))
            .collect();
        if !evolved.is_empty() {
            let r = format!("Beliefs in flux: {}.", evolved.join(", "));
            self.thoughts.push(r.clone());
            log.push(format!("[REFLECT] {}", r));
        }

        // Identity coherence signal
        if identity.thinking.metrics_driven > 0.82 {
            let r = "Identity signal: metrics-driven thinking is dominant. Every claim backed by data.".to_string();
            self.thoughts.push(r.clone());
            log.push(format!("[REFLECT] {}", r));
        }

        // Recency bias check: very recent memories all same kind?
        let very_recent: Vec<&Memory> = memories.iter().rev().take(8).collect();
        if !very_recent.is_empty() {
            let first_kind = &very_recent[0].kind;
            let all_same = very_recent.iter().all(|m| &m.kind == first_kind);
            if all_same && very_recent.len() >= 5 {
                let r = format!("Last {} memories all '{}' — single-track focus detected.", very_recent.len(), first_kind);
                self.thoughts.push(r.clone());
                log.push(format!("[REFLECT] {}", r));
            }
        }

        // ── PLAN: What to do next? ─────────────────────────────────────────
        self.phase = Phase::Plan;
        self.active_plan.clear();

        for (topic, count) in top_topics.iter().take(3) {
            if **count >= 3 {
                self.active_plan.push(format!("Continue '{}' ({} recent mentions)", topic, count));
            }
        }
        if decisions >= 2 {
            self.active_plan.push(format!("Review {} recent decisions for consistency", decisions));
        }
        if identity.beliefs.values().any(|b| !b.history.is_empty()) {
            self.active_plan.push("Reconcile evolved beliefs with current priorities".to_string());
        }
        if self.active_plan.is_empty() {
            self.active_plan.push("Exploration mode — no dominant focus".to_string());
        }
        log.push(format!("[PLAN] {}", self.active_plan.join(" | ")));

        // ── ACT: Generate insight memory if something meaningful was found ─
        self.phase = Phase::Act;
        if !self.thoughts.is_empty() {
            let insight = format!(
                "[Consciousness Cycle {} | {}] {}",
                self.cycle, self.last_tick_ts,
                self.thoughts.join(" | ")
            );
            new_mems.push((insight, "reflection".to_string(), 0.65));
            self.insights_total += 1;
        }

        // ── DREAM: Deep pattern analysis every 10 cycles ──────────────────
        if self.cycle % 10 == 0 && memories.len() >= 10 {
            self.phase = Phase::Dream;
            if let Some(dream) = self.dream(memories, identity) {
                // Keep dream log bounded at 50 entries
                self.dream_log.push(dream.clone());
                if self.dream_log.len() > 50 { self.dream_log.remove(0); }
                new_mems.push((dream.clone(), "insight".to_string(), 0.85));
                log.push(format!("[DREAM] {}", dream));
                self.insights_total += 1;
            }
        }

        self.phase = Phase::Observe; // ready for next cycle
        (new_mems, log)
    }

    /// Dream Engine: consolidate across ALL memories, find deep patterns.
    fn dream(&self, memories: &[Memory], identity: &IdentityModel) -> Option<String> {
        let total      = memories.len();
        let critical   = memories.iter().filter(|m| m.importance >= 0.9).count();
        let decisions  = memories.iter().filter(|m| m.kind == "decision").count();
        let reflections = memories.iter().filter(|m| m.kind == "reflection").count();
        let insights   = memories.iter().filter(|m| m.kind == "insight").count();

        // Kind diversity
        let mut kinds: Vec<&str> = memories.iter().map(|m| m.kind.as_str()).collect();
        kinds.sort_unstable();
        kinds.dedup();
        let diversity = kinds.len();

        let core_val = identity.top_values(1)
            .first()
            .map(|v| v.name.as_str())
            .unwrap_or("undefined");

        let evolved_beliefs = identity.beliefs.values()
            .filter(|b| !b.history.is_empty())
            .count();

        let pattern = if critical as f64 > total as f64 * 0.25 {
            format!("Intense builder — {:.0}% of memories are critical-importance", critical as f64 / total as f64 * 100.0)
        } else if decisions as f64 > total as f64 * 0.20 {
            format!("Decision-maker personality — {:.0}% decisions", decisions as f64 / total as f64 * 100.0)
        } else if diversity >= 5 {
            format!("Multi-dimensional growth — {} memory types active", diversity)
        } else {
            format!("Focused specialist — depth over breadth")
        };

        Some(format!(
            "[Dream | Cycle {}] {} total memories | core value: '{}' | {} decisions | {} reflections | {} insights | {} evolved beliefs | {}",
            self.cycle, total, core_val, decisions, reflections, insights, evolved_beliefs, pattern
        ))
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "cycle":           self.cycle,
            "phase":           self.phase.to_string(),
            "total_ticks":     self.total_ticks,
            "insights_generated": self.insights_total,
            "last_tick":       self.last_tick_ts,
            "observations":    self.observations,
            "current_thoughts": self.thoughts,
            "active_plan":     self.active_plan,
            "recent_dreams":   self.dream_log.iter().rev().take(5).collect::<Vec<_>>(),
        })
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn topic_frequency<'a>(memories: &[&'a Memory]) -> std::collections::HashMap<String, usize> {
    let mut freq = std::collections::HashMap::new();
    for m in memories {
        for word in m.content.split_whitespace() {
            let w: String = word.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            if w.len() >= 5 && !is_stop(&w) {
                *freq.entry(w).or_insert(0) += 1;
            }
        }
    }
    freq
}

const STOP_WORDS: &[&str] = &[
    "which", "where", "their", "there", "these", "those", "would", "could", "should",
    "about", "after", "being", "every", "other", "since", "under", "until", "while",
    "layer", "build", "built", "using", "based", "bench", "types", "store", "value",
    "query", "block", "added", "total", "result", "returns", "column", "memory",
    "function", "method", "string", "vector", "number", "object", "struct",
];

fn is_stop(w: &str) -> bool {
    STOP_WORDS.contains(&w)
}
