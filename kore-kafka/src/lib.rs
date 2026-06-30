//! KORE Layer 72 — Kafka-compatible streaming source/sink (simulated).
//!
//! No real Kafka broker is required; the consumer generates synthetic batches
//! and the producer serialises to JSON.  This gives a fully compile-time-safe
//! streaming layer that can later be wired to `rdkafka` without API changes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use kore_core::{Column, ColumnData, DataBlock, Value};
use serde_json::{json, Value as JValue};

// ─── Configuration ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct KafkaTopic {
    pub name:       String,
    pub partitions: usize,
}

#[derive(Debug, Clone)]
pub struct KafkaConfig {
    pub brokers:  Vec<String>,
    pub group_id: String,
}

impl KafkaConfig {
    pub fn new(brokers: Vec<String>, group_id: impl Into<String>) -> Self {
        Self { brokers, group_id: group_id.into() }
    }
    pub fn local() -> Self {
        Self::new(vec!["localhost:9092".into()], "kore-group")
    }
}

// ─── Stream batch ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct StreamBatch {
    pub topic:     String,
    pub partition: usize,
    pub offset:    u64,
    pub data:      DataBlock,
}

impl StreamBatch {
    pub fn new(topic: impl Into<String>, partition: usize, offset: u64, data: DataBlock) -> Self {
        Self { topic: topic.into(), partition, offset, data }
    }
}

// ─── Consumer ─────────────────────────────────────────────────────────────────

pub struct KafkaConsumer {
    config:     KafkaConfig,
    topics:     Vec<String>,
    // simulated: track next offset per (topic, partition)
    offsets:    Arc<Mutex<HashMap<(String, usize), u64>>>,
    batch_size: usize,
}

impl KafkaConsumer {
    pub fn new(config: KafkaConfig, topics: Vec<String>) -> Self {
        Self {
            config,
            topics,
            offsets: Arc::new(Mutex::new(HashMap::new())),
            batch_size: 1_000,
        }
    }

    pub fn with_batch_size(mut self, n: usize) -> Self {
        self.batch_size = n;
        self
    }

    /// Simulated poll: generates a synthetic DataBlock per topic.
    pub fn poll_batch(&self, _timeout_ms: u64) -> Vec<StreamBatch> {
        let now_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as i64;

        let mut batches = Vec::new();
        let mut offsets = self.offsets.lock().unwrap();

        for topic in &self.topics {
            let partition = 0usize;
            let entry = offsets.entry((topic.clone(), partition)).or_insert(0);
            let offset_start = *entry;

            let n = self.batch_size;
            let ids: Vec<Option<i64>> = (0..n)
                .map(|i| Some((offset_start + i as u64) as i64))
                .collect();
            let ts: Vec<Option<i64>> = (0..n)
                .map(|i| Some(now_us + i as i64 * 1_000))
                .collect();
            let vals: Vec<Option<f64>> = (0..n)
                .map(|i| Some((i as f64 * 3.14) % 100.0))
                .collect();

            let block = DataBlock::new(vec![
                Column::int64("event_id",  ids),
                Column::int64("event_ts",  ts),
                Column::float64("value",   vals),
            ]).expect("valid schema");

            *entry += n as u64;
            batches.push(StreamBatch::new(topic.clone(), partition, offset_start, block));
        }

        batches
    }

    pub fn committed_offset(&self, topic: &str, partition: usize) -> u64 {
        self.offsets.lock().unwrap()
            .get(&(topic.to_string(), partition))
            .copied()
            .unwrap_or(0)
    }
}

// ─── Producer ─────────────────────────────────────────────────────────────────

pub struct KafkaProducer {
    pub config: KafkaConfig,
    // simulated: track offsets
    offsets: Arc<Mutex<HashMap<String, u64>>>,
}

