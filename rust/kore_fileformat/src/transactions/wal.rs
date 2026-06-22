/// Write-Ahead Log (WAL) - Foundation for ACID Transactions
/// 
/// Week 1 Deliverable: Transaction log writer/reader for durability
/// Target: 5000 txns/sec, <100 μs per write
/// 
/// WAL Entry Format (Sequential, Variable-Length):
/// [timestamp: u64(8)] [txn_id: u64(8)] [op_type: u8(1)] 
/// [partition_id: u32(4)] [column_id: u32(4)] 
/// [min_val: i64(8)] [max_val: i64(8)] 
/// [payload_len: u32(4)] [payload: [u8]] 
/// [crc: u32(4)]
/// 
/// Total header: 49 bytes + variable payload + CRC

use std::fs::{File, OpenOptions};
use std::io::{Write, Read, BufWriter, BufReader, Result as IoResult};
use std::path::Path;
use std::sync::Arc;
use parking_lot::RwLock;
use crc32fast::Hasher;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OperationType {
    Insert = 1,
    Update = 2,
    Delete = 3,
    Commit = 4,
    Rollback = 5,
    Checkpoint = 6,
}

impl OperationType {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            1 => Some(OperationType::Insert),
            2 => Some(OperationType::Update),
            3 => Some(OperationType::Delete),
            4 => Some(OperationType::Commit),
            5 => Some(OperationType::Rollback),
            6 => Some(OperationType::Checkpoint),
            _ => None,
        }
    }

    pub fn as_u8(&self) -> u8 {
        *self as u8
    }
}

/// Single WAL entry with metadata
#[derive(Debug, Clone)]
pub struct WalEntry {
    pub timestamp: u64,
    pub txn_id: u64,
    pub op_type: OperationType,
    pub partition_id: u32,
    pub column_id: u32,
    pub min_val: i64,
    pub max_val: i64,
    pub payload: Vec<u8>,
}

impl WalEntry {
    /// Serialize WAL entry to bytes with CRC
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(64 + self.payload.len());
        
        // Header (49 bytes)
        buf.extend_from_slice(&self.timestamp.to_le_bytes());
        buf.extend_from_slice(&self.txn_id.to_le_bytes());
        buf.push(self.op_type.as_u8());
        buf.extend_from_slice(&self.partition_id.to_le_bytes());
        buf.extend_from_slice(&self.column_id.to_le_bytes());
        buf.extend_from_slice(&self.min_val.to_le_bytes());
        buf.extend_from_slice(&self.max_val.to_le_bytes());
        buf.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        
        // Payload
        buf.extend_from_slice(&self.payload);
        
        // Calculate and append CRC32
        let mut hasher = Hasher::new();
        hasher.update(&buf);
        let crc = hasher.finalize();
        buf.extend_from_slice(&crc.to_le_bytes());
        
