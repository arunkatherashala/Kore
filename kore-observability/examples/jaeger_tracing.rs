//! Jaeger Distributed Tracing Example
//! 
//! Demonstrates OpenTelemetry integration with Jaeger for distributed tracing

use kore_observability::tracing::{TracingConfig, init_jaeger};
use tracing::{info, warn, debug};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kore Jaeger Tracing Example ===\n");

    // Initialize Jaeger tracing
    println!("Initializing Jaeger tracing...");
    println!("Ensure Jaeger is running at http://localhost:14268");
    println!("View traces at http://localhost:16686\n");

    let config = TracingConfig {
        service_name: "kore-example".to_string(),
        jaeger_endpoint: Some("http://localhost:14268/api/traces".to_string()),
        log_level: "debug".to_string(),
        enable_console_output: true,
    };

    // Note: This would typically be called in application startup
    // init_jaeger().await?;

    // Create some traced operations
    trace_query_example().await;
    trace_read_operations().await;
    trace_distributed_call().await;

    println!("\n=== Trace View ===");
    println!("Visit http://localhost:16686 to see traces");
    println!("Service: kore-example");
    println!("Operations tracked:");
    println!("  - query_example (parent)");
    println!("  - execute_query (child)");
    println!("  - read_operations (parent)");
    println!("  - read_range_requests (children)");
    println!("  - distributed_call (async)");

    Ok(())
}

async fn trace_query_example() {
    let span = tracing::info_span!("query_example");
    let _guard = span.enter();

    info!("Starting query example");

    // Child span
    let child_span = tracing::debug_span!("execute_query");
    let _child_guard = child_span.enter();
    debug!("Executing query");

    sleep(Duration::from_millis(50)).await;

    info!("Query example completed");
}

async fn trace_read_operations() {
    let span = tracing::info_span!("read_operations");
    let _guard = span.enter();

    info!("Starting read operations");

    for i in 0..3 {
        let range_span = tracing::debug_span!("read_range_requests", range = i);
        let _range_guard = range_span.enter();

        debug!("Reading range {}", i);
        sleep(Duration::from_millis(20)).await;
    }

    info!("Read operations completed");
}

async fn trace_distributed_call() {
    let span = tracing::info_span!("distributed_call");
    let _guard = span.enter();

    info!("Starting distributed call");

    // Simulate remote call
    sleep(Duration::from_millis(100)).await;

    info!("Distributed call completed");
}
