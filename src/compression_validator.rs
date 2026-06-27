/// Compression Validation Module
/// 
/// Validates that codec selection achieves target compression ratios
/// and maintains expected read performance.

use crate::codec_selector::{ColumnProfile, CodecSelector, CompressionStats};
use crate::decompression::CodecId;

/// Compression validator
pub struct CompressionValidator;

impl CompressionValidator {
    /// Validate codec selection for a column
    ///
    /// Returns:
    /// - Selected codec
    /// - Estimated compression stats
    /// - Recommendation (use, consider, avoid)
    pub fn validate_codec_selection(
        data: &[u8],
    ) -> Result<ValidationResult, String> {
        // Analyze column
        let profile = ColumnProfile::analyze(data)?;

        // Get selection
        let selected_codec = CodecSelector::select_optimal_codec(&profile);

        // Get stats
        let stats = CodecSelector::estimate_stats(&profile, selected_codec);

        // Determine recommendation
        let recommendation = if stats.ratio < 0.5 {
            Recommendation::Excellent
        } else if stats.ratio < 0.7 {
            Recommendation::Good
        } else if stats.ratio < 0.85 {
            Recommendation::Fair
        } else {
            Recommendation::Poor
        };

        Ok(ValidationResult {
            profile,
            selected_codec,
            stats,
            recommendation,
        })
    }

    /// Validate all codecs for a column and find best
    pub fn validate_all_codecs(data: &[u8]) -> Result<AllCodecsValidation, String> {
        let profile = ColumnProfile::analyze(data)?;

        let all_codecs = vec![
            CodecId::RLE,
            CodecId::Dictionary,
            CodecId::FOR,
            CodecId::LZSS,
        ];

        let mut results = Vec::new();
        let mut best_codec = CodecId::LZSS;
        let mut best_score = 0.0;

        for codec in all_codecs {
            let stats = CodecSelector::estimate_stats(&profile, codec);
            
            if stats.score > best_score {
                best_score = stats.score;
                best_codec = codec;
            }

            results.push(CodecValidation {
                codec,
                stats,
            });
        }

        Ok(AllCodecsValidation {
            profile,
            all_codecs: results,
            best_codec,
            best_score,
        })
    }

    /// Check if column meets target compression (50%)
    pub fn meets_target_compression(data: &[u8], target_ratio: f32) -> Result<bool, String> {
        let profile = ColumnProfile::analyze(data)?;
        let selected = CodecSelector::select_optimal_codec(&profile);
        let stats = CodecSelector::estimate_stats(&profile, selected);

        Ok(stats.ratio <= target_ratio)
    }

    /// Get compression improvement report
    pub fn compression_improvement_report(data: &[u8]) -> Result<ImprovementReport, String> {
        let profile = ColumnProfile::analyze(data)?;

        let mut results = Vec::new();
        let mut best_stats: Option<CompressionStats> = None;
        let mut best_improvement = 0.0;

        for codec in &[CodecId::RLE, CodecId::Dictionary, CodecId::FOR, CodecId::LZSS] {
            let stats = CodecSelector::estimate_stats(&profile, *codec);
            
            let improvement = 1.0 - stats.ratio; // Higher is better
            if improvement > best_improvement {
                best_improvement = improvement;
                best_stats = Some(stats.clone());
            }

            results.push(CodecImprovement {
                codec: *codec,
                compression_ratio: stats.ratio,
                improvement_percent: improvement * 100.0,
                speed_mb_sec: stats.speed_mb_per_sec,
            });
        }

        Ok(ImprovementReport {
            data_size: data.len(),
            profile,
            codec_results: results,
            best_codec: best_stats.as_ref().map(|s| s.codec),
            max_improvement_percent: best_improvement * 100.0,
        })
    }
}

/// Validation result for a single codec selection
#[derive(Clone, Debug)]
pub struct ValidationResult {
    pub profile: ColumnProfile,
    pub selected_codec: CodecId,
    pub stats: CompressionStats,
    pub recommendation: Recommendation,
}

