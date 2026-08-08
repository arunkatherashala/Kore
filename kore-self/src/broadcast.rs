// kore-self  —  Broadcast Protocol: MIND.kore
//
// "Not a message to aliens. A fingerprint of how a human thinks."
//
// MIND.kore is a universal, language-agnostic cognitive fingerprint.
// Pure mathematics + patterns. No natural language dependency.
// Any intelligence — human, AI, or otherwise — can parse and understand it.
//
// self_broadcast  → generate MIND.kore from your identity + patterns
// self_merge      → load someone else's MIND.kore into your perspective
// self_perspectives → compare minds: where do they align? where diverge?
//
// Inspired by Voyager Golden Record — but instead of sounds and images,
// this is HOW A HUMAN ACTUALLY THINKS.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::Memory;
use crate::identity::IdentityModel;
use crate::dream::DreamEngine;
use crate::predictive::PredictiveEngine;
use crate::kore_query;

// ─── MIND.kore format ─────────────────────────────────────────────────────────

/// The universal cognitive fingerprint.
/// Self-contained. No external deps to read.
/// Version-stamped for future compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MindExport {
    pub format:        String,   // "MIND.kore/v1"
    pub generated_at:  String,
    pub owner:         String,

    /// Pure mathematical representation of how this mind thinks.
    pub fingerprint:   CognitiveFingerprint,

    /// Statistical summary of what this mind has experienced.
    pub histogram:     MemoryHistogram,

    /// The mind's evolution: how it changed over time.
    pub evolution:     MindEvolution,

    /// Integrity checksum (simple, readable).
    pub checksum:      String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitiveFingerprint {
    /// Core values as a normalized vector (0.0–1.0).
    /// Language-free: pure weights. Any intelligence can compare two minds by cosine similarity.
    pub values:              Vec<(String, f64)>,

    /// Thinking style dimensions.
    pub thinking:            HashMap<String, f64>,

    /// Communication style dimensions.
    pub voice:               HashMap<String, f64>,

    /// Top obsessions — concepts this mind returns to repeatedly.
    pub obsessions:          Vec<String>,

    /// Decision patterns: (context_signal, consistent_choice, confidence).
    /// Reveals decision-making under uncertainty.
    pub decision_patterns:   Vec<DecisionEntry>,

    /// Belief contradictions — where the mind changed course.
    pub contradictions:      usize,

    /// Cognitive fingerprint hash — unique per mind, stable over time.
    /// Computed from values + thinking vectors.
    pub fingerprint_hash:    String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEntry {
    pub signal:     String,
    pub choice:     String,
    pub confidence: f64,
    pub frequency:  u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryHistogram {
    pub total:                usize,
    pub by_kind:              HashMap<String, usize>,
    pub avg_importance:       f64,
    pub high_importance_count: usize,  // importance >= 0.8
    pub importance_p50:       f64,     // median
    pub importance_p90:       f64,     // 90th percentile
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MindEvolution {
    /// How did values shift from early → late memories?
    pub value_drift:     Vec<(String, f64, f64)>,  // (value, early_strength, late_strength)
    /// Net trajectory: "growing" | "stable" | "declining"
    pub trajectory:      String,
    /// Dominant transition: what the mind moved FROM and TO
    pub primary_shift:   Option<(String, String)>,
}

// ─── Merged Mind ──────────────────────────────────────────────────────────────

/// A foreign mind loaded via self_merge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergedMind {
    pub source_file: String,
    pub mind:        MindExport,
    pub loaded_at:   String,
    pub alignment:   f64,    // 0.0–1.0: cosine similarity of value vectors vs self
    pub divergence:  Vec<String>,  // key differences
}

// ─── Broadcast Engine ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastEngine {
    pub broadcasts:    Vec<String>,    // paths to generated MIND.kore files
    pub merged_minds:  Vec<MergedMind>,
    pub total_broadcast: u32,
}

impl BroadcastEngine {
    pub fn new() -> Self {
        Self {
            broadcasts:      vec![],
            merged_minds:    vec![],
            total_broadcast: 0,
        }
    }

    // ── self_broadcast ────────────────────────────────────────────────────────

    pub fn broadcast(
        &mut self,
        owner:      &str,
        memories:   &[Memory],
        identity:   &IdentityModel,
        dream:      &DreamEngine,
        predictive: &PredictiveEngine,
    ) -> (MindExport, String) {
        let export = self.build_export(owner, memories, identity, dream, predictive);

        // Write to disk
        let ts   = crate::now().replace(':', "-").replace(' ', "T");
        let path = broadcast_dir(owner).join(format!("MIND_{ts}.kore"));
        fs::create_dir_all(path.parent().unwrap()).ok();

        let json = serde_json::to_string_pretty(&export).unwrap_or_default();
        if let Err(e) = fs::write(&path, json.as_bytes()) {
            eprintln!("[kore-self:broadcast] Write failed: {e}");
        }

        let path_str = path.to_string_lossy().to_string();
        self.broadcasts.push(path_str.clone());
        self.total_broadcast += 1;

        // Keep last 10
        if self.broadcasts.len() > 10 { self.broadcasts.remove(0); }

        (export, path_str)
    }

    fn build_export(
        &self,
        owner:      &str,
        memories:   &[Memory],
        identity:   &IdentityModel,
        dream:      &DreamEngine,
        predictive: &PredictiveEngine,
    ) -> MindExport {
        // ── Fingerprint ───────────────────────────────────────────────────────
        let values: Vec<(String, f64)> = identity.top_values(10)
            .iter().map(|v| (v.name.clone(), v.value_norm())).collect();

        let mut thinking = HashMap::new();
        thinking.insert("metrics_driven".to_string(), identity.thinking.metrics_driven);
        thinking.insert("risk_tolerance".to_string(), identity.thinking.risk_tolerance);
        thinking.insert("decision_speed".to_string(), identity.thinking.decision_speed);
        thinking.insert("perfectionism".to_string(),  identity.thinking.perfectionism);

        let mut voice = HashMap::new();
        voice.insert("directness".to_string(),      identity.voice.directness);
        voice.insert("technical_depth".to_string(), identity.voice.technical_depth);
        voice.insert("certainty".to_string(),        identity.voice.certainty);

        let obsessions: Vec<String> = dream.discoveries.iter()
            .filter(|d| d.kind == "obsession")
            .take(5)
            .map(|d| {
                // extract topic from description like "'perf' appears in 4 memories"
                d.description.split('\'').nth(1).unwrap_or(&d.description).to_string()
            })
            .collect();

        let decision_patterns: Vec<DecisionEntry> = predictive.patterns.iter()
            .take(10)
            .map(|p| DecisionEntry {
                signal:     p.context.clone(),
                choice:     p.choice.clone(),
                confidence: p.confidence,
                frequency:  p.count,
            })
            .collect();

        let fp_hash = fingerprint_hash(&values, &thinking);

        let fingerprint = CognitiveFingerprint {
            values,
            thinking,
            voice,
            obsessions,
            decision_patterns,
            contradictions: identity.beliefs.values()
                .filter(|b| !b.history.is_empty()).count(),
            fingerprint_hash: fp_hash,
        };

        // ── Histogram via KORE SQL ─────────────────────────────────────────────
        let kind_dist = kore_query::kind_distribution(memories);
        let high_imp  = kore_query::high_importance(memories, 0.8);

        let by_kind: HashMap<String, usize> = kind_dist.iter()
            .map(|(k, c, _)| (k.clone(), *c as usize))
            .collect();
        let avg_importance: f64 = if kind_dist.is_empty() { 0.0 } else {
            kind_dist.iter().map(|(_, c, avg)| avg * *c as f64).sum::<f64>()
            / kind_dist.iter().map(|(_, c, _)| *c as f64).sum::<f64>()
        };

        let mut imps: Vec<f64> = memories.iter().map(|m| m.importance).collect();
        imps.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p50 = percentile(&imps, 0.50);
        let p90 = percentile(&imps, 0.90);

        let histogram = MemoryHistogram {
            total:                memories.len(),
            by_kind,
            avg_importance,
            high_importance_count: high_imp.len(),
            importance_p50:       p50,
            importance_p90:       p90,
        };

        // ── Evolution: early vs late value strengths ───────────────────────────
        let half = (memories.len() / 2).max(1);
        let early = &memories[..half.min(memories.len())];
        let late  = &memories[memories.len().saturating_sub(half)..];

        let early_vals = value_weights(early);
        let late_vals  = value_weights(late);

        let mut value_drift = vec![];
        let mut growing = 0i32;
        let mut shrinking = 0i32;
        for (k, late_w) in &late_vals {
            let early_w = early_vals.get(k).copied().unwrap_or(0.0);
            let drift = late_w - early_w;
            if drift.abs() > 0.05 {
                value_drift.push((k.clone(), early_w, *late_w));
                if drift > 0.0 { growing += 1; } else { shrinking += 1; }
            }
        }
        value_drift.sort_by(|a, b| (b.2 - b.1).abs().partial_cmp(&(a.2 - a.1).abs()).unwrap());

        let trajectory = if growing > shrinking { "growing".to_string() }
                         else if shrinking > growing { "shifting".to_string() }
                         else { "stable".to_string() };

        let primary_shift = value_drift.first().map(|(k, e, l)| {
            if l > e { ("uncertainty".to_string(), k.clone()) }
            else { (k.clone(), "focus".to_string()) }
        });

        let evolution = MindEvolution { value_drift, trajectory, primary_shift };

        // ── Checksum ──────────────────────────────────────────────────────────
        let checksum = format!("MIND-{owner}-{}-mem{}-v1",
            &fingerprint.fingerprint_hash[..8.min(fingerprint.fingerprint_hash.len())],
            memories.len());

        MindExport {
            format:       "MIND.kore/v1".to_string(),
            generated_at: crate::now(),
            owner:        owner.to_string(),
            fingerprint,
            histogram,
            evolution,
            checksum,
        }
    }

    // ── self_merge ────────────────────────────────────────────────────────────

    /// Load a foreign MIND.kore file and compute alignment with self.
    pub fn merge(
        &mut self,
        path:     &str,
        identity: &IdentityModel,
    ) -> Result<MergedMind, String> {
        let bytes = fs::read(path).map_err(|e| format!("Cannot read {path}: {e}"))?;
        let mind: MindExport = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid MIND.kore: {e}"))?;

        // Compute alignment: cosine similarity of value vectors
        let self_vals: HashMap<String, f64> = identity.top_values(10)
            .iter().map(|v| (v.name.clone(), v.value_norm())).collect();
        let other_vals: HashMap<String, f64> = mind.fingerprint.values.iter().cloned().collect();

        let alignment = cosine_similarity(&self_vals, &other_vals);

        // Find key divergences
        let mut divergence = vec![];
        for (k, sv) in &self_vals {
            if let Some(ov) = other_vals.get(k) {
                let diff = (sv - ov).abs();
                if diff > 0.25 {
                    divergence.push(format!(
                        "'{}': self={:.0}% vs {}={:.0}% (Δ{:.0}%)",
                        k, sv*100.0, mind.owner, ov*100.0, diff*100.0
                    ));
                }
            }
        }
        // Check opposite thinking styles
        let self_m  = identity.thinking.metrics_driven;
        let other_m = mind.fingerprint.thinking.get("metrics_driven").copied().unwrap_or(0.5);
        if (self_m - other_m).abs() > 0.3 {
            divergence.push(format!(
                "thinking style: self is {:.0}% metrics-driven vs {} at {:.0}%",
                self_m*100.0, mind.owner, other_m*100.0
            ));
        }

        let mm = MergedMind {
            source_file: path.to_string(),
            mind:        mind.clone(),
            loaded_at:   crate::now(),
            alignment,
            divergence,
        };
        self.merged_minds.push(mm.clone());

        // Keep max 10 merged minds
        if self.merged_minds.len() > 10 { self.merged_minds.remove(0); }

        Ok(mm)
    }

    // ── self_perspectives ─────────────────────────────────────────────────────

    pub fn perspectives_report(&self, identity: &IdentityModel) -> serde_json::Value {
        let self_vals: HashMap<String, f64> = identity.top_values(10)
            .iter().map(|v| (v.name.clone(), v.value_norm())).collect();

        let minds: Vec<_> = self.merged_minds.iter().map(|mm| {
            serde_json::json!({
                "owner":      mm.mind.owner,
                "alignment":  format!("{:.0}%", mm.alignment * 100.0),
                "divergence": mm.divergence,
                "their_top_values": mm.mind.fingerprint.values.iter().take(3)
                    .map(|(k, v)| format!("{}:{:.0}%", k, v*100.0))
                    .collect::<Vec<_>>(),
                "their_obsessions":  mm.mind.fingerprint.obsessions.iter().take(3).collect::<Vec<_>>(),
                "loaded_at":  mm.loaded_at,
            })
        }).collect();

        serde_json::json!({
            "self_owner":    identity.owner,
            "self_top_values": self_vals.iter()
                .map(|(k, v)| format!("{}:{:.0}%", k, v*100.0))
                .collect::<Vec<_>>(),
            "merged_minds":  self.merged_minds.len(),
            "perspectives":  minds,
            "broadcasts":    self.broadcasts.len(),
        })
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "total_broadcast": self.total_broadcast,
            "broadcasts":      self.broadcasts,
            "merged_minds":    self.merged_minds.len(),
        })
    }
}

