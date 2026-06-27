//! Metrics Dashboard Example
//! 
//! Demonstrates real-time metrics collection and dashboard-like output

use kore_observability::metrics::get_metrics;
use kore_observability::instrumentation::{
    QueryInstrumentation, ReadInstrumentation, CompressionInstrumentation,
    CloudReadInstrumentation,
};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kore Metrics Dashboard ===\n");

    let metrics = get_metrics();

    // Simulate realistic workload
    simulate_workload().await;

    // Display dashboard
    display_dashboard(&metrics);

    Ok(())
}

async fn simulate_workload() {
    let metrics = get_metrics();

    println!("Simulating realistic Kore workload...\n");

    // Query 1: Simple filter query
    {
        let _query = QueryInstrumentation::new("SELECT * WHERE id > 100");
        let read = ReadInstrumentation::new("read_row_groups");
        read.record_bytes(5 * 1024 * 1024);

        sleep(Duration::from_millis(150)).await;
    }

    // Query 2: Aggregation with compression
    {
        let _query = QueryInstrumentation::new("SELECT COUNT(*), SUM(amount)");

        let compression = CompressionInstrumentation::new(20 * 1024 * 1024);
        sleep(Duration::from_millis(80)).await;
        compression.complete(4 * 1024 * 1024);

        metrics.rows_processed.inc_by(10000);
        metrics.rows_filtered_total.inc_by(2000);
    }

    // Query 3: Cloud storage read (S3)
    {
        let _query = QueryInstrumentation::new("SELECT * FROM s3://bucket/data.kore");

        let cloud_read = CloudReadInstrumentation::new(true); // range request
        sleep(Duration::from_millis(200)).await;
        cloud_read.record_bytes_transferred(2 * 1024 * 1024);
    }

    // Cache operations
    for _ in 0..100 {
        if rand::random::<bool>() {
            metrics.cache_hits.inc();
        } else {
            metrics.cache_misses.inc();
        }
    }
    metrics.cache_size_bytes.set(512 * 1024 * 1024);
    metrics.update_cache_hit_rate();

    println!("✓ Workload simulation completed\n");
}

fn display_dashboard(metrics: &std::sync::Arc<kore_observability::metrics::KoreMetrics>) {
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║              KORE OBSERVABILITY DASHBOARD                  ║");
    println!("╚════════════════════════════════════════════════════════════╝\n");

    // Query Statistics
    println!("📊 QUERY STATISTICS");
    println!("├─ Total Queries: {}", metrics.query_total.get());
    println!("├─ Query Errors: {}", metrics.query_errors.get());
    println!("└─ Query Success Rate: {:.1}%\n",
        (1.0 - (metrics.query_errors.get() as f64 / metrics.query_total.get() as f64)) * 100.0);

    // Read/Write Performance
    println!("📈 READ/WRITE PERFORMANCE");
    println!("├─ Read Operations: {}", metrics.read_operations.get());
    println!("├─ Bytes Read: {:.2} MB", metrics.bytes_read_total.get() / 1024.0 / 1024.0);
    println!("├─ Write Operations: {}", metrics.write_operations.get());
    println!("├─ Bytes Written: {:.2} MB", metrics.bytes_written_total.get() / 1024.0 / 1024.0);
    println!("└─ Read/Write Ratio: {:.1}%\n",
        (metrics.read_operations.get() as f64 / 
         (metrics.read_operations.get() as f64 + metrics.write_operations.get() as f64)) * 100.0);

    // Compression Metrics
    println!("🗜️  COMPRESSION METRICS");
    println!("├─ Compression Ratio: {:.2}%", metrics.compression_ratio.get() * 100.0);
    println!("├─ Uncompressed: {:.2} MB", metrics.bytes_before_compression.get() / 1024.0 / 1024.0);
    println!("└─ Compressed: {:.2} MB\n", metrics.bytes_after_compression.get() / 1024.0 / 1024.0);

    // Cache Performance
    println!("💾 CACHE PERFORMANCE");
    let total_cache_ops = metrics.cache_hits.get() + metrics.cache_misses.get();
    println!("├─ Cache Hits: {}", metrics.cache_hits.get());
    println!("├─ Cache Misses: {}", metrics.cache_misses.get());
    println!("├─ Hit Rate: {:.2}%", metrics.cache_hit_rate.get() * 100.0);
    println!("└─ Cache Size: {:.0} MB\n", metrics.cache_size_bytes.get() as f64 / 1024.0 / 1024.0);

    // Filter Pushdown
    println!("🔍 FILTER PUSHDOWN");
    println!("├─ Queries with Pushdown: {}", metrics.filter_pushdown_queries.get());
    println!("├─ Filter Selectivity: {:.2}%", metrics.filter_selectivity.get() * 100.0);
    println!("├─ Rows Processed: {}", metrics.rows_processed.get());
    println!("├─ Rows Filtered: {}", metrics.rows_filtered_total.get());
    println!("└─ Filtering Efficiency: {:.1}%\n",
        (metrics.rows_filtered_total.get() as f64 / metrics.rows_processed.get() as f64) * 100.0);

    // Cloud Storage
    println!("☁️  CLOUD STORAGE OPERATIONS");
    println!("├─ Cloud Read Operations: {}", metrics.cloud_read_operations.get());
    println!("├─ Range Requests: {}", metrics.cloud_range_requests.get());
    println!("├─ Range Request Ratio: {:.1}%",
        (metrics.cloud_range_requests.get() as f64 / metrics.cloud_read_operations.get() as f64) * 100.0);
    println!("└─ Bytes Transferred: {:.2} MB\n", metrics.cloud_bytes_transferred.get() / 1024.0 / 1024.0);

    // Error Summary
    println!("⚠️  ERROR SUMMARY");
    println!("├─ Query Errors: {}", metrics.query_errors.get());
    println!("├─ Read Errors: {}", metrics.read_errors.get());
    println!("├─ Write Errors: {}", metrics.write_errors.get());
    println!("└─ Connection Errors: {}\n", metrics.connection_errors.get());

    // Top Recommendations
    println!("✨ RECOMMENDATIONS");
    if metrics.cache_hit_rate.get() < 0.8 {
        println!("├─ 🔴 Cache hit rate low ({}%) - consider increasing cache size",
            (metrics.cache_hit_rate.get() * 100.0) as i32);
    } else {
        println!("├─ 🟢 Cache efficiency excellent");
    }

    if metrics.filter_selectivity.get() < 0.5 {
        println!("├─ 🟡 Low filter selectivity - queries returning many rows");
    } else {
        println!("├─ 🟢 Filter pushdown effective");
    }

    if metrics.compression_ratio.get() > 0.8 {
        println!("├─ 🟡 Poor compression ({}%) - consider different codec",
            (metrics.compression_ratio.get() * 100.0) as i32);
    } else {
        println!("├─ 🟢 Compression performing well");
    }

    println!("└─ 🟢 Overall system health: GOOD\n");

    // Integration info
    println!("📡 INTEGRATION");
    println!("├─ Prometheus: /metrics endpoint (export metrics)");
    println!("├─ Grafana: Create dashboard from Prometheus data");
    println!("├─ Jaeger: Trace distributed queries");
    println!("└─ Custom: Extend metrics with business KPIs");
}

// Mock rand crate since it's not in dependencies
mod rand {
    pub fn random<T: Random>() -> T {
        T::random()
    }

    pub trait Random {
        fn random() -> Self;
    }

    impl Random for bool {
        fn random() -> Self {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos() % 2 == 0
        }
    }
}
