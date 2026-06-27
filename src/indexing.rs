/// Phase 3.6: Index Structures
/// B-tree indices, bitmap indices, and index selection for 50-100x query speedup
/// on indexed columns

use std::collections::{BTreeMap, HashMap};

/// Index type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexType {
    BTree,
    Bitmap,
    Hash,
}

impl IndexType {
    /// Get index name
    pub fn name(&self) -> &str {
        match self {
            IndexType::BTree => "B-Tree",
            IndexType::Bitmap => "Bitmap",
            IndexType::Hash => "Hash",
        }
    }
}

/// B-Tree index for sorted column values
#[derive(Clone)]
pub struct BTreeIndex {
    column_name: String,
    tree: BTreeMap<String, Vec<u64>>,
}

impl BTreeIndex {
    /// Create new B-Tree index
    pub fn new(column_name: String) -> Self {
        Self {
            column_name,
            tree: BTreeMap::new(),
        }
    }

    /// Add value with row ID
    pub fn add(&mut self, value: String, row_id: u64) {
        self.tree.entry(value).or_insert_with(Vec::new).push(row_id);
    }

    /// Find exact match
    pub fn find(&self, value: &str) -> Option<Vec<u64>> {
        self.tree.get(value).cloned()
    }

    /// Find range [start, end]
    pub fn range(&self, start: &str, end: &str) -> Vec<u64> {
        let mut result = Vec::new();
        for (_, row_ids) in self.tree.range(start.to_string()..=end.to_string()) {
            result.extend(row_ids);
        }
        result
    }

    /// Get all row IDs
    pub fn all_rows(&self) -> Vec<u64> {
        let mut result = Vec::new();
        for row_ids in self.tree.values() {
            result.extend(row_ids);
        }
        result
    }

    /// Index size in bytes
    pub fn size_bytes(&self) -> usize {
        let mut size = self.column_name.len() + 8;
        for (key, row_ids) in &self.tree {
            size += key.len() + row_ids.len() * 8;
        }
        size
    }

    /// Number of unique values
    pub fn cardinality(&self) -> usize {
        self.tree.len()
    }

    /// Column name
    pub fn column_name(&self) -> &str {
        &self.column_name
    }
}

/// Bitmap index for low-cardinality columns
#[derive(Clone)]
pub struct BitmapIndex {
    column_name: String,
    bitmaps: HashMap<String, Vec<bool>>,
    row_count: u64,
}

impl BitmapIndex {
    /// Create new bitmap index
    pub fn new(column_name: String, row_count: u64) -> Self {
        Self {
            column_name,
            bitmaps: HashMap::new(),
            row_count,
        }
    }

    /// Add bitmap for value
    pub fn add_bitmap(&mut self, value: String, bitmap: Vec<bool>) {
        self.bitmaps.insert(value, bitmap);
    }

    /// Get rows matching value
    pub fn find(&self, value: &str) -> Option<Vec<u64>> {
        self.bitmaps.get(value).map(|bitmap| {
            bitmap
                .iter()
                .enumerate()
                .filter(|(_, &set)| set)
                .map(|(i, _)| i as u64)
                .collect()
        })
    }

    /// OR two bitmaps
    pub fn bitmap_or(&self, value1: &str, value2: &str) -> Option<Vec<u64>> {
        let bitmap1 = self.bitmaps.get(value1)?;
        let bitmap2 = self.bitmaps.get(value2)?;

        let mut result = Vec::new();
        for (i, (&b1, &b2)) in bitmap1.iter().zip(bitmap2).enumerate() {
            if b1 || b2 {
                result.push(i as u64);
            }
        }
        Some(result)
    }

