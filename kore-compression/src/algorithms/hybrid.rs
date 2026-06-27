use crate::entropy::calculate_entropy;
use super::zstd::{compress_zstd, decompress_zstd, CompressionLevel as ZstdLevel};
use super::brotli::{compress_brotli, decompress_brotli, BrotliQuality};

/// Hybrid compression result
#[derive(Debug, Clone)]
pub struct CompressionResult {
    pub data: Vec<u8>,
    pub algorithm: String,
    pub ratio: f64,
    pub original_size: usize,
    pub compressed_size: usize,
}

impl CompressionResult {
    pub fn compression_percentage(&self) -> f64 {
        self.ratio * 100.0
    }
}

/// Intelligently select and apply best compression algorithm
/// Analyzes data entropy and characteristics to choose optimal strategy
pub fn compress_hybrid(data: &[u8]) -> Result<CompressionResult, String> {
    if data.is_empty() {
        return Ok(CompressionResult {
            data: vec![],
            algorithm: "None".to_string(),
            ratio: 0.0,
            original_size: 0,
            compressed_size: 0,
        });
    }

    let entropy = calculate_entropy(data);
    let original_size = data.len();

    // Try multiple algorithms and pick the best
    let results = vec![
        try_zstd(data, original_size, entropy),
        try_brotli(data, original_size, entropy),
        try_delta_brotli(data, original_size, entropy),
    ];

    // Find best compression (smallest output)
    let best = results
        .into_iter()
        .filter_map(|r| r.ok())
        .min_by_key(|r| r.compressed_size)
        .ok_or_else(|| "All compression algorithms failed".to_string())?;

    Ok(best)
}

/// Decompress hybrid-encoded data
pub fn decompress_hybrid(compressed: &[u8], algorithm: &str) -> Result<Vec<u8>, String> {
    match algorithm {
        "Zstd" => decompress_zstd(compressed),
        "Brotli" => decompress_brotli(compressed),
        "DeltaBrotli" => super::brotli::decompress_delta_brotli(compressed),
        _ => Err(format!("Unknown algorithm: {}", algorithm)),
    }
}

/// Try Zstd compression
fn try_zstd(data: &[u8], original_size: usize, entropy: f64) -> Result<CompressionResult, String> {
    // Zstd is best for high-entropy data (fast compression)
    let level = if entropy > 7.0 {
        ZstdLevel::Fast
    } else {
        ZstdLevel::Balanced
    };

    let compressed = compress_zstd(data, level)?;
    let ratio = 1.0 - (compressed.len() as f64 / original_size as f64);

    Ok(CompressionResult {
        data: compressed.clone(),
        algorithm: "Zstd".to_string(),
        ratio,
        original_size,
        compressed_size: compressed.len(),
    })
}

/// Try Brotli compression
fn try_brotli(data: &[u8], original_size: usize, entropy: f64) -> Result<CompressionResult, String> {
    // Brotli is best for medium entropy data with good compression
    let quality = if entropy > 6.0 {
        BrotliQuality::Fast
    } else if entropy > 4.0 {
        BrotliQuality::Balanced
    } else {
        BrotliQuality::BestRatio
    };

    let compressed = compress_brotli(data, quality)?;
    let ratio = 1.0 - (compressed.len() as f64 / original_size as f64);

    Ok(CompressionResult {
        data: compressed.clone(),
        algorithm: "Brotli".to_string(),
        ratio,
        original_size,
        compressed_size: compressed.len(),
    })
}

