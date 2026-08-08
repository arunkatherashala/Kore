//! Distributed query planner — strategy detection (Phase 6 + Phase 10 broadcast).

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DistributedStrategy {
    Filter,
    GlobalAgg,
    GroupBy { keys: Vec<String> },
    ShuffleJoin,
    /// Broadcast the small side; each worker runs a local hash join.
    BroadcastJoin,
    Passthrough,
}

/// Row-count threshold below which a join build side is a broadcast candidate.
/// Set via `KORE_BROADCAST_ROWS`, default 100_000 rows.
///
/// This mirrors Spark's `spark.sql.autoBroadcastJoinThreshold` (default 10 MB
/// there — we use row count as a first-order proxy).
pub fn broadcast_row_threshold() -> usize {
    std::env::var("KORE_BROADCAST_ROWS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000)
}

/// Decide broadcast vs shuffle join given the sizes of both sides.
/// Returns `BroadcastJoin` when the smaller side fits under the threshold,
/// otherwise `ShuffleJoin`.
pub fn plan_join(left_rows: usize, right_rows: usize) -> DistributedStrategy {
    let smaller = left_rows.min(right_rows);
    if smaller <= broadcast_row_threshold() {
        DistributedStrategy::BroadcastJoin
    } else {
        DistributedStrategy::ShuffleJoin
    }
}

#[derive(Debug, Clone)]
pub struct DistributedPlan {
    pub strategy: DistributedStrategy,
    pub map_sql: String,
    pub reduce_sql: Option<String>,
    pub shuffle_keys: Vec<String>,
    pub use_local_tables: bool,
}

pub fn plan(sql: &str, table_name: &str) -> DistributedPlan {
    let lower = sql.to_lowercase();
    let use_local = kore_net::cluster_local_tables();

    if lower.contains("join") {
        return DistributedPlan {
            strategy: DistributedStrategy::ShuffleJoin,
            map_sql: sql.to_string(),
            reduce_sql: None,
            shuffle_keys: vec![],
            use_local_tables: use_local,
        };
    }

    let has_group = lower.contains("group by");
    let has_agg = lower.contains("sum(")
        || lower.contains("count(")
        || lower.contains("avg(")
        || lower.contains("min(")
        || lower.contains("max(");

    if has_group && has_agg {
        let keys = extract_group_keys(&lower);
        let reduce = crate::build_merge_sql(sql).replace("FROM data", "FROM merged");
        return DistributedPlan {
            strategy: DistributedStrategy::GroupBy { keys: keys.clone() },
            map_sql: sql.to_string(),
            reduce_sql: Some(reduce),
            shuffle_keys: keys,
            use_local_tables: use_local,
        };
    }

    if has_agg {
        let reduce = crate::build_merge_sql(sql).replace("FROM data", "FROM merged");
        return DistributedPlan {
            strategy: DistributedStrategy::GlobalAgg,
            map_sql: sql.to_string(),
            reduce_sql: Some(reduce),
            shuffle_keys: vec![],
            use_local_tables: use_local,
        };
    }

    DistributedPlan {
        strategy: DistributedStrategy::Filter,
        map_sql: rewrite_table(sql, table_name),
        reduce_sql: None,
        shuffle_keys: vec![],
        use_local_tables: use_local,
    }
}

fn extract_group_keys(lower: &str) -> Vec<String> {
    let Some(pos) = lower.rfind("group by") else {
        return vec![];
    };
    lower[pos + 8..]
        .split("order by")
        .next()
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().split_whitespace().next().unwrap_or("").to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn rewrite_table(sql: &str, table_name: &str) -> String {
    if sql.to_lowercase().contains("from data") {
        return sql.to_string();
    }
    sql.replace(
        &format!("FROM {table_name}"),
        &format!("FROM {table_name}"),
    )
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_group_by() {
        let p = plan(
            "SELECT region, SUM(sales) AS total FROM sales GROUP BY region",
            "sales",
        );
        assert_eq!(
            p.strategy,
            DistributedStrategy::GroupBy {
                keys: vec!["region".into()]
            }
        );
        assert!(p.reduce_sql.is_some());
        assert!(p.use_local_tables);
    }

    #[test]
    fn plans_filter() {
        let p = plan("SELECT * FROM sales WHERE sales > 100", "sales");
        assert_eq!(p.strategy, DistributedStrategy::Filter);
        assert!(p.reduce_sql.is_none());
    }

    #[test]
    fn plans_join_broadcast_when_small_side_fits() {
        std::env::remove_var("KORE_BROADCAST_ROWS");
        // 200 dim rows vs 10M fact rows → broadcast dim.
        assert_eq!(plan_join(10_000_000, 200), DistributedStrategy::BroadcastJoin);
        assert_eq!(plan_join(200, 10_000_000), DistributedStrategy::BroadcastJoin);
    }

    #[test]
    fn plans_join_shuffle_when_both_large() {
        std::env::remove_var("KORE_BROADCAST_ROWS");
        assert_eq!(plan_join(5_000_000, 3_000_000), DistributedStrategy::ShuffleJoin);
    }

    #[test]
    fn broadcast_threshold_respects_env() {
        std::env::set_var("KORE_BROADCAST_ROWS", "10");
        // Even 50 rows now exceeds a threshold of 10.
        assert_eq!(plan_join(1_000_000, 50), DistributedStrategy::ShuffleJoin);
        std::env::set_var("KORE_BROADCAST_ROWS", "10000");
        assert_eq!(plan_join(1_000_000, 50), DistributedStrategy::BroadcastJoin);
        std::env::remove_var("KORE_BROADCAST_ROWS");
    }
}
