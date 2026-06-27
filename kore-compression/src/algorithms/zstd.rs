/// Zstd compression wrapper using zstd::bulk API
/// Fast compression suitable for high-entropy (random) data

#[derive(Debug, Clone, Copy)]
pub enum CompressionLevel {
    Fast,           // 1-3
    Balanced,       // 4-7
    BestCompression, // 8-22
}

impl CompressionLevel {
    pub fn to_i32(&self) -> i32 {
        match self {
            CompressionLevel::Fast => 3,
            CompressionLevel::Balanced => 7,
            CompressionLevel::BestCompression => 15,
        }
    }
}

/// Compress data using Zstd algorithm
pub fn compress_zstd(data: &[u8], level: CompressionLevel) -> Result<Vec<u8>, String> {
    if data.is_empty() {
        return Ok(vec![]);
    }

    let zstd_level = level.to_i32();
    zstd::bulk::compress(data, zstd_level)
        .map_err(|e| format!("Zstd compression failed: {}", e))
}

/// Decompress Zstd data
pub fn decompress_zstd(compressed: &[u8]) -> Result<Vec<u8>, String> {
    if compressed.is_empty() {
        return Ok(vec![]);
    }

    // Zstd frames may contain uncompressed size hint, try progressively larger buffers
    let mut estimated_capacity = (compressed.len() * 10).max(1024 * 1024);
    
    loop {
        match zstd::bulk::decompress(compressed, estimated_capacity) {
            Ok(result) => return Ok(result),
            Err(_) if estimated_capacity < 1024 * 1024 * 100 => {
                estimated_capacity *= 2;
            }
            Err(e) => return Err(format!("Zstd decompression failed: {}", e)),
        }
    }
}

/// Select Zstd level based on entropy
pub fn select_zstd_level(data: &[u8]) -> CompressionLevel {
    if data.len() < 100 {
        return CompressionLevel::Fast;
    }

    // Count byte diversity
    let mut frequency = [0u32; 256];
    for &byte in data {
        frequency[byte as usize] += 1;
    }

    let unique_bytes = frequency.iter().filter(|&&f| f > 0).count();
    let entropy_ratio = unique_bytes as f64 / 256.0;

    // High entropy (random) = use fast compression
    if entropy_ratio > 0.8 {
        CompressionLevel::Fast
    }
    // Medium entropy = balanced
    else if entropy_ratio > 0.5 {
        CompressionLevel::Balanced
    }
    // Low entropy = best compression
    else {
        CompressionLevel::BestCompression
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let data = b"Hello, World!";
        let compressed = compress_zstd(data, CompressionLevel::Balanced).unwrap();
        let decompressed = decompress_zstd(&compressed).unwrap();
        assert_eq!(data.to_vec(), decompressed);
    }

    #[test]
    fn test_empty() {
        let compressed = compress_zstd(b"", CompressionLevel::Balanced).unwrap();
        assert!(compressed.is_empty());
    }

    #[test]
    fn test_compression_levels() {
        let data = vec![42u8; 5000];
        let _fast = compress_zstd(&data, CompressionLevel::Fast).unwrap();
        let _balanced = compress_zstd(&data, CompressionLevel::Balanced).unwrap();
        let _best = compress_zstd(&data, CompressionLevel::BestCompression).unwrap();
    }

    #[test]
    fn test_high_entropy_selection() {
        let mut data = vec![];
        for i in 0..256 {
            data.push((i * 17) as u8);
        }
        let level = select_zstd_level(&data);
        matches!(level, CompressionLevel::Fast);
    }

    #[test]
    fn test_low_entropy_selection() {
        let data = vec![1u8; 1000];
        let level = select_zstd_level(&data);
        matches!(level, CompressionLevel::BestCompression);
    }

    #[test]
    fn test_compression_ratio() {
        let data = vec![42u8; 100000];
        let compressed = compress_zstd(&data, CompressionLevel::BestCompression).unwrap();
        assert!(compressed.len() < 10000);
    }

    #[test]
    fn test_large_data() {
        let data = vec![7u8; 1000000];
        let compressed = compress_zstd(&data, CompressionLevel::Balanced).unwrap();
        let decompressed = decompress_zstd(&compressed).unwrap();
        assert_eq!(data.len(), decompressed.len());
    }
}
