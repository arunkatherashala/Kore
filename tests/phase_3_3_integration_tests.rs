use kore_fileformat::caching::{RowCache, QueryResultCache, CacheStats, CachingLayer};
use kore_fileformat::predicates::{ColumnSelection, PredicateExpression, QueryFilter};

/// Test row cache with multiple entries
#[test]
fn test_row_cache_multiple_entries_lru() {
    let mut cache = RowCache::with_size(300);
    
    // Add 3 entries (100 bytes each)
    cache.get_or_insert("row1", vec![1; 100]);
    cache.get_or_insert("row2", vec![2; 100]);
    cache.get_or_insert("row3", vec![3; 100]);
    
    // Cache is exactly full (300 bytes)
    assert_eq!(cache.size_bytes(), 300);
    assert_eq!(cache.len(), 3);
    
    // Add 4th entry - should evict row1 (oldest)
    cache.get_or_insert("row4", vec![4; 100]);
    
    assert_eq!(cache.size_bytes(), 300);
    assert_eq!(cache.len(), 3);
    assert!(cache.get("row1").is_none()); // row1 evicted
    assert!(cache.get("row2").is_some());
    assert!(cache.get("row3").is_some());
    assert!(cache.get("row4").is_some());
}

/// Test row cache access pattern preserves LRU order
#[test]
fn test_row_cache_access_moves_to_end_of_lru() {
    let mut cache = RowCache::with_size(400);
    
    // Add 4 entries
    for i in 1..=4 {
        cache.get_or_insert(&format!("row{}", i), vec![i as u8; 100]);
    }
    
    // Access row1 (moves it to end of LRU order)
    cache.get("row1");
    
    // Add new entry - should evict row2 (oldest after row1 moved)
    cache.get_or_insert("row5", vec![5; 100]);
    
    assert!(cache.get("row1").is_some()); // row1 still there
    assert!(cache.get("row2").is_none()); // row2 evicted
    assert!(cache.get("row3").is_some());
    assert!(cache.get("row4").is_some());
    assert!(cache.get("row5").is_some());
}

/// Test query cache with selectivity tracking
#[test]
fn test_query_cache_selectivity_discrimination() {
    let mut cache = QueryResultCache::new();
    
    // Low selectivity query (10% of rows)
    cache.insert("q1".to_string(), vec![0, 10, 20, 30, 40], 0.1);
    
    // Medium selectivity query (50% of rows)
    cache.insert("q2".to_string(), (0..500).collect(), 0.5);
    
    // High selectivity query (1% of rows)
    cache.insert("q3".to_string(), vec![0, 1, 2], 0.01);
    
    let e1 = cache.get_entry("q1").unwrap();
    let e2 = cache.get_entry("q2").unwrap();
    let e3 = cache.get_entry("q3").unwrap();
    
    assert!((e1.selectivity - 0.1).abs() < 0.01);
    assert!((e2.selectivity - 0.5).abs() < 0.01);
    assert!((e3.selectivity - 0.01).abs() < 0.01);
}

/// Test cache stats with realistic access pattern
#[test]
fn test_cache_stats_realistic_pattern() {
    let mut stats = CacheStats::new();
    
    // Simulate 100 cache operations: 70 hits, 30 misses
    for _ in 0..70 {
        stats.record_hit();
    }
    for _ in 0..30 {
        stats.record_miss();
    }
    
    assert_eq!(stats.total_gets, 100);
    assert!((stats.hit_rate() - 0.7).abs() < 0.01);
    assert!((stats.miss_rate() - 0.3).abs() < 0.01);
    
    // Track memory growth
    stats.record_memory(1_000_000); // 1MB
    stats.record_memory(2_000_000); // 2MB
    stats.record_memory(1_500_000); // 1.5MB (decrease)
    
    assert_eq!(stats.current_memory_bytes, 1_500_000);
    assert_eq!(stats.peak_memory_bytes, 2_000_000);
}

/// Test caching layer query result caching
#[test]
fn test_caching_layer_query_result_caching() {
    let layer = CachingLayer::new();
    
    // Simulate filter query execution
    let indices = vec![0, 10, 20, 30, 40, 50];
    let cache_key = "age > 25 AND status = active".to_string();
    
    // First execution: cache miss, insert result
    layer.insert_query(cache_key.clone(), indices.clone(), 0.6);
    
    // Second execution: cache hit
    let cached = layer.get_query(&cache_key);
    assert_eq!(cached, Some(indices));
    
    // Check stats
    let stats = layer.get_stats();
    assert_eq!(stats.cache_hits, 1);
    assert_eq!(stats.cache_misses, 0);
}

