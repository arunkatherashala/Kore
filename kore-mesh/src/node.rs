//! KORE-Mesh node.
//!
//! A `MeshNode` owns one or more transports, maintains a peer table, and routes
//! federation messages using gossip + store-and-forward. It is deliberately
//! transport-agnostic so the same node can speak TCP, UDP, radio, light, file
//! drops, or any future medium.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::Mutex;

use kore_federation::{FederationEngine, FederationMessage, NodeIdentity, PeerInfo};

use crate::message::{Envelope, MeshCommand};
use crate::transport::{SharedTransport, Transport, TransportError};

/// Outbound message waiting for a `MeshAck` from the destination.
#[derive(Debug, Clone)]
pub struct PendingDelivery {
    pub envelope: Envelope,
    pub attempts: u8,
    pub max_attempts: u8,
    pub next_retry_at: u64,
}

impl PendingDelivery {
    pub fn new(envelope: Envelope, max_attempts: u8) -> Self {
        Self {
            envelope,
            attempts: 1,
            max_attempts,
            next_retry_at: unix_now(),
        }
    }
}

/// A record of a peer we know about, independent of which transport it came in on.
#[derive(Debug, Clone, Default)]
pub struct MeshPeer {
    pub node_id: String,
    pub owner: String,
    pub addresses: Vec<String>,
    pub transports: Vec<String>,
    pub last_seen: u64,
    pub trusted: bool,
    pub capabilities: Vec<String>,
}

impl MeshPeer {
    pub fn from_peer_info(info: &PeerInfo, transport: &str) -> Self {
        let mut addresses = Vec::new();
        if let Some(addr) = &info.address {
            addresses.push(addr.clone());
        }
        Self {
            node_id: info.node_id.clone(),
            owner: info.owner.clone(),
            addresses,
            transports: vec![transport.to_string()],
            last_seen: unix_now(),
            trusted: false,
            capabilities: info.capabilities.clone(),
        }
    }
}

/// MeshNode is the runtime engine of KORE-Mesh.
pub struct MeshNode {
    pub identity: NodeIdentity,
    pub peers: HashMap<String, MeshPeer>,
    pub transports: Vec<SharedTransport>,
    /// Messages waiting for a destination to come online.
    pub store_forward: VecDeque<Envelope>,
    /// Message ids we have already processed.
    pub seen: HashSet<String>,
    /// Inbound messages waiting to be consumed by the application layer.
    pub inbound: VecDeque<Envelope>,
    /// Transport sender address aligned with `inbound` (same pop order).
    pub inbound_senders: VecDeque<String>,
    /// Optional federation engine used to verify/ingest payloads.
    pub federation: Option<FederationEngine>,
    /// Envelopes sent with reliability that have not been acked yet.
    pub pending_acks: HashMap<String, PendingDelivery>,
    pub stats: MeshStats,
}

#[derive(Debug, Clone, Default)]
pub struct MeshStats {
    pub received: u64,
    pub forwarded: u64,
    pub sent: u64,
    pub delivered: u64,
    pub dropped: u64,
    pub stored: u64,
    pub acked: u64,
    pub ack_pending: u64,
    pub retries: u64,
}

impl MeshNode {
    pub fn new(identity: NodeIdentity) -> Self {
        Self {
            identity,
            peers: HashMap::new(),
            transports: Vec::new(),
            store_forward: VecDeque::new(),
            seen: HashSet::new(),
            inbound: VecDeque::new(),
            inbound_senders: VecDeque::new(),
            federation: None,
            pending_acks: HashMap::new(),
            stats: MeshStats::default(),
        }
    }

    pub fn with_federation(mut self, federation: FederationEngine) -> Self {
        self.federation = Some(federation);
        self
    }

    pub fn add_transport(&mut self, transport: Box<dyn Transport>) {
        self.transports.push(Arc::new(Mutex::new(transport)));
    }

    pub fn address_book(&self) -> Vec<String> {
        self.transports
            .iter()
            .map(|t| {
                let t = t.blocking_lock();
                format!("{}://{}", t.kind(), t.local_address())
            })
            .collect()
    }

