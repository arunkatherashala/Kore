//! KORE-Survival integration for kore-self.
//!
//! Tracks energy budget and decides whether KORE can afford to think, mesh, or
//! evolve. In low-power situations it reduces workload automatically.

use std::sync::{Arc, Mutex};
use tokio::time::Duration;

use kore_survival::{PowerSource, SurvivalDecision, SurvivalReport};

use crate::KoreSelf;

pub const SURVIVAL_TICK_SECS: f64 = 60.0;

/// Start the survival monitoring loop. This periodically updates the energy
/// budget and updates KoreSelf's internal flags so the heartbeat can respect
/// power constraints.
pub async fn survival_monitor(shared_me: Arc<Mutex<KoreSelf>>) {
    let mut interval = tokio::time::interval(Duration::from_secs_f64(SURVIVAL_TICK_SECS));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        let mut me = shared_me.lock().unwrap();
        me.survival.tick(SURVIVAL_TICK_SECS);
        let report = me.survival.report();
        if !report.ok() {
            eprintln!("[kore-survival] {}", me.survival.summary());
        }
    }
}

/// Read the current survival report without mutation.
pub fn survival_report(me: &KoreSelf) -> SurvivalReport {
    me.survival.report()
}

/// Configure power source and drain from a tool call.
pub fn configure(me: &mut KoreSelf, source: PowerSource, charging_watts: f64, drain_watts: f64) -> SurvivalReport {
    me.survival.set_source(source, charging_watts);
    me.survival.set_drain(drain_watts);
    me.survival.report()
}

/// Accept-loop sleep duration based on survival mode.
pub fn mesh_tick_interval_ms(decision: &SurvivalDecision) -> u64 {
    match decision {
        SurvivalDecision::Normal | SurvivalDecision::Conserve => 100,
        SurvivalDecision::Sleep => 250,
        SurvivalDecision::Hibernate => 500,
        SurvivalDecision::Critical => 1000,
    }
}

/// Outbound discover / rendezvous interval based on survival mode.
pub fn mesh_discover_interval_secs(decision: &SurvivalDecision) -> u64 {
    match decision {
        SurvivalDecision::Normal => 30,
        SurvivalDecision::Conserve => 60,
        SurvivalDecision::Sleep => 120,
        SurvivalDecision::Hibernate => 300,
        SurvivalDecision::Critical => 600,
    }
}

/// Whether this node should originate mesh traffic (always listen when mesh is up).
pub fn mesh_should_transmit(decision: &SurvivalDecision, mesh_enabled: bool) -> bool {
    mesh_enabled && !matches!(decision, SurvivalDecision::Critical)
}

/// Whether periodic discovery / rendezvous should run.
pub fn mesh_should_discover(decision: &SurvivalDecision, mesh_enabled: bool) -> bool {
    if !mesh_enabled {
        return false;
    }
    !matches!(decision, SurvivalDecision::Critical)
}
