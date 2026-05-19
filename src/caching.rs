use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::predicates::QueryFilter;

/// Maximum cache size in bytes (default 100MB)
const DEFAULT_MAX_CACHE_SIZE: u64 = 100 * 1024 * 1024;

/// Row-level LRU cache for hot data
#[derive(Debug, Clone)]
pub struct RowCache {
    max_size: u64,
    current_size: u64,
    entries: HashMap<String, CacheEntry>,
    lru_order: Vec<String>, // Track access order for LRU eviction
}

#[derive(Debug, Clone)]
struct CacheEntry {
    data: Vec<u8>,
    size: u64,
    access_count: u64,
}

impl RowCache {
    /// Create new row cache with default size (100MB)
    pub fn new() -> Self {
        Self {
            max_size: DEFAULT_MAX_CACHE_SIZE,
            current_size: 0,
            entries: HashMap::new(),
            lru_order: Vec::new(),
        }
    }

    /// Create new row cache with custom max size
    pub fn with_size(max_size: u64) -> Self {
        Self {
            max_size,
            current_size: 0,
            entries: HashMap::new(),
            lru_order: Vec::new(),
        }
    }

    /// Add data to cache (or retrieve if exists)
    pub fn get_or_insert(&mut self, key: &str, data: Vec<u8>) -> Vec<u8> {
        let size = data.len() as u64;

        // If key exists, update access and return
        if let Some(entry) = self.entries.get_mut(key) {
            entry.access_count += 1;
            // Move to end of LRU order
            if let Some(pos) = self.lru_order.iter().position(|k| k == key) {
                self.lru_order.remove(pos);
            }
            self.lru_order.push(key.to_string());
            return entry.data.clone();
        }

        // Evict if necessary
        while self.current_size + size > self.max_size && !self.lru_order.is_empty() {
            self.evict_oldest();
        }

        // Insert new entry
        self.entries.insert(
            key.to_string(),
            CacheEntry {
                data: data.clone(),
                size,
                access_count: 1,
            },
        );
        self.lru_order.push(key.to_string());
        self.current_size += size;

        data
    }

    /// Get data from cache if exists
    pub fn get(&mut self, key: &str) -> Option<Vec<u8>> {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.access_count += 1;
            // Move to end of LRU order
            if let Some(pos) = self.lru_order.iter().position(|k| k == key) {
                self.lru_order.remove(pos);
            }
            self.lru_order.push(key.to_string());
            Some(entry.data.clone())
        } else {
            None
        }
    }

    /// Evict least recently used entry
    fn evict_oldest(&mut self) {
        if !self.lru_order.is_empty() {
            let key = self.lru_order.remove(0);
            if let Some(entry) = self.entries.remove(&key) {
                self.current_size -= entry.size;
            }
        }
    }

    /// Clear entire cache
    pub fn clear(&mut self) {
        self.entries.clear();
        self.lru_order.clear();
        self.current_size = 0;
    }

    /// Get cache size in bytes
    pub fn size_bytes(&self) -> u64 {
        self.current_size
    }

    /// Get number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get entry count for testing
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for RowCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Query result cache with selectivity-based TTL
#[derive(Debug, Clone)]
pub struct QueryResultCache {
    max_entries: usize,
    entries: HashMap<String, QueryCacheEntry>,
    lru_order: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct QueryCacheEntry {
    pub row_indices: Vec<usize>,
    pub selectivity: f64,
    pub access_count: u64,
    pub size_estimate: u64,
}

impl QueryResultCache {
    /// Create new query result cache
    pub fn new() -> Self {
        Self {
            max_entries: 1000,
            entries: HashMap::new(),
            lru_order: Vec::new(),
        }
    }

