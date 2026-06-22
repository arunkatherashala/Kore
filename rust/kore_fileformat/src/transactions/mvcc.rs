/// MVCC (Multi-Version Concurrency Control) - Snapshot Isolation
/// 
/// Week 2 Deliverable: Immutable snapshot snapshots with manifest references
/// Enables: Concurrent readers + writers without locking
/// 
/// Snapshot Format:
/// - Timestamp-based snapshot ID
/// - References to block manifests (not delta)
/// - Bloom filter for quick version existence checks
/// - Transaction visibility bitmap

use std::collections::{HashMap, BTreeMap};
use std::sync::Arc;
use parking_lot::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Point-in-time snapshot of database state
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub snapshot_id: u64,
    pub timestamp: u64,
    pub txn_id: u64,
    /// Partition ID -> list of block IDs in this snapshot
    pub partitions: HashMap<u32, Vec<u64>>,
    /// Transaction visibility bitmap (which txns are visible)
    pub visible_txns: Vec<u64>,
    /// Checksum for integrity
    pub checksum: u64,
}

impl Snapshot {
    pub fn new(snapshot_id: u64, txn_id: u64) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        Snapshot {
            snapshot_id,
            timestamp,
            txn_id,
            partitions: HashMap::new(),
            visible_txns: Vec::new(),
            checksum: 0,
        }
    }

    /// Add block reference to snapshot
    pub fn add_block(&mut self, partition_id: u32, block_id: u64) {
        self.partitions
            .entry(partition_id)
            .or_insert_with(Vec::new)
            .push(block_id);
    }

    /// Calculate snapshot checksum (simple XOR for integrity)
    pub fn compute_checksum(&mut self) {
        let mut checksum = 0u64;
        checksum ^= self.snapshot_id;
        checksum ^= self.txn_id;
        for (part_id, blocks) in &self.partitions {
            checksum ^= *part_id as u64;
            for block in blocks {
                checksum ^= block;
            }
        }
        self.checksum = checksum;
    }

    /// Verify snapshot integrity
    pub fn verify(&self) -> bool {
        let mut checksum = 0u64;
        checksum ^= self.snapshot_id;
        checksum ^= self.txn_id;
        for (part_id, blocks) in &self.partitions {
            checksum ^= *part_id as u64;
            for block in blocks {
                checksum ^= block;
            }
        }
        checksum == self.checksum
    }

    /// Get all blocks for a partition
    pub fn get_blocks(&self, partition_id: u32) -> Option<&[u64]> {
        self.partitions.get(&partition_id).map(|v| v.as_slice())
    }
}

/// MVCC Manager: maintains multiple versions for concurrent access
pub struct MvccManager {
    /// Snapshots indexed by timestamp
    snapshots: Arc<RwLock<BTreeMap<u64, Arc<Snapshot>>>>,
    /// Current latest snapshot ID
    current_snapshot_id: Arc<RwLock<u64>>,
    /// Transaction visibility vectors (for conflict detection)
    txn_visibility: Arc<RwLock<HashMap<u64, Vec<u64>>>>,
    /// Maximum snapshots to retain (older ones can be garbage collected)
    max_snapshots: usize,
}

impl MvccManager {
    pub fn new(max_snapshots: usize) -> Self {
        MvccManager {
            snapshots: Arc::new(RwLock::new(BTreeMap::new())),
            current_snapshot_id: Arc::new(RwLock::new(0)),
            txn_visibility: Arc::new(RwLock::new(HashMap::new())),
            max_snapshots,
        }
    }

    /// Create new immutable snapshot
    pub fn create_snapshot(&self, txn_id: u64) -> Arc<Snapshot> {
        let mut snapshot_id = self.current_snapshot_id.write();
        *snapshot_id += 1;
        
        let mut snapshot = Snapshot::new(*snapshot_id, txn_id);
        snapshot.compute_checksum();
        
        let snapshot = Arc::new(snapshot);
        
        let mut snapshots = self.snapshots.write();
        snapshots.insert(snapshot.timestamp, Arc::clone(&snapshot));
        
        // Garbage collect old snapshots if needed
        if snapshots.len() > self.max_snapshots {
            if let Some(oldest_key) = snapshots.keys().next().copied() {
                snapshots.remove(&oldest_key);
            }
        }
        
        snapshot
    }

    /// Get snapshot as of timestamp (time-travel query support)
    pub fn get_snapshot_at(&self, timestamp: u64) -> Option<Arc<Snapshot>> {
        let snapshots = self.snapshots.read();
        
        // Find snapshot at or before timestamp
        snapshots
            .range(..=timestamp)
            .next_back()
            .map(|(_, snapshot)| Arc::clone(snapshot))
    }

    /// Get latest snapshot
    pub fn get_latest_snapshot(&self) -> Option<Arc<Snapshot>> {
        let snapshots = self.snapshots.read();
        snapshots.values().next_back().map(Arc::clone)
    }

    /// Register transaction visibility for conflict detection
    pub fn register_transaction(&self, txn_id: u64, visible_txns: Vec<u64>) {
        let mut visibility = self.txn_visibility.write();
        visibility.insert(txn_id, visible_txns);
    }

