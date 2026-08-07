//! KORE Layer 22 — Columnar Storage Engine
//!
//! Binary on-disk format for DataBlocks.
//!
//! File layout:
//! ```text
//! [magic:4]  [version:2]  [num_cols:4]  [num_rows:8]
//! Schema section — one entry per column:
//!   [name_len:2]  [name:name_len]  [dtype:1]
//! Data section — one block per column:
//!   [compression:1]  [has_nulls:1]  [null_bitmap:ceil(n/8)]  [data_len:8]  [data:data_len]
//! ```
//!
//! dtype:  1=i64  2=f64  3=bool  4=str
//! compression:  0=raw  1=rle  2=delta (i64 only)

pub mod compress;
pub mod reader;
pub mod writer;

pub use reader::KoreReader;
pub use writer::KoreWriter;

use kore_core::KoreError;

pub const MAGIC:   &[u8; 4] = b"KORE";
pub const VERSION: u16       = 2;   // v2: native StrDict + f64 dict + NaN sentinel

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DType { 
    I64 = 1, F64 = 2, Bool = 3, Str = 4, StrDict = 5,
    Array = 6,   // Array<T>, element type stored separately
    Struct = 7,  // Struct with named fields
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Compression { Raw = 0, Rle = 1, Delta = 2, Dict = 3, NanRaw = 4, Lz4 = 5, Zstd = 6 }

impl TryFrom<u8> for DType {
    type Error = KoreError;
    fn try_from(v: u8) -> Result<Self, KoreError> {
        match v {
            1 => Ok(DType::I64), 2 => Ok(DType::F64),
            3 => Ok(DType::Bool), 4 => Ok(DType::Str), 5 => Ok(DType::StrDict),
            6 => Ok(DType::Array), 7 => Ok(DType::Struct),
            _ => Err(KoreError::InvalidArgument(format!("unknown dtype {v}"))),
        }
    }
}

/// Schema evolution metadata — tracks column history and changes
#[derive(Debug, Clone)]
pub struct SchemaEvolution {
    pub col_id: u32,           // Unique column ID
    pub added_version: u16,    // Schema version when added
    pub deprecated_version: Option<u16>, // Removed in this version
}

/// Version snapshot for MVCC + time travel
#[derive(Debug, Clone)]
pub struct VersionSnapshot {
    pub version_id: u32,       // Unique version ID
    pub timestamp: u64,        // Unix timestamp (ns)
    pub block_offset: u64,     // Byte offset of data block
    pub row_count: u64,        // Rows in this version
    pub prev_version: Option<u32>, // Link to previous version
}

/// Partition specification for partition evolution
#[derive(Debug, Clone)]
pub struct PartitionSpec {
    pub spec_id: u16,          // Partition spec version
    pub columns: Vec<u16>,     // Column indices to partition on
    pub transforms: Vec<String>, // "identity", "bucket(N)", "year", "month", etc.
    pub parent_spec_id: Option<u16>, // Previous partition spec
}

/// Delete vector for row-level soft deletes
#[derive(Debug, Clone)]
pub struct DeleteVector {
    pub bitmap: Vec<u8>,       // Bit = 1 if row deleted
    pub cardinality: u32,      // Number of deleted rows
    pub timestamp: u64,        // When rows were deleted
}

/// Append mode metadata — supports incremental writes
#[derive(Debug, Clone)]
pub struct AppendMetadata {
    pub is_append_mode: bool,  // True if file supports append writes
    pub num_blocks: u32,       // Number of blocks written so far
    pub block_offsets: Vec<u64>, // Byte offset of each block start
}

/// Encryption metadata — AES-256-GCM per column
#[derive(Debug, Clone)]
pub struct EncryptionMetadata {
    pub encrypted_cols: Vec<u32>, // Column IDs that are encrypted
    pub algorithm: String,         // "AES-256-GCM"
    pub kdf: String,              // "PBKDF2" or "scrypt"
    pub salt: Vec<u8>,            // Salt for KDF
    pub nonce: Vec<u8>,           // Nonce/IV
}

impl TryFrom<u8> for Compression {
    type Error = KoreError;
    fn try_from(v: u8) -> Result<Self, KoreError> {
        match v {
            0 => Ok(Compression::Raw), 1 => Ok(Compression::Rle),
            2 => Ok(Compression::Delta), 3 => Ok(Compression::Dict),
            4 => Ok(Compression::NanRaw), 5 => Ok(Compression::Lz4),
            6 => Ok(Compression::Zstd),
            _ => Err(KoreError::InvalidArgument(format!("unknown compression {v}"))),
        }
    }
}

