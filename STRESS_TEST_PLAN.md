# KORE v1.8.0 - Stress Testing & Hardening Plan

**Status:** Ready to Execute  
**Duration:** 3-5 days intensive testing  
**Target:** Production hardiness certification

---

## 🧪 Test Suites

### 1. Chaos Engineering Tests

#### Network Partition Tests
```rust
#[test]
fn test_network_partition_recovery() {
    // Scenario: Coordinator loses connection to Worker 2 for 30s
    // Expected: Query completes with remaining workers + auto-retry
    let cluster = start_cluster(4);  // 1 coord + 3 workers
    
    cluster.worker(2).disconnect();  // Simulate network failure
    thread::sleep(Duration::from_secs(30));
    
    let query = "SELECT COUNT(*) FROM large_table GROUP BY region";
    let result = cluster.submit_query(query);  // Should NOT panic
    
    assert!(result.is_ok(), "Should handle network failure gracefully");
    assert_eq!(result.unwrap().rows, 10);  // Correct result
}
```

#### Worker Crash & Recovery
```rust
#[test]
fn test_worker_crash_during_shuffle() {
    // Scenario: Worker crashes mid-shuffle
    // Expected: Coordinator retries failed partitions
    let cluster = start_cluster(4);
    
    let shuffle_task = cluster.submit_query(
        "SELECT * FROM t1 JOIN t2 USING (id) GROUP BY id"
    );
    
    // Kill a worker during shuffle phase
    thread::sleep(Duration::from_millis(100));
    cluster.worker(2).kill();
    
    let result = shuffle_task.wait();
    assert!(result.is_ok(), "Should recover from worker crash");
}
```

#### Coordinator Failover
```rust
#[test]
fn test_coordinator_failover() {
    // Scenario: Primary coordinator crashes
    // Expected: Standby coordinator takes over
    let cluster = start_cluster_with_ha(4);  // 2 coordinators
    
    let query_id = cluster.submit_query("SELECT ... (long running query)");
    
    thread::sleep(Duration::from_secs(2));
    cluster.primary_coordinator().kill();
    
    // Secondary should detect failure and take over
    thread::sleep(Duration::from_millis(500));
    
    let result = cluster.query_status(query_id);
    assert!(result.is_some(), "Query should continue on failover");
}
```

### 2. Memory Stress Tests

#### Large Dataset Processing
```rust
#[test]
fn test_1gb_aggregate_no_overflow() {
    // Generate 1GB of data (100M rows × 10 columns)
    let block = generate_large_datablock(100_000_000);
    
    // Query: GROUP BY on low-cardinality column
    let result = kore.query(&format!(
        "SELECT category, COUNT(*) FROM large GROUP BY category"
    )).unwrap();
    
    // Should complete without OOM or panicking
    assert!(result.rows > 0);
}
```

#### Out-of-Core (Spilling)
```rust
#[test]
fn test_spill_to_disk_on_memory_limit() {
    // Set memory limit to 500MB (but data is 1GB)
    let ctx = KqlContext::with_memory_limit(500 * 1024 * 1024);
    
    let block = generate_large_datablock(100_000_000);
    ctx.register("huge", block);
    
    // Query that requires all data in memory
    let result = ctx.query(
        "SELECT * FROM huge ORDER BY id"  // Full sort
    );
    
    assert!(result.is_ok(), "Should spill to disk gracefully");
}
```

#### Memory Leak Detection
```rust
#[test]
fn test_no_memory_leaks_after_1000_queries() {
    let ctx = KqlContext::new();
    
    for i in 0..1000 {
        let result = ctx.query(&format!(
            "SELECT COUNT(*) FROM table WHERE id = {}",
            i % 100
        ));
        assert!(result.is_ok());
    }
    
    // Check RSS memory doesn't grow unboundedly
    // (In real testing, use valgrind/heaptrack)
}
```

### 3. Concurrent Query Tests

#### Race Condition Detection
```rust
#[test]
fn test_concurrent_writes_and_reads() {
    let ctx = Arc::new(RwLock::new(KqlContext::new()));
    let mut handles = vec![];
    
    // 10 threads writing simultaneously
    for i in 0..10 {
        let ctx_clone = Arc::clone(&ctx);
        handles.push(thread::spawn(move || {
            ctx_clone.write().unwrap().query(&format!(
                "INSERT INTO users VALUES ({}, 'user{}')",
                i, i
            )).unwrap();
        }));
    }
    
    // 10 threads reading simultaneously
    for _ in 0..10 {
        let ctx_clone = Arc::clone(&ctx);
        handles.push(thread::spawn(move || {
            ctx_clone.read().unwrap().query(
                "SELECT COUNT(*) FROM users"
            ).unwrap();
        }));
    }
    
    for handle in handles {
        handle.join().unwrap();
    }
    
    // Verify data consistency
    let count = ctx.read().unwrap().query("SELECT COUNT(*) FROM users")
        .unwrap().rows;
    assert_eq!(count, 10);  // No lost inserts
}
```

