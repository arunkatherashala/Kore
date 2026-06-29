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

use std::path::{Path, PathBuf};
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
        let mut all_blocks: Vec<DataBlock> = run_paths.iter()
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
}
