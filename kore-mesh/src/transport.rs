//! Transport abstraction for KORE-Mesh.
//!
//! A transport is any medium that can carry bytes to another node: TCP, UDP,
//! file drops, radio, light, sound, memory channels, etc.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Connection refused or unavailable: {0}")]
    Unavailable(String),
    #[error("Unsupported transport: {0}")]
    Unsupported(String),
    #[error("Send to unknown peer: {0}")]
    UnknownPeer(String),
}

/// A pluggable mesh transport. Every transport is both a listener and sender.
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    /// Human readable transport name (tcp, memory, udp, radio, ...).
    fn kind(&self) -> &'static str;

    /// Local address this transport is bound to.
    fn local_address(&self) -> String;

    /// Accept one incoming message. Returns `(sender_address, bytes)`.
    async fn accept(&mut self) -> Result<(String, Vec<u8>), TransportError>;

    /// Send bytes to a remote address. Address format depends on transport.
    async fn send(&mut self, address: &str, data: &[u8]) -> Result<(), TransportError>;
}

/// TCP transport: line-delimited JSON over TCP.
pub struct TcpTransport {
    listener: TcpListener,
    local_address: String,
}

impl TcpTransport {
    pub async fn bind(addr: &str) -> Result<Self, TransportError> {
        let listener = TcpListener::bind(addr).await?;
        let local = listener.local_addr()?.to_string();
        Ok(Self {
            listener,
            local_address: local,
        })
    }
}

#[async_trait::async_trait]
impl Transport for TcpTransport {
    fn kind(&self) -> &'static str {
        "tcp"
    }

    fn local_address(&self) -> String {
        self.local_address.clone()
    }

    async fn accept(&mut self) -> Result<(String, Vec<u8>), TransportError> {
        let (socket, peer) = self.listener.accept().await?;
        let peer_addr = peer.to_string();
        let mut reader = BufReader::new(socket);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        Ok((peer_addr, line.into_bytes()))
    }

    async fn send(&mut self, address: &str, data: &[u8]) -> Result<(), TransportError> {
        let stream = TcpStream::connect(address)
            .await
            .map_err(|e| TransportError::Unavailable(format!("{}: {}", address, e)))?;
        let mut stream = stream;
        stream.write_all(data).await?;
        stream.write_all(b"\n").await?;
        stream.flush().await?;
        Ok(())
    }
}

/// UDP transport: datagrams, one packet per message. Better for NAT traversal
/// and resilient broadcasting than TCP. Each message is one UDP datagram.
pub struct UdpTransport {
    socket: Arc<UdpSocket>,
    local_address: String,
    max_datagram: usize,
}

impl UdpTransport {
    pub async fn bind(addr: &str) -> Result<Self, TransportError> {
        let socket = UdpSocket::bind(addr).await?;
        socket.set_broadcast(true)?;
        let local = socket.local_addr()?.to_string();
        Ok(Self {
            socket: Arc::new(socket),
            local_address: local,
            max_datagram: 65507, // IPv4 max UDP payload
        })
    }
}

#[async_trait::async_trait]
impl Transport for UdpTransport {
    fn kind(&self) -> &'static str {
        "udp"
    }

    fn local_address(&self) -> String {
        self.local_address.clone()
    }

    async fn accept(&mut self) -> Result<(String, Vec<u8>), TransportError> {
        let mut buf = vec![0u8; self.max_datagram];
        let (n, peer) = self.socket.recv_from(&mut buf).await?;
        buf.truncate(n);
        Ok((peer.to_string(), buf))
    }

    async fn send(&mut self, address: &str, data: &[u8]) -> Result<(), TransportError> {
        if data.len() > self.max_datagram {
            return Err(TransportError::Unsupported(format!(
                "UDP datagram too large: {} > {}",
                data.len(),
                self.max_datagram
            )));
        }
        let addr = address
            .parse::<std::net::SocketAddr>()
            .map_err(|e| TransportError::Unavailable(format!("bad address {}: {}", address, e)))?;
        self.socket.send_to(data, addr).await?;
        Ok(())
    }
}

/// In-memory transport for unit tests and same-process mesh simulations.
pub struct MemoryTransport {
    name: String,
    inbox: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    peers: Arc<Mutex<HashMap<String, Arc<Mutex<Vec<(String, Vec<u8>)>>>>>>,
}

impl MemoryTransport {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            inbox: Arc::new(Mutex::new(Vec::new())),
            peers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn clone_inbox(&self) -> Arc<Mutex<Vec<(String, Vec<u8>)>>> {
        self.inbox.clone()
    }

    pub fn connect(&mut self, other: &Self) {
        let mut peers = self.peers.lock().unwrap();
        peers.insert(other.name.clone(), other.inbox.clone());
        let mut other_peers = other.peers.lock().unwrap();
        other_peers.insert(self.name.clone(), self.inbox.clone());
    }
}

