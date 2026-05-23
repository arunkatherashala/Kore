use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server, StatusCode};
use std::net::SocketAddr;

async fn handle(_req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(r#"{"status":"healthy","version":"1.2.2","service":"kore-cloud"}"#))
        .unwrap();
    Ok(response)
}

#[tokio::main]
async fn main() {
    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    
    let make_svc = make_service_fn(|_conn| async {
        Ok::<_, hyper::Error>(service_fn(handle))
    });

    let server = Server::bind(&addr).serve(make_svc);
    
    println!("Kore Cloud Service listening on {}", addr);
    
    if let Err(e) = server.await {
        eprintln!("Server error: {}", e);
    }
}
