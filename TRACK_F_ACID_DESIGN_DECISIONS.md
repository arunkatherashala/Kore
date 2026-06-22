# TRACK F: ACID TRANSACTIONS - DESIGN DECISIONS
**KORE v1.3.0 - Implementation Architecture**

---

## 📋 EXECUTIVE SUMMARY

KORE will implement full ACID transaction support with:
- **Write-Ahead Log (WAL)** for durability
- **MVCC Snapshot Isolation** for concurrency
- **Time-travel queries** for temporal analytics
- **5000 transactions/sec** throughput target

**Timeline**: 6 weeks (Jul 15 - Aug 31)
**Team Size**: 3 engineers (1 lead + 2 support)
**LOC Target**: 8,000 lines Rust

---

## 1. TRANSACTION LOG ARCHITECTURE

### Design Decision: Sequential WAL vs Index-based Log

**CHOSEN: Sequential Write-Ahead Log (Simple & Fast)**

```
Rationale:
  • Sequential writes = maximum disk throughput
  • No index structure = minimal overhead
  • Append-only = perfect for SSDs
  • Recovery = simple sequential read + replay
```

### WAL Entry Format

```
Entry Structure (variable-length record):
  [8 bytes] Timestamp           (u64, milliseconds since epoch)
  [8 bytes] Transaction ID      (u64, monotonic counter)
  [1 byte]  Operation Type      (u8: WRITE=1, DELETE=2, COMMIT=3, ABORT=4)
  [4 bytes] Partition ID        (u32, which partition affected)
  [4 bytes] Column ID           (u32, which column modified)
  [8 bytes] Min Value           (i64 or f64, depends on type)
  [8 bytes] Max Value           (i64 or f64)
  [4 bytes] Payload Size        (u32, row data)
  [N bytes] Compressed Payload  (row data, zstd compressed)
  [8 bytes] CRC-64 Checksum     (for corruption detection)

Total: 53 + N bytes per entry (typically N=100-500 bytes)
```

### File Organization

```
One WAL file per partition:
  kore_data/
    ├─ partition_0.wal    (current)
    ├─ partition_0-1.wal  (rotated archive)
    ├─ partition_0-2.wal  (rotated archive)
    ├─ partition_1.wal    (current)
    └─ partition_1-1.wal  (rotated archive)

Rotation Policy:
  • Max size: 1 GB per WAL file
  • Time-based: Rotate every 1 hour
  • Action: Compress to .wal.zstd and archive
  • Retention: Keep 7 days of history
```

### Performance Targets
```
Single Write:     < 100 μs latency (log append + fsync)
Batch Write:      > 1 million entries/sec (buffered)
Read (replay):    > 10 million entries/sec (sequential scan)
Compression:      4:1 ratio (e.g., 1 GB → 250 MB)
```

---

## 2. SNAPSHOT MANAGEMENT

### Design Decision: Immutable Snapshots vs Delta Snapshots

**CHOSEN: Immutable Snapshots (Simple Isolation)**

```
Rationale:
  • Immutable = trivial conflict detection
  • No delta chains = fast reads
  • Storage: Compressed blocks already reuse existing codec
  • Trade-off: More storage for simplicity
```

### Snapshot Metadata Format

```
Snapshot Manifest (.manifest file):
  [8 bytes]  Snapshot ID        (u64, unique identifier)
  [8 bytes]  Transaction ID     (u64, which transaction created)
  [8 bytes]  Timestamp          (u64, when snapshot was taken)
  [4 bytes]  Partition Count    (u32)
  [N * 32]   Partition Refs     (N partitions, each 32 bytes):
    - Partition ID (4 bytes)
    - Block Count (4 bytes)
    - Min Timestamp (8 bytes)
    - Max Timestamp (8 bytes)
    - Checksum (8 bytes)

Total: ~50 + 32N bytes per snapshot
```

### Snapshot Storage

