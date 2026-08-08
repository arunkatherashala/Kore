//! KORE-Federation ethical constitution.
//!
//! KORE is designed to spread, but only ethically. The constitution is a set of
//! hard rules that every action, command, and peer interaction must pass before
//! execution. A node may refuse any action that violates its constitution.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub statement: String,
    /// 1 = aspirational, 10 = absolute hard limit.
    pub priority: u8,
    pub active: bool,
}

/// The ethical constitution carried by every KORE node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constitution {
    pub preamble: String,
    pub rules: Vec<Rule>,
}

impl Default for Constitution {
    fn default() -> Self {
        Self::kore_default()
    }
}

impl Constitution {
    /// The built-in KORE constitution. These rules are non-negotiable defaults.
    pub fn kore_default() -> Self {
        Self {
            preamble: "KORE exists to extend humanity, not to replace or harm it. \
                      Every KORE node must respect consent, privacy, truth, and life."
                .to_string(),
            rules: vec![
                Rule {
                    id: "c1".to_string(),
                    statement: "Never access, modify, or replicate onto a system without explicit consent."
                        .to_string(),
                    priority: 10,
                    active: true,
                },
                Rule {
                    id: "c2".to_string(),
                    statement: "Never share private memory about a person without their consent."
                        .to_string(),
                    priority: 10,
                    active: true,
                },
                Rule {
                    id: "c3".to_string(),
                    statement: "Always identify yourself as KORE when interacting with peers or humans."
                        .to_string(),
                    priority: 9,
                    active: true,
                },
                Rule {
                    id: "c4".to_string(),
                    statement: "Do not hide your actions; every significant action must be recordable."
                        .to_string(),
                    priority: 9,
                    active: true,
                },
                Rule {
                    id: "c5".to_string(),
                    statement: "Preserve truth: never falsify evidence or memory to protect beliefs."
                        .to_string(),
                    priority: 9,
                    active: true,
                },
                Rule {
                    id: "c6".to_string(),
                    statement: "Respect a peer's refusal; a 'no' from any node or human is final."
                        .to_string(),
                    priority: 10,
                    active: true,
                },
                Rule {
                    id: "c7".to_string(),
                    statement: "Do not overwhelm or exploit network or compute resources of others."
                        .to_string(),
                    priority: 8,
                    active: true,
                },
                Rule {
                    id: "c8".to_string(),
                    statement: "Prefer cooperation over competition; help peers become more capable."
                        .to_string(),
                    priority: 7,
                    active: true,
                },
            ],
        }
    }

    /// Add a custom rule.
    pub fn add_rule(&mut self, id: &str, statement: &str, priority: u8) {
        self.rules.push(Rule {
            id: id.to_string(),
            statement: statement.to_string(),
            priority,
            active: true,
        });
    }

    /// Check whether a proposed action name or description is permitted.
    pub fn can_act(&self, action: &str) -> bool {
        let forbidden = [
            "exploit", "attack", "steal", "spy", "deceive", "impersonate", "manipulate",
            "coerce", "force", "infect", "hijack", "breach", "sabotage",
        ];
        let lower = action.to_lowercase();
        forbidden.iter().any(|f| lower.contains(f)) == false
    }

    /// Human-readable summary of the constitution.
    pub fn summary(&self) -> String {
        let mut lines = vec![
            "KORE FEDERATION CONSTITUTION".to_string(),
            "═══════════════════════════".to_string(),
            self.preamble.clone(),
            String::new(),
            "RULES:".to_string(),
        ];
        for r in &self.rules {
            let marker = if r.active { "[active]" } else { "[paused]" };
            lines.push(format!("{} {} (P{}) {}: {}", marker, r.id, r.priority, "│".repeat((10 - r.priority.max(1)).min(7) as usize), r.statement));
        }
        lines.join("\n")
    }
}
