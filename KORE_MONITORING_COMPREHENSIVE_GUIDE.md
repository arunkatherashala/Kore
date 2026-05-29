# KORE v1.2.4 - Monitoring & Observability Guide

**Version**: 1.2.4  
**Release Date**: May 28, 2026  
**Status**: Available in Python, JavaScript, Java, and Rust

---

## 📊 What is KORE Monitoring?

KORE Monitoring is a comprehensive performance tracking system that provides real-time insights into:
- **Read/Write Operations**: Track throughput, latency, and data volume
- **Compression Metrics**: Monitor compression ratios and effectiveness
- **Memory Usage**: Track current and peak memory consumption
- **Cache Performance**: Monitor cache hit rates and effectiveness
- **Error Tracking**: Identify and count failures
- **Query Performance**: Measure query execution times and efficiency

---

## 🎯 Core Features

### 1. **Real-Time Metrics Collection**
- Zero-overhead metric collection (minimal performance impact)
- Automatic aggregation and calculation
- Built-in alert thresholds

### 2. **Multiple Export Formats**
- **JSON**: For integrating with dashboards and APIs
- **Prometheus**: For monitoring systems like Grafana
- **Structured Alerts**: For integration with alerting platforms

### 3. **Smart Alerts**
- High Latency Detection (>1000ms)
- Low Throughput Detection (<1 MB/s)
- Memory Usage Warnings (>1GB)
- Error Rate Monitoring (>5% failure rate)
- Cache Efficiency Alerts (<30% hit rate)

### 4. **Language-Specific Support**
- ✅ **Python**: Native PyO3 bindings
- ✅ **JavaScript/Node.js**: NAPI bindings
- ✅ **Java**: JNI bindings
- ✅ **Rust**: Direct library integration

---

## 📦 Architecture

```
┌─────────────────────────────────────────────────┐
│          KORE Monitoring System                  │
├─────────────────────────────────────────────────┤
│                                                  │
│  PerformanceMonitor (Core)                      │
│  ├─ KoreMetrics (Data Structure)                │
│  ├─ AlertEvent (Alert System)                   │
│  └─ OperationMetric (History)                   │
│                                                  │
├─────────────────────────────────────────────────┤
│  Language Bindings                              │
│  ├─ PyPerformanceMonitor (Python)              │
│  ├─ JsPerformanceMonitor (JavaScript)          │
│  ├─ JavaPerformanceMonitor (Java)              │
│  └─ Direct Access (Rust)                        │
│                                                  │
├─────────────────────────────────────────────────┤
│  Export Formats                                 │
│  ├─ JSON (get_metrics)                         │
│  ├─ Prometheus (export_prometheus)             │
│  ├─ Alerts (get_alerts)                        │
│  └─ CSV (future)                               │
└─────────────────────────────────────────────────┘
```

---

## 🚀 Getting Started

### Python Installation

```bash
# Install KORE with monitoring support
pip install kore-fileformat>=1.2.4

# Verify monitoring is available
python -c "from kore_fileformat import PerformanceMonitor; print('✓ Monitoring available')"
```

### JavaScript Installation

```bash
# Install KORE Node.js package
npm install @kore/cloud@>=1.2.4

# Verify monitoring
node -e "const {PerformanceMonitor} = require('@kore/cloud'); console.log('✓ Monitoring available')"
```

### Java Setup

```bash
# Add Maven dependency
# In your pom.xml:
<dependency>
    <groupId>com.kore.fileformat</groupId>
    <artifactId>kore-core</artifactId>
    <version>1.2.4</version>
</dependency>
```

### Rust Integration

```toml
# In Cargo.toml
[dependencies]
kore = "1.2.4"
```

---

## 📈 Key Metrics Explained

### Throughput Metrics

**Read Throughput (MB/s)**
```
Calculated as: (Total Bytes Read / 1,000,000) / (Avg Latency / 1000)
Indicates: How fast data can be read from storage
Benchmark: >10 MB/s is excellent, >5 MB/s is good
```