    /// Create with custom max entries
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            max_entries,
            entries: HashMap::new(),
            lru_order: Vec::new(),
        }
    }

    /// Generate cache key from filter
    pub fn make_key(filter: &QueryFilter) -> String {
        format!("{:?}", filter)
    }

    /// Get cached query result
    pub fn get(&mut self, key: &str) -> Option<Vec<usize>> {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.access_count += 1;
            // Move to end of LRU order
            if let Some(pos) = self.lru_order.iter().position(|k| k == key) {
                self.lru_order.remove(pos);
            }
            self.lru_order.push(key.to_string());
            Some(entry.row_indices.clone())
        } else {
            None
        }
    }

    /// Insert query result
    pub fn insert(&mut self, key: String, row_indices: Vec<usize>, selectivity: f64) {
        let size_estimate = (row_indices.len() * 8) as u64; // Estimate: 8 bytes per index

        // Evict if at capacity
        while self.entries.len() >= self.max_entries && !self.lru_order.is_empty() {
            self.evict_oldest();
        }

        self.entries.insert(
            key.clone(),
            QueryCacheEntry {
                row_indices,
                selectivity,
                access_count: 1,
                size_estimate,
            },
        );
        self.lru_order.push(key);
    }

    /// Evict oldest entry
    fn evict_oldest(&mut self) {
        if !self.lru_order.is_empty() {
            let key = self.lru_order.remove(0);
            self.entries.remove(&key);
        }
    }

    /// Clear entire cache
    pub fn clear(&mut self) {
        self.entries.clear();
        self.lru_order.clear();
    }

    /// Get number of cached queries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Estimate total memory usage
    pub fn memory_usage(&self) -> u64 {
        self.entries.values().map(|e| e.size_estimate).sum()
    }

    /// Get entry by key (for testing)
    pub fn get_entry(&self, key: &str) -> Option<&QueryCacheEntry> {
        self.entries.get(key)
    }
}

impl Default for QueryResultCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache statistics and performance tracking
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_gets: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub total_evictions: u64,
    pub peak_memory_bytes: u64,
    pub current_memory_bytes: u64,
}

impl CacheStats {
    /// Create new stats tracker
    pub fn new() -> Self {
        Self {
            total_gets: 0,
            cache_hits: 0,
            cache_misses: 0,
            total_evictions: 0,
            peak_memory_bytes: 0,
            current_memory_bytes: 0,
        }
    }

    /// Record a cache hit
    pub fn record_hit(&mut self) {
        self.total_gets += 1;
        self.cache_hits += 1;
    }

    /// Record a cache miss
    pub fn record_miss(&mut self) {
        self.total_gets += 1;
        self.cache_misses += 1;
    }

    /// Record an eviction
    pub fn record_eviction(&mut self) {
        self.total_evictions += 1;
    }

    /// Record memory update
    pub fn record_memory(&mut self, bytes: u64) {
        self.current_memory_bytes = bytes;
        if bytes > self.peak_memory_bytes {
            self.peak_memory_bytes = bytes;
        }
    }

    /// Calculate hit rate (0.0-1.0)
    pub fn hit_rate(&self) -> f64 {
        if self.total_gets == 0 {
            return 0.0;
        }
        self.cache_hits as f64 / self.total_gets as f64
    }

    /// Calculate miss rate (0.0-1.0)
    pub fn miss_rate(&self) -> f64 {
        if self.total_gets == 0 {
            return 0.0;
        }
        self.cache_misses as f64 / self.total_gets as f64
    }

    /// Reset all stats
    pub fn reset(&mut self) {
        self.total_gets = 0;
        self.cache_hits = 0;
        self.cache_misses = 0;
        self.total_evictions = 0;
        self.peak_memory_bytes = 0;
        self.current_memory_bytes = 0;
    }
}

impl Default for CacheStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe caching layer combining all components
#[derive(Debug, Clone)]
pub struct CachingLayer {
    row_cache: Arc<Mutex<RowCache>>,
    query_cache: Arc<Mutex<QueryResultCache>>,
    stats: Arc<Mutex<CacheStats>>,
}

