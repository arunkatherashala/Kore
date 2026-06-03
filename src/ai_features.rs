/// Phase 4: AI Features for KORE
/// 
/// Features:
/// 1. AI Codec Selector: Learns from data patterns to choose optimal compression
/// 2. Natural Language Parser: Converts user text → SQL queries
/// 3. Pattern-based Intelligence: Adaptive compression based on column characteristics

use std::collections::HashMap;

/// Data patterns recognized by the AI system
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DataPattern {
    LowCardinality,     // Few unique values (good for dictionary compression)
    Monotonic,          // Values consistently increase/decrease (Delta encoding)
    Repetitive,         // Many repeated values (RLE compression)
    Random,             // No discernible pattern (Zstandard)
    TimeSeries,         // Timestamp data (special handling)
    Categorical,        // Category/enum data (dictionary + RLE)
}

/// ML-based codec recommendation
#[derive(Debug, Clone)]
pub struct CodecRecommendation {
    pub codec: String,
    pub confidence: f64,  // 0.0 to 1.0
    pub estimated_ratio: f64,  // Compression ratio estimate
    pub pattern: DataPattern,
}

/// Natural language query intent
#[derive(Debug, Clone)]
pub enum QueryIntent {
    Filter(String),     // "Show me records where..."
    Aggregate(String),  // "Count how many...", "Sum all..."
    GroupAnalysis(String), // "Group by... and count"
    Trend(String),      // "Show trend over time"
    TopN(String),       // "Show top 10..."
    JoinQuery(String),  // "Join table A with table B..."
}

pub struct AICodecSelector;

impl AICodecSelector {
    /// Analyze column data and recommend optimal codec
    pub fn recommend_codec(values: &[String]) -> CodecRecommendation {
        if values.is_empty() {
            return CodecRecommendation {
                codec: "Raw".to_string(),
                confidence: 0.1,
                estimated_ratio: 1.0,
                pattern: DataPattern::Random,
            };
        }

        // Detect data pattern
        let pattern = Self::detect_pattern(values);

        // Recommend codec based on pattern
        let (codec, confidence, ratio) = match pattern {
            DataPattern::LowCardinality => {
                let unique_count = Self::count_unique(values);
                let cardinality_ratio = unique_count as f64 / values.len() as f64;
                
                if cardinality_ratio < 0.01 {
                    ("DictRLE".to_string(), 0.95, 0.15)  // Dictionary + Run-Length Encoding
                } else {
                    ("Dict".to_string(), 0.90, 0.30)     // Dictionary compression
                }
            }
            DataPattern::Monotonic => {
                ("Delta".to_string(), 0.92, 0.25)        // Delta encoding (diffs are small)
            }
            DataPattern::Repetitive => {
                ("RLE".to_string(), 0.88, 0.20)          // Run-Length Encoding
            }
            DataPattern::TimeSeries => {
                ("CDelta".to_string(), 0.90, 0.22)       // Combined Delta for timestamps
            }
            DataPattern::Categorical => {
                ("DictRLE".to_string(), 0.85, 0.18)      // Dictionary + RLE
            }
            DataPattern::Random => {
                ("Zstd".to_string(), 0.70, 0.45)         // Zstandard compression
            }
        };

        CodecRecommendation {
            codec,
            confidence,
            estimated_ratio: ratio,
            pattern,
        }
    }

    /// Detect the data pattern in a column
    fn detect_pattern(values: &[String]) -> DataPattern {
        let unique_count = Self::count_unique(values);
        let cardinality = unique_count as f64 / values.len() as f64;

        // Check for monotonic pattern
        if Self::is_monotonic(values) {
            return DataPattern::Monotonic;
        }

        // Check for time series (ISO timestamps, epoch times)
        if Self::is_timestamp_series(values) {
            return DataPattern::TimeSeries;
        }

        // Check for low cardinality
        if cardinality < 0.05 {
            // Check for RLE opportunities
            if Self::has_high_repetition(values) {
                return DataPattern::Repetitive;
            }
            return DataPattern::LowCardinality;
        }

        // Check for categorical (strings that look like categories)
        if Self::is_categorical(values) {
            return DataPattern::Categorical;
        }

        // Default to random
        DataPattern::Random
    }

