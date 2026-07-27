// identity.rs — Layer 65: Who you are. Not static — evolves with every memory.

use std::collections::HashMap;
use serde::{Serialize, Deserialize};

/// The deep model of a person — values, thinking style, voice, beliefs.
/// Never manually edited. Entirely learned from memories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityModel {
    pub owner:    String,
    pub values:   Vec<CoreValue>,
    pub thinking: ThinkingStyle,
    pub voice:    VoiceProfile,
    pub beliefs:  HashMap<String, Belief>,
}

/// Something you care about — learned from how you talk and decide.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreValue {
    pub name:     String,
    pub strength: f64,   // 0.0–1.0
    pub evidence: u32,   // how many memories support this
}

impl CoreValue {
    /// Normalized strength for use in fingerprint vectors (same as strength).
    pub fn value_norm(&self) -> f64 { self.strength }
}

/// How you think — learned from decision patterns and language.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingStyle {
    pub metrics_driven: f64,  // uses benchmarks/numbers to decide
    pub risk_tolerance: f64,  // bold vs cautious
    pub decision_speed: f64,  // quick vs deliberate
    pub perfectionism:  f64,  // "good enough" vs perfect
}

/// How you write and communicate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceProfile {
    pub directness:      f64,  // "do X" vs "maybe consider doing X"
    pub technical_depth: f64,  // ratio of technical terms
    pub certainty:       f64,  // confident assertions vs hedging
}

/// A tracked belief about a topic — including full contradiction history.
#[derive(Debug, Clone, Serialize, Deserialize)]
/// A tracked belief — with evidence, contradiction history, and wisdom.
/// KORE v3: beliefs are not just stances. They are living epistemic records.
pub struct Belief {
    pub topic:          String,
    pub stance:         String,
    pub confidence:     f64,
    pub formed_at:      String,
    pub updated_at:     String,
    pub history:        Vec<String>,   // past stances with timestamps
    // ── Wisdom layer additions ─────────────────────────────────────────────
    pub evidence_for:   Vec<String>,   // memories/events that support this belief
    pub evidence_against: Vec<String>, // what contradicts it
    pub change_reason:  String,        // why the last change happened
    pub version:        u32,           // how many times this belief has changed
}

impl IdentityModel {
    pub fn new(owner: &str) -> Self {
        Self {
            owner: owner.to_string(),
            values: vec![],
            thinking: ThinkingStyle {
                metrics_driven: 0.5,
                risk_tolerance: 0.5,
                decision_speed: 0.5,
                perfectionism:  0.5,
            },
            voice: VoiceProfile {
                directness:      0.5,
                technical_depth: 0.5,
                certainty:       0.5,
            },
            beliefs: HashMap::new(),
        }
    }

