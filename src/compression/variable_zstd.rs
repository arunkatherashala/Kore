// src/compression/variable_zstd.rs
// Adaptive Zstd compression level based on data characteristics

pub struct VariableZstdCompressor;

#[derive(Debug, Clone, Copy)]
pub enum DataProfile {
    Numeric,
    Categorical,
    String,
    Binary,
    Unknown,
}

impl VariableZstdCompressor {
    /// Detect data profile from sample
    pub fn detect_profile(sample: &[&[u8]]) -> DataProfile {
        if sample.is_empty() {
            return DataProfile::Unknown;
        }

        let mut numeric_count = 0;
        let mut categorical_count = 0;
        let mut string_count = 0;

        for item in sample.iter().take(100) {
            if item.len() <= 8 && Self::is_numeric_like(item) {
                numeric_count += 1;
            } else if item.len() <= 32 && Self::is_categorical_like(item) {
                categorical_count += 1;
            } else if Self::is_string_like(item) {
                string_count += 1;
            }
        }

        let max_count = numeric_count.max(categorical_count).max(string_count);

        match max_count {
            _ if max_count == numeric_count => DataProfile::Numeric,
            _ if max_count == categorical_count => DataProfile::Categorical,
            _ if max_count == string_count => DataProfile::String,
            _ => DataProfile::Unknown,
        }
    }

    /// Get optimal compression level for data profile
    pub fn get_compression_level(profile: DataProfile) -> u32 {
        match profile {
            DataProfile::Numeric => 9,      // High compression for numbers
            DataProfile::Categorical => 6,  // Medium for categories
            DataProfile::String => 7,       // Medium-high for strings
            DataProfile::Binary => 3,       // Low for binary (can't compress well)
            DataProfile::Unknown => 5,      // Default middle ground
        }
    }

    /// Check if data looks numeric (bytes that could represent numbers)
    fn is_numeric_like(data: &[u8]) -> bool {
        data.iter().all(|&b| {
            (b >= b'0' && b <= b'9') || b == b'.' || b == b'-' || b == b'e' || b == b'E'
        })
    }

    /// Check if data looks categorical (short, ASCII)
    fn is_categorical_like(data: &[u8]) -> bool {
        data.len() < 32 && data.iter().all(|&b| b >= 32 && b < 127)
    }

    /// Check if data looks like text
    fn is_string_like(data: &[u8]) -> bool {
        let printable_count = data.iter().filter(|&&b| (b >= 32 && b < 127) || b == b'\n').count();
        printable_count as f64 / data.len() as f64 > 0.8
    }

    /// Select compression strategy based on profile
    pub fn select_strategy(profile: DataProfile) -> &'static str {
        match profile {
            DataProfile::Numeric => "high_level_zstd",
            DataProfile::Categorical => "dictionary_then_zstd",
            DataProfile::String => "zstd_with_training",
            DataProfile::Binary => "no_compression_fallback",
            DataProfile::Unknown => "standard_zstd",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numeric_detection() {
        let numeric = vec![b"12345".as_ref(), b"67890".as_ref(), b"123.45".as_ref()];
        let profile = VariableZstdCompressor::detect_profile(&numeric);
        assert!(matches!(profile, DataProfile::Numeric));
    }

    #[test]
    fn test_categorical_detection() {
        let categorical = vec![b"active".as_ref(), b"inactive".as_ref(), b"pending".as_ref()];
        let profile = VariableZstdCompressor::detect_profile(&categorical);
        assert!(matches!(profile, DataProfile::Categorical));
    }

    #[test]
    fn test_compression_levels() {
        assert_eq!(VariableZstdCompressor::get_compression_level(DataProfile::Numeric), 9);
        assert_eq!(VariableZstdCompressor::get_compression_level(DataProfile::Categorical), 6);
        assert_eq!(VariableZstdCompressor::get_compression_level(DataProfile::String), 7);
    }
}