/// Try Delta + Brotli pipeline
fn try_delta_brotli(data: &[u8], original_size: usize, entropy: f64) -> Result<CompressionResult, String> {
    // Delta + Brotli is best for low-entropy, repetitive data
    if entropy < 4.0 {
        let compressed = super::brotli::compress_delta_brotli(data)?;
        let ratio = 1.0 - (compressed.len() as f64 / original_size as f64);

        Ok(CompressionResult {
            data: compressed.clone(),
            algorithm: "DeltaBrotli".to_string(),
            ratio,
            original_size,
            compressed_size: compressed.len(),
        })
    } else {
        Err("Entropy too high for Delta+Brotli".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hybrid_random_data() {
        let mut data = vec![];
        for i in 0..10000 {
            data.push((i * 7) as u8);
        }

        let result = compress_hybrid(&data).unwrap();
        assert_eq!(result.algorithm, "Zstd", "Random data should select Zstd");
        assert!(result.ratio > 0.0, "Should have some compression");
    }

    #[test]
    fn test_hybrid_repetitive_data() {
        let data = vec![42u8; 100000];

        let result = compress_hybrid(&data).unwrap();
        // Repetitive data should be highly compressible with any algorithm
        assert!(result.ratio > 0.85, "Should achieve high compression on repetitive data");
    }

    #[test]
    fn test_hybrid_mixed_data() {
        let mut data = vec![];
        // Create pattern with some repetition
        for i in 0..5000 {
            if i % 10 < 5 {
                data.push(42u8);
            } else {
                data.push((i * 3) as u8);
            }
        }

        let result = compress_hybrid(&data).unwrap();
        assert!(result.ratio > 0.0, "Should compress mixed data");
    }

    #[test]
    fn test_hybrid_roundtrip() {
        let data = b"This is a test message that will be compressed and decompressed";

        let compressed = compress_hybrid(data).unwrap();
        let decompressed = decompress_hybrid(&compressed.data, &compressed.algorithm).unwrap();

        assert_eq!(data, decompressed.as_slice(), "Roundtrip should preserve data");
    }

    #[test]
    fn test_hybrid_empty_data() {
        let data = b"";

        let result = compress_hybrid(data).unwrap();
        assert_eq!(result.compressed_size, 0, "Empty data should compress to empty");
    }

    #[test]
    fn test_hybrid_compression_result() {
        let data = vec![5u8; 50000];

        let result = compress_hybrid(&data).unwrap();
        assert_eq!(result.original_size, 50000);
        assert!(result.compressed_size < 50000);
        assert!(result.ratio > 0.5);
        assert!(result.compression_percentage() > 50.0);
    }

    #[test]
    fn test_hybrid_selects_best() {
        let mut data = vec![];
        for i in 0..20000 {
            data.push((i * 11 + 7) as u8);
        }

        let result = compress_hybrid(&data).unwrap();
        // Should pick the algorithm that gives best compression

        let zstd_result = try_zstd(&data, data.len(), calculate_entropy(&data)).unwrap();
        let brotli_result = try_brotli(&data, data.len(), calculate_entropy(&data)).unwrap();

        // Result should be as good or better than individual algorithms
        assert!(
            result.compressed_size <= zstd_result.compressed_size,
            "Hybrid should not be worse than Zstd"
        );
        assert!(
            result.compressed_size <= brotli_result.compressed_size,
            "Hybrid should not be worse than Brotli"
        );
    }

    #[test]
    fn test_decompress_hybrid_zstd() {
        let data = b"Test data for Zstd decompression";
        let result = compress_hybrid(data).unwrap();

        if result.algorithm == "Zstd" {
            let decompressed = decompress_hybrid(&result.data, "Zstd").unwrap();
            assert_eq!(data, decompressed.as_slice());
        }
    }

    #[test]
    fn test_decompress_unknown_algorithm() {
        let result = decompress_hybrid(&vec![1, 2, 3], "UnknownAlgo");
        assert!(result.is_err(), "Should fail on unknown algorithm");
    }

    #[test]
    fn test_hybrid_large_data() {
        let data = vec![100u8; 10_000_000];

        let result = compress_hybrid(&data).unwrap();
        assert!(result.compressed_size < data.len(), "Should compress large data");

        let decompressed = decompress_hybrid(&result.data, &result.algorithm).unwrap();
        assert_eq!(data.len(), decompressed.len(), "Large data roundtrip should work");
    }
}
