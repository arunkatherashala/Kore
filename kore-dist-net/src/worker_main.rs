fn main() {
    let port = std::env::args()
        .skip_while(|a| a != "--port")
        .nth(1)
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(9001);

    let addr = format!("0.0.0.0:{port}");
    eprintln!("[kore-worker-node] Starting on {addr}");
    eprintln!("[kore-worker-node] Ready to receive distributed query tasks");
    
    kore_dist_net::run_worker(&addr).expect("Worker failed");
}
