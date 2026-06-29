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
pub const VERSION: u16       = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DType { I64 = 1, F64 = 2, Bool = 3, Str = 4 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Compression { Raw = 0, Rle = 1, Delta = 2 }

impl TryFrom<u8> for DType {
    type Error = KoreError;
    fn try_from(v: u8) -> Result<Self, KoreError> {
        match v {
            1 => Ok(DType::I64), 2 => Ok(DType::F64),
            3 => Ok(DType::Bool), 4 => Ok(DType::Str),
            _ => Err(KoreError::InvalidArgument(format!("unknown dtype {v}"))),
        }
    }
}

impl TryFrom<u8> for Compression {
    type Error = KoreError;
    fn try_from(v: u8) -> Result<Self, KoreError> {
        match v {
            0 => Ok(Compression::Raw), 1 => Ok(Compression::Rle), 2 => Ok(Compression::Delta),
            _ => Err(KoreError::InvalidArgument(format!("unknown compression {v}"))),
        }
    }
}

