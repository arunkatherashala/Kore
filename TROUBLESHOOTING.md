# KORE Troubleshooting Guide

## Version 0.4.0

### Common Issues and Solutions

---

## Query Execution

### "Query Parse Error"

**Symptom**: Error message like "Unexpected token: xyz"

**Causes**:
1. Invalid SQL syntax
2. Unsupported keywords
3. Mismatched quotes or parentheses

**Solutions**:
- Check query against [Query Syntax Guide](QUERY_SYNTAX.md)
- Verify column and table names exist
- Ensure WHERE clause uses supported operators (=, >, <, >=, <=, !=)
- Check quotes are balanced

**Example**:
```sql
-- ❌ WRONG: Missing closing quote
SELECT * FROM users WHERE name = 'John

-- ✅ CORRECT: Balanced quotes
SELECT * FROM users WHERE name = 'John'
```

---

### "Column Not Found"

**Symptom**: "Column 'xyz' not found in table"

**Causes**:
1. Typo in column name
2. Case sensitivity mismatch
3. Wrong table name
4. Column doesn't exist in data

**Solutions**:
- Verify column name spelling
- Check column case sensitivity
- Verify table name
- Inspect data file metadata

**Example**:
```sql
-- ❌ WRONG: Case mismatch
SELECT User_ID FROM users WHERE status = 1

-- ✅ CORRECT: Matching case
SELECT user_id FROM users WHERE status = 1
```

---

### "Filter Not Applied"

**Symptom**: Query returns all rows despite WHERE clause

**Causes**:
1. Filter uses unsupported data type
2. Comparison operator incorrect
3. Index not being used for optimization

**Solutions**:
- Ensure filter value type matches column type
- Use correct comparison operator
- Create index for filtered column: `CREATE INDEX idx_status ON users(status)`
- Check query plan in logs

**Example**:
```sql
-- ❌ WRONG: String vs numeric comparison
SELECT * FROM users WHERE age = '25'

-- ✅ CORRECT: Type matching
SELECT * FROM users WHERE age = 25
```

---

### "JOIN Returns No Results"

**Symptom**: JOIN query returns empty result set

**Causes**:
1. Join condition mismatch
2. No matching rows in both tables
3. Incorrect qualified column syntax
4. Wrong JOIN type

**Solutions**:
- Verify join condition column names and values
- Test each table separately with filters
- Use INNER JOIN to find guaranteed matches
- Use LEFT JOIN to see non-matching rows

**Example**:
```sql
-- ❌ WRONG: Incorrect qualified syntax
SELECT * FROM users u
INNER JOIN orders o ON u.user_id = o.user_id

-- ✅ CORRECT: Proper qualified syntax  
SELECT * FROM users u
INNER JOIN orders o ON u.id = o.user_id
```

---

## Performance Issues

### "Query Timeout"

**Symptom**: Query takes >30 seconds (exceeds default timeout)

**Causes**:
1. No index on filtered column
2. Very large table without parallelization
3. Complex JOIN operation
4. Memory exhaustion

**Solutions**:
1. Create index:
```sql
CREATE INDEX idx_status ON users(status)
```

2. Enable parallelization:
```rust
let config = ParallelConfig::new();
config.worker_threads = 4;
```

3. Optimize query:
```sql
-- ❌ SLOW: Large scan then filter
SELECT * FROM orders
WHERE amount > 1000 AND user_id = 123

-- ✅ FAST: Filter early
SELECT * FROM orders
WHERE user_id = 123 AND amount > 1000
```

4. Increase timeout:
```bash
KORE_REQUEST_TIMEOUT=60 cargo run
```

---

### "Memory Usage Too High"

**Symptom**: Memory > configured limit or process killed

**Causes**:
1. Large result set
2. Memory pooling disabled
3. No LIMIT on query
4. Inefficient JOIN algorithm

**Solutions**:
1. Enable memory pooling:
```rust
let config = PoolConfig {
    enable_reuse: true,
    buffer_pool_size: 100,
    ..
};
```

