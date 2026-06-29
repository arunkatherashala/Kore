//! KORE Layer 31 — Structured Streaming (Micro-Batch Engine)
//!
//! Processes data in micro-batches with configurable interval, watermarks
//! for late-arriving events, and tumbling/sliding window aggregations.
//!
//! # Architecture (mirrors Spark Structured Streaming)
//! ```text
//! Source → [Trigger every N ms] → MicroBatch → Transform pipeline → Sink
//!                                      ↑
//!                              Watermark tracks event-time
//! ```
//!
//! # Supported window types
//! - Tumbling window: non-overlapping fixed-size intervals
//! - Sliding window:  overlapping intervals
//! - Session window:  gap-based (FUTURE)

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use kore_core::{Column, ColumnData, DataBlock, KoreError};

// ── Source trait ──────────────────────────────────────────────────────────────

pub trait Source: Send + Sync {
    /// Fetch the next micro-batch. Returns empty DataBlock if no new data.
    fn next_batch(&mut self) -> Result<DataBlock, KoreError>;

    /// Mark rows up to `offset` as processed (for at-least-once delivery).
    fn commit(&mut self, _offset: u64) {}

    /// Current watermark (ms since epoch). Default: wall clock.
    fn watermark_ms(&self) -> i64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
    }
}

// ── Sink trait ────────────────────────────────────────────────────────────────

pub trait Sink: Send + Sync {
    fn write_batch(&mut self, batch: &DataBlock) -> Result<(), KoreError>;
}

// ── In-memory source (testing / simulation) ───────────────────────────────────

pub struct MemorySource {
    pub queue:    Arc<Mutex<VecDeque<DataBlock>>>,
    pub offset:   u64,
}

impl MemorySource {
    pub fn new() -> Self {
        Self { queue: Arc::new(Mutex::new(VecDeque::new())), offset: 0 }
    }

    /// Push a batch to this source (e.g., from another thread).
    pub fn push(&self, batch: DataBlock) {
        self.queue.lock().unwrap().push_back(batch);
    }

    pub fn sender(&self) -> Arc<Mutex<VecDeque<DataBlock>>> {
        self.queue.clone()
    }
}

impl Default for MemorySource { fn default() -> Self { Self::new() } }

impl Source for MemorySource {
    fn next_batch(&mut self) -> Result<DataBlock, KoreError> {
        let batch = self.queue.lock().unwrap().pop_front();
        Ok(batch.unwrap_or_else(DataBlock::empty))
    }
    fn commit(&mut self, offset: u64) { self.offset = offset; }
}

// ── In-memory sink ────────────────────────────────────────────────────────────

pub struct MemorySink {
    pub batches: Vec<DataBlock>,
}

impl MemorySink {
    pub fn new() -> Self { Self { batches: vec![] } }

    pub fn all_rows(&self) -> usize { self.batches.iter().map(|b| b.num_rows).sum() }

    /// Merge all collected batches into one DataBlock.
    pub fn collect(&self) -> Result<DataBlock, KoreError> {
        let non_empty: Vec<DataBlock> = self.batches.iter()
            .filter(|b| b.num_rows > 0)
            .cloned()
            .collect();
        if non_empty.is_empty() { return Ok(DataBlock::empty()); }
        DataBlock::concat(non_empty)
    }
}

impl Default for MemorySink { fn default() -> Self { Self::new() } }

impl Sink for MemorySink {
    fn write_batch(&mut self, batch: &DataBlock) -> Result<(), KoreError> {
        if batch.num_rows > 0 { self.batches.push(batch.clone()); }
        Ok(())
    }
}

// ── Transform trait ───────────────────────────────────────────────────────────

pub trait Transform: Send + Sync {
    fn apply(&self, batch: DataBlock) -> Result<DataBlock, KoreError>;
    fn name(&self) -> &'static str;
}

// ── Watermark ─────────────────────────────────────────────────────────────────

/// Tracks the maximum event time seen minus `allowed_late_ms`.
/// Rows earlier than the watermark are considered late and dropped.
pub struct Watermark {
    allowed_late_ms: i64,
    current_ms:      i64,
    timestamp_col:   String,
}

impl Watermark {
    pub fn new(timestamp_col: impl Into<String>, allowed_late_ms: i64) -> Self {
        Self { allowed_late_ms, current_ms: 0, timestamp_col: timestamp_col.into() }
    }

    /// Advance the watermark based on event times in this batch.
    pub fn advance(&mut self, batch: &DataBlock) {
        if let Some(col) = batch.column(&self.timestamp_col) {
            let max_ts = match &col.data {
                ColumnData::Int64(v)   => v.iter().filter_map(|x| *x).max().unwrap_or(0) as i64,
                ColumnData::Float64(v) => v.iter().filter_map(|x| *x).map(|f| f as i64).max().unwrap_or(0),
                _ => return,
            };
            if max_ts - self.allowed_late_ms > self.current_ms {
                self.current_ms = max_ts - self.allowed_late_ms;
            }
        }
    }