    /// Submit a command to the mesh. The command will be serialized into an envelope.
    pub async fn command(&mut self, cmd: MeshCommand) -> Result<String, TransportError> {
        let origin = self.identity.node_id.clone();
        let envelope = match cmd {
            MeshCommand::Broadcast { payload } => Envelope::new(origin, None, payload),
            MeshCommand::SendTo { destination, payload } => {
                Envelope::new(origin, Some(destination), payload)
            }
            MeshCommand::Discover => {
                let payload = serde_json::to_string(&FederationMessage::DiscoverPeers).map_err(
                    |e| TransportError::Unsupported(format!("serialize discover: {}", e)),
                )?;
                Envelope::new(origin, None, payload)
            }
            MeshCommand::SendReliable { destination, payload } => {
                Envelope::new_reliable(origin, Some(destination), payload)
            }
        };

        let id = envelope.id.clone();
        let track_ack = envelope.ack_requested;
        let destination = envelope.destination.clone();
        self.seen.insert(id.clone());

        if track_ack {
            if let Some(dest) = destination.as_ref() {
                self.track_pending_delivery(envelope.clone(), dest).await?;
            }
        } else if let Some(dest) = destination.as_ref() {
            self.send_to_destination(dest, &envelope).await;
        } else {
            self.broadcast_envelope(envelope).await;
        }
        Ok(id)
    }

    async fn send_to_destination(&mut self, dest: &str, envelope: &Envelope) {
        let addrs: Vec<String> = self
            .peers
            .get(dest)
            .map(|p| p.addresses.clone())
            .unwrap_or_default();
        let mut sent = false;
        for addr in addrs {
            if self.send_to_address(&addr, envelope).await.is_ok() {
                sent = true;
                break;
            }
        }
        if !sent {
            self.store_forward.push_back(envelope.clone());
            self.stats.stored += 1;
        }
        self.stats.sent += 1;
    }

    /// Send a discover (or any) envelope directly to one address without flooding peers.
    pub async fn send_envelope_to_address(
        &mut self,
        address: &str,
        envelope: &Envelope,
    ) -> Result<(), TransportError> {
        self.seen.insert(envelope.id.clone());
        self.send_to_address(address, envelope).await?;
        self.stats.sent += 1;
        Ok(())
    }

    /// Serialize a federation message and send it to one address (unicast).
    pub async fn send_federation_to_address(
        &mut self,
        address: &str,
        destination_node_id: Option<String>,
        message: &FederationMessage,
    ) -> Result<String, TransportError> {
        let payload = serde_json::to_string(message).map_err(|e| {
            TransportError::Unsupported(format!("serialize federation message: {}", e))
        })?;
        let origin = self.identity.node_id.clone();
        let envelope = Envelope::new(origin, destination_node_id, payload);
        let id = envelope.id.clone();
        self.send_envelope_to_address(address, &envelope).await?;
        Ok(id)
    }

    async fn track_pending_delivery(
        &mut self,
        envelope: Envelope,
        dest: &str,
    ) -> Result<(), TransportError> {
        let id = envelope.id.clone();
        let addrs: Vec<String> = self
            .peers
            .get(dest)
            .map(|p| p.addresses.clone())
            .unwrap_or_default();
        let mut sent = false;
        for addr in &addrs {
            if self.send_to_address(addr, &envelope).await.is_ok() {
                sent = true;
                break;
            }
        }
        if !sent {
            return Err(TransportError::UnknownPeer(dest.to_string()));
        }
        self.pending_acks
            .insert(id.clone(), PendingDelivery::new(envelope, 5));
        self.stats.sent += 1;
        self.stats.ack_pending = self.pending_acks.len() as u64;
        Ok(())
    }

    fn handle_mesh_ack(&mut self, envelope_id: &str) {
        if self.pending_acks.remove(envelope_id).is_some() {
            self.stats.acked += 1;
            self.stats.ack_pending = self.pending_acks.len() as u64;
        }
    }

    async fn send_delivery_ack(&mut self, envelope_id: &str, to_node: &str) {
        let payload = match serde_json::to_string(&FederationMessage::MeshAck {
            envelope_id: envelope_id.to_string(),
        }) {
            Ok(p) => p,
            Err(_) => return,
        };
        let origin = self.identity.node_id.clone();
        let ack = Envelope::new(origin, Some(to_node.to_string()), payload);
        let addrs: Vec<String> = self
            .peers
            .get(to_node)
            .map(|p| p.addresses.clone())
            .unwrap_or_default();
        for addr in addrs {
            if self.send_to_address(&addr, &ack).await.is_ok() {
                self.stats.sent += 1;
                break;
            }
        }
    }

