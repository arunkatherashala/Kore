/// Concurrent Transaction Writers - Lock-Free Design
/// 
/// Week 2 Deliverable: Parallel WAL writes without blocking
/// Target: 5000+ transactions/sec with minimal lock contention
/// Strategy: Atomic transaction assignment + parallel WAL channels

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use parking_lot::RwLock;
use std::thread;
use std::sync::mpsc;

/// Lock-free transaction ID generator
pub struct TxnIdGenerator {
    next_id: AtomicU64,
}

impl TxnIdGenerator {
    pub fn new() -> Self {
        TxnIdGenerator {
            next_id: AtomicU64::new(1),
        }
    }

    /// Atomically allocate next transaction ID (lock-free)
    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Get current transaction ID without incrementing
    pub fn current_id(&self) -> u64 {
        self.next_id.load(Ordering::SeqCst)
    }
}

/// Multi-channel WAL writer for parallel writes
pub struct ParallelWalWriter {
    /// Multiple WAL channels for parallel writes (sharded by partition)
    channels: Vec<mpsc::Sender<super::wal::WalEntry>>,
    /// Thread handles for WAL writers
    _handles: Vec<thread::JoinHandle<()>>,
    /// Transaction ID generator (shared, lock-free)
    txn_id_gen: Arc<TxnIdGenerator>,
    /// Number of parallel writers
    num_writers: usize,
}

impl ParallelWalWriter {
    /// Create parallel WAL writer with N channels
    pub fn new(num_writers: usize) -> (Self, Arc<TxnIdGenerator>) {
        let mut channels = Vec::new();
        let mut handles = Vec::new();
        let txn_id_gen = Arc::new(TxnIdGenerator::new());

        for writer_id in 0..num_writers {
            let (tx, rx) = mpsc::channel::<super::wal::WalEntry>();
            channels.push(tx);

            // Spawn WAL writer thread
            let handle = thread::spawn(move || {
                // Each writer processes entries from its channel
                while let Ok(_entry) = rx.recv() {
                    // In production: write to persistent WAL file
                    // For now: simulate work
                    let _ = writer_id;
                }
            });

            handles.push(handle);
        }

        let writer = ParallelWalWriter {
            channels,
            _handles: handles,
            txn_id_gen: Arc::clone(&txn_id_gen),
            num_writers,
        };

        (writer, Arc::clone(&txn_id_gen))
    }

    /// Submit entry to appropriate channel (shard by partition)
    pub fn submit_async(&self, mut entry: super::wal::WalEntry) -> u64 {
        let txn_id = self.txn_id_gen.next_id();
        entry.txn_id = txn_id;

        // Shard entries by partition_id to distribute load
        let channel_idx = (entry.partition_id as usize) % self.num_writers;
        
        let _ = self.channels[channel_idx].send(entry);
        
        txn_id
    }

    /// Batch submit entries (even more efficient)
    pub fn submit_batch(&self, entries: Vec<super::wal::WalEntry>) -> Vec<u64> {
        let mut txn_ids = Vec::new();
        
        // Group entries by partition for locality
        let mut partitioned: Vec<Vec<_>> = vec![Vec::new(); self.num_writers];
        
        for mut entry in entries {
            let txn_id = self.txn_id_gen.next_id();
            entry.txn_id = txn_id;
            txn_ids.push(txn_id);
            
            let channel_idx = (entry.partition_id as usize) % self.num_writers;
            partitioned[channel_idx].push(entry);
        }
        
        // Send to each writer
        for (idx, batch) in partitioned.into_iter().enumerate() {
            for entry in batch {
                let _ = self.channels[idx].send(entry);
            }
        }
        
        txn_ids
    }
}

/// Transaction context with timeout support
#[derive(Debug, Clone)]
pub struct ConcurrentTransactionContext {
    pub txn_id: u64,
    pub start_time: std::time::Instant,
    pub timeout_ms: u64,
    pub status: TransactionStatus,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransactionStatus {
    Running,
    Committed,
    Rolledback,
    TimedOut,
}

impl ConcurrentTransactionContext {
    pub fn new(txn_id: u64, timeout_ms: u64) -> Self {
        ConcurrentTransactionContext {
            txn_id,
            start_time: std::time::Instant::now(),
            timeout_ms,
            status: TransactionStatus::Running,
        }
    }

    /// Check if transaction has timed out
    pub fn is_timed_out(&self) -> bool {
        self.start_time.elapsed().as_millis() > self.timeout_ms as u128
    }

    /// Mark as committed
    pub fn commit(&mut self) {
        self.status = TransactionStatus::Committed;
    }

    /// Mark as rolled back
    pub fn rollback(&mut self) {
        self.status = TransactionStatus::Rolledback;
    }

