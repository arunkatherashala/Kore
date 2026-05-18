//! KORE File Format v2.0 - Reader with decompression support
//!
//! Reads KORE binary format files and decompresses columns using registered codecs.
//! Supports both v1.0 (no compression) and v2.0 (multi-codec) files.

use crate::decompression::{CodecId, CodecRegistry};
use crate::binary_format::BinaryFormatError;
use std::io::{Read, Cursor};

const MAGIC_BYTES: &[u8; 4] = b"KORE";
const FORMAT_VERSION_V1: u8 = 0x01;
const FORMAT_VERSION_V2: u8 = 0x02;

/// Metadata about a single column in KORE file
#[derive(Debug, Clone)]
pub struct ColumnMetadata {
    /// Column name
    pub name: String,
    /// Data type (0=i64, 1=f64, 2=string, 3=bool, 4=bytes)
    pub data_type: u8,
    /// Codec used for this column (v2.0 only)
    pub codec_id: CodecId,
    /// Offset into data section
    pub offset: u64,
    /// Compressed size in bytes
    pub compressed_size: u64,
    /// Uncompressed size in bytes
    pub uncompressed_size: u64,
}

/// Header information from KORE file
#[derive(Debug)]
pub struct KoreHeader {
    pub version: u8,
    pub column_count: u32,
    pub row_count: u64,
    pub columns: Vec<ColumnMetadata>,
}

/// KORE file reader with decompression
pub struct KoreReader {
    data: Cursor<Vec<u8>>,
    header: KoreHeader,
}

impl KoreReader {
    /// Create a reader from file bytes
    pub fn new(file_bytes: Vec<u8>) -> Result<Self, BinaryFormatError> {
        let mut cursor = Cursor::new(file_bytes);
        let header = Self::read_header(&mut cursor)?;

        Ok(KoreReader {
            data: cursor,
            header,
        })
    }

    /// Read and validate file header
    fn read_header(cursor: &mut Cursor<Vec<u8>>) -> Result<KoreHeader, BinaryFormatError> {
        let mut magic = [0u8; 4];
        cursor
            .read_exact(&mut magic)
            .map_err(|e| BinaryFormatError::DecompressionError(format!("Failed to read magic: {}", e)))?;

        if magic != MAGIC_BYTES[..] {
            return Err(BinaryFormatError::DecompressionError(
                "Invalid KORE file: wrong magic bytes".to_string(),
            ));
        }

        let mut version_byte = [0u8; 1];
        cursor
            .read_exact(&mut version_byte)
            .map_err(|e| BinaryFormatError::DecompressionError(format!("Failed to read version: {}", e)))?;

        let version = version_byte[0];
        if version != FORMAT_VERSION_V1 && version != FORMAT_VERSION_V2 {
            return Err(BinaryFormatError::DecompressionError(
                format!("Unsupported KORE version: {}", version),
            ));
        }

        // Read column count (4 bytes, little-endian)
        let mut col_count_bytes = [0u8; 4];
        cursor.read_exact(&mut col_count_bytes).map_err(|e| {
            BinaryFormatError::DecompressionError(format!("Failed to read column count: {}", e))
        })?;
        let column_count = u32::from_le_bytes(col_count_bytes);

        // Read row count (8 bytes, little-endian)
        let mut row_count_bytes = [0u8; 8];
        cursor.read_exact(&mut row_count_bytes).map_err(|e| {
            BinaryFormatError::DecompressionError(format!("Failed to read row count: {}", e))
        })?;
        let row_count = u64::from_le_bytes(row_count_bytes);

        // Read column metadata
        let mut columns = Vec::new();
        for _ in 0..column_count {
            let col = Self::read_column_metadata(cursor, version)?;
            columns.push(col);
        }

        Ok(KoreHeader {
            version,
            column_count,
            row_count,
            columns,
        })
    }

