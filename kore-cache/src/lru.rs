//! LRU (Least-Recently-Used) cache.
//!
//! Implementation: `HashMap<K, (V, u64)>` where the `u64` is a logical clock
//! timestamp.  Eviction finds the minimum-clock entry in O(capacity).
//! For production use-cases with large capacities, prefer the `lru` crate; this
//! implementation is zero-dependency and sufficient for the benchmark targets.

use std::collections::HashMap;
use std::hash::Hash;

pub struct LruCache<K, V> {
    capacity: usize,
    clock:    u64,
    map:      HashMap<K, (V, u64)>,
    hits:     u64,
    misses:   u64,
}

impl<K: Eq + Hash + Clone, V> LruCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "capacity must be > 0");
        Self {
            capacity,
            clock: 0,
            map: HashMap::with_capacity(capacity + 1),
            hits: 0,
            misses: 0,
        }
    }

    /// Look up `key`; updates access time on hit.
    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.clock += 1;
        let clock = self.clock;
        if let Some((v, ts)) = self.map.get_mut(key) {
            *ts = clock;
            self.hits += 1;
            Some(v)
        } else {
            self.misses += 1;
            None
        }
    }

    /// Insert / update `key → value`.  Evicts LRU entry when at capacity.
    pub fn put(&mut self, key: K, value: V) {
        self.clock += 1;
        if self.map.len() >= self.capacity && !self.map.contains_key(&key) {
            self.evict_lru();
        }
        self.map.insert(key, (value, self.clock));
    }

    pub fn contains(&self, key: &K) -> bool { self.map.contains_key(key) }
    pub fn len(&self)      -> usize { self.map.len() }
    pub fn is_empty(&self) -> bool  { self.map.is_empty() }
    pub fn hits(&self)     -> u64   { self.hits }
    pub fn misses(&self)   -> u64   { self.misses }
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 { 0.0 } else { self.hits as f64 / total as f64 }
    }

    /// Remove a key explicitly (e.g. on table mutation / cache invalidation).
    pub fn invalidate(&mut self, key: &K) -> bool {
        self.map.remove(key).is_some()
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    fn evict_lru(&mut self) {
        if let Some(lru_key) = self
            .map
            .iter()
            .min_by_key(|(_, (_, ts))| *ts)
            .map(|(k, _)| k.clone())
        {
            self.map.remove(&lru_key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_lru() {
        let mut c: LruCache<i32, &str> = LruCache::new(3);
        c.put(1, "a");
        c.put(2, "b");
        c.put(3, "c");
        assert_eq!(c.get(&1), Some(&"a")); // 1 is now most-recently used
        c.put(4, "d");                     // evicts 2 (LRU)
        assert!(c.get(&2).is_none());
        assert_eq!(c.get(&1), Some(&"a"));
        assert_eq!(c.get(&4), Some(&"d"));
    }

    #[test]
    fn hit_rate() {
        let mut c: LruCache<&str, i32> = LruCache::new(10);
        c.put("x", 1);
        c.get(&"x");
        c.get(&"y"); // miss
        assert!((c.hit_rate() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn invalidate() {
        let mut c: LruCache<i32, i32> = LruCache::new(4);
        c.put(1, 10);
        assert!(c.invalidate(&1));
        assert!(!c.contains(&1));
    }
}
