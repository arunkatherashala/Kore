/// Adaptive block sizing for v1.1.6
/// Analyzes data entropy and selects optimal block size (4KB-256KB)
/// Lower entropy = larger blocks, Higher entropy = smaller blocks
/// Target: 15-25% compression improvement from better block granularity

#[derive(Debug, Clone)]
pub struct BlockProfile {
    pub optimal_block_size: usize,
    pub entropy: f64,
    pub repetitiveness: f64,
    pub recommended_codec: &'static str,
}

/// Adaptive block sizing engine
pub struct AdaptiveBlocker;

impl AdaptiveBlocker {
    const MIN_BLOCK_SIZE: usize = 4096;      // 4KB
    const MAX_BLOCK_SIZE: usize = 262144;    // 256KB
    const DEFAULT_BLOCK_SIZE: usize = 65536; // 64KB

    /// Analyze data and recommend block size
    pub fn analyze_block_profile(data: &[u8]) -> BlockProfile {
        let entropy = Self::calculate_entropy(data);
        let repetitiveness = Self::calculate_repetitiveness(data);

        let optimal_block_size = Self::calculate_optimal_block_size(entropy, repetitiveness);
        let recommended_codec = Self::recommend_codec(entropy);

        BlockProfile {
            optimal_block_size,
            entropy,
            repetitiveness,
            recommended_codec,
        }
    }

    /// Calculate Shannon entropy (0-8 bits per byte)
    fn calculate_entropy(data: &[u8]) -> f64 {
        let mut freq = [0usize; 256];
        for &byte in data {
            freq[byte as usize] += 1;
        }

        let len = data.len() as f64;
        let mut entropy = 0.0;

        for count in &freq {
            if *count > 0 {
                let p = *count as f64 / len;
                entropy -= p * p.log2();
            }
        }
        entropy
    }

    /// Calculate repetitiveness score (how much is repeated)
    /// Returns 0.0 (random) to 1.0 (all same)
    fn calculate_repetitiveness(data: &[u8]) -> f64 {
        if data.is_empty() {
            return 0.0;
        }

        let mut run_count = 0;
        let mut total_run_bytes = 0;
        let mut current_run = 1;

        for window in data.windows(2) {
            if window[0] == window[1] {
                current_run += 1;
            } else {
                if current_run > 1 {
                    run_count += 1;
                    total_run_bytes += current_run;
                }
                current_run = 1;
            }
        }

        // If last bytes were a run
        if current_run > 1 {
            run_count += 1;
            total_run_bytes += current_run;
        }

        // Score: proportion of data in runs
        let run_ratio = total_run_bytes as f64 / data.len() as f64;
        run_ratio.min(1.0)
    }

    /// Calculate optimal block size based on entropy and repetitiveness
    /// Low entropy + high repetitiveness = larger blocks
    /// High entropy + low repetitiveness = smaller blocks
    fn calculate_optimal_block_size(entropy: f64, repetitiveness: f64) -> usize {
        // Score: 0 (random) to 2 (highly compressible)
        let compressibility = (8.0 - entropy) + repetitiveness;

        match compressibility {
            c if c > 6.0 => 262144,  // 256KB - highly repetitive
            c if c > 5.0 => 131072,  // 128KB - very compressible
            c if c > 4.0 => 65536,   // 64KB - compressible
            c if c > 3.0 => 32768,   // 32KB - moderate
            c if c > 2.0 => 16384,   // 16KB - low compressibility
            _ => 8192,               // 8KB - random/high entropy
        }
    }

    /// Recommend codec based on entropy
    fn recommend_codec(entropy: f64) -> &'static str {
        match entropy {
            e if e < 2.0 => "RLE",           // Highly repetitive
            e if e < 3.0 => "Dictionary",    // Repetitive patterns
            e if e < 4.0 => "FOR",           // Mixed patterns
            e if e < 5.0 => "LZSS",          // Moderate entropy
            e if e < 6.0 => "ZSTD_Advanced", // High entropy
            _ => "LZ4",                      // Very high entropy
        }
    }

    /// Split data into adaptive blocks
    pub fn create_adaptive_blocks(data: &[u8]) -> Vec<Vec<u8>> {
        let profile = Self::analyze_block_profile(data);
        let block_size = profile.optimal_block_size;

        data.chunks(block_size)
            .map(|chunk| chunk.to_vec())
            .collect()
    }

    /// Calculate compression effectiveness for a block
    pub fn estimate_compression_ratio(block: &[u8]) -> f64 {
        let entropy = Self::calculate_entropy(block);
        
        // Heuristic: entropy_bits / 8 = uncompressed ratio
        // But we can always compress to at least entropy bits
        let theoretical_min = entropy / 8.0;
        
        // Add overhead factor (header, etc)
        let overhead = 0.02; // 2% overhead
        (theoretical_min + overhead).min(1.0) // Can't exceed 100%
    }
}

