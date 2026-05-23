//! Prometheus metrics for Kore operations

use prometheus::{
    Counter, Gauge, Histogram, HistogramOpts, IntCounter, IntGauge, Opts, Registry,
};
use std::sync::Arc;

/// Kore metrics registry and collectors
pub struct KoreMetrics {
    pub registry: Registry,

    // Query execution metrics
    pub query_duration_secs: Histogram,
    pub query_total: IntCounter,
    pub query_errors: IntCounter,

    // Data read metrics
    pub bytes_read_total: Counter,
    pub read_operations: IntCounter,
    pub read_errors: IntCounter,
    pub read_latency_ms: Histogram,

    // Data write metrics
    pub bytes_written_total: Counter,
    pub write_operations: IntCounter,
    pub write_errors: IntCounter,
    pub write_latency_ms: Histogram,

    // Compression metrics
    pub compression_ratio: Gauge,
    pub bytes_before_compression: Counter,
    pub bytes_after_compression: Counter,
    pub compression_time_ms: Histogram,

    // Cache metrics
    pub cache_hits: IntCounter,
    pub cache_misses: IntCounter,
    pub cache_hit_rate: Gauge,
    pub cache_size_bytes: IntGauge,

    // Filter pushdown metrics
    pub filter_pushdown_queries: IntCounter,
    pub filter_selectivity: Gauge,
    pub rows_filtered: IntCounter,

    // Row group metrics
    pub row_groups_read: IntCounter,
    pub rows_processed: IntCounter,
    pub rows_filtered_total: IntCounter,

    // Cloud storage metrics (S3/GCS/Azure)
    pub cloud_read_operations: IntCounter,
    pub cloud_read_latency_ms: Histogram,
    pub cloud_range_requests: IntCounter,
    pub cloud_bytes_transferred: Counter,

    // Connection pool metrics
    pub active_connections: IntGauge,
    pub connection_errors: IntCounter,
}