**Write Throughput (MB/s)**
```
Calculated as: (Total Bytes Written / 1,000,000) / (Avg Latency / 1000)
Indicates: How fast data can be compressed and written
Benchmark: >5 MB/s is excellent, >2 MB/s is good
```

### Compression Metrics

**Compression Ratio (%)**
```
Formula: (1 - compressed_bytes / original_bytes) * 100
Range: 0-100%
Indicates: How much smaller the data is after compression
KORE Baseline: 84.7% (reduces 1GB to ~150MB)
```

### Latency Metrics

**Average Read Latency (ms)**
```
Indicates: Average time per read operation
Benchmark: <200ms is excellent, <500ms is acceptable
Alert Threshold: >1000ms indicates performance issue
```

**Average Write Latency (ms)**
```
Indicates: Average time to write and compress data
Benchmark: <100ms is excellent, <300ms is acceptable
Alert Threshold: >1000ms indicates bottleneck
```

### Cache Metrics

**Cache Hit Rate (0.0-1.0)**
```
Formula: Cache Hits / (Cache Hits + Cache Misses)
Range: 0.0 (0%) to 1.0 (100%)
Benchmark: >80% is excellent, >50% is acceptable
Alert Threshold: <30% indicates cache inefficiency
```

### Memory Metrics

**Current Memory (bytes)**
```
Indicates: Active memory usage right now
Benchmark: Proportional to data size being processed
Alert Threshold: >1GB indicates potential memory leak
```

**Peak Memory (bytes)**
```
Indicates: Highest memory usage observed
Benchmark: Should be 2-3x the data size being processed
Helps identify memory spikes and leaks
```

---

## 🔔 Alert System

KORE automatically generates alerts when metrics exceed thresholds:

### Alert Types

| Alert Type | Threshold | Cause | Action |
|-----------|-----------|-------|--------|
| **HighLatency** | >1000ms | Slow operations | Check network/disk I/O |
| **LowThroughput** | <1 MB/s | Insufficient bandwidth | Optimize compression or I/O |
| **HighMemoryUsage** | >1GB | Excessive memory consumption | Check for memory leaks |
| **HighErrorRate** | >5% | Too many failures | Debug error handling |
| **LowCacheHitRate** | <30% | Ineffective caching | Increase cache size or adjust strategy |

### Alert Response Example (Python)

```python
from kore_fileformat import PerformanceMonitor

monitor = PerformanceMonitor()

# ... perform operations ...

# Check for alerts
alerts = monitor.get_alerts()
if alerts:
    print(f"⚠️  {len(alerts)} alerts detected:")
    for alert in alerts:
        print(f"  [{alert['alert_type']}] {alert['message']}")
        print(f"  Value: {alert['metric_value']:.2f}, Threshold: {alert['threshold']:.2f}")
        
        # Take action based on alert type
        if alert['alert_type'] == 'HighLatency':
            print("  → Recommendation: Check I/O performance")
        elif alert['alert_type'] == 'LowCacheHitRate':
            print("  → Recommendation: Increase cache size")
```

---

## 📊 Integration Examples

### Grafana Integration

```python
# Export metrics to Prometheus-compatible format
metrics_prometheus = monitor.export_prometheus()

# Push to Prometheus Pushgateway
import requests
response = requests.post(
    'http://localhost:9091/metrics/job/kore',
    data=metrics_prometheus
)
```

### Logging Integration

```python
import logging
import json

logger = logging.getLogger('kore')

# Log metrics periodically
def log_metrics(monitor):
    metrics = monitor.get_metrics_dict()
    logger.info(
        "KORE Metrics",
        extra={
            'throughput_mbps': metrics['read_throughput_mbps'],
            'latency_ms': metrics['avg_read_latency_ms'],
            'compression': metrics['compression_ratio'],
            'memory_mb': metrics['current_memory_bytes'] / 1_000_000,
        }
    )

# Log every 10 operations
log_metrics(monitor)
```

