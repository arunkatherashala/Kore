/// Week 10 Complete: Full round-trip file I/O testing
/// 
/// Tests that verify:
/// 1. KoreWriter writes valid files
/// 2. KoreReader can read them back
/// 3. Data survives round-trip unchanged (byte-fidelity)
/// 4. Compression is real and effective
/// 5. Codec selection works in actual file I/O

use crate::kore_reader::KoreReader;
use crate::kore_writer::{ColumnData, KoreWriter, WriteResult};
use crate::binary_format::BinaryFormatError;

/// Full round-trip file I/O test result
#[derive(Clone, Debug)]
pub struct FileRoundTripResult {
    pub original_data: Vec<ColumnData>,
    pub file_bytes: Vec<u8>,
    pub read_back_data: Vec<Vec<u8>>,
    pub byte_fidelity: bool,
    pub compression_ratio: f32,
    pub write_result: Option<WriteResult>,
}

/// File I/O integration testing
pub struct FileIOValidator;

impl FileIOValidator {
    /// Validate complete write → read → verify cycle
    pub fn validate_roundtrip_file_io(
        columns: Vec<ColumnData>,
        row_count: u64,
    ) -> Result<FileRoundTripResult, BinaryFormatError> {
        // Step 1: Write file
        let mut writer = KoreWriter::new(row_count);
        for col in &columns {
            if let Some(bitmap) = &col.null_bitmap {
                writer.add_column_with_nulls(col.name.clone(), col.data_type, col.data.clone(), bitmap.clone());
            } else {
                writer.add_column(col.name.clone(), col.data_type, col.data.clone());
            }
        }
        let (file_bytes, write_result) = writer.write()?;

        // Step 2: Read file back
        let mut reader = KoreReader::new(file_bytes.clone())?;
        let mut read_back_data = Vec::new();
        
        for i in 0..columns.len() {
            let col_data = reader.read_column(i)?;
            read_back_data.push(col_data);
        }

        // Step 3: Verify byte-fidelity
        let mut byte_fidelity = true;
        for (i, col) in columns.iter().enumerate() {
            if read_back_data[i] != col.data {
                byte_fidelity = false;
                break;
            }
        }

        Ok(FileRoundTripResult {
            original_data: columns,
            file_bytes,
            read_back_data,
            byte_fidelity,
            compression_ratio: write_result.compression_ratio,
            write_result: Some(write_result),
        })
    }

    /// Validate file header is correct
    pub fn validate_file_header(file_bytes: &[u8]) -> Result<bool, BinaryFormatError> {
        if file_bytes.len() < 5 {
            return Ok(false);
        }

        // Check magic bytes
        if &file_bytes[0..4] != b"KORE" {
            return Ok(false);
        }

        // Check version is 2 (byte 4)
        let version = file_bytes[4];

        Ok(version == 2)
    }

    /// Validate compression effectiveness
    pub fn validate_compression_effectiveness(
        result: &FileRoundTripResult,
        min_compression: f32,
    ) -> bool {
        result.compression_ratio <= min_compression
    }

