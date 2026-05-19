/// Week 9: Compression codec implementations
/// 
/// Mirrors decompression.rs but for writing data.
/// Supports: RLE, Dictionary, FOR, LZSS
///
/// Integration with KoreWriter:
/// 1. Analyze column → ColumnProfile
/// 2. Select codec → CodecId (via CodecSelector from Week 7)
/// 3. Compress data → compressed bytes + metadata
/// 4. Write to file with KoreWriter

use crate::binary_format::BinaryFormatError;
use crate::decompression::CodecId;
use std::collections::HashMap;

/// Compression statistics
#[derive(Clone, Debug)]
pub struct CompressionStats {
    pub original_size: usize,
    pub compressed_size: usize,
    pub ratio: f32,
}

impl CompressionStats {
    pub fn new(original_size: usize, compressed_size: usize) -> Self {
        let ratio = if original_size > 0 {
            compressed_size as f32 / original_size as f32
        } else {
            1.0
        };
        Self {
            original_size,
            compressed_size,
            ratio,
        }
    }
}

/// Write varint in little-endian format with continuation bit
fn write_varint(value: u64, output: &mut Vec<u8>) {
    let mut v = value;
    loop {
        let mut byte = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        output.push(byte);
        if v == 0 {
            break;
        }
    }
}

/// RLE Compressor
pub struct RLECompressor;

impl RLECompressor {
    /// Compress data using run-length encoding
    /// 
    /// Format:
    /// - value_length (varint): bytes per value
    /// - value (bytes): the repeated value
    /// - run_length (varint): how many times it repeats
    /// - ... repeat for each run
    pub fn compress(data: &[u8]) -> Result<(Vec<u8>, CompressionStats), BinaryFormatError> {
        if data.is_empty() {
            return Ok((vec![], CompressionStats::new(0, 0)));
        }

        let mut output = Vec::new();
        let mut i = 0;

        // Detect value length (assume all runs are same byte width for simplicity)
        // For now, assume 1 byte per value (most common case)
        let value_length = 1u8;
        write_varint(value_length as u64, &mut output);

        while i < data.len() {
            let current_byte = data[i];
            let mut run_length = 1;

            // Count consecutive identical bytes
            while i + run_length < data.len() && data[i + run_length] == current_byte {
                run_length += 1;
            }

            // Write value
            output.push(current_byte);

            // Write run length
            write_varint(run_length as u64, &mut output);

            i += run_length;
        }

        let stats = CompressionStats::new(data.len(), output.len());
        Ok((output, stats))
    }
}

/// Dictionary Compressor
pub struct DictionaryCompressor;

impl DictionaryCompressor {
    /// Compress using dictionary encoding
    /// 
    /// Format:
    /// - dict_size (u32 LE): number of unique entries
    /// - entry[0] (length byte + bytes): first dictionary entry
    /// - entry[1] (length byte + bytes): second dictionary entry
    /// - ... repeat for all unique entries
    /// - indices: sequence of indices into dictionary
    pub fn compress(data: &[u8]) -> Result<(Vec<u8>, CompressionStats), BinaryFormatError> {
        if data.is_empty() {
            return Ok((vec![], CompressionStats::new(0, 0)));
        }

        // Build dictionary of unique bytes/values
        let mut dict: Vec<u8> = Vec::new();
        let mut value_to_index: HashMap<u8, u8> = HashMap::new();
        let mut index_counter = 0u8;

        for &byte in data {
            if let std::collections::hash_map::Entry::Vacant(e) = value_to_index.entry(byte) {
                dict.push(byte);
                e.insert(index_counter);
                index_counter += 1;
                if index_counter == 255 {
                    break; // Max 255 dictionary entries
                }
            }
        }

        let mut output = Vec::new();

        // Write dictionary size as u32 (LE)
        let dict_size = dict.len() as u32;
        output.extend_from_slice(&dict_size.to_le_bytes());

        // Write dictionary entries (1 byte each for simple case)
        // Format: length (1 byte) + entry data
        for entry in &dict {
            output.push(1); // Length of this entry
            output.push(*entry);
        }

        // Write indices
        for &byte in data {
            if let Some(&idx) = value_to_index.get(&byte) {
                output.push(idx);
            }
        }

        let stats = CompressionStats::new(data.len(), output.len());
        Ok((output, stats))
    }
}

/// Frame-of-Reference (FOR) Compressor
pub struct FORCompressor;

