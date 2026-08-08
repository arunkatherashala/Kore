//! NAT hole punching via bootstrap rendezvous.
//!
//! Private nodes register their UDP endpoint with bootstrap peers. When two
//! nodes want to connect, the bootstrap (or a peer that saw a `Rendezvous`)
//! emits a `HolePunch` message so both sides send UDP probes to each other.

use kore_federation::FederationMessage;

use crate::message::Envelope;
use crate::node::MeshNode;
use crate::transport::TransportError;
use crate::Bootstrap;

/// Announce this node's UDP endpoint to every bootstrap address for rendezvous.
pub async fn announce_rendezvous(
    node: &mut MeshNode,
    bootstrap: &Bootstrap,
    target_node_id: Option<String>,
) -> Result<usize, TransportError> {
    if bootstrap.is_empty() {
        return Ok(0);
    }
    let udp = node
        .local_udp_endpoint()
        .unwrap_or_else(|| "0.0.0.0:0".to_string());
    let msg = FederationMessage::Rendezvous {
        node_id: node.identity.node_id.clone(),
        udp_endpoint: udp,
        target_node_id,
    };
    let payload = serde_json::to_string(&msg).map_err(|e| {
        TransportError::Unsupported(format!("serialize rendezvous: {}", e))
    })?;
    let origin = node.identity.node_id.clone();
    let mut sent = 0;
    for addr in &bootstrap.addresses {
        let envelope = Envelope::new(origin.clone(), None, payload.clone());
        node.send_envelope_to_address(addr, &envelope).await?;
        sent += 1;
    }
    Ok(sent)
}

/// Apply a rendezvous registration: remember the peer's UDP endpoint.
pub fn register_rendezvous_peer(
    node: &mut MeshNode,
    node_id: &str,
    udp_endpoint: &str,
) {
    if node_id == node.identity.node_id {
        return;
    }
    node.peers
        .entry(node_id.to_string())
        .and_modify(|p| {
            p.last_seen = crate::node::unix_now();
            if !p.addresses.contains(&udp_endpoint.to_string()) {
                p.addresses.push(udp_endpoint.to_string());
            }
            if !p.transports.contains(&"udp".to_string()) {
                p.transports.push("udp".to_string());
            }
        })
        .or_insert_with(|| crate::MeshPeer {
            node_id: node_id.to_string(),
            addresses: vec![udp_endpoint.to_string()],
            transports: vec!["udp".to_string()],
            last_seen: crate::node::unix_now(),
            ..Default::default()
        });
}

/// Send coordinated UDP punch packets to both endpoints listed in a HolePunch message.
pub async fn execute_hole_punch(
    node: &mut MeshNode,
    session_id: &str,
    initiator_id: &str,
    initiator_udp: &str,
    target_id: &str,
    target_udp: &str,
) -> Result<(), TransportError> {
    let local_id = node.identity.node_id.clone();
    register_rendezvous_peer(node, initiator_id, initiator_udp);
    register_rendezvous_peer(node, target_id, target_udp);

    let punch_payload = serde_json::to_string(&FederationMessage::Ping {
        nonce: session_id.len() as u64,
    })
    .unwrap_or_else(|_| "{}".to_string());

    let punch_to = if local_id == initiator_id {
        target_udp
    } else if local_id == target_id {
        initiator_udp
    } else {
        // Bootstrap relay: poke both sides to open pinholes.
        let origin = node.identity.node_id.clone();
        for endpoint in [initiator_udp, target_udp] {
            let env = Envelope::new(origin.clone(), None, punch_payload.clone());
            let _ = node.send_envelope_to_address(endpoint, &env).await;
        }
        return Ok(());
    };

    let origin = node.identity.node_id.clone();
    let env = Envelope::new(origin, None, punch_payload);
    node.send_envelope_to_address(punch_to, &env).await
}