    /// Retry reliable deliveries whose backoff window has elapsed.
    pub async fn retry_pending_acks(&mut self) {
        let now = unix_now();
        let mut due: Vec<PendingDelivery> = Vec::new();
        self.pending_acks.retain(|_, pending| {
            if pending.attempts >= pending.max_attempts {
                return false;
            }
            if pending.next_retry_at <= now {
                due.push(pending.clone());
                false
            } else {
                true
            }
        });
        for mut pending in due {
            pending.attempts += 1;
            pending.next_retry_at = now + retry_backoff_secs(pending.attempts);
            let dest = pending.envelope.destination.clone().unwrap_or_default();
            let addrs: Vec<String> = self
                .peers
                .get(&dest)
                .map(|p| p.addresses.clone())
                .unwrap_or_default();
            let mut sent = false;
            for addr in addrs {
                if self.send_to_address(&addr, &pending.envelope).await.is_ok() {
                    sent = true;
                    self.stats.retries += 1;
                    break;
                }
            }
            if sent && pending.attempts < pending.max_attempts {
                self.pending_acks.insert(pending.envelope.id.clone(), pending);
            }
        }
        self.stats.ack_pending = self.pending_acks.len() as u64;
    }

    pub fn local_udp_endpoint(&self) -> Option<String> {
        for transport in &self.transports {
            let t = transport.blocking_lock();
            if t.kind() == "udp" {
                return Some(t.local_address().to_string());
            }
        }
        None
    }

    /// Directly broadcast an envelope to all transports that can reach peers.
    async fn broadcast_envelope(&mut self, envelope: Envelope) {
        // Collect all known addresses first to avoid borrow issues.
        let addrs: Vec<String> = self
            .peers
            .values()
            .flat_map(|p| p.addresses.clone())
            .collect();
        for addr in addrs {
            let _ = self.send_to_address(&addr, &envelope).await;
        }
        // If no peers are known, store for later delivery.
        if self.peers.is_empty() {
            self.store_forward.push_back(envelope.clone());
            self.stats.stored += 1;
        }
        self.stats.sent += 1;
    }

    /// Send an envelope to a specific address by trying every transport.
    pub async fn send_to_address(&self, address: &str, envelope: &Envelope) -> Result<(), TransportError> {
        let data = envelope.payload_bytes();
        for transport in &self.transports {
            let mut t = transport.lock().await;
            if let Err(_e) = t.send(address, &data).await {
                continue;
            }
            return Ok(());
        }
        Err(TransportError::Unavailable(address.to_string()))
    }

    /// Receive an envelope from the mesh. The application should poll this.
    pub fn next_inbound(&mut self) -> Option<(Envelope, String)> {
        let env = self.inbound.pop_front()?;
        let sender = self
            .inbound_senders
            .pop_front()
            .unwrap_or_else(|| "unknown".to_string());
        Some((env, sender))
    }

    /// Legacy helper: envelope only.
    pub fn next_inbound_envelope(&mut self) -> Option<Envelope> {
        self.next_inbound().map(|(e, _)| e)
    }