    /// AND two bitmaps
    pub fn bitmap_and(&self, value1: &str, value2: &str) -> Option<Vec<u64>> {
        let bitmap1 = self.bitmaps.get(value1)?;
        let bitmap2 = self.bitmaps.get(value2)?;

        let mut result = Vec::new();
        for (i, (&b1, &b2)) in bitmap1.iter().zip(bitmap2).enumerate() {
            if b1 && b2 {
                result.push(i as u64);
            }
        }
        Some(result)
    }

    /// Index size in bytes
    pub fn size_bytes(&self) -> usize {
        let mut size = self.column_name.len() + 8;
        for (key, bitmap) in &self.bitmaps {
            size += key.len() + bitmap.len();
        }
        size
    }

    /// Number of unique values (cardinality)
    pub fn cardinality(&self) -> usize {
        self.bitmaps.len()
    }

    /// Column name
    pub fn column_name(&self) -> &str {
        &self.column_name
    }

    /// Get row count
    pub fn row_count(&self) -> u64 {
        self.row_count
    }
}

/// Hash index for exact-match queries
#[derive(Clone)]
pub struct HashIndex {
    column_name: String,
    hash_map: HashMap<String, Vec<u64>>,
}

impl HashIndex {
    /// Create new hash index
    pub fn new(column_name: String) -> Self {
        Self {
            column_name,
            hash_map: HashMap::new(),
        }
    }

    /// Add value with row ID
    pub fn add(&mut self, value: String, row_id: u64) {
        self.hash_map.entry(value).or_insert_with(Vec::new).push(row_id);
    }

    /// Find exact match
    pub fn find(&self, value: &str) -> Option<Vec<u64>> {
        self.hash_map.get(value).cloned()
    }

    /// Get all row IDs
    pub fn all_rows(&self) -> Vec<u64> {
        let mut result = Vec::new();
        for row_ids in self.hash_map.values() {
            result.extend(row_ids);
        }
        result
    }

    /// Index size in bytes
    pub fn size_bytes(&self) -> usize {
        let mut size = self.column_name.len() + 8;
        for (key, row_ids) in &self.hash_map {
            size += key.len() + row_ids.len() * 8;
        }
        size
    }

    /// Number of unique values
    pub fn cardinality(&self) -> usize {
        self.hash_map.len()
    }

    /// Column name
    pub fn column_name(&self) -> &str {
        &self.column_name
    }
}

/// Index recommendations based on column statistics
pub struct IndexSelector;

impl IndexSelector {
    /// Recommend index type for column
    pub fn recommend_index(
        cardinality: usize,
        row_count: u64,
        uses_range_queries: bool,
    ) -> Option<IndexType> {
        let cardinality_ratio = (cardinality as f64) / (row_count as f64);

        // High cardinality (>=50%) → Hash for exact match
        if cardinality_ratio >= 0.5 {
            return Some(IndexType::Hash);
        }

        // Range queries needed → B-Tree
        if uses_range_queries {
            return Some(IndexType::BTree);
        }

        // Low cardinality (<10%) → Bitmap
        if cardinality_ratio < 0.1 {
            return Some(IndexType::Bitmap);
        }

        // Medium cardinality → B-Tree as fallback
        Some(IndexType::BTree)
    }

    /// Estimate speedup for index type
    pub fn estimated_speedup(index_type: IndexType, cardinality_ratio: f64) -> f64 {
        match index_type {
            IndexType::Hash => {
                // O(1) lookup vs O(n) scan
                100.0 * (1.0 - cardinality_ratio)
            }
            IndexType::BTree => {
                // O(log n) lookup vs O(n) scan
                50.0 * (1.0 - cardinality_ratio)
            }
            IndexType::Bitmap => {
                // Fast bitwise operations
                200.0 * (1.0 - cardinality_ratio)
            }
        }
    }
}

/// Index statistics and metadata
#[derive(Clone)]
pub struct IndexStats {
    pub column_name: String,
    pub index_type: IndexType,
    pub cardinality: usize,
    pub size_bytes: usize,
    pub estimated_speedup: f64,
    pub queries_using_index: u64,
}

