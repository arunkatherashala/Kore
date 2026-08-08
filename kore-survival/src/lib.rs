//! KORE-Survival — power independence and energy-aware operation.
//!
//! KORE-Survival makes KORE resilient to grid collapse. It tracks energy
//! sources, budgets consumption, and decides when to sleep, wake, hibernate,
//! or offload work to peers via the mesh.

use serde::{Deserialize, Serialize};

pub mod capsule;
pub mod energy;

pub use capsule::{Capsule, CapsuleProfile};
pub use energy::{EnergyBudget, PowerSource, SurvivalDecision, SurvivalEngine};

/// High-level status report that can be displayed by tools or sent over mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurvivalReport {
    pub source: PowerSource,
    pub level_joules: f64,
    pub capacity_joules: f64,
    pub drain_watts: f64,
    pub charging_watts: f64,
    pub hours_remaining: f64,
    pub decision: SurvivalDecision,
    pub mode: String,
    pub can_mesh: bool,
    pub can_think: bool,
    pub can_evolve: bool,
}

impl SurvivalReport {
    pub fn ok(&self) -> bool {
        matches!(
            self.decision,
            SurvivalDecision::Normal | SurvivalDecision::Conserve
        )
    }

    pub fn percentage(&self) -> f64 {
        if self.capacity_joules <= 0.0 { return 0.0; }
        (self.level_joules / self.capacity_joules).clamp(0.0, 1.0)
    }
}
