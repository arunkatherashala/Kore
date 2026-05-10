/// KORE Index Management System
/// Provides column indexing, index selection, and index-based query optimization

use std::collections::{HashMap, BTreeMap};

/// Index type
#[derive(Debug, Clone, PartialEq)]
pub enum IndexType {
    Hash,       // Hash index for equality lookups
    BTree,      // B-Tree index for range queries
    Bitmap,     // Bitmap index for low-cardinality columns
    FullText,   // Full-text index for text search
}

/// Column index
#[derive(Debug, Clone)]
pub struct ColumnIndex {
    pub table: String,
    pub column: String,
    pub index_type: IndexType,
    pub cardinality: usize,      // Unique values
    pub size_bytes: u64,
    pub is_unique: bool,
    pub is_sparse: bool,
}

impl ColumnIndex {
    /// Estimate index space requirement
    pub fn estimate_size(cardinality: usize, data_size_bytes: u64) -> u64 {
        // Heuristic: index is roughly 10-20% of data size
        (data_size_bytes / 10).max(1000) as u64
    }

    /// Get index selectivity (0.0 = low selectivity, 1.0 = high selectivity)
    pub fn selectivity(&self) -> f64 {
        if self.cardinality == 0 {
            0.0
        } else {
            1.0 / (self.cardinality as f64)
        }
    }
}

/// Index manager
pub struct IndexManager {
    indices: HashMap<String, ColumnIndex>,
    index_stats: HashMap<String, IndexStats>,
}

impl IndexManager {
    /// Create new index manager
    pub fn new() -> Self {
        Self {
            indices: HashMap::new(),
            index_stats: HashMap::new(),
        }
    }

    /// Create index on column
    pub fn create_index(
        &mut self,
        table: &str,
        column: &str,
        index_type: IndexType,
        cardinality: usize,
        data_size_bytes: u64,
    ) -> Result<(), String> {
        let index_key = format!("{}.{}", table, column);
        
        if self.indices.contains_key(&index_key) {
            return Err(format!("Index already exists on {}.{}", table, column));
        }

        let index = ColumnIndex {
            table: table.to_string(),
            column: column.to_string(),
            index_type,
            cardinality,
            size_bytes: ColumnIndex::estimate_size(cardinality, data_size_bytes),
            is_unique: cardinality == 1,
            is_sparse: cardinality < (data_size_bytes / 1000) as usize,
        };

        self.indices.insert(index_key.clone(), index);
        self.index_stats.insert(index_key, IndexStats::default());
        
        Ok(())
    }

    /// Drop index
    pub fn drop_index(&mut self, table: &str, column: &str) -> Result<(), String> {
        let index_key = format!("{}.{}", table, column);
        
        self.indices.remove(&index_key);
        self.index_stats.remove(&index_key);
        
        Ok(())
    }

    /// Get index if it exists
    pub fn get_index(&self, table: &str, column: &str) -> Option<&ColumnIndex> {
        let index_key = format!("{}.{}", table, column);
        self.indices.get(&index_key)
    }

    /// Find best index for column
    pub fn find_best_index(&self, table: &str, column: &str) -> Option<&ColumnIndex> {
        let index_key = format!("{}.{}", table, column);
        self.indices.get(&index_key)
    }

    /// Get indexes on table
    pub fn get_table_indexes(&self, table: &str) -> Vec<&ColumnIndex> {
        self.indices
            .values()
            .filter(|idx| idx.table == table)
            .collect()
    }

    /// Calculate total index space
    pub fn total_index_space(&self) -> u64 {
        self.indices.values().map(|idx| idx.size_bytes).sum()
    }

    /// Get index statistics
    pub fn get_stats(&self, table: &str, column: &str) -> Option<&IndexStats> {
        let index_key = format!("{}.{}", table, column);
        self.index_stats.get(&index_key)
    }

    /// Update index hit count
    pub fn record_index_usage(&mut self, table: &str, column: &str) {
        let index_key = format!("{}.{}", table, column);
        if let Some(stats) = self.index_stats.get_mut(&index_key) {
            stats.hits += 1;
        }
    }

