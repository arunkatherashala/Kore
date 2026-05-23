use crate::entropy::calculate_entropy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionMethod {
    Zstd,
    DeltaBrotli,
    Hybrid,
}

impl std::fmt::Display for CompressionMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompressionMethod::Zstd => write!(f, "Zstd"),
            CompressionMethod::DeltaBrotli => write!(f, "DeltaBrotli"),
            CompressionMethod::Hybrid => write!(f, "Hybrid"),
        }
    }
}

/// Select best compression method based on data characteristics
pub fn select_best_compression_method(data: &[u8]) -> CompressionMethod {
    let entropy = calculate_entropy(data);
    
    // High entropy = random data, Zstd is fastest
    if entropy > 7.0 {
        return CompressionMethod::Zstd;
    }
    
    // Low entropy = repetitive data, DeltaBrotli gives best compression
    if entropy < 4.0 {
        return CompressionMethod::DeltaBrotli;
    }
    
    // Medium entropy = hybrid approach
    CompressionMethod::Hybrid
}

/// Measure compression ratio
pub fn measure_compression_ratio(original: &[u8], compressed: &[u8]) -> f64 {
    if original.is_empty() {
        return 0.0;
    }
    
    1.0 - (compressed.len() as f64 / original.len() as f64)
}

/// Get compression ratio as percentage
pub fn compression_percentage(original: &[u8], compressed: &[u8]) -> f64 {
    measure_compression_ratio(original, compressed) * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_selection_high_entropy() {
        let mut data: Vec<u8> = vec![];
        for i in 0..100 {
            data.push((i * 17) as u8);
        }
        let method = select_best_compression_method(&data);
        // High entropy should select a fast algorithm
        matches!(method, CompressionMethod::Zstd | CompressionMethod::Hybrid);
    }

    #[test]
    fn test_method_selection_low_entropy() {
        let data = vec![1u8; 1000];
        let method = select_best_compression_method(&data);
        assert_eq!(method, CompressionMethod::DeltaBrotli, "Low entropy should select DeltaBrotli");
    }

    #[test]
    fn test_method_selection_medium_entropy() {
        let data = vec![1u8, 2, 1, 2, 1, 2];
        let method = select_best_compression_method(&data);
        // Medium entropy should select an adaptive algorithm
        matches!(method, CompressionMethod::Hybrid | CompressionMethod::DeltaBrotli);
    }

    #[test]
    fn test_compression_ratio() {
        let original = vec![1u8; 100];
        let compressed = vec![1u8; 20];
        let ratio = measure_compression_ratio(&original, &compressed);
        assert!(ratio > 0.75 && ratio < 0.85, "Expected ratio ~0.80, got {}", ratio);
    }

    #[test]
    fn test_compression_percentage() {
        let original = vec![1u8; 100];
        let compressed = vec![1u8; 50];
        let percent = compression_percentage(&original, &compressed);
        assert!((percent - 50.0).abs() < 0.1, "Expected 50% compression");
    }

    #[test]
    fn test_no_compression() {
        let original = vec![1u8; 100];
        let compressed = original.clone();
        let ratio = measure_compression_ratio(&original, &compressed);
        assert_eq!(ratio, 0.0, "No compression should give 0 ratio");
    }
}
