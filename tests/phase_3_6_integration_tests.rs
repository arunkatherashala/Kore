use kore_fileformat::indexing::*;

/// Test B-Tree index basic functionality
#[test]
fn test_btree_index_creation() {
    let index = BTreeIndex::new("product_id".to_string());
    assert_eq!(index.column_name(), "product_id");
    assert_eq!(index.cardinality(), 0);
}

/// Test B-Tree index insertion
#[test]
fn test_btree_index_insertion_and_retrieval() {
    let mut index = BTreeIndex::new("id".to_string());
    index.add("100".to_string(), 0);
    index.add("200".to_string(), 1);
    index.add("300".to_string(), 2);

    assert_eq!(index.find("100"), Some(vec![0]));
    assert_eq!(index.find("200"), Some(vec![1]));
    assert_eq!(index.find("300"), Some(vec![2]));
    assert_eq!(index.find("400"), None);
}

/// Test B-Tree index range query
#[test]
fn test_btree_index_range_query() {
    let mut index = BTreeIndex::new("price".to_string());
    for i in 0..100 {
        index.add(format!("{:06}", i), i as u64);
    }

    let range_result = index.range("000010", "000020");
    assert!(range_result.len() > 0);
    assert!(range_result.len() <= 11);
}

/// Test B-Tree index multiple rows per value
#[test]
fn test_btree_index_duplicate_values() {
    let mut index = BTreeIndex::new("category".to_string());
    index.add("electronics".to_string(), 0);
    index.add("electronics".to_string(), 1);
    index.add("electronics".to_string(), 2);

    let rows = index.find("electronics");
    assert_eq!(rows, Some(vec![0, 1, 2]));
}

/// Test B-Tree index all rows retrieval
#[test]
fn test_btree_index_all_rows() {
    let mut index = BTreeIndex::new("id".to_string());
    index.add("100".to_string(), 0);
    index.add("200".to_string(), 1);
    index.add("300".to_string(), 2);

    let all_rows = index.all_rows();
    assert_eq!(all_rows.len(), 3);
}

/// Test B-Tree index size calculation
#[test]
fn test_btree_index_size_bytes() {
    let mut index = BTreeIndex::new("id".to_string());
    index.add("100".to_string(), 0);
    index.add("200".to_string(), 1);

    let size = index.size_bytes();
    assert!(size > 0);
    assert!(size > 10);
}

/// Test bitmap index creation
#[test]
fn test_bitmap_index_creation() {
    let index = BitmapIndex::new("status".to_string(), 1000);
    assert_eq!(index.column_name(), "status");
    assert_eq!(index.row_count(), 1000);
    assert_eq!(index.cardinality(), 0);
}

/// Test bitmap index insertion
#[test]
fn test_bitmap_index_insertion_and_retrieval() {
    let mut index = BitmapIndex::new("active".to_string(), 10);
    let bitmap = vec![true, false, true, false, false, false, false, false, false, false];
    index.add_bitmap("yes".to_string(), bitmap);

    let rows = index.find("yes");
    assert_eq!(rows, Some(vec![0, 2]));
}

/// Test bitmap index OR operation
#[test]
fn test_bitmap_index_or_operation() {
    let mut index = BitmapIndex::new("tags".to_string(), 5);
    index.add_bitmap("tag1".to_string(), vec![true, false, true, false, false]);
    index.add_bitmap("tag2".to_string(), vec![false, true, false, true, false]);

    let result = index.bitmap_or("tag1", "tag2");
    assert_eq!(result, Some(vec![0, 1, 2, 3]));
}

/// Test bitmap index AND operation
#[test]
fn test_bitmap_index_and_operation() {
    let mut index = BitmapIndex::new("flags".to_string(), 5);
    index.add_bitmap("flag1".to_string(), vec![true, true, true, false, false]);
    index.add_bitmap("flag2".to_string(), vec![true, false, true, true, false]);

    let result = index.bitmap_and("flag1", "flag2");
    assert_eq!(result, Some(vec![0, 2]));
}

/// Test bitmap index cardinality
#[test]
fn test_bitmap_index_cardinality() {
    let mut index = BitmapIndex::new("status".to_string(), 100);
    for i in 0..10 {
        index.add_bitmap(format!("status_{}", i), vec![false; 100]);
    }

    assert_eq!(index.cardinality(), 10);
}

