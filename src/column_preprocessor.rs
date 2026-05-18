/// Column-specific preprocessing for v1.1.6
/// Analyzes column data type and applies optimal transformations before compression
/// Target: 20-30% compression improvement across all data types

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColumnDataType {
    Integer,
    Float,
    Timestamp,
    String,
    Categorical,
    Binary,
}

#[derive(Debug, Clone)]
pub struct ColumnProfile {
    pub data_type: ColumnDataType,
    pub is_sorted: bool,
    pub is_nullable: bool,
    pub cardinality: usize,
    pub null_count: usize,
    pub is_monotonic: bool,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
}

/// Column preprocessor: applies type-specific transformations
pub struct ColumnPreprocessor;

impl ColumnPreprocessor {
    /// Analyze column and return optimal preprocessing strategy
    pub fn analyze_column(data: &[Option<Vec<u8>>]) -> ColumnProfile {
        let mut null_count = 0;
        let mut values = Vec::new();

        for item in data {
            if item.is_none() {
                null_count += 1;
            } else {
                values.push(item.clone());
            }
        }

        ColumnProfile {
            data_type: Self::detect_data_type(&values),
            is_sorted: Self::is_sorted(&values),
            is_nullable: null_count > 0,
            cardinality: values.iter().collect::<std::collections::HashSet<_>>().len(),
            null_count,
            is_monotonic: Self::is_monotonic_integer(&values),
            min_value: Self::extract_min_float(&values),
            max_value: Self::extract_max_float(&values),
        }
    }

    /// Preprocess numeric column with delta encoding
    pub fn preprocess_numeric(data: &[Option<Vec<u8>>]) -> (Vec<u8>, Vec<usize>) {
        let mut null_bitmap = Vec::new();
        let mut numeric_values = Vec::new();

        for item in data {
            if let Some(bytes) = item {
                null_bitmap.push(1);
                numeric_values.extend_from_slice(bytes);
            } else {
                null_bitmap.push(0);
            }
        }

        // Null bitmap: pack 8 nulls per byte
        let mut null_bytes = Vec::new();
        for chunk in null_bitmap.chunks(8) {
            let mut byte = 0u8;
            for (i, &bit) in chunk.iter().enumerate() {
                if bit == 1 {
                    byte |= 1 << i;
                }
            }
            null_bytes.push(byte);
        }

        (numeric_values, null_bitmap)
    }

    /// Preprocess string column with prefix compression
    pub fn preprocess_string(strings: &[Option<String>]) -> (Vec<u8>, Vec<usize>) {
        let mut result = Vec::new();
        let mut lengths = Vec::new();

        let mut previous = String::new();

        for item in strings {
            if let Some(s) = item {
                // Count common prefix with previous
                let common_len = s
                    .bytes()
                    .zip(previous.bytes())
                    .take_while(|(a, b)| a == b)
                    .count();

                // Store: [prefix_len (1 byte), suffix_len (2 bytes), suffix bytes]
                result.push(common_len as u8);
                let suffix = &s[common_len..];
                result.extend_from_slice(&(suffix.len() as u16).to_le_bytes());
                result.extend_from_slice(suffix.as_bytes());

                lengths.push(s.len());
                previous = s.clone();
            } else {
                result.push(0);
                result.extend_from_slice(&0u16.to_le_bytes());
                lengths.push(0);
            }
        }

        (result, lengths)
    }

    /// Preprocess categorical column (low cardinality strings)
    pub fn preprocess_categorical(values: &[Option<String>]) -> (HashMap<String, u8>, Vec<u8>) {
        let mut dictionary = HashMap::new();
        let mut dict_id = 0u8;
        let mut indices = Vec::new();

        for item in values {
            if let Some(s) = item {
                let id = *dictionary.entry(s.clone()).or_insert_with(|| {
                    let current_id = dict_id;
                    dict_id = dict_id.wrapping_add(1);
                    current_id
                });
                indices.push(id);
            } else {
                indices.push(255); // NULL marker
            }
        }

        (dictionary, indices)
    }

    /// Preprocess timestamp column (Gorilla-style delta encoding)
    pub fn preprocess_timestamp(timestamps: &[Option<i64>]) -> Vec<i64> {
        let mut result = Vec::new();
        let mut previous: Option<i64> = None;

        for &ts in timestamps {
            if let Some(current) = ts {
                if let Some(prev) = previous {
                    result.push(current - prev); // Store delta
                } else {
                    result.push(current); // First value as-is
                }
                previous = Some(current);
            } else {
                result.push(i64::MIN); // NULL marker
            }
        }

        result
    }

