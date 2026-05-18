/// Week 10: KoreWriter - File format writer with compression integration
/// 
/// Combines all Week 1-9 components:
/// 1. ColumnProfile analysis (Week 7)
/// 2. Codec selection (Week 7)
/// 3. Compression (Week 9)
/// 4. Format v2.0 writing (Week 1)
/// 
/// Writes Kore binary format with automatic codec selection per column

use crate::binary_format::BinaryFormatError;
use crate::codec_selector::{ColumnProfile, CodecSelector};
use crate::compression::CompressionRegistry;
use crate::decompression::CodecId;

/// Column data to write
#[derive(Clone, Debug)]
pub struct ColumnData {
    pub name: String,
    pub data_type: u8,
    pub data: Vec<u8>,
}

/// Kore file writer with automatic codec selection
pub struct KoreWriter {
    version: u32,
    columns: Vec<ColumnData>,
    row_count: u64,
}

/// Column metadata for header
#[derive(Clone, Debug)]
pub struct ColumnMetadata {
    pub name: String,
    pub data_type: u8,
    pub codec_id: CodecId,
    pub offset: u64,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
}

/// Write result with compression statistics
#[derive(Clone, Debug)]
pub struct WriteResult {
    pub total_bytes_written: u64,
    pub original_size: u64,
    pub compressed_size: u64,
    pub compression_ratio: f32,
    pub column_count: usize,
    pub row_count: u64,
    pub columns_metadata: Vec<ColumnMetadata>,
}

impl KoreWriter {
    /// Create new writer for v2.0 format
    pub fn new(row_count: u64) -> Self {
        Self {
            version: 2,
            columns: Vec::new(),
            row_count,
        }
    }

    /// Add a column to write
    pub fn add_column(&mut self, name: String, data_type: u8, data: Vec<u8>) {
        self.columns.push(ColumnData {
            name,
            data_type,
            data,
        });
    }

    /// Write all columns to bytes with automatic codec selection
    pub fn write(&self) -> Result<(Vec<u8>, WriteResult), BinaryFormatError> {
        let mut output = Vec::new();
        let mut column_metadata = Vec::new();
        let mut total_original = 0u64;
        let mut total_compressed = 0u64;

        // Pre-calculate header size to know where data starts
        // Magic (4) + Version (1) + Column count (4) + Row count (8) = 17 bytes
        let mut header_size = 17u64;
        for col in &self.columns {
            // name length (1) + name + data_type (1) + codec (1) + offset (8) + compressed_size (8) + uncompressed_size (8)
            header_size += 1 + col.name.len() as u64 + 1 + 1 + 8 + 8 + 8;
        }

        // Process each column
        let data_start_offset = header_size;
        let mut current_offset = data_start_offset;

        for col in &self.columns {
            // Step 1: Analyze column to select codec
            let profile = ColumnProfile::analyze(&col.data)
                .map_err(|e| BinaryFormatError::InvalidData(e))?;
            let codec = CodecSelector::select_optimal_codec(&profile);

            // Step 2: Compress with selected codec
            let (compressed_data, stats) = CompressionRegistry::compress(codec, &col.data)?;

            let uncompressed_size = col.data.len() as u64;
            let compressed_size = compressed_data.len() as u64;

            total_original += uncompressed_size;
            total_compressed += compressed_size;

            // Step 3: Record metadata (but don't write yet - save for header)
            column_metadata.push((
                ColumnMetadata {
                    name: col.name.clone(),
                    data_type: col.data_type,
                    codec_id: codec,
                    offset: current_offset,
                    compressed_size,
                    uncompressed_size,
                },
                compressed_data,
            ));

            current_offset += compressed_size;
        }

        // Now write: header + all compressed data
        Self::write_header(&mut output, self.version, self.row_count, &column_metadata)?;

        // Write all compressed data
        for (_metadata, compressed_data) in &column_metadata {
            output.extend_from_slice(compressed_data);
        }

        let compression_ratio = if total_original > 0 {
            total_compressed as f32 / total_original as f32
        } else {
            1.0
        };

        let final_metadata = column_metadata
            .into_iter()
            .map(|(m, _)| m)
            .collect();

        let result = WriteResult {
            total_bytes_written: output.len() as u64,
            original_size: total_original,
            compressed_size: total_compressed,
            compression_ratio,
            column_count: self.columns.len(),
            row_count: self.row_count,
            columns_metadata: final_metadata,
        };

        Ok((output, result))
    }

