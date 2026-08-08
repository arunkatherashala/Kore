//! KORE Internet — overlay network for connecting KORE instances and devices.
//!
//! Layers (bottom to top):
//! - **Transports:** TCP, UDP, file-drop, radio/light/sound stubs
//! - **LAN beacons:** UDP broadcast so phones, PCs, and capsules on the same Wi‑Fi find each other
//! - **Bootstrap + rendezvous:** wide-area peer lists and NAT hole punch
//! - **Relay nodes:** optional `KORE_MESH_RELAY=1` hosts that forward when direct paths fail
//! - **Names:** `kore://node-id` resolves via the local mesh peer table

use kore_federation::{FederationMessage, PeerInfo};

use crate::message::Envelope;
use crate::node::MeshNode;
use crate::transport::TransportError;

/// Configuration for the KORE Internet overlay on this device.
#[derive(Debug, Clone)]
pub struct KoreInternet {
    /// Broadcast LAN beacons on the local network.
    pub lan_discovery: bool,
    /// UDP broadcast target, e.g. `255.255.255.255:8980`.
    pub lan_broadcast: String,
    /// Public/LAN IP to advertise for TCP (optional). Peers still learn UDP return paths from packets.
    pub advertise_host: Option<String>,
    /// Device class: pc, phone, capsule, bootstrap, iot, ...
    pub device_kind: String,
    /// This node will forward RelayFrame messages for others.
    pub relay_enabled: bool,
}

impl Default for KoreInternet {
    fn default() -> Self {
        Self::from_env()
    }
}

impl KoreInternet {
    pub fn from_env() -> Self {
        let lan_discovery = std::env::var("KORE_INTERNET_LAN")
            .map(|v| v != "0" && v.to_lowercase() != "false")
            .unwrap_or(true);
        let port = std::env::var("KORE_MESH_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8980);
        let lan_broadcast = std::env::var("KORE_INTERNET_LAN_BROADCAST")
            .unwrap_or_else(|_| format!("255.255.255.255:{}", port));
        let advertise_host = std::env::var("KORE_MESH_ADVERTISE_HOST")
            .ok()
            .filter(|s| !s.is_empty());
        let device_kind =
            std::env::var("KORE_DEVICE_KIND").unwrap_or_else(|_| "pc".to_string());
        let relay_enabled = std::env::var("KORE_MESH_RELAY")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        Self {
            lan_discovery,
            lan_broadcast,
            advertise_host,
            device_kind,
            relay_enabled,
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "KORE Internet: lan={} broadcast={} advertise={} device={} relay={}",
            self.lan_discovery,
            self.lan_broadcast,
            self.advertise_host.as_deref().unwrap_or("(auto)"),
            self.device_kind,
            self.relay_enabled,
        )
    }
}

/// Parse `kore://node-id` into a node id string.
pub fn parse_kore_uri(uri: &str) -> Option<String> {
    uri.strip_prefix("kore://").map(|s| s.trim().to_string())
}

/// Resolve a KORE URI to a dialable `host:port` using the mesh peer table.
pub fn resolve_kore_uri(node: &MeshNode, uri: &str) -> Option<String> {
    let id = parse_kore_uri(uri)?;
    node.peers.get(&id).and_then(|p| p.addresses.first().cloned())
}

/// Build a TCP endpoint string for beacons (best effort).
pub fn advertised_tcp_endpoint(internet: &KoreInternet, mesh_port: u16) -> String {
    if let Some(host) = &internet.advertise_host {
        return format!("{}:{}", host, mesh_port);
    }
    format!("0.0.0.0:{}", mesh_port)
}

/// UDP broadcast a device beacon so other KORE devices on the LAN can connect.
pub async fn broadcast_lan_beacon(
    node: &mut MeshNode,
    internet: &KoreInternet,
    mesh_port: u16,
    federation_port: u16,
) -> Result<(), TransportError> {
    if !internet.lan_discovery {
        return Ok(());
    }
    let msg = FederationMessage::DeviceBeacon {
        node_id: node.identity.node_id.clone(),
        owner: node.identity.owner.clone(),
        mesh_port,
        federation_port,
        device_kind: internet.device_kind.clone(),
        capabilities: vec![
            "mesh".to_string(),
            "federation".to_string(),
            internet.device_kind.clone(),
        ],
    };
    let payload = serde_json::to_string(&msg).map_err(|e| {
        TransportError::Unsupported(format!("serialize beacon: {}", e))
    })?;
    let origin = node.identity.node_id.clone();
    let envelope = Envelope::new(origin, None, payload);
    node.send_envelope_to_address(&internet.lan_broadcast, &envelope)
        .await
}

/// Merge a device beacon; `sender_addr` is the UDP/TCP address the beacon arrived from.
pub fn merge_device_beacon(
    node: &mut MeshNode,
    sender_addr: &str,
    node_id: &str,
    owner: &str,
    mesh_port: u16,
    device_kind: &str,
    capabilities: Vec<String>,
) -> bool {
    if node_id == node.identity.node_id {
        return false;
    }
    let dial = tcp_endpoint_from_sender(sender_addr, mesh_port);
    let id = node_id.to_string();
    let is_new = !node.peers.contains_key(&id);
    node.peers
        .entry(id.clone())
        .and_modify(|p| {
            p.owner = owner.to_string();
            p.last_seen = crate::node::unix_now();
            if !p.addresses.contains(&dial) {
                p.addresses.push(dial.clone());
            }
            if !p.transports.contains(&"lan".to_string()) {
                p.transports.push("lan".to_string());
            }
            p.capabilities = capabilities.clone();
        })
        .or_insert_with(|| crate::MeshPeer {
            node_id: id,
            owner: owner.to_string(),
            addresses: vec![dial],
            transports: vec!["lan".to_string(), "udp".to_string()],
            last_seen: crate::node::unix_now(),
            capabilities,
            ..Default::default()
        });
    is_new
}

fn tcp_endpoint_from_sender(sender_addr: &str, mesh_port: u16) -> String {
    if let Some(host) = sender_addr.rsplit_once(':') {
        format!("{}:{}", host.0, mesh_port)
    } else {
        format!("{}:{}", sender_addr, mesh_port)
    }
}

/// Convert beacon + federation ports into peer infos for persistence.
pub fn beacon_to_peer_info(
    node_id: String,
    owner: String,
    sender_addr: &str,
    mesh_port: u16,
    capabilities: Vec<String>,
) -> PeerInfo {
    PeerInfo {
        node_id,
        owner,
        address: Some(tcp_endpoint_from_sender(sender_addr, mesh_port)),
        capabilities,
    }
}

/// Whether this node should forward a relay frame from `origin`.
pub fn should_relay(internet: &KoreInternet, node: &MeshNode, origin: &str) -> bool {
    if internet.relay_enabled {
        return true;
    }
    node.peers
        .get(origin)
        .map(|p| p.trusted)
        .unwrap_or(false)
}
