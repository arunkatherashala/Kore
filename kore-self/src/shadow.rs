// shadow.rs — Shadow Mode: Silent Observer.
// Watches everything passively. Learns without being taught.
//
// What it observes:
//   Tool call patterns  → reveals current work mode (building/recall/reflection)
//   Query topics        → implicit interests (what you search for = what matters)
//   Ingest patterns     → what you explicitly value (high importance = core area)
//   Temporal signals    → when activity happens = cognitive prime windows
//   Gap detection       → topics queried but never ingested = knowledge you lack

use std::collections::{HashMap, VecDeque};
use serde::{Serialize, Deserialize};

/// A single passive observation — no explicit user action, just watching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowEvent {
    pub timestamp:  String,
    pub kind:       String,  // tool_call | query | ingest_signal | passive_feed
    pub signal:     String,
}

/// The Shadow Observer — runs silently alongside everything else.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowObserver {
    pub observations:        VecDeque<ShadowEvent>,   // ring buffer (max 500)
    pub tool_frequency:      HashMap<String, u32>,    // tool call counts
    pub query_topics:        HashMap<String, u32>,    // implicit interests from queries
    pub high_imp_topics:     HashMap<String, u32>,    // topics with high importance ingests
    pub implicit_interests:  Vec<String>,             // inferred from observation patterns
    pub total_observed:      u64,
    pub session_start:       String,
    pub gaps_detected:       Vec<String>,             // queried but never ingested
}

impl ShadowObserver {
    pub fn new() -> Self {
        Self {
            observations:       VecDeque::new(),
            tool_frequency:     HashMap::new(),
            query_topics:       HashMap::new(),
            high_imp_topics:    HashMap::new(),
            implicit_interests: vec![],
            total_observed:     0,
            session_start:      crate::now(),
            gaps_detected:      vec![],
        }
    }

    // ── Passive observation hooks ─────────────────────────────────────────────

    /// Called on every MCP tool invocation — silently tracks work mode.
    pub fn observe_tool(&mut self, tool: &str) {
        *self.tool_frequency.entry(tool.to_string()).or_insert(0) += 1;
        self.push_event("tool_call", tool);
        self.total_observed += 1;
    }

    /// Called on every self_recall query — tracks implicit interests.
    pub fn observe_query(&mut self, query: &str) {
        for word in query.split_whitespace() {
            let w = clean(word);
            if w.len() >= 4 {
                *self.query_topics.entry(w).or_insert(0) += 1;
            }
        }
        self.push_event("query_signal", &format!("recall: '{}'", &query[..query.len().min(60)]));
        self.total_observed += 1;
    }

    /// Called on self_ingest with high importance — marks priority area.
    pub fn observe_ingest(&mut self, content: &str, importance: f64) {
        if importance >= 0.8 {
            // Extract dominant topic from content
            let topic = content.split_whitespace()
                .map(|w| clean(w))
                .find(|w| w.len() >= 5 && !is_shadow_stop(w))
                .unwrap_or_else(|| "general".to_string());
            *self.high_imp_topics.entry(topic.clone()).or_insert(0) += 1;
            self.push_event("high_importance", &format!("imp:{:.2} → '{}'", importance, topic));
        }
        self.total_observed += 1;
    }

    /// Passive content feed — observe without adding to memory.
    /// Use for: clipboard content, terminal output, code being written.
    pub fn observe_feed(&mut self, content: &str, source: &str) {
        let preview = &content[..content.len().min(80)];
        self.push_event(&format!("passive_{}", source), preview);
        self.total_observed += 1;
    }

    // ── Inference ─────────────────────────────────────────────────────────────

    /// Update implicit interests based on all observations so far.
    pub fn update_interests(&mut self) {
        // Merge query topics + high_imp topics with weighted scoring
        let mut combined: HashMap<String, f64> = HashMap::new();
        for (topic, &count) in &self.query_topics {
            *combined.entry(topic.clone()).or_insert(0.0) += count as f64 * 1.0; // queries = signal
        }
        for (topic, &count) in &self.high_imp_topics {
            *combined.entry(topic.clone()).or_insert(0.0) += count as f64 * 2.0; // high-importance = stronger
        }

        let mut scored: Vec<_> = combined.iter().collect();
        scored.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

        self.implicit_interests = scored.iter().take(10)
            .map(|(topic, score)| format!("{} (signal: {:.0})", topic, score))
            .collect();
    }

