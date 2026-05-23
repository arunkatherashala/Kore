//! Append-only streaming mode

use crate::error::Result;
use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Record in append-only mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendRecord {
    /// Sequence number (monotonic)
    pub sequence: u64,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Data payload
    pub data: Vec<u8>,
}

impl AppendRecord {
    /// Create new append record
    pub fn new(sequence: u64, data: Vec<u8>) -> Self {
        AppendRecord {
            sequence,
            timestamp: chrono::Utc::now(),
            data,
        }
    }

    /// Get data size in bytes
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

/// Append-only writer trait
#[async_trait]
pub trait AppendOnlyWriter: Send + Sync {
    /// Append record to stream
    async fn append(&self, record: AppendRecord) -> Result<u64>;

    /// Append batch of records
    async fn append_batch(&self, records: Vec<AppendRecord>) -> Result<u64>;

    /// Flush pending writes
    async fn flush(&self) -> Result<()>;

    /// Get current sequence number
    fn current_sequence(&self) -> u64;

    /// Get total bytes written
    fn total_bytes(&self) -> u64;
}

/// Append-only reader trait
#[async_trait]
pub trait AppendOnlyReader: Send + Sync {
    /// Read from specific sequence
    async fn read_from(&self, sequence: u64) -> Result<Vec<AppendRecord>>;

    /// Read latest N records
    async fn read_latest(&self, limit: usize) -> Result<Vec<AppendRecord>>;

    /// Stream records starting from sequence (iterator)
    async fn stream_from(&self, sequence: u64) -> Result<Vec<AppendRecord>>;

    /// Get total records
    fn total_records(&self) -> u64;

    /// Get latest sequence
    fn latest_sequence(&self) -> u64;
}

/// In-memory append-only writer/reader
pub struct InMemoryAppendOnlyStore {
    records: Arc<parking_lot::Mutex<Vec<AppendRecord>>>,
    current_sequence: std::sync::atomic::AtomicU64,
}

impl InMemoryAppendOnlyStore {
    /// Create new in-memory store
    pub fn new() -> Self {
        InMemoryAppendOnlyStore {
            records: Arc::new(parking_lot::Mutex::new(Vec::new())),
            current_sequence: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl Default for InMemoryAppendOnlyStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AppendOnlyWriter for InMemoryAppendOnlyStore {
    async fn append(&self, record: AppendRecord) -> Result<u64> {
        let mut records = self.records.lock();
        records.push(record.clone());
        let seq = self
            .current_sequence
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(seq)
    }

    async fn append_batch(&self, batch: Vec<AppendRecord>) -> Result<u64> {
        let mut records = self.records.lock();
        let mut last_seq = self.current_sequence.load(std::sync::atomic::Ordering::SeqCst);

        for record in batch {
            records.push(record);
            last_seq = self
                .current_sequence
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }

        Ok(last_seq)
    }

    async fn flush(&self) -> Result<()> {
        // In-memory, no-op
        Ok(())
    }

    fn current_sequence(&self) -> u64 {
        self.current_sequence.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn total_bytes(&self) -> u64 {
        let records = self.records.lock();
        records.iter().map(|r| r.size() as u64).sum()
    }
}

#[async_trait]
impl AppendOnlyReader for InMemoryAppendOnlyStore {
    async fn read_from(&self, sequence: u64) -> Result<Vec<AppendRecord>> {
        let records = self.records.lock();
        Ok(records
            .iter()
            .skip(sequence as usize)
            .cloned()
            .collect())
    }

    async fn read_latest(&self, limit: usize) -> Result<Vec<AppendRecord>> {
        let records = self.records.lock();
        Ok(records
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect())
    }

    async fn stream_from(&self, sequence: u64) -> Result<Vec<AppendRecord>> {
        self.read_from(sequence).await
    }

    fn total_records(&self) -> u64 {
        self.records.lock().len() as u64
    }

    fn latest_sequence(&self) -> u64 {
        self.current_sequence.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_append_record() {
        let record = AppendRecord::new(0, vec![1, 2, 3, 4]);
        assert_eq!(record.sequence, 0);
        assert_eq!(record.size(), 4);
    }

    #[tokio::test]
    async fn test_in_memory_append() {
        let store = InMemoryAppendOnlyStore::new();

        let record = AppendRecord::new(0, vec![1, 2, 3]);
        let seq = store.append(record).await.unwrap();

        assert_eq!(seq, 0);
        assert_eq!(store.total_records(), 1);
    }

    #[tokio::test]
    async fn test_in_memory_append_batch() {
        let store = InMemoryAppendOnlyStore::new();

        let records = vec![
            AppendRecord::new(0, vec![1, 2, 3]),
            AppendRecord::new(1, vec![4, 5, 6]),
        ];

        store.append_batch(records).await.unwrap();

        assert_eq!(store.total_records(), 2);
    }

    #[tokio::test]
    async fn test_in_memory_read() {
        let store = InMemoryAppendOnlyStore::new();

        for i in 0..5 {
            let record = AppendRecord::new(i, vec![i as u8]);
            store.append(record).await.unwrap();
        }

        let records = store.read_from(2).await.unwrap();
        assert_eq!(records.len(), 3);
    }
}