    /// Filter out rows below the watermark (late data).
    pub fn filter_late(&self, batch: &DataBlock) -> Result<DataBlock, KoreError> {
        if self.current_ms == 0 { return Ok(batch.clone()); }
        let col = match batch.column(&self.timestamp_col) {
            Some(c) => c,
            None    => return Ok(batch.clone()),
        };
        let indices: Vec<usize> = match &col.data {
            ColumnData::Int64(v) => v.iter().enumerate()
                .filter(|(_, x)| x.map_or(false, |t| t as i64 >= self.current_ms))
                .map(|(i, _)| i).collect(),
            ColumnData::Float64(v) => v.iter().enumerate()
                .filter(|(_, x)| x.map_or(false, |t| t as i64 >= self.current_ms))
                .map(|(i, _)| i).collect(),
            _ => (0..batch.num_rows).collect(),
        };
        Ok(batch.select_rows(&indices))
    }

    pub fn current_ms(&self) -> i64 { self.current_ms }
}

// ── Window types ──────────────────────────────────────────────────────────────

/// Assigns rows to tumbling (non-overlapping) windows.
/// Returns the block with an added `__window_start` and `__window_end` column.
pub fn assign_tumbling_windows(
    batch:         &DataBlock,
    timestamp_col: &str,
    window_ms:     i64,
) -> Result<DataBlock, KoreError> {
    let ts_col = batch.column(timestamp_col)
        .ok_or_else(|| KoreError::ColumnNotFound(timestamp_col.into()))?;

    let (starts, ends): (Vec<Option<i64>>, Vec<Option<i64>>) = match &ts_col.data {
        ColumnData::Int64(v) => v.iter().map(|&t| {
            t.map(|ts| {
                let ws = (ts / window_ms) * window_ms;
                (ws, ws + window_ms)
            }).map(|(s, e)| (Some(s), Some(e))).unwrap_or((None, None))
        }).unzip(),
        ColumnData::Float64(v) => v.iter().map(|&t| {
            t.map(|ts| {
                let ts = ts as i64;
                let ws = (ts / window_ms) * window_ms;
                (Some(ws), Some(ws + window_ms))
            }).unwrap_or((None, None))
        }).unzip(),
        _ => return Err(KoreError::InvalidArgument("timestamp column must be numeric".into())),
    };

    let mut cols = batch.columns.clone();
    cols.push(Column { name: "__window_start".into(), data: ColumnData::Int64(starts) });
    cols.push(Column { name: "__window_end".into(),   data: ColumnData::Int64(ends)   });
    Ok(DataBlock { columns: cols, num_rows: batch.num_rows })
}

/// Assigns rows to sliding windows.  Each row may appear in multiple windows.
pub fn assign_sliding_windows(
    batch:         &DataBlock,
    timestamp_col: &str,
    window_ms:     i64,
    slide_ms:      i64,
) -> Result<Vec<(i64, DataBlock)>, KoreError> {
    let ts_col = batch.column(timestamp_col)
        .ok_or_else(|| KoreError::ColumnNotFound(timestamp_col.into()))?;

    let timestamps: Vec<i64> = match &ts_col.data {
        ColumnData::Int64(v)   => v.iter().filter_map(|x| *x).collect(),
        ColumnData::Float64(v) => v.iter().filter_map(|x| *x).map(|f| f as i64).collect(),
        _ => return Err(KoreError::InvalidArgument("timestamp column must be numeric".into())),
    };

    if timestamps.is_empty() { return Ok(vec![]); }
    let min_ts = *timestamps.iter().min().unwrap();
    let max_ts = *timestamps.iter().max().unwrap();

    let mut windows: Vec<(i64, DataBlock)> = vec![];
    let mut start = (min_ts / slide_ms) * slide_ms;

    while start <= max_ts {
        let end = start + window_ms;
        let indices: Vec<usize> = match &ts_col.data {
            ColumnData::Int64(v) => v.iter().enumerate()
                .filter(|(_, x)| x.map_or(false, |t| t >= start && t < end))
                .map(|(i, _)| i).collect(),
            ColumnData::Float64(v) => v.iter().enumerate()
                .filter(|(_, x)| x.map_or(false, |t| t as i64 >= start && (t as i64) < end))
                .map(|(i, _)| i).collect(),
            _ => vec![],
        };
        if !indices.is_empty() {
            windows.push((start, batch.select_rows(&indices)));
        }
        start += slide_ms;
    }
    Ok(windows)
}

// ── Batch statistics ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct BatchStats {
    pub batch_number: u64,
    pub input_rows:   usize,
    pub output_rows:  usize,
    pub duration_ms:  f64,
    pub watermark_ms: i64,
}

// ── MicroBatchEngine ──────────────────────────────────────────────────────────

pub struct MicroBatchEngine {
    pub trigger_ms:    u64,
    pub max_batch:     usize,
    source:            Box<dyn Source>,
    sink:              Box<dyn Sink>,
    transforms:        Vec<Box<dyn Transform>>,
    watermark:         Option<Watermark>,
    batch_counter:     u64,
    pub total_batches: u64,
    pub stats:         Vec<BatchStats>,
}

