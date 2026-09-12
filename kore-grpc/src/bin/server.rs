use kore_grpc::{KoreGrpcServer, ServerConfig};
use tracing_subscriber::layer::SubscriberExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fmt_layer = tracing_subscriber::fmt::layer().pretty();
    let subscriber = tracing_subscriber::registry().with(fmt_layer);
    tracing::subscriber::set_global_default(subscriber)?;

    let config = ServerConfig::default();
    let server = KoreGrpcServer::new(config);
    server.start().await?;
    
    Ok(())
}