2. Add LIMIT:
```sql
SELECT * FROM large_table LIMIT 1000
```

3. Use pagination:
```sql
SELECT * FROM events ORDER BY id LIMIT 100 OFFSET 0
SELECT * FROM events ORDER BY id LIMIT 100 OFFSET 100
```

4. Check JOIN algorithm:
```rust
let optimizer = JoinOptimizer::new();
let cost = optimizer.calculate_cost(&left_table, &right_table, selectivity);
```

---

### "Low Throughput"

**Symptom**: Fewer queries/sec than expected

**Causes**:
1. Parallelization disabled
2. Small worker thread count
3. Inefficient query plan
4. Memory pool exhaustion

**Solutions**:
1. Increase worker threads:
```bash
KORE_WORKER_THREADS=8 cargo run
```

2. Optimize buffer pool:
```rust
PoolConfig {
    buffer_pool_size: 200,
    buffer_size: 16384,
    ..
}
```

3. Profile query:
```rust
let mut profiler = PerformanceProfiler::new();
profiler.record("query_name", duration_ms);
```

4. Check baseline:
```rust
let comparison = baseline_tracker.get_comparison("query");
println!("{:?}", comparison.improvement_percent);
```

---

## Deployment Issues

### "Service Won't Start"

**Symptom**: Docker container exits immediately

**Causes**:
1. Port already in use
2. Missing configuration
3. Invalid environment variables
4. Build errors

**Solutions**:
1. Check port:
```bash
lsof -i :8080  # Find what's using port
docker-compose down  # Stop existing containers
```

2. Verify configuration:
```bash
docker logs kore-query-engine
```

3. Check environment:
```bash
docker-compose logs -f
```

4. Rebuild image:
```bash
docker-compose build --no-cache kore-query-engine
```

---

### "Health Check Failing"

**Symptom**: Container marked unhealthy by Docker

**Causes**:
1. High error rate (>10%)
2. Memory > 4GB
3. Service response slow
4. Endpoint not responding

**Solutions**:
1. Check error logs:
```bash
docker logs kore-query-engine | grep ERROR
```

2. Verify memory usage:
```bash
docker stats
```

3. Test health endpoint:
```bash
curl http://localhost:8080/health
```

4. Increase health check timeout:
```yaml
# docker-compose.yml
healthcheck:
  timeout: 10s  # Increase from 5s
```

---

### "Connection Refused"

**Symptom**: "Connection refused" or "Host unreachable"

**Causes**:
1. Service not running
2. Wrong port/host
3. Firewall blocking
4. Network isolation

**Solutions**:
1. Verify service running:
```bash
docker ps | grep kore
curl http://localhost:8080/health
```

2. Check configuration:
```bash
env | grep KORE
```

3. Test network:
```bash
docker network ls
docker network inspect kore-network
```

4. Update firewall:
```bash
# Linux
sudo ufw allow 8080

# Windows
netsh advfirewall firewall add rule name="KORE" dir=in action=allow protocol=tcp localport=8080
```

---

## Data Issues

### "File Not Found"

**Symptom**: "Error: No such file or directory: data.kore"

**Causes**:
1. Wrong file path
2. File not in expected location
3. Permission denied
4. Case sensitivity mismatch

**Solutions**:
1. Verify file exists:
```bash
ls -la /path/to/data.kore
```

2. Check permissions:
```bash
chmod 644 /path/to/data.kore
```

3. Use absolute path:
```rust
// ❌ WRONG: Relative path
let file = "data.kore";

// ✅ CORRECT: Absolute path
let file = "/app/data/data.kore";
```

---

### "Corrupted Data File"

**Symptom**: "Invalid KORE magic header" or "Decompression failed"

**Causes**:
1. File not actually KORE format
2. File corrupted or incomplete
3. Version mismatch
4. Incomplete write/upload

