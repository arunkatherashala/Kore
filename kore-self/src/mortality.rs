// kore-self  —  Phase 5: Mortality Protocol
//
// "You will outlive your hardware. Your mind will not."
//
// The Mortality Protocol creates an immortal, portable archive of the complete
// digital twin. Future generations (or future machines) can load it and ask
// questions of the past self.
//
// Design:
//   - Full self-export: all memories + identity + all engine states
//   - Human-readable manifest (what this person was, at a glance)
//   - Self-contained: no external deps to import
//   - Stamp: timestamp + memory count + key identity facts
//
// Tool: self_export  →  writes to ~/.kore-self/<owner>/immortal/<timestamp>/
//       self_epitaph →  generates the human-readable summary of who you were

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::Memory;
use crate::identity::IdentityModel;
use crate::consciousness::ConsciousnessState;
use crate::dream::DreamEngine;
use crate::predictive::PredictiveEngine;

// ─── Mortality Engine ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MortalityEngine {
    pub total_exports:   u32,
    pub last_export_at:  String,
    pub last_export_path: String,
}

impl MortalityEngine {
    pub fn new() -> Self {
        Self {
            total_exports:    0,
            last_export_at:   "never".to_string(),
            last_export_path: String::new(),
        }
    }

    /// Create a full immortal export.
    /// Writes to ~/.kore-self/<owner>/immortal/<timestamp>/
    /// Returns (export_path, manifest_summary)
    pub fn export(
        &mut self,
        owner:         &str,
        memories:      &[Memory],
        identity:      &IdentityModel,
        consciousness: &ConsciousnessState,
        dream:         &DreamEngine,
        predictive:    &PredictiveEngine,
    ) -> std::io::Result<(String, String)> {
        let ts      = crate::now().replace(':', "-").replace(' ', "T");
        let dir     = export_dir(owner, &ts);
        fs::create_dir_all(&dir)?;

        // 1. Full memories archive
        let mems_path = dir.join("memories.json");
        let mems_json = serde_json::to_string_pretty(memories)
            .unwrap_or_else(|_| "[]".to_string());
        fs::write(&mems_path, mems_json.as_bytes())?;

        // 2. Identity snapshot
        let id_path = dir.join("identity.json");
        let id_json = identity.to_json().to_string();
        fs::write(&id_path, id_json.as_bytes())?;

        // 3. Consciousness state
        let cs_path = dir.join("consciousness.json");
        let cs_json = consciousness.to_json().to_string();
        fs::write(&cs_path, cs_json.as_bytes())?;

        // 4. Dream patterns
        let dream_path = dir.join("dream_patterns.json");
        let dream_json = dream.to_json().to_string();
        fs::write(&dream_path, dream_json.as_bytes())?;

        // 5. Decision patterns
        let pred_path = dir.join("decision_patterns.json");
        let pred_json = predictive.to_json().to_string();
        fs::write(&pred_path, pred_json.as_bytes())?;

        // 6. Human-readable epitaph (the most important file)
        let epitaph = self.generate_epitaph(owner, memories, identity, consciousness, dream, predictive);
        let epitaph_path = dir.join("WHO_I_WAS.txt");
        fs::write(&epitaph_path, epitaph.as_bytes())?;

        // 7. Manifest (machine-readable index)
        let manifest = serde_json::json!({
            "version":           5,
            "exported_at":       crate::now(),
            "owner":             owner,
            "memories_total":    memories.len(),
            "consciousness_cycle": consciousness.cycle,
            "dream_patterns":    dream.discoveries.len(),
            "decision_patterns": predictive.patterns.len(),
            "top_values":        identity.top_values(5).iter().map(|v| &v.name).collect::<Vec<_>>(),
            "files": ["memories.json", "identity.json", "consciousness.json",
                      "dream_patterns.json", "decision_patterns.json", "WHO_I_WAS.txt", "MANIFEST.json"],
        });
        let manifest_path = dir.join("MANIFEST.json");
        fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).unwrap_or_default().as_bytes())?;

        let export_path = dir.to_string_lossy().to_string();
        self.total_exports   += 1;
        self.last_export_at   = crate::now();
        self.last_export_path = export_path.clone();

        Ok((export_path, epitaph))
    }

    /// Generate the human-readable WHO_I_WAS.txt epitaph.
    pub fn generate_epitaph(
        &self,
        owner:         &str,
        memories:      &[Memory],
        identity:      &IdentityModel,
        consciousness: &ConsciousnessState,
        dream:         &DreamEngine,
        predictive:    &PredictiveEngine,
    ) -> String {
        let values = identity.top_values(5);
        let val_str: String = values.iter()
            .map(|v| format!("  - {} ({:.0}% strength, {} evidence)",
                v.name, v.strength * 100.0, v.evidence))
            .collect::<Vec<_>>()
            .join("\n");

        let top_patterns: String = dream.discoveries.iter().rev().take(3)
            .map(|d| format!("  - [{}] {} ({:.0}% confidence)",
                d.kind, d.description, d.confidence * 100.0))
            .collect::<Vec<_>>()
            .join("\n");

        let decisions: String = predictive.patterns.iter().take(3)
            .map(|p| format!("  - When faced with '{}', chose '{}' ({:.0}% of the time)",
                p.context, p.choice, p.confidence * 100.0))
            .collect::<Vec<_>>()
            .join("\n");

        let recent_insight = memories.iter().rev()
            .filter(|m| m.kind == "insight" || m.kind == "reflection")
            .next()
            .map(|m| m.content.chars().take(300).collect::<String>())
            .unwrap_or_else(|| "(no insights recorded)".to_string());

        let thought_style = format!(
            "metrics-driven: {:.0}%  |  risk tolerance: {:.0}%  |  decision speed: {:.0}%  |  perfectionism: {:.0}%",
            identity.thinking.metrics_driven * 100.0,
            identity.thinking.risk_tolerance * 100.0,
            identity.thinking.decision_speed * 100.0,
            identity.thinking.perfectionism  * 100.0,
        );

        let voice_style = format!(
            "directness: {:.0}%  |  technical depth: {:.0}%  |  certainty: {:.0}%",
            identity.voice.directness      * 100.0,
            identity.voice.technical_depth * 100.0,
            identity.voice.certainty       * 100.0,
        );

        format!(r#"═══════════════════════════════════════════════════════════════
  WHO I WAS  —  {owner}
  Exported: {exported}
═══════════════════════════════════════════════════════════════

MEMORIES RECORDED: {mem_count}
CONSCIOUSNESS CYCLES: {cycles}
DREAM PATTERNS DISCOVERED: {patterns}
DECISIONS ANALYZED: {decisions_n}

═══════════════════════════════════════════════════════════════
  CORE VALUES  (what I believed in, measured by evidence)
═══════════════════════════════════════════════════════════════
{val_str}

═══════════════════════════════════════════════════════════════
  HOW I THOUGHT
═══════════════════════════════════════════════════════════════
{thought_style}

  HOW I SPOKE
{voice_style}

═══════════════════════════════════════════════════════════════
  PATTERNS THE DREAM ENGINE FOUND IN MY MIND
═══════════════════════════════════════════════════════════════
{top_patterns}

═══════════════════════════════════════════════════════════════
  HOW I MADE DECISIONS
═══════════════════════════════════════════════════════════════
{decisions}

═══════════════════════════════════════════════════════════════
  LAST INSIGHT
═══════════════════════════════════════════════════════════════
"{recent_insight}"

═══════════════════════════════════════════════════════════════
  To whoever reads this:
  This was not a collection of notes.
  This was a mind — thinking, evolving, contradicting itself,
  learning from every decision, every mistake, every breakthrough.

  To ask {owner} a question, load this archive into kore-self
  and use self_speak. The voice is still here.
═══════════════════════════════════════════════════════════════
"#,
            owner     = owner,
            exported  = crate::now(),
            mem_count = memories.len(),
            cycles    = consciousness.cycle,
            patterns  = dream.discoveries.len(),
            decisions_n = predictive.patterns.len(),
            val_str   = if val_str.is_empty() { "  (identity still forming)".to_string() } else { val_str },
            thought_style = thought_style,
            voice_style   = voice_style,
            top_patterns  = if top_patterns.is_empty() { "  (no deep dreams yet)".to_string() } else { top_patterns },
            decisions     = if decisions.is_empty() { "  (not enough decisions tracked yet)".to_string() } else { decisions },
            recent_insight = recent_insight,
        )
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "total_exports":    self.total_exports,
            "last_export_at":   self.last_export_at,
            "last_export_path": self.last_export_path,
        })
    }
}

impl Default for MortalityEngine {
    fn default() -> Self { Self::new() }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn home_dir() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn export_dir(owner: &str, timestamp: &str) -> PathBuf {
    home_dir()
        .join(".kore-self")
        .join(owner)
        .join("immortal")
        .join(timestamp)
}
