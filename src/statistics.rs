//! Statistics Module for Phase 3.2
//!
//! Provides column statistics, histograms, and block-level metadata
//! for advanced query optimization.

use std::collections::{HashMap, BTreeMap};

/// Statistics for a single column
#[derive(Debug, Clone)]
pub struct ColumnStatistics {
    pub name: String,
    pub data_type: String,
    pub row_count: u64,
    pub null_count: u64,
    pub distinct_count: u64,
    pub min_value: Option<String>,
    pub max_value: Option<String>,
    pub avg_length: f64,
    pub compression_ratio: f64,
}

impl ColumnStatistics {
    /// Create new column statistics
    pub fn new(
        name: String,
        data_type: String,
        row_count: u64,
        null_count: u64,
        distinct_count: u64,
    ) -> Self {
        ColumnStatistics {
            name,
            data_type,
            row_count,
            null_count,
            distinct_count,
            min_value: None,
            max_value: None,
            avg_length: 0.0,
            compression_ratio: 1.0,
        }
    }

    /// Set min/max values
    pub fn with_range(mut self, min: Option<String>, max: Option<String>) -> Self {
        self.min_value = min;
        self.max_value = max;
        self
    }

    /// Set average length
    pub fn with_avg_length(mut self, len: f64) -> Self {
        self.avg_length = len;
        self
    }

    /// Set compression ratio
    pub fn with_compression_ratio(mut self, ratio: f64) -> Self {
        self.compression_ratio = ratio;
        self
    }

    /// Calculate cardinality (distinct values / total rows)
    pub fn cardinality(&self) -> f64 {
        if self.row_count == 0 {
            return 0.0;
        }
        self.distinct_count as f64 / self.row_count as f64
    }

    /// Calculate null percentage
    pub fn null_percent(&self) -> f64 {
        if self.row_count == 0 {
            return 0.0;
        }
        (self.null_count as f64 / self.row_count as f64) * 100.0
    }

    /// Estimate bytes used (uncompressed)
    pub fn estimated_size_bytes(&self) -> u64 {
        if self.data_type.contains("Int64") || self.data_type.contains("Float64") {
            self.row_count * 8 // 8 bytes per value
        } else if self.data_type.contains("Int32") || self.data_type.contains("Float32") {
            self.row_count * 4
        } else if self.data_type.contains("String") {
            (self.row_count as f64 * self.avg_length) as u64
        } else if self.data_type.contains("Boolean") {
            (self.row_count + 7) / 8 // 1 bit per value
        } else {
            (self.row_count as f64 * self.avg_length) as u64
        }
    }

    /// Estimate compressed size
    pub fn estimated_compressed_size(&self) -> u64 {
        (self.estimated_size_bytes() as f64 * self.compression_ratio) as u64
    }

    /// Is this column good for filtering (low cardinality)
    pub fn is_good_filter_column(&self) -> bool {
        self.cardinality() < 0.1 // Less than 10% distinct values
    }

    /// Is this column sparse (mostly nulls)
    pub fn is_sparse(&self) -> bool {
        self.null_percent() > 50.0
    }
}

/// Histogram bucket for data distribution
#[derive(Debug, Clone)]
pub struct HistogramBucket {
    pub lower_bound: String,
    pub upper_bound: String,
    pub count: u64,
    pub frequency: f64,
}

impl HistogramBucket {
    /// Create new histogram bucket
    pub fn new(lower: String, upper: String, count: u64) -> Self {
        HistogramBucket {
            lower_bound: lower,
            upper_bound: upper,
            count,
            frequency: 0.0,
        }
    }

    /// Set frequency (percentage)
    pub fn with_frequency(mut self, freq: f64) -> Self {
        self.frequency = freq;
        self
    }
}

/// Histogram for column value distribution
#[derive(Debug, Clone)]
pub struct Histogram {
    pub column_name: String,
    pub buckets: Vec<HistogramBucket>,
    pub total_count: u64,
}

