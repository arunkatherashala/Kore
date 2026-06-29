//! Layer 15 — KoreJoin
//!
//! Three join strategies mirroring Spark SQL's physical plans:
//!   • Hash Join        — O(n) build + O(m) probe; best for medium tables
//!   • Broadcast Join   — broadcast the smaller side; ideal when one side fits in memory
//!   • Sort-Merge Join  — O(n log n + m log m); handles arbitrarily large tables

pub mod hash_join;
pub mod broadcast_join;
pub mod sort_merge_join;

pub use hash_join::HashJoin;
pub use broadcast_join::BroadcastJoin;
pub use sort_merge_join::SortMergeJoin;

use kore_core::JoinType;

/// Unified join configuration
#[derive(Debug, Clone)]
pub struct JoinConfig {
    pub left_key:  String,
    pub right_key: String,
    pub join_type: JoinType,
}

impl JoinConfig {
    pub fn new(left_key: &str, right_key: &str, join_type: JoinType) -> Self {
        Self {
            left_key:  left_key.into(),
            right_key: right_key.into(),
            join_type,
        }
    }

    pub fn inner(left_key: &str, right_key: &str) -> Self {
        Self::new(left_key, right_key, JoinType::Inner)
    }

    pub fn left(left_key: &str, right_key: &str) -> Self {
        Self::new(left_key, right_key, JoinType::Left)
    }
}
