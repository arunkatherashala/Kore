// Simple integration tests for Compression Phase 1
// Tests compression components working together

#[cfg(test)]
mod compression_tests {
    use kore_fileformat::compression::{DictionaryEncoder, ZstdCompressor};

    #[test]
    fn test_basic_zstd() {
        let data = b"Hello World! ".repeat(100);
        let compressor = ZstdCompressor::default_fast();
        let compressed = compressor.compress(&data).unwrap();
        println!("Basic compression ratio: {:.1}%", compressed.len() as f64 / data.len() as f64 * 100.0);
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_dictionary_basic() {
        let values: Vec<String> = vec!["test".to_string(); 10];
        let encoder = DictionaryEncoder::encode(&values).unwrap();
        assert_eq!(encoder.statistics().unique_values, 1);
    }
}
