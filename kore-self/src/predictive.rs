// kore-self  —  Phase 3: Predictive Self
//
// "Based on 847+ past decisions, you would choose performance over readability here — 94% confidence"
//
// Pure algorithmic prediction using KORE's own pattern analysis.
// No external LLM. No magic. Just your own history.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::Memory;

// ─── Data types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionPattern {
    pub id:           u64,
    pub context:      String,   // keywords that trigger this pattern
    pub choice:       String,   // the choice that was made
    pub count:        u32,      // how many times
    pub confidence:   f64,      // 0.0 – 1.0
    pub last_seen:    String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    pub for_context:  String,
    pub predicted:    String,
    pub confidence:   f64,      // 0.0 – 1.0
    pub basis:        String,   // explanation
    pub made_at:      String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contradiction {
    pub id:           u64,
    pub topic:        String,
    pub old_stance:   String,
    pub new_stance:   String,
    pub detected_at:  String,
    pub resolved:     bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictiveEngine {
    pub total_predictions:   u64,
    pub correct_predictions: u64,    // future: can verify if user confirms
    pub patterns:            Vec<DecisionPattern>,
    pub recent_predictions:  Vec<Prediction>,   // last 50
    pub contradictions:      Vec<Contradiction>,
    pub next_id:             u64,
}

impl PredictiveEngine {
    pub fn new() -> Self {
        Self {
            total_predictions:   0,
            correct_predictions: 0,
            patterns:            vec![],
            recent_predictions:  vec![],
            contradictions:      vec![],
            next_id:             1,
        }
    }

    // ── Core: build patterns from memory history ──────────────────────────────

    /// Re-analyze ALL memories to build/refresh decision patterns.
    /// Called automatically by KoreSelf::tick() every 50+ memories.
    pub fn analyze_memories(&mut self, memories: &[Memory]) {
        if memories.len() < 10 { return; }

        // Build topic→choices map: for each "decision" memory, what choice was made?
        let mut topic_choices: HashMap<String, Vec<(String, f64)>> = HashMap::new();

        for m in memories {
            if m.kind != "decision" && m.kind != "insight" { continue; }

            let words = extract_keywords(&m.content);
            let choice = extract_choice(&m.content);
            if choice.is_empty() { continue; }

            for word in &words {
                topic_choices
                    .entry(word.clone())
                    .or_default()
                    .push((choice.clone(), m.importance));
            }
        }

        // Build patterns from dominant choices per topic
        self.patterns.clear();
        for (topic, choices) in &topic_choices {
            if choices.len() < 2 { continue; }  // need at least 2 data points

            // Count each choice
            let mut counts: HashMap<&str, (u32, f64)> = HashMap::new();
            for (c, imp) in choices {
                let e = counts.entry(c.as_str()).or_insert((0, 0.0));
                e.0 += 1;
                e.1 += imp;
            }

            // Find dominant choice
            let total = choices.len() as f64;
            if let Some((choice, (count, _))) = counts.iter().max_by_key(|(_, v)| v.0) {
                let confidence = *count as f64 / total;
                if confidence >= 0.6 {   // only patterns with ≥60% consistency
                    self.patterns.push(DecisionPattern {
                        id:         self.next_id,
                        context:    topic.clone(),
                        choice:     choice.to_string(),
                        count:      *count,
                        confidence,
                        last_seen:  crate::now(),
                    });
                    self.next_id += 1;
                }
            }
        }

        // Keep top 200 patterns by confidence
        self.patterns.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        self.patterns.truncate(200);
    }

    // ── Predict future choice ─────────────────────────────────────────────────

    /// Given a context string, predict what choice the user would make.
    /// Returns None if insufficient data (< 3 matching patterns).
    pub fn predict(&mut self, context: &str) -> Option<Prediction> {
        let keywords = extract_keywords(context);
        if keywords.is_empty() { return None; }

        // Find patterns matching any keyword
        let mut matches: Vec<&DecisionPattern> = self.patterns.iter()
            .filter(|p| keywords.iter().any(|k| p.context.contains(k.as_str()) || k.contains(p.context.as_str())))
            .collect();

        if matches.len() < 2 { return None; }

        // Weight by confidence
        matches.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

        // Vote: which choice wins across matched patterns?
        let mut votes: HashMap<&str, (f64, u32)> = HashMap::new();
        for p in &matches {
            let e = votes.entry(&p.choice).or_insert((0.0, 0));
            e.0 += p.confidence;
            e.1 += p.count;
        }

        let best = votes.iter().max_by(|a, b| a.1.0.partial_cmp(&b.1.0).unwrap())?;
        let total_weight: f64 = votes.values().map(|(w, _)| w).sum();
        let final_confidence = if total_weight > 0.0 { best.1.0 / total_weight } else { 0.0 };

        if final_confidence < 0.55 { return None; }  // not confident enough

        let basis = format!(
            "Based on {} matching patterns ({} total decisions analyzed)",
            matches.len(),
            matches.iter().map(|p| p.count as usize).sum::<usize>()
        );

        let pred = Prediction {
            for_context: context.to_string(),
            predicted:   best.0.to_string(),
            confidence:  final_confidence,
            basis,
            made_at:     crate::now(),
        };

        // Keep last 50 predictions
        self.recent_predictions.push(pred.clone());
        if self.recent_predictions.len() > 50 {
            self.recent_predictions.remove(0);
        }
        self.total_predictions += 1;

        Some(pred)
    }

    // ── Contradiction detection ───────────────────────────────────────────────

    /// Compare a new stance against all patterns related to a topic.
    /// Returns a contradiction record if a reversal is detected.
    pub fn check_contradiction(&mut self, topic: &str, new_stance: &str) -> Option<Contradiction> {
        let keywords = extract_keywords(topic);

        // Find the dominant historical stance on this topic
        let related: Vec<&DecisionPattern> = self.patterns.iter()
            .filter(|p| keywords.iter().any(|k| p.context.contains(k.as_str())))
            .collect();

        if related.is_empty() { return None; }

        // Dominant past choice
        let mut votes: HashMap<&str, f64> = HashMap::new();
        for p in &related {
            *votes.entry(&p.choice).or_insert(0.0) += p.confidence * p.count as f64;
        }
        let past_choice = votes.iter().max_by(|a, b| a.1.partial_cmp(b.1).unwrap())?.0;

        // Detect contradiction: new stance is opposite to past pattern
        if is_opposite(past_choice, new_stance) {
            let c = Contradiction {
                id:          self.next_id,
                topic:       topic.to_string(),
                old_stance:  past_choice.to_string(),
                new_stance:  new_stance.to_string(),
                detected_at: crate::now(),
                resolved:    false,
            };
            self.contradictions.push(c.clone());
            self.next_id += 1;

            // Keep last 100
            if self.contradictions.len() > 100 {
                self.contradictions.remove(0);
            }

            return Some(c);
        }

        None
    }

    // ── Summary ───────────────────────────────────────────────────────────────

    pub fn accuracy_pct(&self) -> f64 {
        if self.total_predictions == 0 { return 0.0; }
        self.correct_predictions as f64 / self.total_predictions as f64 * 100.0
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "total_predictions":    self.total_predictions,
            "correct_predictions":  self.correct_predictions,
            "accuracy_pct":         format!("{:.1}%", self.accuracy_pct()),
            "patterns_learned":     self.patterns.len(),
            "contradictions":       self.contradictions.len(),
            "unresolved_contradictions": self.contradictions.iter().filter(|c| !c.resolved).count(),
            "top_patterns": self.patterns.iter().take(10).map(|p| serde_json::json!({
                "context":    p.context,
                "choice":     p.choice,
                "confidence": format!("{:.0}%", p.confidence * 100.0),
                "count":      p.count,
            })).collect::<Vec<_>>(),
            "recent_contradictions": self.contradictions.iter().rev().take(5).map(|c| serde_json::json!({
                "topic":      c.topic,
                "old_stance": c.old_stance,
                "new_stance": c.new_stance,
                "when":       c.detected_at,
                "resolved":   c.resolved,
            })).collect::<Vec<_>>(),
        })
    }
}

