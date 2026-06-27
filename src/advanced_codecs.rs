/// Advanced codec implementations for v1.1.6+
/// Target: 25-35% compression ratio (vs current 55%)
/// Strategy: Dictionary size optimization, content-aware compression levels, delta-aware ZSTD

use std::collections::HashMap;

/// Advanced ZSTD with 128KB dictionary and content-aware compression levels
pub struct AdvancedZSTDCodec {
    dictionary: Vec<u8>,
    compression_level: u32,
    entropy_threshold: f64,
}

impl AdvancedZSTDCodec {
    pub fn new() -> Self {
        Self {
            dictionary: Vec::with_capacity(131072), // 128KB
            compression_level: 19, // Default aggressive
            entropy_threshold: 0.7,
        }
    }

    /// Analyze data entropy to determine compression level (18-22)
    /// Lower entropy = higher compression level
    pub fn calculate_compression_level(data: &[u8]) -> u32 {
        let entropy = Self::calculate_entropy(data);
        
        match entropy {
            e if e < 2.0 => 22,  // Highly repetitive - max compression
            e if e < 3.0 => 21,  // Repetitive patterns
            e if e < 4.0 => 20,  // Mixed patterns
            e if e < 5.0 => 19,  // Moderate entropy
            e if e < 6.0 => 18,  // High entropy
            _ => 17,             // Very high entropy - standard compression
        }
    }

    /// Calculate Shannon entropy of data (0-8 bits per byte)
    fn calculate_entropy(data: &[u8]) -> f64 {
        let mut freq = [0usize; 256];
        for &byte in data {
            freq[byte as usize] += 1;
        }
        
        let len = data.len() as f64;
        let mut entropy = 0.0;
        
        for count in &freq {
            if *count > 0 {
                let p = *count as f64 / len;
                entropy -= p * p.log2();
            }
        }
        entropy
    }

    /// Build dictionary from sample patterns (128KB max)
    pub fn build_dictionary_from_patterns(samples: &[&[u8]]) -> Vec<u8> {
        let mut dictionary = Vec::with_capacity(131072);
        let mut pattern_freq: HashMap<Vec<u8>, usize> = HashMap::new();

        // Extract 4-32 byte patterns
        for sample in samples {
            for window_size in [4, 8, 16, 32] {
                for window in sample.windows(window_size) {
                    if window.len() == window_size {
                        *pattern_freq.entry(window.to_vec()).or_insert(0) += 1;
                    }
                }
            }
        }

        // Sort by frequency and add top patterns
        let mut patterns: Vec<_> = pattern_freq.into_iter().collect();
        patterns.sort_by(|a, b| b.1.cmp(&a.1));

        for (pattern, _) in patterns {
            if dictionary.len() + pattern.len() > 131072 {
                break;
            }
            dictionary.extend_from_slice(&pattern);
        }

        dictionary
    }

    /// Compress with optimized dictionary and level
    pub fn compress(&self, data: &[u8]) -> Vec<u8> {
        // In production, use zstd-rs with custom dictionary
        // For now: placeholder showing intent
        let level = Self::calculate_compression_level(data);
        
        // This would use zstd_rs::Codec with:
        // - dictionary: &self.dictionary
        // - level: level
        // - long_distance_matching: true
        // - window_log: 31 (max for ZSTD)
        
        data.to_vec() // Placeholder
    }

    pub fn decompress(&self, compressed: &[u8]) -> Vec<u8> {
        // Placeholder
        compressed.to_vec()
    }
}

impl Default for AdvancedZSTDCodec {
    fn default() -> Self {
        Self::new()
    }
}

/// Delta encoding for numeric columns (int, float, timestamp)
pub struct DeltaEncoder;

impl DeltaEncoder {
    /// Encode: store deltas instead of absolute values
    /// 1000000, 1000001, 1000002 -> 1000000, 1, 1 (99% smaller for sorted data!)
    pub fn encode_i64(data: &[i64]) -> Vec<i64> {
        if data.is_empty() {
            return Vec::new();
        }

        let mut encoded = Vec::with_capacity(data.len());
        encoded.push(data[0]); // First value as-is

        for window in data.windows(2) {
            encoded.push(window[1] - window[0]); // Delta
        }
        encoded
    }

    /// Encode floating point (with quantization to int first)
    pub fn encode_f64(data: &[f64], quantization_bits: u32) -> Vec<i64> {
        let scale = (1u64 << quantization_bits) as f64;
        let quantized: Vec<i64> = data
            .iter()
            .map(|&v| (v * scale) as i64)
            .collect();
        Self::encode_i64(&quantized)
    }

