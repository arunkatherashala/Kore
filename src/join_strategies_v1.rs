/// Join Strategies for KORE v1.6.0
/// 
/// Multiple join algorithm implementations optimized for different scenarios:
/// - Nested Loop: Always correct, O(n*m), suitable for small tables
/// - Hash Join: Fast for in-memory, O(n+m), best when one side fits in memory
/// - Merge Join: For sorted data, O(n+m), good for large sorted sets
/// - Sort-Merge: Sorts then merges, most flexible, O(n log n + m log m)

use std::collections::HashMap;

/// Represents a row in a join operation (simplified as HashMap<String, String>)
pub type Row = HashMap<String, String>;

/// Different join strategy choices
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinStrategy {
    /// Nested loop: For each left row, scan all right rows
    /// - Pros: Always works, no memory requirements
    /// - Cons: O(n*m) complexity, very slow for large tables
    /// - Best for: Small inner relations, or when no better option
    NestedLoop,

    /// Hash Join: Build hash table on smaller side, probe with larger
    /// - Pros: O(n+m), fast for in-memory data
    /// - Cons: Requires hash function, memory for hash table
    /// - Best for: When one side fits in memory
    Hash,

    /// Merge Join: For pre-sorted data
    /// - Pros: O(n+m) after sort, cache-efficient
    /// - Cons: Requires sort or index
    /// - Best for: Sorted data or when multiple joins use same keys
    Merge,

    /// Sort-Merge: Sort both sides then merge
    /// - Pros: Most flexible, works for all data
    /// - Cons: Sort overhead, but can reuse sorted order
    /// - Best for: Complex join orders, multiple joins on same keys
    SortMerge,
}

impl JoinStrategy {
    pub fn name(&self) -> &'static str {
        match self {
            JoinStrategy::NestedLoop => "NestedLoop",
            JoinStrategy::Hash => "Hash",
            JoinStrategy::Merge => "Merge",
            JoinStrategy::SortMerge => "SortMerge",
        }
    }

    pub fn estimated_complexity(&self) -> &'static str {
        match self {
            JoinStrategy::NestedLoop => "O(n*m)",
            JoinStrategy::Hash => "O(n+m)",
            JoinStrategy::Merge => "O(n+m)",
            JoinStrategy::SortMerge => "O(n log n + m log m)",
        }
    }
}

/// Configuration thresholds for strategy selection
#[derive(Debug, Clone)]
pub struct JoinStrategyConfig {
    /// Rows below this use nested loop
    pub nested_loop_threshold: u64,
    /// Rows below this use hash join (if memory available)
    pub hash_join_threshold: u64,
    /// Bytes of memory available for hash join
    pub available_memory_bytes: u64,
}

impl Default for JoinStrategyConfig {
    fn default() -> Self {
        JoinStrategyConfig {
            nested_loop_threshold: 1_000,        // < 1K rows use nested loop
            hash_join_threshold: 10_000_000,    // < 10M rows use hash join
            available_memory_bytes: 1024 * 1024 * 1024, // 1GB available
        }
    }
}

/// Choose optimal join strategy based on table sizes and configuration
pub fn choose_join_strategy(
    left_rows: u64,
    right_rows: u64,
    config: &JoinStrategyConfig,
) -> JoinStrategy {
    let smaller = left_rows.min(right_rows);
    let larger = left_rows.max(right_rows);

    // Very small tables: nested loop is fine
    if larger < config.nested_loop_threshold {
        return JoinStrategy::NestedLoop;
    }

    // If either side is tiny, nested loop is good enough
    if smaller < 100 {
        return JoinStrategy::NestedLoop;
    }

    // If smaller side fits in available memory, use hash join
    let estimated_memory_bytes = (smaller * 64) as u64; // Rough estimate: 64 bytes per row
    if estimated_memory_bytes < config.available_memory_bytes && larger < config.hash_join_threshold {
        return JoinStrategy::Hash;
    }

    // Large tables: use sort-merge for better scaling
    JoinStrategy::SortMerge
}