#### Deadlock Detection
```rust
#[test]
fn test_no_deadlocks_under_concurrent_joins() {
    let ctx = Arc::new(RwLock::new(KqlContext::new()));
    
    // Create circular join pattern that might deadlock
    ctx.write().unwrap().register("t1", sample_data());
    ctx.write().unwrap().register("t2", sample_data());
    
    let handles: Vec<_> = (0..20)
        .map(|_| {
            let ctx = Arc::clone(&ctx);
            thread::spawn(move || {
                let result = ctx.read().unwrap().query(
                    "SELECT * FROM t1 JOIN t2 ON t1.id = t2.id"
                );
                result.unwrap()
            })
        })
        .collect();
    
    for handle in handles {
        handle.join().unwrap();  // Timeout = deadlock detected
    }
}
```

### 4. Edge Case Tests

#### Boundary Conditions
```rust
#[test]
fn test_integer_overflow_in_sum() {
    let block = DataBlock::new(vec![
        Column::int64("value", vec![
            Some(i64::MAX),
            Some(i64::MAX),
            Some(1),  // This causes overflow
        ]),
    ]).unwrap();
    
    ctx.register("t", block);
    let result = ctx.query("SELECT SUM(value) FROM t");
    
    // Should detect overflow, not silently wrap
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), KoreError::Overflow(_)));
}
```

#### Null Handling
```rust
#[test]
fn test_nulls_in_aggregation() {
    let block = DataBlock::new(vec![
        Column::int64("val", vec![Some(1), None, Some(3), None, Some(5)]),
    ]).unwrap();
    
    ctx.register("t", block);
    let result = ctx.query("SELECT COUNT(*), COUNT(val), SUM(val) FROM t")
        .unwrap();
    
    assert_eq!(result.rows[0], Value::Int(5));      // COUNT(*) = 5
    assert_eq!(result.rows[1], Value::Int(3));      // COUNT(val) = 3 (ignores NULLs)
    assert_eq!(result.rows[2], Value::Int(9));      // SUM(val) = 9 (ignores NULLs)
}
```

#### Empty Dataset
```rust
#[test]
fn test_aggregate_on_empty_table() {
    let block = DataBlock::new(vec![
        Column::int64("id", vec![]),
    ]).unwrap();
    
    ctx.register("empty", block);
    let result = ctx.query("SELECT COUNT(*), SUM(id), AVG(id) FROM empty")
        .unwrap();
    
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.get_value(0, 0), Value::Int(0));    // COUNT(*)
    assert_eq!(result.get_value(0, 1), Value::Null);      // SUM
    assert_eq!(result.get_value(0, 2), Value::Null);      // AVG
}
```

---

## 🧬 Load Testing

### TPC-H Stress at Scale

```bash
# Run TPC-H at multiple scale factors sequentially
for sf in 1 5 10 20; do
    echo "Testing SF-$sf ($(( $sf * 6 ))M rows)..."
    time cargo run --release --bin kore-tpch -- --scale $sf
    
    # Measure:
    # - Peak memory usage
    # - Total time
    # - Per-query latency
done
```

### Concurrent Query Stress

```bash
# Submit 100 concurrent queries, track success rate
python stress_test.py --workers 100 --duration 300 --target-qps 50

# Measure:
# - Success rate (target: 99.9%+)
# - P50/P95/P99 latency
# - Throughput (queries/sec)
# - Error rate
```

---

## 📋 Stress Test Checklist

### Before Release

- [ ] Run all chaos engineering tests (must pass)
- [ ] Run memory stress tests with valgrind (no leaks)
- [ ] Run concurrent query tests (no deadlocks)
- [ ] Run edge case tests (all pass)
- [ ] TPC-H stress at SF-10+ (no crashes)
- [ ] 100+ concurrent queries (99.9% success)
- [ ] Network partition recovery (< 30s)
- [ ] Worker crash recovery (auto-retry works)
- [ ] 24-hour soak test (no memory creep)

### Success Criteria

| Test | Target | Pass/Fail |
|------|--------|-----------|
| Chaos tests | 100% pass | [ ] |
| Memory leaks | 0 (valgrind clean) | [ ] |
| Concurrency | 0 deadlocks | [ ] |
| Edge cases | 100% correct | [ ] |
| TPC-H SF-20 | Completes | [ ] |
| Concurrent load | 99.9% success | [ ] |
| Network recovery | < 30s | [ ] |
| Soak test 24h | No hangs | [ ] |

---

*Hardening Plan Created: 2026-08-29*  
*Estimated Duration: 3-5 days*  
*Status: Ready to Execute*
