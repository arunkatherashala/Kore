use kore_server::KoreServer;

#[tokio::main]
async fn main() {
    let addr = std::env::var("KORE_SERVER_ADDR").unwrap_or_else(|_| "0.0.0.0:5433".into());
    eprintln!("[kore-server] PostgreSQL wire protocol on {addr}");
    let server = KoreServer::new(&addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    server.serve(listener).await;
}
