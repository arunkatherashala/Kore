fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let bind = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:7878".into());
    println!("[kore-coord] listening on {bind}");
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&bind).await.unwrap();
        let coord = std::sync::Arc::new(kore_coord::Coordinator::new());
        coord.run(listener).await;
    });
}