/// Test caching layer with row cache size limit
#[test]
fn test_caching_layer_with_row_cache_size_limit() {
    let layer = CachingLayer::with_row_cache_size(500);
    
    // Insert rows up to cache capacity
    layer.insert_row("row1".to_string(), vec![1; 250]);
    layer.insert_row("row2".to_string(), vec![2; 250]);
    
    let (count, size) = layer.row_cache_info();
    assert_eq!(count, 2);
    assert_eq!(size, 500);
    
    // Insert 3rd row - should evict oldest
    layer.insert_row("row3".to_string(), vec![3; 250]);
    
    let (count, size) = layer.row_cache_info();
    assert_eq!(count, 2);
    assert_eq!(size, 500);
}

/// Test query cache capacity enforcement
#[test]
fn test_query_cache_capacity_enforcement() {
    let mut cache = QueryResultCache::with_capacity(5);
    
    // Fill to capacity
    for i in 0..5 {
        cache.insert(
            format!("q{}", i),
            vec![i as usize],
            0.1 * i as f64,
        );
    }
    assert_eq!(cache.len(), 5);
    
    // Add one more - should evict oldest (q0)
    cache.insert("q5".to_string(), vec![5], 0.5);
    
    assert_eq!(cache.len(), 5);
    assert!(cache.get("q0").is_none());
    assert!(cache.get("q5").is_some());
}

/// Test caching layer combined row and query operations
#[test]
fn test_caching_layer_combined_operations() {
    let layer = CachingLayer::new();
    
    // Cache some rows
    layer.insert_row("row_a".to_string(), vec![1, 2, 3]);
    layer.insert_row("row_b".to_string(), vec![4, 5, 6]);
    
    // Cache query results
    layer.insert_query("filter1".to_string(), vec![0, 1], 0.2);
    layer.insert_query("filter2".to_string(), vec![2, 3, 4], 0.3);
    
    // Test retrieval
    assert_eq!(layer.get_row("row_a"), Some(vec![1, 2, 3]));
    assert_eq!(layer.get_query("filter1"), Some(vec![0, 1]));
    
    // Verify stats
    let stats = layer.get_stats();
    assert_eq!(stats.cache_hits, 2);
    assert_eq!(stats.cache_misses, 0);
}

/// Test cache stats reset functionality
#[test]
fn test_cache_stats_reset_clears_all() {
    let mut stats = CacheStats::new();
    
    // Build up stats
    stats.record_hit();
    stats.record_miss();
    stats.record_eviction();
    stats.record_eviction();
    stats.record_memory(5_000_000);
    
    assert!(stats.total_gets > 0);
    assert!(stats.total_evictions > 0);
    assert!(stats.peak_memory_bytes > 0);
    
    // Reset
    stats.reset();
    
    assert_eq!(stats.total_gets, 0);
    assert_eq!(stats.cache_hits, 0);
    assert_eq!(stats.cache_misses, 0);
    assert_eq!(stats.total_evictions, 0);
    assert_eq!(stats.peak_memory_bytes, 0);
    assert_eq!(stats.current_memory_bytes, 0);
}

/// Test caching layer clear all
#[test]
fn test_caching_layer_clear_all_resets_everything() {
    let layer = CachingLayer::new();
    
    // Populate both caches
    layer.insert_row("row1".to_string(), vec![1, 2, 3]);
    layer.insert_query("q1".to_string(), vec![0, 1], 0.5);
    layer.get_row("row1"); // Record hit
    
    // Verify content
    assert_eq!(layer.row_cache_info().0, 1);
    assert_eq!(layer.query_cache_info().0, 1);
    let stats = layer.get_stats();
    assert!(stats.cache_hits > 0);
    
    // Clear all
    layer.clear_all();
    
    // Verify all cleared
    assert_eq!(layer.row_cache_info(), (0, 0));
    assert_eq!(layer.query_cache_info(), (0, 0));
    let stats = layer.get_stats();
    assert_eq!(stats.cache_hits, 0);
    assert_eq!(stats.cache_misses, 0);
}

/// Test row cache with very small size
#[test]
fn test_row_cache_small_size_aggressive_eviction() {
    let mut cache = RowCache::with_size(50);
    
    // Try to add entry larger than cache - should still work
    cache.get_or_insert("large", vec![0; 30]);
    assert_eq!(cache.len(), 1);
    
    // Add another
    cache.get_or_insert("another", vec![0; 30]);
    
    // First should be evicted
    assert!(cache.get("large").is_none());
    assert_eq!(cache.len(), 1);
}

/// Test query cache memory estimation
#[test]
fn test_query_cache_memory_estimation_accuracy() {
    let mut cache = QueryResultCache::new();
    
    // Insert queries with different result set sizes
    cache.insert("q1".to_string(), vec![0; 100], 0.1); // 100 indices
    cache.insert("q2".to_string(), vec![0; 1000], 0.5); // 1000 indices
    cache.insert("q3".to_string(), vec![0; 10], 0.01); // 10 indices
    
    let e1 = cache.get_entry("q1").unwrap();
    let e2 = cache.get_entry("q2").unwrap();
    let e3 = cache.get_entry("q3").unwrap();
    
    assert_eq!(e1.size_estimate, 800); // 100 * 8
    assert_eq!(e2.size_estimate, 8000); // 1000 * 8
    assert_eq!(e3.size_estimate, 80); // 10 * 8
    
    let total = cache.memory_usage();
    assert_eq!(total, 800 + 8000 + 80);
}

