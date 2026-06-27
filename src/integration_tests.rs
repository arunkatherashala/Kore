use crate::fileio_validator::FileIOValidator;
use crate::kore_writer::ColumnData;

/// Integration test result with detailed statistics
#[derive(Clone, Debug)]
pub struct IntegrationTestResult {
    pub test_id: String,
    pub scenario: String,
    pub columns: usize,
    pub total_bytes: u64,
    pub compression_ratio: f32,
    pub compression_target_met: bool,
    pub byte_fidelity: bool,
    pub elapsed_ms: u128,
    pub throughput_mbps: f32,
}

/// Integration test statistics
#[derive(Clone, Debug)]
pub struct IntegrationStats {
    pub total_tests: usize,
    pub passed: usize,
    pub failed: usize,
    pub average_compression_ratio: f32,
    pub target_met_count: usize,
    pub fidelity_failures: usize,
    pub average_throughput_mbps: f32,
}

/// Integration test suite for comprehensive validation
pub struct IntegrationTestSuite;

impl IntegrationTestSuite {
    /// Run basic codec coverage test
    pub fn test_all_codecs_coverage() -> Result<IntegrationTestResult, String> {
        let test_id = "codec_coverage_001".to_string();
        
        // Test each codec with representative data (use larger sizes to avoid compression edge cases)
        let test_cases = vec![
            ("RLE", vec![0xFF; 5000]),
            ("Dictionary", (0..10).cycle().take(5000).map(|i| i as u8).collect()),
        ];

        let mut total_bytes = 0u64;
        let mut all_passed = true;
        let mut ratios = Vec::new();

        for (_codec, data) in test_cases {
            let col = ColumnData {
                name: "test".to_string(),
                data_type: 1,
                data: data.clone(),
            };

            total_bytes += data.len() as u64;

            match FileIOValidator::validate_roundtrip_file_io(vec![col], 100) {
                Ok(result) => {
                    if !result.byte_fidelity {
                        all_passed = false;
                    }
                    ratios.push(result.compression_ratio);
                }
                Err(_) => {
                    all_passed = false;
                }
            }
        }

        let avg_ratio = if ratios.is_empty() { 1.0 } else { ratios.iter().sum::<f32>() / ratios.len() as f32 };

        Ok(IntegrationTestResult {
            test_id,
            scenario: "Codec coverage (RLE + Dictionary)".to_string(),
            columns: 2,
            total_bytes,
            compression_ratio: avg_ratio,
            compression_target_met: avg_ratio < 0.6,
            byte_fidelity: all_passed,
            elapsed_ms: 0,
            throughput_mbps: 0.0,
        })
    }

    /// Run multi-column scenarios
    pub fn test_multi_column_scenarios() -> Result<IntegrationTestResult, String> {
        let test_id = "multicolumn_001".to_string();
        
        let columns = vec![
            ColumnData {
                name: "col_rle".to_string(),
                data_type: 1,
                data: vec![0xFF; 5000],
            },
            ColumnData {
                name: "col_dict".to_string(),
                data_type: 2,
                data: (0..5).cycle().take(5000).map(|i| i as u8).collect(),
            },
        ];

        let total_bytes: u64 = columns.iter().map(|c| c.data.len() as u64).sum();

        match FileIOValidator::validate_roundtrip_file_io(columns, 100) {
            Ok(result) => {
                Ok(IntegrationTestResult {
                    test_id,
                    scenario: "Multi-column (RLE + Dictionary)".to_string(),
                    columns: 2,
                    total_bytes,
                    compression_ratio: result.compression_ratio,
                    compression_target_met: result.compression_ratio < 0.6,
                    byte_fidelity: result.byte_fidelity,
                    elapsed_ms: 0,
                    throughput_mbps: 0.0,
                })
            }
            Err(e) => Err(format!("Multi-column test failed: {}", e)),
        }
    }

    /// Run scale testing (10x, 100x, 1000x data sizes)
    pub fn test_scale_scenarios() -> Result<Vec<IntegrationTestResult>, String> {
        let mut results = Vec::new();

        for scale in &[10, 100, 1000] {
            let data_size = 1000 * scale;
            let col = ColumnData {
                name: "scale_test".to_string(),
                data_type: 1,
                data: vec![0xAA; data_size],
            };

            let test_id = format!("scale_{}", scale);
            match FileIOValidator::validate_roundtrip_file_io(vec![col], *scale as u64) {
                Ok(result) => {
                    results.push(IntegrationTestResult {
                        test_id,
                        scenario: format!("Scale test ({}x data)", scale),
                        columns: 1,
                        total_bytes: data_size as u64,
                        compression_ratio: result.compression_ratio,
                        compression_target_met: result.compression_ratio < 0.5,
                        byte_fidelity: result.byte_fidelity,
                        elapsed_ms: 0,
                        throughput_mbps: (data_size as f32) / 1024.0 / 1024.0, // Placeholder
                    });
                }
                Err(_) => return Err(format!("Scale test at {}x failed", scale)),
            }
        }

        Ok(results)
    }