/// Test hash index creation
#[test]
fn test_hash_index_creation() {
    let index = HashIndex::new("email".to_string());
    assert_eq!(index.column_name(), "email");
    assert_eq!(index.cardinality(), 0);
}

/// Test hash index insertion and exact match
#[test]
fn test_hash_index_exact_match() {
    let mut index = HashIndex::new("email".to_string());
    index.add("john@example.com".to_string(), 0);
    index.add("jane@example.com".to_string(), 1);

    assert_eq!(index.find("john@example.com"), Some(vec![0]));
    assert_eq!(index.find("jane@example.com"), Some(vec![1]));
    assert_eq!(index.find("unknown@example.com"), None);
}

/// Test hash index all rows
#[test]
fn test_hash_index_all_rows() {
    let mut index = HashIndex::new("id".to_string());
    index.add("1".to_string(), 0);
    index.add("2".to_string(), 1);
    index.add("3".to_string(), 2);

    let all = index.all_rows();
    assert_eq!(all.len(), 3);
}

/// Test hash index cardinality
#[test]
fn test_hash_index_cardinality() {
    let mut index = HashIndex::new("user_id".to_string());
    for i in 0..1000 {
        index.add(format!("user_{}", i), i as u64);
    }

    assert_eq!(index.cardinality(), 1000);
}

/// Test index type enumeration
#[test]
fn test_index_type_enum() {
    assert_eq!(IndexType::BTree.name(), "B-Tree");
    assert_eq!(IndexType::Bitmap.name(), "Bitmap");
    assert_eq!(IndexType::Hash.name(), "Hash");
}

/// Test index selector recommends hash for high cardinality
#[test]
fn test_index_selector_high_cardinality() {
    let recommended = IndexSelector::recommend_index(800, 1000, false);
    assert_eq!(recommended, Some(IndexType::Hash));
}

/// Test index selector recommends bitmap for low cardinality
#[test]
fn test_index_selector_low_cardinality() {
    let recommended = IndexSelector::recommend_index(50, 1000, false);
    assert_eq!(recommended, Some(IndexType::Bitmap));
}

/// Test index selector recommends B-Tree for range queries
#[test]
fn test_index_selector_range_queries() {
    let recommended = IndexSelector::recommend_index(200, 1000, true);
    assert_eq!(recommended, Some(IndexType::BTree));
}

/// Test index selector speedup estimation
#[test]
fn test_index_selector_speedup_hash() {
    let speedup = IndexSelector::estimated_speedup(IndexType::Hash, 0.1);
    assert!(speedup > 50.0);
}

/// Test index selector speedup bitmap
#[test]
fn test_index_selector_speedup_bitmap() {
    let speedup = IndexSelector::estimated_speedup(IndexType::Bitmap, 0.05);
    assert!(speedup > 150.0);
}

/// Test index stats creation
#[test]
fn test_index_stats_creation() {
    let stats = IndexStats::new(
        "product_id".to_string(),
        IndexType::BTree,
        10000,
        100000,
        75.0,
    );
    assert_eq!(stats.column_name, "product_id");
    assert_eq!(stats.cardinality, 10000);
    assert_eq!(stats.size_bytes, 100000);
}

/// Test index stats query recording
#[test]
fn test_index_stats_query_recording() {
    let mut stats = IndexStats::new(
        "id".to_string(),
        IndexType::Hash,
        1000,
        20000,
        100.0,
    );

    for _ in 0..50 {
        stats.record_query();
    }

    assert_eq!(stats.queries_using_index, 50);
}

/// Test index stats space efficiency
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

/// Test index manager creation
#[test]
fn test_index_manager_creation() {
    let manager = IndexManager::new();
    assert_eq!(manager.index_count(), 0);
}

/// Test index manager add indices
#[test]
fn test_index_manager_add_multiple_indices() {
    let mut manager = IndexManager::new();
    
    let btree = BTreeIndex::new("id".to_string());
    let hash = HashIndex::new("email".to_string());
    let bitmap = BitmapIndex::new("status".to_string(), 1000);

    manager.add_btree("id".to_string(), btree);
    manager.add_hash("email".to_string(), hash);
    manager.add_bitmap("status".to_string(), bitmap);

    assert_eq!(manager.index_count(), 3);
}

/// Test index manager retrieve indices
#[test]
fn test_index_manager_retrieve_indices() {
    let mut manager = IndexManager::new();
    let btree = BTreeIndex::new("id".to_string());
    
    manager.add_btree("id".to_string(), btree);

    assert!(manager.btree("id").is_some());
    assert!(manager.hash("id").is_none());
}

