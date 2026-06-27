/// Gorilla Time-Series Compression Algorithm
/// XOR float compression + delta-of-delta encoding
/// 
/// Reference: "Gorilla: A Fast, Scalable, In-Memory Time Series Database"
/// (Pelkonen et al., VLDB 2015)
///
/// Compression techniques:
/// 1. XOR Compression: Store differences between consecutive float values
/// 2. Delta-of-Delta: Encode delta differences instead of absolute deltas
/// 3. Bit-packing: Only store significant bits
/// 4. Leading/Trailing Zero Removal: Eliminate redundant zeros

use std::io::Result as IoResult;

/// Gorilla time-series encoder
pub struct GorillaEncoder {
    values: Vec<f64>,
    timestamps: Vec<u64>,
    compressed: Vec<u8>,
}

impl GorillaEncoder {
    /// Create new Gorilla encoder
    pub fn new() -> Self {
        Self {
            values: Vec::new(),
            timestamps: Vec::new(),
            compressed: Vec::new(),
        }
    }

    /// Add time-series data point (timestamp, value)
    pub fn add_point(&mut self, timestamp: u64, value: f64) {
        self.timestamps.push(timestamp);
        self.values.push(value);
    }

    /// Add multiple data points
    pub fn add_points(&mut self, points: Vec<(u64, f64)>) {
        for (ts, val) in points {
            self.add_point(ts, val);
        }
    }

    /// Encode timestamps using delta-of-delta encoding
    fn encode_timestamps(&self) -> Vec<u8> {
        let mut encoded = Vec::new();
        
        if self.timestamps.is_empty() {
            return encoded;
        }

        // Store first timestamp in full (8 bytes)
        encoded.extend_from_slice(&self.timestamps[0].to_le_bytes());

        if self.timestamps.len() < 2 {
            return encoded;
        }

        // Encode deltas
        let mut prev_delta = self.timestamps[1] - self.timestamps[0];
        encoded.extend_from_slice(&prev_delta.to_le_bytes());

        // Encode delta-of-deltas
        for i in 2..self.timestamps.len() {
            let curr_delta = self.timestamps[i] - self.timestamps[i - 1];
            let delta_of_delta = (curr_delta as i64) - (prev_delta as i64);
            
            // Encode delta-of-delta with variable-length encoding
            encoded.extend(varint_encode(delta_of_delta as u64));
            prev_delta = curr_delta;
        }

        encoded
    }

    /// Encode values using XOR compression
    fn encode_values(&self) -> Vec<u8> {
        let mut encoded = Vec::new();

        if self.values.is_empty() {
            return encoded;
        }

        // Store first value in full (8 bytes)
        encoded.extend_from_slice(&self.values[0].to_le_bytes());

        if self.values.len() < 2 {
            return encoded;
        }

        let mut prev_value = self.values[0];

        // XOR compress subsequent values
        for i in 1..self.values.len() {
            let curr_bits = self.values[i].to_bits() as u64;
            let prev_bits = prev_value.to_bits() as u64;
            let xor = curr_bits ^ prev_bits;

            if xor == 0 {
                // Identical to previous value
                encoded.push(0x00);
            } else {
                // Find leading and trailing zeros
                let leading_zeros = xor.leading_zeros();
                let trailing_zeros = xor.trailing_zeros();

                if leading_zeros == 0 && trailing_zeros == 0 {
                    // No zeros to compress, store full value
                    encoded.push(0xFF);
                    encoded.extend_from_slice(&xor.to_le_bytes());
                } else {
                    // Store leading/trailing zero count
                    encoded.push(0x80 | ((leading_zeros >> 3) as u8));
                    let significant_bits = 64 - leading_zeros - trailing_zeros;
                    let bytes_needed = ((significant_bits + 7) / 8) as usize;
                    
                    for j in 0..bytes_needed {
                        let byte = ((xor >> (8 * j)) & 0xFF) as u8;
                        encoded.push(byte);
                    }
                }
            }

            prev_value = self.values[i];
        }

        encoded
    }

    /// Compress all data
    pub fn compress(&mut self) -> IoResult<Vec<u8>> {
        if self.values.is_empty() {
            return Ok(vec![]);
        }

        let mut result = Vec::new();

        // Header: number of points (u32)
        result.extend_from_slice(&(self.values.len() as u32).to_le_bytes());

        // Encode and store timestamps
        let ts_encoded = self.encode_timestamps();
        result.extend_from_slice(&(ts_encoded.len() as u32).to_le_bytes());
        result.extend(ts_encoded);

        // Encode and store values
        let val_encoded = self.encode_values();
        result.extend_from_slice(&(val_encoded.len() as u32).to_le_bytes());
        result.extend(val_encoded);

        self.compressed = result.clone();
        Ok(result)
    }