impl CachingLayer {
    /// Create new caching layer
    pub fn new() -> Self {
        Self {
            row_cache: Arc::new(Mutex::new(RowCache::new())),
            query_cache: Arc::new(Mutex::new(QueryResultCache::new())),
            stats: Arc::new(Mutex::new(CacheStats::new())),
        }
    }

    /// Create with custom row cache size
    pub fn with_row_cache_size(size: u64) -> Self {
        Self {
            row_cache: Arc::new(Mutex::new(RowCache::with_size(size))),
            query_cache: Arc::new(Mutex::new(QueryResultCache::new())),
            stats: Arc::new(Mutex::new(CacheStats::new())),
        }
    }

    /// Get row from cache (with stats tracking)
    pub fn get_row(&self, key: &str) -> Option<Vec<u8>> {
        let mut row_cache = self.row_cache.lock().unwrap();
        let result = row_cache.get(key);
        
        let mut stats = self.stats.lock().unwrap();
        if result.is_some() {
            stats.record_hit();
        } else {
            stats.record_miss();
        }
        
        result
    }

    /// Insert row into cache (with stats tracking)
    pub fn insert_row(&self, key: String, data: Vec<u8>) {
        let mut row_cache = self.row_cache.lock().unwrap();
        row_cache.get_or_insert(&key, data);
        
        let mut stats = self.stats.lock().unwrap();
        stats.record_memory(row_cache.size_bytes());
    }

    /// Get cached query result (with stats)
    pub fn get_query(&self, key: &str) -> Option<Vec<usize>> {
        let mut query_cache = self.query_cache.lock().unwrap();
        let result = query_cache.get(key);
        
        let mut stats = self.stats.lock().unwrap();
        if result.is_some() {
            stats.record_hit();
        } else {
            stats.record_miss();
        }
        
        result
    }

    /// Insert query result (with stats)
    pub fn insert_query(&self, key: String, row_indices: Vec<usize>, selectivity: f64) {
        let mut query_cache = self.query_cache.lock().unwrap();
        query_cache.insert(key, row_indices, selectivity);
        
        let mut stats = self.stats.lock().unwrap();
        stats.record_memory(query_cache.memory_usage());
    }

    /// Get cache statistics
    pub fn get_stats(&self) -> CacheStats {
        self.stats.lock().unwrap().clone()
    }

    /// Clear all caches
    pub fn clear_all(&self) {
        self.row_cache.lock().unwrap().clear();
        self.query_cache.lock().unwrap().clear();
        self.stats.lock().unwrap().reset();
    }

    /// Get row cache stats
    pub fn row_cache_info(&self) -> (usize, u64) {
        let cache = self.row_cache.lock().unwrap();
        (cache.len(), cache.size_bytes())
    }

    /// Get query cache stats
    pub fn query_cache_info(&self) -> (usize, u64) {
        let cache = self.query_cache.lock().unwrap();
        (cache.len(), cache.memory_usage())
    }
}

