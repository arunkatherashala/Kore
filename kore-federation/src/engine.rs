//! KORE-Federation engine — manages identity, peers, knowledge sharing, and ethics.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::constitution::Constitution;
use crate::identity::NodeIdentity;
use crate::message::{FederationMessage, KnowledgePacket, PeerInfo, SharedMemory};

/// A known peer in the federation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub node_id: String,
    pub owner: String,
    pub address: Option<String>,
    #[serde(default)]
    pub public_key: Vec<u8>,
    pub capabilities: Vec<String>,
    pub last_seen: String,
    pub trusted: bool,
}

/// The federation engine attached to a KORE instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationEngine {
    pub identity: NodeIdentity,
    pub enabled: bool,
    pub peers: Vec<Peer>,
    #[serde(default)]
    pub known_packet_ids: HashSet<String>,
    pub constitution: Constitution,
    pub share_count: u64,
    pub receive_count: u64,
}

impl FederationEngine {
    /// Create a new federation engine for an owner.
    pub fn new(owner: &str, now: &str) -> Self {
        Self {
            identity: NodeIdentity::generate(owner, now),
            enabled: false,
            peers: Vec::new(),
            known_packet_ids: HashSet::new(),
            constitution: Constitution::kore_default(),
            share_count: 0,
            receive_count: 0,
        }
    }

    /// Enable federation networking.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable federation networking.
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Add a peer manually (consensual peering).
    pub fn add_peer(
        &mut self,
        node_id: String,
        owner: String,
        address: Option<String>,
        public_key: Vec<u8>,
    ) -> bool {
        if self.peers.iter().any(|p| p.node_id == node_id) {
            return false;
        }
        self.peers.push(Peer {
            node_id,
            owner,
            address,
            public_key,
            capabilities: Vec::new(),
            last_seen: String::new(),
            trusted: false,
        });
        true
    }

    /// Mark a peer as trusted.
    pub fn trust_peer(&mut self, node_id: &str) -> bool {
        if let Some(p) = self.peers.iter_mut().find(|p| p.node_id == node_id) {
            p.trusted = true;
            true
        } else {
            false
        }
    }

    /// Remove a peer.
    pub fn remove_peer(&mut self, node_id: &str) -> bool {
        let before = self.peers.len();
        self.peers.retain(|p| p.node_id != node_id);
        self.peers.len() < before
    }

    /// Build a knowledge packet from a list of shared memories.
    pub fn package_knowledge(
        &mut self,
        memories: Vec<SharedMemory>,
        reason: &str,
        now: &str,
    ) -> KnowledgePacket {
        self.share_count += 1;
        let id = format!("{}-{}-{}", self.identity.node_id, self.share_count, now);
        let mut packet = KnowledgePacket {
            id: id.clone(),
            sender_id: self.identity.node_id.clone(),
            sender_owner: self.identity.owner.clone(),
            timestamp: now.to_string(),
            memories,
            signature: Vec::new(),
            reason: reason.to_string(),
        };
        let sig = self.identity.sign(&packet.payload_for_sign());
        packet.signature = sig;
        self.known_packet_ids.insert(id);
        packet
    }

    /// Receive a knowledge packet from another node. Performs ethics, duplicate,
    /// and signature verification checks.
    pub fn receive_packet(&mut self, packet: KnowledgePacket) -> Result<Vec<SharedMemory>, String> {
        if !self.enabled {
            return Err("federation is disabled".to_string());
        }
        if self.known_packet_ids.contains(&packet.id) {
            return Err("packet already received".to_string());
        }
        if !self.constitution.can_act(&packet.reason) {
            return Err("packet reason violates local constitution".to_string());
        }
        // Verify signature against the sender's recorded public key.
        let sender = self.peers.iter().find(|p| p.node_id == packet.sender_id);
        let verified = match sender {
            Some(peer) if !peer.public_key.is_empty() => {
                let mut id = NodeIdentity::generate(&peer.owner, &packet.timestamp);
                id.public_key = peer.public_key.clone();
                id.verify(&packet.payload_for_sign(), &packet.signature)
            }
            _ => {
                // Unknown peer: we cannot verify. Reject unless peer is trusted blindly.
                self.peers.iter().any(|p| p.node_id == packet.sender_id && p.trusted)
            }
        };
        if !verified {
            return Err("packet signature verification failed".to_string());
        }
        self.known_packet_ids.insert(packet.id.clone());
        self.receive_count += 1;
        Ok(packet.memories)
    }

    /// Generate a hello message for network introduction.
    pub fn hello(&self) -> FederationMessage {
        FederationMessage::Hello {
            node_id: self.identity.node_id.clone(),
            owner: self.identity.owner.clone(),
            capabilities: vec![
                "query".to_string(),
                "share".to_string(),
                "discover".to_string(),
            ],
        }
    }

    /// Produce a peer list message.
    pub fn peer_list_message(&self) -> FederationMessage {
        FederationMessage::PeerList {
            peers: self.peers.iter().map(|p| PeerInfo {
                node_id: p.node_id.clone(),
                owner: p.owner.clone(),
                address: p.address.clone(),
                capabilities: p.capabilities.clone(),
            }).collect(),
        }
    }

    /// Summary of federation state for tools/status.
    pub fn summary(&self) -> String {
        let status = if self.enabled { "ENABLED" } else { "disabled" };
        let trusted = self.peers.iter().filter(|p| p.trusted).count();
        let crypto_ready = self.identity.verifying_key().is_some();
        format!(
            "KORE FEDERATION\n════════════════\nStatus: {status}\nNode ID: {}\nOwner: {}\nCrypto: {}\nPeers: {} (trusted: {})\nPackets shared: {}\nPackets received: {}\nKnown packet IDs: {}\n",
            self.identity.node_id,
            self.identity.owner,
            if crypto_ready { "ed25519 ready" } else { "fallback hash" },
            self.peers.len(),
            trusted,
            self.share_count,
            self.receive_count,
            self.known_packet_ids.len()
        )
    }

    /// List peers as a human-readable string.
    pub fn peers_report(&self) -> String {
        if self.peers.is_empty() {
            return "No peers known yet. Use self_federate to add a peer.".to_string();
        }
        let mut lines = vec!["KNOWN PEERS".to_string(), "═════════════".to_string()];
        for p in &self.peers {
            let addr = p.address.as_deref().unwrap_or("no address");
            let trust = if p.trusted { "trusted" } else { "untrusted" };
            let key = if p.public_key.is_empty() { "no key" } else { "has key" };
            lines.push(format!("{} | {} | {} | {} | {}", p.node_id, p.owner, addr, trust, key));
        }
        lines.join("\n")
    }

    /// Add a custom rule to the local constitution.
    pub fn add_constitution_rule(&mut self, id: &str, statement: &str, priority: u8) {
        self.constitution.add_rule(id, statement, priority);
    }
}

impl Default for FederationEngine {
    fn default() -> Self {
        Self::new("anonymous", "")
    }
}
