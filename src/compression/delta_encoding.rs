// src/compression/delta_encoding.rs
// Delta encoding for monotonic/near-monotonic numeric columns

pub struct DeltaEncoder;

impl DeltaEncoder {
    /// Check if column is monotonic or near-monotonic
    /// Applies if >90% of deltas are positive and increasing
    pub fn detect_monotonic(data: &[u32]) -> bool {
        if data.len() < 2 {
            return false;
        }

        let mut monotonic_count = 0;
        for i in 1..data.len() {
            if data[i] >= data[i - 1] {
                monotonic_count += 1;
            }
        }

        let ratio = monotonic_count as f64 / (data.len() - 1) as f64;
        ratio > 0.9
    }

    /// Encode deltas instead of absolute values
    /// [100, 103, 105, 108, 110] → [100, 3, 2, 3, 2]
    pub fn encode_deltas(data: &[u32]) -> Vec<u32> {
        let mut result = vec![];
        
        if data.is_empty() {
            return result;
        }

        result.push(data[0]); // Store first value
        
        for i in 1..data.len() {
            let delta = data[i].saturating_sub(data[i - 1]);
            result.push(delta);
        }

        result
    }

    /// Decode deltas back to original values
    pub fn decode_deltas(encoded: &[u32]) -> Vec<u32> {
        let mut result = vec![];
        
        if encoded.is_empty() {
            return result;
        }

        result.push(encoded[0]);
        
        for i in 1..encoded.len() {
            let value = result[i - 1].saturating_add(encoded[i]);
            result.push(value);
        }

        result
    }

    /// Double delta encoding: encode deltas of deltas
    /// Even better compression for smooth monotonic sequences
    pub fn encode_double_deltas(data: &[u32]) -> Vec<u32> {
        let deltas = Self::encode_deltas(data);
        let double_deltas = Self::encode_deltas(&deltas);
        double_deltas
    }

    /// Estimate compression ratio with delta encoding
    pub fn estimate_gain(data: &[u32]) -> f64 {
        if !Self::detect_monotonic(data) {
            return 0.0; // No gain on non-monotonic data
        }

        let deltas = Self::encode_deltas(data);
        
        // Average delta value
        let avg_delta: u32 = if deltas.len() > 1 {
            deltas[1..].iter().sum::<u32>() / (deltas.len() - 1) as u32
        } else {
            0
        };

        // If deltas are small, compression gains are high
        if avg_delta < 100 {
            0.5 // 50% compression gain expected
        } else if avg_delta < 10000 {
            0.2 // 20% compression gain
        } else {
            0.0 // No gain
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delta_encoding() {
        let data = vec![100, 103, 105, 108, 110];
        let encoded = DeltaEncoder::encode_deltas(&data);
        
        assert_eq!(encoded[0], 100); // First value unchanged
        assert_eq!(encoded[1], 3);   // 103 - 100
        assert_eq!(encoded[2], 2);   // 105 - 103
    }

    #[test]
    fn test_delta_decoding() {
        let original = vec![100, 103, 105, 108, 110];
        let encoded = DeltaEncoder::encode_deltas(&original);
        let decoded = DeltaEncoder::decode_deltas(&encoded);
        
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_monotonic_detection() {
        let monotonic = vec![1, 2, 3, 4, 5];
        assert!(DeltaEncoder::detect_monotonic(&monotonic));

        let random = vec![5, 1, 3, 2, 4];
        assert!(!DeltaEncoder::detect_monotonic(&random));
    }
}
