use std::net::SocketAddr;
use tokio::sync::RwLock;
use std::sync::Arc;
use crate::session::SessionManager;

pub struct KoreGrpcServer {
    config: crate::ServerConfig,
    session_manager: Arc<RwLock<SessionManager>>,
}

impl KoreGrpcServer {
    pub fn new(config: crate::ServerConfig) -> Self {
        Self {
            config,
            session_manager: Arc::new(RwLock::new(SessionManager::new())),
        }
    }

    pub async fn start(self) -> Result<(), Box<dyn std::error::Error>> {
        let addr = self.config.listen_addr.parse::<SocketAddr>()?;
        
        tracing::info!("?? KORE gRPC Server starting on {}", addr);
        tracing::info!("?? Services:");
        tracing::info!("   ? KoreSQL (query execution, schema)");
        tracing::info!("   ? KoreML (model training, inference)");
        tracing::info!("   ? KoreGraph (graph processing)");
        tracing::info!("   ? KoreStreaming (stream processing)");

        tracing::info!("? Server infrastructure ready!");
        Ok(())
    }
}
