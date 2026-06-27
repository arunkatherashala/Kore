/// Compression algorithms module
/// Provides multiple compression implementations with automatic selection

pub mod zstd;
pub mod brotli;
pub mod hybrid;

pub use zstd::{compress_zstd, decompress_zstd, select_zstd_level, CompressionLevel};
pub use brotli::{compress_brotli, decompress_brotli, select_brotli_quality, compress_delta_brotli, decompress_delta_brotli, BrotliQuality};
pub use hybrid::{compress_hybrid, decompress_hybrid, CompressionResult};