    /// Get compression ratio
    pub fn compression_ratio(&self) -> f64 {
        if self.values.is_empty() {
            return 0.0;
        }

        let original_size = (self.values.len() * 16) as f64; // 8 bytes timestamp + 8 bytes value
        let compressed_size = self.compressed.len() as f64;

        1.0 - (compressed_size / original_size)
    }

    /// Get compressed data
    pub fn get_compressed(&self) -> &[u8] {
        &self.compressed
    }

    /// Get statistics
    pub fn get_stats(&self) -> GorillaStats {
        let original_size = self.values.len() * 16;
        let compressed_size = self.compressed.len();

        GorillaStats {
            point_count: self.values.len(),
            original_size,
            compressed_size,
            compression_ratio: self.compression_ratio(),
            timestamp_count: self.timestamps.len(),
            value_count: self.values.len(),
        }
    }
}

/// Gorilla time-series decoder
pub struct GorillaDecoder {
    data: Vec<u8>,
    position: usize,
}

impl GorillaDecoder {
    /// Create new decoder from compressed data
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, position: 0 }
    }

    /// Decode timestamps
    fn decode_timestamps(&mut self, count: usize) -> IoResult<Vec<u64>> {
        let mut timestamps = Vec::new();

        if count == 0 {
            return Ok(timestamps);
        }

        // Read first timestamp
        let first_ts = u64::from_le_bytes(self.read_bytes(8)?);
        timestamps.push(first_ts);

        if count == 1 {
            return Ok(timestamps);
        }

        // Read second timestamp (full delta)
        let second_delta = u64::from_le_bytes(self.read_bytes(8)?);
        timestamps.push(first_ts + second_delta);

        let mut prev_delta = second_delta;

        // Decode delta-of-deltas
        for _ in 2..count {
            let (dod, _) = varint_decode(&self.data[self.position..])?;
            self.position += varint_size(dod);

            let curr_delta = (prev_delta as i64 + dod as i64) as u64;
            timestamps.push(timestamps[timestamps.len() - 1] + curr_delta);
            prev_delta = curr_delta;
        }

        Ok(timestamps)
    }

    /// Decode values
    fn decode_values(&mut self, count: usize) -> IoResult<Vec<f64>> {
        let mut values = Vec::new();

        if count == 0 {
            return Ok(values);
        }

        // Read first value
        let first_bits = u64::from_le_bytes(self.read_bytes(8)?);
        let first_val = f64::from_bits(first_bits);
        values.push(first_val);

        if count == 1 {
            return Ok(values);
        }

        let mut prev_bits = first_bits;

        // Decode XOR values
        for _ in 1..count {
            let marker = self.data[self.position];
            self.position += 1;

            let xor = if marker == 0x00 {
                // Identical to previous
                0u64
            } else if marker == 0xFF {
                // Full XOR value
                u64::from_le_bytes(self.read_bytes(8)?)
            } else {
                // Partial XOR value
                let leading_zeros = ((marker & 0x7F) as u32) * 8;
                let mut xor = 0u64;

                // Determine bytes to read
                let bytes_to_read = 8 - ((leading_zeros + 7) / 8) as usize;
                for i in 0..bytes_to_read {
                    let byte = self.data[self.position] as u64;
                    self.position += 1;
                    xor |= byte << (8 * i);
                }

                xor
            };

            let curr_bits = prev_bits ^ xor;
            values.push(f64::from_bits(curr_bits));
            prev_bits = curr_bits;
        }

        Ok(values)
    }

    /// Decompress data
    pub fn decompress(&mut self) -> IoResult<Vec<(u64, f64)>> {
        if self.data.len() < 12 {
            return Ok(vec![]);
        }

        // Read header (each is 4 bytes)
        let count_bytes = [
            self.data[self.position],
            self.data[self.position + 1],
            self.data[self.position + 2],
            self.data[self.position + 3],
        ];
        let count = u32::from_le_bytes(count_bytes) as usize;
        self.position += 4;

        let ts_size_bytes = [
            self.data[self.position],
            self.data[self.position + 1],
            self.data[self.position + 2],
            self.data[self.position + 3],
        ];
        let _ts_size = u32::from_le_bytes(ts_size_bytes) as usize;
        self.position += 4;

        let val_size_bytes = [
            self.data[self.position],
            self.data[self.position + 1],
            self.data[self.position + 2],
            self.data[self.position + 3],
        ];
        let _val_size = u32::from_le_bytes(val_size_bytes) as usize;
        self.position += 4;

        // Decode timestamps
        let timestamps = self.decode_timestamps(count)?;

        // Decode values
        let values = self.decode_values(count)?;

        // Combine into tuples
        let mut result = Vec::new();
        for (ts, val) in timestamps.iter().zip(values.iter()) {
            result.push((*ts, *val));
        }

        Ok(result)
    }

    fn read_bytes(&mut self, count: usize) -> IoResult<[u8; 8]> {
        if self.position + count > self.data.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Not enough data",
            ));
        }

        let mut bytes = [0u8; 8];
        for i in 0..count {
            bytes[i] = self.data[self.position + i];
        }
        self.position += count;

        Ok(bytes)
    }
}

