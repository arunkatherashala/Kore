/// Transaction Management System (Track F)
/// 
/// This module provides ACID transaction support through:
/// 1. WAL (Write-Ahead Log) - durability
/// 2. MVCC (Multi-Version Concurrency Control) - isolation
/// 3. Snapshot Management - time-travel queries
/// 4. Conflict Detection - optimistic concurrency

pub mod wal;
pub mod mvcc;

pub use wal::{WalManager, WalEntry, OperationType};
pub use mvcc::{MvccManager, Snapshot, TransactionContext};