impl Histogram {
    /// Create new histogram
    pub fn new(column_name: String) -> Self {
        Histogram {
            column_name,
            buckets: Vec::new(),
            total_count: 0,
        }
    }

    /// Add a bucket to histogram
    pub fn add_bucket(&mut self, bucket: HistogramBucket) {
        self.total_count += bucket.count;
        self.buckets.push(bucket);
    }

    /// Calculate frequencies for all buckets
    pub fn calculate_frequencies(&mut self) {
        if self.total_count == 0 {
            return;
        }
        for bucket in &mut self.buckets {
            bucket.frequency = (bucket.count as f64 / self.total_count as f64) * 100.0;
        }
    }

    /// Get bucket for a value (string representation)
    pub fn find_bucket(&self, value: &str) -> Option<&HistogramBucket> {
        self.buckets.iter().find(|b| {
            value >= b.lower_bound.as_str() && value <= b.upper_bound.as_str()
        })
    }

    /// Estimate selectivity for range predicate
    pub fn estimate_selectivity(&self, lower: &str, upper: &str) -> f64 {
        let mut count = 0u64;
        for bucket in &self.buckets {
            if bucket.lower_bound.as_str() >= lower && bucket.upper_bound.as_str() <= upper {
                count += bucket.count;
            } else if bucket.lower_bound.as_str() < upper && bucket.upper_bound.as_str() > lower {
                // Partial overlap - estimate
                count += (bucket.count as f64 * 0.5) as u64;
            }
        }
        if self.total_count == 0 {
            return 1.0;
        }
        count as f64 / self.total_count as f64
    }
}

/// Block-level metadata
#[derive(Debug, Clone)]
pub struct BlockMetadata {
    pub block_id: u32,
    pub column_name: String,
    pub row_count: u64,
    pub min_value: Option<String>,
    pub max_value: Option<String>,
    pub null_count: u64,
    pub size_bytes: u64,
    pub compressed_size_bytes: u64,
}

impl BlockMetadata {
    /// Create new block metadata
    pub fn new(
        block_id: u32,
        column_name: String,
        row_count: u64,
        size_bytes: u64,
    ) -> Self {
        BlockMetadata {
            block_id,
            column_name,
            row_count,
            min_value: None,
            max_value: None,
            null_count: 0,
            size_bytes,
            compressed_size_bytes: size_bytes,
        }
    }

    /// Set min/max values
    pub fn with_range(mut self, min: Option<String>, max: Option<String>) -> Self {
        self.min_value = min;
        self.max_value = max;
        self
    }

    /// Set null count
    pub fn with_null_count(mut self, count: u64) -> Self {
        self.null_count = count;
        self
    }

    /// Set compressed size
    pub fn with_compressed_size(mut self, size: u64) -> Self {
        self.compressed_size_bytes = size;
        self
    }

    /// Can this block be skipped based on predicate?
    pub fn can_skip_for_range(&self, lower: &str, upper: &str) -> bool {
        match (&self.min_value, &self.max_value) {
            (Some(min), Some(max)) => {
                // Block is entirely outside range
                max.as_str() < lower || min.as_str() > upper
            }
            _ => false, // Can't skip if we don't have min/max
        }
    }

    /// Compression ratio
    pub fn compression_ratio(&self) -> f64 {
        if self.size_bytes == 0 {
            return 1.0;
        }
        self.compressed_size_bytes as f64 / self.size_bytes as f64
    }
}

/// Table statistics collection
#[derive(Debug, Clone)]
pub struct TableStatistics {
    pub table_name: String,
    pub row_count: u64,
    pub column_stats: HashMap<String, ColumnStatistics>,
    pub histograms: HashMap<String, Histogram>,
    pub blocks: Vec<BlockMetadata>,
}

impl TableStatistics {
    /// Create new table statistics
    pub fn new(table_name: String, row_count: u64) -> Self {
        TableStatistics {
            table_name,
            row_count,
            column_stats: HashMap::new(),
            histograms: HashMap::new(),
            blocks: Vec::new(),
        }
    }

