# KORE Query Engine - Production Deployment Guide

## Version 0.4.0

### Quick Start

#### Using Docker Compose
```bash
docker-compose up -d
```

This starts:
- **kore-query-engine**: Main service on port 8080
- **Prometheus**: Metrics collection on port 9090
- **Grafana**: Dashboards on port 3000
- **kore-benchmarking**: Performance validation

#### Using Docker
```bash
docker build -f Dockerfile.prod -t kore:0.4.0 .
docker run -p 8080:8080 kore:0.4.0
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `info` | Log level (debug, info, warn, error) |
| `KORE_ENV` | `production` | Environment (development, staging, production) |
| `KORE_PORT` | `8080` | Service port |
| `KORE_HOST` | `0.0.0.0` | Bind address |
| `KORE_MAX_CONNECTIONS` | `1000` | Max concurrent connections |
| `KORE_REQUEST_TIMEOUT` | `30` | Request timeout (seconds) |

### Health Checks

The service provides health check endpoints:

```bash
# Basic health check
curl http://localhost:8080/health

# Returns:
{
  "status": "healthy",
  "uptime_secs": 3600,
  "queries_processed": 1000,
  "errors": 5,
  "memory_mb": 256
}
```

### Performance Tuning

#### 1. Query Parallelization
Enable multi-threaded query execution:
```rust
let config = ParallelConfig {
    worker_threads: 4,
    chunk_size: 10000,
    enable_parallel_joins: true,
};
```

**Expected Improvement**: 3.4x speedup on 4-core systems

#### 2. Memory Pooling
Reduce allocation overhead:
```rust
let pool_config = PoolConfig {
    buffer_pool_size: 100,
    buffer_size: 8192,
    row_pool_size: 10000,
    enable_reuse: true,
};
```

**Expected Improvement**: 15-25% memory reduction

#### 3. Index Creation
Speed up filtered queries:
```rust
index_manager.create_index(
    "users",
    "status",
    IndexType::Hash,
);
```

**Expected Improvement**: 2-3x speedup on indexed columns

### Monitoring

#### Prometheus Metrics
Default Prometheus scrape target: `http://localhost:9090`

Key metrics:
- `kore_queries_total`: Total queries processed
- `kore_query_duration_ms`: Query execution time
- `kore_memory_usage_mb`: Memory consumption
- `kore_parallelization_speedup`: Parallelization benefit

#### Grafana Dashboards
Access at `http://localhost:3000`

Default credentials:
- Username: `admin`
- Password: `admin`

### Scaling

#### Horizontal Scaling
For production with multiple instances:

```yaml
# kubernetes deployment example
replicas: 3
resources:
  requests:
    cpu: 2
    memory: 4Gi
  limits:
    cpu: 4
    memory: 8Gi
```

#### Vertical Scaling
For single instance optimization:
- Increase `worker_threads` for parallelization
- Increase `buffer_pool_size` for memory pooling
- Create indexes on frequently filtered columns

### Troubleshooting

#### High Memory Usage
**Symptom**: Memory usage exceeds configured limit
**Solution**: 
- Enable memory pooling
- Reduce `buffer_pool_size`
- Implement query result pagination

#### Slow Queries
**Symptom**: Queries taking >1 second
**Solution**:
- Check if indexes exist on filtered columns
- Enable query parallelization
- Review query complexity

#### Connection Timeouts
**Symptom**: "Connection timeout" errors
**Solution**:
- Increase `KORE_MAX_CONNECTIONS`
- Check system file descriptor limits
- Add connection pooling

### Database Connections

KORE uses a columnar format (KORE files), not traditional databases. However, you can export to/from:

```bash
# Export to CSV
cargo run --release -- export-csv input.kore output.csv

# Import from CSV
cargo run --release -- import-csv input.csv output.kore

# Export to Parquet
cargo run --release -- export-parquet input.kore output.parquet
```

### Backup & Recovery

#### Creating Backups
```bash
# Full backup
tar -czf kore_backup_$(date +%Y%m%d).tar.gz ./data

# Incremental backup (assumes delta tracking)
rsync -av ./data backup/latest/
```

#### Recovery
```bash
# Restore from backup
tar -xzf kore_backup_20260511.tar.gz -C ./

# Verify data integrity
cargo run --release -- validate-kore ./data/*.kore
```

### Security

#### TLS/HTTPS
For production deployment, use a reverse proxy (nginx, HAProxy):

```nginx
server {
    listen 443 ssl;
    server_name kore.example.com;
    
    ssl_certificate /etc/ssl/certs/kore.crt;
    ssl_certificate_key /etc/ssl/private/kore.key;
    
    location / {
        proxy_pass http://localhost:8080;
    }
}
```

#### Authentication
Implement API key validation:
```rust
if !request.headers().contains_key("X-API-Key") {
    return Err("Missing API key");
}
```

### Updates & Maintenance

#### Version Upgrades
```bash
# Check current version
kore --version

# Upgrade process
1. Backup data
2. Stop service
3. Pull new version
4. Run migration (if needed)
5. Restart service
6. Verify health checks
```

#### Regular Maintenance
- Daily: Monitor error rates and latency
- Weekly: Review slow query logs
- Monthly: Analyze index usage, rebuild if needed
- Quarterly: Plan capacity upgrades

### Support & Issues

For issues or questions:
1. Check [Troubleshooting Guide](TROUBLESHOOTING.md)
2. Review [Architecture Documentation](ARCHITECTURE.md)
3. Check [Query Syntax Guide](QUERY_SYNTAX.md)
4. Open GitHub issue with:
   - Error message
   - Query example
   - System specs
   - Reproducible steps

### Next Steps

1. **Configure Monitoring**: Set up Grafana dashboards
2. **Enable Security**: Configure TLS and authentication
3. **Plan Scaling**: Determine horizontal/vertical scaling needs
4. **Schedule Backups**: Implement automated backup strategy
