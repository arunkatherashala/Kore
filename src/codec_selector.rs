/// Week 7: Hybrid Compression Selection
/// 
/// Automatically analyzes column data and selects the optimal decompression codec
/// to maximize compression ratio while maintaining read speed (800+ MB/s).
///
/// Strategy:
/// 1. Profile column: Count unique values, detect distribution, measure patterns
/// 2. Select codec: RLE for runs, Dictionary for low-cardinality, FOR for numeric, LZSS fallback
/// 3. Validate: Measure actual compression ratio and speed trade-off
/// 4. Report: Provide selection reasoning and metrics

use std::collections::HashSet;
use crate::decompression::CodecId;

/// Column data characteristics for codec selection
#[derive(Clone, Debug)]
pub struct ColumnProfile {
    /// Total number of values in column
    pub total_values: u64,
    /// Number of unique values
    pub unique_values: usize,
    /// Ratio: unique_values / total_values (0.0 to 1.0)
    pub cardinality_ratio: f32,
    /// Detected data distribution pattern
    pub distribution: DataDistribution,
    /// Maximum run length (consecutive identical values)
    pub max_run_length: usize,
    /// Average run length
    pub avg_run_length: f32,
    /// Whether data appears numeric (all values are numbers)
    pub is_numeric: bool,
    /// Numeric range if applicable (max - min)
    pub numeric_range: Option<u64>,
    /// Sample size used for analysis (actual profile may be based on subset)
    pub sample_size: usize,
}

/// Detected data distribution pattern
#[derive(Clone, Debug, PartialEq)]
pub enum DataDistribution {
    /// >90% of values are identical or in very short runs (RLE optimal)
    HighlyRepetitive,
    /// <10% unique values - low cardinality categorical (Dictionary optimal)
    LowCardinality,
    /// 10-100 unique values - categorical but more diverse (Dictionary still good)
    Categorical,
    /// All or nearly all unique values (LZSS fallback)
    HighCardinality,
    /// Numeric data with tight range (FOR optimal)
    NumericRange,
    /// Mixed/unstructured data (LZSS fallback)
    Mixed,
}

impl ColumnProfile {
    /// Analyze column data and create a profile for codec selection
    ///
    /// Samples the data to detect:
    /// - Unique value count
    /// - Run length patterns
    /// - Numeric range
    /// - Cardinality distribution
    pub fn analyze(data: &[u8]) -> Result<Self, String> {
        if data.is_empty() {
            return Ok(Self {
                total_values: 0,
                unique_values: 0,
                cardinality_ratio: 0.0,
                distribution: DataDistribution::HighlyRepetitive,
                max_run_length: 0,
                avg_run_length: 0.0,
                is_numeric: false,
                numeric_range: None,
                sample_size: 0,
            });
        }

        // For now, treat each byte as a value (simplified)
        // In practice, would need to respect actual value boundaries
        let sample_size = data.len();
        let mut unique_set = HashSet::new();
        let mut max_run = 0;
        let mut current_run = 1;
        let mut total_runs = 0;
        let mut sum_run_lengths = 0;

        // Analyze runs and unique values
        for i in 0..data.len() {
            unique_set.insert(data[i]);

            if i > 0 && data[i] == data[i - 1] {
                current_run += 1;
            } else {
                if current_run > max_run {
                    max_run = current_run;
                }
                sum_run_lengths += current_run;
                total_runs += 1;
                current_run = 1;
            }
        }

        // Don't forget last run
        sum_run_lengths += current_run;
        total_runs += 1;
        if current_run > max_run {
            max_run = current_run;
        }

        let unique_count = unique_set.len();
        let cardinality_ratio = unique_count as f32 / data.len() as f32;
        let avg_run_length = sum_run_lengths as f32 / total_runs as f32;

        // Detect numeric data (all values are ASCII digits or common separators)
        let is_numeric = data.iter().all(|&b| b.is_ascii_digit() || b == b'.' || b == b'-' || b == b'+');

        // Determine distribution pattern
        let distribution = Self::classify_distribution(
            cardinality_ratio,
            max_run,
            data.len(),
            is_numeric,
        );

        Ok(Self {
            total_values: data.len() as u64,
            unique_values: unique_count,
            cardinality_ratio,
            distribution,
            max_run_length: max_run,
            avg_run_length,
            is_numeric,
            numeric_range: if is_numeric { Some(256) } else { None }, // Simplified
            sample_size,
        })
    }

