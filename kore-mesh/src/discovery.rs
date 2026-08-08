//! KORE-Mesh discovery and NAT traversal helpers.
//!
//! Discovery: KORE learns about other nodes from a small set of bootstrap
//! addresses, then asks those peers for their peer lists. This is the seed
//! that grows the mesh even when there is no central tracker.
//!
//! NAT traversal: UDP is used because it makes hole punching possible. A
//! future version can add STUN/TURN-style rendezvous; for now, KORE relies on
//! at least one public/bootstrap node or pre-shared addresses.

use crate::node::MeshNode;
use crate::transport::TransportError;
use kore_federation::FederationMessage;
use crate::message::Envelope;

/// Bootstrap configuration for a KORE node.
#[derive(Debug, Clone, Default)]
pub struct Bootstrap {
    pub addresses: Vec<String>,
}

impl Bootstrap {
    pub fn from_env() -> Self {
        let mut addrs = Vec::new();
        if let Ok(val) = std::env::var("KORE_MESH_BOOTSTRAP") {
            for a in val.split(',') {
                let a = a.trim();
                if !a.is_empty() {
                    addrs.push(a.to_string());
                }
            }
        }
        Self { addresses: addrs }
    }

    pub fn is_empty(&self) -> bool {
        self.addresses.is_empty()
    }
}

/// Try to discover peers through all bootstrap addresses. Returns the number of
/// discovery messages sent. The responses will arrive as inbound envelopes and
/// be ingested by the normal mesh loop.
pub async fn discover_from_bootstrap(node: &mut MeshNode, bootstrap: &Bootstrap) -> Result<usize, TransportError> {
    if bootstrap.is_empty() {
        return Ok(0);
    }
    let mut sent = 0;
    let origin = node.identity.node_id.clone();
    let payload = serde_json::to_string(&FederationMessage::DiscoverPeers).map_err(|e| {
        TransportError::Unsupported(format!("serialize discover: {}", e))
    })?;
    for addr in &bootstrap.addresses {
        let envelope = Envelope::new(origin.clone(), None, payload.clone());
        node.send_envelope_to_address(addr, &envelope).await?;
        // Remember the bootstrap as a peer so future replies route back.
        node.peers.insert(
            format!("bootstrap-{}", addr),
            crate::MeshPeer {
                node_id: format!("bootstrap-{}", addr),
                addresses: vec![addr.clone()],
                transports: vec!["udp".to_string(), "tcp".to_string()],
                last_seen: crate::node::unix_now(),
                trusted: true,
                ..Default::default()
            },
        );
        sent += 1;
    }
    Ok(sent)
}

/// Merge a received peer list into the mesh peer table. Duplicates are ignored.
pub fn merge_peer_list(node: &mut MeshNode, peers: Vec<kore_federation::PeerInfo>) -> usize {
    let mut added = 0;
    for info in peers {
        if info.node_id == node.identity.node_id {
            continue;
        }
        let id = info.node_id.clone();
        let mut addresses = Vec::new();
        if let Some(addr) = &info.address {
            addresses.push(addr.clone());
        }
        node.peers
            .entry(id.clone())
            .and_modify(|p| {
                p.last_seen = crate::node::unix_now();
                for a in &addresses {
                    if !p.addresses.contains(a) {
                        p.addresses.push(a.clone());
                    }
                }
            })
            .or_insert_with(|| {
                added += 1;
                crate::MeshPeer {
                    node_id: id,
                    owner: info.owner,
                    addresses,
                    transports: vec!["tcp".to_string(), "udp".to_string()],
                    last_seen: crate::node::unix_now(),
                    trusted: false,
                    capabilities: info.capabilities,
                }
            });
    }
    added
}