#[async_trait::async_trait]
impl Transport for MemoryTransport {
    fn kind(&self) -> &'static str {
        "memory"
    }

    fn local_address(&self) -> String {
        self.name.clone()
    }

    async fn accept(&mut self) -> Result<(String, Vec<u8>), TransportError> {
        loop {
            {
                let mut inbox = self.inbox.lock().unwrap();
                if let Some(msg) = inbox.pop() {
                    return Ok(msg);
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
    }

    async fn send(&mut self, address: &str, data: &[u8]) -> Result<(), TransportError> {
        let peers = self.peers.lock().unwrap();
        let inbox = peers
            .get(address)
            .cloned()
            .ok_or_else(|| TransportError::UnknownPeer(address.to_string()))?;
        inbox.lock().unwrap().push((self.name.clone(), data.to_vec()));
        Ok(())
    }
}

/// Physical transport stubs for KORE-Physical layer.
/// These are placeholders that expose the same Transport interface.
/// Real implementations will interface with hardware (SDR, LED, speaker, etc.).

/// Radio / RF transport (air medium). Stub: stores last packet for testing.
pub struct RadioTransport {
    frequency_mhz: f64,
    last_received: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
}

impl RadioTransport {
    pub fn new(frequency_mhz: f64) -> Self {
        Self {
            frequency_mhz,
            last_received: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Inject a fake packet for testing/demo.
    pub fn inject(&self, sender: &str, data: &[u8]) {
        self.last_received.lock().unwrap().push((sender.to_string(), data.to_vec()));
    }
}

#[async_trait::async_trait]
impl Transport for RadioTransport {
    fn kind(&self) -> &'static str { "radio" }
    fn local_address(&self) -> String { format!("{:.3}MHz", self.frequency_mhz) }

    async fn accept(&mut self) -> Result<(String, Vec<u8>), TransportError> {
        loop {
            if let Some(msg) = self.last_received.lock().unwrap().pop() {
                return Ok(msg);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn send(&mut self, _address: &str, data: &[u8]) -> Result<(), TransportError> {
        eprintln!("[kore-mesh:radio] TX {} bytes on {}", data.len(), self.local_address());
        Ok(())
    }
}

/// Light / optical transport (air medium). Stub: LEDs, Li-Fi, lasers.
pub struct LightTransport {
    channel: String,
    last_received: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
}

impl LightTransport {
    pub fn new(channel: impl Into<String>) -> Self {
        Self {
            channel: channel.into(),
            last_received: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait::async_trait]
impl Transport for LightTransport {
    fn kind(&self) -> &'static str { "light" }
    fn local_address(&self) -> String { self.channel.clone() }

    async fn accept(&mut self) -> Result<(String, Vec<u8>), TransportError> {
        loop {
            if let Some(msg) = self.last_received.lock().unwrap().pop() {
                return Ok(msg);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn send(&mut self, _address: &str, data: &[u8]) -> Result<(), TransportError> {
        eprintln!("[kore-mesh:light] TX {} bytes on {}", data.len(), self.channel);
        Ok(())
    }
}

/// Sound / acoustic transport (air or water medium). Stub for ultrasonic modems.
pub struct SoundTransport {
    frequency_hz: u64,
    medium: String,
    last_received: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
}

impl SoundTransport {
    pub fn new(frequency_hz: u64, medium: impl Into<String>) -> Self {
        Self {
            frequency_hz,
            medium: medium.into(),
            last_received: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait::async_trait]
impl Transport for SoundTransport {
    fn kind(&self) -> &'static str { "sound" }
    fn local_address(&self) -> String { format!("{}Hz/{}", self.frequency_hz, self.medium) }

    async fn accept(&mut self) -> Result<(String, Vec<u8>), TransportError> {
        loop {
            if let Some(msg) = self.last_received.lock().unwrap().pop() {
                return Ok(msg);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn send(&mut self, _address: &str, data: &[u8]) -> Result<(), TransportError> {
        eprintln!("[kore-mesh:sound] TX {} bytes on {}", data.len(), self.local_address());
        Ok(())
    }
}

/// File-drop transport (sneakernet / dead drops). Messages are written to and
/// read from a directory. Survives when all networks are down.
pub struct FileDropTransport {
    dir: std::path::PathBuf,
    seen: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl FileDropTransport {
    pub fn new(dir: impl Into<std::path::PathBuf>) -> Result<Self, TransportError> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            seen: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    fn filename(&self, id: &str) -> std::path::PathBuf {
        self.dir.join(format!("kore-{}.json", id))
    }
}

#[async_trait::async_trait]
impl Transport for FileDropTransport {
    fn kind(&self) -> &'static str { "filedrop" }
    fn local_address(&self) -> String { self.dir.to_string_lossy().to_string() }

    async fn accept(&mut self) -> Result<(String, Vec<u8>), TransportError> {
        loop {
            let entries: Vec<std::path::PathBuf> = std::fs::read_dir(&self.dir)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
                .collect();
            for path in entries {
                let id = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                if self.seen.lock().unwrap().insert(id.clone()) {
                    let bytes = std::fs::read(&path)?;
                    return Ok((self.local_address(), bytes));
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    async fn send(&mut self, _address: &str, data: &[u8]) -> Result<(), TransportError> {
        let id = format!("{}-{:x}", std::process::id(), rand::random::<u64>());
        let path = self.filename(&id);
        std::fs::write(&path, data)?;
        eprintln!("[kore-mesh:filedrop] wrote {} bytes to {}", data.len(), path.display());
        Ok(())
    }
}

/// Helper that builds a transport address string for TCP.
pub fn tcp_address(host: &str, port: u16) -> String {
    format!("{}:{}", host, port)
}

/// Wrapper for transports that need to be shared safely across tasks.
pub type SharedTransport = Arc<tokio::sync::Mutex<Box<dyn Transport>>>;
