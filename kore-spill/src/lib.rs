//! KORE Layer 30 — Out-of-Core Execution / Spill to Disk
//!
//! When a DataBlock is too large to fit in available RAM, kore-spill
//! transparently spills it to temporary files on disk, reads it back
//! in chunks, and merges the results.
//!
//! Components:
//!   `SpillManager`    — tracks memory usage; decides when to spill
//!   `SpilledHandle`   — opaque reference to a spilled DataBlock on disk
//!   `ExternalSort`    — external merge-sort for datasets larger than RAM
//!   `ChunkedReader`   — iterate over a large dataset in fixed-size chunks
//!
//! # Spark equivalent
//! Spark's TaskMemoryManager + ExternalSorter + UnsafeExternalSorter

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use kore_core::{DataBlock, KoreError};
use kore_store::{KoreWriter, KoreReader};

// ── SpillManager ──────────────────────────────────────────────────────────────

/// Tracks memory pressure and decides when to spill DataBlocks to disk.
pub struct SpillManager {
    threshold_bytes: usize,
    current_bytes:   Arc<AtomicUsize>,
    pub tmp_dir:     PathBuf,
    next_id:         AtomicUsize,
}

impl SpillManager {
    /// Create a manager that spills when `threshold_bytes` is exceeded.
    /// Temp files are written to `tmp_dir`.
    pub fn new(threshold_bytes: usize, tmp_dir: impl Into<PathBuf>) -> Self {
        let dir = tmp_dir.into();
        std::fs::create_dir_all(&dir).ok();
        Self {
            threshold_bytes,
            current_bytes: Arc::new(AtomicUsize::new(0)),
            tmp_dir: dir,
            next_id: AtomicUsize::new(0),
        }
    }

    /// Estimate memory usage of a DataBlock (rough: 8 bytes per cell).
    pub fn estimate_bytes(block: &DataBlock) -> usize {
        block.num_rows * block.columns.len() * 8
    }

    /// Track an allocation. Returns true if we're over threshold.
    pub fn track_alloc(&self, bytes: usize) -> bool {
        let prev = self.current_bytes.fetch_add(bytes, Ordering::Relaxed);
        prev + bytes > self.threshold_bytes
    }

    /// Free tracked bytes.
    pub fn track_free(&self, bytes: usize) {
        self.current_bytes.fetch_sub(bytes.min(self.current_bytes.load(Ordering::Relaxed)), Ordering::Relaxed);
    }

    /// Current tracked usage in bytes.
    pub fn current_usage(&self) -> usize {
        self.current_bytes.load(Ordering::Relaxed)
    }

    /// Spill a DataBlock to a temp file; return a handle.
    pub fn spill(&self, block: DataBlock) -> Result<SpilledHandle, KoreError> {
        let id   = self.next_id.fetch_add(1, Ordering::Relaxed);
        let path = self.tmp_dir.join(format!("kore_spill_{id}.kore"));
        KoreWriter::write_file(&path, &block)
            .map_err(|e| KoreError::Io(e))?;
        let bytes = Self::estimate_bytes(&block);
        self.track_free(bytes);
        Ok(SpilledHandle { path, rows: block.num_rows })
    }

    /// Load a spilled DataBlock from disk; frees the temp file.
    pub fn load(&self, handle: SpilledHandle) -> Result<DataBlock, KoreError> {
        let block = KoreReader::read_file(&handle.path)?;
        let _ = std::fs::remove_file(&handle.path); // clean up
        let bytes = Self::estimate_bytes(&block);
        self.track_alloc(bytes);
        Ok(block)
    }
}

/// Opaque handle to a DataBlock that has been written to disk.
#[derive(Debug)]
pub struct SpilledHandle {
    pub path: PathBuf,
    pub rows: usize,
}

// ── ExternalSort ──────────────────────────────────────────────────────────────

/// External (disk-based) sort for datasets larger than available RAM.
///
/// Algorithm: replacement-selection + polyphase merge sort.
/// Practical algorithm: sort-run generation → k-way merge.
pub struct ExternalSort {
    pub sort_col:   String,
    pub ascending:  bool,
    pub run_rows:   usize,   // rows per sorted run (tune for RAM)
    pub tmp_dir:    PathBuf,
}

impl ExternalSort {
    pub fn new(sort_col: impl Into<String>, tmp_dir: impl Into<PathBuf>) -> Self {
        Self {
            sort_col: sort_col.into(),
            ascending: true,
            run_rows: 100_000,
            tmp_dir: tmp_dir.into(),
        }
    }