    /// Get elapsed time in microseconds
    pub fn elapsed_us(&self) -> u64 {
        self.start_time.elapsed().as_micros() as u64
    }
}

/// Concurrent transaction manager with timeout detection
pub struct ConcurrentTransactionManager {
    /// Active transactions indexed by txn_id
    active: Arc<RwLock<std::collections::HashMap<u64, ConcurrentTransactionContext>>>,
    /// Timeout check interval
    timeout_check_ms: u64,
}

impl ConcurrentTransactionManager {
    pub fn new(timeout_check_ms: u64) -> Self {
        ConcurrentTransactionManager {
            active: Arc::new(RwLock::new(std::collections::HashMap::new())),
            timeout_check_ms,
        }
    }

    /// Register new transaction
    pub fn begin_transaction(&self, txn_id: u64, timeout_ms: u64) -> ConcurrentTransactionContext {
        let ctx = ConcurrentTransactionContext::new(txn_id, timeout_ms);
        self.active.write().insert(txn_id, ctx.clone());
        ctx
    }

    /// Complete transaction (remove from active)
    pub fn end_transaction(&self, txn_id: u64) {
        self.active.write().remove(&txn_id);
    }

    /// Check for timed-out transactions and rollback
    pub fn check_timeouts(&self) -> Vec<u64> {
        let mut timed_out = Vec::new();
        let mut active = self.active.write();
        
        for (txn_id, ctx) in active.iter_mut() {
            if ctx.is_timed_out() && ctx.status == TransactionStatus::Running {
                ctx.status = TransactionStatus::TimedOut;
                timed_out.push(*txn_id);
            }
        }
        
        // Remove timed out transactions
        for txn_id in &timed_out {
            active.remove(txn_id);
        }
        
        timed_out
    }

    /// Get number of active transactions
    pub fn active_count(&self) -> usize {
        self.active.read().len()
    }

    /// Get transaction status
    pub fn get_status(&self, txn_id: u64) -> Option<TransactionStatus> {
        self.active.read().get(&txn_id).map(|ctx| ctx.status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_txn_id_generator_lock_free() {
        let gen = Arc::new(TxnIdGenerator::new());
        let mut handles = vec![];

        // Spawn 10 threads, each getting 100 IDs
        for _ in 0..10 {
            let gen_clone = Arc::clone(&gen);
            let handle = thread::spawn(move || {
                let mut ids = vec![];
                for _ in 0..100 {
                    ids.push(gen_clone.next_id());
                }
                ids
            });
            handles.push(handle);
        }

        // Collect all IDs
        let mut all_ids = vec![];
        for handle in handles {
            let ids = handle.join().unwrap();
            all_ids.extend(ids);
        }

        // Verify all IDs are unique
        assert_eq!(all_ids.len(), 1000);
        all_ids.sort();
        for (i, id) in all_ids.iter().enumerate() {
            assert_eq!(*id, (i + 1) as u64);
        }
    }

    #[test]
    fn test_concurrent_transaction_context() {
        let mut ctx = ConcurrentTransactionContext::new(1, 100);
        
        assert_eq!(ctx.status, TransactionStatus::Running);
        assert!(!ctx.is_timed_out());
        
        ctx.commit();
        assert_eq!(ctx.status, TransactionStatus::Committed);
    }

    #[test]
    fn test_transaction_timeout() {
        let ctx = ConcurrentTransactionContext::new(1, 1);
        
        // Sleep longer than timeout
        thread::sleep(std::time::Duration::from_millis(2));
        
        assert!(ctx.is_timed_out());
    }

    #[test]
    fn test_transaction_manager_registration() {
        let mgr = ConcurrentTransactionManager::new(100);
        
        let ctx = mgr.begin_transaction(1, 1000);
        assert_eq!(ctx.txn_id, 1);
        assert_eq!(mgr.active_count(), 1);
        
        mgr.end_transaction(1);
        assert_eq!(mgr.active_count(), 0);
    }

    #[test]
    fn test_transaction_manager_timeouts() {
        let mgr = ConcurrentTransactionManager::new(10);
        
        mgr.begin_transaction(1, 1);
        mgr.begin_transaction(2, 1000);
        
        assert_eq!(mgr.active_count(), 2);
        
        thread::sleep(std::time::Duration::from_millis(2));
        
        let timed_out = mgr.check_timeouts();
        assert!(timed_out.contains(&1));
        assert!(!timed_out.contains(&2));
    }

    #[test]
    fn test_parallel_wal_writer_sharding() {
        let (writer, _gen) = ParallelWalWriter::new(4);
        
        let mut entries = vec![];
        for i in 0..100 {
            entries.push(super::super::wal::WalEntry {
                timestamp: 1000 + i,
                txn_id: 0,
                op_type: super::super::wal::OperationType::Insert,
                partition_id: (i % 10) as u32,
                column_id: 1,
                min_val: 0,
                max_val: 100,
                payload: vec![],
            });
        }
        
        let txn_ids = writer.submit_batch(entries);
        assert_eq!(txn_ids.len(), 100);
        
        // Verify IDs are sequential
        for (i, txn_id) in txn_ids.iter().enumerate() {
            assert_eq!(*txn_id, (i + 1) as u64);
        }
    }
}