    /// Classify data distribution based on characteristics
    fn classify_distribution(
        cardinality_ratio: f32,
        max_run: usize,
        data_size: usize,
        is_numeric: bool,
    ) -> DataDistribution {
        // Highly repetitive: max run is >50% of data
        if max_run as f32 > data_size as f32 * 0.5 {
            return DataDistribution::HighlyRepetitive;
        }

        // Numeric range: numeric data with reasonable run lengths
        if is_numeric && max_run > 10 {
            return DataDistribution::NumericRange;
        }

        // Very low cardinality: <1% unique
        if cardinality_ratio < 0.01 {
            return DataDistribution::LowCardinality;
        }

        // Low cardinality: <10% unique (includes 10%)
        if cardinality_ratio <= 0.1 {
            return DataDistribution::Categorical;
        }

        // High cardinality: >50% unique
        if cardinality_ratio > 0.5 {
            return DataDistribution::HighCardinality;
        }

        // Default: mixed
        DataDistribution::Mixed
    }
}

/// Compression statistics for a codec selection
#[derive(Clone, Debug)]
pub struct CompressionStats {
    /// Which codec this represents
    pub codec: CodecId,
    /// Original uncompressed size
    pub original_size: usize,
    /// Compressed size after applying codec
    pub compressed_size: usize,
    /// Compression ratio: compressed / original
    pub ratio: f32,
    /// Estimated speed in MB/s
    pub speed_mb_per_sec: f32,
    /// Quality score (higher is better)
    pub score: f32,
}

/// Main codec selector
pub struct CodecSelector;

impl CodecSelector {
    /// Select best codec for a column based on profile
    ///
    /// Decision tree:
    /// 1. HighlyRepetitive → RLE (fast, excellent ratio on runs)
    /// 2. NumericRange → FOR (fastest, best for numeric)
    /// 3. LowCardinality/Categorical → Dictionary (good ratio, moderate speed)
    /// 4. Everything else → LZSS (general purpose fallback)
    pub fn select_codec(profile: &ColumnProfile) -> CodecId {
        match profile.distribution {
            DataDistribution::HighlyRepetitive => CodecId::RLE,
            DataDistribution::NumericRange => CodecId::FOR,
            DataDistribution::LowCardinality => CodecId::Dictionary,
            DataDistribution::Categorical => CodecId::Dictionary,
            DataDistribution::HighCardinality => CodecId::LZSS,
            DataDistribution::Mixed => CodecId::LZSS,
        }
    }

    /// Estimate compression stats without actually compressing
    ///
    /// Based on profile characteristics and known codec performance
    pub fn estimate_stats(profile: &ColumnProfile, codec: CodecId) -> CompressionStats {
        let (estimated_ratio, speed) = match codec {
            CodecId::RLE => {
                // RLE: excellent on repetitive data
                if profile.max_run_length > 100 {
                    (0.1, 1000.0) // Very good compression on high runs
                } else if profile.max_run_length > 10 {
                    (0.3, 1000.0) // Good compression on moderate runs
                } else {
                    (0.8, 1000.0) // Poor compression, low runs
                }
            }
            CodecId::Dictionary => {
                // Dictionary: depends on cardinality
                if profile.cardinality_ratio <= 0.01 {
                    (0.15, 500.0) // Excellent: very few unique values (<1%)
                } else if profile.cardinality_ratio <= 0.1 {
                    (0.35, 500.0) // Good: low cardinality (1-10%)
                } else {
                    (0.7, 500.0) // Moderate: higher cardinality
                }
            }
            CodecId::FOR => {
                // FOR: best for numeric ranges
                if profile.is_numeric {
                    (0.25, 2000.0) // Excellent on numeric data
                } else {
                    (0.8, 2000.0) // Poor on non-numeric
                }
            }
            CodecId::LZSS => {
                // LZSS: general purpose
                if profile.cardinality_ratio < 0.3 {
                    (0.5, 800.0) // Some pattern
                } else {
                    (0.7, 800.0) // High entropy, limited compression
                }
            }
            CodecId::None => (1.0, 0.0), // No compression
        };

        let original_size = profile.sample_size;
        let compressed_size = (original_size as f32 * estimated_ratio) as usize;
        let ratio = estimated_ratio;

        // Score: balance compression ratio with speed
        // Higher ratio (better compression) + higher speed = higher score
        let score = (1.0 - ratio) * 100.0 + (speed / 100.0);

        CompressionStats {
            codec,
            original_size,
            compressed_size,
            ratio,
            speed_mb_per_sec: speed,
            score,
        }
    }

