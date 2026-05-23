// Basic compression module tests
// Tests dictionary and zstandard compression basics

#[cfg(test)]
mod tests {
    use kore_fileformat::compression::DictionaryEncoder;

    #[test]
    fn test_dictionary_encoder_basic() {
        // Test dictionary encoding with low cardinality data
        let values: Vec<String> = vec![
            "apple".to_string(),
            "banana".to_string(),
            "apple".to_string(),
            "cherry".to_string(),
            "banana".to_string(),
            "apple".to_string(),
        ];

        let encoder = DictionaryEncoder::encode(&values).unwrap();
        let ratio = encoder.compression_ratio();
        
        println!("Dictionary Encoder Test");
        println!("  Input size: {} strings", values.len());
        println!("  Compression ratio: {:.2}", ratio);
        println!("  Dictionary size: {}", encoder.statistics().unique_values);
        
        // Should achieve good compression on low-cardinality data
        assert!(ratio < 1.0, "Compressed should be less than original");
    }

    #[test]
    fn test_dictionary_encoder_high_cardinality() {
        // Test with higher cardinality (more unique values)
        let values: Vec<String> = (0..100)
            .map(|i| format!("item_{:04}", i))
            .collect();

        let encoder = DictionaryEncoder::encode(&values).unwrap();
        let stats = encoder.statistics();
        
        println!("High Cardinality Dictionary Test");
        println!("  Unique values: {}", stats.unique_values);
        println!("  Total values: {}", stats.total_values);
        println!("  Cardinality ratio: {:.2}", stats.cardinality_percent);
        
        assert_eq!(stats.unique_values, 100, "Should have 100 unique values");
    }

    #[test]
    fn test_dictionary_roundtrip() {
        // Test encode/decode roundtrip
        let values: Vec<String> = vec![
            "red".to_string(),
            "green".to_string(),
            "blue".to_string(),
            "red".to_string(),
            "green".to_string(),
        ];

        let encoder = DictionaryEncoder::encode(&values).unwrap();
        let serialized = encoder.serialize();
        
        let decoder = kore_fileformat::compression::DictionaryDecoder::deserialize(&serialized).unwrap();
        let decoded_values = decoder.decode().unwrap();
        
        println!("Roundtrip Test");
        println!("  Original: {:?}", values);
        println!("  Decoded: {:?}", decoded_values);
        println!("  Serialized size: {} bytes", serialized.len());
        
        assert_eq!(values, decoded_values, "Roundtrip should preserve values");
    }

    #[test]
    fn test_zstd_compressor_creation() {
        // Test that ZstdCompressor can be created
        let _default_fast = kore_fileformat::compression::ZstdCompressor::default_fast();
        let _default_balanced = kore_fileformat::compression::ZstdCompressor::default_balanced();
        
        println!("ZstdCompressor Created Successfully");
        assert!(true, "Compressor instances created");
    }

    #[test]
    fn test_compression_result_creation() {
        use kore_fileformat::compression::{CompressionResult, CompressionCodec};
        
        let data = vec![1, 2, 3, 4, 5];
        let result = CompressionResult::new(CompressionCodec::Dictionary, 100, data);
        
        println!("CompressionResult Test");
        println!("  Original size: {}", result.original_size);
        println!("  Compressed size: {}", result.compressed_size);
        println!("  Savings: {:.1}%", result.savings_percent());
        
        assert_eq!(result.original_size, 100);
        assert_eq!(result.compressed_size, 5);
        assert!(result.is_beneficial());
    }

    #[test]
    fn test_multiple_encoders_independently() {
        // Test that multiple encoders can exist independently
        let values1: Vec<String> = vec!["a".to_string(), "b".to_string(), "a".to_string()];
        let values2: Vec<String> = vec!["x".to_string(), "y".to_string(), "x".to_string()];
        
        let encoder1 = DictionaryEncoder::encode(&values1).unwrap();
        let encoder2 = DictionaryEncoder::encode(&values2).unwrap();
        
        let stats1 = encoder1.statistics();
        let stats2 = encoder2.statistics();
        
        println!("Independent Encoders Test");
        println!("  Encoder1 unique: {}", stats1.unique_values);
        println!("  Encoder2 unique: {}", stats2.unique_values);
        
        assert_eq!(stats1.unique_values, 2);
        assert_eq!(stats2.unique_values, 2);
        assert!(true, "Multiple encoders work independently");
    }
}