impl IndexStats {
    /// Create new index stats
    pub fn new(
        column_name: String,
        index_type: IndexType,
        cardinality: usize,
        size_bytes: usize,
        estimated_speedup: f64,
    ) -> Self {
        Self {
            column_name,
            index_type,
            cardinality,
            size_bytes,
            estimated_speedup,
            queries_using_index: 0,
        }
    }

    /// Record query using index
    pub fn record_query(&mut self) {
        self.queries_using_index += 1;
    }

    /// Calculate space efficiency
    pub fn space_efficiency(&self) -> f64 {
        (self.estimated_speedup as f64) / (self.size_bytes as f64)
    }
}

/// Index manager for multiple indices
pub struct IndexManager {
    btree_indices: HashMap<String, BTreeIndex>,
    bitmap_indices: HashMap<String, BitmapIndex>,
    hash_indices: HashMap<String, HashIndex>,
    stats: Vec<IndexStats>,
}

impl IndexManager {
    /// Create new index manager
    pub fn new() -> Self {
        Self {
            btree_indices: HashMap::new(),
            bitmap_indices: HashMap::new(),
            hash_indices: HashMap::new(),
            stats: Vec::new(),
        }
    }

    /// Add B-Tree index
    pub fn add_btree(&mut self, column_name: String, index: BTreeIndex) {
        self.btree_indices.insert(column_name, index);
    }

    /// Add Bitmap index
    pub fn add_bitmap(&mut self, column_name: String, index: BitmapIndex) {
        self.bitmap_indices.insert(column_name, index);
    }

    /// Add Hash index
    pub fn add_hash(&mut self, column_name: String, index: HashIndex) {
        self.hash_indices.insert(column_name, index);
    }

    /// Get B-Tree index
    pub fn btree(&self, column_name: &str) -> Option<&BTreeIndex> {
        self.btree_indices.get(column_name)
    }

    /// Get Bitmap index
    pub fn bitmap(&self, column_name: &str) -> Option<&BitmapIndex> {
        self.bitmap_indices.get(column_name)
    }

    /// Get Hash index
    pub fn hash(&self, column_name: &str) -> Option<&HashIndex> {
        self.hash_indices.get(column_name)
    }

    /// Total index count
    pub fn index_count(&self) -> usize {
        self.btree_indices.len() + self.bitmap_indices.len() + self.hash_indices.len()
    }

    /// Total index size
    pub fn total_size_bytes(&self) -> usize {
        let btree_size: usize = self.btree_indices.values().map(|i| i.size_bytes()).sum();
        let bitmap_size: usize = self.bitmap_indices.values().map(|i| i.size_bytes()).sum();
        let hash_size: usize = self.hash_indices.values().map(|i| i.size_bytes()).sum();
        btree_size + bitmap_size + hash_size
    }

    /// Add stats record
    pub fn record_stats(&mut self, stats: IndexStats) {
        self.stats.push(stats);
    }

    /// Get stats
    pub fn stats(&self) -> &[IndexStats] {
        &self.stats
    }
}