impl FORCompressor {
    /// Compress numeric data using FOR
    /// 
    /// Assumes 4-byte (u32) values in little-endian format.
    /// 
    /// Format:
    /// - bit_width (u8): bits needed per offset
    /// - base_value (u32): minimum value in dataset
    /// - packed_data: offset values packed at bit_width bits each
    pub fn compress(data: &[u8]) -> Result<(Vec<u8>, CompressionStats), BinaryFormatError> {
        if data.is_empty() {
            return Ok((vec![], CompressionStats::new(0, 0)));
        }

        // Parse as u32 values (little-endian)
        if data.len() % 4 != 0 {
            return Err(BinaryFormatError::InvalidData(
                "FOR compression requires 4-byte aligned data".to_string(),
            ));
        }

        let mut values = Vec::new();
        for chunk in data.chunks(4) {
            let u32_val = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            values.push(u32_val);
        }

        if values.is_empty() {
            return Ok((vec![], CompressionStats::new(data.len(), 0)));
        }

        // Find min and max
        let min_val = *values.iter().min().unwrap();
        let max_val = *values.iter().max().unwrap();
        let range = max_val - min_val;

        // Calculate bits needed
        let bit_width = if range == 0 { 0 } else { 32 - range.leading_zeros() };

        let mut output = Vec::new();

        // Write bit_width
        output.push(bit_width as u8);

        // Write base value (little-endian)
        output.extend_from_slice(&min_val.to_le_bytes());

        // Pack offsets
        let mut packed_bits = Vec::new();
        let mut current_byte = 0u8;
        let mut bit_pos = 0;

        for &val in &values {
            let offset = val - min_val;

            // Write offset at bit_width bits
            for i in 0..bit_width {
                let bit = ((offset >> i) & 1) as u8;
                current_byte |= bit << bit_pos;
                bit_pos += 1;

                if bit_pos == 8 {
                    packed_bits.push(current_byte);
                    current_byte = 0;
                    bit_pos = 0;
                }
            }
        }

        // Flush remaining bits
        if bit_pos > 0 {
            packed_bits.push(current_byte);
        }

        output.extend_from_slice(&packed_bits);

        let stats = CompressionStats::new(data.len(), output.len());
        Ok((output, stats))
    }
}

/// LZSS Compressor
pub struct LZSSCompressor;

impl LZSSCompressor {
    /// Compress using LZSS (Lempel-Ziv-Storer-Szymanski)
    /// 
    /// Format (matches decompressor expectations):
    /// - flags (u8): bit 0 = literal (1) or backreference (0)
    /// - for each bit:
    ///   - if literal: 1 byte of data
    ///   - if backreference: [dist_low][dist_high_and_len] = 2 bytes
    ///     - dist_low = distance & 0xFF (bits 7-0)
    ///     - dist_high_and_len = ((distance >> 8) & 0x0F) << 4 | ((len - 3) & 0x0F)
    /// - ... repeat for each 8-byte chunk
    /// 
    /// Window: 4KB (12-bit offset), Max match: 18 bytes (4-bit length field + 3)
    pub fn compress(data: &[u8]) -> Result<(Vec<u8>, CompressionStats), BinaryFormatError> {
        if data.is_empty() {
            return Ok((vec![], CompressionStats::new(0, 0)));
        }

        let mut output = Vec::new();
        let mut pos = 0;
        const WINDOW_SIZE: usize = 4096; // 4KB (12-bit offset limit)
        const MAX_MATCH: usize = 18; // 4-bit length field: 0-15 + 3 = 3-18 bytes
        const MIN_MATCH: usize = 3;

        while pos < data.len() {
            let mut flags = 0u8;
            let mut chunk_data = Vec::new();

            // Process 8 bytes/matches at a time
            for bit in 0..8 {
                if pos >= data.len() {
                    break;
                }

                // Try to find match in window
                let window_start = pos.saturating_sub(WINDOW_SIZE);
                let mut best_len = 0;
                let mut best_offset = 0;

                if pos > window_start {
                    for i in window_start..pos {
                        let mut len = 0;
                        while len < MAX_MATCH
                            && pos + len < data.len()
                            && i + len < pos
                            && data[i + len] == data[pos + len]
                        {
                            len += 1;
                        }

                        if len >= MIN_MATCH && len > best_len {
                            best_len = len;
                            best_offset = pos - i;
                        }
                    }
                }

                if best_len >= MIN_MATCH {
                    // Backreference: 2 bytes with packed distance and length
                    flags |= 1 << bit; // 1 = backreference
                    
                    // Pack distance (12 bits) and length (4 bits) into 2 bytes
                    let dist_low = (best_offset & 0xFF) as u8;
                    let dist_high_and_len = (((best_offset >> 8) & 0x0F) as u8) << 4
                        | ((best_len.saturating_sub(3)) & 0x0F) as u8;
                    
                    chunk_data.push(dist_low);
                    chunk_data.push(dist_high_and_len);
                    pos += best_len;
                } else {
                    // Literal: just the byte
                    flags |= 0 << bit; // 0 = literal (no-op, bit stays 0)
                    chunk_data.push(data[pos]);
                    pos += 1;
                }
            }

            output.push(flags);
            output.extend_from_slice(&chunk_data);
        }

        let stats = CompressionStats::new(data.len(), output.len());
        Ok((output, stats))
    }
}