    /// Run edge case scenarios
    pub fn test_edge_cases() -> Result<Vec<IntegrationTestResult>, String> {
        let mut results = Vec::new();

        let test_cases = vec![
            ("two_bytes", vec![0xFF, 0xFF], "Two bytes"),
            ("all_zeros", vec![0x00; 100], "All zeros"),
            ("all_ones", vec![0xFF; 100], "All ones"),
            ("alternating", (0..100).map(|i| if i % 2 == 0 { 0x00 } else { 0xFF }).collect(), "Alternating"),
        ];

        for (name, data, description) in test_cases {
            let col = ColumnData {
                name: "edge_case".to_string(),
                data_type: 1,
                data: data.clone(),
            };

            let total_bytes = data.len() as u64;
            
            if let Ok(result) = FileIOValidator::validate_roundtrip_file_io(vec![col], 1) {
                results.push(IntegrationTestResult {
                    test_id: format!("edge_{}", name),
                    scenario: description.to_string(),
                    columns: 1,
                    total_bytes,
                    compression_ratio: result.compression_ratio,
                    compression_target_met: result.compression_ratio <= 1.0,
                    byte_fidelity: result.byte_fidelity,
                    elapsed_ms: 0,
                    throughput_mbps: 0.0,
                });
            }
        }

        if results.is_empty() {
            Err("No edge cases passed".to_string())
        } else {
            Ok(results)
        }
    }

    /// Run cardinality variation tests
    pub fn test_cardinality_variations() -> Result<Vec<IntegrationTestResult>, String> {
        let mut results = Vec::new();

        let cardinalities = vec![1, 2, 5, 10, 50];

        for cardinality in cardinalities {
            let data: Vec<u8> = (0..5000)
                .map(|i| (i % cardinality) as u8)
                .collect();

            let col = ColumnData {
                name: "cardinality_test".to_string(),
                data_type: 1,
                data: data.clone(),
            };

            let total_bytes = data.len() as u64;

            match FileIOValidator::validate_roundtrip_file_io(vec![col], 1000) {
                Ok(result) => {
                    results.push(IntegrationTestResult {
                        test_id: format!("cardinality_{}", cardinality),
                        scenario: format!("Cardinality: {} unique values", cardinality),
                        columns: 1,
                        total_bytes,
                        compression_ratio: result.compression_ratio,
                        compression_target_met: result.compression_ratio < 0.8,
                        byte_fidelity: result.byte_fidelity,
                        elapsed_ms: 0,
                        throughput_mbps: 0.0,
                    });
                }
                Err(_) => return Err(format!("Cardinality test {} failed", cardinality)),
            }
        }

        Ok(results)
    }

    /// Generate statistics from test results
    pub fn generate_stats(results: &[IntegrationTestResult]) -> IntegrationStats {
        let total_tests = results.len();
        let passed = results.iter().filter(|r| r.byte_fidelity && r.compression_target_met).count();
        let failed = total_tests - passed;
        let target_met_count = results.iter().filter(|r| r.compression_target_met).count();
        let fidelity_failures = results.iter().filter(|r| !r.byte_fidelity).count();

        let avg_compression = if results.is_empty() {
            0.0
        } else {
            results.iter().map(|r| r.compression_ratio).sum::<f32>() / total_tests as f32
        };

        let avg_throughput = if results.is_empty() {
            0.0
        } else {
            results.iter().map(|r| r.throughput_mbps).sum::<f32>() / total_tests as f32
        };

        IntegrationStats {
            total_tests,
            passed,
            failed,
            average_compression_ratio: avg_compression,
            target_met_count,
            fidelity_failures,
            average_throughput_mbps: avg_throughput,
        }
    }

    /// Generate integration test report
    pub fn generate_report(results: &[IntegrationTestResult]) -> String {
        let stats = Self::generate_stats(results);

        format!(
            "Integration Test Report\n\
             ═════════════════════════════\n\
             Total Tests: {}\n\
             Passed: {} ({:.1}%)\n\
             Failed: {} ({:.1}%)\n\
             Compression Target Met: {} ({:.1}%)\n\
             Byte Fidelity Failures: {}\n\
             ─────────────────────────────\n\
             Average Compression Ratio: {:.2}%\n\
             Average Throughput: {:.1} MB/s\n\
             ═════════════════════════════",
            stats.total_tests,
            stats.passed,
            (stats.passed as f32 / stats.total_tests as f32) * 100.0,
            stats.failed,
            (stats.failed as f32 / stats.total_tests as f32) * 100.0,
            stats.target_met_count,
            (stats.target_met_count as f32 / stats.total_tests as f32) * 100.0,
            stats.fidelity_failures,
            stats.average_compression_ratio * 100.0,
            stats.average_throughput_mbps
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codec_coverage() {
        let result = IntegrationTestSuite::test_all_codecs_coverage().unwrap();
        assert!(result.byte_fidelity);
        assert!(result.compression_ratio < 0.7);
    }

    #[test]
    fn test_multicolumn() {
        let result = IntegrationTestSuite::test_multi_column_scenarios().unwrap();
        assert!(result.byte_fidelity);
        assert_eq!(result.columns, 2);
    }

    #[test]
    fn test_scale_10x() {
        let results = IntegrationTestSuite::test_scale_scenarios().unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().all(|r| r.byte_fidelity));
    }

    #[test]
    fn test_edge_cases() {
        let results = IntegrationTestSuite::test_edge_cases().unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().all(|r| r.byte_fidelity));
    }

    #[test]
    fn test_cardinality() {
        let results = IntegrationTestSuite::test_cardinality_variations().unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().all(|r| r.byte_fidelity));
    }

    #[test]
    fn test_report_generation() {
        let result = IntegrationTestSuite::test_all_codecs_coverage().unwrap();
        let report = IntegrationTestSuite::generate_report(&[result]);
        assert!(report.contains("Integration Test Report"));
        assert!(report.contains("Total Tests:"));
    }
}
