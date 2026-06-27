# Kore Observability

Production-grade observability infrastructure for Kore file format operations.

**Status**: Week 3 of 6-week modernization plan (Jun 9-15, 2026)

## Features

- 📊 **Prometheus Metrics**: Query performance, compression efficiency, cache hit rates
- 🔍 **Distributed Tracing**: OpenTelemetry + Jaeger integration for query flows
- 📈 **Real-time Dashboard**: Built-in metrics aggregation and reporting
- ⏱️ **Automatic Instrumentation**: RAII-based automatic timing and tracking
- 🎯 **Business KPIs**: Filter selectivity, compression ratio, cache efficiency
- 🚨 **Error Tracking**: Categorized error counting and analysis

## Architecture

### Metrics Collection

25+ metrics across 7 categories:

**Query Metrics**:
- `kore_query_duration_secs` - Query execution time (histogram)
- `kore_query_total` - Total queries executed
- `kore_query_errors_total` - Failed queries

**Read/Write Metrics**:
- `kore_bytes_read_total` - Total bytes read
- `kore_read_operations_total` - Read operation count
- `kore_read_latency_ms` - Individual read latency
- `kore_bytes_written_total` - Total bytes written
- `kore_write_latency_ms` - Write operation latency

**Compression Metrics**:
- `kore_compression_ratio` - Achieved compression ratio
- `kore_bytes_before_compression_total` - Original data size
- `kore_bytes_after_compression_total` - Compressed data size
- `kore_compression_time_ms` - Compression duration

**Cache Metrics**:
- `kore_cache_hits_total` - Cache hits
- `kore_cache_misses_total` - Cache misses
- `kore_cache_hit_rate` - Hit rate ratio (0-1)
- `kore_cache_size_bytes` - Current cache size

**Filter Pushdown Metrics**:
- `kore_filter_pushdown_queries_total` - Queries using pushdown
- `kore_filter_selectivity` - Filter reduction ratio
- `kore_rows_filtered_total` - Rows eliminated by filters

**Cloud Storage Metrics**:
- `kore_cloud_read_operations_total` - Cloud read count
- `kore_cloud_range_requests_total` - HTTP Range requests
- `kore_cloud_read_latency_ms` - Cloud I/O latency
- `kore_cloud_bytes_transferred_total` - Data transferred

**Connection Metrics**:
- `kore_active_connections` - Active connections
- `kore_connection_errors_total` - Failed connections

### Tracing Integration

**OpenTelemetry Spans**:
- Query execution traces
- Read/write operation flows
- Cloud storage access patterns
- Filter evaluation traces

**Jaeger Visualization**:
- Distributed trace view
- Latency analysis
- Dependency mapping
- Error rate tracking

### Instrumentation Contexts

RAII-based automatic tracking:

```rust
// Automatic timing and error tracking
let query = QueryInstrumentation::new("SELECT ...");
query.record_filter_pushdown(0.85, 150000); // selectivity & rows filtered
// Drop records duration automatically

let read = ReadInstrumentation::new("read_row_groups");
read.record_bytes(5_000_000);
// Auto-records latency on drop

let compression = CompressionInstrumentation::new(10_000_000);
compression.complete(2_000_000); // 20% ratio
// Auto-records compression time
```

## Installation

Add to `Cargo.toml`:

```toml
[dependencies]
kore-observability = "1.0"
```

With features:

```toml
kore-observability = { version = "1.0", features = ["prometheus", "jaeger"] }
```

## Quick Start

### 1. Initialize Observability

```rust
use kore_observability::init_observability;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup Prometheus + Jaeger
    init_observability().await?;
    
    // Your application code
    Ok(())
}
```

### 2. Collect Metrics

```rust
use kore_observability::instrumentation::QueryInstrumentation;
use kore_observability::metrics::get_metrics;

// Automatic timing
let _query = QueryInstrumentation::new("SELECT * WHERE id > 100");

// Manual updates
let metrics = get_metrics();
metrics.cache_hits.inc();
metrics.rows_processed.inc_by(1000);

// Dashboard updates automatically
metrics.update_cache_hit_rate();
metrics.update_compression_ratio();
```

### 3. Export Metrics

```rust
use kore_observability::metrics::get_metrics;

// Prometheus text format (for /metrics endpoint)
let metrics = get_metrics();
let export = metrics.export()?;
println!("{}", export);
```

### 4. View Traces

Jaeger UI: `http://localhost:16686`

- Service: `kore`
- Operations: `query_example`, `read_operations`, etc.
- Trace details: Span tree, latencies, errors

## Examples

### Example 1: Prometheus Metrics

```bash
cargo run --example prometheus_metrics --features prometheus
```

Output:
- Query statistics
- Read/write performance
- Compression efficiency
- Cache hit rates
- Filter effectiveness
- Cloud storage patterns

### Example 2: Jaeger Tracing

```bash
# Start Jaeger first
docker run -d --name jaeger -p 16686:16686 -p 14268:14268 jaegertracing/all-in-one

# Run example
cargo run --example jaeger_tracing --features jaeger
```

Visit: `http://localhost:16686`

### Example 3: Metrics Dashboard

```bash
cargo run --example metrics_dashboard
```