/// Test caching layer hit rate metrics
#[test]
fn test_caching_layer_hit_rate_metrics() {
    let layer = CachingLayer::new();
    
    // Cache a row
    layer.insert_row("row1".to_string(), vec![1, 2, 3]);
    
    // Simulate access pattern: 3 hits, 2 misses
    layer.get_row("row1"); // hit
    layer.get_row("row1"); // hit
    layer.get_row("row1"); // hit
    layer.get_row("row2"); // miss
    layer.get_row("row3"); // miss
    
    let stats = layer.get_stats();
    assert_eq!(stats.total_gets, 5);
    assert_eq!(stats.cache_hits, 3);
    assert_eq!(stats.cache_misses, 2);
    assert!((stats.hit_rate() - 0.6).abs() < 0.01);
}

/// Test row cache entry update doesn't increase entry count
#[test]
fn test_row_cache_repeated_key_updates() {
    let mut cache = RowCache::new();
    
    let data1 = vec![1, 2, 3];
    cache.get_or_insert("key1", data1.clone());
    assert_eq!(cache.len(), 1);
    
    // Try to "update" with new data - returns old data
    let result = cache.get_or_insert("key1", vec![4, 5, 6]);
    assert_eq!(result, data1); // Returns original, not new
    assert_eq!(cache.len(), 1); // Still 1 entry
}

/// Test query cache selectivity distribution
#[test]
fn test_query_cache_selectivity_range() {
    let mut cache = QueryResultCache::new();
    
    // Create queries with various selectivity levels
    let selectivities = vec![0.01, 0.05, 0.1, 0.25, 0.5, 0.75, 0.9, 0.95, 0.99];
    
    for (i, sel) in selectivities.iter().enumerate() {
        cache.insert(format!("q{}", i), vec![i], *sel);
    }
    
    // Verify all stored correctly
    for (i, sel) in selectivities.iter().enumerate() {
        let entry = cache.get_entry(&format!("q{}", i)).unwrap();
        assert!((entry.selectivity - sel).abs() < 0.001);
    }
}

/// Test caching layer with stress test (many operations)
#[test]
fn test_caching_layer_stress_many_operations() {
    let layer = CachingLayer::new();
    
    // Insert 100 rows
    for i in 0..100 {
        layer.insert_row(format!("row{}", i), vec![i as u8; 10]);
    }
    
    // Insert 50 query results
    for i in 0..50 {
        layer.insert_query(format!("q{}", i), vec![i], 0.5);
    }
    
    // Perform 200 random accesses
    for i in 0..200 {
        if i % 2 == 0 {
            let _ = layer.get_row(&format!("row{}", i % 100));
        } else {
            let _ = layer.get_query(&format!("q{}", i % 50));
        }
    }
    
    // Verify stats are tracked
    let stats = layer.get_stats();
    assert_eq!(stats.total_gets, 200);
    assert!(stats.cache_hits >= 50); // Should have some hits
}

/// Test row cache eviction order with multiple different sizes
#[test]
fn test_row_cache_mixed_size_eviction() {
    let mut cache = RowCache::with_size(500);
    
    cache.get_or_insert("small", vec![1; 50]);
    cache.get_or_insert("medium", vec![2; 200]);
    cache.get_or_insert("large", vec![3; 200]);
    
    assert_eq!(cache.size_bytes(), 450);
    
    // Add entry that requires eviction
    cache.get_or_insert("new", vec![4; 100]);
    
    // Should evict oldest (small)
    assert!(cache.get("small").is_none());
    assert_eq!(cache.size_bytes(), 500);
}

/// Test query cache capacity with high access frequency
#[test]
fn test_query_cache_high_access_frequency() {
    let mut cache = QueryResultCache::with_capacity(3);
    
    cache.insert("q1".to_string(), vec![1], 0.1);
    cache.insert("q2".to_string(), vec![2], 0.2);
    cache.insert("q3".to_string(), vec![3], 0.3);
    
    // Access q1 many times (makes it most recently used)
    for _ in 0..10 {
        cache.get("q1");
    }
    
    // Add new query - should evict q2 (least recently used)
    cache.insert("q4".to_string(), vec![4], 0.4);
    
    assert!(cache.get("q1").is_some()); // Most accessed, not evicted
    assert!(cache.get("q2").is_none()); // Least accessed, evicted
    assert!(cache.get("q3").is_some());
    assert!(cache.get("q4").is_some());
}