    /// Process raw bytes that arrived from a transport.
    pub async fn ingest(&mut self, transport: &str, sender: &str, bytes: &[u8]) {
        let envelope: Envelope = match serde_json::from_slice(bytes) {
            Ok(e) => e,
            Err(_) => return,
        };

        if !self.seen.insert(envelope.id.clone()) {
            return; // already processed
        }

        self.stats.received += 1;

        // Update peer table from origin if we know an address.
        // We try to parse the origin as a PeerInfo fallback, or use the sender.
        let origin_id = envelope.origin.clone();
        let origin_peer = serde_json::from_str::<PeerInfo>(&envelope.origin).ok();
        self.peers
            .entry(origin_id.clone())
            .and_modify(|p| {
                p.last_seen = unix_now();
                if !p.addresses.contains(&sender.to_string()) {
                    p.addresses.push(sender.to_string());
                }
                if !p.transports.contains(&transport.to_string()) {
                    p.transports.push(transport.to_string());
                }
                if let Some(info) = &origin_peer {
                    p.capabilities = info.capabilities.clone();
                }
            })
            .or_insert_with(|| {
                origin_peer
                    .map(|info| MeshPeer::from_peer_info(&info, transport))
                    .unwrap_or_else(|| MeshPeer {
                        node_id: origin_id.clone(),
                        addresses: vec![sender.to_string()],
                        transports: vec![transport.to_string()],
                        last_seen: unix_now(),
                        ..Default::default()
                    })
            });

        // Is this for us? Either no destination, or destination matches us.
        let for_us = envelope
            .destination
            .as_ref()
            .map(|d| d == &self.identity.node_id)
            .unwrap_or(true);

        if for_us {
            self.stats.delivered += 1;
            if envelope.ack_requested {
                self.send_delivery_ack(&envelope.id, &envelope.origin).await;
            }
            if let Ok(FederationMessage::MeshAck { envelope_id }) =
                serde_json::from_str(&envelope.payload)
            {
                self.handle_mesh_ack(&envelope_id);
                return;
            }
            self.inbound.push_back(envelope.clone());
            self.inbound_senders.push_back(sender.to_string());
        } else if let Some(dest) = &envelope.destination {
            // Route towards a known peer.
            let addrs: Vec<String> = self
                .peers
                .get(dest)
                .map(|p| p.addresses.clone())
                .unwrap_or_default();
            if addrs.is_empty() {
                // Destination unknown: store and forward.
                self.store_forward.push_back(envelope.clone());
                self.stats.stored += 1;
            } else {
                for addr in addrs {
                    let mut fwd = envelope.clone();
                    if !fwd.forward() {
                        self.stats.dropped += 1;
                        continue;
                    }
                    let _ = self.send_to_address(&addr, &fwd).await;
                    self.stats.forwarded += 1;
                }
            }
        } else {
            // Broadcast: flood to all peers except the sender direction.
            let mut fwd = envelope.clone();
            if !fwd.forward() {
                self.stats.dropped += 1;
                return;
            }
            let addrs: Vec<String> = self
                .peers
                .values()
                .flat_map(|p| p.addresses.clone())
                .filter(|a| a != sender)
                .collect();
            for addr in addrs {
                let _ = self.send_to_address(&addr, &fwd).await;
            }
            self.stats.forwarded += 1;
        }
    }

    /// Run one accept loop tick for every transport. This is non-blocking; call it
    /// repeatedly from an async loop.
    pub async fn tick(&mut self) -> Result<(), TransportError> {
        let mut results = Vec::new();
        for transport in &self.transports {
            let mut t = transport.lock().await;
            match t.accept().await {
                Ok((sender, bytes)) => {
                    let kind = t.kind().to_string();
                    drop(t);
                    results.push((kind, sender, bytes));
                }
                Err(_) => {}
            }
        }
        for (kind, sender, bytes) in results {
            self.ingest(&kind, &sender, &bytes).await;
        }
        Ok(())
    }

    /// Try to drain the store-and-forward queue when a new peer appears.
    pub async fn flush_store_forward(&mut self) {
        let mut remaining = VecDeque::new();
        while let Some(envelope) = self.store_forward.pop_front() {
            let mut sent = false;
            if let Some(dest) = &envelope.destination {
                let addrs: Vec<String> = self
                    .peers
                    .get(dest)
                    .map(|p| p.addresses.clone())
                    .unwrap_or_default();
                for addr in addrs {
                    if self.send_to_address(&addr, &envelope).await.is_ok() {
                        sent = true;
                        break;
                    }
                }
            }
            if !sent {
                remaining.push_back(envelope);
            }
        }
        self.store_forward = remaining;
    }

    pub fn summary(&self) -> String {
        format!(
            "KORE-Mesh node {}: {} transports, {} peers, {} inbound, {} store-forward, {} seen | sent {} forwarded {} delivered {} dropped {} stored {} acked {} pending {} retries {}",
            self.identity.node_id,
            self.transports.len(),
            self.peers.len(),
            self.inbound.len(),
            self.store_forward.len(),
            self.seen.len(),
            self.stats.sent,
            self.stats.forwarded,
            self.stats.delivered,
            self.stats.dropped,
            self.stats.stored,
            self.stats.acked,
            self.stats.ack_pending,
            self.stats.retries,
        )
    }
}

fn retry_backoff_secs(attempt: u8) -> u64 {
    match attempt {
        1 => 2,
        2 => 5,
        3 => 15,
        4 => 30,
        _ => 60,
    }
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
