//! Instrumentation helpers for auto-tracking operations

use async_trait::async_trait;
use std::time::Instant;
use tracing::{info, warn};
use crate::metrics::get_metrics;

/// Operation instrumentation trait
#[async_trait]
pub trait Instrumented: Send + Sync {
    async fn execute_instrumented<F, T>(&self, name: &str, f: F) -> Result<T, String>
    where
        F: std::future::Future<Output = Result<T, String>> + Send,
        T: Send,
    {
        let start = Instant::now();
        let metrics = get_metrics();

        info!("Starting operation: {}", name);

        let result = f.await;

        let duration_ms = start.elapsed().as_millis() as f64;

        match &result {
            Ok(_) => {
                info!("Completed operation: {} in {:.2}ms", name, duration_ms);
            }
            Err(e) => {
                warn!("Failed operation: {} after {:.2}ms: {}", name, duration_ms, e);
            }
        }

        result
    }
}

/// Sync instrumentation helper
pub fn instrument_sync<F, T>(name: &str, f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    let start = Instant::now();
    let metrics = get_metrics();

    info!("Starting operation: {}", name);

    let result = f();

    let duration_ms = start.elapsed().as_millis() as f64;

    match &result {
        Ok(_) => {
            info!("Completed operation: {} in {:.2}ms", name, duration_ms);
        }
        Err(e) => {
            warn!("Failed operation: {} after {:.2}ms: {}", name, duration_ms, e);
        }
    }

    result
}

/// Async instrumentation helper
pub async fn instrument_async<F, Fut, T>(name: &str, f: F) -> Result<T, String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, String>> + Send,
    T: Send,
{
    let start = Instant::now();
    let metrics = get_metrics();

    info!("Starting operation: {}", name);

    let result = f().await;

    let duration_ms = start.elapsed().as_millis() as f64;

    match &result {
        Ok(_) => {
            info!("Completed operation: {} in {:.2}ms", name, duration_ms);
        }
        Err(e) => {
            warn!("Failed operation: {} after {:.2}ms: {}", name, duration_ms, e);
        }
    }

    result
}

/// Query instrumentation context
pub struct QueryInstrumentation {
    start: Instant,
    query_name: String,
}

impl QueryInstrumentation {
    /// Create new query instrumentation
    pub fn new(query_name: &str) -> Self {
        let metrics = get_metrics();
        info!("Starting query: {}", query_name);
        metrics.query_total.inc();

        QueryInstrumentation {
            start: Instant::now(),
            query_name: query_name.to_string(),
        }
    }

    /// Record filter pushdown
    pub fn record_filter_pushdown(&self, selectivity: f64, rows_filtered: u64) {
        let metrics = get_metrics();
        metrics.filter_pushdown_queries.inc();
        metrics.filter_selectivity.set(selectivity);
        metrics.rows_filtered.inc_by(rows_filtered);
    }

    /// Record rows processed
    pub fn record_rows_processed(&self, count: u64) {
        let metrics = get_metrics();
        metrics.rows_processed.inc_by(count);
    }
}

impl Drop for QueryInstrumentation {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        let metrics = get_metrics();
        let duration_secs = duration.as_secs_f64();
        metrics
            .query_duration_secs
            .observe(duration_secs);
        info!(
            "Query completed: {} in {:.3}s",
            self.query_name, duration_secs
        );
    }
}

/// Read operation instrumentation context
pub struct ReadInstrumentation {
    start: Instant,
    operation_name: String,
}

impl ReadInstrumentation {
    /// Create new read instrumentation
    pub fn new(operation_name: &str) -> Self {
        let metrics = get_metrics();
        metrics.read_operations.inc();
        info!("Starting read operation: {}", operation_name);

        ReadInstrumentation {
            start: Instant::now(),
            operation_name: operation_name.to_string(),
        }
    }

    /// Record bytes read
    pub fn record_bytes(&self, bytes: u64) {
        let metrics = get_metrics();
        metrics
            .bytes_read_total
            .inc_by(bytes as f64);
    }

    /// Record error
    pub fn record_error(&self) {
        let metrics = get_metrics();
        metrics.read_errors.inc();
    }
}

