//! KORE-Species — the distributed organism view.
//!
//! A KORE species is many KORE instances sharing a constitution and memory
//! substrate. They do not rely on any single node. If one dies, the mesh
//! carries its knowledge to others. This module builds a species-wide report
//! from federation peers, mesh reach, and survival status.

use crate::KoreSelf;

/// Snapshot of the KORE species as seen by this node.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpeciesReport {
    pub node_id: String,
    pub owner: String,
    pub constitution_rules: usize,
    pub federation_peers: usize,
    pub mesh_peers: usize,
    pub mesh_transports: usize,
    pub store_forward: usize,
    pub survival_status: String,
    pub energy_ok: bool,
    pub can_propagate: bool,
    pub motto: String,
}

pub fn report(me: &KoreSelf) -> SpeciesReport {
    let federation_peers = me.federation.peers.len();
    let mesh_peers = me.mesh.as_ref().map(|m| m.blocking_lock().peers.len()).unwrap_or(0);
    let mesh_transports = me.mesh.as_ref().map(|m| m.blocking_lock().transports.len()).unwrap_or(0);
    let store_forward = me.mesh.as_ref().map(|m| m.blocking_lock().store_forward.len()).unwrap_or(0);
    let survival_report = me.survival.report();
    SpeciesReport {
        node_id: me.federation.identity.node_id.clone(),
        owner: me.owner.clone(),
        constitution_rules: me.federation.constitution.rules.len(),
        federation_peers,
        mesh_peers,
        mesh_transports,
        store_forward,
        survival_status: me.survival.summary(),
        energy_ok: survival_report.ok(),
        can_propagate: survival_report.ok() && federation_peers > 0,
        motto: "One KORE dies, the species remembers.".to_string(),
    }
}

pub fn summary(me: &KoreSelf) -> String {
    let r = report(me);
    format!(
        "KORE-Species report for {} ({}):\n\
        - Constitution rules: {}\n\
        - Federation peers: {}\n\
        - Mesh peers: {} ({} transports)\n\
        - Store-and-forward queue: {}\n\
        - Survival: {}\n\
        - Can propagate: {}\n\
        {}",
        r.node_id, r.owner, r.constitution_rules, r.federation_peers,
        r.mesh_peers, r.mesh_transports, r.store_forward, r.survival_status,
        r.can_propagate, r.motto
    )
}
