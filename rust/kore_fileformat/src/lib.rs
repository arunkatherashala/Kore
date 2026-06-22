use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, Write};

// ============================================================================
// TRACK A: Performance & SIMD Optimizations
// ============================================================================
#[cfg(feature = "simd-optimize")]
pub mod codecs_simd;

#[cfg(feature = "pyo3")]
pub mod bindings_pyo3;

// ============================================================================
// TRACK B: Ecosystem Integration
// ============================================================================
#[cfg(feature = "duckdb-ffi")]
pub mod ffi_duckdb;

// ============================================================================
// TRACK C: Compliance & Security
// ============================================================================
// WAL and audit logging already in wal.rs

// ============================================================================
// TRACK D: Time-Series Optimization
// ============================================================================
#[cfg(feature = "timeseries-opt")]
pub mod codec_timeseries;

// ============================================================================
// TRACK E: GPU & Advanced Features
// ============================================================================
#[cfg(feature = "gpu-cuda")]
pub mod gpu_cuda;

// ============================================================================
// TRACK F: ACID Transactions (Phase 2 Implementation)
// ============================================================================
#[cfg(feature = "acid-transactions")]
pub mod transactions;

// ============================================================================
// COMPREHENSIVE TRACK TESTING (All Tracks A-F)
// ============================================================================
#[cfg(test)]
mod track_tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnStats {
    pub min: Option<String>,
    pub max: Option<String>,
    pub null_count: u64,
}

/// Compact binary representation of per-row-group metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RowGroupMetadata {
    pub row_count: u64,
    pub column_stats: Vec<ColumnStats>,
}

impl RowGroupMetadata {
    pub fn to_bytes(&self) -> Vec<u8> {
        // Simple binary format: row_count (u64) + num_cols (u32) + [for each col: min_len (u32) + min + max_len (u32) + max + null_count (u64)]
        let mut buf = Vec::new();
        buf.extend_from_slice(&self.row_count.to_le_bytes());
        buf.extend_from_slice(&(self.column_stats.len() as u32).to_le_bytes());
        for stat in &self.column_stats {
            let min_bytes = stat.min.as_ref().map(|s| s.as_bytes()).unwrap_or(b"");
            buf.extend_from_slice(&(min_bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(min_bytes);
            let max_bytes = stat.max.as_ref().map(|s| s.as_bytes()).unwrap_or(b"");
            buf.extend_from_slice(&(max_bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(max_bytes);
            buf.extend_from_slice(&stat.null_count.to_le_bytes());
        }
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        let mut cursor = std::io::Cursor::new(bytes);
        let mut buf8 = [0u8; 8];
        let mut buf4 = [0u8; 4];

        cursor.read_exact(&mut buf8)?;
        let row_count = u64::from_le_bytes(buf8);

        cursor.read_exact(&mut buf4)?;
        let num_cols = u32::from_le_bytes(buf4) as usize;

        let mut column_stats = Vec::new();
        for _ in 0..num_cols {
            cursor.read_exact(&mut buf4)?;
            let min_len = u32::from_le_bytes(buf4) as usize;
            let mut min_buf = vec![0u8; min_len];
            cursor.read_exact(&mut min_buf)?;
            let min = if min_len > 0 { Some(String::from_utf8_lossy(&min_buf).into_owned()) } else { None };

            cursor.read_exact(&mut buf4)?;
            let max_len = u32::from_le_bytes(buf4) as usize;
            let mut max_buf = vec![0u8; max_len];
            cursor.read_exact(&mut max_buf)?;
            let max = if max_len > 0 { Some(String::from_utf8_lossy(&max_buf).into_owned()) } else { None };

            cursor.read_exact(&mut buf8)?;
            let null_count = u64::from_le_bytes(buf8);

            column_stats.push(ColumnStats { min, max, null_count });
        }

        Ok(Self { row_count, column_stats })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Footer {
    pub version: u32,
    pub column_stats: Vec<ColumnStats>,
}

impl Footer {
    pub fn new(version: u32, column_stats: Vec<ColumnStats>) -> Self {
        Self { version, column_stats }
    }
}

pub struct KoreReader {
    footer: Footer,
}

impl KoreReader {
    pub fn from_footer(footer: Footer) -> Self {
        Self { footer }
    }

    pub fn column_stats(&self) -> &Vec<ColumnStats> {
        &self.footer.column_stats
    }
}

impl Footer {
    /// Serialize footer to JSON bytes (demo helper).
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serialize footer")
    }

    /// Deserialize footer from JSON bytes (demo helper).
    pub fn from_bytes(bytes: &[u8]) -> Self {
        serde_json::from_slice(bytes).expect("deserialize footer")
    }
}

/// Simple on-disk writer/reader helpers for demo purposes.
pub fn write_file_with_footer(path: &std::path::Path, payload: &[u8], footer: &Footer) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    f.write_all(payload)?;
    let footer_bytes = footer.to_bytes();
    // write footer length as u32 little-endian
    let len = footer_bytes.len() as u32;
    f.write_all(&footer_bytes)?;
    f.write_all(&len.to_le_bytes())?;
    Ok(())
}

pub fn read_footer_from_file(path: &std::path::Path) -> std::io::Result<Footer> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let meta = f.metadata()?;
    if meta.len() < 4 {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "file too small"));
    }
    // read last 4 bytes for length
    f.seek(std::io::SeekFrom::End(-4))?;
    let mut len_bytes = [0u8;4];
    f.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as u64;
    // read footer bytes
    if meta.len() < 4 + len {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "invalid footer length"));
    }
    f.seek(std::io::SeekFrom::End(-(4 + len as i64)))?;
    let mut buf = vec![0u8; len as usize];
    f.read_exact(&mut buf)?;
    Ok(Footer::from_bytes(&buf))
}