impl Drop for ReadInstrumentation {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        let metrics = get_metrics();
        let duration_ms = duration.as_millis() as f64;
        metrics.read_latency_ms.observe(duration_ms);
        info!(
            "Read operation completed: {} in {:.2}ms",
            self.operation_name, duration_ms
        );
    }
}

/// Write operation instrumentation context
pub struct WriteInstrumentation {
    start: Instant,
    operation_name: String,
}

impl WriteInstrumentation {
    /// Create new write instrumentation
    pub fn new(operation_name: &str) -> Self {
        let metrics = get_metrics();
        metrics.write_operations.inc();
        info!("Starting write operation: {}", operation_name);

        WriteInstrumentation {
            start: Instant::now(),
            operation_name: operation_name.to_string(),
        }
    }

    /// Record bytes written
    pub fn record_bytes(&self, bytes: u64) {
        let metrics = get_metrics();
        metrics
            .bytes_written_total
            .inc_by(bytes as f64);
    }

    /// Record error
    pub fn record_error(&self) {
        let metrics = get_metrics();
        metrics.write_errors.inc();
    }
}

impl Drop for WriteInstrumentation {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        let metrics = get_metrics();
        let duration_ms = duration.as_millis() as f64;
        metrics.write_latency_ms.observe(duration_ms);
        info!(
            "Write operation completed: {} in {:.2}ms",
            self.operation_name, duration_ms
        );
    }
}

/// Cloud read instrumentation context
pub struct CloudReadInstrumentation {
    start: Instant,
    is_range_request: bool,
}

impl CloudReadInstrumentation {
    /// Create new cloud read instrumentation
    pub fn new(is_range_request: bool) -> Self {
        let metrics = get_metrics();
        metrics.cloud_read_operations.inc();
        if is_range_request {
            metrics.cloud_range_requests.inc();
        }

        CloudReadInstrumentation {
            start: Instant::now(),
            is_range_request,
        }
    }

    /// Record bytes transferred
    pub fn record_bytes_transferred(&self, bytes: u64) {
        let metrics = get_metrics();
        metrics
            .cloud_bytes_transferred
            .inc_by(bytes as f64);
    }
}

impl Drop for CloudReadInstrumentation {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        let metrics = get_metrics();
        let duration_ms = duration.as_millis() as f64;
        metrics.cloud_read_latency_ms.observe(duration_ms);
    }
}

/// Compression instrumentation context
pub struct CompressionInstrumentation {
    start: Instant,
    bytes_before: u64,
}

impl CompressionInstrumentation {
    /// Create new compression instrumentation
    pub fn new(bytes_before: u64) -> Self {
        let metrics = get_metrics();
        metrics.bytes_before_compression.inc_by(bytes_before as f64);

        CompressionInstrumentation {
            start: Instant::now(),
            bytes_before,
        }
    }

    /// Record compression result
    pub fn complete(&self, bytes_after: u64) {
        let metrics = get_metrics();
        metrics
            .bytes_after_compression
            .inc_by(bytes_after as f64);
        metrics.update_compression_ratio();
    }
}

impl Drop for CompressionInstrumentation {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        let metrics = get_metrics();
        let duration_ms = duration.as_millis() as f64;
        metrics.compression_time_ms.observe(duration_ms);
    }
}

/// Instrumentation helper macro
#[macro_export]
macro_rules! instrument {
    ($name:expr) => {
        $crate::instrumentation::instrument_sync($name, || {
            Ok(())
        })
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_instrumentation() {
        let _instr = QueryInstrumentation::new("test_query");
        // Drop is called here
    }

    #[test]
    fn test_read_instrumentation() {
        let instr = ReadInstrumentation::new("test_read");
        instr.record_bytes(1024);
        // Drop is called here
    }

    #[test]
    fn test_write_instrumentation() {
        let instr = WriteInstrumentation::new("test_write");
        instr.record_bytes(2048);
        // Drop is called here
    }

    #[test]
    fn test_compression_instrumentation() {
        let instr = CompressionInstrumentation::new(1000);
        instr.complete(200);
        // Drop is called here
    }
}