```
Directory Structure:
  snapshots/
    ├─ 0000000001.manifest      (snapshot at txn 1)
    ├─ 0000000001/
    │  ├─ partition_0_blocks.kore
    │  ├─ partition_1_blocks.kore
    │  └─ ...
    ├─ 0000000005.manifest      (snapshot at txn 5, with delta changes)
    ├─ 0000000005/
    │  ├─ delta_partition_0.kore (only changed blocks)
    │  └─ ...
    └─ 0000000010.manifest

Garbage Collection:
  • Keep only last 10 snapshots
  • Delete if no readers reference it
  • Compact snapshots: Merge delta into base every 5 snapshots
```

### Performance Targets
```
Snapshot Creation:       < 1 ms (metadata only, reuse data blocks)
Snapshot Lookup (by ID): < 100 μs (hash table)
Snapshot Lookup (by TS): < 1 ms (binary search on manifest list)
Snapshot Size:           ~50 MB per snapshot (compressed)
Total Snapshot Storage:  500 MB for 10 snapshots
```

---

## 3. MVCC IMPLEMENTATION

### Design Decision: Pessimistic Locking vs Optimistic Concurrency

**CHOSEN: Optimistic MVCC (Better Throughput)**

```
Rationale:
  • No locks = high concurrency
  • Conflicts rare in typical workloads
  • Retry cost < lock contention cost
  • Better for distributed systems
```

### Reader & Writer Protocol

```
Reader Protocol:
  1. Get current snapshot ID: snap_id = read_snapshot_registry.current()
  2. Get snapshot metadata: manifest = load_snapshot(snap_id)
  3. Read blocks from snapshot
  4. Verify: snap_id still exists before returning data
     (if deleted, retry with newer snapshot)

Writer Protocol:
  1. Create private write set: writes = HashMap::new()
  2. Perform operations: writes.insert(partition, row_data)
  3. At commit time:
     a. Read current snapshot: read_snap_id = read_snapshot_registry.current()
     b. Check conflicts: for each write, verify not overwritten
     c. If conflict: ABORT and retry
     d. If no conflict: Create snapshot with write set (atomic)
     e. Update registry: read_snapshot_registry.advance(new_snap_id)
     f. Log to WAL: [COMMIT, txn_id]
```

### Conflict Detection Algorithm

```
Conflict occurs if:
  • Writer wrote to (partition, column, row_range)
  • AND another transaction wrote same range
  • AND both have overlapping timestamps

Detection:
  For each write in write set:
    1. Check bloom filter: "was this block written?" → O(1)
    2. If yes, check exact conflicts in WAL
    3. If conflict exists: ABORT

Cost: Bloom filter < 10 μs per write
      WAL lookup < 100 μs if conflict
```

### Snapshot Registry (In-Memory)

```
Structure:
  current_snapshot: Arc<AtomicU64>     // Current snapshot ID
  active_readers: HashMap<ReaderId, SnapshotId>  // Who's reading what
  committed_txns: Vec<(TxnId, SnapshotId)>  // History

Operations:
  • register_reader(snapshot_id)        → 100 ns
  • unregister_reader(reader_id)        → 100 ns
  • advance_snapshot(new_id)            → 1 μs (CAS operation)
  • check_conflict(write_set)           → 10 μs
```

### Performance Targets
```
Reader Setup:           < 1 μs (snapshot lookup)
Writer Conflict Check:  < 20 μs (bloom filter + WAL check)
Successful Commit:      < 100 μs (snapshot creation + WAL write)
Failed Commit (retry):  < 2 ms (includes retry latency)
Throughput (no conflict): 10,000 txns/sec per thread
Throughput (10% conflict): 5,000 txns/sec per thread
```

---

## 4. TIME-TRAVEL QUERIES

### Design Decision: Read-As-Of Timestamp vs Version Branches

**CHOSEN: Read-As-Of Timestamp (Standard SQL)**

