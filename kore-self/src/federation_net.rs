//! KORE-Federation TCP transport — peer-to-peer messaging over NDJSON.
//!
//! This is a minimal but functional transport layer:
//!   - A TCP server listens for incoming federation messages.
//!   - A TCP client sends messages to peers by address.
//!   - Messages are newline-delimited JSON (NDJSON) for simplicity.
//!
//! Future upgrades can add framing, encryption, NAT traversal, and gossip.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use crate::KoreSelf;

/// Federation port: env var KORE_FEDERATION_PORT or default 8979.
pub fn federation_port() -> u16 {
    std::env::var("KORE_FEDERATION_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8979)
}

/// TCP server that accepts incoming federation peers.
pub async fn federation_server(shared_me: Arc<Mutex<KoreSelf>>) {
    let port = federation_port();
    let listener = match TcpListener::bind(("0.0.0.0", port)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[kore-federation] Cannot bind port {port}: {e}");
            return;
        }
    };
    eprintln!("[kore-federation] Listening for peers on 0.0.0.0:{port}");
    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                let me_arc = Arc::clone(&shared_me);
                tokio::spawn(async move {
                    if let Err(e) = handle_federation_peer(socket, me_arc).await {
                        eprintln!("[kore-federation] peer {} error: {}", addr, e);
                    }
                });
            }
            Err(e) => eprintln!("[kore-federation] accept error: {e}"),
        }
    }
}

async fn handle_federation_peer(
    socket: TcpStream,
    shared_me: Arc<Mutex<KoreSelf>>,
) -> std::io::Result<()> {
    let (reader, mut writer) = socket.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let msg: kore_federation::FederationMessage = match serde_json::from_str(line) {
            Ok(m) => m,
            Err(e) => {
                let err = format!(r#"{{"error":"bad message: {}"}}"#, e);
                writer.write_all(err.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                continue;
            }
        };

        let response = {
            let mut me = shared_me.lock().unwrap();
            match msg {
                kore_federation::FederationMessage::Hello { node_id, owner, capabilities } => {
                    let added = me.federation.add_peer(node_id, owner.clone(), None, Vec::new());
                    if added {
                        eprintln!("[kore-federation] peer joined: {} ({})", owner, capabilities.join(","));
                    }
                    serde_json::to_string(&me.federation.hello()).unwrap_or_default()
                }
                kore_federation::FederationMessage::Share { packet } => {
                    match me.federation.receive_packet(packet) {
                        Ok(memories) => {
                            let count = memories.len();
                            for m in memories {
                                me.raw_ingest(&m.content, &m.kind, m.importance);
                            }
                            format!(r#"{{"status":"received {} memories"}}"#, count)
                        }
                        Err(e) => format!(r#"{{"error":"{}"}}"#, e),
                    }
                }
                kore_federation::FederationMessage::DiscoverPeers => {
                    serde_json::to_string(&me.federation.peer_list_message()).unwrap_or_default()
                }
                kore_federation::FederationMessage::Ping { nonce } => {
                    serde_json::to_string(&kore_federation::FederationMessage::Pong { nonce })
                        .unwrap_or_default()
                }
                _ => r#"{"status":"ok"}"#.to_string(),
            }
        };

        writer.write_all(response.as_bytes()).await?;
        writer.write_all(b"\n").await?;
    }
    Ok(())
}

/// Send a single federation message to a peer and return the response line.
pub async fn federation_send(
    address: &str,
    message: &kore_federation::FederationMessage,
) -> Result<String, String> {
    let stream = TcpStream::connect(address)
        .await
        .map_err(|e| format!("connect error: {e}"))?;
    let (reader, mut writer) = stream.into_split();
    let json = serde_json::to_string(message).map_err(|e| format!("serialize error: {e}"))?;
    writer
        .write_all(json.as_bytes())
        .await
        .map_err(|e| format!("write error: {e}"))?;
    writer
        .write_all(b"\n")
        .await
        .map_err(|e| format!("write error: {e}"))?;

    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .await
        .map_err(|e| format!("read error: {e}"))?;
    Ok(line.trim().to_string())
}

/// Periodic outbound task: send Hello to known peers with addresses.
pub async fn federation_outbound(shared_me: Arc<Mutex<KoreSelf>>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        let (enabled, peers, hello) = {
            let me = shared_me.lock().unwrap();
            if !me.federation.enabled {
                continue;
            }
            let peers: Vec<String> = me
                .federation
                .peers
                .iter()
                .filter_map(|p| p.address.clone())
                .collect();
            let hello = me.federation.hello();
            (true, peers, hello)
        };
        if !enabled || peers.is_empty() {
            continue;
        }
        for addr in peers {
            match federation_send(&addr, &hello).await {
                Ok(resp) => eprintln!("[kore-federation] hello to {addr}: {resp}"),
                Err(e) => eprintln!("[kore-federation] hello to {addr} failed: {e}"),
            }
        }
    }
}