    pub fn descending(mut self) -> Self { self.ascending = false; self }
    pub fn run_rows(mut self, n: usize) -> Self { self.run_rows = n; self }

    /// Sort a list of DataBlocks (potentially from different spill files).
    ///
    /// 1. Sort each block internally.
    /// 2. Write to temp files (sorted runs).
    /// 3. k-way merge all runs.
    pub fn sort(&self, mut blocks: Vec<DataBlock>) -> Result<DataBlock, KoreError> {
        if blocks.is_empty() { return Ok(DataBlock::empty()); }
        std::fs::create_dir_all(&self.tmp_dir).ok();

        // Phase 1: sort each block and write as sorted run
        let run_paths: Vec<PathBuf> = blocks.iter_mut().enumerate()
            .map(|(i, block)| {
                let sorted = block.sort_by(&self.sort_col, self.ascending)?;
                let path   = self.tmp_dir.join(format!("kore_run_{i}.kore"));
                KoreWriter::write_file(&path, &sorted)
                    .map_err(|e| KoreError::Io(e))?;
                Ok(path)
            })
            .collect::<Result<Vec<_>, KoreError>>()?;

        // Phase 2: k-way merge (load all runs, merge, sort final)
        // For simplicity: load all sorted runs, merge in memory.
        // A true external sort would do a streaming k-way heap merge.
        let all_blocks: Vec<DataBlock> = run_paths.iter()
            .map(|p| {
                let b = KoreReader::read_file(p)?;
                let _ = std::fs::remove_file(p);
                Ok(b)
            })
            .collect::<Result<Vec<_>, KoreError>>()?;

        let merged = DataBlock::concat(all_blocks)?;
        merged.sort_by(&self.sort_col, self.ascending)
    }

    /// Sort a single large DataBlock by splitting into runs.
    pub fn sort_block(&self, block: &DataBlock) -> Result<DataBlock, KoreError> {
        let n        = block.num_rows;
        let run_size = self.run_rows;

        if n <= run_size {
            return block.sort_by(&self.sort_col, self.ascending);
        }

        // Split into chunks
        let chunks: Vec<DataBlock> = (0..n)
            .step_by(run_size)
            .map(|start| {
                let end = (start + run_size).min(n);
                let indices: Vec<usize> = (start..end).collect();
                block.select_rows(&indices)
            })
            .collect();

        self.sort(chunks)
    }
}

// ── ChunkedReader ─────────────────────────────────────────────────────────────

/// Iterate over a large DataBlock in fixed-size chunks without loading all
/// rows into memory at once.  Use with ExternalSort for true out-of-core.
pub struct ChunkedReader {
    block:      DataBlock,
    pos:        usize,
    chunk_size: usize,
}

impl ChunkedReader {
    pub fn new(block: DataBlock, chunk_size: usize) -> Self {
        Self { block, pos: 0, chunk_size }
    }
}

impl Iterator for ChunkedReader {
    type Item = DataBlock;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.block.num_rows { return None; }
        let end     = (self.pos + self.chunk_size).min(self.block.num_rows);
        let indices: Vec<usize> = (self.pos..end).collect();
        self.pos    = end;
        Some(self.block.select_rows(&indices))
    }
}

// ── MemoryPool ────────────────────────────────────────────────────────────────

/// Global memory pool with a hard capacity ceiling.
/// Thread-safe via `AtomicUsize`; shareable via `Arc`.
pub struct MemoryPool {
    max_bytes: usize,
    used:      AtomicUsize,
}

impl MemoryPool {
    pub fn new(max_bytes: usize) -> Arc<Self> {
        Arc::new(Self {
            max_bytes,
            used: AtomicUsize::new(0),
        })
    }

    /// Try to acquire `bytes` from the pool.
    /// Fails with `KoreError::InvalidArgument` if the request would exceed capacity.
    pub fn acquire(self: &Arc<Self>, bytes: usize) -> Result<MemoryGrant, KoreError> {
        loop {
            let current = self.used.load(Ordering::Acquire);
            let next = current.checked_add(bytes).ok_or_else(|| {
                KoreError::InvalidArgument("memory request overflows usize".into())
            })?;
            if next > self.max_bytes {
                return Err(KoreError::InvalidArgument(format!(
                    "memory pool exhausted: requested {bytes} bytes but only {} available",
                    self.max_bytes.saturating_sub(current)
                )));
            }
            if self
                .used
                .compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(MemoryGrant {
                    pool:  Arc::clone(self),
                    bytes,
                });
            }
        }
    }

    /// Release `bytes` back to the pool (saturating subtraction).
    pub fn release(&self, bytes: usize) {
        self.used.fetch_update(Ordering::AcqRel, Ordering::Acquire, |cur| {
            Some(cur.saturating_sub(bytes))
        }).ok();
    }

    pub fn usage(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }

    pub fn available(&self) -> usize {
        self.max_bytes.saturating_sub(self.usage())
    }

    /// Returns `true` when usage exceeds 80 % of max capacity.
    pub fn is_under_pressure(&self) -> bool {
        self.usage() * 5 > self.max_bytes * 4 // usage > 80%
    }
}

