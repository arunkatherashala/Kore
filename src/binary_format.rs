//! Kore Binary Format - Core encoding and compression
//!
//! Implements delta encoding, dictionary compression, and incremental encoding
//! for achieving 5-10x compression on real-world datasets.

use std::collections::HashMap;

/// Error types for binary format operations
#[derive(Debug)]
pub enum BinaryFormatError {
    /// Compression failed
    CompressionError(String),
    /// Decompression failed
    DecompressionError(String),
    /// Encoding error
    EncodingError(String),
    /// Invalid configuration
    InvalidConfig(String),
}

impl std::fmt::Display for BinaryFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            BinaryFormatError::CompressionError(e) => write!(f, "Compression error: {}", e),
            BinaryFormatError::DecompressionError(e) => write!(f, "Decompression error: {}", e),
            BinaryFormatError::EncodingError(e) => write!(f, "Encoding error: {}", e),
            BinaryFormatError::InvalidConfig(e) => write!(f, "Invalid config: {}", e),
        }
    }
}

impl std::error::Error for BinaryFormatError {}

/// Delta encoding for integer sequences
/// 
/// Stores differences between consecutive values instead of absolute values,
/// dramatically reducing storage space for time-series and sorted data.
/// 
/// # Examples
/// 
/// ```ignore
/// let values = vec![100, 105, 103, 108, 110];  // Original
/// let encoded = DeltaEncoder::encode(&values)?;
/// // Stores: [100, 5, -2, 5, 2] (much more compressible)
/// ```
#[derive(Debug)]
pub struct DeltaEncoder;

impl DeltaEncoder {
    /// Encode integer sequence using delta encoding
    pub fn encode(values: &[i64]) -> Result<Vec<u8>, BinaryFormatError> {
        if values.is_empty() {
            return Ok(Vec::new());
        }
        
        let mut encoded = Vec::with_capacity(values.len() * 8);
        let mut prev = values[0];
        
        // Store first value as baseline
        encoded.extend_from_slice(&prev.to_le_bytes());
        
        // Store deltas for remaining values
        for &value in &values[1..] {
            let delta = value.wrapping_sub(prev);
            encoded.extend_from_slice(&delta.to_le_bytes());
            prev = value;
        }
        
        Ok(encoded)
    }
    
    /// Decode delta-encoded integer sequence
    pub fn decode(bytes: &[u8]) -> Result<Vec<i64>, BinaryFormatError> {
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        
        if bytes.len() % 8 != 0 {
            return Err(BinaryFormatError::DecompressionError(
                "Invalid delta encoded data: length not multiple of 8".to_string(),
            ));
        }
        
        let mut values = Vec::new();
        let mut prev = i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        values.push(prev);
        
        // Decode deltas
        for chunk in bytes[8..].chunks(8) {
            let delta = i64::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3],
                chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
            prev = prev.wrapping_add(delta);
            values.push(prev);
        }
        
        Ok(values)
    }
}

/// Dictionary compression for categorical data
/// 
/// Replaces frequently occurring values with indices into a dictionary.
/// Excellent for high-cardinality categorical columns.
/// 
/// # Examples
/// 
/// ```ignore
/// let values = vec!["red", "blue", "red", "green", "blue", "red"];
/// let (compressed, dict) = DictionaryCompressor::compress(&values)?;
/// // Dictionary: {"red": 0, "blue": 1, "green": 2}
/// // Compressed: [0, 1, 0, 2, 1, 0]
/// ```
#[derive(Debug)]
pub struct DictionaryCompressor;

impl DictionaryCompressor {
    /// Compress string values using dictionary encoding
    /// 
    /// Returns (compressed indices, dictionary)
    pub fn compress_strings(
        values: &[&str],
    ) -> Result<(Vec<u8>, HashMap<String, u32>), BinaryFormatError> {
        let mut dictionary: HashMap<String, u32> = HashMap::new();
        let mut indices = Vec::with_capacity(values.len());
        let mut next_id = 0u32;
        
        for &value in values {
            let id = dictionary.entry(value.to_string()).or_insert_with(|| {
                let id = next_id;
                next_id += 1;
                id
            });
            indices.push(*id);
        }
        
        // Encode indices
        let mut encoded = Vec::new();
        for idx in indices {
            encoded.extend_from_slice(&idx.to_le_bytes());
        }
        
        Ok((encoded, dictionary))
    }
    
