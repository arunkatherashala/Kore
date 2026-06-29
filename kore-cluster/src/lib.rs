//! Layer 19 — KoreCluster
//!
//! Multi-process query distribution over TCP.
//!
//! Architecture:
//!   Coordinator  — splits DataBlocks into shards, distributes to Workers,
//!                  merges partial results.
//!   Worker       — tokio TCP server; executes tasks (currently: row count +
//!                  pass-through) and returns partial DataBlocks.
//!   Protocol     — length-prefixed JSON messages (u32 BE length + JSON body).

pub mod protocol;
pub mod worker;
pub mod coordinator;

pub use coordinator::Coordinator;
pub use worker::Worker;
pub use protocol::{KoreMessage, TaskPayload, ResultPayload};
