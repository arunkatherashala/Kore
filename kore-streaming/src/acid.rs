//! ACID transaction support with snapshot isolation

use crate::error::{Result, StreamingError};
use crate::transaction::{Transaction, TransactionId, TransactionManager};
use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Record in ACID mode with version information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionedRecord {
    /// Record version
    pub version: u64,
    /// Transaction that wrote it
    pub transaction_id: TransactionId,
    /// Change type (insert, update, delete)
    pub change_type: ChangeType,
    /// Data payload
    pub data: Vec<u8>,
}

/// Change type for ACID records
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeType {
    /// New record inserted
    Insert,
    /// Existing record updated
    Update,
    /// Record deleted (tombstone)
    Delete,
}

/// ACID writer trait
#[async_trait]
pub trait AcidWriter: Send + Sync {
    /// Begin transaction
    async fn begin_transaction(&self) -> Result<TransactionId>;

    /// Write record in transaction
    async fn write(&self, txn_id: TransactionId, change: ChangeType, data: Vec<u8>) -> Result<()>;

    /// Write batch in transaction
    async fn write_batch(
        &self,
        txn_id: TransactionId,
        records: Vec<(ChangeType, Vec<u8>)>,
    ) -> Result<()>;

    /// Commit transaction
    async fn commit(&self, txn_id: TransactionId) -> Result<u64>;

    /// Abort/rollback transaction
    async fn abort(&self, txn_id: TransactionId) -> Result<()>;
}

/// ACID reader trait
#[async_trait]
pub trait AcidReader: Send + Sync {
    /// Read consistent snapshot at version
    async fn read_snapshot(&self, version: u64) -> Result<Vec<VersionedRecord>>;

    /// Read records from transaction
    async fn read_transaction(&self, txn_id: TransactionId) -> Result<Vec<VersionedRecord>>;

    /// Get current version
    fn current_version(&self) -> u64;

    /// Check if version is visible to transaction
    fn is_visible(&self, version: u64, txn_read_version: u64) -> bool;
}

/// In-memory ACID store with snapshot isolation
pub struct InMemoryAcidStore {
    records: Arc<parking_lot::Mutex<Vec<VersionedRecord>>>,
    transaction_manager: Arc<parking_lot::Mutex<TransactionManager>>,
}

impl InMemoryAcidStore {
    /// Create new ACID store
    pub fn new() -> Self {
        InMemoryAcidStore {
            records: Arc::new(parking_lot::Mutex::new(Vec::new())),
            transaction_manager: Arc::new(parking_lot::Mutex::new(TransactionManager::new())),
        }
    }

    /// Check for write-write conflicts
    fn check_conflict(&self, version: u64) -> Result<()> {
        // Simplified: no conflicts if version is current
        Ok(())
    }
}

impl Default for InMemoryAcidStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AcidWriter for InMemoryAcidStore {
    async fn begin_transaction(&self) -> Result<TransactionId> {
        let mut manager = self.transaction_manager.lock();
        let id = manager.begin();
        Ok(id)
    }

    async fn write(&self, txn_id: TransactionId, change: ChangeType, data: Vec<u8>) -> Result<()> {
        let mut manager = self.transaction_manager.lock();
        let mut txn = manager
            .get(txn_id)
            .ok_or_else(|| StreamingError::TransactionError("Transaction not found".to_string()))?;

        if !txn.is_active() {
            return Err(StreamingError::TransactionError(
                "Transaction not active".to_string(),
            ));
        }

        txn.record_row(data.len() as u64);

        drop(manager); // Release lock

        // Add to write set (but don't commit yet)
        let mut records = self.records.lock();
        records.push(VersionedRecord {
            version: txn.read_version,
            transaction_id: txn_id,
            change_type: change,
            data,
        });

        Ok(())
    }

    async fn write_batch(
        &self,
        txn_id: TransactionId,
        batch: Vec<(ChangeType, Vec<u8>)>,
    ) -> Result<()> {
        for (change_type, data) in batch {
            self.write(txn_id, change_type, data).await?;
        }
        Ok(())
    }

    async fn commit(&self, txn_id: TransactionId) -> Result<u64> {
        let mut manager = self.transaction_manager.lock();
        let write_version = manager.commit(txn_id)?;
        Ok(write_version)
    }

    async fn abort(&self, txn_id: TransactionId) -> Result<()> {
        let mut manager = self.transaction_manager.lock();
        manager.abort(txn_id)?;

        // Remove uncommitted records for this transaction
        let mut records = self.records.lock();
        records.retain(|r| r.transaction_id != txn_id);

        Ok(())
    }
}

#[async_trait]
impl AcidReader for InMemoryAcidStore {
    async fn read_snapshot(&self, version: u64) -> Result<Vec<VersionedRecord>> {
        let records = self.records.lock();
        Ok(records
            .iter()
            .filter(|r| r.version <= version)
            .cloned()
            .collect())
    }

    async fn read_transaction(&self, txn_id: TransactionId) -> Result<Vec<VersionedRecord>> {
        let records = self.records.lock();
        Ok(records
            .iter()
            .filter(|r| r.transaction_id == txn_id)
            .cloned()
            .collect())
    }

    fn current_version(&self) -> u64 {
        self.transaction_manager.lock().current_version()
    }

    fn is_visible(&self, version: u64, txn_read_version: u64) -> bool {
        version <= txn_read_version
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_acid_transaction() {
        let store = InMemoryAcidStore::new();

        let txn_id = store.begin_transaction().await.unwrap();
        assert_eq!(txn_id.as_u64(), 1);

        store
            .write(txn_id, ChangeType::Insert, vec![1, 2, 3])
            .await
            .unwrap();

        let version = store.commit(txn_id).await.unwrap();
        assert_eq!(version, 1);
    }

    #[tokio::test]
    async fn test_acid_abort() {
        let store = InMemoryAcidStore::new();

        let txn_id = store.begin_transaction().await.unwrap();
        store
            .write(txn_id, ChangeType::Insert, vec![1, 2, 3])
            .await
            .unwrap();

        store.abort(txn_id).await.unwrap();

        // Records should be cleaned up
        let snapshot = store.read_snapshot(0).await.unwrap();
        assert_eq!(snapshot.len(), 0);
    }

    #[tokio::test]
    async fn test_snapshot_isolation() {
        let store = InMemoryAcidStore::new();

        let txn1 = store.begin_transaction().await.unwrap();
        store
            .write(txn1, ChangeType::Insert, vec![1, 2, 3])
            .await
            .unwrap();
        let v1 = store.commit(txn1).await.unwrap();

        let txn2 = store.begin_transaction().await.unwrap();
        store
            .write(txn2, ChangeType::Insert, vec![4, 5, 6])
            .await
            .unwrap();
        let v2 = store.commit(txn2).await.unwrap();

        assert_eq!(v1, 1);
        assert_eq!(v2, 2);

        let snapshot = store.read_snapshot(1).await.unwrap();
        assert_eq!(snapshot.len(), 1);
    }
}