    /// Decompress dictionary-encoded values
    pub fn decompress_strings(
        bytes: &[u8],
        dictionary: &HashMap<String, u32>,
    ) -> Result<Vec<String>, BinaryFormatError> {
        if bytes.len() % 4 != 0 {
            return Err(BinaryFormatError::DecompressionError(
                "Invalid dictionary encoded data: length not multiple of 4".to_string(),
            ));
        }
        
        // Create reverse dictionary
        let mut reverse_dict: HashMap<u32, String> = HashMap::new();
        for (key, &idx) in dictionary.iter() {
            reverse_dict.insert(idx, key.clone());
        }
        
        let mut result = Vec::new();
        for chunk in bytes.chunks(4) {
            let idx = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let value = reverse_dict
                .get(&idx)
                .ok_or_else(|| {
                    BinaryFormatError::DecompressionError(format!("Unknown dictionary index: {}", idx))
                })?
                .clone();
            result.push(value);
        }
        
        Ok(result)
    }
}

/// Incremental encoding for time-series data
/// 
/// Optimized for append-only workloads where new data is added incrementally.
/// Tracks state of previous rows and only encodes changes.
#[derive(Debug)]
pub struct IncrementalEncoder {
    column_types: Vec<ColumnType>,
    last_values: Vec<Vec<u8>>,
}

/// Column data type
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnType {
    /// 64-bit integer
    Int64,
    /// 64-bit floating point
    Float64,
    /// Variable length string
    String,
    /// Fixed length binary
    Binary(usize),
}

impl IncrementalEncoder {
    /// Create new incremental encoder with schema
    pub fn new(schema: Vec<ColumnType>) -> Result<Self, BinaryFormatError> {
        Ok(IncrementalEncoder {
            column_types: schema.clone(),
            last_values: vec![Vec::new(); schema.len()],
        })
    }
    
    /// Encode a single row incrementally
    pub fn encode_row(&mut self, row: &[&[u8]]) -> Result<Vec<u8>, BinaryFormatError> {
        if row.len() != self.column_types.len() {
            return Err(BinaryFormatError::EncodingError(
                format!("Row size {} doesn't match schema size {}", row.len(), self.column_types.len()),
            ));
        }
        
        let mut encoded = Vec::new();
        
        for (i, column_data) in row.iter().enumerate() {
            let changed = &self.last_values[i] != column_data;
            encoded.push(if changed { 1u8 } else { 0u8 });
            
            if changed {
                // Store length of new value
                let len = column_data.len() as u32;
                encoded.extend_from_slice(&len.to_le_bytes());
                // Store value
                encoded.extend_from_slice(column_data);
                self.last_values[i] = column_data.to_vec();
            }
        }
        
        Ok(encoded)
    }
}

/// Compression level configuration (1-9)
/// 1 = fastest, 9 = best compression
#[derive(Debug, Clone, Copy)]
pub struct CompressionLevel(u8);

impl CompressionLevel {
    /// Create compression level (1-9)
    pub fn new(level: u8) -> Result<Self, BinaryFormatError> {
        if level < 1 || level > 9 {
            return Err(BinaryFormatError::InvalidConfig(
                format!("Compression level must be 1-9, got {}", level),
            ));
        }
        Ok(CompressionLevel(level))
    }
    
    /// Get numeric level
    pub fn level(&self) -> u8 {
        self.0
    }
}

/// Column statistics for query optimization
#[derive(Debug, Clone)]
pub struct ColumnStats {
    /// Column name
    pub name: String,
    /// Number of non-null values
    pub count: u64,
    /// Number of null values
    pub null_count: u64,
    /// Distinct value count (for selectivity estimation)
    pub distinct_count: u64,
    /// Minimum value (as bytes)
    pub min_value: Option<Vec<u8>>,
    /// Maximum value (as bytes)
    pub max_value: Option<Vec<u8>>,
}