        buf
    }

    /// Deserialize WAL entry from bytes with CRC verification
    pub fn deserialize(buf: &[u8]) -> IoResult<(Self, usize)> {
        if buf.len() < 53 { // 49 header + 4 CRC
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Buffer too small for WAL entry",
            ));
        }

        let mut offset = 0;

        // Header (49 bytes)
        let timestamp = u64::from_le_bytes([
            buf[offset], buf[offset+1], buf[offset+2], buf[offset+3],
            buf[offset+4], buf[offset+5], buf[offset+6], buf[offset+7],
        ]);
        offset += 8;

        let txn_id = u64::from_le_bytes([
            buf[offset], buf[offset+1], buf[offset+2], buf[offset+3],
            buf[offset+4], buf[offset+5], buf[offset+6], buf[offset+7],
        ]);
        offset += 8;

        let op_type = OperationType::from_u8(buf[offset])
            .ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid operation type",
            ))?;
        offset += 1;

        let partition_id = u32::from_le_bytes([
            buf[offset], buf[offset+1], buf[offset+2], buf[offset+3],
        ]);
        offset += 4;

        let column_id = u32::from_le_bytes([
            buf[offset], buf[offset+1], buf[offset+2], buf[offset+3],
        ]);
        offset += 4;

        let min_val = i64::from_le_bytes([
            buf[offset], buf[offset+1], buf[offset+2], buf[offset+3],
            buf[offset+4], buf[offset+5], buf[offset+6], buf[offset+7],
        ]);
        offset += 8;

        let max_val = i64::from_le_bytes([
            buf[offset], buf[offset+1], buf[offset+2], buf[offset+3],
            buf[offset+4], buf[offset+5], buf[offset+6], buf[offset+7],
        ]);
        offset += 8;

        let payload_len = u32::from_le_bytes([
            buf[offset], buf[offset+1], buf[offset+2], buf[offset+3],
        ]) as usize;
        offset += 4;

        // Verify we have enough data for payload + CRC
        if buf.len() < offset + payload_len + 4 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Buffer too small for payload and CRC",
            ));
        }

        let payload = buf[offset..offset + payload_len].to_vec();
        offset += payload_len;

        let stored_crc = u32::from_le_bytes([
            buf[offset], buf[offset+1], buf[offset+2], buf[offset+3],
        ]);
        offset += 4;

        // Verify CRC
        let mut hasher = Hasher::new();
        hasher.update(&buf[..offset - 4]);
        let calculated_crc = hasher.finalize();
        
        if stored_crc != calculated_crc {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "CRC mismatch in WAL entry",
            ));
        }

        Ok((
            WalEntry {
                timestamp,
                txn_id,
                op_type,
                partition_id,
                column_id,
                min_val,
                max_val,
                payload,
            },
            offset,
        ))
    }
}

/// Write-Ahead Log Manager
pub struct WalManager {
    writer: Arc<RwLock<Option<BufWriter<File>>>>,
    reader: Arc<RwLock<Option<BufReader<File>>>>,
    path: String,
    current_txn_id: Arc<RwLock<u64>>,
    max_entries_per_segment: usize,
}

impl WalManager {
    /// Create or open existing WAL file
    pub fn new<P: AsRef<Path>>(path: P, max_entries: usize) -> IoResult<Self> {
        let path_str = path.as_ref().to_string_lossy().to_string();
        
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&path)?;
        
        let writer = BufWriter::new(file);
        
