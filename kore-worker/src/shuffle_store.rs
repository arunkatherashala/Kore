//! Worker-local shuffle store — receives pushes from peer workers.
//!
//! Layout: `(shuffle_id, reduce_partition) -> Vec<Entry>` where each `Entry`
//! is either an in-memory `DataBlock` or a `SpillRef` pointing at a file on
//! disk. Spill kicks in when the estimated in-memory byte count crosses a
//! configurable threshold (`KORE_SHUFFLE_MEM_MB`, default 512 MB). Reduce
//! reads transparently materialize spilled blocks.
//!
//! Each map task hash-partitions its output and pushes each partition to the
//! peer that owns that reduce partition. When a `ShuffleReduceTask` arrives,
//! the reducer waits until it has received `expected_maps` blocks for its
//! partition, then concats them and runs the reduce SQL.
//!
//! `wait_for` uses a `tokio::sync::Notify` so reducer tasks sleep efficiently
//! instead of polling.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kore_core::DataBlock;
use tokio::sync::Notify;

// ─── Entry (memory or spilled) ───────────────────────────────────────────────

enum Entry {
    InMem(DataBlock),
    Spill(SpillRef),
}

struct SpillRef {
    path: PathBuf,
    num_rows: usize,
}

// ─── Store ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ShuffleStore {
    inner:  Arc<Mutex<HashMap<(String, usize), Vec<Entry>>>>,
    notify: Arc<Notify>,
    /// Estimated in-memory bytes (updated by push/spill/drain).
    mem_bytes: Arc<AtomicU64>,
    /// Config: when memory bytes exceed this, spill oldest blocks.
    mem_limit_bytes: u64,
    /// Directory for spill files. `None` disables spilling.
    spill_dir: Option<PathBuf>,
}

impl Default for ShuffleStore {
    fn default() -> Self { Self::new() }
}

impl ShuffleStore {
    /// In-memory only (spill disabled). Used by tests + default cluster.
    pub fn new() -> Self {
        Self::from_env()
    }

    /// Read configuration from `KORE_SHUFFLE_SPILL_DIR` and
    /// `KORE_SHUFFLE_MEM_MB` env vars.
    pub fn from_env() -> Self {
        let spill_dir = std::env::var("KORE_SHUFFLE_SPILL_DIR")
            .ok()
            .map(PathBuf::from);
        let mem_mb = std::env::var("KORE_SHUFFLE_MEM_MB")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(512u64);
        Self {
            inner:  Arc::new(Mutex::new(HashMap::new())),
            notify: Arc::new(Notify::new()),
            mem_bytes: Arc::new(AtomicU64::new(0)),
            mem_limit_bytes: mem_mb * 1024 * 1024,
            spill_dir,
        }
    }

    /// Explicit constructor used by tests.
    pub fn with_spill(spill_dir: impl Into<PathBuf>, mem_limit_bytes: u64) -> Self {
        let dir = spill_dir.into();
        std::fs::create_dir_all(&dir).ok();
        Self {
            inner:  Arc::new(Mutex::new(HashMap::new())),
            notify: Arc::new(Notify::new()),
            mem_bytes: Arc::new(AtomicU64::new(0)),
            mem_limit_bytes,
            spill_dir: Some(dir),
        }
    }

    /// Append a pushed block for `(shuffle_id, partition)`, wake waiters.
    /// Triggers spill of *this* block if we're already over the memory limit
    /// (keeps peak usage bounded by `2 × mem_limit` in the worst case).
    pub fn push(&self, shuffle_id: &str, partition: usize, block: DataBlock) {
        let block_bytes = estimate_bytes(&block);
        let key = (shuffle_id.to_string(), partition);
        // Decide spill *before* taking the lock so file I/O is off-lock.
        let over_limit = self.spill_dir.is_some()
            && self.mem_bytes.load(Ordering::Relaxed) + block_bytes as u64 > self.mem_limit_bytes;
        let entry = if over_limit {
            match self.spill_block(shuffle_id, partition, &block) {
                Ok(sr) => Entry::Spill(sr),
                Err(_) => {
                    // Spill failed — fall back to memory so we don't drop data.
                    self.mem_bytes.fetch_add(block_bytes as u64, Ordering::Relaxed);
                    Entry::InMem(block)
                }
            }
        } else {
            self.mem_bytes.fetch_add(block_bytes as u64, Ordering::Relaxed);
            Entry::InMem(block)
        };
        {
            let mut map = self.inner.lock().unwrap();
            map.entry(key).or_default().push(entry);
        }
        // Broadcast: any reducer waiting on any partition should re-check.
        self.notify.notify_waiters();
    }