/// Binary format metadata
#[derive(Debug)]
pub struct FormatMetadata {
    /// Format version (e.g., "1.1.0")
    pub version: String,
    /// Compression algorithm used
    pub compression: String,
    /// Compression level
    pub level: u8,
    /// Row count
    pub row_count: u64,
    /// Column count
    pub column_count: u32,
    /// Statistics per column
    pub column_stats: Vec<ColumnStats>,
    /// Checksum for integrity verification
    pub checksum: u32,
}

/// Enhanced Delta Encoder with bit-packing
/// 
/// Stores deltas in minimal bits needed, reducing storage 2-5x vs standard delta.
/// Automatically detects optimal bit width (1, 2, 4, 8, 16, 32 bits).
#[derive(Debug)]
pub struct BitPackedDelta;

impl BitPackedDelta {
    /// Determine minimum bits needed to represent a value
    fn min_bits_for_value(min_val: i64, max_val: i64) -> u8 {
        let range = (max_val as u128).wrapping_sub(min_val as u128);
        if range == 0 {
            1
        } else {
            let bits = 64 - (range as u64).leading_zeros() as u8;
            // Round up to nearest standard size: 1, 2, 4, 8, 16, 32, 64
            match bits {
                0 => 1,
                1..=1 => 1,
                2..=2 => 2,
                3..=4 => 4,
                5..=8 => 8,
                9..=16 => 16,
                _ => 32,
            }
        }
    }
    
    /// Encode with automatic bit-packing
    pub fn encode(values: &[i64]) -> Result<Vec<u8>, BinaryFormatError> {
        if values.is_empty() {
            return Ok(Vec::new());
        }
        
        // Calculate deltas
        let mut deltas = Vec::with_capacity(values.len());
        let mut prev = values[0];
        // deltas.push(prev);
        
        for &value in &values[1..] {
            let delta = value.wrapping_sub(prev);
            deltas.push(delta);
            prev = value;
        }
        
        // Find min/max deltas
        let min_delta = *deltas.iter().min().unwrap_or(&0);
        let max_delta = *deltas.iter().max().unwrap_or(&0);
        
        // Determine bit width
        let bits_needed = Self::min_bits_for_value(min_delta, max_delta);
        
        // Normalize to positive range (frame-of-reference technique)
        let frame_value = min_delta;
        let normalized: Vec<u64> = deltas
            .iter()
            .map(|d| d.wrapping_sub(frame_value) as u64)
            .collect();
        
        // Pack into bytes
        let mut encoded = Vec::new();
        
        // Header: baseline value (8 bytes) + bit width (1 byte) + frame value (8 bytes)
        encoded.extend_from_slice(&values[0].to_le_bytes());
        encoded.push(bits_needed);
        encoded.extend_from_slice(&frame_value.to_le_bytes());
        
        // Store number of values (4 bytes)
        encoded.extend_from_slice(&(deltas.len() as u32).to_le_bytes());
        
        // Pack normalized deltas
        match bits_needed {
            1 => Self::pack_bits(&normalized, 1, &mut encoded),
            2 => Self::pack_bits(&normalized, 2, &mut encoded),
            4 => Self::pack_bits(&normalized, 4, &mut encoded),
            8 => Self::pack_bytes(&normalized, 1, &mut encoded),
            16 => Self::pack_bytes(&normalized, 2, &mut encoded),
            32 => Self::pack_bytes(&normalized, 4, &mut encoded),
            _ => Self::pack_bytes(&normalized, 8, &mut encoded),
        }
        
        Ok(encoded)
    }
    
    /// Pack values into sub-byte boundaries
    fn pack_bits(values: &[u64], bits: usize, output: &mut Vec<u8>) {
        let mut byte_buffer = 0u8;
        let mut bit_pos = 0;
        let mask = (1u64 << bits) - 1;
        
        for &value in values {
            let val = (value & mask) as u8;
            
            if bit_pos + bits <= 8 {
                byte_buffer |= (val << bit_pos) as u8;
                bit_pos += bits;
                
                if bit_pos == 8 {
                    output.push(byte_buffer);
                    byte_buffer = 0;
                    bit_pos = 0;
                }
            } else {
                // Value spans byte boundary
                let bits_in_current = 8 - bit_pos;
                byte_buffer |= (val << bit_pos) as u8;
                output.push(byte_buffer);
                
                byte_buffer = (val >> bits_in_current) as u8;
                bit_pos = bits - bits_in_current;
            }
        }
        
        if bit_pos > 0 {
            output.push(byte_buffer);
        }
    }
    
