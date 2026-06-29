//! Layer 16 — KoreCache
//!
//! • `LruCache<K,V>`       — generic, thread-safe LRU with O(1) amortised ops
//! • `QueryCache`          — typed alias for caching DataBlock query results
//! • `MatViewRegistry`     — named materialised views with staleness tracking

pub mod lru;
pub mod matview;

pub use lru::LruCache;
pub use matview::{MatView, MatViewRegistry};

use kore_core::DataBlock;
use std::sync::{Arc, Mutex};

/// Thread-safe query result cache keyed by SQL/query fingerprint.
pub type QueryCache = Arc<Mutex<LruCache<String, DataBlock>>>;

/// Convenience constructor.
pub fn new_query_cache(capacity: usize) -> QueryCache {
    Arc::new(Mutex::new(LruCache::new(capacity)))
}