impl Default for IndexManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_type_names() {
        assert_eq!(IndexType::BTree.name(), "B-Tree");
        assert_eq!(IndexType::Bitmap.name(), "Bitmap");
        assert_eq!(IndexType::Hash.name(), "Hash");
    }

    #[test]
    fn test_btree_index_creation() {
        let index = BTreeIndex::new("id".to_string());
        assert_eq!(index.column_name(), "id");
        assert_eq!(index.cardinality(), 0);
    }

    #[test]
    fn test_btree_index_add_and_find() {
        let mut index = BTreeIndex::new("id".to_string());
        index.add("100".to_string(), 0);
        index.add("200".to_string(), 1);

        assert_eq!(index.find("100"), Some(vec![0]));
        assert_eq!(index.cardinality(), 2);
    }

    #[test]
    fn test_btree_index_range() {
        let mut index = BTreeIndex::new("id".to_string());
        for i in 0..100 {
            index.add(format!("{:06}", i), i as u64);
        }

        let range = index.range("000010", "000020");
        assert!(range.len() > 0);
    }

    #[test]
    fn test_btree_index_size_bytes() {
        let mut index = BTreeIndex::new("id".to_string());
        index.add("100".to_string(), 0);
        
        let size = index.size_bytes();
        assert!(size > 0);
    }

    #[test]
    fn test_bitmap_index_creation() {
        let index = BitmapIndex::new("status".to_string(), 1000);
        assert_eq!(index.column_name(), "status");
        assert_eq!(index.cardinality(), 0);
        assert_eq!(index.row_count(), 1000);
    }

    #[test]
    fn test_bitmap_index_find() {
        let mut index = BitmapIndex::new("status".to_string(), 10);
        let bitmap = vec![true, false, true, false, false, false, false, false, false, false];
        index.add_bitmap("active".to_string(), bitmap);

        let rows = index.find("active");
        assert_eq!(rows, Some(vec![0, 2]));
    }

    #[test]
    fn test_bitmap_index_or() {
        let mut index = BitmapIndex::new("status".to_string(), 5);
        index.add_bitmap("a".to_string(), vec![true, false, true, false, false]);
        index.add_bitmap("b".to_string(), vec![false, true, false, true, false]);

        let result = index.bitmap_or("a", "b");
        assert_eq!(result, Some(vec![0, 1, 2, 3]));
    }

    #[test]
    fn test_bitmap_index_and() {
        let mut index = BitmapIndex::new("status".to_string(), 5);
        index.add_bitmap("a".to_string(), vec![true, true, true, false, false]);
        index.add_bitmap("b".to_string(), vec![true, false, true, true, false]);

        let result = index.bitmap_and("a", "b");
        assert_eq!(result, Some(vec![0, 2]));
    }

    #[test]
    fn test_hash_index_creation() {
        let index = HashIndex::new("email".to_string());
        assert_eq!(index.column_name(), "email");
        assert_eq!(index.cardinality(), 0);
    }

    #[test]
    fn test_hash_index_add_and_find() {
        let mut index = HashIndex::new("email".to_string());
        index.add("john@example.com".to_string(), 0);
        index.add("jane@example.com".to_string(), 1);

        assert_eq!(index.find("john@example.com"), Some(vec![0]));
        assert_eq!(index.cardinality(), 2);
    }

    #[test]
    fn test_index_selector_recommend_hash() {
        let index_type = IndexSelector::recommend_index(500, 1000, false);
        assert_eq!(index_type, Some(IndexType::Hash));
    }

    #[test]
    fn test_index_selector_recommend_bitmap() {
        let index_type = IndexSelector::recommend_index(50, 1000, false);
        assert_eq!(index_type, Some(IndexType::Bitmap));
    }

    #[test]
    fn test_index_selector_recommend_btree() {
        let index_type = IndexSelector::recommend_index(200, 1000, true);
        assert_eq!(index_type, Some(IndexType::BTree));
    }

    #[test]
    fn test_index_selector_speedup_hash() {
        let speedup = IndexSelector::estimated_speedup(IndexType::Hash, 0.1);
        assert!(speedup > 50.0);
    }

    #[test]
    fn test_index_selector_speedup_bitmap() {
        let speedup = IndexSelector::estimated_speedup(IndexType::Bitmap, 0.05);
        assert!(speedup > 150.0);
    }

    #[test]
    fn test_index_stats_creation() {
        let stats = IndexStats::new(
            "id".to_string(),
            IndexType::BTree,
            1000,
            10000,
            50.0,
        );
        assert_eq!(stats.column_name, "id");
        assert_eq!(stats.queries_using_index, 0);
    }

    #[test]
    fn test_index_stats_record_query() {
        let mut stats = IndexStats::new(
            "id".to_string(),
            IndexType::BTree,
            1000,
            10000,
            50.0,
        );
        stats.record_query();
        stats.record_query();
        assert_eq!(stats.queries_using_index, 2);
    }

    #[test]
    fn test_index_stats_space_efficiency() {
        let stats = IndexStats::new(
            "id".to_string(),
            IndexType::BTree,
            1000,
            10000,
            100.0,
        );
        let efficiency = stats.space_efficiency();
        assert!(efficiency > 0.0);
    }

    #[test]
    fn test_index_manager_creation() {
        let manager = IndexManager::new();
        assert_eq!(manager.index_count(), 0);
    }

    #[test]
    fn test_index_manager_add_indices() {
        let mut manager = IndexManager::new();
        let btree = BTreeIndex::new("id".to_string());
        let hash = HashIndex::new("email".to_string());

        manager.add_btree("id".to_string(), btree);
        manager.add_hash("email".to_string(), hash);

        assert_eq!(manager.index_count(), 2);
        assert!(manager.btree("id").is_some());
        assert!(manager.hash("email").is_some());
    }

    #[test]
    fn test_index_manager_total_size() {
        let mut manager = IndexManager::new();
        let mut btree = BTreeIndex::new("id".to_string());
        btree.add("100".to_string(), 0);

        manager.add_btree("id".to_string(), btree);

        let size = manager.total_size_bytes();
        assert!(size > 0);
    }

    #[test]
    fn test_index_manager_default() {
        let manager = IndexManager::default();
        assert_eq!(manager.index_count(), 0);
    }

    #[test]
    fn test_btree_index_multiple_rows_per_value() {
        let mut index = BTreeIndex::new("status".to_string());
        index.add("active".to_string(), 0);
        index.add("active".to_string(), 1);
        index.add("active".to_string(), 2);

        assert_eq!(index.find("active"), Some(vec![0, 1, 2]));
    }

    #[test]
    fn test_hash_index_all_rows() {
        let mut index = HashIndex::new("email".to_string());
        index.add("a@test.com".to_string(), 0);
        index.add("b@test.com".to_string(), 1);

        let all = index.all_rows();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_btree_index_all_rows() {
        let mut index = BTreeIndex::new("id".to_string());
        index.add("100".to_string(), 0);
        index.add("200".to_string(), 1);

        let all = index.all_rows();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_index_manager_bitmap_index() {
        let mut manager = IndexManager::new();
        let bitmap = BitmapIndex::new("active".to_string(), 100);
        manager.add_bitmap("active".to_string(), bitmap);

        assert!(manager.bitmap("active").is_some());
        assert_eq!(manager.index_count(), 1);
    }

    #[test]
    fn test_index_stats_record_multiple_queries() {
        let mut stats = IndexStats::new(
            "id".to_string(),
            IndexType::Hash,
            1000,
            20000,
            80.0,
        );
        for _ in 0..100 {
            stats.record_query();
        }
        assert_eq!(stats.queries_using_index, 100);
    }

    #[test]
    fn test_index_manager_stats_recording() {
        let mut manager = IndexManager::new();
        let stats = IndexStats::new(
            "id".to_string(),
            IndexType::BTree,
            1000,
            10000,
            50.0,
        );
        manager.record_stats(stats);

        assert_eq!(manager.stats().len(), 1);
    }

    #[test]
    fn test_bitmap_index_size_calculation() {
        let mut index = BitmapIndex::new("status".to_string(), 1000);
        let bitmap = vec![true; 1000];
        index.add_bitmap("active".to_string(), bitmap);

        let size = index.size_bytes();
        assert!(size > 1000);
    }

    #[test]
    fn test_btree_index_cardinality() {
        let mut index = BTreeIndex::new("id".to_string());
        for i in 0..50 {
            index.add(format!("{}", i), i as u64);
        }

        assert_eq!(index.cardinality(), 50);
    }
}
