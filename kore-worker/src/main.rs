fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let coord_addr = std::env::args().nth(1)
        .unwrap_or_else(|| kore_net::coord_bind_addr());
    let worker_id  = std::env::args().nth(2).unwrap_or_else(|| "worker-1".into());
    println!("[kore-worker] id={worker_id}  coord={coord_addr}");
    println!("[kore-worker] task bind={}", kore_net::worker_bind_addr());
    if let Ok(ad) = std::env::var("KORE_WORKER_ADVERTISE") {
        println!("[kore-worker] advertise host={ad}");
    }
    rt.block_on(async move {
        let w = kore_worker::Worker::new(worker_id);
        if let Err(e) = w.run(&coord_addr).await {
            eprintln!("worker error: {e}");
        }
    });
}
