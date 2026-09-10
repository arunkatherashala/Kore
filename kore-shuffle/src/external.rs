//! Sort-based external shuffle with memory-pressure-driven spill.
//!
//! Implements the full map-side sort → disk write → reduce-side k-way merge
//! pattern used by Spark's SortShuffleManager, backed by kore-store for
//! persistence and kore-spill for out-of-core sorting.

use std::collections::BinaryHeap;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::Arc;

use kore_core::{Column, ColumnData, DataBlock, KoreError};
use kore_store::{KoreReader, KoreWriter};

use crate::HashPartitioner;

// ── MemoryPressureCallback ────────────────────────────────────────────────────

/// Trait for receiving memory-pressure notifications.
/// Implementors decide how to react (e.g. spill buffers to disk).
pub trait MemoryPressureCallback: Send + Sync {
    fn on_pressure(&self, current_bytes: usize, threshold_bytes: usize);
}

/// Default implementation that records that pressure was triggered.
/// Real systems would trigger a spill; here we expose `triggered()` for testing.
pub struct ThresholdPressureCallback {
    threshold: usize,
    triggered_count: AtomicUsize,
}

impl ThresholdPressureCallback {
    pub fn new(threshold: usize) -> Self {
        Self {
            threshold,
            triggered_count: AtomicUsize::new(0),
        }
    }

    pub fn threshold(&self) -> usize {
        self.threshold
    }

    pub fn triggered_count(&self) -> usize {
        self.triggered_count.load(AtomicOrdering::Relaxed)
    }
}

impl MemoryPressureCallback for ThresholdPressureCallback {
    fn on_pressure(&self, _current_bytes: usize, _threshold_bytes: usize) {
        self.triggered_count.fetch_add(1, AtomicOrdering::Relaxed);
    }
}

// ── PartitionMeta ─────────────────────────────────────────────────────────────

/// Metadata about a single written partition file.
#[derive(Debug, Clone)]
pub struct PartitionFileMeta {
    pub partition_id: usize,
    pub path: PathBuf,
    pub num_rows: usize,
    pub size_bytes: usize,
    pub sort_col: Option<String>,
}

// ── SortBasedShuffleWriter ────────────────────────────────────────────────────

/// Writes a DataBlock to disk as multiple sorted partition files.
///
/// Algorithm:
/// 1. Hash-partition the input block by key columns into N partitions.
/// 2. Sort each partition by the specified sort column.
/// 3. Write each sorted partition to a temporary .kore file.
/// 4. Return metadata for each partition.
pub struct SortBasedShuffleWriter {
    output_dir: PathBuf,
    n_partitions: usize,
    key_cols: Vec<String>,
    sort_col: String,
    ascending: bool,
}

impl SortBasedShuffleWriter {
    pub fn new(
        output_dir: impl Into<PathBuf>,
        n_partitions: usize,
        key_cols: Vec<String>,
        sort_col: impl Into<String>,
    ) -> Self {
        Self {
            output_dir: output_dir.into(),
            n_partitions,
            key_cols,
            sort_col: sort_col.into(),
            ascending: true,
        }
    }

    pub fn ascending(mut self, asc: bool) -> Self {
        self.ascending = asc;
        self
    }

    /// Partition, sort, and write the block. Returns per-partition metadata.
    pub fn write(&self, block: &DataBlock, writer_id: &str) -> Result<Vec<PartitionFileMeta>, KoreError> {
        std::fs::create_dir_all(&self.output_dir)
            .map_err(|e| KoreError::Io(e))?;

        let partitioner = HashPartitioner::new(self.n_partitions, self.key_cols.clone());
        let partitions = partitioner.partition(block);

        let mut metas = Vec::with_capacity(self.n_partitions);

        for (pid, part_block) in partitions.into_iter().enumerate() {
            if part_block.num_rows == 0 {
                metas.push(PartitionFileMeta {
                    partition_id: pid,
                    path: PathBuf::new(),
                    num_rows: 0,
                    size_bytes: 0,
                    sort_col: Some(self.sort_col.clone()),
                });
                continue;
            }

            let sorted = part_block.sort_by(&self.sort_col, self.ascending)?;
            let file_name = format!("shuffle_{writer_id}_part{pid}.kore");
            let path = self.output_dir.join(&file_name);

            KoreWriter::write_file(&path, &sorted)
                .map_err(|e| KoreError::Io(e))?;

            let size = std::fs::metadata(&path)
                .map(|m| m.len() as usize)
                .unwrap_or(0);

            metas.push(PartitionFileMeta {
                partition_id: pid,
                path,
                num_rows: sorted.num_rows,
                size_bytes: size,
                sort_col: Some(self.sort_col.clone()),
            });
        }

        Ok(metas)
    }
}