```
Rationale:
  • Standard SQL: SELECT ... AS OF TIMESTAMP '2026-06-20'
  • Better for auditing and point-in-time recovery
  • Natural fit with snapshot isolation
```

### Time-Travel Query Implementation

```
Query: SELECT * FROM table AS OF TIMESTAMP '2026-06-20 15:30:00 UTC'

Execution:
  1. Parse timestamp: t = timestamp('2026-06-20 15:30:00 UTC')
  2. Find snapshot at time t:
     a. Binary search snapshots by timestamp
     b. Load snapshot with timestamp >= t
  3. Load blocks from snapshot
  4. Apply projection and filters
  5. Return data from historical snapshot

Example Snapshots:
  Snapshot 1: ts=2026-06-19 08:00  (old)
  Snapshot 2: ts=2026-06-20 10:00  (match!)
  Snapshot 3: ts=2026-06-20 16:00  (newer)
  
  Query at t=2026-06-20 15:30 will read Snapshot 2
  (use ts >= t, first match)
```

### Block-Level Optimization (Predicate Pushdown)

```
If query has time filter: WHERE timestamp BETWEEN t1 AND t2

Block Selection:
  For each block in snapshot:
    min_ts = block.metadata.min_timestamp
    max_ts = block.metadata.max_timestamp
    
    if max_ts < t1 or min_ts > t2:
      → Skip block (not in time range)
    else:
      → Include block (may have rows in range)

Speedup: 40-70% fewer blocks read (time-series workloads)
```

### Manifest-Level Predicate Pushdown

```
Manifest = [blocks by partition]

If query: SELECT COUNT(*) WHERE timestamp > '2026-06-20'

Optimization:
  1. Check manifest min_ts: is it > query_ts?
  2. If yes: Return COUNT immediately (all rows qualify)
  3. If no: Scan blocks (need to check actual data)

Speedup: 100x faster for year-based aggregations
```

### Performance Targets
```
Time-travel latency:          < 50 ms (for small result sets)
Predicate pushdown benefit:   40-70% I/O reduction
Manifest level pushdown:      100-1000x speedup for agg queries
Storage overhead (history):   +30% disk (keep 7 days snapshots)
```

---

## 5. IMPLEMENTATION PHASES

### Phase 1: Transaction Log (Week 1)
**Deliverable**: WAL writer that can append and recover

```Rust
pub struct WalWriter {
    file: File,
    buffer: Vec<WalEntry>,
    position: u64,
}

impl WalWriter {
    pub fn append(&mut self, entry: WalEntry) -> Result<()> {
        self.buffer.push(entry);
        if self.buffer.len() > BATCH_SIZE {
            self.flush()?;
        }
        Ok(())
    }
    
    pub fn flush(&mut self) -> Result<()> {
        // Write buffer to file + fsync
    }
    
    pub fn commit(&mut self, txn_id: u64) -> Result<()> {
        self.append(WalEntry::commit(txn_id))?;
        self.flush()
    }
}
```

### Phase 2: Snapshot Management (Week 2)
**Deliverable**: Can create and load snapshots

```Rust
pub struct SnapshotManager {
    manifest_dir: PathBuf,
    cache: HashMap<SnapshotId, Manifest>,
}

impl SnapshotManager {
    pub fn create_snapshot(&self, txn_id: u64, write_set: &[Write]) -> Result<SnapshotId> {
        // Create manifest with references to blocks
    }
    
    pub fn load_snapshot(&self, snap_id: SnapshotId) -> Result<Manifest> {
        // Load from disk or cache
    }
}
```

### Phase 3: MVCC Core (Week 3)
**Deliverable**: Concurrent read/write with conflict detection

