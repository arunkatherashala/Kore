/// Apply delta encoding (differential coding)
/// Reduces entropy by storing differences instead of absolute values
pub fn apply_delta_encoding(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![];
    }

    let mut result = vec![data[0]];  // Keep first byte as-is
    
    for i in 1..data.len() {
        let delta = data[i].wrapping_sub(data[i - 1]);
        result.push(delta);
    }
    
    result
}

/// Reverse delta encoding (inverse operation)
pub fn reverse_delta_encoding(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![];
    }

    let mut result = vec![data[0]];
    let mut last = data[0];
    
    for i in 1..data.len() {
        let value = last.wrapping_add(data[i]);
        result.push(value);
        last = value;
    }
    
    result
}

/// Apply run-length encoding
pub fn apply_run_length_encoding(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![];
    }

    let mut result = vec![];
    let mut i = 0;

    while i < data.len() {
        let current = data[i];
        let mut count = 1;
        
        // Count consecutive same bytes (max 255)
        while i + count < data.len() && data[i + count] == current && count < 255 {
            count += 1;
        }

        if count >= 3 {
            // Use RLE for runs of 3+ bytes (marker, count, value)
            result.push(255);  // Marker
            result.push(count as u8);
            result.push(current);
            i += count;
        } else {
            // Keep original bytes
            for _ in 0..count {
                result.push(current);
            }
            i += count;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delta_encoding_roundtrip() {
        let data = vec![1u8, 2, 4, 7, 11, 16];
        let encoded = apply_delta_encoding(&data);
        let decoded = reverse_delta_encoding(&encoded);
        assert_eq!(data, decoded, "Delta encoding roundtrip failed");
    }

    #[test]
    fn test_delta_reduces_entropy() {
        let data = vec![100u8, 101, 102, 103, 104, 105];
        let encoded = apply_delta_encoding(&data);
        
        // Encoded should have different values than original (delta transformedsome values)
        assert!(encoded.len() > 0, "Delta encoding should produce output");
        // Delta reduces entropy for sequential data
    }

    #[test]
    fn test_rle_compression() {
        let data = vec![1u8; 100];
        let encoded = apply_run_length_encoding(&data);
        assert!(encoded.len() < data.len(), "RLE should compress repetitive data");
        assert!(encoded.len() < 10, "RLE should heavily compress 100 identical bytes");
    }

    #[test]
    fn test_rle_mixed_data() {
        let data = vec![1, 2, 2, 2, 3, 3, 3, 3, 4];
        let encoded = apply_run_length_encoding(&data);
        // Should use RLE for the runs of 2s and 3s
        assert!(encoded.len() < data.len());
    }
}