    /// Read metadata for a single column
    fn read_column_metadata(
        cursor: &mut Cursor<Vec<u8>>,
        version: u8,
    ) -> Result<ColumnMetadata, BinaryFormatError> {
        // Read name length (1 byte)
        let mut name_len_byte = [0u8; 1];
        cursor.read_exact(&mut name_len_byte).map_err(|e| {
            BinaryFormatError::DecompressionError(format!("Failed to read name length: {}", e))
        })?;
        let name_len = name_len_byte[0] as usize;

        // Read name
        let mut name_bytes = vec![0u8; name_len];
        cursor.read_exact(&mut name_bytes).map_err(|e| {
            BinaryFormatError::DecompressionError(format!("Failed to read column name: {}", e))
        })?;
        let name =
            String::from_utf8(name_bytes).map_err(|e| {
                BinaryFormatError::DecompressionError(format!("Invalid UTF-8 in column name: {}", e))
            })?;

        // Read data type (1 byte)
        let mut data_type_byte = [0u8; 1];
        cursor.read_exact(&mut data_type_byte).map_err(|e| {
            BinaryFormatError::DecompressionError(format!("Failed to read data type: {}", e))
        })?;
        let data_type = data_type_byte[0];

        // Read codec ID (v2.0 only)
        let codec_id = if version == FORMAT_VERSION_V2 {
            let mut codec_byte = [0u8; 1];
            cursor.read_exact(&mut codec_byte).map_err(|e| {
                BinaryFormatError::DecompressionError(format!("Failed to read codec ID: {}", e))
            })?;
            CodecId::from_u8(codec_byte[0])?
        } else {
            CodecId::None // v1.0 files have no compression
        };

        // Read offset (8 bytes)
        let mut offset_bytes = [0u8; 8];
        cursor.read_exact(&mut offset_bytes).map_err(|e| {
            BinaryFormatError::DecompressionError(format!("Failed to read offset: {}", e))
        })?;
        let offset = u64::from_le_bytes(offset_bytes);

        // Read compressed size (8 bytes)
        let mut comp_size_bytes = [0u8; 8];
        cursor.read_exact(&mut comp_size_bytes).map_err(|e| {
            BinaryFormatError::DecompressionError(format!("Failed to read compressed size: {}", e))
        })?;
        let compressed_size = u64::from_le_bytes(comp_size_bytes);

        // Read uncompressed size (8 bytes)
        let mut uncomp_size_bytes = [0u8; 8];
        cursor.read_exact(&mut uncomp_size_bytes).map_err(|e| {
            BinaryFormatError::DecompressionError(format!("Failed to read uncompressed size: {}", e))
        })?;
        let uncompressed_size = u64::from_le_bytes(uncomp_size_bytes);

        Ok(ColumnMetadata {
            name,
            data_type,
            codec_id,
            offset,
            compressed_size,
            uncompressed_size,
        })
    }

    /// Read and decompress a column
    pub fn read_column(&mut self, column_idx: usize) -> Result<Vec<u8>, BinaryFormatError> {
        if column_idx >= self.header.columns.len() {
            return Err(BinaryFormatError::DecompressionError(
                format!("Column index out of range: {}", column_idx),
            ));
        }

        let col = &self.header.columns[column_idx];
        let base_pos = self.data.get_ref().len() as u64 - self.data.get_ref().len() as u64 + col.offset;
        self.data.set_position(base_pos);

        // Read compressed data
        let mut compressed_data = vec![0u8; col.compressed_size as usize];
        self.data
            .read_exact(&mut compressed_data)
            .map_err(|e| {
                BinaryFormatError::DecompressionError(format!("Failed to read column data: {}", e))
            })?;

        // Decompress using appropriate codec
        CodecRegistry::decompress(col.codec_id, &compressed_data)
    }

    /// Get header information
    pub fn header(&self) -> &KoreHeader {
        &self.header
    }

    /// Get column metadata by name
    pub fn get_column_by_name(&self, name: &str) -> Option<&ColumnMetadata> {
        self.header.columns.iter().find(|c| c.name == name)
    }

    /// Get all column names
    pub fn column_names(&self) -> Vec<&str> {
        self.header.columns.iter().map(|c| c.name.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_bytes_validation() {
        let invalid_data = vec![0x00, 0x01, 0x02, 0x03];
        let result = KoreReader::new(invalid_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_version_validation() {
        let mut data = Vec::from(&b"KORE"[..]);
        data.push(0xFF); // Invalid version
        let result = KoreReader::new(data);
        assert!(result.is_err());
    }
}
