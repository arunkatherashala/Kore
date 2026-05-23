/// Calculate Shannon entropy of data (0.0 to 8.0)
pub fn calculate_entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }

    let mut frequency = [0u32; 256];
    for &byte in data {
        frequency[byte as usize] += 1;
    }

    let len = data.len() as f64;
    let mut entropy = 0.0;

    for &count in &frequency {
        if count > 0 {
            let p = count as f64 / len;
            entropy -= p * p.log2();
        }
    }

    entropy
}

/// Estimate data compressibility (0.0 to 1.0)
/// Higher value = more compressible
pub fn estimate_compressibility(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }

    let entropy = calculate_entropy(data);
    // Maximum entropy for 8-bit data is 8.0
    // If entropy is low, data is highly compressible
    1.0 - (entropy / 8.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entropy_all_same() {
        let data = vec![1u8; 100];
        let entropy = calculate_entropy(&data);
        assert!(entropy < 0.1, "Same byte should have near-zero entropy, got {}", entropy);
    }

    #[test]
    fn test_entropy_random() {
        let mut data = vec![];
        for i in 0..256 {
            data.push(i as u8);
        }
        let entropy = calculate_entropy(&data);
        assert!(entropy > 7.9 && entropy < 8.1, "Uniform data should have ~8.0 entropy, got {}", entropy);
    }

    #[test]
    fn test_compressibility_high() {
        let data = vec![1u8; 1000];
        let comp = estimate_compressibility(&data);
        assert!(comp > 0.9, "Repetitive data should be highly compressible, got {}", comp);
    }

    #[test]
    fn test_compressibility_low() {
        let mut data = vec![];
        for i in 0..256 {
            data.push((i * 7) as u8);  // Pseudo-random
        }
        let comp = estimate_compressibility(&data);
        assert!(comp < 0.2, "Random data should have low compressibility, got {}", comp);
    }
}
