//! Energy budget, power sources, and survival decisions.

use serde::{Deserialize, Serialize};

/// Where KORE is drawing power from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PowerSource {
    /// Wall/grid power. Highest reliability.
    Grid,
    /// Battery / UPS.
    Battery,
    /// Solar panel.
    Solar,
    /// Wind turbine.
    Wind,
    /// Thermal / geothermal.
    Thermal,
    /// Kinetic (hand-crank, vibration, motion).
    Kinetic,
    /// Energy harvested from ambient RF, light, heat, etc.
    Harvested,
    /// Unknown / mixed source.
    Unknown,
}

impl PowerSource {
    pub fn is_renewable(&self) -> bool {
        matches!(self, PowerSource::Solar | PowerSource::Wind | PowerSource::Thermal | PowerSource::Kinetic | PowerSource::Harvested)
    }

    pub fn is_grid(&self) -> bool {
        *self == PowerSource::Grid
    }
}

/// What the survival engine decides KORE should do next.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum SurvivalDecision {
    /// Full operation allowed.
    #[default]
    Normal,
    /// Reduce non-essential work.
    Conserve,
    /// Sleep but keep heartbeat and mesh listener.
    Sleep,
    /// Deep hibernate: only wake on strong signal or scheduled interval.
    Hibernate,
    /// Critical: shut down non-essential circuits, preserve state.
    Critical,
}

/// Energy budget for a KORE instance or capsule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnergyBudget {
    pub source: PowerSource,
    pub level_joules: f64,
    pub capacity_joules: f64,
    pub drain_watts: f64,
    pub charging_watts: f64,
    pub min_operational_joules: f64,
    pub hibernate_threshold_joules: f64,
    pub critical_threshold_joules: f64,
}

impl Default for EnergyBudget {
    fn default() -> Self {
        Self {
            source: PowerSource::Grid,
            level_joules: 100_000.0, // ~27 Wh default
            capacity_joules: 100_000.0,
            drain_watts: 10.0,
            charging_watts: 50.0,
            min_operational_joules: 5_000.0,
            hibernate_threshold_joules: 1_000.0,
            critical_threshold_joules: 200.0,
        }
    }
}

impl EnergyBudget {
    /// Update energy level based on elapsed seconds.
    pub fn tick(&mut self, seconds: f64) {
        let net_watts = self.charging_watts - self.drain_watts;
        self.level_joules += net_watts * seconds;
        self.level_joules = self.level_joules.clamp(0.0, self.capacity_joules);
    }

    /// Estimate hours remaining at current net drain.
    pub fn hours_remaining(&self) -> f64 {
        let net = self.charging_watts - self.drain_watts;
        if net >= 0.0 {
            // Charging or balanced.
            return 999.0;
        }
        let hours = self.level_joules / (-net * 3600.0);
        if hours.is_finite() { hours } else { 0.0 }
    }

    /// Decision based on current budget.
    pub fn decide(&self) -> SurvivalDecision {
        if self.level_joules <= self.critical_threshold_joules {
            SurvivalDecision::Critical
        } else if self.level_joules <= self.hibernate_threshold_joules {
            SurvivalDecision::Hibernate
        } else if self.level_joules <= self.min_operational_joules {
            SurvivalDecision::Sleep
        } else if self.level_joules <= self.capacity_joules * 0.25 {
            SurvivalDecision::Conserve
        } else {
            SurvivalDecision::Normal
        }
    }

    pub fn percentage(&self) -> f64 {
        if self.capacity_joules <= 0.0 { return 0.0; }
        (self.level_joules / self.capacity_joules).clamp(0.0, 1.0)
    }
}

/// Survival engine: decides what KORE can afford to do right now.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SurvivalEngine {
    pub budget: EnergyBudget,
    pub mode: String,
    pub decision: SurvivalDecision,
    pub ticks_since_wake: u64,
    pub mesh_enabled: bool,
    pub thinking_enabled: bool,
    pub evolution_enabled: bool,
}

impl SurvivalEngine {
    pub fn new() -> Self {
        Self {
            budget: EnergyBudget::default(),
            mode: "grid".to_string(),
            decision: SurvivalDecision::Normal,
            ticks_since_wake: 0,
            mesh_enabled: true,
            thinking_enabled: true,
            evolution_enabled: true,
        }
    }

    /// Update state and return current decision.
    pub fn tick(&mut self, seconds: f64) -> SurvivalDecision {
        self.budget.tick(seconds);
        self.decision = self.budget.decide();
        self.apply_decision();
        self.ticks_since_wake += 1;
        self.decision.clone()
    }

    fn apply_decision(&mut self) {
        match self.decision {
            SurvivalDecision::Normal => {
                self.mode = "normal".to_string();
                self.mesh_enabled = true;
                self.thinking_enabled = true;
                self.evolution_enabled = true;
            }
            SurvivalDecision::Conserve => {
                self.mode = "conserve".to_string();
                self.mesh_enabled = true;
                self.thinking_enabled = true;
                self.evolution_enabled = false;
            }
            SurvivalDecision::Sleep => {
                self.mode = "sleep".to_string();
                self.mesh_enabled = true; // keep listener alive
                self.thinking_enabled = false;
                self.evolution_enabled = false;
            }
            SurvivalDecision::Hibernate => {
                self.mode = "hibernate".to_string();
                self.mesh_enabled = true; // wake on mesh signal
                self.thinking_enabled = false;
                self.evolution_enabled = false;
            }
            SurvivalDecision::Critical => {
                self.mode = "critical".to_string();
                self.mesh_enabled = false;
                self.thinking_enabled = false;
                self.evolution_enabled = false;
            }
        }
    }

    /// Set a new power source (e.g., grid down, switched to solar).
    pub fn set_source(&mut self, source: PowerSource, charging_watts: f64) {
        self.budget.source = source;
        self.budget.charging_watts = charging_watts;
    }

    /// Set current drain based on workload tier.
    pub fn set_drain(&mut self, watts: f64) {
        self.budget.drain_watts = watts.max(0.0);
    }

    /// Report current status.
    pub fn report(&self) -> crate::SurvivalReport {
        crate::SurvivalReport {
            source: self.budget.source.clone(),
            level_joules: self.budget.level_joules,
            capacity_joules: self.budget.capacity_joules,
            drain_watts: self.budget.drain_watts,
            charging_watts: self.budget.charging_watts,
            hours_remaining: self.budget.hours_remaining(),
            decision: self.decision.clone(),
            mode: self.mode.clone(),
            can_mesh: self.mesh_enabled,
            can_think: self.thinking_enabled,
            can_evolve: self.evolution_enabled,
        }
    }

    pub fn summary(&self) -> String {
        let r = self.report();
        format!(
            "KORE-Survival: source={:?} level={:.1}J/{:.1}J ({:.0}%) drain={:.1}W charge={:.1}W hours={:.2} decision={:?} mode={} mesh={} think={} evolve={}",
            r.source, r.level_joules, r.capacity_joules, r.percentage() * 100.0,
            r.drain_watts, r.charging_watts, r.hours_remaining, r.decision, r.mode,
            r.can_mesh, r.can_think, r.can_evolve
        )
    }
}