    /// Add column statistics
    pub fn add_column_stats(&mut self, stats: ColumnStatistics) {
        self.column_stats.insert(stats.name.clone(), stats);
    }

    /// Add histogram
    pub fn add_histogram(&mut self, histogram: Histogram) {
        self.histograms.insert(histogram.column_name.clone(), histogram);
    }

    /// Add block metadata
    pub fn add_block(&mut self, block: BlockMetadata) {
        self.blocks.push(block);
    }

    /// Get blocks that can be skipped for range predicate
    pub fn get_skippable_blocks(&self, column: &str, lower: &str, upper: &str) -> Vec<u32> {
        self.blocks
            .iter()
            .filter(|b| b.column_name == column && b.can_skip_for_range(lower, upper))
            .map(|b| b.block_id)
            .collect()
    }

    /// Estimate selectivity using histogram
    pub fn estimate_selectivity(&self, column: &str, lower: &str, upper: &str) -> Option<f64> {
        self.histograms.get(column).map(|h| h.estimate_selectivity(lower, upper))
    }

    /// Get total size of all blocks
    pub fn total_block_size(&self) -> u64 {
        self.blocks.iter().map(|b| b.size_bytes).sum()
    }

    /// Get total compressed size
    pub fn total_compressed_size(&self) -> u64 {
        self.blocks.iter().map(|b| b.compressed_size_bytes).sum()
    }