impl Default for CachingLayer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_row_cache_insert_and_get() {
        let mut cache = RowCache::new();
        let data = vec![1, 2, 3, 4, 5];
        cache.get_or_insert("key1", data.clone());
        assert_eq!(cache.get("key1"), Some(data));
    }

    #[test]
    fn test_row_cache_lru_eviction() {
        let mut cache = RowCache::with_size(100);
        cache.get_or_insert("key1", vec![0; 50]);
        cache.get_or_insert("key2", vec![0; 50]);
        cache.get_or_insert("key3", vec![0; 50]); // Should evict key1
        assert_eq!(cache.get("key1"), None); // key1 evicted
        assert!(cache.get("key2").is_some());
        assert!(cache.get("key3").is_some());
    }

    #[test]
    fn test_row_cache_size_tracking() {
        let mut cache = RowCache::new();
        cache.get_or_insert("key1", vec![0; 1000]);
        assert_eq!(cache.size_bytes(), 1000);
        cache.get_or_insert("key2", vec![0; 500]);
        assert_eq!(cache.size_bytes(), 1500);
    }

    #[test]
    fn test_row_cache_empty() {
        let cache = RowCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_query_cache_insert_get() {
        let mut cache = QueryResultCache::new();
        let indices = vec![0, 1, 2, 3];
        cache.insert("filter1".to_string(), indices.clone(), 0.5);
        assert_eq!(cache.get("filter1"), Some(indices));
    }

    #[test]
    fn test_query_cache_capacity_eviction() {
        let mut cache = QueryResultCache::with_capacity(2);
        cache.insert("q1".to_string(), vec![1, 2], 0.5);
        cache.insert("q2".to_string(), vec![3, 4], 0.5);
        cache.insert("q3".to_string(), vec![5, 6], 0.5); // Should evict q1
        assert_eq!(cache.get("q1"), None);
        assert!(cache.get("q2").is_some());
        assert!(cache.get("q3").is_some());
    }

    #[test]
    fn test_cache_stats_hit_rate() {
        let mut stats = CacheStats::new();
        stats.record_hit();
        stats.record_hit();
        stats.record_miss();
        assert!((stats.hit_rate() - (2.0 / 3.0)).abs() < 0.01);
        assert!((stats.miss_rate() - (1.0 / 3.0)).abs() < 0.01);
    }

    #[test]
    fn test_cache_stats_evictions() {
        let mut stats = CacheStats::new();
        stats.record_eviction();
        stats.record_eviction();
        assert_eq!(stats.total_evictions, 2);
    }

    #[test]
    fn test_cache_stats_memory_tracking() {
        let mut stats = CacheStats::new();
        stats.record_memory(1000);
        stats.record_memory(2000);
        assert_eq!(stats.current_memory_bytes, 2000);
        assert_eq!(stats.peak_memory_bytes, 2000);
    }

    #[test]
    fn test_caching_layer_row_operations() {
        let layer = CachingLayer::new();
        let data = vec![1, 2, 3];
        layer.insert_row("row1".to_string(), data.clone());
        assert_eq!(layer.get_row("row1"), Some(data));
    }

    #[test]
    fn test_caching_layer_query_operations() {
        let layer = CachingLayer::new();
        let indices = vec![0, 10, 20];
        layer.insert_query("filter1".to_string(), indices.clone(), 0.3);
        assert_eq!(layer.get_query("filter1"), Some(indices));
    }

    #[test]
    fn test_caching_layer_stats() {
        let layer = CachingLayer::new();
        layer.insert_row("row1".to_string(), vec![1, 2, 3]);
        layer.get_row("row1"); // hit
        layer.get_row("row2"); // miss
        let stats = layer.get_stats();
        assert_eq!(stats.cache_hits, 1);
        assert_eq!(stats.cache_misses, 1);
        assert!((stats.hit_rate() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_row_cache_access_order() {
        let mut cache = RowCache::new();
        cache.get_or_insert("key1", vec![1]);
        cache.get_or_insert("key2", vec![2]);
        cache.get("key1"); // Access key1
        assert_eq!(cache.lru_order, vec!["key2", "key1"]); // key1 moved to end
    }

    #[test]
    fn test_query_cache_memory_estimation() {
        let mut cache = QueryResultCache::new();
        let indices = vec![0; 100]; // 100 indices
        cache.insert("q1".to_string(), indices, 0.5);
        let entry = cache.get_entry("q1").unwrap();
        assert_eq!(entry.size_estimate, 800); // 100 * 8 bytes
    }

    #[test]
    fn test_row_cache_update_existing_key() {
        let mut cache = RowCache::new();
        let data1 = vec![1, 2];
        let data2 = vec![3, 4];
        let key1_copy = data1.clone();
        cache.get_or_insert("key1", data1);
        let result = cache.get_or_insert("key1", data2.clone());
        assert_eq!(result, key1_copy); // Returns old data, doesn't update
        assert_eq!(cache.len(), 1); // Only 1 entry
    }

    #[test]
    fn test_row_cache_clear() {
        let mut cache = RowCache::new();
        cache.get_or_insert("key1", vec![1, 2, 3]);
        cache.get_or_insert("key2", vec![4, 5, 6]);
        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.size_bytes(), 0);
    }

    #[test]
    fn test_query_cache_selectivity_tracking() {
        let mut cache = QueryResultCache::new();
        cache.insert("q1".to_string(), vec![1, 2], 0.25);
        cache.insert("q2".to_string(), vec![1, 2, 3, 4], 0.5);
        let entry1 = cache.get_entry("q1").unwrap();
        let entry2 = cache.get_entry("q2").unwrap();
        assert!((entry1.selectivity - 0.25).abs() < 0.01);
        assert!((entry2.selectivity - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_caching_layer_clear_all() {
        let layer = CachingLayer::new();
        layer.insert_row("r1".to_string(), vec![1]);
        layer.insert_query("q1".to_string(), vec![0], 0.5);
        layer.get_row("r1");
        layer.clear_all();
        assert_eq!(layer.row_cache_info(), (0, 0));
        assert_eq!(layer.query_cache_info(), (0, 0));
        let stats = layer.get_stats();
        assert_eq!(stats.cache_hits, 0);
    }

    #[test]
    fn test_cache_stats_reset() {
        let mut stats = CacheStats::new();
        stats.record_hit();
        stats.record_miss();
        stats.record_eviction();
        stats.record_memory(1000);
        stats.reset();
        assert_eq!(stats.cache_hits, 0);
        assert_eq!(stats.cache_misses, 0);
        assert_eq!(stats.total_evictions, 0);
    }

    #[test]
    fn test_row_cache_large_data() {
        let mut cache = RowCache::with_size(10000);
        let large_data = vec![0u8; 5000];
        cache.get_or_insert("key1", large_data.clone());
        assert_eq!(cache.size_bytes(), 5000);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_query_cache_default() {
        let cache = QueryResultCache::default();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_caching_layer_row_cache_info() {
        let layer = CachingLayer::new();
        layer.insert_row("r1".to_string(), vec![1, 2, 3]);
        layer.insert_row("r2".to_string(), vec![4, 5]);
        let (count, size) = layer.row_cache_info();
        assert_eq!(count, 2);
        assert!(size > 0);
    }

    #[test]
    fn test_caching_layer_query_cache_info() {
        let layer = CachingLayer::new();
        layer.insert_query("q1".to_string(), vec![0, 1], 0.5);
        let (count, memory) = layer.query_cache_info();
        assert_eq!(count, 1);
        assert_eq!(memory, 16); // 2 indices * 8 bytes
    }

    #[test]
    fn test_row_cache_access_count() {
        let mut cache = RowCache::new();
        cache.get_or_insert("key1", vec![1]);
        cache.get("key1");
        cache.get("key1");
        let entry = cache.entries.get("key1").unwrap();
        assert_eq!(entry.access_count, 3); // 1 initial + 2 gets
    }

    #[test]
    fn test_query_cache_access_count() {
        let mut cache = QueryResultCache::new();
        cache.insert("q1".to_string(), vec![0], 0.5);
        cache.get("q1");
        cache.get("q1");
        let entry = cache.get_entry("q1").unwrap();
        assert_eq!(entry.access_count, 3); // 1 initial + 2 gets
    }

    #[test]
    fn test_cache_stats_multiple_operations() {
        let mut stats = CacheStats::new();
        for _ in 0..10 {
            stats.record_hit();
        }
        for _ in 0..5 {
            stats.record_miss();
        }
        assert_eq!(stats.total_gets, 15);
        assert_eq!(stats.cache_hits, 10);
        assert_eq!(stats.cache_misses, 5);
        assert!((stats.hit_rate() - (10.0 / 15.0)).abs() < 0.01);
    }
}