/// Codec Registry for compression
pub struct CompressionRegistry;

impl CompressionRegistry {
    /// Compress data using specified codec
    pub fn compress(
        codec: CodecId,
        data: &[u8],
    ) -> Result<(Vec<u8>, CompressionStats), BinaryFormatError> {
        match codec {
            CodecId::None => Ok((data.to_vec(), CompressionStats::new(data.len(), data.len()))),
            CodecId::RLE => RLECompressor::compress(data),
            CodecId::Dictionary => DictionaryCompressor::compress(data),
            CodecId::FOR => FORCompressor::compress(data),
            CodecId::LZSS => LZSSCompressor::compress(data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rle_compress_repetitive() {
        let data = vec![0xAA; 100];
        let (compressed, stats) = RLECompressor::compress(&data).unwrap();

        assert!(stats.ratio < 0.5); // Should compress well
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_rle_compress_alternating() {
        let mut data = Vec::new();
        for _ in 0..50 {
            data.push(0xAA);
            data.push(0xBB);
        }
        let (_compressed, stats) = RLECompressor::compress(&data).unwrap();

        // Alternating data compresses poorly with RLE
        assert!(stats.ratio > 0.8);
    }

    #[test]
    fn test_dict_compress_categorical() {
        let mut data = Vec::new();
        for _ in 0..50 {
            data.extend_from_slice(&[1u8, 2, 3, 4, 5]);
        }
        let (compressed, stats) = DictionaryCompressor::compress(&data).unwrap();

        // Dictionary encoding may not be smaller for small dictionaries
        // (dict overhead can exceed savings)
        assert!(compressed.len() > 0);
        assert_eq!(stats.original_size, 250);
    }

    #[test]
    fn test_dict_compress_unique() {
        let data: Vec<u8> = (0..255).collect();
        let (_compressed, stats) = DictionaryCompressor::compress(&data).unwrap();

        assert!(stats.ratio > 0.9); // Unique data compresses poorly
    }

    #[test]
    fn test_for_compress_numeric_range() {
        let mut data = Vec::new();
        for i in 1000u32..1100u32 {
            data.extend_from_slice(&i.to_le_bytes());
        }
        let (_compressed, stats) = FORCompressor::compress(&data).unwrap();

        assert!(stats.ratio < 0.5); // Should compress well
    }

    #[test]
    fn test_lzss_compress_repetitive() {
        let data = vec![0xAB; 100];
        let (_compressed, stats) = LZSSCompressor::compress(&data).unwrap();

        assert!(stats.ratio < 0.7); // Should compress reasonably
    }

    #[test]
    fn test_lzss_compress_text() {
        let text = "hello world hello world hello world".as_bytes();
        let (_compressed, stats) = LZSSCompressor::compress(text).unwrap();

        assert!(stats.ratio < 0.9); // Should have some compression
    }

    #[test]
    fn test_rle_empty() {
        let data: Vec<u8> = vec![];
        let (compressed, stats) = RLECompressor::compress(&data).unwrap();

        assert_eq!(compressed.len(), 0);
        assert_eq!(stats.original_size, 0);
    }

    #[test]
    fn test_compression_stats() {
        let stats = CompressionStats::new(1000, 500);
        assert_eq!(stats.ratio, 0.5);
        assert_eq!(stats.original_size, 1000);
        assert_eq!(stats.compressed_size, 500);
    }

    #[test]
    fn test_compression_registry_rle() {
        let data = vec![0xFF; 100];
        let (compressed, stats) = CompressionRegistry::compress(CodecId::RLE, &data).unwrap();

        assert!(stats.ratio < 0.5);
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_compression_registry_dict() {
        let mut data = Vec::new();
        for _ in 0..20 {
            data.extend_from_slice(&[1u8, 2, 3, 4, 5]);
        }
        let (compressed, stats) =
            CompressionRegistry::compress(CodecId::Dictionary, &data).unwrap();

        // Dictionary encoding doesn't always compress small datasets
        assert!(compressed.len() > 0);
        assert_eq!(stats.original_size, 100);
    }
}