    /// Recommend indexes based on query patterns
    pub fn recommend_indexes(
        &self,
        table: &str,
        frequently_filtered_cols: &[&str],
    ) -> Vec<IndexRecommendation> {
        let mut recommendations = Vec::new();

        for col in frequently_filtered_cols {
            let index_key = format!("{}.{}", table, col);
            if !self.indices.contains_key(&index_key) {
                recommendations.push(IndexRecommendation {
                    table: table.to_string(),
                    column: col.to_string(),
                    index_type: IndexType::Hash,
                    estimated_benefit: 0.3, // Estimated 30% query speedup
                });
            }
        }

        recommendations
    }
}

/// Index statistics
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub hits: usize,
    pub misses: usize,
    pub last_used: Option<u64>, // Unix timestamp
}

/// Index recommendation
#[derive(Debug, Clone)]
pub struct IndexRecommendation {
    pub table: String,
    pub column: String,
    pub index_type: IndexType,
    pub estimated_benefit: f64,
}

/// Query with index hints
#[derive(Debug, Clone)]
pub struct IndexedQuery {
    pub query_text: String,
    pub suggested_indexes: Vec<String>,
    pub estimated_speedup: f64,
}

/// Index optimizer
pub struct IndexOptimizer;

impl IndexOptimizer {
    /// Analyze query to suggest index usage
    pub fn analyze_query(
        query_text: &str,
        manager: &IndexManager,
    ) -> IndexedQuery {
        let mut suggested_indexes = Vec::new();
        let mut speedup: f64 = 1.0;

        // Simple heuristic: if query has WHERE clause, suggest indexes on filtered columns
        if query_text.contains("WHERE") {
            // Find common filter columns in WHERE clause
            let common_cols = vec!["id", "user_id", "status", "created_at"];
            for col in common_cols {
                if query_text.contains(col) && manager.find_best_index("table", col).is_some() {
                    suggested_indexes.push(col.to_string());
                    speedup *= 1.5; // 50% speedup per index used
                }
            }
        }

        IndexedQuery {
            query_text: query_text.to_string(),
            suggested_indexes,
            estimated_speedup: speedup.min(3.0), // Max 3x speedup
        }
    }

    /// Estimate cardinality of column
    pub fn estimate_cardinality(
        sample_size: usize,
        unique_in_sample: usize,
    ) -> usize {
        // Simple extrapolation
        if sample_size == 0 {
            0
        } else {
            (unique_in_sample * 100) / sample_size
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_index() {
        let mut manager = IndexManager::new();
        let result = manager.create_index("users", "id", IndexType::Hash, 10000, 1000000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_index() {
        let mut manager = IndexManager::new();
        manager.create_index("users", "id", IndexType::Hash, 10000, 1000000).unwrap();
        
        let index = manager.get_index("users", "id");
        assert!(index.is_some());
    }

    #[test]
    fn test_index_selectivity() {
        let index = ColumnIndex {
            table: "users".to_string(),
            column: "status".to_string(),
            index_type: IndexType::Bitmap,
            cardinality: 5,
            size_bytes: 50000,
            is_unique: false,
            is_sparse: false,
        };

        assert!(index.selectivity() > 0.0);
        assert!(index.selectivity() < 1.0);
    }

    #[test]
    fn test_drop_index() {
        let mut manager = IndexManager::new();
        manager.create_index("users", "id", IndexType::Hash, 10000, 1000000).unwrap();
        let drop_result = manager.drop_index("users", "id");
        
        assert!(drop_result.is_ok());
        assert!(manager.get_index("users", "id").is_none());
    }

    #[test]
    fn test_total_index_space() {
        let mut manager = IndexManager::new();
        manager.create_index("users", "id", IndexType::Hash, 10000, 1000000).unwrap();
        manager.create_index("users", "email", IndexType::BTree, 10000, 1000000).unwrap();
        
        let total = manager.total_index_space();
        assert!(total > 0);
    }

    #[test]
    fn test_index_recommendation() {
        let mut manager = IndexManager::new();
        let recommendations = manager.recommend_indexes("users", &["id", "email"]);
        
        assert!(recommendations.len() > 0);
    }

    #[test]
    fn test_estimate_cardinality() {
        let cardinality = IndexOptimizer::estimate_cardinality(1000, 500);
        assert!(cardinality > 0);
    }

    #[test]
    fn test_index_stats() {
        let mut manager = IndexManager::new();
        manager.create_index("users", "id", IndexType::Hash, 10000, 1000000).unwrap();
        manager.record_index_usage("users", "id");
        
        let stats = manager.get_stats("users", "id").unwrap();
        assert_eq!(stats.hits, 1);
    }
}