    /// Pack values into byte boundaries
    fn pack_bytes(values: &[u64], bytes: usize, output: &mut Vec<u8>) {
        for &value in values {
            match bytes {
                1 => output.push(value as u8),
                2 => output.extend_from_slice(&(value as u16).to_le_bytes()),
                4 => output.extend_from_slice(&(value as u32).to_le_bytes()),
                _ => output.extend_from_slice(&value.to_le_bytes()),
            }
        }
    }
    
    /// Decode bit-packed deltas
    pub fn decode(bytes: &[u8]) -> Result<Vec<i64>, BinaryFormatError> {
        if bytes.len() < 21 {
            return Err(BinaryFormatError::DecompressionError(
                "BitPacked data too short".to_string(),
            ));
        }
        
        // Read header
        let baseline = i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        let bits_needed = bytes[8];
        let frame_value = i64::from_le_bytes([
            bytes[9], bytes[10], bytes[11], bytes[12],
            bytes[13], bytes[14], bytes[15], bytes[16],
        ]);
        let count = u32::from_le_bytes([bytes[17], bytes[18], bytes[19], bytes[20]]) as usize;
        
        let mut result = vec![baseline];
        let mut prev = baseline;
        
        if count == 0 {
            return Ok(result);
        }
        
        // Unpack based on bit width
        let packed_data = &bytes[21..];
        match bits_needed {
            1 => {
                let unpacked = Self::unpack_bits(packed_data, 1, count)?;
                for delta_norm in unpacked {
                    let delta = (delta_norm as i64).wrapping_add(frame_value);
                    prev = prev.wrapping_add(delta);
                    result.push(prev);
                }
            }
            2 => {
                let unpacked = Self::unpack_bits(packed_data, 2, count)?;
                for delta_norm in unpacked {
                    let delta = (delta_norm as i64).wrapping_add(frame_value);
                    prev = prev.wrapping_add(delta);
                    result.push(prev);
                }
            }
            4 => {
                let unpacked = Self::unpack_bits(packed_data, 4, count)?;
                for delta_norm in unpacked {
                    let delta = (delta_norm as i64).wrapping_add(frame_value);
                    prev = prev.wrapping_add(delta);
                    result.push(prev);
                }
            }
            8 => {
                for i in 0..count {
                    let delta_norm = packed_data[i] as i64;
                    let delta = delta_norm.wrapping_add(frame_value);
                    prev = prev.wrapping_add(delta);
                    result.push(prev);
                }
            }
            16 => {
                for i in 0..count {
                    let delta_norm = u16::from_le_bytes([packed_data[i * 2], packed_data[i * 2 + 1]]) as i64;
                    let delta = delta_norm.wrapping_add(frame_value);
                    prev = prev.wrapping_add(delta);
                    result.push(prev);
                }
            }
            _ => {
                for i in 0..count {
                    let delta_norm = u32::from_le_bytes([
                        packed_data[i * 4],
                        packed_data[i * 4 + 1],
                        packed_data[i * 4 + 2],
                        packed_data[i * 4 + 3],
                    ]) as i64;
                    let delta = delta_norm.wrapping_add(frame_value);
                    prev = prev.wrapping_add(delta);
                    result.push(prev);
                }
            }
        }
        
        Ok(result)
    }
    
    /// Unpack sub-byte bit-packed values
    fn unpack_bits(data: &[u8], bits: usize, count: usize) -> Result<Vec<u64>, BinaryFormatError> {
        let mut result = Vec::with_capacity(count);
        let mut byte_idx = 0;
        let mut bit_pos = 0;
        let mask = (1u64 << bits) - 1;
        
        for _ in 0..count {
            if byte_idx >= data.len() {
                return Err(BinaryFormatError::DecompressionError(
                    "Not enough data to unpack".to_string(),
                ));
            }
            
            let mut value = 0u64;
            let mut bits_read = 0;
            
            while bits_read < bits && byte_idx < data.len() {
                let byte_val = data[byte_idx] as u64;
                let bits_available = 8 - bit_pos;
                let bits_to_read = std::cmp::min(bits - bits_read, bits_available);
                
                let bits_mask = ((1u64 << bits_to_read) - 1) << bit_pos;
                let bits_val = (byte_val & bits_mask) >> bit_pos;
                value |= bits_val << bits_read;
                
                bits_read += bits_to_read;
                bit_pos += bits_to_read;
                
                if bit_pos >= 8 {
                    bit_pos = 0;
                    byte_idx += 1;
                }
            }
            
            result.push(value & mask);
        }
        
        Ok(result)
    }
}