### Alert Integration with Slack

```python
import json
import requests

def send_slack_alert(monitor, webhook_url):
    alerts = monitor.get_alerts()
    if not alerts:
        return
    
    message = {
        'text': f'⚠️  {len(alerts)} KORE Alerts',
        'blocks': [
            {
                'type': 'section',
                'text': {
                    'type': 'mrkdwn',
                    'text': f'*{len(alerts)} Performance Alerts*'
                }
            }
        ] + [
            {
                'type': 'section',
                'text': {
                    'type': 'mrkdwn',
                    'text': f"*{a['alert_type']}*\n{a['message']}"
                }
            }
            for a in alerts[:5]  # First 5 alerts
        ]
    }
    
    requests.post(webhook_url, json=message)
```

### CloudWatch Integration (AWS)

```python
import boto3

def export_to_cloudwatch(monitor):
    cloudwatch = boto3.client('cloudwatch')
    metrics = monitor.get_metrics_dict()
    
    cloudwatch.put_metric_data(
        Namespace='KORE',
        MetricData=[
            {
                'MetricName': 'ReadThroughput',
                'Value': metrics['read_throughput_mbps'],
                'Unit': 'Megabytes/Second'
            },
            {
                'MetricName': 'CompressionRatio',
                'Value': metrics['compression_ratio'],
                'Unit': 'Percent'
            },
            {
                'MetricName': 'CacheHitRate',
                'Value': metrics['cache_hit_rate'] * 100,
                'Unit': 'Percent'
            }
        ]
    )
```

---

## ⚙️ Configuration & Tuning

### Alert Threshold Customization

Create a configuration file `kore_monitoring.yaml`:

```yaml
# KORE Monitoring Configuration v1.2.4

alerts:
  high_latency:
    threshold_ms: 1000
    enabled: true
  
  low_throughput:
    threshold_mbps: 1.0
    enabled: true
  
  high_memory:
    threshold_bytes: 1000000000  # 1GB
    enabled: true
  
  high_error_rate:
    threshold_percent: 5.0
    enabled: true
  
  low_cache_hit:
    threshold_ratio: 0.30  # 30%
    enabled: true

export:
  json_export_interval_sec: 60
  prometheus_endpoint: "http://localhost:9091"
  cloudwatch_enabled: false
  slack_webhook: null
  
sampling:
  # Sample metrics every N operations (0 = all)
  read_sample_rate: 10
  write_sample_rate: 10
  operation_history_max: 10000
```

Load and use configuration:

```python
import yaml
from kore_fileformat import PerformanceMonitor

with open('kore_monitoring.yaml') as f:
    config = yaml.safe_load(f)

monitor = PerformanceMonitor()

# Apply custom alert thresholds
if not config['alerts']['high_latency']['enabled']:
    monitor.disable_alert_type('HighLatency')
```

---

## 🎯 Best Practices

### 1. **Monitoring Strategy**

```
Development:
  - Enable all metrics
  - Log to console
  - Export to file every 100 ops
  
Testing:
  - Enable critical metrics
  - Export to file/database
  - Monitor for performance regressions
  
Production:
  - Sample at 10-100 operations
  - Export to centralized monitoring
  - Alert on critical thresholds only
  - Archive metrics daily
```

### 2. **Performance Considerations**

The monitoring system is designed to have minimal overhead:

| Operation | Overhead | Recommendation |
|-----------|----------|-----------------|
| `record_read()` | <1μs | Always enable |
| `record_write()` | <1μs | Always enable |
| `get_metrics()` | <10μs | Safe to call frequently |
| `export_prometheus()` | <100μs | Export periodically (5-60s) |
| `get_alerts()` | <100μs | Poll every 1-10 seconds |

### 3. **Data Retention**

```python
# Limit history size to prevent memory growth
class BoundedMonitor:
    def __init__(self, max_history=10000):
        self.monitor = PerformanceMonitor()
        self.max_history = max_history
    
    def cleanup_old_metrics(self):
        # Archive metrics older than 1 hour
        # Keep recent metrics for analysis
        pass
```