    /// Learn from a new memory — continuously refines identity signals.
    pub fn absorb(&mut self, content: &str, kind: &str, importance: f64) {
        let lower = content.to_lowercase();
        let words = content.split_whitespace().count();

        // ── Core value signals ─────────────────────────────────────────────
        if lower.contains("performance") || lower.contains("faster") || lower.contains("speedup") {
            self.bump_value("performance", importance * 0.07);
        }
        if lower.contains("privacy") || lower.contains("local") || lower.contains("never leaves") {
            self.bump_value("privacy", importance * 0.07);
        }
        if lower.contains("simple") || lower.contains("minimal") || lower.contains("no dep") || lower.contains("zero dep") {
            self.bump_value("simplicity", importance * 0.07);
        }
        if lower.contains("rust") || lower.contains("safe") || lower.contains("correct") || lower.contains("verified") {
            self.bump_value("correctness", importance * 0.05);
        }
        if lower.contains("benchmark") || lower.contains("tpc-h") || lower.contains("ms") {
            self.bump_value("measurement", importance * 0.04);
        }

        // ── Thinking style signals ─────────────────────────────────────────
        // Numbers + units = metrics-driven thinking
        let has_metrics = (lower.contains("ms") || lower.contains("mb") || lower.contains("gb")
            || lower.contains('%') || lower.contains('x'))
            && content.chars().any(|c| c.is_ascii_digit());
        if has_metrics {
            self.thinking.metrics_driven = lerp(self.thinking.metrics_driven, 0.9, 0.04);
        }
        // Decisions reveal speed and risk tolerance
        if kind == "decision" {
            self.thinking.decision_speed = lerp(self.thinking.decision_speed, 0.7, 0.03);
        }
        // High-importance decisions = low risk tolerance (careful)
        if kind == "decision" && importance >= 0.9 {
            self.thinking.perfectionism = lerp(self.thinking.perfectionism, 0.8, 0.03);
        }

        // ── Voice profile signals ──────────────────────────────────────────
        if words < 20 {
            self.voice.directness = lerp(self.voice.directness, 0.85, 0.04);
        }
        let tech_terms = ["rust", "fn ", "impl ", "struct ", "enum ", "trait ", "async ",
                          "tokio", "rayon", "datablock", "columnar", "hashmap", "vec<"];
        let tech_hits = tech_terms.iter().filter(|&&t| lower.contains(t)).count();
        if tech_hits > 0 {
            let ratio = (tech_hits as f64 / words.max(1) as f64 * 15.0).min(1.0);
            self.voice.technical_depth = lerp(self.voice.technical_depth, 0.6 + ratio * 0.4, 0.05);
        }
        if lower.contains("always") || lower.contains("never") || lower.contains("must") || lower.contains("zero") {
            self.voice.certainty = lerp(self.voice.certainty, 0.85, 0.03);
        }
    }

    fn bump_value(&mut self, name: &str, delta: f64) {
        if let Some(v) = self.values.iter_mut().find(|v| v.name == name) {
            v.strength = (v.strength + delta).min(1.0);
            v.evidence += 1;
        } else {
            self.values.push(CoreValue {
                name:     name.to_string(),
                strength: (0.25 + delta).min(1.0),
                evidence: 1,
            });
        }
        self.values.sort_by(|a, b| b.strength.partial_cmp(&a.strength)
            .unwrap_or(std::cmp::Ordering::Equal));
    }

    /// Update or create a belief. Returns Some(msg) if a contradiction was detected.
    pub fn update_belief(&mut self, topic: &str, stance: &str, confidence: f64) -> Option<String> {
        self.update_belief_with_reason(topic, stance, confidence, "")
    }

    /// Update a belief with explicit change_reason and optional evidence.
    pub fn update_belief_with_reason(&mut self, topic: &str, stance: &str, confidence: f64, reason: &str) -> Option<String> {
        let ts = crate::now();
        if let Some(b) = self.beliefs.get_mut(topic) {
            if b.stance != stance {
                // ── CONTRADICTION → WISDOM EVENT ──────────────────────────
                let old = b.stance.clone();
                let old_conf = b.confidence;
                b.history.push(format!("[{}] was: '{}' ({:.0}%)", b.updated_at, old, old_conf*100.0));
                // Move old stance to evidence_against (it was believed but is now rejected)
                b.evidence_against.push(format!("[{}] old belief: '{}' ({:.0}% confidence)", &b.updated_at[..10], &old[..old.len().min(60)], old_conf*100.0));
                b.stance       = stance.to_string();
                b.confidence   = confidence;
                b.updated_at   = ts;
                b.change_reason = if reason.is_empty() { format!("Stance reversed from '{}'", &old[..old.len().min(40)]) } else { reason.to_string() };
                b.version     += 1;
                return Some(format!(
                    "BELIEF CHANGED [v{}]: '{}'\n\
                     Was:    '{}' ({:.0}% confidence)\n\
                     Now:    '{}' ({:.0}% confidence)\n\
                     Reason: {}\n\
                     This belief has changed {} time(s).",
                    b.version, topic,
                    old, old_conf*100.0,
                    stance, confidence*100.0,
                    b.change_reason,
                    b.history.len()
                ));
            }
            // Same stance — reinforce confidence + add supporting evidence
            b.confidence = lerp(b.confidence, confidence, 0.1);
            b.updated_at = ts;
            if !reason.is_empty() {
                b.evidence_for.push(format!("[{}] {}", &b.updated_at[..10], &reason[..reason.len().min(80)]));
                if b.evidence_for.len() > 10 { b.evidence_for.drain(0..5); }
            }
        } else {
            let mut initial_evidence = vec![];
            if !reason.is_empty() { initial_evidence.push(format!("[formed] {}", &reason[..reason.len().min(80)])); }
            self.beliefs.insert(topic.to_string(), Belief {
                topic:          topic.to_string(),
                stance:         stance.to_string(),
                confidence,
                formed_at:      ts.clone(),
                updated_at:     ts,
                history:        vec![],
                evidence_for:   initial_evidence,
                evidence_against: vec![],
                change_reason:  String::new(),
                version:        0,
            });
        }
        None
    }

