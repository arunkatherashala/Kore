// dream.rs — Dream Engine: Deep pattern consolidation.
// Runs when the system is idle. Like REM sleep — consolidates, connects, reveals.
//
// What it finds:
//   Obsessions    — topics appearing in >10% of all memories
//   Evolution     — topics growing vs fading over time
//   Consolidation — clusters of redundant memories to merge
//   Time patterns — when you're most cognitively active
//   Stress signals — recurring struggle patterns
//   Knowledge gaps — topics queried but never ingested

use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use crate::Memory;

/// A discovered pattern — the output of a dream cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternDiscovery {
    pub id:             u64,
    pub kind:           String,    // obsession | evolution | consolidation | time | stress | gap
    pub description:    String,
    pub evidence_count: usize,
    pub confidence:     f64,
    pub discovered_at:  String,
}

/// The Dream Engine state — persisted across sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamEngine {
    pub total_dreams:         u64,
    pub last_deep_dream_ts:   String,
    pub discoveries:          Vec<PatternDiscovery>,   // all found patterns
    pub consolidated_clusters: u64,
}

impl DreamEngine {
    pub fn new() -> Self {
        Self {
            total_dreams:          0,
            last_deep_dream_ts:    "never".to_string(),
            discoveries:           vec![],
            consolidated_clusters: 0,
        }
    }

    /// Full deep dream over ALL memories.
    /// Returns new memories (insights) to store.
    pub fn dream_deep(&mut self, memories: &[Memory]) -> Vec<(String, String, f64)> {
        if memories.len() < 5 { return vec![]; }

        let mut insights: Vec<(String, String, f64)> = vec![];

        insights.extend(self.find_obsessions(memories));
        insights.extend(self.find_evolution(memories));
        insights.extend(self.find_consolidation_clusters(memories));
        insights.extend(self.find_time_patterns(memories));
        insights.extend(self.find_stress_signals(memories));

        self.total_dreams         += 1;
        self.last_deep_dream_ts    = crate::now();

        insights
    }

    // ── 1. Obsessions: topics in >10% of all memories ────────────────────────

    fn find_obsessions(&mut self, memories: &[Memory]) -> Vec<(String, String, f64)> {
        let freq = word_freq_all(memories);
        let threshold = (memories.len() / 10).max(3);

        let mut obsessions: Vec<(&String, &usize)> = freq.iter()
            .filter(|(_, &c)| c >= threshold)
            .collect();
        obsessions.sort_by(|a, b| b.1.cmp(a.1));

        let mut out = vec![];
        for (topic, &count) in obsessions.iter().take(5) {
            let pct = count as f64 / memories.len() as f64 * 100.0;
            let conf = (pct / 50.0).min(0.99);

            let insight = format!(
                "[Dream:Obsession] '{}' appears in {} memories ({:.0}% of all). \
                 This is a core obsession — it defines your intellectual identity.",
                topic, count, pct
            );
            self.push_discovery("obsession", &format!("'{}': {}x ({:.0}%)", topic, count, pct), count, conf);
            out.push((insight, "insight".to_string(), 0.88));
        }
        out
    }

    // ── 2. Evolution: topics growing or fading over time ─────────────────────

