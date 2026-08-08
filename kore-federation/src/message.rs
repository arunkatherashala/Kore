//! KORE-Federation wire messages and knowledge packets.

use serde::{Deserialize, Serialize};

/// A memory fragment that can safely cross node boundaries.
/// This is intentionally a subset of `kore-self::Memory` so the federation crate
/// does not need to depend on `kore-self`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedMemory {
    pub kind: String,
    pub content: String,
    pub tags: Vec<String>,
    pub importance: f64,
}

/// A signed bundle of knowledge offered from one node to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgePacket {
    pub id: String,
    pub sender_id: String,
    pub sender_owner: String,
    pub timestamp: String,
    pub memories: Vec<SharedMemory>,
    pub signature: Vec<u8>,
    /// Optional human-readable reason for sharing.
    pub reason: String,
}

/// Messages exchanged between KORE nodes over a federation channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FederationMessage {
    /// Introduce yourself to a peer.
    Hello {
        node_id: String,
        owner: String,
        capabilities: Vec<String>,
    },
    /// Share a knowledge packet.
    Share { packet: KnowledgePacket },
    /// Request a list of known peers.
    DiscoverPeers,
    /// Respond with a list of known peers.
    PeerList { peers: Vec<PeerInfo> },
    /// Keep-alive.
    Ping { nonce: u64 },
    Pong { nonce: u64 },
    /// Disconnect politely.
    Goodbye { reason: String },
    /// Delivery acknowledgment for a mesh envelope id (reliable mesh).
    MeshAck { envelope_id: String },
    /// Register a UDP endpoint with bootstrap peers for NAT rendezvous.
    Rendezvous {
        node_id: String,
        udp_endpoint: String,
        /// When set, bootstrap should relay introduction to this node only.
        target_node_id: Option<String>,
    },
    /// Coordinated NAT hole punch: both sides send UDP to the listed endpoints.
    HolePunch {
        session_id: String,
        initiator_id: String,
        initiator_udp: String,
        target_id: String,
        target_udp: String,
    },
    /// LAN / local-network announcement so nearby devices find this KORE without bootstrap.
    DeviceBeacon {
        node_id: String,
        owner: String,
        mesh_port: u16,
        federation_port: u16,
        device_kind: String,
        capabilities: Vec<String>,
    },
    /// Ask a relay/bootstrap node to forward an inner federation payload to `destination`.
    RelayFrame {
        destination: String,
        payload: String,
    },
}

/// Lightweight peer descriptor sent in peer lists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub node_id: String,
    pub owner: String,
    pub address: Option<String>,
    pub capabilities: Vec<String>,
}

impl KnowledgePacket {
    /// Compute a canonical byte payload for signing/verification.
    pub fn payload_for_sign(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.id.as_bytes());
        bytes.extend_from_slice(self.sender_id.as_bytes());
        bytes.extend_from_slice(self.timestamp.as_bytes());
        for m in &self.memories {
            bytes.extend_from_slice(m.kind.as_bytes());
            bytes.extend_from_slice(m.content.as_bytes());
        }
        bytes
    }
}
