//! KORE-Mesh integration for kore-self.
//!
//! KORE-Mesh is KORE's own multi-transport internet. This module starts a
//! `MeshNode` over TCP and UDP, respects survival power modes, and bridges
//! federation messages with NAT rendezvous and reliable delivery.

use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::{sleep, Duration};

use kore_mesh::{
    announce_rendezvous, beacon_to_peer_info, broadcast_lan_beacon, discover_from_bootstrap,
    execute_hole_punch, merge_device_beacon, merge_peer_list, register_rendezvous_peer,
    should_relay, Envelope, MeshCommand, MeshNode, MeshStats, TcpTransport, UdpTransport,
    Transport, transport::TransportError,
};
use kore_federation::FederationMessage;
use kore_survival::SurvivalDecision;

use crate::federation_net;
use crate::survival::{
    mesh_discover_interval_secs, mesh_should_discover, mesh_should_transmit, mesh_tick_interval_ms,
};
use crate::KoreSelf;

pub fn mesh_port() -> u16 {
    std::env::var("KORE_MESH_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8980)
}

fn survival_snapshot(shared_me: &Arc<Mutex<KoreSelf>>) -> (SurvivalDecision, bool) {
    let me = shared_me.lock().unwrap();
    (me.survival.decision.clone(), me.survival.mesh_enabled)
}

/// Build a MeshNode for this KORE instance from the federation identity and peers.
pub async fn build_mesh(shared_me: Arc<Mutex<KoreSelf>>) -> Result<MeshNode, TransportError> {
    let (identity, peers) = {
        let me = shared_me.lock().unwrap();
        (me.federation.identity.clone(), me.federation.peers.clone())
    };

    let port = mesh_port();
    let bind_addr = format!("0.0.0.0:{}", port);
    let transport = TcpTransport::bind(&bind_addr).await?;
    let local_addr = transport.local_address().to_string();
    eprintln!("[kore-mesh] TCP transport bound to {}", local_addr);

    let mut mesh = MeshNode::new(identity);
    mesh.add_transport(Box::new(transport));

    let udp_bind = format!("0.0.0.0:{}", port);
    match UdpTransport::bind(&udp_bind).await {
        Ok(udp) => {
            eprintln!("[kore-mesh] UDP transport bound to {}", udp.local_address());
            mesh.add_transport(Box::new(udp));
        }
        Err(e) => eprintln!("[kore-mesh] UDP bind failed: {e}"),
    }

    for peer in peers {
        if let Some(addr) = peer.address.clone() {
            mesh.peers.insert(
                peer.node_id.clone(),
                kore_mesh::MeshPeer {
                    node_id: peer.node_id,
                    owner: peer.owner,
                    addresses: vec![addr],
                    transports: vec!["tcp".to_string(), "udp".to_string()],
                    last_seen: 0,
                    trusted: peer.trusted,
                    capabilities: peer.capabilities,
                },
            );
        }
    }

    Ok(mesh)
}

/// Start the mesh runtime: store the node inside KoreSelf and run accept/route/flush loops.
pub async fn start_mesh(shared_me: Arc<Mutex<KoreSelf>>) -> Result<(), TransportError> {
    let mesh = build_mesh(Arc::clone(&shared_me)).await?;
    let mesh = Arc::new(AsyncMutex::new(mesh));
    let bootstrap = {
        let me = shared_me.lock().unwrap();
        me.mesh_bootstrap.clone()
    };

    {
        let mut me = shared_me.lock().unwrap();
        me.mesh = Some(mesh.clone());
        eprintln!("[kore-internet] {}", me.kore_internet.summary());
    }

    let shared_for_accept = Arc::clone(&shared_me);
    let accept_mesh = mesh.clone();
    tokio::spawn(async move {
        loop {
            let (decision, mesh_enabled) = survival_snapshot(&shared_for_accept);
            let tick_ms = mesh_tick_interval_ms(&decision);

            let inbound: Vec<(Envelope, String)> = {
                let mut m = accept_mesh.lock().await;
                if let Err(e) = m.tick().await {
                    eprintln!("[kore-mesh] tick error: {e}");
                }
                if mesh_should_transmit(&decision, mesh_enabled) {
                    m.retry_pending_acks().await;
                }
                std::iter::from_fn(|| m.next_inbound()).collect()
            };

            for (envelope, sender_addr) in inbound {
                let reply_to = envelope.origin.clone();
                if let Ok(msg) = serde_json::from_str::<FederationMessage>(&envelope.payload) {
                    match handle_mesh_message(
                        Arc::clone(&shared_for_accept),
                        msg,
                        &sender_addr,
                        &reply_to,
                    )
                    .await
                    {
                        Ok(response) => {
                            if should_reply(&envelope.payload) {
                                send_mesh_reply(
                                    Arc::clone(&shared_for_accept),
                                    &reply_to,
                                    &response,
                                )
                                .await;
                            }
                        }
                        Err(e) => eprintln!("[kore-mesh] ingest error: {e}"),
                    }
                }
            }
            sleep(Duration::from_millis(tick_ms)).await;
        }
    });

    let shared_for_periodic = Arc::clone(&shared_me);
    tokio::spawn(async move {
        let mut interval_secs = mesh_discover_interval_secs(&SurvivalDecision::Normal);
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;

            let (decision, mesh_enabled) = survival_snapshot(&shared_for_periodic);
            let new_interval = mesh_discover_interval_secs(&decision);
            if new_interval != interval_secs {
                interval_secs = new_interval;
                interval = tokio::time::interval(Duration::from_secs(interval_secs));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            }

            if !mesh_should_discover(&decision, mesh_enabled) {
                continue;
            }

            let mesh = {
                let me = shared_for_periodic.lock().unwrap();
                me.mesh.clone()
            };
            if let Some(mesh) = mesh {
                let internet = {
                    let me = shared_for_periodic.lock().unwrap();
                    me.kore_internet.clone()
                };
                let mut m = mesh.lock().await;
                if mesh_should_transmit(&decision, mesh_enabled) {
                    if internet.lan_discovery {
                        let mp = mesh_port();
                        let fp = federation_net::federation_port();
                        if let Err(e) = broadcast_lan_beacon(&mut m, &internet, mp, fp).await {
                            eprintln!("[kore-internet] LAN beacon error: {e}");
                        }
                    }
                    if let Err(e) = m.command(MeshCommand::Discover).await {
                        eprintln!("[kore-mesh] discover error: {e}");
                    }
                    if !bootstrap.is_empty() {
                        match discover_from_bootstrap(&mut m, &bootstrap).await {
                            Ok(n) if n > 0 => {
                                eprintln!("[kore-mesh] sent discovery to {} bootstrap(s)", n);
                            }
                            Err(e) => eprintln!("[kore-mesh] bootstrap discovery error: {e}"),
                            _ => {}
                        }
                        if !matches!(decision, SurvivalDecision::Sleep | SurvivalDecision::Hibernate)
                        {
                            match announce_rendezvous(&mut m, &bootstrap, None).await {
                                Ok(n) if n > 0 => {
                                    eprintln!(
                                        "[kore-mesh] rendezvous announced to {} bootstrap(s)",
                                        n
                                    );
                                }
                                Err(e) => eprintln!("[kore-mesh] rendezvous error: {e}"),
                                _ => {}
                            }
                        }
                    }
                    m.flush_store_forward().await;
                }
            }
        }
    });

    Ok(())
}

