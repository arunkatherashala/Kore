//! KORE-Mesh — the decentralized network layer of KORE.
//!
//! KORE-Mesh is KORE's own internet: a multi-transport, peer-to-peer overlay
//! (KORE Internet) that connects devices over LAN broadcast, bootstrap rendezvous,
//! optional relay nodes, TCP, UDP, file drops, radio, light, sound, or any future medium.
//!
//! Core concepts:
//! - `Transport` trait: every medium implements this.
//! - `MeshNode`: one node per KORE instance, owns transports and peer table.
//! - `Envelope`: a routed wrapper around a `FederationMessage`.
//! - `MeshCommand`: high-level API to broadcast, send to one node, or discover.

pub mod discovery;
pub mod internet;
pub mod message;
pub mod nat;
pub mod node;
pub mod transport;

pub use discovery::{Bootstrap, discover_from_bootstrap, merge_peer_list};
pub use internet::{
    advertised_tcp_endpoint, beacon_to_peer_info, broadcast_lan_beacon, merge_device_beacon,
    parse_kore_uri, resolve_kore_uri, should_relay, KoreInternet,
};
pub use message::{Envelope, MeshCommand};
pub use nat::{announce_rendezvous, execute_hole_punch, register_rendezvous_peer};
pub use node::{MeshNode, MeshPeer, MeshStats, PendingDelivery};
pub use transport::{MemoryTransport, TcpTransport, UdpTransport, RadioTransport, LightTransport, SoundTransport, FileDropTransport, Transport, TransportError, SharedTransport, tcp_address};