impl KoreMetrics {
    /// Create new metrics registry
    pub fn new() -> Result<Arc<Self>, prometheus::Error> {
        let registry = Registry::new();

        let metrics = KoreMetrics {
            // Query metrics
            query_duration_secs: Histogram::with_opts(
                HistogramOpts::new("kore_query_duration_secs", "Query execution duration in seconds")
                    .buckets(vec![0.001, 0.01, 0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0]),
            )?,
            query_total: IntCounter::with_opts(Opts::new(
                "kore_query_total",
                "Total number of queries executed",
            ))?,
            query_errors: IntCounter::with_opts(Opts::new(
                "kore_query_errors_total",
                "Total number of query errors",
            ))?,

            // Read metrics
            bytes_read_total: Counter::with_opts(Opts::new(
                "kore_bytes_read_total",
                "Total bytes read from Kore files",
            ))?,
            read_operations: IntCounter::with_opts(Opts::new(
                "kore_read_operations_total",
                "Total read operations",
            ))?,
            read_errors: IntCounter::with_opts(Opts::new(
                "kore_read_errors_total",
                "Total read errors",
            ))?,
            read_latency_ms: Histogram::with_opts(
                HistogramOpts::new("kore_read_latency_ms", "Read operation latency in milliseconds")
                    .buckets(vec![1.0, 5.0, 10.0, 50.0, 100.0, 500.0, 1000.0, 5000.0]),
            )?,

            // Write metrics
            bytes_written_total: Counter::with_opts(Opts::new(
                "kore_bytes_written_total",
                "Total bytes written to Kore files",
            ))?,
            write_operations: IntCounter::with_opts(Opts::new(
                "kore_write_operations_total",
                "Total write operations",
            ))?,
            write_errors: IntCounter::with_opts(Opts::new(
                "kore_write_errors_total",
                "Total write errors",
            ))?,
            write_latency_ms: Histogram::with_opts(
                HistogramOpts::new("kore_write_latency_ms", "Write operation latency in milliseconds")
                    .buckets(vec![1.0, 5.0, 10.0, 50.0, 100.0, 500.0, 1000.0]),
            )?,

            // Compression metrics
            compression_ratio: Gauge::with_opts(Opts::new(
                "kore_compression_ratio",
                "Current compression ratio (compressed/original)",
            ))?,
            bytes_before_compression: Counter::with_opts(Opts::new(
                "kore_bytes_before_compression_total",
                "Total bytes before compression",
            ))?,
            bytes_after_compression: Counter::with_opts(Opts::new(
                "kore_bytes_after_compression_total",
                "Total bytes after compression",
            ))?,
            compression_time_ms: Histogram::with_opts(
                HistogramOpts::new("kore_compression_time_ms", "Compression time in milliseconds")
                    .buckets(vec![1.0, 5.0, 10.0, 50.0, 100.0, 500.0, 1000.0]),
            )?,

            // Cache metrics
            cache_hits: IntCounter::with_opts(Opts::new(
                "kore_cache_hits_total",
                "Total cache hits",
            ))?,
            cache_misses: IntCounter::with_opts(Opts::new(
                "kore_cache_misses_total",
                "Total cache misses",
            ))?,
            cache_hit_rate: Gauge::with_opts(Opts::new(
                "kore_cache_hit_rate",
                "Current cache hit rate (0-1)",
            ))?,
            cache_size_bytes: IntGauge::with_opts(Opts::new(
                "kore_cache_size_bytes",
                "Current cache size in bytes",
            ))?,

            // Filter pushdown metrics
            filter_pushdown_queries: IntCounter::with_opts(Opts::new(
                "kore_filter_pushdown_queries_total",
                "Queries with filter pushdown",
            ))?,
            filter_selectivity: Gauge::with_opts(Opts::new(
                "kore_filter_selectivity",
                "Average filter selectivity ratio",
            ))?,
            rows_filtered: IntCounter::with_opts(Opts::new(
                "kore_rows_filtered_total",
                "Total rows filtered by pushdown",
            ))?,

            // Row group metrics
            row_groups_read: IntCounter::with_opts(Opts::new(
                "kore_row_groups_read_total",
                "Total row groups read",
            ))?,
            rows_processed: IntCounter::with_opts(Opts::new(
                "kore_rows_processed_total",
                "Total rows processed",
            ))?,
            rows_filtered_total: IntCounter::with_opts(Opts::new(
                "kore_rows_filtered_total",
                "Total rows filtered",
            ))?,

            // Cloud storage metrics
            cloud_read_operations: IntCounter::with_opts(Opts::new(
                "kore_cloud_read_operations_total",
                "Cloud storage read operations",
            ))?,
            cloud_read_latency_ms: Histogram::with_opts(
                HistogramOpts::new("kore_cloud_read_latency_ms", "Cloud read latency in milliseconds")
                    .buckets(vec![10.0, 50.0, 100.0, 500.0, 1000.0, 5000.0, 10000.0]),
            )?,
            cloud_range_requests: IntCounter::with_opts(Opts::new(
                "kore_cloud_range_requests_total",
                "HTTP Range requests to cloud storage",
            ))?,
            cloud_bytes_transferred: Counter::with_opts(Opts::new(
                "kore_cloud_bytes_transferred_total",
                "Bytes transferred from cloud storage",
            ))?,

            // Connection metrics
            active_connections: IntGauge::with_opts(Opts::new(
                "kore_active_connections",
                "Current active connections",
            ))?,
            connection_errors: IntCounter::with_opts(Opts::new(
                "kore_connection_errors_total",
                "Total connection errors",
            ))?,

            registry,
        };

        // Register all metrics
        metrics.registry.register(Box::new(metrics.query_duration_secs.clone()))?;
        metrics.registry.register(Box::new(metrics.query_total.clone()))?;
        metrics.registry.register(Box::new(metrics.query_errors.clone()))?;
        metrics.registry.register(Box::new(metrics.bytes_read_total.clone()))?;
        metrics.registry.register(Box::new(metrics.read_operations.clone()))?;
        metrics.registry.register(Box::new(metrics.read_errors.clone()))?;
        metrics.registry.register(Box::new(metrics.read_latency_ms.clone()))?;
        metrics.registry.register(Box::new(metrics.bytes_written_total.clone()))?;
        metrics.registry.register(Box::new(metrics.write_operations.clone()))?;
        metrics.registry.register(Box::new(metrics.write_errors.clone()))?;
        metrics.registry.register(Box::new(metrics.write_latency_ms.clone()))?;
        metrics.registry.register(Box::new(metrics.compression_ratio.clone()))?;
        metrics.registry.register(Box::new(metrics.bytes_before_compression.clone()))?;
        metrics.registry.register(Box::new(metrics.bytes_after_compression.clone()))?;
        metrics.registry.register(Box::new(metrics.compression_time_ms.clone()))?;
        metrics.registry.register(Box::new(metrics.cache_hits.clone()))?;
        metrics.registry.register(Box::new(metrics.cache_misses.clone()))?;
        metrics.registry.register(Box::new(metrics.cache_hit_rate.clone()))?;
        metrics.registry.register(Box::new(metrics.cache_size_bytes.clone()))?;
        metrics.registry.register(Box::new(metrics.filter_pushdown_queries.clone()))?;
        metrics.registry.register(Box::new(metrics.filter_selectivity.clone()))?;
        metrics.registry.register(Box::new(metrics.rows_filtered.clone()))?;
        metrics.registry.register(Box::new(metrics.row_groups_read.clone()))?;
        metrics.registry.register(Box::new(metrics.rows_processed.clone()))?;
        metrics.registry.register(Box::new(metrics.rows_filtered_total.clone()))?;
        metrics.registry.register(Box::new(metrics.cloud_read_operations.clone()))?;
        metrics.registry.register(Box::new(metrics.cloud_read_latency_ms.clone()))?;
        metrics.registry.register(Box::new(metrics.cloud_range_requests.clone()))?;
        metrics.registry.register(Box::new(metrics.cloud_bytes_transferred.clone()))?;
        metrics.registry.register(Box::new(metrics.active_connections.clone()))?;
        metrics.registry.register(Box::new(metrics.connection_errors.clone()))?;

        Ok(Arc::new(metrics))
    }