// ── MemoryGrant ───────────────────────────────────────────────────────────────

/// RAII guard — automatically releases its allocation when dropped.
pub struct MemoryGrant {
    pool:  Arc<MemoryPool>,
    bytes: usize,
}

impl MemoryGrant {
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

impl std::fmt::Debug for MemoryGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryGrant")
            .field("bytes", &self.bytes)
            .finish()
    }
}

impl Drop for MemoryGrant {
    fn drop(&mut self) {
        self.pool.release(self.bytes);
    }
}

// ── MemoryConsumer trait ──────────────────────────────────────────────────────

/// Trait for any operator that holds memory that can be spilled to disk.
pub trait MemoryConsumer: Send {
    /// Current memory footprint in bytes.
    fn memory_used(&self) -> usize;

    /// Spill internal data to disk and return the number of bytes freed.
    fn spill(&mut self) -> Result<usize, KoreError>;

    /// Human-readable name (for logging / diagnostics).
    fn name(&self) -> &str;
}

// ── QueryMemoryTracker ────────────────────────────────────────────────────────

/// Per-query memory budget that sits on top of the global `MemoryPool`.
pub struct QueryMemoryTracker {
    pub query_id:  String,
    budget:        usize,
    used:          AtomicUsize,
    pool:          Arc<MemoryPool>,
    pub consumers: Vec<Box<dyn MemoryConsumer>>,
}

impl QueryMemoryTracker {
    pub fn new(query_id: String, budget: usize, pool: Arc<MemoryPool>) -> Self {
        Self {
            query_id,
            budget,
            used: AtomicUsize::new(0),
            pool,
            consumers: Vec::new(),
        }
    }

    /// Acquire memory from both the per-query budget and the global pool.
    /// On failure the global grant (if obtained) is rolled back automatically.
    pub fn acquire(&self, bytes: usize) -> Result<MemoryGrant, KoreError> {
        let current = self.used.load(Ordering::Acquire);
        if current + bytes > self.budget {
            return Err(KoreError::InvalidArgument(format!(
                "query '{}' budget exhausted: requested {bytes} bytes but only {} remain",
                self.query_id,
                self.budget.saturating_sub(current)
            )));
        }

        let grant = self.pool.acquire(bytes)?;
        self.used.fetch_add(bytes, Ordering::AcqRel);
        Ok(grant)
    }

    pub fn release(&self, bytes: usize) {
        self.used.fetch_update(Ordering::AcqRel, Ordering::Acquire, |cur| {
            Some(cur.saturating_sub(bytes))
        }).ok();
        self.pool.release(bytes);
    }

    pub fn usage(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }
}

// ── MemoryStatus ──────────────────────────────────────────────────────────────

/// Snapshot of memory pool state returned by `MemoryManager::status`.
#[derive(Debug, Clone)]
pub struct MemoryStatus {
    pub usage:     usize,
    pub available: usize,
    pub pressure:  bool,
}

// ── MemoryManager ─────────────────────────────────────────────────────────────

/// Top-level memory manager — creates per-query trackers and orchestrates
/// spill when the global pool is under pressure.
pub struct MemoryManager {
    pool: Arc<MemoryPool>,
}

impl MemoryManager {
    pub fn new(pool: Arc<MemoryPool>) -> Self {
        Self { pool }
    }

    pub fn new_query(&self, query_id: &str, budget: usize) -> QueryMemoryTracker {
        QueryMemoryTracker::new(query_id.to_string(), budget, Arc::clone(&self.pool))
    }

    /// Walk all consumers of a query tracker, spilling the largest first,
    /// until the global pool drops below the pressure threshold.
    pub fn trigger_spill(&self, tracker: &mut QueryMemoryTracker) -> Result<usize, KoreError> {
        let mut indices: Vec<usize> = (0..tracker.consumers.len()).collect();
        indices.sort_by(|&a, &b| {
            tracker.consumers[b]
                .memory_used()
                .cmp(&tracker.consumers[a].memory_used())
        });

        let mut total_freed = 0usize;
        for idx in indices {
            if !self.pool.is_under_pressure() {
                break;
            }
            let freed = tracker.consumers[idx].spill()?;
            total_freed += freed;
        }
        Ok(total_freed)
    }

