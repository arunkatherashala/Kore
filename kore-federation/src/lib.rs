//! KORE-FEDERATION — Voluntary, ethical network between KORE instances.
//!
//! KORE is not meant to live alone. This crate provides the protocol and identity
//! layer for KORE nodes to discover each other, share knowledge consensually,
//! and remain bound by a shared ethical constitution.
//!
//! Core principles:
//!   - Consent: peering is always opt-in.
//!   - Transparency: every shared packet is signed and traceable.
//!   - Ethics: the local constitution can refuse any action or peer request.
//!   - Privacy: private memory is never shared by default; only explicit
//!     `SharedMemory` fragments cross the wire.

pub mod constitution;
pub mod engine;
pub mod identity;
pub mod message;

pub use constitution::{Constitution, Rule};
pub use engine::{FederationEngine, Peer};
pub use identity::NodeIdentity;
pub use message::{FederationMessage, KnowledgePacket, PeerInfo, SharedMemory};