    fn count_unique(values: &[String]) -> usize {
        values.iter().collect::<std::collections::HashSet<_>>().len()
    }

    fn is_monotonic(values: &[String]) -> bool {
        if values.len() < 3 {
            return false;
        }

        // Try numeric comparison
        let nums: Vec<f64> = values
            .iter()
            .filter_map(|v| v.parse::<f64>().ok())
            .collect();

        if nums.len() < 3 {
            return false;
        }

        // Check if strictly increasing or decreasing
        let mut increasing = true;
        let mut decreasing = true;

        for i in 1..nums.len() {
            if nums[i] <= nums[i - 1] {
                increasing = false;
            }
            if nums[i] >= nums[i - 1] {
                decreasing = false;
            }
        }

        increasing || decreasing
    }

    fn is_timestamp_series(values: &[String]) -> bool {
        // Check for ISO 8601 format
        if values.iter().filter(|v| v.contains('T')).count() > values.len() / 2 {
            return true;
        }

        // Check for Unix epoch (large numbers)
        let large_nums = values
            .iter()
            .filter_map(|v| v.parse::<i64>().ok())
            .filter(|&v| v > 1000000000) // After 2001 in Unix time
            .count();

        large_nums > values.len() / 2
    }

    fn has_high_repetition(values: &[String]) -> bool {
        let mut counts = HashMap::new();
        for val in values {
            *counts.entry(val.clone()).or_insert(0) += 1;
        }

        // If top value appears >20% of time, high repetition
        counts.values().max().map_or(false, |&max| max > values.len() / 5)
    }

    fn is_categorical(values: &[String]) -> bool {
        // Look for patterns like enum values, codes, short strings
        let avg_len = values.iter().map(|v| v.len()).sum::<usize>() / values.len();
        avg_len < 20 && values.iter().all(|v| !v.contains(' '))
    }

    /// Learn from actual compression results to improve future recommendations
    pub fn update_from_compression_result(
        pattern: DataPattern,
        codec: &str,
        actual_ratio: f64,
    ) {
        // In a real implementation, this would update a machine learning model
        // For now, we log the observation for offline training
        eprintln!(
            "Compression observation: pattern={:?}, codec={}, ratio={}",
            pattern, codec, actual_ratio
        );
    }
}

pub struct NaturalLanguageParser;

impl NaturalLanguageParser {
    /// Parse user's natural language query into structured intent
    pub fn parse(query: &str) -> Option<QueryIntent> {
        let lower = query.to_lowercase();

        // Filter queries
        if lower.contains("where") || lower.contains("filter") || lower.contains("show me") {
            if let Some(condition) = Self::extract_condition(&lower) {
                return Some(QueryIntent::Filter(condition));
            }
        }

        // Aggregate queries
        if lower.contains("count") && lower.contains("how many") {
            return Some(QueryIntent::Aggregate("COUNT(*)".to_string()));
        }
        if lower.contains("sum") || lower.contains("total") {
            if let Some(col) = Self::extract_column(&lower) {
                return Some(QueryIntent::Aggregate(format!("SUM({})", col)));
            }
        }
        if lower.contains("average") || lower.contains("avg") {
            if let Some(col) = Self::extract_column(&lower) {
                return Some(QueryIntent::Aggregate(format!("AVG({})", col)));
            }
        }

        // Group by queries
        if lower.contains("group by") || (lower.contains("by") && lower.contains("count")) {
            if let Some(col) = Self::extract_group_column(&lower) {
                return Some(QueryIntent::GroupAnalysis(col));
            }
        }

        // Trend queries
        if lower.contains("trend") || lower.contains("over time") {
            return Some(QueryIntent::Trend("time_series".to_string()));
        }

        // Top N queries
        if lower.contains("top") || lower.contains("largest") || lower.contains("smallest") {
            if let Some(n) = Self::extract_number(&lower) {
                return Some(QueryIntent::TopN(format!("LIMIT {}", n)));
            }
        }

        None
    }