    /// Export metrics in Prometheus text format
    pub fn export(&self) -> Result<String, prometheus::Error> {
        let mut buffer = vec![];
        let encoder = prometheus::TextEncoder::new();
        let metric_families = self.registry.gather();
        encoder.encode(&metric_families, &mut buffer)?;
        Ok(String::from_utf8(buffer).unwrap_or_default())
    }

    /// Calculate and update cache hit rate
    pub fn update_cache_hit_rate(&self) {
        let hits = self.cache_hits.get();
        let misses = self.cache_misses.get();
        let total = hits + misses;
        if total > 0 {
            self.cache_hit_rate
                .set((hits as f64) / (total as f64));
        }
    }

    /// Calculate and update compression ratio
    pub fn update_compression_ratio(&self) {
        let before = self.bytes_before_compression.get();
        let after = self.bytes_after_compression.get();
        if before > 0.0 {
            self.compression_ratio.set(after / before);
        }
    }

    /// Calculate and update filter selectivity
    pub fn update_filter_selectivity(&self) {
        let filtered = self.rows_filtered_total.get();
        let processed = self.rows_processed.get();
        if processed > 0 {
            self.filter_selectivity
                .set(1.0 - ((filtered as f64) / (processed as f64)));
        }
    }
}

impl Default for KoreMetrics {
    fn default() -> Self {
        Self::new().unwrap()
    }
}

/// Global metrics registry
pub use once_cell::sync::Lazy;
pub static METRICS: Lazy<Arc<KoreMetrics>> =
    Lazy::new(|| KoreMetrics::new().expect("Failed to initialize metrics"));

/// Initialize Prometheus metrics (global)
pub fn init_prometheus() -> Result<(), Box<dyn std::error::Error>> {
    // Trigger lazy initialization
    let _ = METRICS.clone();
    Ok(())
}

/// Get global metrics registry
pub fn get_metrics() -> Arc<KoreMetrics> {
    METRICS.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = KoreMetrics::new();
        assert!(metrics.is_ok());
    }

    #[test]
    fn test_metrics_export() {
        let metrics = KoreMetrics::new().unwrap();
        let export = metrics.export();
        assert!(export.is_ok());
        let text = export.unwrap();
        assert!(text.contains("kore_query_total"));
    }

    #[test]
    fn test_cache_hit_rate_calculation() {
        let metrics = KoreMetrics::new().unwrap();
        metrics.cache_hits.inc_by(80);
        metrics.cache_misses.inc_by(20);
        metrics.update_cache_hit_rate();
        let rate = metrics.cache_hit_rate.get();
        assert!((rate - 0.8).abs() < 0.01);
    }

    #[test]
    fn test_compression_ratio_calculation() {
        let metrics = KoreMetrics::new().unwrap();
        metrics.bytes_before_compression.inc_by(1000.0);
        metrics.bytes_after_compression.inc_by(200.0);
        metrics.update_compression_ratio();
        let ratio = metrics.compression_ratio.get();
        assert!((ratio - 0.2).abs() < 0.01);
    }

    #[test]
    fn test_filter_selectivity_calculation() {
        let metrics = KoreMetrics::new().unwrap();
        metrics.rows_processed.inc_by(1000);
        metrics.rows_filtered_total.inc_by(300);
        metrics.update_filter_selectivity();
        let selectivity = metrics.filter_selectivity.get();
        assert!((selectivity - 0.7).abs() < 0.01);
    }
}