    fn spill_block(
        &self,
        shuffle_id: &str,
        partition: usize,
        block: &DataBlock,
    ) -> std::io::Result<SpillRef> {
        let dir = self.spill_dir.as_ref().expect("spill_dir must be Some for spill");
        let path = dir.join(format!(
            "kshuf-{shuffle_id}-p{partition}-{}.msg",
            spill_seq_next()
        ));
        // Reuse the wire codec: msgpack + LZ4. Fast and shrinks numeric data.
        let msg = kore_net::KoreMsg::ShufflePush {
            shuffle_id: shuffle_id.to_string(),
            src_worker: String::new(),
            partition,
            data:       block.clone(),
        };
        let bytes = kore_net::__codec_encode_shuffle_push(&msg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        std::fs::write(&path, bytes)?;
        Ok(SpillRef { path, num_rows: block.num_rows })
    }

    /// Snapshot the current count for a key without blocking.
    pub fn count(&self, shuffle_id: &str, partition: usize) -> usize {
        let key = (shuffle_id.to_string(), partition);
        self.inner.lock().unwrap()
            .get(&key)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// Currently estimated in-memory footprint (bytes). Excludes spilled data.
    pub fn mem_bytes(&self) -> u64 {
        self.mem_bytes.load(Ordering::Relaxed)
    }

    /// Wait (with timeout) until `count(shuffle_id, partition) >= expected`.
    /// Returns the accumulated blocks (drained + materialized), or `None`
    /// on timeout.
    pub async fn wait_and_drain(
        &self,
        shuffle_id: &str,
        partition:  usize,
        expected:   usize,
        timeout:    Duration,
    ) -> Option<Vec<DataBlock>> {
        let deadline = tokio::time::Instant::now() + timeout;
        let entries = loop {
            if self.count(shuffle_id, partition) >= expected {
                let key = (shuffle_id.to_string(), partition);
                let mut map = self.inner.lock().unwrap();
                break map.remove(&key)?;
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() { return None; }
            let notified = self.notify.notified();
            let _ = tokio::time::timeout(remaining, notified).await;
        };
        // Materialize spilled entries. Drop mem counter for in-mem entries.
        let mut out = Vec::with_capacity(entries.len());
        for e in entries {
            match e {
                Entry::InMem(block) => {
                    self.mem_bytes.fetch_sub(estimate_bytes(&block) as u64, Ordering::Relaxed);
                    out.push(block);
                }
                Entry::Spill(sr) => {
                    match self.read_spill(&sr) {
                        Ok(b) => out.push(b),
                        Err(e) => eprintln!("[shuffle-store] failed to read spill {:?}: {e}", sr.path),
                    }
                }
            }
        }
        Some(out)
    }

    fn read_spill(&self, sr: &SpillRef) -> std::io::Result<DataBlock> {
        let bytes = std::fs::read(&sr.path)?;
        let msg = kore_net::__codec_decode_shuffle_push(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        // Best-effort cleanup — spill file is single-use.
        let _ = std::fs::remove_file(&sr.path);
        if let kore_net::KoreMsg::ShufflePush { data, .. } = msg {
            debug_assert_eq!(data.num_rows, sr.num_rows);
            Ok(data)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "spill file did not contain ShufflePush",
            ))
        }
    }

    /// Number of distinct (shuffle_id, partition) keys currently held.
    pub fn key_count(&self) -> usize { self.inner.lock().unwrap().len() }

    /// Drop all entries for one shuffle_id (freeing memory + spill files).
    pub fn drop_shuffle(&self, shuffle_id: &str) {
        let mut map = self.inner.lock().unwrap();
        let removed: Vec<Vec<Entry>> = map
            .iter()
            .filter(|((sid, _), _)| sid == shuffle_id)
            .map(|(_, v)| v.iter().map(cheap_ref).collect())
            .collect();
        for entries in removed {
            for e in entries {
                if let Entry::Spill(sr) = e { let _ = std::fs::remove_file(&sr.path); }
            }
        }
        map.retain(|(sid, _), _| sid != shuffle_id);
    }
}

fn cheap_ref(e: &Entry) -> Entry {
    match e {
        Entry::InMem(_) => Entry::InMem(DataBlock::empty()),   // just a marker; not used to read
        Entry::Spill(sr) => Entry::Spill(SpillRef { path: sr.path.clone(), num_rows: sr.num_rows }),
    }
}

fn estimate_bytes(block: &DataBlock) -> usize {
    // Rough: numeric = 9 bytes/cell (8-byte value + 1 null bit),
    // string = column-avg. This mirrors kore-aqe::StageStats::estimate_bytes.
    block.columns.iter().map(|c| {
        use kore_core::ColumnData::*;
        match &c.data {
            Int64(v)   => v.len() * 9,
            Float64(v) => v.len() * 9,
            Bool(v)    => v.len() * 2,
            Str(v)     => v.iter().map(|s| s.as_deref().map(str::len).unwrap_or(0) + 8).sum(),
            StrDict { codes, dict } => codes.len() + dict.iter().map(|s| s.len()).sum::<usize>(),
        }
    }).sum()
}

fn spill_seq_next() -> u64 {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn blk(n: usize) -> DataBlock {
        DataBlock {
            num_rows: n,
            columns: vec![Column {
                name: "x".into(),
                data: ColumnData::Int64((0..n).map(|i| Some(i as i64)).collect()),
            }],
        }
    }

    #[tokio::test]
    async fn push_and_wait_completes() {
        let store = ShuffleStore::new();
        let s2 = store.clone();
        tokio::spawn(async move {
            for _ in 0..3 {
                tokio::time::sleep(Duration::from_millis(5)).await;
                s2.push("sh1", 0, blk(10));
            }
        });
        let got = store
            .wait_and_drain("sh1", 0, 3, Duration::from_secs(2))
            .await
            .expect("expected 3 blocks");
        assert_eq!(got.len(), 3);
        // After drain, count is 0.
        assert_eq!(store.count("sh1", 0), 0);
    }

    #[tokio::test]
    async fn wait_times_out() {
        let store = ShuffleStore::new();
        let got = store
            .wait_and_drain("sh_none", 0, 5, Duration::from_millis(30))
            .await;
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn drop_shuffle_frees_entries() {
        let store = ShuffleStore::new();
        store.push("sh1", 0, blk(1));
        store.push("sh1", 1, blk(1));
        store.push("sh2", 0, blk(1));
        assert_eq!(store.key_count(), 3);
        store.drop_shuffle("sh1");
        assert_eq!(store.key_count(), 1);
        assert_eq!(store.count("sh2", 0), 1);
    }

    #[tokio::test]
    async fn spills_when_over_memory_limit() {
        // 500 bytes cap forces spill for any push after the first small one.
        let tmp = std::env::temp_dir().join("kore_shuffle_spill_test");
        let _ = std::fs::remove_dir_all(&tmp);
        let store = ShuffleStore::with_spill(&tmp, 500);

        // First push: ~90 bytes (10 rows * 9 bytes). Stays in memory.
        store.push("sh_x", 0, blk(10));
        assert!(store.mem_bytes() > 0);
        let mem_after_1 = store.mem_bytes();

        // Second push: 200 rows * 9 = ~1800 bytes. mem_bytes(1st) + 1800 > 500 → spill.
        store.push("sh_x", 0, blk(200));

        // Memory tracker should not have grown by the full spilled block.
        // (Still equal to mem_after_1 because the 2nd push went to disk.)
        assert_eq!(store.mem_bytes(), mem_after_1,
            "expected the 2nd block to spill without growing memory counter");

        // Drain should return both, transparently reading the spill.
        let got = store
            .wait_and_drain("sh_x", 0, 2, Duration::from_secs(2))
            .await
            .expect("drain");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].num_rows + got[1].num_rows, 210);

        // After drain, memory counter returns to zero.
        assert_eq!(store.mem_bytes(), 0);

        std::fs::remove_dir_all(&tmp).ok();
    }
}