    // ============ Detection utilities ============

    fn detect_data_type(values: &[Option<Vec<u8>>]) -> ColumnDataType {
        // Heuristic detection
        if values.is_empty() {
            return ColumnDataType::Binary;
        }

        let sample_size = values.len().min(100);
        let mut all_numeric = true;

        for val in values.iter().take(sample_size).flatten() {
            if val.len() < 8 {
                all_numeric = false;
                break;
            }
        }

        if all_numeric && values.iter().flatten().all(|v| v.len() == 8) {
            ColumnDataType::Integer
        } else if values.iter().flatten().all(|v| v.len() <= 50) {
            ColumnDataType::String
        } else {
            ColumnDataType::Binary
        }
    }

    fn is_sorted(values: &[Option<Vec<u8>>]) -> bool {
        values.windows(2).all(|w| {
            match (&w[0], &w[1]) {
                (Some(a), Some(b)) => a <= b,
                _ => true,
            }
        })
    }

    fn is_monotonic_integer(values: &[Option<Vec<u8>>]) -> bool {
        if values.len() < 2 {
            return true;
        }

        values.windows(2).all(|w| {
            match (&w[0], &w[1]) {
                (Some(a), Some(b)) if a.len() == 8 && b.len() == 8 => {
                    let a_int = i64::from_le_bytes([
                        a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7],
                    ]);
                    let b_int = i64::from_le_bytes([
                        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                    ]);
                    b_int - a_int == 1 || a_int == b_int
                }
                _ => true,
            }
        })
    }

    fn extract_min_float(values: &[Option<Vec<u8>>]) -> Option<f64> {
        values
            .iter()
            .flatten()
            .filter(|v| v.len() == 8)
            .map(|bytes| {
                let mut arr = [0u8; 8];
                arr.copy_from_slice(bytes);
                f64::from_le_bytes(arr)
            })
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    fn extract_max_float(values: &[Option<Vec<u8>>]) -> Option<f64> {
        values
            .iter()
            .flatten()
            .filter(|v| v.len() == 8)
            .map(|bytes| {
                let mut arr = [0u8; 8];
                arr.copy_from_slice(bytes);
                f64::from_le_bytes(arr)
            })
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_categorical_preprocessing() {
        let values = vec![
            Some("red".to_string()),
            Some("blue".to_string()),
            Some("red".to_string()),
            None,
            Some("blue".to_string()),
        ];

        let (dict, indices) = ColumnPreprocessor::preprocess_categorical(&values);
        assert_eq!(dict.len(), 2); // Two unique categories
        assert_eq!(indices.len(), 5);
        assert_eq!(indices[3], 255); // NULL marker
    }

    #[test]
    fn test_string_prefix_compression() {
        let strings = vec![
            Some("prefix_hello_suffix".to_string()),
            Some("prefix_hello_suffix2".to_string()),
            Some("prefix_hello_suffix3".to_string()),
        ];

        let (compressed, lengths) = ColumnPreprocessor::preprocess_string(&strings);
        // Long common prefixes should compress significantly
        // Original: 19 + 20 + 20 = 59 bytes
        // With prefix compression: should be substantially less due to shared prefixes
        assert!(compressed.len() < 50); // Should have some compression benefit
        assert_eq!(lengths, vec![19, 20, 20]);
    }

    #[test]
    fn test_timestamp_delta_encoding() {
        let timestamps = vec![
            Some(1000000),
            Some(1000001),
            Some(1000003),
            Some(1000010),
        ];

        let deltas = ColumnPreprocessor::preprocess_timestamp(&timestamps);
        assert_eq!(deltas[0], 1000000); // First value
        assert_eq!(deltas[1], 1); // Delta: 1000001 - 1000000
        assert_eq!(deltas[2], 2); // Delta: 1000003 - 1000001
        assert_eq!(deltas[3], 7); // Delta: 1000010 - 1000003
    }

    #[test]
    fn test_column_analysis_sorted() {
        let data = vec![
            Some(vec![1, 0, 0, 0, 0, 0, 0, 0]),
            Some(vec![2, 0, 0, 0, 0, 0, 0, 0]),
            Some(vec![3, 0, 0, 0, 0, 0, 0, 0]),
        ];

        let profile = ColumnPreprocessor::analyze_column(&data);
        assert!(profile.is_sorted);
    }
}
