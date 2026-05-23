pub mod entropy;
pub mod delta;
pub mod selector;
pub mod algorithms;

pub use entropy::{calculate_entropy, estimate_compressibility};
pub use delta::{apply_delta_encoding, reverse_delta_encoding, apply_run_length_encoding};
pub use selector::{CompressionMethod, select_best_compression_method, measure_compression_ratio, compression_percentage};
pub use algorithms::{CompressionLevel, compress_zstd, decompress_zstd, select_zstd_level, compress_hybrid, decompress_hybrid, CompressionResult};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integration_repetitive_data() {
        let data = vec![1u8; 10000];
        let entropy = calculate_entropy(&data);
        let method = select_best_compression_method(&data);
        
        assert!(entropy < 1.0, "Repetitive data should have low entropy");
        assert_eq!(method, CompressionMethod::DeltaBrotli, "Should select DeltaBrotli for low entropy");
    }

    #[test]
    fn test_integration_random_data() {
        let mut data = vec![];
        for i in 0..1000 {
            data.push((i * 11) as u8);
        }
        let entropy = calculate_entropy(&data);
        let method = select_best_compression_method(&data);
        
        assert!(entropy > 7.0, "Random data should have high entropy");
        assert_eq!(method, CompressionMethod::Zstd, "Should select Zstd for high entropy");
    }
}
