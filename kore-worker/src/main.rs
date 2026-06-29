fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let coord_addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:7878".into());
    let worker_id  = std::env::args().nth(2).unwrap_or_else(|| "worker-1".into());
    println!("[kore-worker] id={worker_id}  coord={coord_addr}");
    rt.block_on(async move {
        let w = kore_worker::Worker::new(worker_id);
        if let Err(e) = w.run(&coord_addr).await {
            eprintln!("worker error: {e}");
        }
    });
}