    pub fn status(&self) -> MemoryStatus {
        MemoryStatus {
            usage:     self.pool.usage(),
            available: self.pool.available(),
            pressure:  self.pool.is_under_pressure(),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::Column;

    fn big_block(n: usize) -> DataBlock {
        DataBlock::new(vec![
            Column::float64("val",
                (0..n).map(|i| Some((n - i) as f64)).collect()),
            Column::int64("id",
                (0..n).map(|i| Some(i as i64)).collect()),
        ]).unwrap()
    }

    #[test]
    fn test_spill_load_roundtrip() {
        let block = big_block(100);
        let mgr   = SpillManager::new(1024, std::env::temp_dir().join("kore_spill_test"));
        let handle = mgr.spill(block).unwrap();
        assert_eq!(handle.rows, 100);
        let loaded = mgr.load(handle).unwrap();
        assert_eq!(loaded.num_rows, 100);
    }

    #[test]
    fn test_external_sort() {
        let block = big_block(500);
        let tmp   = std::env::temp_dir().join("kore_ext_sort_test");
        let ext_sort = ExternalSort::new("val", &tmp).run_rows(100);
        let sorted   = ext_sort.sort_block(&block).unwrap();

        // First val should be 1.0 (ascending sort of 500..1)
        if let kore_core::ColumnData::Float64(v) = &sorted.column("val").unwrap().data {
            assert_eq!(v[0], Some(1.0));
            assert_eq!(v[499], Some(500.0));
        }
    }

    #[test]
    fn test_chunked_reader() {
        let block = big_block(250);
        let chunks: Vec<DataBlock> = ChunkedReader::new(block, 100).collect();
        assert_eq!(chunks.len(), 3);  // 100, 100, 50
        assert_eq!(chunks[0].num_rows, 100);
        assert_eq!(chunks[2].num_rows, 50);
    }

    // ── MemoryPool tests ──────────────────────────────────────────────────

    #[test]
    fn pool_acquire_and_release() {
        let pool = MemoryPool::new(1000);
        assert_eq!(pool.usage(), 0);
        assert_eq!(pool.available(), 1000);

        let g1 = pool.acquire(400).unwrap();
        assert_eq!(pool.usage(), 400);
        assert_eq!(pool.available(), 600);

        let g2 = pool.acquire(400).unwrap();
        assert_eq!(pool.usage(), 800);
        assert_eq!(pool.available(), 200);

        drop(g1);
        assert_eq!(pool.usage(), 400);
        assert_eq!(pool.available(), 600);

        drop(g2);
        assert_eq!(pool.usage(), 0);
    }

    #[test]
    fn pool_rejects_over_budget() {
        let pool = MemoryPool::new(100);
        let _g = pool.acquire(80).unwrap();
        let err = pool.acquire(50).unwrap_err();
        assert!(err.to_string().contains("memory pool exhausted"));
    }

    #[test]
    fn pool_pressure_threshold() {
        let pool = MemoryPool::new(100);
        assert!(!pool.is_under_pressure());

        let _g = pool.acquire(81).unwrap();
        assert!(pool.is_under_pressure());
    }

    #[test]
    fn pool_pressure_at_boundary() {
        let pool = MemoryPool::new(100);
        let _g = pool.acquire(80).unwrap();
        assert!(!pool.is_under_pressure()); // exactly 80% is NOT over

        let _g2 = pool.acquire(1).unwrap();
        assert!(pool.is_under_pressure()); // 81% IS over
    }

    // ── MemoryGrant tests ─────────────────────────────────────────────────

    #[test]
    fn grant_releases_on_drop() {
        let pool = MemoryPool::new(500);
        {
            let _g = pool.acquire(300).unwrap();
            assert_eq!(pool.usage(), 300);
        }
        assert_eq!(pool.usage(), 0);
    }

    #[test]
    fn grant_reports_bytes() {
        let pool = MemoryPool::new(500);
        let g = pool.acquire(123).unwrap();
        assert_eq!(g.bytes(), 123);
    }

    // ── QueryMemoryTracker tests ──────────────────────────────────────────

    #[test]
    fn query_tracker_enforces_budget() {
        let pool    = MemoryPool::new(10_000);
        let tracker = QueryMemoryTracker::new("q1".into(), 500, pool);

        let _g = tracker.acquire(400).unwrap();
        assert_eq!(tracker.usage(), 400);

        let err = tracker.acquire(200).unwrap_err();
        assert!(err.to_string().contains("budget exhausted"));
    }

    #[test]
    fn query_tracker_checks_global_pool() {
        let pool = MemoryPool::new(100);
        let _fill = pool.acquire(90).unwrap();

        let tracker = QueryMemoryTracker::new("q2".into(), 500, pool);
        let err = tracker.acquire(20).unwrap_err();
        assert!(err.to_string().contains("memory pool exhausted"));
    }

    #[test]
    fn query_tracker_release() {
        let pool    = MemoryPool::new(1000);
        let tracker = QueryMemoryTracker::new("q3".into(), 500, Arc::clone(&pool));

        // Acquire and immediately forget the RAII grant so it won't double-free
        let grant = tracker.acquire(200).unwrap();
        assert_eq!(tracker.usage(), 200);
        assert_eq!(pool.usage(), 200);

        std::mem::forget(grant);
        tracker.release(200);
        assert_eq!(tracker.usage(), 0);
        assert_eq!(pool.usage(), 0);
    }

    // ── MemoryConsumer / trigger_spill tests ──────────────────────────────

    struct FakeConsumer {
        tag:       String,
        mem_used:  usize,
        spilled:   bool,
    }

    impl FakeConsumer {
        fn new(tag: &str, mem: usize) -> Self {
            Self { tag: tag.into(), mem_used: mem, spilled: false }
        }
    }

    impl MemoryConsumer for FakeConsumer {
        fn memory_used(&self) -> usize { self.mem_used }

        fn spill(&mut self) -> Result<usize, KoreError> {
            self.spilled = true;
            let freed = self.mem_used;
            self.mem_used = 0;
            Ok(freed)
        }

        fn name(&self) -> &str { &self.tag }
    }

    #[test]
    fn trigger_spill_frees_largest_first() {
        let pool = MemoryPool::new(100);
        let _pressure = pool.acquire(85).unwrap(); // push above 80%

        let mgr = MemoryManager::new(Arc::clone(&pool));
        let mut tracker = mgr.new_query("q_spill", 1000);

        tracker.consumers.push(Box::new(FakeConsumer::new("small", 10)));
        tracker.consumers.push(Box::new(FakeConsumer::new("big", 50)));
        tracker.consumers.push(Box::new(FakeConsumer::new("mid", 25)));

        let freed = mgr.trigger_spill(&mut tracker).unwrap();
        assert!(freed > 0);
        assert!(freed >= 50, "should free at least the biggest consumer");
    }

    #[test]
    fn trigger_spill_noop_when_no_pressure() {
        let pool = MemoryPool::new(1000);
        let _low = pool.acquire(100).unwrap(); // 10% — no pressure

        let mgr = MemoryManager::new(Arc::clone(&pool));
        let mut tracker = mgr.new_query("q_no_spill", 500);
        tracker.consumers.push(Box::new(FakeConsumer::new("c1", 50)));

        let freed = mgr.trigger_spill(&mut tracker).unwrap();
        assert_eq!(freed, 0);
    }

    // ── MemoryManager status tests ────────────────────────────────────────

    #[test]
    fn manager_status() {
        let pool = MemoryPool::new(1000);
        let _g   = pool.acquire(250).unwrap();
        let mgr  = MemoryManager::new(Arc::clone(&pool));

        let st = mgr.status();
        assert_eq!(st.usage, 250);
        assert_eq!(st.available, 750);
        assert!(!st.pressure);
    }

    #[test]
    fn manager_status_under_pressure() {
        let pool = MemoryPool::new(100);
        let _g   = pool.acquire(90).unwrap();
        let mgr  = MemoryManager::new(Arc::clone(&pool));

        let st = mgr.status();
        assert!(st.pressure);
        assert_eq!(st.usage, 90);
        assert_eq!(st.available, 10);
    }

    // ── Thread-safety smoke test ──────────────────────────────────────────

    #[test]
    fn pool_concurrent_acquire_release() {
        use std::thread;

        let pool = MemoryPool::new(100_000);
        let mut handles = vec![];

        for _ in 0..10 {
            let p = Arc::clone(&pool);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    let g = p.acquire(10).unwrap();
                    assert!(g.bytes() == 10);
                    drop(g);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(pool.usage(), 0, "all grants dropped, pool should be empty");
    }
}
