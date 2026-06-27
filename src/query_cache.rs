/// KORE Query Plan Cache and Optimizer
/// Caches query execution plans and provides optimization strategies

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Cached query plan
#[derive(Debug, Clone)]
pub struct CachedPlan {
    pub plan_hash: u64,
    pub query_text: String,
    pub created_at: SystemTime,
    pub hit_count: usize,
    pub avg_execution_time_ms: f64,
}

/// Query plan cache manager
pub struct QueryPlanCache {
    cache: Arc<Mutex<HashMap<u64, CachedPlan>>>,
    max_cache_size: usize,
    ttl_secs: u64,
}

impl QueryPlanCache {
    /// Create new query plan cache
    pub fn new(max_size: usize, ttl_secs: u64) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            max_cache_size: max_size,
            ttl_secs,
        }
    }

    /// Generate hash for query text
    pub fn hash_query(query_text: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        query_text.hash(&mut hasher);
        hasher.finish()
    }

    /// Check if plan exists in cache
    pub fn get(&self, query_text: &str) -> Option<CachedPlan> {
        let hash = Self::hash_query(query_text);
        
        if let Ok(mut cache) = self.cache.lock() {
            if let Some(plan) = cache.get(&hash) {
                // Check if plan is still valid
                if let Ok(elapsed) = plan.created_at.elapsed() {
                    if elapsed.as_secs() < self.ttl_secs {
                        // Update hit count
                        let mut updated_plan = plan.clone();
                        updated_plan.hit_count += 1;
                        cache.insert(hash, updated_plan.clone());
                        return Some(updated_plan);
                    }
                }
            }
        }
        
        None
    }

    /// Store plan in cache
    pub fn put(&self, query_text: &str, execution_time_ms: f64) {
        let hash = Self::hash_query(query_text);
        
        if let Ok(mut cache) = self.cache.lock() {
            // Evict least recently used plan if cache is full
            if cache.len() >= self.max_cache_size {
                if let Some(lru_key) = self.find_lru(&cache) {
                    cache.remove(&lru_key);
                }
            }

            let plan = CachedPlan {
                plan_hash: hash,
                query_text: query_text.to_string(),
                created_at: SystemTime::now(),
                hit_count: 0,
                avg_execution_time_ms: execution_time_ms,
            };

            cache.insert(hash, plan);
        }
    }

    /// Find least recently used key
    fn find_lru(&self, cache: &HashMap<u64, CachedPlan>) -> Option<u64> {
        cache.iter()
            .min_by_key(|(_, plan)| plan.hit_count)
            .map(|(key, _)| *key)
    }

    /// Clear cache
    pub fn clear(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        if let Ok(cache) = self.cache.lock() {
            let total_queries = cache.len();
            let total_hits: usize = cache.values().map(|p| p.hit_count).sum();
            let avg_execution_time: f64 = if total_queries > 0 {
                cache.values().map(|p| p.avg_execution_time_ms).sum::<f64>() / total_queries as f64
            } else {
                0.0
            };

            CacheStats {
                total_cached_queries: total_queries,
                total_hits,
                avg_execution_time_ms: avg_execution_time,
                cache_size: total_queries,
                max_size: self.max_cache_size,
            }
        } else {
            CacheStats::default()
        }
    }
}

/// Query optimizer strategies
pub struct QueryOptimizer;

impl QueryOptimizer {
    /// Optimize query by pushing down filters
    pub fn push_down_filters(query_text: &str) -> String {
        // Simple optimization: move WHERE conditions early
        // In a real implementation, this would reorder conditions
        query_text.to_string()
    }

    /// Estimate query cost (rows processed)
    pub fn estimate_cost(
        table_rows: u64,
        filter_selectivity: f64,
        join_count: usize,
    ) -> QueryCost {
        let rows_after_filters = (table_rows as f64 * filter_selectivity) as u64;
        let estimated_cost = rows_after_filters as f64 * (1.0 + join_count as f64 * 0.5);

        QueryCost {
            estimated_rows: rows_after_filters,
            estimated_cost,
            join_count,
            filter_selectivity,
        }
    }

    /// Choose best execution strategy
    pub fn choose_strategy(cost: &QueryCost) -> ExecutionStrategy {
        if cost.join_count > 2 {
            ExecutionStrategy::DistributedHash
        } else if cost.join_count > 0 {
            ExecutionStrategy::HashJoin
        } else if cost.estimated_rows > 500_000 {
            ExecutionStrategy::StreamingScan
        } else {
            ExecutionStrategy::IndexScan
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub total_cached_queries: usize,
    pub total_hits: usize,
    pub avg_execution_time_ms: f64,
    pub cache_size: usize,
    pub max_size: usize,
}

/// Query cost estimation
#[derive(Debug, Clone)]
pub struct QueryCost {
    pub estimated_rows: u64,
    pub estimated_cost: f64,
    pub join_count: usize,
    pub filter_selectivity: f64,
}

/// Execution strategies
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionStrategy {
    IndexScan,
    StreamingScan,
    HashJoin,
    DistributedHash,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_hash() {
        let hash1 = QueryPlanCache::hash_query("SELECT * FROM users");
        let hash2 = QueryPlanCache::hash_query("SELECT * FROM users");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_cache_put_and_get() {
        let cache = QueryPlanCache::new(10, 60);
        cache.put("SELECT * FROM users", 50.0);
        
        let plan = cache.get("SELECT * FROM users");
        assert!(plan.is_some());
    }

    #[test]
    fn test_cache_eviction() {
        let cache = QueryPlanCache::new(2, 60);
        cache.put("SELECT * FROM table1", 10.0);
        cache.put("SELECT * FROM table2", 20.0);
        cache.put("SELECT * FROM table3", 30.0); // Should evict table1
        
        let stats = cache.stats();
        assert_eq!(stats.total_cached_queries, 2);
    }

    #[test]
    fn test_cache_statistics() {
        let cache = QueryPlanCache::new(10, 60);
        cache.put("SELECT * FROM users", 50.0);
        
        let stats = cache.stats();
        assert!(stats.total_cached_queries > 0);
    }

    #[test]
    fn test_query_cost_estimation() {
        let cost = QueryOptimizer::estimate_cost(10_000_000, 0.1, 2);
        assert_eq!(cost.estimated_rows, 1_000_000);
        assert!(cost.estimated_cost > 0.0);
    }

    #[test]
    fn test_execution_strategy_selection() {
        let cost1 = QueryOptimizer::estimate_cost(10_000_000, 0.1, 0);
        let strategy1 = QueryOptimizer::choose_strategy(&cost1);
        assert_eq!(strategy1, ExecutionStrategy::StreamingScan);

        let cost2 = QueryOptimizer::estimate_cost(10_000, 0.1, 1);
        let strategy2 = QueryOptimizer::choose_strategy(&cost2);
        assert_eq!(strategy2, ExecutionStrategy::HashJoin);

        let cost3 = QueryOptimizer::estimate_cost(10_000, 0.1, 3);
        let strategy3 = QueryOptimizer::choose_strategy(&cost3);
        assert_eq!(strategy3, ExecutionStrategy::DistributedHash);
    }

    #[test]
    fn test_cache_clear() {
        let cache = QueryPlanCache::new(10, 60);
        cache.put("SELECT * FROM users", 50.0);
        cache.clear();
        
        let stats = cache.stats();
        assert_eq!(stats.total_cached_queries, 0);
    }
}