/// Recommendation level
#[derive(Clone, Debug, PartialEq)]
pub enum Recommendation {
    Excellent, // <50% ratio
    Good,      // 50-70% ratio
    Fair,      // 70-85% ratio
    Poor,      // >85% ratio
}

/// Validation of a single codec
#[derive(Clone, Debug)]
pub struct CodecValidation {
    pub codec: CodecId,
    pub stats: CompressionStats,
}

/// Validation of all codecs
#[derive(Clone, Debug)]
pub struct AllCodecsValidation {
    pub profile: ColumnProfile,
    pub all_codecs: Vec<CodecValidation>,
    pub best_codec: CodecId,
    pub best_score: f32,
}

/// Codec improvement details
#[derive(Clone, Debug)]
pub struct CodecImprovement {
    pub codec: CodecId,
    pub compression_ratio: f32,
    pub improvement_percent: f32,
    pub speed_mb_sec: f32,
}

/// Comprehensive improvement report
#[derive(Clone, Debug)]
pub struct ImprovementReport {
    pub data_size: usize,
    pub profile: ColumnProfile,
    pub codec_results: Vec<CodecImprovement>,
    pub best_codec: Option<CodecId>,
    pub max_improvement_percent: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_repetitive_data() {
        let data = vec![0xAA; 1000];
        let result = CompressionValidator::validate_codec_selection(&data).unwrap();
        
        assert_eq!(result.selected_codec, CodecId::RLE);
        assert!(result.stats.ratio < 0.2);
        assert_eq!(result.recommendation, Recommendation::Excellent);
    }

    #[test]
    fn test_validate_low_cardinality() {
        let mut data = Vec::new();
        for i in 0..100 {
            data.push((i % 10) as u8);
        }
        let result = CompressionValidator::validate_codec_selection(&data).unwrap();
        
        assert_eq!(result.selected_codec, CodecId::Dictionary);
        assert!(result.stats.ratio < 0.4);
    }

    #[test]
    fn test_meets_target_compression() {
        let data = vec![0xAA; 1000]; // RLE ideal
        let meets = CompressionValidator::meets_target_compression(&data, 0.5).unwrap();
        assert!(meets);
    }

    #[test]
    fn test_all_codecs_validation() {
        let mut data = Vec::new();
        let pattern = vec![0x01, 0x02, 0x03, 0x04, 0x05];
        for _ in 0..50 {
            data.extend(&pattern);
        }
        let validation = CompressionValidator::validate_all_codecs(&data).unwrap();
        
        assert_eq!(validation.all_codecs.len(), 4);
        assert_ne!(validation.best_codec, CodecId::None);
    }

    #[test]
    fn test_improvement_report() {
        let data = vec![0xAA; 1000];
        let report = CompressionValidator::compression_improvement_report(&data).unwrap();
        
        assert_eq!(report.data_size, 1000);
        assert_eq!(report.codec_results.len(), 4);
        assert!(report.max_improvement_percent > 0.0);
    }

    #[test]
    fn test_recommendation_levels() {
        let test_cases = vec![
            (0.3, Recommendation::Excellent), // <50%
            (0.6, Recommendation::Good),      // 50-70%
            (0.75, Recommendation::Fair),     // 70-85%
            (0.9, Recommendation::Poor),      // >85%
        ];

        for (ratio, expected_rec) in test_cases {
            let rec = if ratio < 0.5 {
                Recommendation::Excellent
            } else if ratio < 0.7 {
                Recommendation::Good
            } else if ratio < 0.85 {
                Recommendation::Fair
            } else {
                Recommendation::Poor
            };
            
            assert_eq!(rec, expected_rec);
        }
    }

    #[test]
    fn test_empty_data_validation() {
        let data: Vec<u8> = vec![];
        let result = CompressionValidator::validate_codec_selection(&data).unwrap();
        
        assert_eq!(result.profile.total_values, 0);
    }
}