    /// Write header to output buffer
    fn write_header(
        output: &mut Vec<u8>,
        version: u32,
        row_count: u64,
        columns: &[(ColumnMetadata, Vec<u8>)],
    ) -> Result<(), BinaryFormatError> {
        // Header format v2.0 (matches KoreReader):
        // Bytes 0-3:   Magic bytes "KORE"
        // Byte 4:      Version (u8, value 2 for v2.0)
        // Bytes 5-8:   Column count (u32 LE)
        // Bytes 9-16:  Row count (u64 LE)
        // Bytes 17+:   Column metadata (repeated)

        // Magic bytes
        output.extend_from_slice(b"KORE");

        // Version (as u8, not u32)
        output.push(version as u8);

        // Column count (BEFORE row count)
        output.extend_from_slice(&(columns.len() as u32).to_le_bytes());

        // Row count
        output.extend_from_slice(&row_count.to_le_bytes());

        // Column metadata
        for (col, _) in columns {
            // Name length + name
            let name_bytes = col.name.as_bytes();
            output.push(name_bytes.len() as u8);
            output.extend_from_slice(name_bytes);

            // Data type
            output.push(col.data_type);

            // Codec ID
            output.push(col.codec_id.to_u8());

            // Offset, compressed size, uncompressed size
            output.extend_from_slice(&col.offset.to_le_bytes());
            output.extend_from_slice(&col.compressed_size.to_le_bytes());
            output.extend_from_slice(&col.uncompressed_size.to_le_bytes());
        }

        Ok(())
    }

    /// Get write result summary
    pub fn get_summary(result: &WriteResult) -> String {
        format!(
            "Wrote {} bytes: {} columns, {} rows\n\
             Original: {} bytes\n\
             Compressed: {} bytes ({:.1}% ratio)\n\
             Saved: {:.0} bytes ({:.1}% reduction)",
            result.total_bytes_written,
            result.column_count,
            result.row_count,
            result.original_size,
            result.compressed_size,
            result.compression_ratio * 100.0,
            result.original_size as i64 - result.compressed_size as i64,
            (1.0 - result.compression_ratio) * 100.0
        )
    }
}

