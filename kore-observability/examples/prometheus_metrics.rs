//! Prometheus Metrics Example
//! 
//! Demonstrates collecting and exporting Prometheus metrics for Kore operations

use kore_observability::metrics::get_metrics;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get global metrics
    let metrics = get_metrics();

    println!("=== Kore Prometheus Metrics Example ===\n");

    // Simulate query execution
    println!("Simulating query execution...");
    metrics.query_total.inc();
    metrics.query_duration_secs.observe(0.523);

    // Simulate read operations
    println!("Simulating read operations...");
    for _ in 0..10 {
        metrics.read_operations.inc();
        metrics.bytes_read_total.inc_by(1024.0 * 1024.0); // 1MB per read
        metrics.read_latency_ms.observe(45.5);
    }

    // Simulate compression
    println!("Simulating compression...");
    metrics.bytes_before_compression.inc_by(10000.0);
    metrics.bytes_after_compression.inc_by(2000.0);
    metrics.update_compression_ratio();

    // Simulate cache operations
    println!("Simulating cache operations...");
    for _ in 0..80 {
        metrics.cache_hits.inc();
    }
    for _ in 0..20 {
        metrics.cache_misses.inc();
    }
    metrics.cache_size_bytes.set(1024 * 1024 * 100); // 100MB cache
    metrics.update_cache_hit_rate();

    // Simulate filter pushdown
    println!("Simulating filter pushdown...");
    metrics.filter_pushdown_queries.inc_by(50);
    metrics.rows_filtered.inc_by(500000);
    metrics.rows_processed.inc_by(1000000);
    metrics.update_filter_selectivity();

    // Simulate cloud storage reads
    println!("Simulating cloud storage reads...");
    metrics.cloud_read_operations.inc_by(100);
    metrics.cloud_range_requests.inc_by(95); // 95% use range requests
    metrics.cloud_read_latency_ms.observe(125.3);
    metrics.cloud_bytes_transferred.inc_by(1024.0 * 1024.0 * 500.0); // 500MB

    // Export metrics in Prometheus format
    println!("\n=== Prometheus Text Format Export ===\n");
    let export = metrics.export()?;

    // Print first 2KB of metrics (would be exposed on /metrics endpoint)
    let preview = if export.len() > 2000 {
        format!("{}...", &export[..2000])
    } else {
        export.clone()
    };
    println!("{}", preview);

    // Summary statistics
    println!("\n=== Summary Statistics ===");
    println!("Query Total: {}", metrics.query_total.get());
    println!("Read Operations: {}", metrics.read_operations.get());
    println!("Bytes Read: {:.2} MB", metrics.bytes_read_total.get() / 1024.0 / 1024.0);
    println!("Compression Ratio: {:.2}%", metrics.compression_ratio.get() * 100.0);
    println!("Cache Hit Rate: {:.2}%", metrics.cache_hit_rate.get() * 100.0);
    println!("Filter Selectivity: {:.2}%", metrics.filter_selectivity.get() * 100.0);
    println!("Cloud Range Requests: {}", metrics.cloud_range_requests.get());

    println!("\n=== Prometheus Integration ===");
    println!("To use with Prometheus:");
    println!("1. Implement HTTP endpoint at /metrics");
    println!("2. Call metrics.export() and return as text/plain");
    println!("3. Add to prometheus.yml:");
    println!("   scrape_configs:");
    println!("     - job_name: 'kore'");
    println!("       static_configs:");
    println!("         - targets: ['localhost:8080']");
    println!("4. Visualize in Grafana with prometheus data source");

    Ok(())
}
