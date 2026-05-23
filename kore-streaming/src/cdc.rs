//! Change Data Capture (CDC) for real-time streaming

use crate::error::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Change type in CDC stream
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeType {
    /// New record inserted
    Insert,
    /// Existing record updated
    Update,
    /// Record deleted
    Delete,
}

impl std::fmt::Display for ChangeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangeType::Insert => write!(f, "INSERT"),
            ChangeType::Update => write!(f, "UPDATE"),
            ChangeType::Delete => write!(f, "DELETE"),
        }
    }
}

/// Individual change record in CDC stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeRecord {
    /// Monotonic sequence number
    pub sequence: u64,
    /// Change timestamp
    pub timestamp: DateTime<Utc>,
    /// Change type
    pub change_type: ChangeType,
    /// Before image (for updates/deletes)
    pub before: Option<Vec<u8>>,
    /// After image (for inserts/updates)
    pub after: Option<Vec<u8>>,
    /// Affected schema version
    pub schema_version: u32,
}

impl ChangeRecord {
    /// Create new CDC record
    pub fn new(
        sequence: u64,
        change_type: ChangeType,
        after: Option<Vec<u8>>,
    ) -> Self {
        ChangeRecord {
            sequence,
            timestamp: Utc::now(),
            change_type,
            before: None,
            after,
            schema_version: 1,
        }
    }

    /// Create insert record
    pub fn insert(sequence: u64, data: Vec<u8>) -> Self {
        Self::new(sequence, ChangeType::Insert, Some(data))
    }

    /// Create update record with before/after
    pub fn update(sequence: u64, before: Vec<u8>, after: Vec<u8>) -> Self {
        ChangeRecord {
            sequence,
            timestamp: Utc::now(),
            change_type: ChangeType::Update,
            before: Some(before),
            after: Some(after),
            schema_version: 1,
        }
    }

    /// Create delete record
    pub fn delete(sequence: u64, before: Vec<u8>) -> Self {
        ChangeRecord {
            sequence,
            timestamp: Utc::now(),
            change_type: ChangeType::Delete,
            before: Some(before),
            after: None,
            schema_version: 1,
        }
    }

    /// Get data size in bytes
    pub fn size(&self) -> usize {
        let before_size = self.before.as_ref().map(|b| b.len()).unwrap_or(0);
        let after_size = self.after.as_ref().map(|a| a.len()).unwrap_or(0);
        before_size + after_size
    }
}

/// CDC stream consumer trait
#[async_trait]
pub trait CDCStream: Send + Sync {
    /// Subscribe to CDC changes from sequence
    async fn subscribe(&self, from_sequence: u64) -> Result<Vec<ChangeRecord>>;

    /// Publish a change
    async fn publish(&self, record: ChangeRecord) -> Result<()>;

    /// Publish batch of changes
    async fn publish_batch(&self, records: Vec<ChangeRecord>) -> Result<()>;

    /// Get latest sequence
    fn latest_sequence(&self) -> u64;

    /// Get subscriber count
    fn subscriber_count(&self) -> usize;
}

/// In-memory CDC stream
pub struct InMemoryCDCStream {
    records: Arc<parking_lot::Mutex<Vec<ChangeRecord>>>,
    current_sequence: std::sync::atomic::AtomicU64,
    subscribers: std::sync::atomic::AtomicUsize,
}

impl InMemoryCDCStream {
    /// Create new CDC stream
    pub fn new() -> Self {
        InMemoryCDCStream {
            records: Arc::new(parking_lot::Mutex::new(Vec::new())),
            current_sequence: std::sync::atomic::AtomicU64::new(0),
            subscribers: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl Default for InMemoryCDCStream {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CDCStream for InMemoryCDCStream {
    async fn subscribe(&self, from_sequence: u64) -> Result<Vec<ChangeRecord>> {
        self.subscribers
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let records = self.records.lock();
        Ok(records
            .iter()
            .filter(|r| r.sequence >= from_sequence)
            .cloned()
            .collect())
    }

    async fn publish(&self, record: ChangeRecord) -> Result<()> {
        let mut records = self.records.lock();
        records.push(record);
        self.current_sequence
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    async fn publish_batch(&self, batch: Vec<ChangeRecord>) -> Result<()> {
        let mut records = self.records.lock();
        let count = batch.len() as u64;

        for record in batch {
            records.push(record);
        }

        self.current_sequence
            .fetch_add(count, std::sync::atomic::Ordering::SeqCst);

        Ok(())
    }

    fn latest_sequence(&self) -> u64 {
        self.current_sequence.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn subscriber_count(&self) -> usize {
        self.subscribers.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_record_insert() {
        let record = ChangeRecord::insert(0, vec![1, 2, 3]);
        assert_eq!(record.change_type, ChangeType::Insert);
        assert_eq!(record.after, Some(vec![1, 2, 3]));
        assert_eq!(record.before, None);
    }

    #[test]
    fn test_change_record_update() {
        let record = ChangeRecord::update(1, vec![1, 2], vec![2, 3]);
        assert_eq!(record.change_type, ChangeType::Update);
        assert_eq!(record.before, Some(vec![1, 2]));
        assert_eq!(record.after, Some(vec![2, 3]));
    }

    #[test]
    fn test_change_record_delete() {
        let record = ChangeRecord::delete(2, vec![1, 2, 3]);
        assert_eq!(record.change_type, ChangeType::Delete);
        assert_eq!(record.before, Some(vec![1, 2, 3]));
        assert_eq!(record.after, None);
    }

    #[tokio::test]
    async fn test_cdc_stream() {
        let stream = InMemoryCDCStream::new();

        let record1 = ChangeRecord::insert(0, vec![1, 2, 3]);
        let record2 = ChangeRecord::update(1, vec![1, 2], vec![2, 3]);

        stream.publish(record1).await.unwrap();
        stream.publish(record2).await.unwrap();

        assert_eq!(stream.latest_sequence(), 2);

        let changes = stream.subscribe(0).await.unwrap();
        assert_eq!(changes.len(), 2);
    }

    #[tokio::test]
    async fn test_cdc_stream_filter() {
        let stream = InMemoryCDCStream::new();

        for i in 0..10 {
            let record = ChangeRecord::insert(i, vec![i as u8]);
            stream.publish(record).await.unwrap();
        }

        let changes = stream.subscribe(5).await.unwrap();
        assert_eq!(changes.len(), 5);
    }
}