fn should_reply(payload: &str) -> bool {
    payload.contains("DiscoverPeers") || payload.contains("\"Ping\"") || payload.contains("Hello")
}

async fn send_mesh_reply(shared_me: Arc<Mutex<KoreSelf>>, destination: &str, response: &str) {
    let mesh = {
        let me = shared_me.lock().unwrap();
        me.mesh.clone()
    };
    let Some(mesh) = mesh else {
        return;
    };
    let mut m = mesh.lock().await;
    if let Err(e) = m
        .command(MeshCommand::SendTo {
            destination: destination.to_string(),
            payload: response.to_string(),
        })
        .await
    {
        eprintln!("[kore-mesh] reply to {destination} failed: {e}");
    }
}

async fn handle_mesh_message(
    shared_me: Arc<Mutex<KoreSelf>>,
    msg: FederationMessage,
    sender_addr: &str,
    origin_node: &str,
) -> Result<String, String> {
    let mut msg = msg;
    loop {
        if let FederationMessage::RelayFrame { destination, payload } = msg {
            let local_id = {
                let me = shared_me.lock().unwrap();
                me.federation.identity.node_id.clone()
            };
            if destination == local_id {
                msg = serde_json::from_str(&payload)
                    .map_err(|e| format!("relay payload parse: {e}"))?;
                continue;
            }
            let (internet, mesh) = {
                let me = shared_me.lock().unwrap();
                (me.kore_internet.clone(), me.mesh.clone())
            };
            let Some(mesh) = mesh else {
                return Err("mesh not running".to_string());
            };
            let mut m = mesh.lock().await;
            if !should_relay(&internet, &m, origin_node) {
                return Err("relay denied for this origin".to_string());
            }
            m.command(MeshCommand::SendTo {
                destination,
                payload,
            })
            .await
            .map_err(|e| e.to_string())?;
            return Ok("relay forwarded".to_string());
        }
        break;
    }

    if let FederationMessage::DeviceBeacon {
        node_id,
        owner,
        mesh_port,
        federation_port: _fp,
        device_kind,
        capabilities,
    } = &msg
    {
        let fed_added = {
            let info = beacon_to_peer_info(
                node_id.clone(),
                owner.clone(),
                sender_addr,
                *mesh_port,
                capabilities.clone(),
            );
            let mut me = shared_me.lock().unwrap();
            me.federation.add_peer(
                info.node_id,
                info.owner,
                info.address,
                Vec::new(),
            )
        };
        let mesh = {
            let me = shared_me.lock().unwrap();
            me.mesh.clone()
        };
        let is_new = if let Some(mesh) = mesh {
            let mut m = mesh.lock().await;
            merge_device_beacon(
                &mut m,
                sender_addr,
                node_id,
                owner,
                *mesh_port,
                device_kind.as_str(),
                capabilities.clone(),
            )
        } else {
            false
        };
        if is_new {
            eprintln!(
                "[kore-internet] LAN device joined: {} ({}) at {}",
                owner, device_kind, sender_addr
            );
        }
        return Ok(format!(
            "device beacon from {} (new={} persisted={})",
            node_id, is_new, fed_added
        ));
    }

    if let FederationMessage::PeerList { peers } = msg {
        let fed_added = {
            let mut me = shared_me.lock().unwrap();
            let mut added = 0;
            for info in &peers {
                if me.federation.add_peer(
                    info.node_id.clone(),
                    info.owner.clone(),
                    info.address.clone(),
                    Vec::new(),
                ) {
                    added += 1;
                }
            }
            added
        };

        let mesh = {
            let me = shared_me.lock().unwrap();
            me.mesh.clone()
        };
        let count = if let Some(mesh) = mesh {
            let mut m = mesh.lock().await;
            merge_peer_list(&mut m, peers)
        } else {
            0
        };
        return Ok(format!(
            "merged {} peers into mesh, {} new federation peers persisted",
            count, fed_added
        ));
    }

    if let FederationMessage::Rendezvous {
        node_id,
        udp_endpoint,
        target_node_id,
    } = &msg
    {
        let mesh = {
            let me = shared_me.lock().unwrap();
            me.mesh.clone()
        };
        if let Some(mesh) = mesh {
            let mut m = mesh.lock().await;
            register_rendezvous_peer(&mut m, node_id, udp_endpoint);
            let local_id = m.identity.node_id.clone();
            if target_node_id.as_deref() == Some(local_id.as_str()) {
                let session_id = format!("{}-{}", node_id, local_id);
                let local_udp = m.local_udp_endpoint().unwrap_or_default();
                let punch = FederationMessage::HolePunch {
                    session_id,
                    initiator_id: node_id.clone(),
                    initiator_udp: udp_endpoint.clone(),
                    target_id: local_id.clone(),
                    target_udp: local_udp,
                };
                let payload = serde_json::to_string(&punch).unwrap_or_default();
                let _ = m
                    .command(MeshCommand::SendTo {
                        destination: node_id.clone(),
                        payload,
                    })
                    .await;
            }
        }
        return Ok("rendezvous registered".to_string());
    }

    if let FederationMessage::HolePunch {
        session_id,
        initiator_id,
        initiator_udp,
        target_id,
        target_udp,
    } = &msg
    {
        let mesh = {
            let me = shared_me.lock().unwrap();
            me.mesh.clone()
        };
        if let Some(mesh) = mesh {
            let mut m = mesh.lock().await;
            execute_hole_punch(
                &mut m,
                session_id,
                initiator_id,
                initiator_udp,
                target_id,
                target_udp,
            )
            .await
            .map_err(|e| e.to_string())?;
        }
        return Ok("hole punch executed".to_string());
    }

    if matches!(msg, FederationMessage::MeshAck { .. }) {
        return Ok("ack".to_string());
    }

    let mut me = shared_me.lock().unwrap();
    match msg {
        FederationMessage::Hello {
            node_id,
            owner,
            capabilities,
        } => {
            let added = me
                .federation
                .add_peer(node_id, owner.clone(), None, Vec::new());
            if added {
                eprintln!("[kore-mesh] peer joined: {} ({:?})", owner, capabilities);
            }
            Ok(serde_json::to_string(&me.federation.hello()).unwrap_or_default())
        }
        FederationMessage::Share { packet } => {
            match me.federation.receive_packet(packet) {
                Ok(memories) => {
                    let count = memories.len();
                    for m in memories {
                        me.raw_ingest(&m.content, &m.kind, m.importance);
                    }
                    Ok(format!("received {} memories", count))
                }
                Err(e) => Err(e.to_string()),
            }
        }
        FederationMessage::DiscoverPeers => Ok(
            serde_json::to_string(&me.federation.peer_list_message()).unwrap_or_default(),
        ),
        FederationMessage::Ping { nonce } => Ok(
            serde_json::to_string(&FederationMessage::Pong { nonce }).unwrap_or_default(),
        ),
        _ => Ok("ok".to_string()),
    }
}

/// Query mesh stats without consuming the node.
pub async fn mesh_stats(mesh: &AsyncMutex<MeshNode>) -> MeshStats {
    mesh.lock().await.stats.clone()
}