/// Extension trait for CodecId to convert to u8
impl CodecId {
    pub fn to_u8(&self) -> u8 {
        match self {
            CodecId::None => 0,
            CodecId::RLE => 1,
            CodecId::Dictionary => 2,
            CodecId::FOR => 3,
            CodecId::LZSS => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_writer_creation() {
        let writer = KoreWriter::new(1000);
        assert_eq!(writer.version, 2);
        assert_eq!(writer.row_count, 1000);
        assert_eq!(writer.columns.len(), 0);
    }

    #[test]
    fn test_add_column() {
        let mut writer = KoreWriter::new(100);
        writer.add_column("col1".to_string(), 1, vec![0xFF; 50]);
        assert_eq!(writer.columns.len(), 1);
        assert_eq!(writer.columns[0].name, "col1");
    }

    #[test]
    fn test_write_single_column_rle() {
        let mut writer = KoreWriter::new(100);
        // Use larger data so compression isn't dwarfed by header
        writer.add_column("repetitive".to_string(), 1, vec![0xAA; 1000]);

        let (output, result) = writer.write().unwrap();

        assert!(output.len() > 0);
        assert_eq!(result.column_count, 1);
        assert_eq!(result.row_count, 100);
        // Repetitive data should compress better than 50%
        assert!(result.compression_ratio < 0.5);
    }

    #[test]
    fn test_write_multiple_columns() {
        let mut writer = KoreWriter::new(50);
        writer.add_column("col1".to_string(), 1, vec![0xFF; 50]);
        
        let mut col2_data = Vec::new();
        for _ in 0..10 {
            col2_data.extend_from_slice(&[1u8, 2, 3, 4, 5]);
        }
        writer.add_column("col2".to_string(), 1, col2_data);
        
        writer.add_column("col3".to_string(), 1, vec![0x42; 50]);

        let (output, result) = writer.write().unwrap();

        assert!(output.len() > 0);
        assert_eq!(result.column_count, 3);
        assert!(result.compression_ratio < 1.0);
    }

    #[test]
    fn test_write_result_compression_stats() {
        let mut writer = KoreWriter::new(100);
        writer.add_column("test".to_string(), 1, vec![0xAA; 1000]);

        let (_output, result) = writer.write().unwrap();

        assert_eq!(result.column_count, 1);
        assert!(result.compression_ratio > 0.0);
        assert!(result.compression_ratio < 1.0);
        assert!(result.compressed_size < result.original_size);
    }

    #[test]
    fn test_codec_selection_per_column() {
        let mut writer = KoreWriter::new(10);
        
        // RLE candidate: large repetitive data
        writer.add_column("rle_data".to_string(), 1, vec![0xFF; 1000]);
        
        // Dictionary candidate: low cardinality, larger dataset
        let mut dict_data = Vec::new();
        for _ in 0..100 {
            dict_data.extend_from_slice(&[1u8, 2, 3, 4, 5]);
        }
        writer.add_column("dict_data".to_string(), 1, dict_data);

        let (_output, result) = writer.write().unwrap();

        // First column should be RLE or Dictionary (repetitive)
        assert!([CodecId::RLE, CodecId::Dictionary].contains(&result.columns_metadata[0].codec_id));
        
        // Second column should be Dictionary (low cardinality)
        assert_eq!(result.columns_metadata[1].codec_id, CodecId::Dictionary);
    }

    #[test]
    fn test_write_empty_column() {
        let mut writer = KoreWriter::new(0);
        writer.add_column("empty".to_string(), 1, vec![]);

        let (_output, result) = writer.write().unwrap();

        assert_eq!(result.original_size, 0);
        assert_eq!(result.compressed_size, 0);
    }

    #[test]
    fn test_write_summary() {
        let result = WriteResult {
            total_bytes_written: 1000,
            original_size: 5000,
            compressed_size: 2500,
            compression_ratio: 0.5,
            column_count: 2,
            row_count: 100,
            columns_metadata: vec![],
        };

        let summary = KoreWriter::get_summary(&result);

        assert!(summary.contains("1000 bytes"));
        assert!(summary.contains("2 columns"));
        assert!(summary.contains("100 rows"));
        assert!(summary.contains("50.0%"));
    }

    #[test]
    fn test_codec_id_to_u8() {
        assert_eq!(CodecId::None.to_u8(), 0);
        assert_eq!(CodecId::RLE.to_u8(), 1);
        assert_eq!(CodecId::Dictionary.to_u8(), 2);
        assert_eq!(CodecId::FOR.to_u8(), 3);
        assert_eq!(CodecId::LZSS.to_u8(), 4);
    }

    #[test]
    fn test_multiple_write_calls() {
        let mut writer = KoreWriter::new(50);
        writer.add_column("data1".to_string(), 1, vec![0x11; 50]);

        let (output1, result1) = writer.write().unwrap();

        // Write again - should be same
        let (output2, result2) = writer.write().unwrap();

        assert_eq!(output1.len(), output2.len());
        assert_eq!(result1.compression_ratio, result2.compression_ratio);
    }

    #[test]
    fn test_write_high_entropy_data() {
        let mut writer = KoreWriter::new(10);
        let data: Vec<u8> = (0..100).map(|i| (i % 256) as u8).collect();
        writer.add_column("entropy".to_string(), 1, data);

        let (_output, result) = writer.write().unwrap();

        // High entropy data doesn't compress well but should still work
        assert!(result.compression_ratio > 0.8);
        assert_eq!(result.column_count, 1);
    }

    #[test]
    fn test_column_metadata_tracking() {
        let mut writer = KoreWriter::new(100);
        writer.add_column("col1".to_string(), 5, vec![0xFF; 200]);
        writer.add_column("col2".to_string(), 10, vec![0x42; 300]);

        let (_output, result) = writer.write().unwrap();

        assert_eq!(result.columns_metadata.len(), 2);
        assert_eq!(result.columns_metadata[0].name, "col1");
        assert_eq!(result.columns_metadata[0].data_type, 5);
        assert_eq!(result.columns_metadata[1].name, "col2");
        assert_eq!(result.columns_metadata[1].data_type, 10);
    }
}