/// Zigzag encoding for efficient signed integer compression
/// 
/// Maps signed integers to unsigned with smaller magnitude:
/// 0 → 0, -1 → 1, 1 → 2, -2 → 3, 2 → 4, ...
#[derive(Debug)]
pub struct ZigzagEncoding;

impl ZigzagEncoding {
    /// Encode signed integer to unsigned using zigzag
    pub fn encode(n: i64) -> u64 {
        ((n << 1) ^ (n >> 63)) as u64
    }
    
    /// Decode unsigned back to signed integer
    pub fn decode(n: u64) -> i64 {
        ((n >> 1) as i64) ^ -((n & 1) as i64)
    }
    
    /// Encode sequence of signed integers
    pub fn encode_sequence(values: &[i64]) -> Vec<u64> {
        values.iter().map(|&n| Self::encode(n)).collect()
    }
    
    /// Decode sequence of unsigned integers
    pub fn decode_sequence(values: &[u64]) -> Vec<i64> {
        values.iter().map(|&n| Self::decode(n)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_delta_encoding_integers() {
        let values = vec![100, 105, 103, 108, 110];
        let encoded = DeltaEncoder::encode(&values).unwrap();
        let decoded = DeltaEncoder::decode(&encoded).unwrap();
        assert_eq!(values, decoded);
    }
    
    #[test]
    fn test_dictionary_compression() {
        let values = vec!["red", "blue", "red", "green", "blue", "red"];
        let (encoded, dict) = DictionaryCompressor::compress_strings(&values).unwrap();
        let decoded = DictionaryCompressor::decompress_strings(&encoded, &dict).unwrap();
        assert_eq!(
            decoded,
            vec!["red", "blue", "red", "green", "blue", "red"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
    }
    
    #[test]
    fn test_compression_level_validation() {
        assert!(CompressionLevel::new(1).is_ok());
        assert!(CompressionLevel::new(9).is_ok());
        assert!(CompressionLevel::new(0).is_err());
        assert!(CompressionLevel::new(10).is_err());
    }
    
    #[test]
    fn test_incremental_encoder() {
        let schema = vec![ColumnType::Int64, ColumnType::String];
        let mut encoder = IncrementalEncoder::new(schema).unwrap();
        
        let row1 = vec![b"100".as_ref(), b"hello".as_ref()];
        let encoded1 = encoder.encode_row(&row1).unwrap();
        assert!(!encoded1.is_empty());
        
        // Same row should have less data
        let encoded2 = encoder.encode_row(&row1).unwrap();
        assert!(encoded2.len() < encoded1.len());
    }
    
    // BitPackedDelta Tests
    #[test]
    fn test_bitpacked_delta_simple() {
        let values = vec![100, 105, 103, 108, 110];
        let encoded = BitPackedDelta::encode(&values).unwrap();
        let decoded = BitPackedDelta::decode(&encoded).unwrap();
        assert_eq!(values, decoded);
    }
    
    #[test]
    fn test_bitpacked_delta_small_deltas() {
        // Small deltas should fit in 1-2 bits
        let values = vec![1000, 1001, 1000, 1001, 1000, 1001];
        let encoded = BitPackedDelta::encode(&values).unwrap();
        let decoded = BitPackedDelta::decode(&encoded).unwrap();
        assert_eq!(values, decoded);
        // Should be more compressed than standard delta
        assert!(encoded.len() < values.len() * 8);
    }
    
    #[test]
    fn test_bitpacked_delta_large_range() {
        let values = vec![0, 1000, 2000, 3000, 4000, 5000];
        let encoded = BitPackedDelta::encode(&values).unwrap();
        let decoded = BitPackedDelta::decode(&encoded).unwrap();
        assert_eq!(values, decoded);
    }
    
    #[test]
    fn test_bitpacked_delta_negative() {
        let values = vec![-100, -95, -90, -85, -80];
        let encoded = BitPackedDelta::encode(&values).unwrap();
        let decoded = BitPackedDelta::decode(&encoded).unwrap();
        assert_eq!(values, decoded);
    }
    
    #[test]
    fn test_bitpacked_delta_monotonic() {
        // Monotonic sequence (perfect for delta)
        let values: Vec<i64> = (0..100).collect();
        let encoded = BitPackedDelta::encode(&values).unwrap();
        let decoded = BitPackedDelta::decode(&encoded).unwrap();
        assert_eq!(values, decoded);
        // Should compress to ~1/8 of original
        assert!(encoded.len() < values.len() * 2);
    }
    
    #[test]
    fn test_bitpacked_delta_empty() {
        let values: Vec<i64> = vec![];
        let encoded = BitPackedDelta::encode(&values).unwrap();
        assert_eq!(encoded.len(), 0);
    }
    
    #[test]
    fn test_bitpacked_delta_single_value() {
        let values = vec![42];
        let encoded = BitPackedDelta::encode(&values).unwrap();
        let decoded = BitPackedDelta::decode(&encoded).unwrap();
        assert_eq!(values, decoded);
    }
    
    #[test]
    fn test_bitpacked_delta_compression_ratio() {
        // Time-series data (ideal for delta + bit-packing)
        let mut values = vec![];
        let mut val = 1000000i64;
        for _ in 0..1000 {
            values.push(val);
            val += 100; // Small increments
        }
        
        let std_delta = DeltaEncoder::encode(&values).unwrap();
        let bitpacked = BitPackedDelta::encode(&values).unwrap();
        
        // Bitpacked should be significantly smaller
        let ratio = bitpacked.len() as f64 / std_delta.len() as f64;
        assert!(ratio < 0.3, "Bitpacked should be <30% of standard delta, got {}", ratio);
    }
    
    // Zigzag Encoding Tests
    #[test]
    fn test_zigzag_positive() {
        assert_eq!(ZigzagEncoding::encode(0), 0);
        assert_eq!(ZigzagEncoding::encode(1), 2);
        assert_eq!(ZigzagEncoding::encode(2), 4);
        assert_eq!(ZigzagEncoding::encode(3), 6);
    }
    
    #[test]
    fn test_zigzag_negative() {
        assert_eq!(ZigzagEncoding::encode(-1), 1);
        assert_eq!(ZigzagEncoding::encode(-2), 3);
        assert_eq!(ZigzagEncoding::encode(-3), 5);
    }
    
    #[test]
    fn test_zigzag_roundtrip() {
        let values = vec![-100, -50, -1, 0, 1, 50, 100];
        let encoded = ZigzagEncoding::encode_sequence(&values);
        let decoded = ZigzagEncoding::decode_sequence(&encoded);
        assert_eq!(values, decoded);
    }
    
    #[test]
    fn test_zigzag_small_magnitude() {
        // Zigzag makes small magnitudes have small encoded values
        let small_neg = ZigzagEncoding::encode(-1);
        let small_pos = ZigzagEncoding::encode(1);
        let large_neg = ZigzagEncoding::encode(-1000);
        let large_pos = ZigzagEncoding::encode(1000);
        
        assert!(small_neg < large_neg as u64);
        assert!(small_pos < large_pos as u64);
    }
    
    #[test]
    fn test_enhanced_delta_compression_ratio() {
        // Compare all enhancement techniques
        let values = vec![100, 105, 103, 108, 110, 115, 113, 118, 120, 125];
        
        let std_delta = DeltaEncoder::encode(&values).unwrap();
        let bitpacked = BitPackedDelta::encode(&values).unwrap();
        
        // Both should decompress to same result
        let std_decoded = DeltaEncoder::decode(&std_delta).unwrap();
        let bp_decoded = BitPackedDelta::decode(&bitpacked).unwrap();
        assert_eq!(std_decoded, values);
        assert_eq!(bp_decoded, values);
        
        // Bitpacked should be more efficient
        assert!(bitpacked.len() < std_delta.len());
    }
}
