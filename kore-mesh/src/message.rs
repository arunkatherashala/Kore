//! KORE-Mesh message envelope.
//!
//! A mesh message wraps a `FederationMessage` with routing metadata so it can
//! travel across multiple transports and be deduplicated by every node.

use serde::{Deserialize, Serialize};

/// Routing metadata attached to every mesh message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Globally unique message id (for deduplication).
    pub id: String,
    /// Node that originally created this message.
    pub origin: String,
    /// Destination: `None` means broadcast to all reachable nodes.
    pub destination: Option<String>,
    /// Unix timestamp in seconds.
    pub timestamp: u64,
    /// Number of hops this message has taken.
    pub hops: u8,
    /// Maximum hops allowed before the message is dropped.
    pub ttl: u8,
    /// Serialized federation payload.
    pub payload: String,
    /// When true, the recipient should reply with `FederationMessage::MeshAck`.
    #[serde(default)]
    pub ack_requested: bool,
}

impl Envelope {
    pub fn new(origin: String, destination: Option<String>, payload: String) -> Self {
        let id = format!(
            "{}-{}-{:x}",
            origin,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            rand::random::<u64>()
        );
        Self {
            id,
            origin,
            destination,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            hops: 0,
            ttl: 16,
            payload,
            ack_requested: false,
        }
    }

    pub fn new_reliable(origin: String, destination: Option<String>, payload: String) -> Self {
        let mut env = Self::new(origin, destination, payload);
        env.ack_requested = true;
        env
    }

    pub fn payload_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    /// Increment hop count. Returns false if TTL exceeded.
    pub fn forward(&mut self) -> bool {
        self.hops += 1;
        self.hops < self.ttl
    }
}

/// High-level mesh command. This is the API a KORE node uses to talk to the mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeshCommand {
    Broadcast { payload: String },
    SendTo { destination: String, payload: String },
    Discover,
    /// Unicast to one node; retries until ack or max attempts.
    SendReliable { destination: String, payload: String },
}