    /// Decode: reconstruct from deltas
    pub fn decode_i64(encoded: &[i64]) -> Vec<i64> {
        if encoded.is_empty() {
            return Vec::new();
        }

        let mut decoded = Vec::with_capacity(encoded.len());
        let mut current = encoded[0];
        decoded.push(current);

        for &delta in &encoded[1..] {
            current += delta;
            decoded.push(current);
        }
        decoded
    }

    pub fn decode_f64(encoded: &[i64], quantization_bits: u32) -> Vec<f64> {
        let decoded = Self::decode_i64(encoded);
        let scale = (1u64 << quantization_bits) as f64;
        decoded.iter().map(|&v| v as f64 / scale).collect()
    }
}

/// Bit-packing for integer ranges (e.g., 0-255 = 1 byte, packed to 1 bit each if max=1)
pub struct BitPacker;

impl BitPacker {
    /// Calculate bits needed to store max value
    pub fn bits_needed(max_value: u64) -> u32 {
        if max_value == 0 {
            return 1;
        }
        64 - max_value.leading_zeros()
    }

    /// Pack integers into minimal bits
    pub fn pack(values: &[u64], max_value: u64) -> Vec<u8> {
        let bits_per_value = Self::bits_needed(max_value);
        let total_bits = values.len() as u64 * bits_per_value as u64;
        let bytes_needed = ((total_bits + 7) / 8) as usize;
        
        let mut packed = vec![0u8; bytes_needed];
        let mut bit_offset = 0usize;

        for &value in values {
            for bit in 0..bits_per_value {
                let bit_set = (value >> bit) & 1 == 1;
                if bit_set {
                    let byte_idx = bit_offset / 8;
                    let bit_idx = bit_offset % 8;
                    packed[byte_idx] |= 1 << bit_idx;
                }
                bit_offset += 1;
            }
        }

        packed
    }

    /// Unpack from minimal bits
    pub fn unpack(packed: &[u8], count: usize, bits_per_value: u32) -> Vec<u64> {
        let mut unpacked = Vec::with_capacity(count);
        let mut bit_offset = 0usize;

        for _ in 0..count {
            let mut value = 0u64;
            for bit in 0..bits_per_value {
                let byte_idx = bit_offset / 8;
                let bit_idx = bit_offset % 8;
                if byte_idx < packed.len() && (packed[byte_idx] >> bit_idx) & 1 == 1 {
                    value |= 1 << bit;
                }
                bit_offset += 1;
            }
            unpacked.push(value);
        }

        unpacked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delta_encoding_sorted_i64() {
        let data = vec![1000000, 1000001, 1000002, 1000003, 1000004];
        let encoded = DeltaEncoder::encode_i64(&data);
        assert_eq!(encoded[0], 1000000);
        assert_eq!(encoded[1], 1);
        assert_eq!(encoded[2], 1);
        assert_eq!(encoded[3], 1);
        assert_eq!(encoded[4], 1);
    }

    #[test]
    fn test_delta_decoding() {
        let original = vec![100, 102, 103, 110, 105];
        let encoded = DeltaEncoder::encode_i64(&original);
        let decoded = DeltaEncoder::decode_i64(&encoded);
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_bit_packing() {
        let values = vec![0, 1, 1, 0, 1, 0, 1, 1]; // 8 bits stored in 1 byte
        let packed = BitPacker::pack(&values, 1);
        assert_eq!(packed.len(), 1); // 8 bits = 1 byte
        let unpacked = BitPacker::unpack(&packed, 8, 1);
        assert_eq!(unpacked, values);
    }

    #[test]
    fn test_entropy_calculation() {
        // All same value = entropy 0
        let uniform = vec![42u8; 100];
        let e1 = AdvancedZSTDCodec::calculate_entropy(&uniform);
        assert!(e1 < 0.1);

        // Mixed = higher entropy
        let mixed = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 1, 2, 3, 4, 5, 6, 7, 8];
        let e2 = AdvancedZSTDCodec::calculate_entropy(&mixed);
        assert!(e2 > 1.0);
    }

    #[test]
    fn test_compression_level_selection() {
        let low_entropy = vec![1u8; 100]; // All same = very low entropy
        let level_low = AdvancedZSTDCodec::calculate_compression_level(&low_entropy);
        assert_eq!(level_low, 22); // Max compression for repetitive data

        let high_entropy = (0..256).cycle().take(256).map(|x| x as u8).collect::<Vec<_>>();
        let level_high = AdvancedZSTDCodec::calculate_compression_level(&high_entropy);
        assert!(level_high <= 19); // Lower compression for random data
    }
}