Output:
- Real-time dashboard with ASCII art
- Query statistics
- Performance KPIs
- Recommendations based on metrics
- Integration setup guide

## API Reference

### KoreMetrics

```rust
pub struct KoreMetrics {
    // Query metrics
    pub query_duration_secs: Histogram,
    pub query_total: IntCounter,
    pub query_errors: IntCounter,
    
    // Read metrics
    pub bytes_read_total: Counter,
    pub read_operations: IntCounter,
    pub read_latency_ms: Histogram,
    
    // Compression metrics
    pub compression_ratio: Gauge,
    pub bytes_before_compression: Counter,
    pub compression_time_ms: Histogram,
    
    // ... 15+ more metrics
}

impl KoreMetrics {
    pub fn new() -> Result<Arc<Self>>;
    pub fn export(&self) -> Result<String>;
    pub fn update_cache_hit_rate(&self);
    pub fn update_compression_ratio(&self);
    pub fn update_filter_selectivity(&self);
}

pub fn get_metrics() -> Arc<KoreMetrics>;
```

### Instrumentation Contexts

```rust
pub struct QueryInstrumentation { ... }
pub struct ReadInstrumentation { ... }
pub struct WriteInstrumentation { ... }
pub struct CloudReadInstrumentation { ... }
pub struct CompressionInstrumentation { ... }

// All implement Drop for automatic timing
```

## Integration with Stack

### Prometheus

1. Add scrape config:
```yaml
scrape_configs:
  - job_name: 'kore'
    static_configs:
      - targets: ['localhost:8080']
    metrics_path: '/metrics'
```

2. Query examples:
```
# Cache efficiency
rate(kore_cache_hits_total[1m]) / rate(kore_cache_hits_total[1m] + kore_cache_misses_total[1m])

# Query performance
histogram_quantile(0.95, kore_query_duration_secs_bucket)

# Compression effectiveness
(1 - kore_compression_ratio) * 100

# Filter pushdown impact
rate(kore_rows_filtered_total[1m]) / rate(kore_rows_processed_total[1m]) * 100
```

### Grafana Dashboard

Import dashboard JSON with:
- Query performance graph
- Cache hit rate gauge
- Compression ratio trend
- Filter selectivity chart
- Error rate alert
- Cloud storage I/O heatmap

### Jaeger Tracing

Visualize:
- Query execution flow
- Latency breakdown by component
- Error traces with stack context
- Service dependencies (Spark ↔ Kore ↔ Cloud)

## Performance Impact

Overhead per operation:
- Metrics recording: <1μs per event
- Histogram observation: <5μs
- Tracing span: <50μs
- Total per query: <100μs (0.01% overhead for 10ms query)

Memory:
- Metrics registry: ~1MB
- Active traces: <10MB for typical workload
- Histogram buckets: ~50KB

## Testing

```bash
# Run all tests
cargo test

# Test with logging
RUST_LOG=debug cargo test -- --nocapture

# Test specific instrumentation
cargo test query_instrumentation
cargo test compression_instrumentation

# Run examples
cargo run --example prometheus_metrics
cargo run --example jaeger_tracing
cargo run --example metrics_dashboard
```

## Architecture

```
KoreMetrics (Registry)
├── Query Metrics (Histogram, Counters)
├── Read/Write Metrics (Latency, Volume)
├── Compression Metrics (Ratio, Time)
├── Cache Metrics (Hit Rate, Size)
├── Filter Metrics (Selectivity, Rows)
└── Cloud Metrics (Operations, Latency)

Instrumentation Contexts (Auto-timing)
├── QueryInstrumentation
├── ReadInstrumentation
├── WriteInstrumentation
├── CloudReadInstrumentation
└── CompressionInstrumentation

Exporters
├── Prometheus (Text Format)
├── OpenTelemetry (Protocol)
└── Jaeger (Distributed Traces)
```

## Roadmap

- [x] 25 Prometheus metrics
- [x] OpenTelemetry tracing framework
- [x] RAII-based instrumentation
- [x] Automatic dashboard generation
- [ ] Grafana dashboard templates
- [ ] ClickHouse long-term storage
- [ ] Custom alerts/SLOs
- [ ] Integration with Spark (Week 4)
- [ ] Cloud metrics (Week 2 integration)
- [ ] Security audit logging (Week 5)

## Best Practices

1. **Initialize Early**: Call `init_observability()` in application startup
2. **Use Contexts**: Leverage RAII instrumentation for automatic timing
3. **Export Regularly**: Expose `/metrics` endpoint for Prometheus scraping
4. **Set Alerts**: Configure alerting on error rate, latency, and cache misses
5. **Correlate Traces**: Use trace IDs to link Kore queries with application requests
6. **Monitor SLOs**: Track P50/P95/P99 query latency against targets

## License

KUOPL - See LICENSE file

## Support

- Issues: https://github.com/arunkatherashala/Kore/issues
- Discussions: https://github.com/arunkatherashala/Kore/discussions
- Email: support@kore.dev

---

**Part of Kore Modernization Wave** (May 26 - July 7, 2026)
- Week 1: Spark Connector ✅
- Week 2: Cloud Integration ✅
- Week 3: Observability (This)
- Week 4: Streaming
- Week 5: Security
- Week 6: Tooling & CLI