/// Nested loop join: For each left row, scan all right rows and match
pub fn nested_loop_join(
    left: &[Row],
    right: &[Row],
    join_keys: &[String],
) -> Result<Vec<Row>, String> {
    let mut result = Vec::new();

    for left_row in left {
        for right_row in right {
            // Check if all join keys match
            let mut matches = true;
            for key in join_keys {
                let left_val = left_row.get(key);
                let right_val = right_row.get(key);
                if left_val != right_val {
                    matches = false;
                    break;
                }
            }

            if matches {
                // Merge rows
                let mut merged = left_row.clone();
                for (k, v) in right_row {
                    merged.insert(k.clone(), v.clone());
                }
                result.push(merged);
            }
        }
    }

    Ok(result)
}

/// Hash join: Build hash table on smaller side, probe with larger side
pub fn hash_join(
    left: &[Row],
    right: &[Row],
    join_keys: &[String],
) -> Result<Vec<Row>, String> {
    let mut result = Vec::new();

    // Decide which side to build on (smaller is better for hash table)
    let (build_side, probe_side, build_is_left) = if left.len() <= right.len() {
        (left, right, true)
    } else {
        (right, left, false)
    };

    // Build hash table from build side
    let mut hash_table: HashMap<Vec<String>, Vec<Row>> = HashMap::new();
    for row in build_side {
        let key: Vec<String> = join_keys
            .iter()
            .filter_map(|k| row.get(k).cloned())
            .collect();
        hash_table.entry(key).or_insert_with(Vec::new).push(row.clone());
    }

    // Probe hash table with probe side
    for probe_row in probe_side {
        let key: Vec<String> = join_keys
            .iter()
            .filter_map(|k| probe_row.get(k).cloned())
            .collect();

        if let Some(build_rows) = hash_table.get(&key) {
            for build_row in build_rows {
                let merged = if build_is_left {
                    let mut m = build_row.clone();
                    for (k, v) in probe_row {
                        m.insert(k.clone(), v.clone());
                    }
                    m
                } else {
                    let mut m = probe_row.clone();
                    for (k, v) in build_row {
                        m.insert(k.clone(), v.clone());
                    }
                    m
                };
                result.push(merged);
            }
        }
    }

    Ok(result)
}

/// Sort-merge join: Sort both sides by join keys, then merge
pub fn sort_merge_join(
    left: &[Row],
    right: &[Row],
    join_keys: &[String],
) -> Result<Vec<Row>, String> {
    let mut result = Vec::new();

    // Sort both sides by join keys
    let mut sorted_left = left.to_vec();
    let mut sorted_right = right.to_vec();

    sorted_left.sort_by(|a, b| {
        for key in join_keys {
            let a_val = a.get(key).map(|s| s.as_str()).unwrap_or("");
            let b_val = b.get(key).map(|s| s.as_str()).unwrap_or("");
            match a_val.cmp(b_val) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            }
        }
        std::cmp::Ordering::Equal
    });

    sorted_right.sort_by(|a, b| {
        for key in join_keys {
            let a_val = a.get(key).map(|s| s.as_str()).unwrap_or("");
            let b_val = b.get(key).map(|s| s.as_str()).unwrap_or("");
            match a_val.cmp(b_val) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            }
        }
        std::cmp::Ordering::Equal
    });

    // Merge: Two-pointer technique
    let mut left_idx = 0;
    let mut right_idx = 0;

    while left_idx < sorted_left.len() && right_idx < sorted_right.len() {
        let left_row = &sorted_left[left_idx];
        let right_row = &sorted_right[right_idx];

        // Get join key values
        let left_keys: Vec<String> = join_keys
            .iter()
            .filter_map(|k| left_row.get(k).cloned())
            .collect();
        let right_keys: Vec<String> = join_keys
            .iter()
            .filter_map(|k| right_row.get(k).cloned())
            .collect();

        match left_keys.cmp(&right_keys) {
            std::cmp::Ordering::Less => {
                left_idx += 1;
            }
            std::cmp::Ordering::Greater => {
                right_idx += 1;
            }
            std::cmp::Ordering::Equal => {
                // Found matching rows - collect all with same key
                let mut left_matches = vec![left_row.clone()];
                let mut temp_left_idx = left_idx + 1;
                while temp_left_idx < sorted_left.len() {
                    let next_row = &sorted_left[temp_left_idx];
                    let next_keys: Vec<String> = join_keys
                        .iter()
                        .filter_map(|k| next_row.get(k).cloned())
                        .collect();
                    if next_keys == left_keys {
                        left_matches.push(next_row.clone());
                        temp_left_idx += 1;
                    } else {
                        break;
                    }
                }
                left_idx = temp_left_idx;

                let mut right_matches = vec![right_row.clone()];
                let mut temp_right_idx = right_idx + 1;
                while temp_right_idx < sorted_right.len() {
                    let next_row = &sorted_right[temp_right_idx];
                    let next_keys: Vec<String> = join_keys
                        .iter()
                        .filter_map(|k| next_row.get(k).cloned())
                        .collect();
                    if next_keys == right_keys {
                        right_matches.push(next_row.clone());
                        temp_right_idx += 1;
                    } else {
                        break;
                    }
                }
                right_idx = temp_right_idx;

                // Cross product of matching rows
                for lm in &left_matches {
                    for rm in &right_matches {
                        let mut merged = lm.clone();
                        for (k, v) in rm {
                            merged.insert(k.clone(), v.clone());
                        }
                        result.push(merged);
                    }
                }
            }
        }
    }

    Ok(result)
}

