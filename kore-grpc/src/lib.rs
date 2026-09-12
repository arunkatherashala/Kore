//! KORE gRPC Server - Multi-language RPC interface
pub mod server;
pub mod session;
pub mod monitoring;

pub use server::KoreGrpcServer;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub listen_addr: String,
    pub max_connections: usize,
    pub request_timeout_ms: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:50051".to_string(),
            max_connections: 1000,
            request_timeout_ms: 30000,
        }
    }
}
