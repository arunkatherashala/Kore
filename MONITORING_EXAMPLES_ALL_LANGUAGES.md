# KORE v1.2.4 - Performance Monitoring Examples

Complete examples showing how to use KORE monitoring across all programming languages.

## Table of Contents
- [Python Examples](#python-examples)
- [JavaScript/Node.js Examples](#javascriptnodejs-examples)
- [Java Examples](#java-examples)
- [Rust Examples](#rust-examples)

---

## Python Examples

### Basic Monitoring

```python
from kore_fileformat import PerformanceMonitor, KoreReader, KoreWriter

# Create a performance monitor instance
monitor = PerformanceMonitor()

# Simulate read operations with monitoring
bytes_read = 10_000_000  # 10 MB
latency_ms = 150  # 150 milliseconds
monitor.record_read(bytes_read, latency_ms)

# Simulate write operations
bytes_written = 1_500_000  # 1.5 MB (with compression)
write_latency = 75  # 75 milliseconds
monitor.record_write(bytes_written, write_latency)

# Record compression statistics
original_size = 10_000_000
compressed_size = 1_500_000
monitor.record_compression(original_size, compressed_size)

# Update rows and columns processed
rows = 100_000
columns = 50
monitor.record_rows_columns(rows, columns)

# Get metrics as dictionary
metrics = monitor.get_metrics_dict()
print(f"Read Throughput: {metrics['read_throughput_mbps']:.2f} MB/s")
print(f"Write Throughput: {metrics['write_throughput_mbps']:.2f} MB/s")
print(f"Compression Ratio: {metrics['compression_ratio']:.1f}%")
print(f"Rows Processed: {metrics['rows_processed']:,}")
```

### Advanced Monitoring with Real Operations

```python
import time
from kore_fileformat import PerformanceMonitor, KoreReader

monitor = PerformanceMonitor()

# Monitor actual read operation
start_time = time.time()
reader = KoreReader("large_file.kore")
file_size, version = reader.read_file()
elapsed_ms = (time.time() - start_time) * 1000

# Record to monitor
monitor.record_read(file_size, elapsed_ms)

# Monitor memory usage
current_memory = 512_000_000  # 512 MB
peak_memory = 1_000_000_000   # 1 GB
monitor.update_memory(current_memory, peak_memory)

# Monitor cache effectiveness
cache_hit_rate = 0.85  # 85% hit rate
monitor.update_cache_stats(cache_hit_rate)

# Get comprehensive metrics
metrics = monitor.get_metrics_dict()
print(f"Operation Time: {metrics['avg_read_latency_ms']:.2f}ms")
print(f"Memory Usage: {metrics['current_memory_bytes'] / 1_000_000:.1f}MB")
print(f"Cache Hit Rate: {metrics['cache_hit_rate']:.1%}")
```

### Export and Analysis

```python
from kore_fileformat import PerformanceMonitor
import json

monitor = PerformanceMonitor()

# ... perform operations ...

# Export as JSON
json_metrics = monitor.get_metrics()
print(json_metrics)

# Save to file
with open("kore_metrics.json", "w") as f:
    f.write(json_metrics)

# Export as Prometheus format
prometheus_metrics = monitor.export_prometheus()
print(prometheus_metrics)

# Get alerts
alerts = monitor.get_alerts()
print(f"Number of alerts: {len(alerts)}")
for alert in alerts:
    print(f"  - {alert['alert_type']}: {alert['message']}")

# Clear alerts
monitor.clear_alerts()
```

### Streaming Operations with Monitoring

```python
from kore_fileformat import PerformanceMonitor, KoreStreamingWriter

monitor = PerformanceMonitor()

# Create streaming writer
writer = KoreStreamingWriter("output.kore", chunk_size=100_000)

# Process rows with monitoring
rows_processed = 0
columns = 50
start_time = time.time()

for row in large_dataset:
    writer.write_row(row)
    rows_processed += 1
    
    # Update metrics every 10k rows
    if rows_processed % 10_000 == 0:
        elapsed_ms = (time.time() - start_time) * 1000
        monitor.record_rows_columns(rows_processed, columns)
        print(f"Processed {rows_processed:,} rows")

# Final flush and monitoring
elapsed_ms = (time.time() - start_time) * 1000
writer.flush()
monitor.record_write(writer.bytes_written(), elapsed_ms)

print(f"Total: {rows_processed:,} rows in {elapsed_ms:.0f}ms")
print(f"Throughput: {monitor.get_metrics_dict()['write_throughput_mbps']:.2f} MB/s")
```

---

## JavaScript/Node.js Examples

### Basic Monitoring

```javascript
const { PerformanceMonitor } = require('@kore/cloud');

// Create a performance monitor instance
const monitor = new PerformanceMonitor();

// Simulate read operations
const bytesRead = 10_000_000;
const readLatencyMs = 150;
monitor.recordRead(bytesRead, readLatencyMs);

// Simulate write operations
const bytesWritten = 1_500_000;
const writeLatencyMs = 75;
monitor.recordWrite(bytesWritten, writeLatencyMs);

// Record compression
monitor.recordCompression(10_000_000, 1_500_000);

// Record rows and columns
monitor.recordRowsColumns(100_000, 50);

// Get metrics
const metrics = monitor.getMetricsDict();
console.log(`Read Throughput: ${metrics.read_throughput_mbps.toFixed(2)} MB/s`);
console.log(`Compression Ratio: ${metrics.compression_ratio.toFixed(1)}%`);
console.log(`Rows Processed: ${metrics.rows_processed.toLocaleString()}`);
```

### Async Operations with Monitoring

```javascript
const { PerformanceMonitor, KoreReader } = require('@kore/cloud');

async function monitoredRead() {
  const monitor = new PerformanceMonitor();
  
  try {
    const startTime = Date.now();
    
    // Perform actual read
    const reader = new KoreReader('large_file.kore');
    const data = await reader.readFile();
    
    const elapsedMs = Date.now() - startTime;
    
    // Record operation
    monitor.recordRead(data.length, elapsedMs);
    
    // Get metrics
    const metrics = monitor.getMetricsDict();
    console.log(`Latency: ${metrics.avg_read_latency_ms.toFixed(2)}ms`);
    console.log(`Throughput: ${metrics.read_throughput_mbps.toFixed(2)} MB/s`);
    
  } catch (error) {
    monitor.recordError();
    console.error('Read failed:', error);
  }
}

monitoredRead();
```

### Real-time Performance Dashboard

```javascript
const { PerformanceMonitor } = require('@kore/cloud');

class KorePerformanceDashboard {
  constructor() {
    this.monitor = new PerformanceMonitor();
    this.updateInterval = 1000; // 1 second
  }

  async startMonitoring(operationInterval = 100) {
    setInterval(() => {
      // Simulate operations
      const bytes = Math.random() * 1_000_000;
      const latency = Math.random() * 500;
      
      if (Math.random() > 0.5) {
        this.monitor.recordRead(bytes, latency);
      } else {
        this.monitor.recordWrite(bytes, latency);
      }
    }, operationInterval);

    // Print dashboard
    setInterval(() => {
      const metrics = this.monitor.getMetricsDict();
      console.clear();
      console.log('=== KORE Performance Dashboard ===');
      console.log(`Read Ops:     ${metrics.read_operations}`);
      console.log(`Write Ops:    ${metrics.write_operations}`);
      console.log(`Read Lat:     ${metrics.avg_read_latency_ms.toFixed(2)}ms`);
      console.log(`Write Lat:    ${metrics.avg_write_latency_ms.toFixed(2)}ms`);
      console.log(`Read Tput:    ${metrics.read_throughput_mbps.toFixed(2)} MB/s`);
      console.log(`Write Tput:   ${metrics.write_throughput_mbps.toFixed(2)} MB/s`);
      console.log(`Compression:  ${metrics.compression_ratio.toFixed(1)}%`);
      console.log(`Cache Hit:    ${(metrics.cache_hit_rate * 100).toFixed(1)}%`);
      console.log(`Memory:       ${(metrics.current_memory_bytes / 1_000_000).toFixed(1)} MB`);
      console.log(`Errors:       ${metrics.total_errors}`);
    }, this.updateInterval);
  }

  exportPrometheus() {
    return this.monitor.exportPrometheus();
  }

  getAlerts() {
    return this.monitor.getAlerts();
  }
}

// Usage
const dashboard = new KorePerformanceDashboard();
dashboard.startMonitoring();
```

### Export Metrics to Files

```javascript
const { PerformanceMonitor } = require('@kore/cloud');
const fs = require('fs');

const monitor = new PerformanceMonitor();

// ... perform operations ...

// Export JSON
const jsonMetrics = monitor.getMetrics();
fs.writeFileSync('kore_metrics.json', jsonMetrics);

// Export Prometheus format
const prometheusMetrics = monitor.exportPrometheus();
fs.writeFileSync('kore_metrics.prom', prometheusMetrics);

// Get and log alerts
const alerts = monitor.getAlerts();
console.log(`${alerts.length} alerts detected:`);
alerts.forEach(alert => {
  console.log(`  [${alert.alert_type}] ${alert.message}`);
});
```

---

## Java Examples

### Basic Monitoring

```java
import com.kore.cloud.PerformanceMonitor;

public class KoreMonitoringExample {
  public static void main(String[] args) {
    // Create performance monitor
    PerformanceMonitor monitor = new PerformanceMonitor();

    // Record read operation
    long bytesRead = 10_000_000L;
    double readLatencyMs = 150.0;
    monitor.recordRead(bytesRead, readLatencyMs);

    // Record write operation
    long bytesWritten = 1_500_000L;
    double writeLatencyMs = 75.0;
    monitor.recordWrite(bytesWritten, writeLatencyMs);

    // Record compression
    monitor.recordCompression(10_000_000L, 1_500_000L);

    // Record rows and columns
    monitor.recordRowsColumns(100_000L, 50L);

    // Get metrics
    KoreMetrics metrics = monitor.getMetrics();
    System.out.printf("Read Throughput: %.2f MB/s%n", metrics.getReadThroughputMbps());
    System.out.printf("Compression: %.1f%%%n", metrics.getCompressionRatio());
    System.out.printf("Rows Processed: %,d%n", metrics.getRowsProcessed());
  }
}
```

### Advanced Monitoring with File Operations

```java
import com.kore.cloud.PerformanceMonitor;
import com.kore.cloud.KoreReader;
import java.io.IOException;

public class AdvancedMonitoringExample {
  public static void main(String[] args) throws IOException {
    PerformanceMonitor monitor = new PerformanceMonitor();
    
    long startTime = System.nanoTime();
    
    try {
      // Perform actual read
      KoreReader reader = new KoreReader("large_file.kore");
      byte[] data = reader.readFile();
      
      // Calculate elapsed time
      long elapsedNano = System.nanoTime() - startTime;
      double elapsedMs = elapsedNano / 1_000_000.0;
      
      // Record operation
      monitor.recordRead(data.length, elapsedMs);
      
      // Update memory statistics
      Runtime runtime = Runtime.getRuntime();
      long currentMemory = runtime.totalMemory() - runtime.freeMemory();
      long maxMemory = runtime.maxMemory();
      monitor.updateMemory(currentMemory, maxMemory);
      
      // Print metrics
      KoreMetrics metrics = monitor.getMetrics();
      System.out.printf("Operation Latency: %.2f ms%n", metrics.getAvgReadLatencyMs());
      System.out.printf("Throughput: %.2f MB/s%n", metrics.getReadThroughputMbps());
      System.out.printf("Memory Usage: %.1f MB%n", currentMemory / 1_000_000.0);
      
    } catch (Exception e) {
      monitor.recordError();
      System.err.println("Operation failed: " + e.getMessage());
    }
  }
}
```

### Streaming with Monitoring

```java
import com.kore.cloud.PerformanceMonitor;
import com.kore.cloud.KoreStreamingWriter;
import java.io.IOException;

public class StreamingMonitoringExample {
  public static void main(String[] args) throws IOException {
    PerformanceMonitor monitor = new PerformanceMonitor();
    KoreStreamingWriter writer = new KoreStreamingWriter("output.kore", 100_000);
    
    long startTime = System.nanoTime();
    long rowsProcessed = 0;
    int columns = 50;
    
    try {
      // Process rows
      for (String[] row : getLargeDataset()) {
        writer.writeRow(row);
        rowsProcessed++;
        
        // Update metrics periodically
        if (rowsProcessed % 10_000 == 0) {
          monitor.recordRowsColumns(rowsProcessed, columns);
          System.out.printf("Processed %,d rows%n", rowsProcessed);
        }
      }
      
      // Final flush
      long elapsedNano = System.nanoTime() - startTime;
      double elapsedMs = elapsedNano / 1_000_000.0;
      
      writer.flush();
      monitor.recordWrite(writer.getBytesWritten(), elapsedMs);
      
      // Print summary
      KoreMetrics metrics = monitor.getMetrics();
      System.out.printf("%nTotal: %,d rows in %.0f ms%n", rowsProcessed, elapsedMs);
      System.out.printf("Throughput: %.2f MB/s%n", metrics.getWriteThroughputMbps());
      System.out.printf("Compression: %.1f%%%n", metrics.getCompressionRatio());
      
    } finally {
      writer.close();
    }
  }

  static java.util.List<String[]> getLargeDataset() {
    // Return your data here
    return new java.util.ArrayList<>();
  }
}
```

### Metrics Export and Alerts

```java
import com.kore.cloud.PerformanceMonitor;
import com.kore.cloud.AlertEvent;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.List;

public class MetricsExportExample {
  public static void main(String[] args) throws Exception {
    PerformanceMonitor monitor = new PerformanceMonitor();
    
    // ... perform operations ...
    
    // Export as JSON
    String jsonMetrics = monitor.getMetrics();
    Files.write(Paths.get("kore_metrics.json"), jsonMetrics.getBytes());
    
    // Export as Prometheus format
    String prometheusMetrics = monitor.exportPrometheus();
    Files.write(Paths.get("kore_metrics.prom"), prometheusMetrics.getBytes());
    
    // Get and print alerts
    List<AlertEvent> alerts = monitor.getAlerts();
    System.out.printf("Detected %d alerts:%n", alerts.size());
    for (AlertEvent alert : alerts) {
      System.out.printf("  [%s] %s (%.2f > %.2f)%n",
        alert.getAlertType(),
        alert.getMessage(),
        alert.getMetricValue(),
        alert.getThreshold()
      );
    }
    
    // Clear alerts
    monitor.clearAlerts();
  }
}
```

---

## Rust Examples

### Basic Monitoring in Rust

```rust
use kore::monitoring::{PerformanceMonitor, KoreMetrics};

fn main() {
    // Create a performance monitor
    let monitor = PerformanceMonitor::new();

    // Record read operation
    let bytes_read = 10_000_000u64;
    let read_latency_ms = 150.0;
    monitor.record_read(bytes_read, read_latency_ms);

    // Record write operation
    let bytes_written = 1_500_000u64;
    let write_latency_ms = 75.0;
    monitor.record_write(bytes_written, write_latency_ms);

    // Record compression
    monitor.record_compression(10_000_000, 1_500_000);

    // Record rows and columns
    monitor.record_rows_columns(100_000, 50);

    // Get metrics
    let metrics = monitor.get_metrics();
    println!("Read Throughput: {:.2} MB/s", metrics.read_throughput_mbps());
    println!("Compression Ratio: {:.1}%", metrics.compression_ratio);
    println!("Rows Processed: {}", metrics.rows_processed);
}
```

### Advanced Monitoring with Error Handling

```rust
use kore::monitoring::PerformanceMonitor;
use std::time::Instant;

fn monitored_file_operation() -> Result<(), Box<dyn std::error::Error>> {
    let monitor = PerformanceMonitor::new();
    
    let start = Instant::now();
    
    // Perform operation
    let result = std::fs::read("large_file.kore")?;
    
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    
    // Record operation
    monitor.record_read(result.len() as u64, elapsed_ms);
    
    // Get metrics
    let metrics = monitor.get_metrics();
    println!("Latency: {:.2}ms", metrics.avg_read_latency_ms);
    println!("Throughput: {:.2} MB/s", metrics.read_throughput_mbps());
    
    Ok(())
}

fn main() {
    if let Err(e) = monitored_file_operation() {
        eprintln!("Error: {}", e);
    }
}
```

---

## Performance Monitoring Best Practices

### 1. **Sampling Strategy**
```python
# Record metrics periodically, not for every operation
if operation_count % 100 == 0:
    metrics = monitor.get_metrics_dict()
    save_metrics(metrics)
```

### 2. **Alert Thresholds**
- High Latency: > 1000ms
- Low Throughput: < 1 MB/s
- High Memory: > 1GB
- Error Rate: > 5%
- Low Cache Hit: < 30%

### 3. **Metric Export**
Export metrics regularly for analysis:
- JSON format for dashboards
- Prometheus format for Grafana/monitoring systems
- CSV for trend analysis

### 4. **Continuous Monitoring**
```javascript
// Update dashboard every 1-5 seconds
setInterval(() => {
  const metrics = monitor.getMetricsDict();
  updateDashboard(metrics);
}, 5000);
```

### 5. **Production Deployment**
- Enable all monitoring in development
- Selectively enable in production (performance overhead)
- Use sampling for high-volume operations
- Export metrics to centralized logging system

---

## API Reference

### PerformanceMonitor Methods

| Method | Parameters | Purpose |
|--------|-----------|---------|
| `record_read()` | bytes, latency_ms | Record read operation |
| `record_write()` | bytes, latency_ms | Record write operation |
| `record_compression()` | original, compressed | Record compression stats |
| `record_rows_columns()` | rows, columns | Track rows/columns processed |
| `update_memory()` | current, peak | Update memory statistics |
| `update_cache_stats()` | hit_rate | Update cache hit rate |
| `record_error()` | - | Increment error counter |
| `get_metrics()` | - | Return JSON string |
| `get_metrics_dict()` | - | Return dict/object |
| `get_alerts()` | - | Return list of alerts |
| `clear_alerts()` | - | Clear alert history |
| `export_prometheus()` | - | Export Prometheus format |

### KoreMetrics Fields

| Field | Type | Unit |
|-------|------|------|
| `total_bytes_read` | u64 | bytes |
| `total_bytes_written` | u64 | bytes |
| `read_operations` | u64 | count |
| `write_operations` | u64 | count |
| `avg_read_latency_ms` | f64 | ms |
| `avg_write_latency_ms` | f64 | ms |
| `compression_ratio` | f64 | % |
| `current_memory_bytes` | u64 | bytes |
| `peak_memory_bytes` | u64 | bytes |
| `rows_processed` | u64 | count |
| `columns_processed` | u64 | count |
| `cache_hit_rate` | f64 | 0.0-1.0 |
| `total_errors` | u32 | count |

---

**Version**: 1.2.4  
**Language Support**: Python, JavaScript, Java, Rust  
**Last Updated**: May 28, 2026
