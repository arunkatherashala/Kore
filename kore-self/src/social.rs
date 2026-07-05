// kore-self  —  Phase 4: Social Layer
//
// kore-self can speak AS you.
// Answer emails, respond to questions, write messages — all in YOUR voice.
// Uses Identity Model (voice profile) + Memory recall to generate responses.
//
// No external LLM. Pure pattern assembly from YOUR history.

use serde::{Deserialize, Serialize};
use crate::Memory;
use crate::identity::IdentityModel;

// ─── Voice Engine ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceEngine {
    pub total_spoken:    u64,
    pub style_cache:     Vec<StyleSample>,   // last 20 generated responses
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleSample {
    pub prompt:      String,
    pub response:    String,
    pub generated_at: String,
}

impl VoiceEngine {
    pub fn new() -> Self {
        Self {
            total_spoken: 0,
            style_cache:  vec![],
        }
    }

    /// Speak AS the user: given a prompt, generate a response in their voice.
    ///
    /// Algorithm:
    ///   1. Recall relevant memories (context)
    ///   2. Extract key facts & stances
    ///   3. Apply voice profile (directness, technical depth, certainty)
    ///   4. Assemble response
    ///
    /// Returns (response_text, context_used_count)
    pub fn speak_as(
        &mut self,
        prompt: &str,
        memories: &[Memory],
        identity: &IdentityModel,
    ) -> (String, usize) {
        // Step 1: Recall relevant memories
        let q     = prompt.to_lowercase();
        let words: Vec<&str> = q.split_whitespace().collect();
        let n     = memories.len();

        let mut scored: Vec<(f64, &Memory)> = memories.iter().enumerate()
            .filter_map(|(i, m)| {
                let c    = m.content.to_lowercase();
                let hits = words.iter().filter(|&&w| c.contains(w)).count() as f64;
                if hits == 0.0 { return None; }
                let recency = 1.0 / (1.0 + n.saturating_sub(i) as f64 * 0.05);
                Some((hits * m.importance * (1.0 + recency), m))
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let top: Vec<&Memory> = scored.into_iter().take(8).map(|(_, m)| m).collect();
        let context_count = top.len();

        // Step 2: Extract key stances from relevant memories
        let key_facts: Vec<String> = top.iter()
            .filter(|m| m.importance >= 0.8)
            .map(|m| summarize_memory(&m.content))
            .collect();

        // Step 3: Apply voice profile
        let voice   = &identity.voice;
        let think   = &identity.thinking;
        let values  = identity.top_values(3);

        // Build the response in layers
        let mut parts: Vec<String> = vec![];

        // Opening: direct vs. soft
        if voice.directness > 0.7 {
            parts.push(respond_direct(prompt, &key_facts, identity));
        } else {
            parts.push(respond_thoughtful(prompt, &key_facts, identity));
        }

        // Add technical depth if topic warrants it
        if voice.technical_depth > 0.65 && context_count >= 2 {
            let tech = add_technical_layer(&top, identity);
            if !tech.is_empty() {
                parts.push(tech);
            }
        }

        // Add values alignment if relevant
        let val_note = values_note(prompt, &values);
        if !val_note.is_empty() {
            parts.push(val_note);
        }

        // Closing confidence marker
        if voice.certainty > 0.75 && think.decision_speed > 0.7 {
            parts.push(closing_decisive());
        } else if voice.certainty < 0.4 {
            parts.push(closing_exploratory());
        }

        let response = parts.join(" ");
        self.total_spoken += 1;

        // Cache for style learning
        self.style_cache.push(StyleSample {
            prompt:       prompt.to_string(),
            response:     response.clone(),
            generated_at: crate::now(),
        });
        if self.style_cache.len() > 20 {
            self.style_cache.remove(0);
        }

        (response, context_count)
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "total_spoken":    self.total_spoken,
            "style_cache_size": self.style_cache.len(),
            "recent": self.style_cache.iter().rev().take(3).map(|s| serde_json::json!({
                "prompt":   &s.prompt[..s.prompt.len().min(80)],
                "response": &s.response[..s.response.len().min(120)],
                "when":     s.generated_at,
            })).collect::<Vec<_>>(),
        })
    }
}

impl Default for VoiceEngine {
    fn default() -> Self { Self::new() }
}

// ─── Response builders ────────────────────────────────────────────────────────

fn respond_direct(prompt: &str, facts: &[String], identity: &IdentityModel) -> String {
    let owner = &identity.owner;
    let top_values = identity.top_values(2);

    if facts.is_empty() {
        // No specific memory context — speak from values
        let val_str = if top_values.is_empty() {
            "my core principles".to_string()
        } else {
            format!("{} and {}",
                top_values[0].name,
                top_values.get(1).map(|v| v.name.as_str()).unwrap_or("precision")
            )
        };
        format!(
            "From {owner}'s perspective: this comes down to {val_str}. \
             The answer is straightforward once you frame it that way."
        )
    } else {
        format!(
            "Based on direct experience: {}. This isn't theoretical.",
            facts[0]
        )
    }
}

fn respond_thoughtful(prompt: &str, facts: &[String], identity: &IdentityModel) -> String {
    let _ = prompt; // context for future use
    if facts.is_empty() {
        format!("This depends on context, but generally I'd consider the trade-offs carefully. \
                 The nuance matters here — {}.",
            identity.top_values(1).first()
                .map(|v| format!("{} is the primary lens", v.name))
                .unwrap_or("first principles drive the decision".to_string())
        )
    } else {
        format!("Looking at this from multiple angles — {}. \
                 There's more depth to unpack.",
            facts.iter().take(2).cloned().collect::<Vec<_>>().join("; ")
        )
    }
}

fn add_technical_layer(memories: &[&Memory], identity: &IdentityModel) -> String {
    // Extract the most technical fact
    let tech_mem = memories.iter()
        .filter(|m| m.kind == "code" || m.kind == "benchmark" || m.kind == "decision")
        .max_by(|a, b| a.importance.partial_cmp(&b.importance).unwrap_or(std::cmp::Ordering::Equal));

    match tech_mem {
        Some(m) => {
            let snip = summarize_memory(&m.content);
            if identity.thinking.metrics_driven > 0.7 {
                format!("The data backs this: {}.", snip)
            } else {
                format!("Technically: {}.", snip)
            }
        }
        None => String::new(),
    }
}

fn values_note(prompt: &str, values: &[crate::identity::CoreValue]) -> String {
    let p = prompt.to_lowercase();
    for v in values {
        let vl = v.name.to_lowercase();
        if p.contains(&vl) || vl.contains("performance") && p.contains("speed")
            || vl.contains("reliability") && p.contains("stable")
        {
            return format!("This aligns with a core value: {} (strength {:.0}%).", v.name, v.strength * 100.0);
        }
    }
    String::new()
}

fn closing_decisive() -> String {
    "Decision: proceed.".to_string()
}

fn closing_exploratory() -> String {
    "Worth exploring further before committing.".to_string()
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Extract a concise summary from a memory's content (first 120 chars, trimmed at word boundary).
fn summarize_memory(content: &str) -> String {
    let max = 120;
    if content.len() <= max {
        return content.trim().to_string();
    }
    let trimmed = &content[..max];
    match trimmed.rfind(' ') {
        Some(pos) => format!("{}...", &trimmed[..pos]),
        None      => format!("{}...", trimmed),
    }
}