impl KafkaProducer {
    pub fn new(config: KafkaConfig) -> Self {
        Self { config, offsets: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Serialise `data` to JSON and simulate sending to a topic.
    /// Returns the new offset on success.
    pub fn send(&self, topic: &str, data: &DataBlock) -> Result<u64, String> {
        let _json = datablock_to_ndjson(data);
        let mut offsets = self.offsets.lock().unwrap();
        let entry = offsets.entry(topic.to_string()).or_insert(0);
        *entry += data.num_rows as u64;
        Ok(*entry)
    }

    pub fn current_offset(&self, topic: &str) -> u64 {
        self.offsets.lock().unwrap().get(topic).copied().unwrap_or(0)
    }
}

fn datablock_to_ndjson(block: &DataBlock) -> String {
    let mut out = String::new();
    for r in 0..block.num_rows {
        let mut obj = serde_json::Map::new();
        for col in &block.columns {
            let v = match col.data.get_value(r) {
                Value::Int(i)   => JValue::Number(i.into()),
                Value::Float(f) => serde_json::Number::from_f64(f)
                    .map(JValue::Number).unwrap_or(JValue::Null),
                Value::Bool(b)  => JValue::Bool(b),
                Value::Str(s)   => JValue::String(s),
                Value::Null     => JValue::Null,
            };
            obj.insert(col.name.clone(), v);
        }
        out.push_str(&serde_json::to_string(&JValue::Object(obj)).unwrap_or_default());
        out.push('\n');
    }
    out
}

// ─── Stream processor ─────────────────────────────────────────────────────────

pub struct StreamProcessor {
    pub input_topic:  String,
    pub output_topic: String,
    transform: Box<dyn Fn(DataBlock) -> DataBlock + Send + Sync>,
}

impl StreamProcessor {
    pub fn new(
        input:  impl Into<String>,
        output: impl Into<String>,
        transform: impl Fn(DataBlock) -> DataBlock + Send + Sync + 'static,
    ) -> Self {
        Self {
            input_topic:  input.into(),
            output_topic: output.into(),
            transform:    Box::new(transform),
        }
    }

    pub fn run_batch(&self, batch: StreamBatch) -> StreamBatch {
        let out_data = (self.transform)(batch.data);
        StreamBatch {
            topic:     self.output_topic.clone(),
            partition: batch.partition,
            offset:    batch.offset,
            data:      out_data,
        }
    }
}

// ─── Watermark ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WatermarkTracker {
    pub max_event_time:    i64,
    pub watermark_lag_secs: i64,
}

impl WatermarkTracker {
    pub fn new(lag_secs: i64) -> Self {
        Self { max_event_time: 0, watermark_lag_secs: lag_secs }
    }

    pub fn update(&mut self, event_ts: i64) {
        if event_ts > self.max_event_time {
            self.max_event_time = event_ts;
        }
    }