impl Default for BroadcastEngine {
    fn default() -> Self { Self::new() }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn broadcast_dir(owner: &str) -> PathBuf {
    std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."))
        .join(".kore-self").join(owner).join("broadcast")
}

/// Compute a stable fingerprint hash from values + thinking.
fn fingerprint_hash(values: &[(String, f64)], thinking: &HashMap<String, f64>) -> String {
    let mut sig: u64 = 0xcafe_babe_dead_beef;
    for (k, v) in values {
        let bits = (*v * 1000.0) as u64;
        sig = sig.wrapping_mul(6364136223846793005)
                 .wrapping_add(bits)
                 .wrapping_add(k.len() as u64);
    }
    for (k, v) in thinking {
        let bits = (*v * 1000.0) as u64;
        sig = sig.wrapping_mul(6364136223846793005)
                 .wrapping_add(bits)
                 .wrapping_add(k.len() as u64);
    }
    format!("{sig:016x}")
}

/// Cosine similarity between two value maps.
fn cosine_similarity(a: &HashMap<String, f64>, b: &HashMap<String, f64>) -> f64 {
    let dot: f64 = a.iter()
        .filter_map(|(k, av)| b.get(k).map(|bv| av * bv))
        .sum();
    let mag_a: f64 = a.values().map(|v| v * v).sum::<f64>().sqrt();
    let mag_b: f64 = b.values().map(|v| v * v).sum::<f64>().sqrt();
    if mag_a == 0.0 || mag_b == 0.0 { return 0.0; }
    (dot / (mag_a * mag_b)).clamp(0.0, 1.0)
}

/// Extract value keyword weights from memories (for evolution analysis).
fn value_weights(memories: &[Memory]) -> HashMap<String, f64> {
    let mut map: HashMap<String, f64> = HashMap::new();
    let n = memories.len().max(1) as f64;
    for m in memories {
        for w in m.content.split_whitespace() {
            let w = w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
            if w.len() >= 5 {
                *map.entry(w).or_insert(0.0) += m.importance / n;
            }
        }
    }
    map
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() { return 0.0; }
    let idx = ((sorted.len() as f64 - 1.0) * p) as usize;
    sorted[idx.min(sorted.len() - 1)]
}