/// Test row cache with single large entry
#[test]
fn test_row_cache_single_large_entry() {
    let mut cache = RowCache::with_size(10000);
    let large_data = vec![42u8; 8000];
    cache.get_or_insert("large", large_data.clone());
    
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.size_bytes(), 8000);
    assert_eq!(cache.get("large"), Some(large_data));
}

/// Test query cache miss statistics
#[test]
fn test_query_cache_miss_statistics() {
    let mut cache = QueryResultCache::new();
    cache.insert("q1".to_string(), vec![0, 1], 0.2);
    
    // Multiple misses
    cache.get("q99");
    cache.get("q98");
    cache.get("q97");
    
    // Should have None for all misses
    assert!(cache.get("q99").is_none());
}

/// Test cache stats memory peak tracking
#[test]
fn test_cache_stats_memory_peak_tracking() {
    let mut stats = CacheStats::new();
    
    stats.record_memory(500);
    assert_eq!(stats.peak_memory_bytes, 500);
    
    stats.record_memory(2000);
    assert_eq!(stats.peak_memory_bytes, 2000);
    
    stats.record_memory(1000);
    assert_eq!(stats.peak_memory_bytes, 2000); // Still tracks max
    assert_eq!(stats.current_memory_bytes, 1000);
}

/// Test row cache empty get returns none
#[test]
fn test_row_cache_empty_get_returns_none() {
    let mut cache = RowCache::new();
    assert!(cache.get("nonexistent").is_none());
    assert_eq!(cache.len(), 0);
}

/// Test query cache entry access count tracking
#[test]
fn test_query_cache_entry_access_count() {
    let mut cache = QueryResultCache::new();
    cache.insert("q1".to_string(), vec![0, 1], 0.5);
    
    let e1 = cache.get_entry("q1").unwrap();
    assert_eq!(e1.access_count, 1);
    
    cache.get("q1");
    cache.get("q1");
    
    let e1 = cache.get_entry("q1").unwrap();
    assert_eq!(e1.access_count, 3);
}

/// Test caching layer row cache default size
#[test]
fn test_caching_layer_default_row_cache_size() {
    let layer = CachingLayer::new();
    layer.insert_row("r1".to_string(), vec![0; 10_000]);
    
    let (count, size) = layer.row_cache_info();
    assert_eq!(count, 1);
    assert!(size >= 10_000);
}

/// Test caching layer query cache default capacity
#[test]
fn test_caching_layer_query_cache_default_capacity() {
    let layer = CachingLayer::new();
    
    for i in 0..50 {
        layer.insert_query(format!("q{}", i), vec![i], 0.5);
    }
    
    let (count, _) = layer.query_cache_info();
    assert_eq!(count, 50);
}

/// Test cache stats zero hit rate
#[test]
fn test_cache_stats_zero_hit_rate() {
    let mut stats = CacheStats::new();
    
    for _ in 0..10 {
        stats.record_miss();
    }
    
    assert_eq!(stats.hit_rate(), 0.0);
    assert_eq!(stats.miss_rate(), 1.0);
}

/// Test cache stats perfect hit rate
#[test]
fn test_cache_stats_perfect_hit_rate() {
    let mut stats = CacheStats::new();
    
    for _ in 0..10 {
        stats.record_hit();
    }
    
    assert_eq!(stats.hit_rate(), 1.0);
    assert_eq!(stats.miss_rate(), 0.0);
}

/// Test row cache with binary data
#[test]
fn test_row_cache_binary_data_preservation() {
    let mut cache = RowCache::new();
    let binary_data = vec![0u8, 1, 2, 255, 254, 128, 127, 64];
    
    cache.get_or_insert("bin", binary_data.clone());
    assert_eq!(cache.get("bin"), Some(binary_data));
}

/// Test query cache with empty row indices
#[test]
fn test_query_cache_empty_row_indices() {
    let mut cache = QueryResultCache::new();
    cache.insert("q1".to_string(), vec![], 0.0);
    
    assert_eq!(cache.get("q1"), Some(vec![]));
    assert_eq!(cache.len(), 1);
}


/// Test caching layer memory info accuracy
#[test]
fn test_caching_layer_memory_info_accuracy() {
    let layer = CachingLayer::new();
    
    layer.insert_row("r1".to_string(), vec![0; 500]);
    layer.insert_row("r2".to_string(), vec![0; 300]);
    layer.insert_query("q1".to_string(), vec![0; 50], 0.5); // 50 * 8 = 400 bytes
    
    let (row_count, row_size) = layer.row_cache_info();
    let (query_count, query_memory) = layer.query_cache_info();
    
    assert_eq!(row_count, 2);
    assert_eq!(row_size, 800);
    assert_eq!(query_count, 1);
    assert_eq!(query_memory, 400);
}