    /// Current watermark (μs)
    pub fn watermark(&self) -> i64 {
        self.max_event_time - self.watermark_lag_secs * 1_000_000
    }
}

// ─── Tumbling window aggregation ──────────────────────────────────────────────

/// Aggregate `agg_col` (Float64/Int64) per `group_col` within fixed-size
/// tumbling windows (defined by `window_secs`).
///
/// Returns a DataBlock with columns: `window_start`, `group`, `count`, `sum`, `avg`.
pub fn tumbling_window(
    batches:     &[StreamBatch],
    window_secs: u64,
    group_col:   &str,
    agg_col:     &str,
) -> DataBlock {
    let window_us = window_secs as i64 * 1_000_000;

    // (window_start, group) → (count, sum)
    let mut agg: HashMap<(i64, String), (i64, f64)> = HashMap::new();

    for batch in batches {
        let data = &batch.data;
        let ts_col = data.column("event_ts");
        let grp_col = data.column(group_col);
        let val_col = data.column(agg_col);

        for r in 0..data.num_rows {
            let ts = ts_col
                .and_then(|c| if let Value::Int(i) = c.data.get_value(r) { Some(i) } else { None })
                .unwrap_or(0);
            let window_start = (ts / window_us) * window_us;

            let group = grp_col
                .map(|c| match c.data.get_value(r) {
                    Value::Str(s) => s,
                    Value::Int(i) => i.to_string(),
                    _             => "null".to_string(),
                })
                .unwrap_or_else(|| "all".to_string());

            let v = val_col
                .map(|c| match c.data.get_value(r) {
                    Value::Float(f) => f,
                    Value::Int(i)   => i as f64,
                    _               => 0.0,
                })
                .unwrap_or(0.0);

            let entry = agg.entry((window_start, group)).or_insert((0, 0.0));
            entry.0 += 1;
            entry.1 += v;
        }
    }

    let mut ws_col:  Vec<Option<i64>>   = Vec::new();
    let mut grp_col_out: Vec<Option<String>> = Vec::new();
    let mut cnt_col: Vec<Option<i64>>   = Vec::new();
    let mut sum_col: Vec<Option<f64>>   = Vec::new();
    let mut avg_col: Vec<Option<f64>>   = Vec::new();

    let mut entries: Vec<_> = agg.into_iter().collect();
    entries.sort_by_key(|((ws, g), _)| (*ws, g.clone()));

    for ((ws, g), (cnt, sum)) in entries {
        ws_col.push(Some(ws));
        grp_col_out.push(Some(g));
        cnt_col.push(Some(cnt));
        sum_col.push(Some(sum));
        avg_col.push(Some(if cnt > 0 { sum / cnt as f64 } else { 0.0 }));
    }

    DataBlock::new(vec![
        Column::int64("window_start", ws_col),
        Column::str_col("group",      grp_col_out),
        Column::int64("count",        cnt_col),
        Column::float64("sum",        sum_col),
        Column::float64("avg",        avg_col),
    ]).expect("valid schema")
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_consumer() -> KafkaConsumer {
        KafkaConsumer::new(KafkaConfig::local(), vec!["events".into()])
            .with_batch_size(1_000)
    }

    #[test]
    fn poll_five_batches() {
        let consumer = make_consumer();
        let mut total_rows = 0usize;
        for _ in 0..5 {
            let batches = consumer.poll_batch(100);
            assert_eq!(batches.len(), 1);
            total_rows += batches[0].data.num_rows;
        }
        assert_eq!(total_rows, 5_000);
    }

    #[test]
    fn offsets_advance() {
        let consumer = make_consumer();
        consumer.poll_batch(100);
        consumer.poll_batch(100);
        assert_eq!(consumer.committed_offset("events", 0), 2_000);
    }

    #[test]
    fn producer_send() {
        let prod = KafkaProducer::new(KafkaConfig::local());
        let consumer = make_consumer();
        let batches = consumer.poll_batch(100);
        let offset = prod.send("output", &batches[0].data).unwrap();
        assert_eq!(offset, 1_000);
    }

    #[test]
    fn stream_processor_passthrough() {
        let proc = StreamProcessor::new("in", "out", |b| b);
        let consumer = make_consumer();
        let batches = consumer.poll_batch(100);
        let out = proc.run_batch(batches.into_iter().next().unwrap());
        assert_eq!(out.topic, "out");
        assert_eq!(out.data.num_rows, 1_000);
    }

    #[test]
    fn tumbling_window_aggregation() {
        let consumer = make_consumer();
        let mut batches = Vec::new();
        for _ in 0..5 {
            batches.extend(consumer.poll_batch(100));
        }
        // group by "event_id" bucket via string cast, 60-second windows
        let result = tumbling_window(&batches, 60, "event_id", "value");
        assert!(result.num_rows > 0, "should have at least one window");
    }

    #[test]
    fn watermark_tracker() {
        let mut wm = WatermarkTracker::new(5);
        wm.update(100_000_000);
        wm.update(200_000_000);
        assert_eq!(wm.watermark(), 200_000_000 - 5 * 1_000_000);
    }
}