    /// Average compression ratio
    pub fn avg_compression_ratio(&self) -> f64 {
        if self.blocks.is_empty() {
            return 1.0;
        }
        let total_ratio: f64 = self.blocks.iter().map(|b| b.compression_ratio()).sum();
        total_ratio / self.blocks.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_column_statistics_cardinality() {
        let stats = ColumnStatistics::new(
            "age".to_string(),
            "Int64".to_string(),
            1000,
            10,
            100,
        );
        assert_eq!(stats.cardinality(), 0.1);
    }

    #[test]
    fn test_column_statistics_null_percent() {
        let stats = ColumnStatistics::new(
            "email".to_string(),
            "String".to_string(),
            1000,
            100,
            950,
        );
        assert_eq!(stats.null_percent(), 10.0);
    }

    #[test]
    fn test_column_statistics_is_good_filter() {
        let stats = ColumnStatistics::new(
            "status".to_string(),
            "String".to_string(),
            1000,
            0,
            5, // 5 distinct values (0.5% cardinality)
        );
        assert!(stats.is_good_filter_column());
    }

    #[test]
    fn test_column_statistics_is_sparse() {
        let stats = ColumnStatistics::new(
            "rare_field".to_string(),
            "String".to_string(),
            1000,
            600, // 60% nulls
            400,
        );
        assert!(stats.is_sparse());
    }

    #[test]
    fn test_column_statistics_estimated_size_int64() {
        let stats = ColumnStatistics::new(
            "id".to_string(),
            "Int64".to_string(),
            1000,
            0,
            1000,
        );
        assert_eq!(stats.estimated_size_bytes(), 8000); // 1000 * 8
    }

    #[test]
    fn test_histogram_create_and_add_buckets() {
        let mut hist = Histogram::new("age".to_string());
        hist.add_bucket(HistogramBucket::new("000".to_string(), "010".to_string(), 100));
        hist.add_bucket(HistogramBucket::new("011".to_string(), "020".to_string(), 200));
        
        assert_eq!(hist.buckets.len(), 2);
        assert_eq!(hist.total_count, 300);
    }

    #[test]
    fn test_histogram_calculate_frequencies() {
        let mut hist = Histogram::new("age".to_string());
        hist.add_bucket(HistogramBucket::new("000".to_string(), "010".to_string(), 100));
        hist.add_bucket(HistogramBucket::new("011".to_string(), "020".to_string(), 200));
        
        hist.calculate_frequencies();
        
        assert!((hist.buckets[0].frequency - 33.33).abs() < 0.1);
        assert!((hist.buckets[1].frequency - 66.67).abs() < 0.1);
    }

    #[test]
    fn test_histogram_find_bucket() {
        let mut hist = Histogram::new("age".to_string());
        hist.add_bucket(HistogramBucket::new("000".to_string(), "010".to_string(), 100));
        hist.add_bucket(HistogramBucket::new("011".to_string(), "020".to_string(), 200));
        
        let bucket = hist.find_bucket("005");
        assert!(bucket.is_some());
        assert_eq!(bucket.unwrap().count, 100);
    }

    #[test]
    fn test_histogram_estimate_selectivity() {
        let mut hist = Histogram::new("age".to_string());
        hist.add_bucket(HistogramBucket::new("000".to_string(), "010".to_string(), 100));
        hist.add_bucket(HistogramBucket::new("011".to_string(), "020".to_string(), 200));
        hist.add_bucket(HistogramBucket::new("021".to_string(), "030".to_string(), 300));
        
        let selectivity = hist.estimate_selectivity("005", "025");
        assert!(selectivity > 0.3 && selectivity < 0.8);
    }

    #[test]
    fn test_block_metadata_can_skip_for_range() {
        let block = BlockMetadata::new(0, "age".to_string(), 1000, 8000)
            .with_range(Some("000".to_string()), Some("050".to_string()));
        
        assert!(!block.can_skip_for_range("025", "075")); // Overlap
        assert!(block.can_skip_for_range("100", "200")); // Outside
        assert!(block.can_skip_for_range("-100", "-50")); // Before
    }

    #[test]
    fn test_block_metadata_compression_ratio() {
        let block = BlockMetadata::new(0, "data".to_string(), 1000, 8000)
            .with_compressed_size(2000);
        
        assert_eq!(block.compression_ratio(), 0.25);
    }

    #[test]
    fn test_table_statistics_create() {
        let stats = TableStatistics::new("users".to_string(), 10000);
        assert_eq!(stats.table_name, "users");
        assert_eq!(stats.row_count, 10000);
    }

    #[test]
    fn test_table_statistics_add_column_stats() {
        let mut table = TableStatistics::new("users".to_string(), 10000);
        let col_stats = ColumnStatistics::new("id".to_string(), "Int64".to_string(), 10000, 0, 10000);
        table.add_column_stats(col_stats);
        
        assert_eq!(table.column_stats.len(), 1);
        assert!(table.column_stats.contains_key("id"));
    }

    #[test]
    fn test_table_statistics_get_skippable_blocks() {
        let mut table = TableStatistics::new("data".to_string(), 10000);
        
        let block1 = BlockMetadata::new(0, "age".to_string(), 1000, 8000)
            .with_range(Some("000".to_string()), Some("050".to_string()));
        let block2 = BlockMetadata::new(1, "age".to_string(), 1000, 8000)
            .with_range(Some("100".to_string()), Some("150".to_string()));
        
        table.add_block(block1);
        table.add_block(block2);
        
        let skippable = table.get_skippable_blocks("age", "200", "250");
        assert_eq!(skippable.len(), 2); // Both blocks can be skipped
    }

    #[test]
    fn test_table_statistics_total_block_size() {
        let mut table = TableStatistics::new("data".to_string(), 10000);
        table.add_block(BlockMetadata::new(0, "col".to_string(), 1000, 8000));
        table.add_block(BlockMetadata::new(1, "col".to_string(), 1000, 12000));
        
        assert_eq!(table.total_block_size(), 20000);
    }

    #[test]
    fn test_table_statistics_avg_compression_ratio() {
        let mut table = TableStatistics::new("data".to_string(), 10000);
        let block1 = BlockMetadata::new(0, "col".to_string(), 1000, 8000)
            .with_compressed_size(4000);
        let block2 = BlockMetadata::new(1, "col".to_string(), 1000, 8000)
            .with_compressed_size(2000);
        
        table.add_block(block1);
        table.add_block(block2);
        
        let avg = table.avg_compression_ratio();
        assert!((avg - 0.375).abs() < 0.01); // (0.5 + 0.25) / 2
    }
}