/// Execute join with specified strategy
pub fn execute_join(
    left: &[Row],
    right: &[Row],
    join_keys: &[String],
    strategy: JoinStrategy,
) -> Result<Vec<Row>, String> {
    match strategy {
        JoinStrategy::NestedLoop => nested_loop_join(left, right, join_keys),
        JoinStrategy::Hash => hash_join(left, right, join_keys),
        JoinStrategy::Merge => sort_merge_join(left, right, join_keys),
        JoinStrategy::SortMerge => sort_merge_join(left, right, join_keys),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_row(key: &str, val: &str) -> Row {
        let mut m = HashMap::new();
        m.insert(key.to_string(), val.to_string());
        m
    }

    #[test]
    fn test_choose_strategy_small_tables() {
        let config = JoinStrategyConfig::default();
        let strategy = choose_join_strategy(10, 20, &config);
        assert_eq!(strategy, JoinStrategy::NestedLoop);
    }

    #[test]
    fn test_choose_strategy_medium_tables() {
        let config = JoinStrategyConfig::default();
        let strategy = choose_join_strategy(5_000, 1_000_000, &config);
        assert_eq!(strategy, JoinStrategy::Hash);
    }

    #[test]
    fn test_nested_loop_join() {
        let left = vec![make_row("id", "1"), make_row("id", "2")];
        let right = vec![make_row("id", "1"), make_row("id", "3")];
        let result = nested_loop_join(&left, &right, &vec!["id".to_string()]).unwrap();
        assert_eq!(result.len(), 1); // Only id=1 matches
    }

    #[test]
    fn test_hash_join() {
        let mut left = HashMap::new();
        left.insert("id".to_string(), "1".to_string());
        left.insert("name".to_string(), "Alice".to_string());

        let mut right = HashMap::new();
        right.insert("id".to_string(), "1".to_string());
        right.insert("age".to_string(), "30".to_string());

        let result = hash_join(&vec![left], &vec![right], &vec!["id".to_string()]).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].get("name").map(|s| s.as_str()), Some("Alice"));
        assert_eq!(result[0].get("age").map(|s| s.as_str()), Some("30"));
    }

    #[test]
    fn test_sort_merge_join() {
        let mut left1 = HashMap::new();
        left1.insert("id".to_string(), "1".to_string());

        let mut left2 = HashMap::new();
        left2.insert("id".to_string(), "2".to_string());

        let mut right1 = HashMap::new();
        right1.insert("id".to_string(), "1".to_string());

        let mut right2 = HashMap::new();
        right2.insert("id".to_string(), "2".to_string());

        let result = sort_merge_join(
            &vec![left2.clone(), left1.clone()],
            &vec![right2.clone(), right1.clone()],
            &vec!["id".to_string()],
        ).unwrap();
        
        assert_eq!(result.len(), 2); // Both match
    }

    #[test]
    fn test_strategy_name() {
        assert_eq!(JoinStrategy::Hash.name(), "Hash");
        assert_eq!(JoinStrategy::NestedLoop.name(), "NestedLoop");
    }

    #[test]
    fn test_strategy_complexity() {
        assert_eq!(JoinStrategy::Hash.estimated_complexity(), "O(n+m)");
        assert_eq!(JoinStrategy::NestedLoop.estimated_complexity(), "O(n*m)");
    }
}