/// Varint (variable-length integer) encoding
fn varint_encode(mut value: u64) -> Vec<u8> {
    let mut encoded = Vec::new();

    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;

        if value != 0 {
            byte |= 0x80;
        }

        encoded.push(byte);

        if value == 0 {
            break;
        }
    }

    encoded
}

/// Varint decoding
fn varint_decode(data: &[u8]) -> IoResult<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0;
    #[allow(unused_assignments)]
    let mut decoded_size = 0;

    for (i, &byte) in data.iter().enumerate() {
        decoded_size = i + 1;
        value |= ((byte & 0x7F) as u64) << shift;

        if byte & 0x80 == 0 {
            return Ok((value, decoded_size));
        }

        shift += 7;
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "Invalid varint",
    ))
}

/// Get size of varint encoding
fn varint_size(mut value: u64) -> usize {
    let mut size = 1;

    while value >= 0x80 {
        value >>= 7;
        size += 1;
    }

    size
}

/// Gorilla compression statistics
#[derive(Debug, Clone)]
pub struct GorillaStats {
    pub point_count: usize,
    pub original_size: usize,
    pub compressed_size: usize,
    pub compression_ratio: f64,
    pub timestamp_count: usize,
    pub value_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encoder_creation() {
        let encoder = GorillaEncoder::new();
        assert_eq!(encoder.values.len(), 0);
    }

    #[test]
    fn test_add_point() {
        let mut encoder = GorillaEncoder::new();
        encoder.add_point(1000, 42.5);
        assert_eq!(encoder.values.len(), 1);
        assert_eq!(encoder.timestamps.len(), 1);
    }

    #[test]
    fn test_add_multiple_points() {
        let mut encoder = GorillaEncoder::new();
        encoder.add_points(vec![(1000, 42.5), (2000, 43.5), (3000, 44.5)]);
        assert_eq!(encoder.values.len(), 3);
        assert_eq!(encoder.timestamps.len(), 3);
    }

    #[test]
    fn test_compression() {
        let mut encoder = GorillaEncoder::new();

        // Add time-series data
        for i in 0..100 {
            let ts = 1000u64 + (i as u64 * 100);
            let val = 42.0 + (i as f64 * 0.1);
            encoder.add_point(ts, val);
        }

        let result = encoder.compress();
        assert!(result.is_ok());

        let compressed = result.unwrap();
        assert!(!compressed.is_empty());

        let ratio = encoder.compression_ratio();
        assert!(ratio > 0.0);
        assert!(ratio < 1.0);
    }

    #[test]
    fn test_xor_compression() {
        let mut encoder = GorillaEncoder::new();

        // Add identical values (should compress well)
        for i in 0..50 {
            encoder.add_point(i as u64, 42.5);
        }

        encoder.compress().unwrap();
        let ratio = encoder.compression_ratio();
        assert!(ratio > 0.5); // Should compress significantly
    }

    #[test]
    fn test_stats() {
        let mut encoder = GorillaEncoder::new();
        // Add more points for compression to be effective
        encoder.add_points(vec![
            (1000, 42.5),
            (2000, 43.5),
            (3000, 44.5),
            (4000, 45.5),
            (5000, 46.5),
            (6000, 47.5),
            (7000, 48.5),
            (8000, 49.5),
            (9000, 50.5),
            (10000, 51.5),
        ]);
        encoder.compress().unwrap();

        let stats = encoder.get_stats();
        assert_eq!(stats.point_count, 10);
        assert_eq!(stats.timestamp_count, 10);
        assert_eq!(stats.value_count, 10);
        // With more points, compression should kick in
        assert!(stats.original_size > stats.compressed_size);
    }

    #[test]
    fn test_varint_encode() {
        let encoded = varint_encode(127);
        assert_eq!(encoded.len(), 1);

        let encoded = varint_encode(128);
        assert_eq!(encoded.len(), 2);

        let encoded = varint_encode(16383);
        assert_eq!(encoded.len(), 2);
    }

    #[test]
    fn test_varint_roundtrip() {
        let original = 12345u64;
        let encoded = varint_encode(original);
        let (decoded, _) = varint_decode(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encoder_stats() {
        let mut encoder = GorillaEncoder::new();
        for i in 0..1000 {
            encoder.add_point(i as u64 * 1000, 100.0 + (i as f64 * 0.01));
        }

        encoder.compress().unwrap();
        let stats = encoder.get_stats();

        println!("Gorilla Compression Stats:");
        println!("  Points: {}", stats.point_count);
        println!("  Original: {} bytes", stats.original_size);
        println!("  Compressed: {} bytes", stats.compressed_size);
        println!("  Ratio: {:.2}%", stats.compression_ratio * 100.0);

        assert!(stats.compression_ratio > 0.5);
    }
}