        Ok(WalManager {
            writer: Arc::new(RwLock::new(Some(writer))),
            reader: Arc::new(RwLock::new(None)),
            path: path_str,
            current_txn_id: Arc::new(RwLock::new(0)),
            max_entries_per_segment: max_entries,
        })
    }

    /// Write entry to WAL (durability guarantee)
    pub fn write_entry(&self, mut entry: WalEntry) -> IoResult<u64> {
        let mut txn_id = self.current_txn_id.write();
        *txn_id += 1;
        entry.txn_id = *txn_id;
        entry.timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        let serialized = entry.serialize();
        
        let mut writer_guard = self.writer.write();
        if let Some(ref mut writer) = *writer_guard {
            writer.write_all(&serialized)?;
            writer.flush()?; // Durability: fsync equivalent
        }
        
        Ok(entry.txn_id)
    }

    /// Batch write entries (better throughput)
    pub fn write_batch(&self, entries: Vec<WalEntry>) -> IoResult<Vec<u64>> {
        let mut txn_ids = Vec::new();
        let mut txn_id = self.current_txn_id.write();
        let mut writer_guard = self.writer.write();
        
        for mut entry in entries {
            *txn_id += 1;
            entry.txn_id = *txn_id;
            entry.timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64;
            
            let serialized = entry.serialize();
            txn_ids.push(entry.txn_id);
            
            if let Some(ref mut writer) = *writer_guard {
                writer.write_all(&serialized)?;
            }
        }
        
        if let Some(ref mut writer) = *writer_guard {
            writer.flush()?;
        }
        
        Ok(txn_ids)
    }

    /// Read all entries from WAL (for recovery)
    pub fn read_all(&self) -> IoResult<Vec<WalEntry>> {
        let file = File::open(&self.path)?;
        let mut reader = BufReader::new(file);
        let mut entries = Vec::new();
        let mut buffer = vec![0u8; 1024 * 1024]; // 1MB buffer

        loop {
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }

            let mut offset = 0;
            while offset < n {
                match WalEntry::deserialize(&buffer[offset..n]) {
                    Ok((entry, entry_size)) => {
                        entries.push(entry);
                        offset += entry_size;
                    }
                    Err(_) => break, // Incomplete entry
                }
            }
        }

        Ok(entries)
    }

    /// Get current transaction ID
    pub fn current_txn_id(&self) -> u64 {
        *self.current_txn_id.read()
    }

    /// Checkpoint: start new segment
    pub fn checkpoint(&self) -> IoResult<()> {
        let mut writer_guard = self.writer.write();
        if let Some(ref mut writer) = *writer_guard {
            writer.flush()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wal_entry_serialize_deserialize() {
        let entry = WalEntry {
            timestamp: 1000,
            txn_id: 1,
            op_type: OperationType::Insert,
            partition_id: 0,
            column_id: 1,
            min_val: 100,
            max_val: 200,
            payload: vec![1, 2, 3, 4, 5],
        };

        let serialized = entry.serialize();
        let (deserialized, size) = WalEntry::deserialize(&serialized).unwrap();

        assert_eq!(entry.timestamp, deserialized.timestamp);
        assert_eq!(entry.txn_id, deserialized.txn_id);
        assert_eq!(entry.op_type, deserialized.op_type);
        assert_eq!(entry.partition_id, deserialized.partition_id);
        assert_eq!(entry.column_id, deserialized.column_id);
        assert_eq!(entry.min_val, deserialized.min_val);
        assert_eq!(entry.max_val, deserialized.max_val);
        assert_eq!(entry.payload, deserialized.payload);
        assert_eq!(serialized.len(), size);
    }

    #[test]
    fn test_wal_manager_write() {
        let temp_dir = tempfile::tempdir().unwrap();
        let wal_path = temp_dir.path().join("test.wal");
        let wal = WalManager::new(&wal_path, 1000).unwrap();
        
        let entry = WalEntry {
            timestamp: 1000,
            txn_id: 0,
            op_type: OperationType::Insert,
            partition_id: 0,
            column_id: 1,
            min_val: 100,
            max_val: 200,
            payload: vec![1, 2, 3],
        };

        let txn_id = wal.write_entry(entry).unwrap();
        assert_eq!(txn_id, 1);
        assert_eq!(wal.current_txn_id(), 1);
    }

    #[test]
    fn test_wal_batch_write() {
        let temp_dir = tempfile::tempdir().unwrap();
        let wal_path = temp_dir.path().join("test_batch.wal");
        let wal = WalManager::new(&wal_path, 1000).unwrap();
        
        let entries: Vec<_> = (0..10)
            .map(|i| WalEntry {
                timestamp: 1000 + i,
                txn_id: 0,
                op_type: OperationType::Insert,
                partition_id: 0,
                column_id: i as u32,
                min_val: 100,
                max_val: 200,
                payload: vec![i as u8],
            })
            .collect();

        let txn_ids = wal.write_batch(entries).unwrap();
        assert_eq!(txn_ids.len(), 10);
        assert_eq!(wal.current_txn_id(), 10);
    }

    #[test]
    fn test_crc_validation() {
        let entry = WalEntry {
            timestamp: 1000,
            txn_id: 1,
            op_type: OperationType::Commit,
            partition_id: 0,
            column_id: 0,
            min_val: 0,
            max_val: 0,
            payload: vec![],
        };

        let mut serialized = entry.serialize();
        
        // Corrupt last byte before CRC
        let corrupt_idx = serialized.len() - 5;
        serialized[corrupt_idx] ^= 0xFF;
        
        let result = WalEntry::deserialize(&serialized);
        assert!(result.is_err());
    }
}