    fn find_evolution(&mut self, memories: &[Memory]) -> Vec<(String, String, f64)> {
        if memories.len() < 10 { return vec![]; }

        let mid   = memories.len() / 2;
        let early = word_freq_all(&memories[..mid]);
        let late  = word_freq_all(&memories[mid..]);

        let mut growing: Vec<(String, usize, usize)> = vec![];
        let mut fading:  Vec<(String, usize, usize)> = vec![];

        for (topic, &late_n) in &late {
            let early_n = early.get(topic).copied().unwrap_or(0);
            if late_n >= early_n * 3 && late_n >= 3 {
                growing.push((topic.clone(), early_n, late_n));
            }
        }
        for (topic, &early_n) in &early {
            let late_n = late.get(topic).copied().unwrap_or(0);
            if early_n >= late_n * 3 && early_n >= 3 {
                fading.push((topic.clone(), early_n, late_n));
            }
        }

        growing.sort_by(|a, b| b.2.cmp(&a.2));
        fading.sort_by(|a, b| b.1.cmp(&a.1));

        let mut out = vec![];
        for (topic, early_n, late_n) in growing.iter().take(2) {
            let insight = format!(
                "[Dream:Evolution] '{}' is GROWING: was {} mentions → now {}. \
                 Emerging focus area — your interests are shifting here.",
                topic, early_n, late_n
            );
            self.push_discovery("evolution_growing", &format!("'{}': {} → {}", topic, early_n, late_n), *late_n, 0.8);
            out.push((insight, "insight".to_string(), 0.82));
        }
        for (topic, early_n, late_n) in fading.iter().take(2) {
            let insight = format!(
                "[Dream:Evolution] '{}' is FADING: was {} mentions → now {}. \
                 This chapter may be closing in your journey.",
                topic, early_n, late_n
            );
            self.push_discovery("evolution_fading", &format!("'{}': {} → {}", topic, early_n, late_n), *early_n, 0.75);
            out.push((insight, "insight".to_string(), 0.75));
        }
        out
    }

    // ── 3. Consolidation: clusters of similar memories ────────────────────────

    fn find_consolidation_clusters(&mut self, memories: &[Memory]) -> Vec<(String, String, f64)> {
        if memories.len() < 6 { return vec![]; }

        // Group memories by their top non-stop word
        let mut clusters: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, m) in memories.iter().enumerate() {
            let top = m.content.split_whitespace()
                .map(|w| clean_word(w))
                .find(|w| w.len() >= 5 && !is_stop(w));
            if let Some(key) = top {
                clusters.entry(key).or_default().push(i);
            }
        }