/// Block compression result tracking
#[derive(Debug, Clone)]
pub struct BlockCompressionResult {
    pub block_index: usize,
    pub original_size: usize,
    pub compressed_size: usize,
    pub compression_ratio: f64,
    pub codec_used: String,
    pub compression_time_us: u128,
}

impl BlockCompressionResult {
    pub fn new(
        block_index: usize,
        original_size: usize,
        compressed_size: usize,
        codec_used: String,
    ) -> Self {
        let compression_ratio = if original_size > 0 {
            compressed_size as f64 / original_size as f64
        } else {
            1.0
        };

        Self {
            block_index,
            original_size,
            compressed_size,
            compression_ratio,
            codec_used,
            compression_time_us: 0,
        }
    }

    /// Calculate compression savings in bytes
    pub fn bytes_saved(&self) -> usize {
        self.original_size.saturating_sub(self.compressed_size)
    }
}

/// Multi-block compression orchestrator
pub struct BlockCompressionOrchestrator {
    results: Vec<BlockCompressionResult>,
    total_original: usize,
    total_compressed: usize,
}

impl BlockCompressionOrchestrator {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            total_original: 0,
            total_compressed: 0,
        }
    }

    pub fn add_result(&mut self, result: BlockCompressionResult) {
        self.total_original += result.original_size;
        self.total_compressed += result.compressed_size;
        self.results.push(result);
    }

    pub fn overall_ratio(&self) -> f64 {
        if self.total_original == 0 {
            1.0
        } else {
            self.total_compressed as f64 / self.total_original as f64
        }
    }

    pub fn best_block(&self) -> Option<&BlockCompressionResult> {
        self.results
            .iter()
            .min_by(|a, b| a.compression_ratio.partial_cmp(&b.compression_ratio).unwrap_or(std::cmp::Ordering::Equal))
    }

    pub fn worst_block(&self) -> Option<&BlockCompressionResult> {
        self.results
            .iter()
            .max_by(|a, b| a.compression_ratio.partial_cmp(&b.compression_ratio).unwrap_or(std::cmp::Ordering::Equal))
    }

    pub fn summary(&self) -> String {
        format!(
            "Blocks: {} | Original: {} MB | Compressed: {} MB | Ratio: {:.1}%",
            self.results.len(),
            self.total_original / 1024 / 1024,
            self.total_compressed / 1024 / 1024,
            self.overall_ratio() * 100.0
        )
    }
}

impl Default for BlockCompressionOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entropy_uniform() {
        let uniform = vec![42u8; 1000];
        let entropy = AdaptiveBlocker::calculate_entropy(&uniform);
        assert!(entropy < 0.1);
    }

    #[test]
    fn test_entropy_random() {
        let random: Vec<u8> = (0..256).cycle().take(1024).map(|x| x as u8).collect();
        let entropy = AdaptiveBlocker::calculate_entropy(&random);
        assert!(entropy > 7.5); // ~8 bits per byte
    }

    #[test]
    fn test_repetitiveness_high() {
        let repetitive = [1u8, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2]
            .to_vec();
        let rep = AdaptiveBlocker::calculate_repetitiveness(&repetitive);
        assert!(rep > 0.5);
    }

    #[test]
    fn test_block_size_selection() {
        // High entropy = smaller blocks
        let high_entropy_size = AdaptiveBlocker::calculate_optimal_block_size(7.5, 0.1);
        assert!(high_entropy_size <= 65536);

        // Low entropy = larger blocks
        let low_entropy_size = AdaptiveBlocker::calculate_optimal_block_size(1.0, 0.9);
        assert_eq!(low_entropy_size, 262144);
    }

    #[test]
    fn test_compression_ratio_estimation() {
        let uniform = vec![1u8; 1000];
        let ratio = AdaptiveBlocker::estimate_compression_ratio(&uniform);
        assert!(ratio < 0.5); // Should be highly compressible
    }

    #[test]
    fn test_adaptive_blocks() {
        let data = (0..10000usize).map(|x| x as u8).collect::<Vec<u8>>();
        let blocks = AdaptiveBlocker::create_adaptive_blocks(&data);
        assert!(!blocks.is_empty());
        assert!(blocks[0].len() <= AdaptiveBlocker::MAX_BLOCK_SIZE);
    }

    #[test]
    fn test_orchestrator() {
        let mut orch = BlockCompressionOrchestrator::new();
        orch.add_result(BlockCompressionResult::new(0, 1000, 500, "RLE".to_string()));
        orch.add_result(BlockCompressionResult::new(1, 1000, 700, "ZSTD".to_string()));

        assert_eq!(orch.overall_ratio(), 0.6);
        assert!(orch.best_block().is_some());
    }
}