    /// Add evidence for or against a belief without changing the stance.
    pub fn add_evidence(&mut self, topic: &str, evidence: &str, supports: bool) {
        if let Some(b) = self.beliefs.get_mut(topic) {
            let entry = format!("[{}] {}", &crate::now()[..10], &evidence[..evidence.len().min(100)]);
            if supports {
                b.evidence_for.push(entry);
                b.confidence = (b.confidence + 0.02).min(1.0);
                if b.evidence_for.len() > 10 { b.evidence_for.drain(0..5); }
            } else {
                b.evidence_against.push(entry);
                b.confidence = (b.confidence - 0.03).max(0.0);
                if b.evidence_against.len() > 10 { b.evidence_against.drain(0..5); }
            }
        }
    }

    pub fn top_values(&self, n: usize) -> &[CoreValue] {
        &self.values[..self.values.len().min(n)]
    }

    pub fn summary(&self) -> String {
        let vals: Vec<String> = self.top_values(4)
            .iter()
            .map(|v| format!("{}:{:.0}%", v.name, v.strength * 100.0))
            .collect();
        format!(
            "{}[{}|metrics:{:.0}%|tech:{:.0}%|direct:{:.0}%|beliefs:{}]",
            self.owner,
            vals.join(","),
            self.thinking.metrics_driven * 100.0,
            self.voice.technical_depth * 100.0,
            self.voice.directness * 100.0,
            self.beliefs.len(),
        )
    }

    pub fn to_json(&self) -> serde_json::Value {
        let contradictions: Vec<_> = self.beliefs.values()
            .filter(|b| !b.history.is_empty())
            .map(|b| serde_json::json!({
                "topic": b.topic,
                "current": b.stance,
                "changed": b.history.len(),
                "history": b.history,
            }))
            .collect();

        serde_json::json!({
            "owner": self.owner,
            "core_values": self.top_values(6).iter().map(|v| serde_json::json!({
                "name": v.name,
                "strength": format!("{:.1}%", v.strength * 100.0),
                "evidence": v.evidence,
            })).collect::<Vec<_>>(),
            "thinking_style": {
                "metrics_driven":  format!("{:.0}%", self.thinking.metrics_driven * 100.0),
                "risk_tolerance":  format!("{:.0}%", self.thinking.risk_tolerance * 100.0),
                "decision_speed":  format!("{:.0}%", self.thinking.decision_speed * 100.0),
                "perfectionism":   format!("{:.0}%", self.thinking.perfectionism  * 100.0),
            },
            "voice_profile": {
                "directness":      format!("{:.0}%", self.voice.directness      * 100.0),
                "technical_depth": format!("{:.0}%", self.voice.technical_depth * 100.0),
                "certainty":       format!("{:.0}%", self.voice.certainty       * 100.0),
            },
            "beliefs_tracked":      self.beliefs.len(),
            "belief_contradictions": contradictions,
        })
    }
}

#[inline]
fn lerp(a: f64, b: f64, t: f64) -> f64 { a + (b - a) * t }