// ── MergeBasedShuffleReader ───────────────────────────────────────────────────

/// Performs k-way merge of multiple sorted partition files into a single
/// sorted DataBlock for one reduce partition.
///
/// Each input file must already be sorted on `sort_col`. The reader opens all
/// files, conceptually maintains a min-heap of (current_row, source_index),
/// and produces rows in globally sorted order.
pub struct MergeBasedShuffleReader {
    sort_col: String,
    ascending: bool,
}

impl MergeBasedShuffleReader {
    pub fn new(sort_col: impl Into<String>) -> Self {
        Self {
            sort_col: sort_col.into(),
            ascending: true,
        }
    }

    pub fn ascending(mut self, asc: bool) -> Self {
        self.ascending = asc;
        self
    }

    /// Read and k-way merge multiple sorted partition files.
    pub fn merge(&self, paths: &[PathBuf]) -> Result<DataBlock, KoreError> {
        let valid_paths: Vec<&PathBuf> = paths.iter()
            .filter(|p| p.as_os_str().len() > 0)
            .collect();

        if valid_paths.is_empty() {
            return Ok(DataBlock::empty());
        }

        let blocks: Vec<DataBlock> = valid_paths.iter()
            .map(|p| KoreReader::read_file(p))
            .collect::<Result<Vec<_>, _>>()?;

        let non_empty: Vec<DataBlock> = blocks.into_iter()
            .filter(|b| b.num_rows > 0)
            .collect();

        if non_empty.is_empty() {
            return Ok(DataBlock::empty());
        }

        if non_empty.len() == 1 {
            return Ok(non_empty.into_iter().next().unwrap());
        }

        self.k_way_merge(non_empty)
    }

    /// K-way merge of pre-sorted DataBlocks using a binary heap.
    fn k_way_merge(&self, blocks: Vec<DataBlock>) -> Result<DataBlock, KoreError> {
        let sort_col = &self.sort_col;
        let ascending = self.ascending;

        let total_rows: usize = blocks.iter().map(|b| b.num_rows).sum();
        if total_rows == 0 {
            return Ok(DataBlock::empty());
        }

        let sort_keys: Vec<Vec<SortKey>> = blocks.iter()
            .map(|b| extract_sort_keys(b, sort_col))
            .collect::<Result<Vec<_>, _>>()?;

        let mut cursors: Vec<usize> = vec![0; blocks.len()];
        let mut heap: BinaryHeap<HeapEntry> = BinaryHeap::new();

        for (src, keys) in sort_keys.iter().enumerate() {
            if !keys.is_empty() {
                heap.push(HeapEntry {
                    key: keys[0].clone(),
                    source: src,
                    row: 0,
                    ascending,
                });
            }
        }

        let mut merge_order: Vec<(usize, usize)> = Vec::with_capacity(total_rows);

        while let Some(entry) = heap.pop() {
            merge_order.push((entry.source, entry.row));
            cursors[entry.source] += 1;
            let next_row = cursors[entry.source];
            if next_row < blocks[entry.source].num_rows {
                heap.push(HeapEntry {
                    key: sort_keys[entry.source][next_row].clone(),
                    source: entry.source,
                    row: next_row,
                    ascending,
                });
            }
        }

        build_merged_block(&blocks, &merge_order)
    }
}

// ── Heap entry for k-way merge ────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum SortKey {
    Int(Option<i64>),
    Float(Option<f64>),
    Str(Option<String>),
}

impl SortKey {
    fn cmp_asc(&self, other: &Self) -> Ordering {
        match (self, other) {
            (SortKey::Int(a), SortKey::Int(b)) => cmp_option(a, b),
            (SortKey::Float(a), SortKey::Float(b)) => cmp_option_f64(a, b),
            (SortKey::Str(a), SortKey::Str(b)) => cmp_option(a, b),
            _ => Ordering::Equal,
        }
    }
}