    /// Generate compression report for file I/O
    pub fn generate_file_report(result: &FileRoundTripResult) -> String {
        let write_res = match &result.write_result {
            Some(wr) => wr.clone(),
            None => return "No write result".to_string(),
        };

        format!(
            "File I/O Round-Trip Report:\n\
             ─────────────────────────\n\
             Columns: {}\n\
             File Size: {} bytes\n\
             Original: {} bytes\n\
             Compressed: {} bytes\n\
             Compression Ratio: {:.1}%\n\
             Compression Savings: {} bytes ({:.1}%)\n\
             Byte Fidelity: {}\n\
             ─────────────────────────",
            write_res.column_count,
            result.file_bytes.len(),
            write_res.original_size,
            write_res.compressed_size,
            write_res.compression_ratio * 100.0,
            write_res.original_size as i64 - write_res.compressed_size as i64,
            (1.0 - write_res.compression_ratio) * 100.0,
            if result.byte_fidelity { "✅ PASS" } else { "❌ FAIL" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_roundtrip_single_column() {
        let columns = vec![ColumnData {
            name: "test".to_string(),
            data_type: 1,
            data: vec![0xFF; 100],
            null_bitmap: None,
        }];

        let result = FileIOValidator::validate_roundtrip_file_io(columns, 100).unwrap();

        assert!(result.byte_fidelity);
        assert!(!result.file_bytes.is_empty());
    }

    #[test]
    fn test_file_roundtrip_multiple_columns() {
        let columns = vec![
            ColumnData {
                name: "col1".to_string(),
                data_type: 1,
                data: vec![0xFF; 100],
                null_bitmap: None,
            },
            ColumnData {
                name: "col2".to_string(),
                data_type: 2,
                data: vec![0x42; 100],
                null_bitmap: None,
            },
        ];

        let result = FileIOValidator::validate_roundtrip_file_io(columns, 100).unwrap();

        assert!(result.byte_fidelity);
        assert_eq!(result.read_back_data.len(), 2);
    }

    #[test]
    fn test_file_header_validation() {
        let columns = vec![ColumnData {
            name: "test".to_string(),
            data_type: 1,
            data: vec![0xFF; 50],
            null_bitmap: None,
        }];

        let result = FileIOValidator::validate_roundtrip_file_io(columns, 50).unwrap();

        // Header should be valid (version 2)
        assert!(FileIOValidator::validate_file_header(&result.file_bytes).unwrap());
    }

    #[test]
    fn test_compression_effectiveness() {
        let columns = vec![ColumnData {
            name: "repetitive".to_string(),
            data_type: 1,
            data: vec![0xAA; 1000],
            null_bitmap: None,
        }];

        let result = FileIOValidator::validate_roundtrip_file_io(columns, 1000).unwrap();

        // Should achieve 50% or better compression on repetitive data
        assert!(FileIOValidator::validate_compression_effectiveness(&result, 0.5));
    }

    #[test]
    fn test_file_report_generation() {
        let columns = vec![ColumnData {
            name: "test".to_string(),
            data_type: 1,
            data: vec![0xFF; 100],
            null_bitmap: None,
        }];

        let result = FileIOValidator::validate_roundtrip_file_io(columns, 100).unwrap();
        let report = FileIOValidator::generate_file_report(&result);

        assert!(report.contains("File I/O Round-Trip Report"));
        assert!(report.contains("Columns:"));
        assert!(report.contains("Byte Fidelity"));
    }

    #[test]
    fn test_multiple_roundtrip_cycles() {
        let test_cases = vec![
            vec![ColumnData {
                name: "rle".to_string(),
                data_type: 1,
                data: vec![0xFF; 500],
                null_bitmap: None,
            }],
            vec![ColumnData {
                name: "categorical".to_string(),
                data_type: 1,
                data: (0..100)
                    .cycle()
                    .take(200)
                    .map(|i| (i % 10) as u8)
                    .collect(),
                null_bitmap: None,
            }],
        ];

        for columns in test_cases {
            let result = FileIOValidator::validate_roundtrip_file_io(columns, 100).unwrap();
            assert!(result.byte_fidelity, "Round-trip failed for test case");
        }
    }

    #[test]
    fn test_empty_file() {
        let columns: Vec<ColumnData> = vec![];
        let result = FileIOValidator::validate_roundtrip_file_io(columns, 0).unwrap();

        assert_eq!(result.read_back_data.len(), 0);
    }

    #[test]
    fn test_large_file_compression() {
        let columns = vec![ColumnData {
            name: "large".to_string(),
            data_type: 1,
            data: vec![0xAA; 10000],
            null_bitmap: None,
        }];

        let result = FileIOValidator::validate_roundtrip_file_io(columns, 10000).unwrap();

        assert!(result.byte_fidelity);
        // Large repetitive files should compress very well
        assert!(result.compression_ratio < 0.3);
    }

    #[test]
    fn test_null_bitmap_roundtrip_records_null_count() {
        // Build data with explicit null bitmap: 10 rows, nulls at positions 1,3,7 (3 nulls)
        let mut data = Vec::new();
        let mut null_bits = vec![1u8; 10];
        null_bits[1] = 0; null_bits[3] = 0; null_bits[7] = 0;
        for i in 0..10u8 {
            if null_bits[i as usize] == 1 {
                data.push(i);
            } else {
                data.push(0u8); // placeholder in data bytes
            }
        }

        // pack null bits into bytes (8 per byte)
        let mut packed = Vec::new();
        let mut byte = 0u8;
        for (i, &bit) in null_bits.iter().enumerate() {
            if bit == 1 { byte |= 1 << (i % 8); }
            if i % 8 == 7 { packed.push(byte); byte = 0; }
        }
        if null_bits.len() % 8 != 0 { packed.push(byte); }

        let columns = vec![ColumnData {
            name: "nbcol".to_string(),
            data_type: 1,
            data: data.clone(),
            null_bitmap: Some(packed.clone()),
        }];

        let result = FileIOValidator::validate_roundtrip_file_io(columns, 10).unwrap();
        // parse RG footer from result.file_bytes: last 4 bytes = len
        let fb = &result.file_bytes;
        let len_bytes = &fb[fb.len()-4..];
        let len = u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;
        let start = fb.len() - 4 - len;
        let rg = &fb[start..start+len];
        // skip row_count(8) + num_cols(4) + min_len(4) + min(0) + max_len(4) + max(0) => offset 24
        let null_count_offset = 8 + 4;
        // per-col: min_len(4) + min + max_len(4) + max + null_count(8)
        // here min_len and max_len are 0, so null_count at offset 8+4+4+0+4+0 = 20? compute directly
        let mut cursor = std::io::Cursor::new(rg);
        use std::io::Read;
        let mut buf8 = [0u8;8];
        let mut buf4 = [0u8;4];
        cursor.read_exact(&mut buf8).unwrap(); // row_count
        cursor.read_exact(&mut buf4).unwrap(); // num_cols
        cursor.read_exact(&mut buf4).unwrap(); // min_len
        let min_len = u32::from_le_bytes(buf4) as usize;
        if min_len>0 { let mut tmp = vec![0u8;min_len]; cursor.read_exact(&mut tmp).unwrap(); }
        cursor.read_exact(&mut buf4).unwrap(); // max_len
        let max_len = u32::from_le_bytes(buf4) as usize;
        if max_len>0 { let mut tmp = vec![0u8;max_len]; cursor.read_exact(&mut tmp).unwrap(); }
        cursor.read_exact(&mut buf8).unwrap();
        let null_count = u64::from_le_bytes(buf8);

        assert_eq!(null_count, 3u64);
    }
}