        let mut out = vec![];
        for (topic, indices) in clusters.iter() {
            if indices.len() >= 4 {
                let avg_imp: f64 = indices.iter()
                    .map(|&i| memories[i].importance)
                    .sum::<f64>() / indices.len() as f64;
                let insight = format!(
                    "[Dream:Consolidate] {} memories cluster around '{}' (avg importance: {:.2}). \
                     Consider distilling into one master principle.",
                    indices.len(), topic, avg_imp
                );
                self.consolidated_clusters += 1;
                self.push_discovery("consolidation", &format!("'{}': {} related memories", topic, indices.len()), indices.len(), 0.7);
                out.push((insight, "insight".to_string(), 0.75));
            }
        }
        out.truncate(3); // don't flood
        out
    }

    // ── 4. Time patterns: when is the user most active ────────────────────────

    fn find_time_patterns(&mut self, memories: &[Memory]) -> Vec<(String, String, f64)> {
        let mut hours = [0u32; 24];
        for m in memories {
            if let Some(h) = parse_hour(&m.timestamp) {
                hours[h as usize] += 1;
            }
        }
        let peak = hours.iter().enumerate()
            .max_by_key(|(_, &c)| c)
            .map(|(h, c)| (h, *c));

        if let Some((h, count)) = peak {
            if count >= 3 {
                let period = match h {
                    5..=11  => "morning",
                    12..=17 => "afternoon",
                    18..=22 => "evening",
                    _       => "night/early-morning",
                };
                let insight = format!(
                    "[Dream:TimePattern] Peak activity: {:02}:00 UTC ({} memories, {} period). \
                     This is your cognitive prime window.",
                    h, count, period
                );
                self.push_discovery("time_pattern", &format!("{:02}:00 UTC peak ({} memories)", h, count), count as usize, 0.72);
                return vec![(insight, "insight".to_string(), 0.72)];
            }
        }
        vec![]
    }

    // ── 5. Stress signals: recurring struggle ─────────────────────────────────

    fn find_stress_signals(&mut self, memories: &[Memory]) -> Vec<(String, String, f64)> {
        const STRESS: &[&str] = &[
            "problem", "issue", "broken", "failed", "wrong", "error",
            "cannot", "stuck", "confused", "bug", "fix", "crash",
        ];

        let stress_mems: Vec<_> = memories.iter()
            .filter(|m| {
                let lower = m.content.to_lowercase();
                STRESS.iter().any(|&w| lower.contains(w)) && m.importance >= 0.65
            })
            .collect();

        if stress_mems.len() >= 3 {
            // Find the dominant stress topic
            let stress_freq = word_freq_slice(&stress_mems);
            let top_stress = stress_freq.iter()
                .max_by_key(|(_, &c)| c)
                .map(|(t, _)| t.as_str())
                .unwrap_or("unknown");

            let insight = format!(
                "[Dream:StressSignal] {} high-importance memories contain struggle indicators. \
                 Dominant struggle topic: '{}'. Recurring challenge — worth confronting directly.",
                stress_mems.len(), top_stress
            );
            self.push_discovery("stress", &format!("{} struggle memories, topic: '{}'", stress_mems.len(), top_stress), stress_mems.len(), 0.85);
            return vec![(insight, "insight".to_string(), 0.88)];
        }
        vec![]
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn push_discovery(&mut self, kind: &str, desc: &str, evidence: usize, confidence: f64) {
        let id = self.discoveries.len() as u64 + 1;
        self.discoveries.push(PatternDiscovery {
            id,
            kind:           kind.to_string(),
            description:    desc.to_string(),
            evidence_count: evidence,
            confidence,
            discovered_at:  crate::now(),
        });
        // Keep max 200 discoveries
        if self.discoveries.len() > 200 {
            self.discoveries.remove(0);
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "total_dreams":           self.total_dreams,
            "last_deep_dream":        self.last_deep_dream_ts,
            "patterns_discovered":    self.discoveries.len(),
            "consolidated_clusters":  self.consolidated_clusters,
            "top_patterns": self.discoveries.iter().rev().take(10).map(|d| serde_json::json!({
                "id":         d.id,
                "kind":       d.kind,
                "pattern":    d.description,
                "evidence":   d.evidence_count,
                "confidence": format!("{:.0}%", d.confidence * 100.0),
                "found_at":   d.discovered_at,
            })).collect::<Vec<_>>(),
        })
    }
}

impl Default for DreamEngine {
    fn default() -> Self { Self::new() }
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

fn word_freq_all(memories: &[Memory]) -> HashMap<String, usize> {
    let mut freq = HashMap::new();
    for m in memories {
        for w in m.content.split_whitespace() {
            let w = clean_word(w);
            if w.len() >= 5 && !is_stop(&w) {
                *freq.entry(w).or_insert(0) += 1;
            }
        }
    }
    freq
}

fn word_freq_slice(memories: &[&Memory]) -> HashMap<String, usize> {
    let mut freq = HashMap::new();
    for m in memories {
        for w in m.content.split_whitespace() {
            let w = clean_word(w);
            if w.len() >= 4 && !is_stop(&w) {
                *freq.entry(w).or_insert(0) += 1;
            }
        }
    }
    freq
}

fn clean_word(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

fn parse_hour(ts: &str) -> Option<u8> {
    // "2026-07-01T14:30:00Z" → 14
    ts.split('T').nth(1)
        .and_then(|t| t.split(':').next())
        .and_then(|h| h.parse::<u8>().ok())
}

const STOP_WORDS_D: &[&str] = &[
    "which", "where", "their", "there", "these", "those", "would", "could", "should",
    "about", "after", "being", "every", "other", "since", "under", "until", "while",
    "layer", "build", "built", "using", "based", "bench", "types", "store", "value",
    "query", "block", "added", "total", "result", "column", "memory", "string",
    "function", "method", "vector", "number", "object", "struct", "returns", "always",
    "never", "first", "second", "third", "final", "kore0", "kore1",
];

fn is_stop(w: &str) -> bool {
    STOP_WORDS_D.contains(&w)
}