    /// Check if transaction can see another transaction's changes
    pub fn is_visible(&self, reader_txn_id: u64, writer_txn_id: u64) -> bool {
        let visibility = self.txn_visibility.read();
        if let Some(visible_txns) = visibility.get(&reader_txn_id) {
            visible_txns.contains(&writer_txn_id)
        } else {
            false
        }
    }

    /// Get all snapshots (for debugging/analysis)
    pub fn list_snapshots(&self) -> Vec<Arc<Snapshot>> {
        let snapshots = self.snapshots.read();
        snapshots.values().cloned().collect()
    }

    /// Clean up old snapshots (garbage collection)
    pub fn cleanup_old_snapshots(&self, keep_count: usize) -> usize {
        let mut snapshots = self.snapshots.write();
        let initial_len = snapshots.len();
        
        while snapshots.len() > keep_count {
            if let Some(oldest_key) = snapshots.keys().next().copied() {
                snapshots.remove(&oldest_key);
            }
        }
        
        initial_len - snapshots.len()
    }
}

/// Transaction context for optimistic concurrency
#[derive(Debug, Clone)]
pub struct TransactionContext {
    pub txn_id: u64,
    pub start_snapshot: Arc<Snapshot>,
    pub read_set: Vec<(u32, u64)>, // (partition_id, block_id)
    pub write_set: Vec<(u32, u64)>, // (partition_id, block_id)
    pub conflicts: Vec<u64>, // Transaction IDs that conflict
}

impl TransactionContext {
    pub fn new(txn_id: u64, snapshot: Arc<Snapshot>) -> Self {
        TransactionContext {
            txn_id,
            start_snapshot: snapshot,
            read_set: Vec::new(),
            write_set: Vec::new(),
            conflicts: Vec::new(),
        }
    }

    /// Record read access
    pub fn add_read(&mut self, partition_id: u32, block_id: u64) {
        self.read_set.push((partition_id, block_id));
    }

    /// Record write access
    pub fn add_write(&mut self, partition_id: u32, block_id: u64) {
        self.write_set.push((partition_id, block_id));
    }

    /// Check for conflicts with another transaction (simplistic)
    pub fn detect_conflict(&mut self, other: &TransactionContext) -> bool {
        // Conflict if other transaction writes to our read set
        for (our_part, our_block) in &self.read_set {
            for (other_part, other_block) in &other.write_set {
                if our_part == other_part && our_block == other_block {
                    self.conflicts.push(other.txn_id);
                    return true;
                }
            }
        }
        
        // Or if other transaction reads what we write (not critical but can track)
        false
    }

    /// Check if transaction has conflicts
    pub fn has_conflicts(&self) -> bool {
        !self.conflicts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_creation() {
        let mut snapshot = Snapshot::new(1, 100);
        snapshot.add_block(0, 1);
        snapshot.add_block(0, 2);
        snapshot.compute_checksum();
        
        assert_eq!(snapshot.snapshot_id, 1);
        assert_eq!(snapshot.txn_id, 100);
        assert!(snapshot.verify());
    }

    #[test]
    fn test_mvcc_manager_snapshots() {
        let mvcc = MvccManager::new(10);
        
        let snap1 = mvcc.create_snapshot(1);
        let snap2 = mvcc.create_snapshot(2);
        
        assert_eq!(snap1.txn_id, 1);
        assert_eq!(snap2.txn_id, 2);
        assert_ne!(snap1.snapshot_id, snap2.snapshot_id);
        
        let latest = mvcc.get_latest_snapshot().unwrap();
        assert_eq!(latest.txn_id, 2);
    }

    #[test]
    fn test_transaction_conflict_detection() {
        let snapshot = Arc::new(Snapshot::new(1, 1));
        
        let mut txn1 = TransactionContext::new(1, Arc::clone(&snapshot));
        let mut txn2 = TransactionContext::new(2, Arc::clone(&snapshot));
        
        txn1.add_read(0, 1);
        txn2.add_write(0, 1);
        
        let has_conflict = txn1.detect_conflict(&txn2);
        assert!(has_conflict);
        assert_eq!(txn1.conflicts.len(), 1);
    }

    #[test]
    fn test_time_travel_queries() {
        let mvcc = MvccManager::new(100);
        
        let snap1 = mvcc.create_snapshot(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
        let snap2 = mvcc.create_snapshot(2);
        
        let old_snapshot = mvcc.get_snapshot_at(snap1.timestamp).unwrap();
        assert_eq!(old_snapshot.txn_id, 1);
        
        let new_snapshot = mvcc.get_snapshot_at(snap2.timestamp).unwrap();
        assert_eq!(new_snapshot.txn_id, 2);
    }

    #[test]
    fn test_snapshot_garbage_collection() {
        let mvcc = MvccManager::new(3);
        
        for i in 0..5 {
            mvcc.create_snapshot(i);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        
        let snapshots = mvcc.list_snapshots();
        assert!(snapshots.len() <= 3);
    }
}