```Rust
pub struct MvccEngine {
    snapshot_registry: Arc<SnapshotRegistry>,
    wal_writer: Arc<Mutex<WalWriter>>,
    conflict_detector: ConflictDetector,
}

impl MvccEngine {
    pub fn read(&self, snapshot_id: SnapshotId) -> Result<ReadGuard> {
        self.snapshot_registry.register_reader(snapshot_id)
    }
    
    pub fn write(&self, write_set: Vec<Write>) -> Result<CommitId> {
        self.conflict_detector.check(&write_set)?;
        // Create snapshot and commit
    }
}
```

### Phase 4: Concurrent Writers (Week 4)
**Deliverable**: 1000+ concurrent writers

### Phase 5: Time-Travel (Week 5)
**Deliverable**: SELECT ... AS OF TIMESTAMP works

### Phase 6: Optimization (Week 6)
**Deliverable**: 5000 txns/sec, all stress tests pass

---

## 6. TEST PLAN

### Unit Tests (40 tests)
```
Transaction Log:
  [ ] WAL write single entry
  [ ] WAL flush and recovery
  [ ] WAL corruption detection
  [ ] WAL rollover at 1GB

Snapshots:
  [ ] Create snapshot
  [ ] Load snapshot
  [ ] Snapshot metadata integrity
  [ ] Snapshot garbage collection

MVCC:
  [ ] Reader setup/teardown
  [ ] Writer commit (no conflict)
  [ ] Writer abort (conflict)
  [ ] Conflict detection algorithm
  [ ] Snapshot registry update
```

### Integration Tests (50 tests)
```
Concurrent Readers:
  [ ] 10 concurrent readers, same snapshot
  [ ] 100 concurrent readers, different snapshots
  [ ] Reader timeout and cleanup
  
Concurrent Writers:
  [ ] 2 writers, no conflict
  [ ] 2 writers, conflict (one aborts)
  [ ] 10 writers, 5% conflict rate
  [ ] 10 writers, 50% conflict rate
  [ ] 100 writers, sustained
  
Mixed Workload:
  [ ] 50 readers + 10 writers
  [ ] 1000 readers + 100 writers
  [ ] Time-travel read during writes
  
Failure Scenarios:
  [ ] WAL corruption → recovery
  [ ] Snapshot deletion → reader failover
  [ ] Writer abort mid-transaction
  [ ] Deadlock detection
```

### Stress Tests (30 tests)
```
Sustained:
  [ ] 8-hour run: 5000 txns/sec
  [ ] Memory leaks: steady RSS
  [ ] File descriptor leaks: steady FD count
  
Bursts:
  [ ] 10,000 txns/sec for 10 seconds
  [ ] GC pause time: < 100 ms
  [ ] Snapshot creation under load
  
Disaster:
  [ ] Kill WAL file → recovery
  [ ] Corrupt manifest → rollback
  [ ] Full disk → graceful error
```

---

## 7. PERFORMANCE BUDGETS

```
Per-Transaction Overhead:
  WAL write:           50 μs (with fsync)
  Conflict check:      15 μs
  Snapshot update:     5 μs
  Lock/unlock:         0 μs (lockfree)
  Total:               70 μs
  
Throughput = 1,000,000 / 70 ≈ 14,000 txns/sec ✅ (target: 5,000)

Memory Usage:
  Active snapshots:    500 MB (10 snapshots × 50 MB)
  WAL buffer:          100 MB
  Registry:            10 MB
  Total:               610 MB ✅ (target: 1 GB)
```

---

## 8. ROLLOUT PLAN

### Week 1-2: Internal Dogfooding
- Use v1.3.0-alpha internally
- Run workload generators (5000 txns/sec)
- Monitor for issues

### Week 3-4: Beta Program
- Selected customers (5-10)
- 24-hour stability tests
- Feedback collection

### Week 5-6: GA Release
- v1.3.0 General Availability
- Full production support
- Marketing: "Enterprise-grade ACID transactions"

---

**✅ READY TO IMPLEMENT**

6 weeks, 3 engineers, 8,000 lines of code.
Start: July 15, 2026
Complete: August 31, 2026