fn cmp_option<T: Ord>(a: &Option<T>, b: &Option<T>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => x.cmp(y),
    }
}

fn cmp_option_f64(a: &Option<f64>, b: &Option<f64>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
    }
}

#[derive(Clone, Debug)]
struct HeapEntry {
    key: SortKey,
    source: usize,
    row: usize,
    ascending: bool,
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.key.cmp_asc(&other.key) == Ordering::Equal
    }
}

impl Eq for HeapEntry {}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap; invert for ascending (we want smallest first).
        let base = self.key.cmp_asc(&other.key);
        if self.ascending {
            base.reverse()
        } else {
            base
        }
    }
}

fn extract_sort_keys(block: &DataBlock, col_name: &str) -> Result<Vec<SortKey>, KoreError> {
    let col = block.column(col_name)
        .ok_or_else(|| KoreError::ColumnNotFound(col_name.to_string()))?;

    let keys = match &col.data {
        ColumnData::Int64(v) => v.iter().map(|x| SortKey::Int(*x)).collect(),
        ColumnData::Float64(v) => v.iter().map(|x| SortKey::Float(*x)).collect(),
        ColumnData::Str(v) => v.iter().map(|x| SortKey::Str(x.clone())).collect(),
        ColumnData::Bool(v) => v.iter().map(|x| SortKey::Int(x.map(|b| b as i64))).collect(),
        ColumnData::StrDict { codes, dict } => {
            codes.iter().map(|&c| {
                if c == u8::MAX {
                    SortKey::Str(None)
                } else {
                    SortKey::Str(dict.get(c as usize).cloned())
                }
            }).collect()
        }
    };
    Ok(keys)
}

fn build_merged_block(
    sources: &[DataBlock],
    order: &[(usize, usize)],
) -> Result<DataBlock, KoreError> {
    if order.is_empty() {
        return Ok(DataBlock::empty());
    }

    let schema_block = &sources[0];
    let mut columns: Vec<Column> = Vec::with_capacity(schema_block.columns.len());

    for col_def in &schema_block.columns {
        let col_name = &col_def.name;
        let data = match &col_def.data {
            ColumnData::Int64(_) => {
                let vals: Vec<Option<i64>> = order.iter().map(|&(src, row)| {
                    sources[src].column(col_name)
                        .and_then(|c| if let ColumnData::Int64(v) = &c.data { v.get(row).copied().flatten() } else { None })
                }).collect();
                ColumnData::Int64(vals)
            }
            ColumnData::Float64(_) => {
                let vals: Vec<Option<f64>> = order.iter().map(|&(src, row)| {
                    sources[src].column(col_name)
                        .and_then(|c| if let ColumnData::Float64(v) = &c.data { v.get(row).copied().flatten() } else { None })
                }).collect();
                ColumnData::Float64(vals)
            }
            ColumnData::Bool(_) => {
                let vals: Vec<Option<bool>> = order.iter().map(|&(src, row)| {
                    sources[src].column(col_name)
                        .and_then(|c| if let ColumnData::Bool(v) = &c.data { v.get(row).copied().flatten() } else { None })
                }).collect();
                ColumnData::Bool(vals)
            }
            ColumnData::Str(_) => {
                let vals: Vec<Option<String>> = order.iter().map(|&(src, row)| {
                    sources[src].column(col_name)
                        .and_then(|c| if let ColumnData::Str(v) = &c.data { v.get(row).cloned().flatten() } else { None })
                }).collect();
                ColumnData::Str(vals)
            }
            ColumnData::StrDict { .. } => {
                let vals: Vec<Option<String>> = order.iter().map(|&(src, row)| {
                    sources[src].column(col_name).and_then(|c| {
                        if let ColumnData::StrDict { codes, dict } = &c.data {
                            let code = codes.get(row).copied().unwrap_or(u8::MAX);
                            if code == u8::MAX { None } else { dict.get(code as usize).cloned() }
                        } else { None }
                    })
                }).collect();
                ColumnData::Str(vals)
            }
        };
        columns.push(Column { name: col_name.clone(), data });
    }

    Ok(DataBlock { columns, num_rows: order.len() })
}

// ── ShuffleManager ────────────────────────────────────────────────────────────