/// Test index manager total size calculation
#[test]
fn test_index_manager_total_size() {
    let mut manager = IndexManager::new();
    let mut btree = BTreeIndex::new("id".to_string());
    btree.add("100".to_string(), 0);

    manager.add_btree("id".to_string(), btree);

    let total_size = manager.total_size_bytes();
    assert!(total_size > 0);
}

/// Test index manager default initialization
#[test]
fn test_index_manager_default() {
    let manager = IndexManager::default();
    assert_eq!(manager.index_count(), 0);
}

/// Test index manager stats recording
#[test]
fn test_index_manager_stats_recording() {
    let mut manager = IndexManager::new();
    
    let stats1 = IndexStats::new(
        "id".to_string(),
        IndexType::BTree,
        1000,
        10000,
        50.0,
    );
    let stats2 = IndexStats::new(
        "email".to_string(),
        IndexType::Hash,
        500,
        5000,
        80.0,
    );

    manager.record_stats(stats1);
    manager.record_stats(stats2);

    assert_eq!(manager.stats().len(), 2);
}

/// Test real-world scenario: product catalog indexing
#[test]
fn test_real_world_product_catalog() {
    let mut manager = IndexManager::new();

    // Index product IDs (high cardinality → Hash)
    let mut product_idx = HashIndex::new("product_id".to_string());
    for i in 0..5000 {
        product_idx.add(format!("PROD_{:05}", i), i as u64);
    }
    manager.add_hash("product_id".to_string(), product_idx);

    // Index categories (low cardinality → Bitmap)
    let mut category_idx = BitmapIndex::new("category".to_string(), 5000);
    let mut electronics_bitmap = vec![false; 5000];
    for i in 0..1000 {
        electronics_bitmap[i] = true;
    }
    category_idx.add_bitmap("electronics".to_string(), electronics_bitmap);
    manager.add_bitmap("category".to_string(), category_idx);

    // Index price ranges (range queries → B-Tree)
    let mut price_idx = BTreeIndex::new("price".to_string());
    for i in 0..5000 {
        price_idx.add(format!("{:010}", i * 100), i as u64);
    }
    manager.add_btree("price".to_string(), price_idx);

    assert_eq!(manager.index_count(), 3);
}

/// Test index manager performance metrics
#[test]
fn test_index_manager_with_statistics() {
    let mut manager = IndexManager::new();

    // Create indices with stats
    let btree = BTreeIndex::new("id".to_string());
    let stats = IndexStats::new(
        "id".to_string(),
        IndexType::BTree,
        10000,
        100000,
        75.0,
    );

    manager.add_btree("id".to_string(), btree);
    manager.record_stats(stats);

    let recorded_stats = manager.stats();
    assert_eq!(recorded_stats.len(), 1);
    assert_eq!(recorded_stats[0].estimated_speedup, 75.0);
}

/// Test B-Tree index with large dataset
#[test]
fn test_btree_large_dataset() {
    let mut index = BTreeIndex::new("id".to_string());
    
    for i in 0..10000 {
        index.add(format!("{:08}", i), i as u64);
    }

    assert_eq!(index.cardinality(), 10000);
    let size = index.size_bytes();
    assert!(size > 50000);
}

/// Test bitmap index memory efficiency
#[test]
fn test_bitmap_index_memory_efficiency() {
    let mut index = BitmapIndex::new("flags".to_string(), 100000);
    
    // Low cardinality bitmaps are memory efficient
    for i in 0..10 {
        index.add_bitmap(format!("flag_{}", i), vec![false; 100000]);
    }

    let size = index.size_bytes();
    assert!(size < 2000000); // Should be reasonably compact
}

/// Test combined index queries
#[test]
fn test_combined_index_queries() {
    let mut manager = IndexManager::new();

    // Add multiple index types
    let mut btree = BTreeIndex::new("date".to_string());
    btree.add("2024-01-01".to_string(), 0);
    btree.add("2024-01-02".to_string(), 1);
    
    let mut hash = HashIndex::new("user_id".to_string());
    hash.add("user_123".to_string(), 0);
    
    manager.add_btree("date".to_string(), btree);
    manager.add_hash("user_id".to_string(), hash);

    assert_eq!(manager.index_count(), 2);
    assert!(manager.btree("date").is_some());
    assert!(manager.hash("user_id").is_some());
}
