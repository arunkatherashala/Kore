//! Transaction management for ACID support

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique transaction identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TransactionId(u64);

impl TransactionId {
    /// Create new transaction ID
    pub fn new(id: u64) -> Self {
        TransactionId(id)
    }

    /// Get numeric ID
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl From<u64> for TransactionId {
    fn from(id: u64) -> Self {
        TransactionId(id)
    }
}

/// Transaction state enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionState {
    /// Transaction is active
    Active,
    /// Transaction is preparing to commit
    Preparing,
    /// Transaction committed successfully
    Committed,
    /// Transaction rolled back
    Aborted,
}

/// ACID transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Unique transaction ID
    pub id: TransactionId,
    /// Transaction start timestamp
    pub start_time: DateTime<Utc>,
    /// Transaction state
    pub state: TransactionState,
    /// Read version (snapshot isolation)
    pub read_version: u64,
    /// Write version (if committed)
    pub write_version: Option<u64>,
    /// Affected row count
    pub row_count: u64,
    /// Bytes written
    pub bytes_written: u64,
}

impl Transaction {
    /// Create new transaction
    pub fn new(id: TransactionId, read_version: u64) -> Self {
        Transaction {
            id,
            start_time: Utc::now(),
            state: TransactionState::Active,
            read_version,
            write_version: None,
            row_count: 0,
            bytes_written: 0,
        }
    }

    /// Mark transaction as active
    pub fn set_active(&mut self) {
        self.state = TransactionState::Active;
    }

    /// Mark transaction as preparing
    pub fn set_preparing(&mut self) {
        self.state = TransactionState::Preparing;
    }

    /// Mark transaction as committed
    pub fn set_committed(&mut self, write_version: u64) {
        self.state = TransactionState::Committed;
        self.write_version = Some(write_version);
    }

    /// Mark transaction as aborted
    pub fn set_aborted(&mut self) {
        self.state = TransactionState::Aborted;
    }

    /// Check if transaction is active
    pub fn is_active(&self) -> bool {
        self.state == TransactionState::Active
    }

    /// Check if transaction is committed
    pub fn is_committed(&self) -> bool {
        self.state == TransactionState::Committed
    }

    /// Get transaction duration
    pub fn duration(&self) -> std::time::Duration {
        let end = Utc::now();
        (end - self.start_time)
            .to_std()
            .unwrap_or_default()
    }

    /// Record row written
    pub fn record_row(&mut self, bytes: u64) {
        self.row_count += 1;
        self.bytes_written += bytes;
    }

    /// Record batch written
    pub fn record_batch(&mut self, rows: u64, bytes: u64) {
        self.row_count += rows;
        self.bytes_written += bytes;
    }
}

/// Transaction log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionLogEntry {
    /// Transaction ID
    pub transaction_id: TransactionId,
    /// Entry type
    pub entry_type: String, // "begin", "commit", "abort", "checkpoint"
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Data (JSON encoded)
    pub data: serde_json::Value,
}

/// Transaction manager (in-memory)
pub struct TransactionManager {
    next_id: u64,
    current_version: u64,
    transactions: dashmap::DashMap<TransactionId, Transaction>,
}

impl TransactionManager {
    /// Create new transaction manager
    pub fn new() -> Self {
        TransactionManager {
            next_id: 1,
            current_version: 0,
            transactions: dashmap::DashMap::new(),
        }
    }

    /// Begin new transaction
    pub fn begin(&mut self) -> TransactionId {
        let id = TransactionId::new(self.next_id);
        self.next_id += 1;

        let mut txn = Transaction::new(id, self.current_version);
        txn.set_active();
        self.transactions.insert(id, txn);

        id
    }

    /// Get transaction by ID
    pub fn get(&self, id: TransactionId) -> Option<Transaction> {
        self.transactions.get(&id).map(|r| r.clone())
    }

    /// Commit transaction
    pub fn commit(&mut self, id: TransactionId) -> Result<u64, String> {
        let mut txn = self
            .transactions
            .get_mut(&id)
            .ok_or("Transaction not found")?;

        if !txn.is_active() {
            return Err("Transaction not active".to_string());
        }

        let write_version = self.current_version + 1;
        txn.set_committed(write_version);
        self.current_version = write_version;

        Ok(write_version)
    }

    /// Abort transaction
    pub fn abort(&mut self, id: TransactionId) -> Result<(), String> {
        let mut txn = self
            .transactions
            .get_mut(&id)
            .ok_or("Transaction not found")?;

        txn.set_aborted();
        Ok(())
    }

    /// Get current version
    pub fn current_version(&self) -> u64 {
        self.current_version
    }

    /// Get active transaction count
    pub fn active_count(&self) -> usize {
        self.transactions
            .iter()
            .filter(|entry| entry.value().is_active())
            .count()
    }
}

impl Default for TransactionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_creation() {
        let id = TransactionId::new(1);
        let txn = Transaction::new(id, 0);

        assert_eq!(txn.id, id);
        assert_eq!(txn.state, TransactionState::Active);
        assert_eq!(txn.row_count, 0);
    }

    #[test]
    fn test_transaction_state_transitions() {
        let id = TransactionId::new(1);
        let mut txn = Transaction::new(id, 0);

        assert!(txn.is_active());
        txn.set_preparing();
        assert_eq!(txn.state, TransactionState::Preparing);
        txn.set_committed(1);
        assert!(txn.is_committed());
    }

    #[test]
    fn test_transaction_record_row() {
        let id = TransactionId::new(1);
        let mut txn = Transaction::new(id, 0);

        txn.record_row(100);
        txn.record_row(100);

        assert_eq!(txn.row_count, 2);
        assert_eq!(txn.bytes_written, 200);
    }

    #[test]
    fn test_transaction_manager() {
        let mut manager = TransactionManager::new();

        let id1 = manager.begin();
        let id2 = manager.begin();

        assert_eq!(manager.active_count(), 2);

        manager.commit(id1).unwrap();
        assert_eq!(manager.current_version(), 1);

        manager.abort(id2).unwrap();
        assert_eq!(manager.active_count(), 0);
    }
}