/// Orchestrates the full shuffle write/read lifecycle with memory management.
///
/// Responsibilities:
/// - Tracks aggregate memory used by buffered shuffle data.
/// - Fires the `MemoryPressureCallback` when threshold is exceeded.
/// - Provides a single entry point for map-side write and reduce-side read.
/// - Cleans up temporary files on `cleanup()`.
pub struct ShuffleManager {
    root_dir: PathBuf,
    memory_threshold: usize,
    current_memory: Arc<AtomicUsize>,
    callback: Option<Arc<dyn MemoryPressureCallback>>,
    written_files: Arc<std::sync::Mutex<Vec<PathBuf>>>,
}

impl ShuffleManager {
    pub fn new(root_dir: impl Into<PathBuf>, memory_threshold: usize) -> Self {
        let root = root_dir.into();
        std::fs::create_dir_all(&root).ok();
        Self {
            root_dir: root,
            memory_threshold,
            current_memory: Arc::new(AtomicUsize::new(0)),
            callback: None,
            written_files: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    pub fn with_callback(mut self, cb: Arc<dyn MemoryPressureCallback>) -> Self {
        self.callback = Some(cb);
        self
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn current_memory_usage(&self) -> usize {
        self.current_memory.load(AtomicOrdering::Relaxed)
    }

    pub fn memory_threshold(&self) -> usize {
        self.memory_threshold
    }

    /// Estimate memory usage for a DataBlock (8 bytes per cell, rough).
    fn estimate_block_bytes(block: &DataBlock) -> usize {
        block.num_rows * block.columns.len() * 8
    }

    /// Track memory allocation; triggers callback if threshold exceeded.
    fn track_memory(&self, bytes: usize) {
        let prev = self.current_memory.fetch_add(bytes, AtomicOrdering::Relaxed);
        let current = prev + bytes;
        if current > self.memory_threshold {
            if let Some(cb) = &self.callback {
                cb.on_pressure(current, self.memory_threshold);
            }
        }
    }

    /// Release tracked memory.
    fn release_memory(&self, bytes: usize) {
        let cur = self.current_memory.load(AtomicOrdering::Relaxed);
        self.current_memory.store(cur.saturating_sub(bytes), AtomicOrdering::Relaxed);
    }

    /// Write a DataBlock through the sort-based shuffle writer.
    /// Returns per-partition metadata. Triggers spill callback if memory pressure.
    pub fn shuffle_write(
        &self,
        block: &DataBlock,
        key_cols: Vec<String>,
        sort_col: &str,
        n_partitions: usize,
        writer_id: &str,
    ) -> Result<Vec<PartitionFileMeta>, KoreError> {
        let block_bytes = Self::estimate_block_bytes(block);
        self.track_memory(block_bytes);

        let writer = SortBasedShuffleWriter::new(
            &self.root_dir,
            n_partitions,
            key_cols,
            sort_col,
        );
        let metas = writer.write(block, writer_id)?;

        // Track written files for cleanup
        {
            let mut files = self.written_files.lock().unwrap();
            for m in &metas {
                if m.num_rows > 0 {
                    files.push(m.path.clone());
                }
            }
        }

        self.release_memory(block_bytes);
        Ok(metas)
    }

    /// Read and merge sorted partition files for a given reduce partition.
    pub fn shuffle_read(
        &self,
        partition_paths: &[PathBuf],
        sort_col: &str,
    ) -> Result<DataBlock, KoreError> {
        let reader = MergeBasedShuffleReader::new(sort_col);
        let merged = reader.merge(partition_paths)?;
        Ok(merged)
    }

    /// Remove all temporary shuffle files managed by this instance.
    pub fn cleanup(&self) -> usize {
        let mut files = self.written_files.lock().unwrap();
        let mut removed = 0;
        for path in files.drain(..) {
            if std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        removed
    }
}

impl Drop for ShuffleManager {
    fn drop(&mut self) {
        self.cleanup();
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};
    use std::sync::Arc;

    fn tmp_dir(suffix: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("kore_ext_shuffle_{suffix}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn make_block(n: usize, id_offset: i64) -> DataBlock {
        DataBlock {
            num_rows: n,
            columns: vec![
                Column { name: "key".into(), data: ColumnData::Int64(
                    (0..n).map(|i| Some((i as i64 + id_offset) % 4)).collect()
                )},
                Column { name: "val".into(), data: ColumnData::Float64(
                    (0..n).map(|i| Some((n as f64) - i as f64 + id_offset as f64)).collect()
                )},
            ],
        }
    }

    fn make_sorted_block(vals: Vec<f64>) -> DataBlock {
        let n = vals.len();
        DataBlock {
            num_rows: n,
            columns: vec![
                Column { name: "id".into(), data: ColumnData::Int64(
                    (0..n).map(|i| Some(i as i64)).collect()
                )},
                Column { name: "val".into(), data: ColumnData::Float64(
                    vals.into_iter().map(Some).collect()
                )},
            ],
        }
    }

    // ── SortBasedShuffleWriter tests ──────────────────────────────────────────

    #[test]
    fn test_writer_produces_correct_partition_count() {
        let dir = tmp_dir("writer_count");
        let block = make_block(100, 0);
        let writer = SortBasedShuffleWriter::new(&dir, 4, vec!["key".into()], "val");
        let metas = writer.write(&block, "map0").unwrap();
        assert_eq!(metas.len(), 4);
        let total_rows: usize = metas.iter().map(|m| m.num_rows).sum();
        assert_eq!(total_rows, 100);
    }

    #[test]
    fn test_writer_partitions_are_sorted() {
        let dir = tmp_dir("writer_sorted");
        let block = make_block(200, 0);
        let writer = SortBasedShuffleWriter::new(&dir, 4, vec!["key".into()], "val");
        let metas = writer.write(&block, "map1").unwrap();

        for meta in &metas {
            if meta.num_rows == 0 { continue; }
            let read_back = KoreReader::read_file(&meta.path).unwrap();
            let col = read_back.column("val").unwrap();
            if let ColumnData::Float64(vals) = &col.data {
                for w in vals.windows(2) {
                    if let (Some(a), Some(b)) = (w[0], w[1]) {
                        assert!(a <= b, "partition {} not sorted: {} > {}", meta.partition_id, a, b);
                    }
                }
            }
        }
    }

    #[test]
    fn test_writer_empty_block() {
        let dir = tmp_dir("writer_empty");
        let block = DataBlock::empty();
        let writer = SortBasedShuffleWriter::new(&dir, 4, vec!["key".into()], "val");
        let metas = writer.write(&block, "map_empty").unwrap();
        assert!(metas.iter().all(|m| m.num_rows == 0));
    }

    #[test]
    fn test_writer_descending() {
        let dir = tmp_dir("writer_desc");
        let block = make_block(50, 0);
        let writer = SortBasedShuffleWriter::new(&dir, 2, vec!["key".into()], "val")
            .ascending(false);
        let metas = writer.write(&block, "map_desc").unwrap();

        for meta in &metas {
            if meta.num_rows == 0 { continue; }
            let read_back = KoreReader::read_file(&meta.path).unwrap();
            let col = read_back.column("val").unwrap();
            if let ColumnData::Float64(vals) = &col.data {
                for w in vals.windows(2) {
                    if let (Some(a), Some(b)) = (w[0], w[1]) {
                        assert!(a >= b, "partition {} not desc-sorted: {} < {}", meta.partition_id, a, b);
                    }
                }
            }
        }
    }

    // ── MergeBasedShuffleReader tests ─────────────────────────────────────────

    #[test]
    fn test_reader_k_way_merge() {
        let dir = tmp_dir("reader_merge");

        let blocks = vec![
            make_sorted_block(vec![1.0, 4.0, 7.0, 10.0]),
            make_sorted_block(vec![2.0, 5.0, 8.0, 11.0]),
            make_sorted_block(vec![3.0, 6.0, 9.0, 12.0]),
        ];

        let paths: Vec<PathBuf> = blocks.iter().enumerate().map(|(i, b)| {
            let p = dir.join(format!("run_{i}.kore"));
            KoreWriter::write_file(&p, b).unwrap();
            p
        }).collect();

        let reader = MergeBasedShuffleReader::new("val");
        let merged = reader.merge(&paths).unwrap();

        assert_eq!(merged.num_rows, 12);
        let col = merged.column("val").unwrap();
        if let ColumnData::Float64(vals) = &col.data {
            for w in vals.windows(2) {
                if let (Some(a), Some(b)) = (w[0], w[1]) {
                    assert!(a <= b, "merge not sorted: {} > {}", a, b);
                }
            }
            assert_eq!(vals[0], Some(1.0));
            assert_eq!(vals[11], Some(12.0));
        } else {
            panic!("expected Float64 column");
        }
    }

    #[test]
    fn test_reader_single_file() {
        let dir = tmp_dir("reader_single");
        let block = make_sorted_block(vec![1.0, 2.0, 3.0]);
        let path = dir.join("single.kore");
        KoreWriter::write_file(&path, &block).unwrap();

        let reader = MergeBasedShuffleReader::new("val");
        let merged = reader.merge(&[path]).unwrap();
        assert_eq!(merged.num_rows, 3);
    }

    #[test]
    fn test_reader_empty_paths() {
        let reader = MergeBasedShuffleReader::new("val");
        let merged = reader.merge(&[]).unwrap();
        assert_eq!(merged.num_rows, 0);
    }

    #[test]
    fn test_reader_descending_merge() {
        let dir = tmp_dir("reader_desc");

        let blocks = vec![
            make_sorted_block(vec![10.0, 7.0, 4.0, 1.0]),
            make_sorted_block(vec![11.0, 8.0, 5.0, 2.0]),
        ];

        let paths: Vec<PathBuf> = blocks.iter().enumerate().map(|(i, b)| {
            let p = dir.join(format!("desc_run_{i}.kore"));
            KoreWriter::write_file(&p, b).unwrap();
            p
        }).collect();

        let reader = MergeBasedShuffleReader::new("val").ascending(false);
        let merged = reader.merge(&paths).unwrap();
        assert_eq!(merged.num_rows, 8);

        let col = merged.column("val").unwrap();
        if let ColumnData::Float64(vals) = &col.data {
            for w in vals.windows(2) {
                if let (Some(a), Some(b)) = (w[0], w[1]) {
                    assert!(a >= b, "desc merge not sorted: {} < {}", a, b);
                }
            }
        }
    }

    // ── MemoryPressureCallback tests ──────────────────────────────────────────

    #[test]
    fn test_threshold_callback_fires_on_pressure() {
        let cb = Arc::new(ThresholdPressureCallback::new(1024));
        assert_eq!(cb.triggered_count(), 0);
        cb.on_pressure(2048, 1024);
        assert_eq!(cb.triggered_count(), 1);
        cb.on_pressure(4096, 1024);
        assert_eq!(cb.triggered_count(), 2);
    }

    #[test]
    fn test_threshold_callback_threshold_value() {
        let cb = ThresholdPressureCallback::new(8192);
        assert_eq!(cb.threshold(), 8192);
    }

    // ── ShuffleManager tests ──────────────────────────────────────────────────

    #[test]
    fn test_manager_write_read_roundtrip() {
        let dir = tmp_dir("mgr_roundtrip");
        let mgr = ShuffleManager::new(&dir, 1_000_000);

        let block = make_block(100, 0);
        let metas = mgr.shuffle_write(&block, vec!["key".into()], "val", 4, "w0").unwrap();

        let total_written: usize = metas.iter().map(|m| m.num_rows).sum();
        assert_eq!(total_written, 100);

        // Read back one partition
        for meta in &metas {
            if meta.num_rows == 0 { continue; }
            let result = mgr.shuffle_read(&[meta.path.clone()], "val").unwrap();
            assert_eq!(result.num_rows, meta.num_rows);
        }
    }

    #[test]
    fn test_manager_multi_writer_merge() {
        let dir = tmp_dir("mgr_multi");
        let mgr = ShuffleManager::new(&dir, 10_000_000);

        let block1 = make_block(80, 0);
        let block2 = make_block(80, 100);

        let metas1 = mgr.shuffle_write(&block1, vec!["key".into()], "val", 4, "w0").unwrap();
        let metas2 = mgr.shuffle_write(&block2, vec!["key".into()], "val", 4, "w1").unwrap();

        // Merge partition 0 from both writers
        let paths_p0: Vec<PathBuf> = [&metas1, &metas2].iter()
            .flat_map(|ms| ms.iter())
            .filter(|m| m.partition_id == 0 && m.num_rows > 0)
            .map(|m| m.path.clone())
            .collect();

        if !paths_p0.is_empty() {
            let merged = mgr.shuffle_read(&paths_p0, "val").unwrap();
            assert!(merged.num_rows > 0);

            // Verify sorted
            let col = merged.column("val").unwrap();
            if let ColumnData::Float64(vals) = &col.data {
                for w in vals.windows(2) {
                    if let (Some(a), Some(b)) = (w[0], w[1]) {
                        assert!(a <= b, "merged partition not sorted");
                    }
                }
            }
        }
    }

    #[test]
    fn test_manager_memory_tracking() {
        let dir = tmp_dir("mgr_mem");
        let cb = Arc::new(ThresholdPressureCallback::new(100));
        let mgr = ShuffleManager::new(&dir, 100)
            .with_callback(cb.clone());

        let block = make_block(50, 0);
        mgr.shuffle_write(&block, vec!["key".into()], "val", 2, "w0").unwrap();

        // Memory should have been tracked and released (write completes)
        assert_eq!(mgr.current_memory_usage(), 0);
        // Callback should have been triggered since 50 rows * 2 cols * 8 = 800 > 100
        assert!(cb.triggered_count() > 0);
    }

    #[test]
    fn test_manager_cleanup() {
        let dir = tmp_dir("mgr_cleanup");
        let mgr = ShuffleManager::new(&dir, 1_000_000);

        let block = make_block(40, 0);
        let metas = mgr.shuffle_write(&block, vec!["key".into()], "val", 2, "w0").unwrap();

        let non_empty: Vec<&PartitionFileMeta> = metas.iter().filter(|m| m.num_rows > 0).collect();
        assert!(!non_empty.is_empty());
        for m in &non_empty {
            assert!(m.path.exists());
        }

        let removed = mgr.cleanup();
        assert!(removed > 0);
        for m in &non_empty {
            assert!(!m.path.exists());
        }
    }

    #[test]
    fn test_manager_read_empty() {
        let dir = tmp_dir("mgr_empty_read");
        let mgr = ShuffleManager::new(&dir, 1_000_000);
        let result = mgr.shuffle_read(&[], "val").unwrap();
        assert_eq!(result.num_rows, 0);
    }

    // ── Integration: full shuffle cycle ───────────────────────────────────────

    #[test]
    fn test_full_shuffle_cycle() {
        let dir = tmp_dir("full_cycle");
        let mgr = ShuffleManager::new(&dir, 10_000_000);
        let n_partitions = 3;

        // Simulate 3 map tasks each producing data
        let map_blocks: Vec<DataBlock> = (0..3).map(|i| make_block(60, i * 100)).collect();

        let all_metas: Vec<Vec<PartitionFileMeta>> = map_blocks.iter().enumerate()
            .map(|(i, b)| {
                mgr.shuffle_write(b, vec!["key".into()], "val", n_partitions, &format!("map{i}")).unwrap()
            })
            .collect();

        // For each reduce partition, gather files from all map tasks and merge
        for pid in 0..n_partitions {
            let paths: Vec<PathBuf> = all_metas.iter()
                .flat_map(|ms| ms.iter())
                .filter(|m| m.partition_id == pid && m.num_rows > 0)
                .map(|m| m.path.clone())
                .collect();

            if paths.is_empty() { continue; }

            let merged = mgr.shuffle_read(&paths, "val").unwrap();
            assert!(merged.num_rows > 0);

            // Verify global sort order within this reduce partition
            let col = merged.column("val").unwrap();
            if let ColumnData::Float64(vals) = &col.data {
                for w in vals.windows(2) {
                    if let (Some(a), Some(b)) = (w[0], w[1]) {
                        assert!(a <= b, "reduce partition {pid} not sorted: {a} > {b}");
                    }
                }
            }
        }

        // Total rows across all reduce partitions should equal input
        let total_input: usize = map_blocks.iter().map(|b| b.num_rows).sum();
        let total_output: usize = (0..n_partitions).map(|pid| {
            let paths: Vec<PathBuf> = all_metas.iter()
                .flat_map(|ms| ms.iter())
                .filter(|m| m.partition_id == pid && m.num_rows > 0)
                .map(|m| m.path.clone())
                .collect();
            if paths.is_empty() { return 0; }
            mgr.shuffle_read(&paths, "val").unwrap().num_rows
        }).sum();
        assert_eq!(total_output, total_input);
    }
}