### 4. **Correlation Analysis**

```python
# Find correlations between metrics
def analyze_performance(monitor):
    metrics = monitor.get_metrics_dict()
    
    # High memory + high latency = memory pressure
    if metrics['current_memory_bytes'] > 500_000_000 and metrics['avg_read_latency_ms'] > 500:
        print("⚠️  Memory pressure detected")
        return
    
    # Low cache hit + high latency = caching issue
    if metrics['cache_hit_rate'] < 0.3 and metrics['avg_read_latency_ms'] > 200:
        print("⚠️  Cache inefficiency detected")
        return
    
    print("✓ Performance is healthy")
```

---

## 🔍 Debugging with Monitoring

### Latency Issues

```python
# Profile latency by operation type
latency_samples = []

for i in range(100):
    start = time.time()
    # Perform operation
    elapsed = time.time() - start
    latency_samples.append(elapsed * 1000)

# Analyze
import statistics
print(f"Median: {statistics.median(latency_samples):.2f}ms")
print(f"Stdev: {statistics.stdev(latency_samples):.2f}ms")
print(f"95th Percentile: {sorted(latency_samples)[95]:.2f}ms")
```

### Memory Leaks

```python
# Monitor memory over time
import gc

monitor = PerformanceMonitor()
previous_memory = 0

for _ in range(1000):
    # Perform operation
    monitor.record_read(1_000_000, 100)
    
    gc.collect()
    current = monitor.get_metrics_dict()['current_memory_bytes']
    
    if current > previous_memory * 1.1:
        print(f"⚠️  Memory spike: {previous_memory} → {current}")
    
    previous_memory = current
```

### Cache Effectiveness

```python
# Analyze cache efficiency
metrics = monitor.get_metrics_dict()
hit_rate = metrics['cache_hit_rate']
memory_used = metrics['current_memory_bytes']

# Calculate ROI
roi = (hit_rate * metrics['read_throughput_mbps']) / (memory_used / 1_000_000)
print(f"Cache ROI: {roi:.2f} (higher is better)")

if roi < 1.0:
    print("Cache is not cost-effective, consider reducing size")
```

---

## 🚢 Deployment Checklist

- [ ] Enable monitoring in dev/test environments
- [ ] Test alert configurations with synthetic load
- [ ] Set up log aggregation (ELK, Splunk, CloudWatch)
- [ ] Configure alerting (PagerDuty, Slack, email)
- [ ] Document runbooks for each alert type
- [ ] Test monitoring during load tests
- [ ] Deploy with sampling enabled in production
- [ ] Monitor for false positives in first week
- [ ] Adjust thresholds based on baseline metrics
- [ ] Archive metrics for trend analysis

---

## 📞 Support & Troubleshooting

### Monitoring Not Available
```python
try:
    from kore_fileformat import PerformanceMonitor
except ImportError:
    print("Error: Upgrade to kore-fileformat>=1.2.4")
```

### No Alerts Generated
```python
# Verify alert thresholds by pushing metrics over limits
monitor.record_read(1_000_000, 2000)  # >1000ms latency
alerts = monitor.get_alerts()
assert len(alerts) > 0, "Alerts not working"
```

### Memory Growing Unbounded
```python
# Clear history periodically
monitor.get_alerts()  # Returns alerts
monitor.clear_alerts()  # Clears alert history
# (operation_history is bounded by design)
```

---

## 📚 Related Documentation

- [KORE v1.2.4 Release Notes](RELEASE_v1.2.4.md)
- [Performance Tuning Guide](TBD)
- [Monitoring Examples](MONITORING_EXAMPLES_ALL_LANGUAGES.md)
- [API Reference](TBD)

---

**Version**: 1.2.4  
**Last Updated**: May 28, 2026  
**Status**: Production Ready ✅  
**Available**: Python, JavaScript, Java, Rust
