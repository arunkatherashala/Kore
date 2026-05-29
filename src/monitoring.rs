//! KORE Performance Monitoring
//! 
//! Provides real-time metrics collection, performance tracking, and system health monitoring
//! across all KORE operations. Metrics are collected with minimal overhead and can be
//! exported in multiple formats (JSON, Prometheus, CSV).

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, Duration, Instant};
use std::collections::HashMap;
use serde::{Serialize, Deserialize};

/// Performance metrics for KORE operations
#[derive(Debug, Clone)]
pub struct KoreMetrics {
    /// Total bytes read from files
    pub total_bytes_read: u64,
    
    /// Total bytes written to files
    pub total_bytes_written: u64,
    
    /// Total number of read operations
    pub read_operations: u64,
    
    /// Total number of write operations
    pub write_operations: u64,
    
    /// Average read latency (milliseconds)
    pub avg_read_latency_ms: f64,
    
    /// Average write latency (milliseconds)
    pub avg_write_latency_ms: f64,
    
    /// Total compression achieved (percentage)
    pub compression_ratio: f64,
    
    /// Peak memory usage (bytes)
    pub peak_memory_bytes: u64,
    
    /// Current memory usage (bytes)
    pub current_memory_bytes: u64,
    
    /// Total rows processed
    pub rows_processed: u64,
    
    /// Total columns processed
    pub columns_processed: u64,
    
    /// Timestamp when metrics were collected
    pub timestamp: String,
    
    /// Query cache hit rate (0.0 - 1.0)
    pub cache_hit_rate: f64,
    
    /// Number of active concurrent operations
    pub active_operations: u32,
    
    /// Total errors encountered
    pub total_errors: u32,
}

impl KoreMetrics {
    pub fn new() -> Self {
        Self {
            total_bytes_read: 0,
            total_bytes_written: 0,
            read_operations: 0,
            write_operations: 0,
            avg_read_latency_ms: 0.0,
            avg_write_latency_ms: 0.0,
            compression_ratio: 0.0,
            peak_memory_bytes: 0,
            current_memory_bytes: 0,
            rows_processed: 0,
            columns_processed: 0,
            timestamp: chrono::Local::now().to_rfc3339(),
            cache_hit_rate: 0.0,
            active_operations: 0,
            total_errors: 0,
        }
    }
    
    /// Calculate throughput in MB/s
    pub fn read_throughput_mbps(&self) -> f64 {
        if self.read_operations == 0 {
            return 0.0;
        }
        (self.total_bytes_read as f64 / 1_000_000.0) / (self.avg_read_latency_ms / 1000.0)
    }
    
    /// Calculate write throughput in MB/s
    pub fn write_throughput_mbps(&self) -> f64 {
        if self.write_operations == 0 {
            return 0.0;
        }
        (self.total_bytes_written as f64 / 1_000_000.0) / (self.avg_write_latency_ms / 1000.0)
    }
}

/// Individual operation metric
#[derive(Debug, Clone)]
pub struct OperationMetric {
    pub operation_type: String,
    pub duration_ms: f64,
    pub bytes_processed: u64,
    pub success: bool,
    pub timestamp: Instant,
}

/// Centralized performance monitor for KORE
pub struct PerformanceMonitor {
    metrics: Arc<Mutex<KoreMetrics>>,
    operation_history: Arc<Mutex<Vec<OperationMetric>>>,
    alerts: Arc<Mutex<Vec<AlertEvent>>>,
}