/// Read the binary RowGroupMetadata appended at file end (our compact footer format).
pub fn read_row_group_metadata_from_file(path: &std::path::Path) -> std::io::Result<RowGroupMetadata> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let meta = f.metadata()?;
    if meta.len() < 4 {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "file too small"));
    }
    // Last 4 bytes indicate the row-group metadata length
    f.seek(std::io::SeekFrom::End(-4))?;
    let mut len_bytes = [0u8;4];
    f.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as u64;
    if meta.len() < 4 + len {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "invalid rg metadata length"));
    }
    f.seek(std::io::SeekFrom::End(-(4 + len as i64)))?;
    let mut buf = vec![0u8; len as usize];
    f.read_exact(&mut buf)?;
    RowGroupMetadata::from_bytes(&buf)
}

/// Read RowGroupMetadata from any `Read + Seek` implementor (convenience API)
pub fn read_row_group_metadata_from_reader<R: Read + Seek>(mut reader: R) -> std::io::Result<RowGroupMetadata> {
    // Seek to last 4 bytes to get length
    let cur_pos = reader.seek(std::io::SeekFrom::End(0))?;
    if cur_pos < 4 {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "reader too small"));
    }
    reader.seek(std::io::SeekFrom::End(-4))?;
    let mut len_bytes = [0u8;4];
    reader.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as u64;
    if cur_pos < 4 + len {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "invalid rg metadata length"));
    }
    reader.seek(std::io::SeekFrom::End(-(4 + len as i64)))?;
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf)?;
    RowGroupMetadata::from_bytes(&buf)
}

pub mod kore_block_decoder;
pub mod compaction;
pub mod predicate;
pub mod expression;
pub mod codecs;
pub mod txn;
pub mod wal;

#[cfg(test)]
mod io_roundtrip {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn write_and_read_footer_file() {
        let tmp = std::env::temp_dir().join("kore_footer_test.bin");
        let payload = b"HELLODATA";
        let stats = vec![ColumnStats { min: Some("1".into()), max: Some("9".into()), null_count: 0 }];
        let footer = Footer::new(1, stats.clone());
        write_file_with_footer(&tmp, payload, &footer).expect("write");
        let got = read_footer_from_file(&tmp).expect("read");
        assert_eq!(got.version, 1);
        assert_eq!(got.column_stats[0].min.as_deref(), Some("1"));
        let _ = std::fs::remove_file(tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_roundtrip() {
        let stats = vec![ColumnStats { min: Some("1".into()), max: Some("10".into()), null_count: 0 }];
        let footer = Footer::new(1, stats.clone());
        let reader = KoreReader::from_footer(footer);
        assert_eq!(reader.column_stats().len(), 1);
        assert_eq!(reader.column_stats()[0].min.as_deref(), Some("1"));
    }
}

#[cfg(test)]
mod io_tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn footer_serialize_deserialize() {
        let stats = vec![ColumnStats { min: Some("5".into()), max: Some("20".into()), null_count: 2 }];
        let footer = Footer::new(2, stats.clone());
        let bytes = footer.to_bytes();
        let got = Footer::from_bytes(&bytes);
        assert_eq!(got.version, 2);
        assert_eq!(got.column_stats.len(), 1);
        assert_eq!(got.column_stats[0].max.as_deref(), Some("20"));
    }

    #[test]
    fn row_group_metadata_binary_roundtrip() {
        let stats = vec![
            ColumnStats { min: Some("0".into()), max: Some("100".into()), null_count: 0 },
            ColumnStats { min: Some("a".into()), max: Some("z".into()), null_count: 5 },
        ];
        let rg = RowGroupMetadata { row_count: 1000, column_stats: stats };
        let bytes = rg.to_bytes();
        let got = RowGroupMetadata::from_bytes(&bytes).expect("deserialize");
        assert_eq!(got.row_count, 1000);
        assert_eq!(got.column_stats.len(), 2);
        assert_eq!(got.column_stats[0].min.as_deref(), Some("0"));
        assert_eq!(got.column_stats[1].null_count, 5);

        // Test reader convenience API (in-memory)
        let mut cursor = Cursor::new(Vec::new());
        // write bytes + length suffix
        cursor.write_all(&bytes).expect("write");
        let len = bytes.len() as u32;
        cursor.write_all(&len.to_le_bytes()).expect("write len");
        // seek back to start and read via API
        cursor.seek(std::io::SeekFrom::Start(0)).expect("seek");
        let rg2 = read_row_group_metadata_from_reader(cursor).expect("read rg");
        assert_eq!(rg2.row_count, 1000);
        assert_eq!(rg2.column_stats[1].null_count, 5);
    }
}