    fn extract_condition(query: &str) -> Option<String> {
        // Simple extraction: look for "where" keyword and take rest
        if let Some(idx) = query.find("where") {
            let condition = &query[idx + 5..];
            return Some(condition.trim().to_string());
        }
        None
    }

    fn extract_column(query: &str) -> Option<String> {
        // Look for common column references: "of", "for", "in"
        let keywords = vec!["of ", "for ", "in the "];
        for kw in keywords {
            if let Some(idx) = query.find(kw) {
                let after_kw = &query[idx + kw.len()..];
                let col: String = after_kw
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("")
                    .to_string();
                if !col.is_empty() {
                    return Some(col);
                }
            }
        }
        None
    }

    fn extract_group_column(query: &str) -> Option<String> {
        if let Some(idx) = query.find("by") {
            let after_by = &query[idx + 2..];
            let col: String = after_by
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("")
                .to_string();
            if !col.is_empty() {
                return Some(col);
            }
        }
        None
    }

    fn extract_number(query: &str) -> Option<usize> {
        // Look for numbers in the query
        let words: Vec<&str> = query.split_whitespace().collect();
        for (i, word) in words.iter().enumerate() {
            if let Ok(num) = word.parse::<usize>() {
                return Some(num);
            }
            // Also check for spelled-out numbers
            if i > 0 {
                let prev = words[i - 1];
                if prev == "top" && word.chars().all(|c| c.is_numeric()) {
                    if let Ok(num) = word.parse::<usize>() {
                        return Some(num);
                    }
                }
            }
        }
        Some(10) // Default to top 10
    }

    /// Convert parsed intent into SQL query
    pub fn intent_to_sql(intent: &QueryIntent, table: &str) -> String {
        match intent {
            QueryIntent::Filter(condition) => {
                format!("SELECT * FROM {} WHERE {}", table, condition)
            }
            QueryIntent::Aggregate(func) => {
                format!("SELECT {} FROM {}", func, table)
            }
            QueryIntent::GroupAnalysis(col) => {
                format!("SELECT {}, COUNT(*) as count FROM {} GROUP BY {}", col, table, col)
            }
            QueryIntent::Trend(col) => {
                format!("SELECT {} FROM {} ORDER BY {}", col, table, col)
            }
            QueryIntent::TopN(limit) => {
                format!("SELECT * FROM {} {}", table, limit)
            }
            QueryIntent::JoinQuery(join_spec) => {
                format!("SELECT * FROM {} {}", table, join_spec)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codec_recommendation_low_cardinality() {
        let values = vec!["A".to_string(), "A".to_string(), "B".to_string(), "A".to_string()];
        let rec = AICodecSelector::recommend_codec(&values);
        assert_eq!(rec.pattern, DataPattern::LowCardinality);
    }

    #[test]
    fn test_detect_monotonic() {
        let values = vec!["1", "2", "3", "4", "5"]
            .into_iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let pattern = AICodecSelector::detect_pattern(&values);
        assert_eq!(pattern, DataPattern::Monotonic);
    }

    #[test]
    fn test_natural_language_parsing() {
        let query = "Show me records where age > 30";
        let intent = NaturalLanguageParser::parse(query);
        assert!(intent.is_some());
    }

    #[test]
    fn test_intent_to_sql_filter() {
        let intent = QueryIntent::Filter("age > 30".to_string());
        let sql = NaturalLanguageParser::intent_to_sql(&intent, "users");
        assert!(sql.contains("WHERE"));
    }

    #[test]
    fn test_parse_count_query() {
        let query = "How many records?";
        let intent = NaturalLanguageParser::parse(query);
        assert!(matches!(intent, Some(QueryIntent::Aggregate(_))));
    }
}