/// Performance alert thresholds exceeded
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertEvent {
    pub alert_type: AlertType,
    pub message: String,
    pub timestamp: String,
    pub metric_value: f64,
    pub threshold: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertType {
    HighLatency,
    LowThroughput,
    HighMemoryUsage,
    HighErrorRate,
    LowCacheHitRate,
}

impl PerformanceMonitor {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(Mutex::new(KoreMetrics::new())),
            operation_history: Arc::new(Mutex::new(Vec::new())),
            alerts: Arc::new(Mutex::new(Vec::new())),
        }
    }
    
    /// Record a read operation
    pub fn record_read(&self, bytes: u64, duration_ms: f64) {
        let mut metrics = self.metrics.lock().unwrap();
        metrics.total_bytes_read += bytes;
        metrics.read_operations += 1;
        
        let new_avg = (metrics.avg_read_latency_ms * (metrics.read_operations - 1) as f64 + duration_ms) 
            / metrics.read_operations as f64;
        metrics.avg_read_latency_ms = new_avg;
        
        let op = OperationMetric {
            operation_type: "read".to_string(),
            duration_ms,
            bytes_processed: bytes,
            success: true,
            timestamp: Instant::now(),
        };
        self.operation_history.lock().unwrap().push(op);
        
        // Check alerts
        if duration_ms > 1000.0 {
            self.create_alert(
                AlertType::HighLatency,
                format!("Read operation took {:.2}ms", duration_ms),
                duration_ms,
                1000.0,
            );
        }
    }
    
    /// Record a write operation
    pub fn record_write(&self, bytes: u64, duration_ms: f64) {
        let mut metrics = self.metrics.lock().unwrap();
        metrics.total_bytes_written += bytes;
        metrics.write_operations += 1;
        
        let new_avg = (metrics.avg_write_latency_ms * (metrics.write_operations - 1) as f64 + duration_ms)
            / metrics.write_operations as f64;
        metrics.avg_write_latency_ms = new_avg;
        
        let op = OperationMetric {
            operation_type: "write".to_string(),
            duration_ms,
            bytes_processed: bytes,
            success: true,
            timestamp: Instant::now(),
        };
        self.operation_history.lock().unwrap().push(op);
    }
    
    /// Record compression statistics
    pub fn record_compression(&self, original_bytes: u64, compressed_bytes: u64) {
        let mut metrics = self.metrics.lock().unwrap();
        let ratio = (1.0 - compressed_bytes as f64 / original_bytes as f64) * 100.0;
        metrics.compression_ratio = ratio;
    }
    
    /// Record rows and columns processed
    pub fn record_rows_columns(&self, rows: u64, columns: u64) {
        let mut metrics = self.metrics.lock().unwrap();
        metrics.rows_processed += rows;
        metrics.columns_processed += columns;
    }
    
    /// Update memory usage
    pub fn update_memory(&self, current: u64, peak: u64) {
        let mut metrics = self.metrics.lock().unwrap();
        metrics.current_memory_bytes = current;
        metrics.peak_memory_bytes = peak;
        
        if current > peak {
            metrics.peak_memory_bytes = current;
        }
        
        // Check memory threshold (e.g., 80% of some limit)
        if current > 1_000_000_000 {  // 1GB
            self.create_alert(
                AlertType::HighMemoryUsage,
                format!("Memory usage: {:.2}MB", current as f64 / 1_000_000.0),
                current as f64,
                1_000_000_000.0,
            );
        }
    }
    
    /// Update cache statistics
    pub fn update_cache_stats(&self, hit_rate: f64) {
        let mut metrics = self.metrics.lock().unwrap();
        metrics.cache_hit_rate = hit_rate;
        
        if hit_rate < 0.3 {
            self.create_alert(
                AlertType::LowCacheHitRate,
                format!("Cache hit rate: {:.2}%", hit_rate * 100.0),
                hit_rate,
                0.3,
            );
        }
    }
    
    /// Record an error
    pub fn record_error(&self) {
        let mut metrics = self.metrics.lock().unwrap();
        metrics.total_errors += 1;
        
        let error_rate = metrics.total_errors as f64 / (metrics.read_operations + metrics.write_operations) as f64;
        if error_rate > 0.05 {  // > 5% error rate
            self.create_alert(
                AlertType::HighErrorRate,
                format!("Error rate: {:.2}%", error_rate * 100.0),
                error_rate,
                0.05,
            );
        }
    }
    
    /// Create an alert event
    fn create_alert(&self, alert_type: AlertType, message: String, metric_value: f64, threshold: f64) {
        let alert = AlertEvent {
            alert_type,
            message,
            timestamp: chrono::Local::now().to_rfc3339(),
            metric_value,
            threshold,
        };
        self.alerts.lock().unwrap().push(alert);
    }
    
    /// Get current metrics snapshot
    pub fn get_metrics(&self) -> KoreMetrics {
        self.metrics.lock().unwrap().clone()
    }
    
    /// Get all alerts
    pub fn get_alerts(&self) -> Vec<AlertEvent> {
        self.alerts.lock().unwrap().clone()
    }
    
    /// Clear alerts
    pub fn clear_alerts(&self) {
        self.alerts.lock().unwrap().clear();
    }
    
    /// Export metrics as JSON
    pub fn export_json(&self) -> String {
        let metrics = self.metrics.lock().unwrap();
        format!(
            r#"{{
  "total_bytes_read": {},
  "total_bytes_written": {},
  "read_operations": {},
  "write_operations": {},
  "avg_read_latency_ms": {:.2},
  "avg_write_latency_ms": {:.2},
  "read_throughput_mbps": {:.2},
  "write_throughput_mbps": {:.2},
  "compression_ratio": {:.2},
  "peak_memory_bytes": {},
  "current_memory_bytes": {},
  "rows_processed": {},
  "columns_processed": {},
  "cache_hit_rate": {:.2},
  "active_operations": {},
  "total_errors": {},
  "timestamp": "{}"
}}"#,
            metrics.total_bytes_read,
            metrics.total_bytes_written,
            metrics.read_operations,
            metrics.write_operations,
            metrics.avg_read_latency_ms,
            metrics.avg_write_latency_ms,
            metrics.read_throughput_mbps(),
            metrics.write_throughput_mbps(),
            metrics.compression_ratio,
            metrics.peak_memory_bytes,
            metrics.current_memory_bytes,
            metrics.rows_processed,
            metrics.columns_processed,
            metrics.cache_hit_rate,
            metrics.active_operations,
            metrics.total_errors,
            metrics.timestamp,
        )
    }
    
    /// Export metrics as Prometheus format
    pub fn export_prometheus(&self) -> String {
        let metrics = self.metrics.lock().unwrap();
        format!(
            "kore_total_bytes_read_total {{}} {}\n\
             kore_total_bytes_written_total {{}} {}\n\
             kore_read_operations_total {{}} {}\n\
             kore_write_operations_total {{}} {}\n\
             kore_avg_read_latency_ms {{}} {:.2}\n\
             kore_avg_write_latency_ms {{}} {:.2}\n\
             kore_compression_ratio_percent {{}} {:.2}\n\
             kore_current_memory_bytes {{}} {}\n\
             kore_cache_hit_rate {{}} {:.2}\n\
             kore_total_errors {{}} {}\n",
            metrics.total_bytes_read,
            metrics.total_bytes_written,
            metrics.read_operations,
            metrics.write_operations,
            metrics.avg_read_latency_ms,
            metrics.avg_write_latency_ms,
            metrics.compression_ratio,
            metrics.current_memory_bytes,
            metrics.cache_hit_rate,
            metrics.total_errors,
        )
    }
}

impl Default for PerformanceMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_monitor_read_operation() {
        let monitor = PerformanceMonitor::new();
        monitor.record_read(1_000_000, 100.0);
        
        let metrics = monitor.get_metrics();
        assert_eq!(metrics.total_bytes_read, 1_000_000);
        assert_eq!(metrics.read_operations, 1);
        assert_eq!(metrics.avg_read_latency_ms, 100.0);
    }
    
    #[test]
    fn test_monitor_compression() {
        let monitor = PerformanceMonitor::new();
        monitor.record_compression(10_000_000, 1_500_000);
        
        let metrics = monitor.get_metrics();
        assert!((metrics.compression_ratio - 85.0).abs() < 0.1);
    }
    
    #[test]
    fn test_throughput_calculation() {
        let monitor = PerformanceMonitor::new();
        monitor.record_read(10_000_000, 1000.0);  // 10MB in 1s = 10 MB/s
        
        let metrics = monitor.get_metrics();
        assert!(metrics.read_throughput_mbps() > 9.0 && metrics.read_throughput_mbps() < 11.0);
    }
}
