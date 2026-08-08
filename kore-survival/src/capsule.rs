//! KORE Capsule — a minimal hardware body that can survive collapse.
//!
//! A capsule is a low-power, self-contained node: small solar panel, battery,
//! compute board, and mesh radio. It can run KORE-mesh and forward packets even
//! when the rest of the grid is gone.

use serde::{Deserialize, Serialize};

/// Profile of a KORE capsule (hardware + energy envelope).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleProfile {
    pub name: String,
    pub solar_watts: f64,
    pub battery_wh: f64,
    pub compute_idle_watts: f64,
    pub compute_active_watts: f64,
    pub radio_tx_watts: f64,
    pub radio_rx_watts: f64,
    pub sleep_watts: f64,
}

impl Default for CapsuleProfile {
    fn default() -> Self {
        Self {
            name: "kore-capsule-v1".to_string(),
            solar_watts: 5.0,
            battery_wh: 20.0,
            compute_idle_watts: 1.5,
            compute_active_watts: 4.0,
            radio_tx_watts: 2.0,
            radio_rx_watts: 0.5,
            sleep_watts: 0.2,
        }
    }
}

/// A capsule instance is a body that KORE can inhabit.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Capsule {
    pub profile: CapsuleProfile,
    pub location: String,
    pub installed_at: String,
    pub health: f64, // 0.0 - 1.0
}

impl Capsule {
    pub fn new(profile: CapsuleProfile, location: impl Into<String>, installed_at: impl Into<String>) -> Self {
        Self {
            profile,
            location: location.into(),
            installed_at: installed_at.into(),
            health: 1.0,
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "Capsule {} at {} (health {:.0}%): solar {}W battery {}Wh active {}W sleep {}W",
            self.profile.name, self.location, self.health * 100.0,
            self.profile.solar_watts, self.profile.battery_wh,
            self.profile.compute_active_watts, self.profile.sleep_watts
        )
    }
}