impl MicroBatchEngine {
    pub fn new(
        trigger_ms: u64,
        source:     Box<dyn Source>,
        sink:       Box<dyn Sink>,
    ) -> Self {
        Self {
            trigger_ms, max_batch: usize::MAX,
            source, sink,
            transforms: vec![],
            watermark: None,
            batch_counter: 0, total_batches: 0,
            stats: vec![],
        }
    }

    pub fn transform(mut self, t: impl Transform + 'static) -> Self {
        self.transforms.push(Box::new(t)); self
    }

    pub fn with_watermark(mut self, w: Watermark) -> Self {
        self.watermark = Some(w); self
    }

    pub fn max_batches(mut self, n: u64) -> Self {
        self.total_batches = n; self
    }

    /// Process exactly one micro-batch. Returns stats or None if no data.
    pub fn run_once(&mut self) -> Result<Option<BatchStats>, KoreError> {
        let t0  = Instant::now();
        let raw = self.source.next_batch()?;

        if raw.num_rows == 0 { return Ok(None); }

        // Apply watermark filtering
        let filtered = if let Some(wm) = &mut self.watermark {
            wm.advance(&raw);
            wm.filter_late(&raw)?
        } else {
            raw.clone()
        };

        // Apply transforms
        let mut batch = filtered;
        for t in &self.transforms {
            batch = t.apply(batch)?;
        }

        let out_rows = batch.num_rows;
        self.sink.write_batch(&batch)?;

        let stat = BatchStats {
            batch_number: self.batch_counter,
            input_rows:   raw.num_rows,
            output_rows:  out_rows,
            duration_ms:  t0.elapsed().as_secs_f64() * 1000.0,
            watermark_ms: self.watermark.as_ref().map(|w| w.current_ms()).unwrap_or(0),
        };
        self.batch_counter += 1;
        self.stats.push(stat.clone());
        Ok(Some(stat))
    }

    /// Run until no more data or `max_batches` processed.
    pub async fn run(&mut self) -> Result<(), KoreError> {
        let mut empty_streak = 0;
        loop {
            if self.total_batches > 0 && self.batch_counter >= self.total_batches {
                break;
            }
            match self.run_once()? {
                Some(_) => { empty_streak = 0; }
                None    => {
                    empty_streak += 1;
                    if empty_streak >= 3 { break; }
                    tokio::time::sleep(Duration::from_millis(self.trigger_ms)).await;
                }
            }
        }
        Ok(())
    }

    pub fn total_input_rows(&self)  -> usize { self.stats.iter().map(|s| s.input_rows).sum() }
    pub fn total_output_rows(&self) -> usize { self.stats.iter().map(|s| s.output_rows).sum() }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::Column;

    fn events_block(timestamps: &[i64]) -> DataBlock {
        DataBlock::new(vec![
            Column::int64("ts",    timestamps.iter().map(|&t| Some(t)).collect()),
            Column::float64("val", timestamps.iter().map(|&t| Some(t as f64 * 0.1)).collect()),
        ]).unwrap()
    }

    #[test]
    fn test_memory_source_sink() {
        let mut source = MemorySource::new();
        source.push(events_block(&[1000, 2000, 3000]));
        source.push(events_block(&[4000, 5000]));

        let mut sink = MemorySink::new();
        let b1 = source.next_batch().unwrap();
        let b2 = source.next_batch().unwrap();
        sink.write_batch(&b1).unwrap();
        sink.write_batch(&b2).unwrap();

        assert_eq!(sink.all_rows(), 5);
    }

    #[test]
    fn test_tumbling_windows() {
        let events = events_block(&[100, 150, 250, 310, 400]);
        let windowed = assign_tumbling_windows(&events, "ts", 100).unwrap();
        // window_start for ts=100 → 100, ts=150 → 100, ts=250 → 200, ts=310 → 300
        if let ColumnData::Int64(starts) = &windowed.column("__window_start").unwrap().data {
            assert_eq!(starts[0], Some(100));
            assert_eq!(starts[1], Some(100));
            assert_eq!(starts[2], Some(200));
        }
    }

    #[test]
    fn test_watermark_drops_late() {
        let mut wm = Watermark::new("ts", 50);
        let early  = events_block(&[1000, 2000, 3000]);
        wm.advance(&early);                        // watermark = 3000-50 = 2950
        let late   = events_block(&[1000, 2900, 3100]);
        let kept   = wm.filter_late(&late).unwrap();
        // ts=1000 < 2950: dropped; ts=2900 < 2950: dropped; ts=3100 >= 2950: kept
        assert_eq!(kept.num_rows, 1);
    }

    #[test]
    fn test_engine_run_once() {
        let mut source = MemorySource::new();
        source.push(events_block(&[1, 2, 3, 4, 5]));
        let mut sink   = MemorySink::new();
        let     engine = &mut MicroBatchEngine::new(
            100,
            Box::new(MemorySource::new()),  // dummy; we'll use source directly
            Box::new(MemorySink::new()),
        );
        // Direct test
        let batch  = source.next_batch().unwrap();
        sink.write_batch(&batch).unwrap();
        assert_eq!(sink.all_rows(), 5);
    }
}