    /// Select best codec considering both compression and speed
    pub fn select_optimal_codec(profile: &ColumnProfile) -> CodecId {
        let candidates = vec![
            CodecId::RLE,
            CodecId::Dictionary,
            CodecId::FOR,
        ];

        let mut best_codec = CodecId::RLE;
        let mut best_score = 0.0;

        for codec in candidates {
            let stats = Self::estimate_stats(profile, codec);
            
            if stats.ratio < 0.8 && stats.score > best_score {
                best_score = stats.score;
                best_codec = codec;
            }
        }

        best_codec
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_repetitive_data() {
        // All same value: should detect HighlyRepetitive
        let data = vec![0xAA; 1000];
        let profile = ColumnProfile::analyze(&data).unwrap();
        
        assert_eq!(profile.unique_values, 1);
        assert_eq!(profile.cardinality_ratio, 0.001);
        assert_eq!(profile.max_run_length, 1000);
        assert_eq!(profile.distribution, DataDistribution::HighlyRepetitive);
    }

    #[test]
    fn test_analyze_low_cardinality() {
        // 10 unique values, 100 total: <10% cardinality
        let mut data = Vec::new();
        for i in 0..100 {
            data.push((i % 10) as u8);
        }
        let profile = ColumnProfile::analyze(&data).unwrap();
        
        assert_eq!(profile.unique_values, 10);
        assert!(profile.cardinality_ratio > 0.09 && profile.cardinality_ratio < 0.11);
        assert_eq!(profile.distribution, DataDistribution::Categorical);
    }

    #[test]
    fn test_analyze_high_cardinality() {
        // All unique values: >50% cardinality
        let data: Vec<u8> = (0..100).map(|i| (i % 256) as u8).collect();
        let profile = ColumnProfile::analyze(&data).unwrap();
        
        assert!(profile.cardinality_ratio > 0.3);
        assert_eq!(profile.distribution, DataDistribution::HighCardinality);
    }

    #[test]
    fn test_select_codec_repetitive() {
        let profile = ColumnProfile {
            total_values: 1000,
            unique_values: 1,
            cardinality_ratio: 0.001,
            distribution: DataDistribution::HighlyRepetitive,
            max_run_length: 1000,
            avg_run_length: 1000.0,
            is_numeric: false,
            numeric_range: None,
            sample_size: 1000,
        };

        let selected = CodecSelector::select_codec(&profile);
        assert_eq!(selected, CodecId::RLE);
    }

    #[test]
    fn test_select_codec_numeric() {
        let profile = ColumnProfile {
            total_values: 1000,
            unique_values: 100,
            cardinality_ratio: 0.1,
            distribution: DataDistribution::NumericRange,
            max_run_length: 50,
            avg_run_length: 10.0,
            is_numeric: true,
            numeric_range: Some(1000),
            sample_size: 1000,
        };

        let selected = CodecSelector::select_codec(&profile);
        assert_eq!(selected, CodecId::FOR);
    }

    #[test]
    fn test_select_codec_categorical() {
        let profile = ColumnProfile {
            total_values: 1000,
            unique_values: 25,
            cardinality_ratio: 0.025,
            distribution: DataDistribution::Categorical,
            max_run_length: 20,
            avg_run_length: 5.0,
            is_numeric: false,
            numeric_range: None,
            sample_size: 1000,
        };

        let selected = CodecSelector::select_codec(&profile);
        assert_eq!(selected, CodecId::Dictionary);
    }

    #[test]
    fn test_compression_stats_rle() {
        let profile = ColumnProfile {
            total_values: 1000,
            unique_values: 1,
            cardinality_ratio: 0.001,
            distribution: DataDistribution::HighlyRepetitive,
            max_run_length: 1000,
            avg_run_length: 1000.0,
            is_numeric: false,
            numeric_range: None,
            sample_size: 1000,
        };

        let stats = CodecSelector::estimate_stats(&profile, CodecId::RLE);
        
        assert_eq!(stats.codec, CodecId::RLE);
        assert!(stats.ratio < 0.2); // Should be very good compression
        assert_eq!(stats.speed_mb_per_sec, 1000.0);
    }

    #[test]
    fn test_estimate_stats_dictionary_low_cardinality() {
        let profile = ColumnProfile {
            total_values: 1000,
            unique_values: 10,
            cardinality_ratio: 0.01,
            distribution: DataDistribution::LowCardinality,
            max_run_length: 100,
            avg_run_length: 50.0,
            is_numeric: false,
            numeric_range: None,
            sample_size: 1000,
        };

        let stats = CodecSelector::estimate_stats(&profile, CodecId::Dictionary);
        
        assert!(stats.ratio < 0.3); // Should be good compression
        assert_eq!(stats.speed_mb_per_sec, 500.0);
    }

    #[test]
    fn test_empty_data() {
        let data: Vec<u8> = vec![];
        let profile = ColumnProfile::analyze(&data).unwrap();
        
        assert_eq!(profile.total_values, 0);
        assert_eq!(profile.unique_values, 0);
        assert_eq!(profile.distribution, DataDistribution::HighlyRepetitive);
    }
}