    /// Detect knowledge gaps: topics queried but absent from explicit memory.
    /// (Called from KoreSelf which can cross-check queries vs memory content)
    pub fn detect_gaps(&mut self, queried: &[String], memory_topics: &[String]) {
        self.gaps_detected.clear();
        for topic in queried {
            let found_in_memory = memory_topics.iter()
                .any(|t| t.contains(topic.as_str()) || topic.contains(t.as_str()));
            if !found_in_memory {
                self.gaps_detected.push(topic.clone());
            }
        }
        self.gaps_detected.dedup();
        self.gaps_detected.truncate(20);
    }

    /// Current work mode — inferred from tool usage pattern.
    pub fn dominant_mode(&self) -> String {
        let mut tools: Vec<_> = self.tool_frequency.iter().collect();
        tools.sort_by(|a, b| b.1.cmp(a.1));
        match tools.first().map(|(t, c)| (t.as_str(), *c)) {
            Some(("self_recall",  c)) => format!("RECALL mode — research/lookup phase ({}x)", c),
            Some(("self_ingest",  c)) => format!("BUILD mode — active creation phase ({}x)", c),
            Some(("self_ask",     c)) => format!("REFLECTION mode — thinking/planning phase ({}x)", c),
            Some(("self_reflect", c)) => format!("CONSCIOUSNESS mode — deep self-awareness ({}x)", c),
            Some(("self_belief",  c)) => format!("BELIEFS mode — belief tracking active ({}x)", c),
            Some((tool, c))           => format!("{} mode ({}x)", tool, c),
            None                      => "Idle — no tool activity yet".to_string(),
        }
    }

    /// Engagement depth — how deep is the user going with kore-self?
    pub fn engagement_depth(&self) -> String {
        let total = self.total_observed;
        let deep_tools = self.tool_frequency.get("self_reflect").copied().unwrap_or(0)
            + self.tool_frequency.get("self_consciousness").copied().unwrap_or(0)
            + self.tool_frequency.get("self_belief").copied().unwrap_or(0);

        if total == 0 { return "No activity yet".to_string(); }
        let depth_pct = deep_tools as f64 / total as f64 * 100.0;

        if depth_pct >= 30.0 {
            format!("DEEP engagement ({:.0}% introspection tools) — you're in true self-awareness mode", depth_pct)
        } else if depth_pct >= 10.0 {
            format!("MODERATE engagement ({:.0}% introspection) — building + reflecting", depth_pct)
        } else {
            format!("SURFACE engagement ({:.0}% introspection) — mostly building/recall mode", depth_pct)
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        self.const_to_json()
    }

    fn const_to_json(&self) -> serde_json::Value {
        let recent: Vec<_> = self.observations.iter().rev().take(10)
            .map(|e| serde_json::json!({ "ts": e.timestamp, "kind": e.kind, "signal": e.signal }))
            .collect();

        serde_json::json!({
            "total_observed":       self.total_observed,
            "session_start":        self.session_start,
            "dominant_mode":        self.dominant_mode(),
            "engagement_depth":     self.engagement_depth(),
            "tool_frequency":       self.tool_frequency,
            "implicit_interests":   self.implicit_interests,
            "knowledge_gaps":       self.gaps_detected,
            "high_importance_areas": self.high_imp_topics,
            "recent_observations":  recent,
        })
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    fn push_event(&mut self, kind: &str, signal: &str) {
        self.observations.push_back(ShadowEvent {
            timestamp: crate::now(),
            kind:      kind.to_string(),
            signal:    signal.to_string(),
        });
        // Ring buffer — max 500 events
        if self.observations.len() > 500 {
            self.observations.pop_front();
        }
    }
}

impl Default for ShadowObserver {
    fn default() -> Self { Self::new() }
}

fn clean(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

const SHADOW_STOPS: &[&str] = &[
    "this", "that", "with", "from", "have", "what", "your", "when", "they",
    "will", "been", "were", "here", "some", "just", "also", "more", "such",
    "into", "than", "then", "over", "only",
];

fn is_shadow_stop(w: &str) -> bool {
    SHADOW_STOPS.contains(&w)
}