impl Default for PredictiveEngine {
    fn default() -> Self { Self::new() }
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Extract meaningful keywords from a content string.
fn extract_keywords(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| w.len() >= 5 && !PRED_STOPS.contains(&w.as_str()))
        .collect()
}

/// Try to extract the "choice" from a decision memory.
/// Looks for patterns like "chose X", "decided X", "prefer X", "chose X over Y".
fn extract_choice(text: &str) -> String {
    let lower = text.to_lowercase();
    let markers = ["chose ", "choose ", "decided ", "prefer ", "using ", "picked "];
    for marker in &markers {
        if let Some(pos) = lower.find(marker) {
            let after = &text[pos + marker.len()..];
            let word: String = after.split_whitespace().next().unwrap_or("").to_string();
            let word = word.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
            if word.len() >= 3 {
                return word.to_lowercase();
            }
        }
    }
    // Fallback: first significant word
    extract_keywords(text).into_iter().next().unwrap_or_default()
}

/// Detect if two stances are semantically opposite.
fn is_opposite(a: &str, b: &str) -> bool {
    let opposites = [
        ("performance", "readability"),
        ("readability", "performance"),
        ("fast", "slow"),
        ("simple", "complex"),
        ("complex", "simple"),
        ("safe", "unsafe"),
        ("unsafe", "safe"),
        ("sync", "async"),
        ("async", "sync"),
        ("monolith", "microservices"),
        ("microservices", "monolith"),
        ("yes", "no"),
        ("no", "yes"),
        ("approve", "reject"),
        ("reject", "approve"),
    ];
    let al = a.to_lowercase();
    let bl = b.to_lowercase();
    opposites.iter().any(|(x, y)| al.contains(x) && bl.contains(y))
}

const PRED_STOPS: &[&str] = &[
    "would", "could", "should", "think", "about", "there", "their", "these",
    "those", "which", "while", "where", "other", "after", "before", "every",
    "often", "never", "always", "maybe", "being", "doing", "going", "thing",
    "things", "using", "based", "makes", "might", "needs", "still", "great",
];
