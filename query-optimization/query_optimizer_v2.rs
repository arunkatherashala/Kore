// Phase 7: Query Optimization for Kore
// Adaptive compression, caching, statistics collection, and indexing

use std::collections::HashMap;

/// Data type for compression codec selection
#[derive(Clone, Debug)]
pub enum CompressionCodec {
    RLE,           // Run-length encoding for low cardinality
    FOR,           // Frame-of-Reference for numerics
    Dictionary,    // Huffman dictionary for repeating strings
    LZSS,          // LZ77 variant for high-entropy data
}

/// Column statistics for cost-based query planning
#[derive(Clone, Debug)]
pub struct ColumnStats {
    pub name: String,
    pub data_type: String,
    pub row_count: u64,
    pub cardinality: u64,  // Number of unique values
    pub null_count: u64,
    pub min_value: Option<String>,
    pub max_value: Option<String>,
    pub compression_ratio: f32,
}

/// Query optimizer for Kore
pub struct QueryOptimizer {
    stats: HashMap<String, ColumnStats>,
}

impl QueryOptimizer {
    pub fn new() -> Self {
        QueryOptimizer {
            stats: HashMap::new(),
        }
    }

    /// Collect statistics from a column
    pub fn collect_column_stats(
        &mut self,
        name: String,
        data_type: String,
        values: &[String],
    ) -> ColumnStats {
        let row_count = values.len() as u64;
        let null_count = values.iter().filter(|v| v == &"NULL").count() as u64;
        
        // Count unique values for cardinality
        let mut unique_values = std::collections::HashSet::new();
        for value in values {
            if value != "NULL" {
                unique_values.insert(value.clone());
            }
        }
        let cardinality = unique_values.len() as u64;
        
        // Find min/max (for numeric types)
        let mut min_value = None;
        let mut max_value = None;
        
        if data_type == "Integer" || data_type == "Float" {
            let non_null: Vec<_> = values.iter()
                .filter(|v| *v != &"NULL")
                .collect();
            
            if !non_null.is_empty() {
                let sorted: Vec<_> = non_null.into_iter()
                    .map(|s| s.as_str())
                    .collect();
                min_value = sorted.first().map(|s| s.to_string());
                max_value = sorted.last().map(|s| s.to_string());
            }
        }
        
        // Estimate compression ratio
        let compression_ratio = Self::estimate_compression_ratio(cardinality, row_count);
        
        let stats = ColumnStats {
            name: name.clone(),
            data_type,
            row_count,
            cardinality,
            null_count,
            min_value,
            max_value,
            compression_ratio,
        };
        
        self.stats.insert(name, stats.clone());
        stats
    }
    
    /// Select optimal compression codec based on column characteristics
    pub fn select_compression_codec(&self, column_name: &str) -> CompressionCodec {
        if let Some(stats) = self.stats.get(column_name) {
            Self::codec_for_stats(stats)
        } else {
            CompressionCodec::FOR  // Default fallback
        }
    }
    
    fn codec_for_stats(stats: &ColumnStats) -> CompressionCodec {
        // Low cardinality → Dictionary/RLE
        if stats.cardinality < 1000 {
            return CompressionCodec::Dictionary;
        }
        
        // Boolean or small set → RLE
        if stats.cardinality <= 10 {
            return CompressionCodec::RLE;
        }
        
        // Numeric types → FOR (Frame-of-Reference)
        if stats.data_type == "Integer" || stats.data_type == "Float" {
            return CompressionCodec::FOR;
        }
        
        // Text with repeats → Dictionary
        if stats.data_type == "String" && stats.cardinality < stats.row_count / 2 {
            return CompressionCodec::Dictionary;
        }
        
        // High-entropy data → LZSS
        CompressionCodec::LZSS
    }
    
    fn estimate_compression_ratio(cardinality: u64, row_count: u64) -> f32 {
        if cardinality == 0 {
            return 0.0;
        }
        
        // Shannon entropy-based estimation
        let p = cardinality as f32 / row_count as f32;
        if p == 0.0 || p == 1.0 {
            return 0.1;  // Highly compressible
        }
        
        // H(X) = -p*log2(p) - (1-p)*log2(1-p)
        let entropy = -(p * p.log2()) - ((1.0 - p) * (1.0 - p).log2());
        entropy / 8.0  // Rough approximation to bytes
    }
    
    /// Estimate cost of query execution
    pub fn estimate_query_cost(&self, predicate: Option<&str>) -> f64 {
        let mut cost = 0.0;
        
        // Base cost: read all columns
        for stats in self.stats.values() {
            cost += stats.row_count as f64 * stats.compression_ratio as f64;
        }
        
        // Predicate pushdown reduces rows
        if predicate.is_some() {
            cost *= 0.5;  // Assume predicate filters 50% of rows
        }
        
        cost
    }
}

/// Metadata cache for fast lookups
pub struct MetadataCache {
    schema: HashMap<String, String>,
    stats: HashMap<String, ColumnStats>,
    ttl_seconds: u64,
}

impl MetadataCache {
    pub fn new(ttl_seconds: u64) -> Self {
        MetadataCache {
            schema: HashMap::new(),
            stats: HashMap::new(),
            ttl_seconds,
        }
    }
    
    pub fn cache_column_stats(&mut self, name: String, stats: ColumnStats) {
        self.stats.insert(name, stats);
    }
    
    pub fn get_column_stats(&self, name: &str) -> Option<&ColumnStats> {
        self.stats.get(name)
    }
}

/// Index structure for fast point lookups
pub struct ColumnIndex {
    column: String,
    index_type: String,  // "hash", "btree", "bitmap"
    entries: HashMap<String, Vec<u64>>,  // value → row indices
}

impl ColumnIndex {
    pub fn new(column: String, index_type: String) -> Self {
        ColumnIndex {
            column,
            index_type,
            entries: HashMap::new(),
        }
    }
    
    pub fn build(&mut self, values: &[String]) {
        for (row_idx, value) in values.iter().enumerate() {
            self.entries
                .entry(value.clone())
                .or_insert_with(Vec::new)
                .push(row_idx as u64);
        }
    }
    
    pub fn lookup(&self, value: &str) -> Option<&Vec<u64>> {
        self.entries.get(value)
    }
    
    pub fn size_bytes(&self) -> u64 {
        let mut size = 0u64;
        for (key, rows) in &self.entries {
            size += key.len() as u64;
            size += rows.len() as u64 * 8;  // u64 per row index
        }
        size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_compression_selection() {
        let mut opt = QueryOptimizer::new();
        
        // Low cardinality string → Dictionary
        let values = vec!["A".to_string(); 1000];
        opt.collect_column_stats("col".to_string(), "String".to_string(), &values);
        
        match opt.select_compression_codec("col") {
            CompressionCodec::RLE => {}  // Expected
            _ => panic!("Expected RLE for constant values"),
        }
    }
    
    #[test]
    fn test_index_lookup() {
        let values = vec!["A".to_string(), "B".to_string(), "A".to_string()];
        let mut idx = ColumnIndex::new("col".to_string(), "hash".to_string());
        idx.build(&values);
        
        assert_eq!(idx.lookup("A"), Some(&vec![0, 2]));
        assert_eq!(idx.lookup("B"), Some(&vec![1]));
    }
}
