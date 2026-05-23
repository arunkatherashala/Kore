// Simple integration tests for Compression Phase 1
// Tests all three compression components working together

#[cfg(test)]
mod compression_tests {
    use kore_fileformat::compression::{
        DictionaryEncoder, ZstdCompressor, ZstdDecompressor,
    };

    #[test]
    fn test_dictionary_encoding_basic() {
        let values: Vec<String> = vec![
            "customer".to_string(),
            "vendor".to_string(),
            "customer".to_string(),
            "vendor".to_string(),
        ];
        
        let encoder = DictionaryEncoder::encode(&values).unwrap();
        let stats = encoder.statistics();
        
        println!("Dictionary encoding test:");
        println!("  Unique values: {}", stats.unique_values);
        println!("  Total values: {}", stats.total_values);
        println!("  Cardinality: {:.1}%", stats.cardinality_percent);
        
        assert_eq!(stats.unique_values, 2);
        assert_eq!(stats.total_values, 4);
    }

    #[test]
    fn test_zstd_compressor_creation() {
        // Test that ZstdCompressor can be created
        let _default_fast = kore_fileformat::compression::ZstdCompressor::default_fast();
        let _default_balanced = kore_fileformat::compression::ZstdCompressor::default_balanced();
        
        println!("ZstdCompressor created successfully");
    }

    #[test]
    fn test_zstd_numeric_compression() {
        // Create numeric-like data (repeated patterns)
        let mut data = Vec::new();
        for i in 0..1000 {
            data.extend_from_slice(&(i as u64).to_le_bytes());
        }
        
        let compressor = ZstdCompressor::default_fast();
        let compressed = compressor.compress(&data).unwrap();
        let ratio = compressed.len() as f64 / data.len() as f64;
        
        println!("Zstd compression ratio: {:.2}%", ratio * 100.0);
        // Zstandard typically achieves 40-60% on numeric data
        assert!(ratio < 0.95, "Should achieve some compression");
    }
}