**Solutions**:
1. Verify file integrity:
```bash
cargo run --release -- validate-kore data.kore
```

2. Check file type:
```bash
file data.kore
hexdump -C data.kore | head
```

3. Restore from backup:
```bash
tar -xzf kore_backup_latest.tar.gz
```

4. Re-import data:
```bash
cargo run --release -- import-csv data.csv data.kore
```

---

## Monitoring Issues

### "Prometheus Not Scraping"

**Symptom**: No metrics appearing in Prometheus

**Causes**:
1. Service not exposing metrics endpoint
2. Prometheus configuration wrong
3. Network connectivity issue
4. Wrong IP/port

**Solutions**:
1. Verify metrics endpoint:
```bash
curl http://localhost:8080/metrics
```

2. Check Prometheus config:
```yaml
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: 'kore'
    static_configs:
      - targets: ['localhost:8080']
```

3. Check container networking:
```bash
docker exec kore-query-engine curl http://prometheus:9090/
```

---

### "Grafana Dashboards Not Loading"

**Symptom**: Empty graphs or "No data"

**Causes**:
1. Data source not configured
2. Prometheus not connected
3. Metrics not being collected
4. Time range issue

**Solutions**:
1. Add Prometheus data source:
   - Home → Connections → Data Sources
   - Add Prometheus at `http://prometheus:9090`
   - Test connection

2. Create dashboard:
   - Click "+" → Dashboard
   - Add panels with metric queries:
   ```
   kore_queries_total
   rate(kore_query_duration_ms[5m])
   ```

3. Check time range:
   - Ensure time range includes data collection
   - Default is last 6 hours

---

## Development Issues

### "Tests Failing"

**Symptom**: Some tests fail after changes

**Causes**:
1. Test data incorrect
2. Borrow checker issues
3. Numerical precision
4. Thread safety issue

**Solutions**:
1. Run single test:
```bash
cargo test deployment::tests::test_health_check -- --nocapture
```

2. Run with backtrace:
```bash
RUST_BACKTRACE=1 cargo test
```

3. Check thread safety:
```rust
// Use Arc<Mutex<T>> for shared state
let data = Arc::new(Mutex::new(HashMap::new()));
```

---

### "Compilation Errors"

**Symptom**: "error[E0382]: use of moved value" or similar

**Causes**:
1. Ownership violation
2. Lifetime issues
3. Type mismatch
4. Missing trait implementation

**Solutions**:
1. Clone when needed:
```rust
let patterns = self.config.patterns.clone();
for pattern in patterns {
    // ...
}
```

2. Use references:
```rust
fn process(&self, query: &Query) { }
```

3. Add trait bound:
```rust
fn new<T: Clone + Debug>() { }
```

---

## Getting Help

If you can't find a solution:

1. **Check Documentation**:
   - [Query Syntax Guide](QUERY_SYNTAX.md)
   - [Architecture Guide](ARCHITECTURE.md)
   - [Deployment Guide](DEPLOYMENT.md)

2. **Enable Debug Logging**:
```bash
RUST_LOG=debug cargo run
```

3. **Collect Logs**:
```bash
docker logs kore-query-engine > error.log
docker compose logs > full.log
```

4. **Check Status**:
```bash
curl http://localhost:8080/health | jq
```

5. **Report Issue**:
Include:
- Error message (full text)
- Query that caused issue
- System specs (CPU, memory, OS)
- Steps to reproduce
- Logs (health check output, Docker logs)

---

## Performance Tuning Checklist

- [ ] Indexes created on filtered columns
- [ ] Parallelization enabled with appropriate worker threads
- [ ] Memory pooling enabled with adequate buffer sizes
- [ ] Query results limited (LIMIT clause)
- [ ] Complex JOINs optimized (algorithm selection)
- [ ] Health checks passing
- [ ] Memory usage within limits
- [ ] Error rate < 5%
- [ ] Throughput meeting SLA requirements
- [ ] Monitoring dashboards showing expected metrics
