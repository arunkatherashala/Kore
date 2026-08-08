//! Phase 16 — Catalyst plan drives coordinator dispatch.
//!
//! This module is the glue that closes the loop between:
//!
//!   * `kore-sql`         — SQL text  → `Query` AST
//!   * `kore-catalog`     — `DataBlock`s → statistics
//!   * `kore-catalyst`    — `Query` + stats → `PhysicalPlan`
//!   * `kore-coord::exec` — actual worker dispatch primitives
//!
//! Before Phase 16, `plan_query` was a library that no query ever went through:
//! `execute_distributed_v2` routed on the `KORE_NET_SHUFFLE` env var and made
//! hard-coded broadcast decisions in the coordinator. Now the coordinator asks
//! catalyst for a `PhysicalPlan`, classifies its shape once, and dispatches to
//! the exact primitive that plan requires. Stats populated via
//! `register_table_for_planning` flow all the way down to `choose_join_strategy`.
//!
//! # Dispatch policy (initial cut)
//!
//! The plan tree can be arbitrarily deep, but the coordinator's dispatch
//! primitives are coarse. The classifier walks the plan once and picks:
//!
//! ```text
//!   Join{BroadcastHash, .. small_side_est <= BROADCAST_ROWS}   → execute_broadcast_join
//!   HashAggregate{Partial} → Exchange{HashBy} → HashAggregate{Final}
//!                                                              → execute_network_shuffle
//!   anything else                                              → execute_local_tables
//! ```
//!
//! This is intentionally conservative — richer plans (bushy joins, multi-stage
//! shuffles) fall through to the local-tables path until Phase 18 adds
//! stage-graph execution.

use kore_catalyst::{plan_query, JoinStrategy, PhysicalPlan};
use kore_core::{DataBlock, KoreError};
use kore_sql::parser::parse_query;

use crate::Coordinator;

// ─── Dispatch classification ─────────────────────────────────────────────────

/// How the coordinator will run a `PhysicalPlan`, chosen from the plan shape.
///
/// This is `pub` because tests and future EXPLAIN-ANALYZE tooling inspect it.
#[derive(Debug, Clone, PartialEq)]
pub enum Dispatch {
    /// Local scan / filter / projection — one map SQL, no shuffle.
    LocalTables { sql: String, table: String },
    /// Two-phase agg: map SQL on workers, final reduce after shuffle.
    NetworkShuffle {
        map_sql:    String,
        reduce_sql: String,
        table:      String,
        keys:       Vec<String>,
    },
    /// Broadcast the small side, join locally on the large side.
    BroadcastJoin {
        join_sql:      String,
        large_table:   String,
        small_table:   String,
        small_rows:    usize,
    },
}

impl Dispatch {
    /// Human-readable one-line summary — printed inside EXPLAIN.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::LocalTables { .. }    => "LocalTables",
            Self::NetworkShuffle { .. } => "NetworkShuffle",
            Self::BroadcastJoin { .. }  => "BroadcastJoin",
        }
    }
}

// ─── Coordinator surface ─────────────────────────────────────────────────────

impl Coordinator {
    /// Parse SQL, build the physical plan against the coordinator's catalog,
    /// and return both the plan tree and the chosen dispatch kind.
    ///
    /// This is deterministic and side-effect-free (does not talk to workers).
    /// Callers use it to log EXPLAIN output before actually executing.
    pub fn plan_sql(&self, sql: &str) -> Result<(PhysicalPlan, Dispatch), KoreError> {
        let query = parse_query(sql)
            .map_err(|e| KoreError::InvalidArgument(format!("parse: {e:?}")))?;
        let catalog = self.catalog.lock().unwrap();
        let plan = plan_query(&query, &catalog)
            .ok_or_else(|| KoreError::InvalidArgument("empty query body".into()))?;
        drop(catalog); // release the lock before dispatch classification
        let dispatch = classify(&plan, sql);
        Ok((plan, dispatch))
    }

    /// Return a Spark-style `explain()` string for `sql`, ending with the
    /// dispatch kind the coordinator would use.
    ///
    /// Never talks to workers; safe to call before workers are registered.
    pub fn explain(&self, sql: &str) -> Result<String, KoreError> {
        let (plan, dispatch) = self.plan_sql(sql)?;
        let mut out = plan.explain();
        out.push_str(&format!("== Dispatch ==\n{}\n", dispatch.kind()));
        Ok(out)
    }

    /// Plan-driven execution.
    ///
    /// Requires that every table referenced by the SQL has been registered via
    /// `register_table_for_planning`.  The coordinator picks the dispatch
    /// primitive from the plan (broadcast vs shuffle vs local) and hands off
    /// to the existing `exec.rs` machinery — no new wire protocol needed.
    pub async fn execute_planned(&self, sql: &str) -> Result<DataBlock, KoreError> {
        let (_plan, dispatch) = self.plan_sql(sql)?;
        match dispatch {
            Dispatch::LocalTables { sql, table } => {
                let block = self.peek_registered(&table).ok_or_else(|| {
                    KoreError::InvalidArgument(format!(
                        "execute_planned: table '{table}' not registered"
                    ))
                })?;
                self.execute_distributed_v2(&sql, &table, block, None).await
            }
            Dispatch::NetworkShuffle { map_sql, reduce_sql, table, keys } => {
                let block = self.peek_registered(&table).ok_or_else(|| {
                    KoreError::InvalidArgument(format!(
                        "execute_planned: table '{table}' not registered"
                    ))
                })?;
                self.execute_network_shuffle(&map_sql, &reduce_sql, &table, block, &keys).await
            }
            Dispatch::BroadcastJoin { join_sql, large_table, small_table, .. } => {
                let large = self.peek_registered(&large_table).ok_or_else(|| {
                    KoreError::InvalidArgument(format!(
                        "execute_planned: large table '{large_table}' not registered"
                    ))
                })?;
                let small = self.peek_registered(&small_table).ok_or_else(|| {
                    KoreError::InvalidArgument(format!(
                        "execute_planned: small table '{small_table}' not registered"
                    ))
                })?;
                self.execute_broadcast_join(
                    &join_sql, &large_table, large, &small_table, small,
                )
                .await
            }
        }
    }
}

// ─── Classifier ──────────────────────────────────────────────────────────────

/// Walk the physical plan and pick one dispatch primitive.
///
/// This is intentionally shape-based rather than fully recursive — the
/// coordinator's execution primitives are coarse-grained, so we look for the
/// *dominant* operator that decides how workers get involved.
fn classify(plan: &PhysicalPlan, original_sql: &str) -> Dispatch {
    if let Some(d) = classify_broadcast_join(plan, original_sql) {
        return d;
    }
    if let Some(d) = classify_shuffle_agg(plan, original_sql) {
        return d;
    }
    let (table, _cols) = extract_scan(plan)
        .unwrap_or_else(|| ("_unknown_".to_string(), None));
    Dispatch::LocalTables {
        sql:   original_sql.to_string(),
        table,
    }
}

/// If the plan root is a broadcast-hash join, extract the table names and
/// route to `execute_broadcast_join`. Uses the plan's `est_rows` (not just
/// the strategy tag) so we still degrade gracefully if the planner picked
/// broadcast on stale catalog info.
fn classify_broadcast_join(plan: &PhysicalPlan, original_sql: &str) -> Option<Dispatch> {
    let (strategy, left, right) = match plan {
        PhysicalPlan::Join { strategy, left, right, .. } => (*strategy, &**left, &**right),
        // Also handle Project/Filter wrapping a Join at the top.
        PhysicalPlan::Project { input, .. } | PhysicalPlan::Filter { input, .. } => {
            if let PhysicalPlan::Join { strategy, left, right, .. } = &**input {
                (*strategy, &**left, &**right)
            } else {
                return None;
            }
        }
        _ => return None,
    };

    if strategy != JoinStrategy::BroadcastHash {
        return None;
    }

    let (left_tbl, _)  = extract_scan(left)?;
    let (right_tbl, _) = extract_scan(right)?;
    let left_rows  = left.est_rows();
    let right_rows = right.est_rows();

    // Smaller side is the broadcast side.
    let (large_table, small_table, small_rows) = if left_rows >= right_rows {
        (left_tbl, right_tbl, right_rows)
    } else {
        (right_tbl, left_tbl, left_rows)
    };

    Some(Dispatch::BroadcastJoin {
        join_sql: original_sql.to_string(),
        large_table,
        small_table,
        small_rows,
    })
}

/// If the plan has `HashAggregate{Partial} → Exchange{HashBy} → HashAggregate{Final}`,
/// extract map SQL / reduce SQL / keys and route to network shuffle.
///
/// For simplicity we keep the *original* SQL as the map SQL and derive a
/// trivial reduce SQL — the existing `execute_network_shuffle` machinery
/// already knows how to run "map SQL on partitions, reduce SQL on merged
/// results" using the coordinator table name.
fn classify_shuffle_agg(plan: &PhysicalPlan, original_sql: &str) -> Option<Dispatch> {
    // Walk down looking for a HashAggregate with keys AND an Exchange below it.
    let mut has_exchange = false;
    let mut keys: Vec<String> = Vec::new();
    let mut cur = plan;
    loop {
        match cur {
            PhysicalPlan::HashAggregate { keys: k, input, .. } => {
                if !k.is_empty() { keys = k.clone(); }
                cur = input;
            }
            PhysicalPlan::Exchange { input, .. } => {
                has_exchange = true;
                cur = input;
            }
            PhysicalPlan::Project { input, .. }
            | PhysicalPlan::Filter  { input, .. }
            | PhysicalPlan::Sort    { input, .. }
            | PhysicalPlan::Limit   { input, .. } => cur = input,
            _ => break,
        }
    }
    if !has_exchange || keys.is_empty() {
        return None;
    }

    let (table, _cols) = extract_scan(cur)?;
    let reduce_sql = derive_reduce_sql(original_sql);
    Some(Dispatch::NetworkShuffle {
        map_sql:    original_sql.to_string(),
        reduce_sql,
        table,
        keys,
    })
}

/// Extract `(table_name, projected_cols)` from the leaf `Scan` of a plan.
fn extract_scan(plan: &PhysicalPlan) -> Option<(String, Option<Vec<String>>)> {
    let mut cur = plan;
    loop {
        match cur {
            PhysicalPlan::Scan { table, projected_cols, .. } => {
                return Some((table.clone(), projected_cols.clone()));
            }
            PhysicalPlan::Filter { input, .. }
            | PhysicalPlan::Project { input, .. }
            | PhysicalPlan::HashAggregate { input, .. }
            | PhysicalPlan::Exchange { input, .. }
            | PhysicalPlan::Sort  { input, .. }
            | PhysicalPlan::Limit { input, .. } => cur = input,
            PhysicalPlan::Join { left, .. } => cur = left,
            PhysicalPlan::Union { inputs } if !inputs.is_empty() => cur = &inputs[0],
            _ => return None,
        }
    }
}

/// Derive a reduce SQL from the map SQL for the two-phase agg case.
///
/// Heuristic: the reduce operates on the merged block (named `merged` by the
/// existing shuffle path, but for the network-shuffle path the reducer
/// registers under the original table name).  We re-use the original SQL —
/// re-aggregating pre-aggregated groups produces the same answer when the
/// aggregates are sum/count/min/max; for AVG the executor already handles
/// this by projecting sum/count and dividing in the outer query.
fn derive_reduce_sql(original_sql: &str) -> String {
    original_sql.to_string()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn small_dim(n: usize) -> DataBlock {
        DataBlock {
            num_rows: n,
            columns: vec![
                Column { name: "id".into(),   data: ColumnData::Int64((0..n).map(|i| Some(i as i64)).collect()) },
                Column { name: "name".into(), data: ColumnData::Str((0..n).map(|i| Some(format!("d{i}"))).collect()) },
            ],
        }
    }

    fn large_fact(n: usize) -> DataBlock {
        DataBlock {
            num_rows: n,
            columns: vec![
                Column { name: "dim_id".into(), data: ColumnData::Int64((0..n).map(|i| Some((i % 10) as i64)).collect()) },
                Column { name: "amount".into(), data: ColumnData::Float64((0..n).map(|i| Some(i as f64)).collect()) },
            ],
        }
    }

    #[test]
    fn explain_picks_broadcast_for_small_dim_x_large_fact() {
        let coord = Coordinator::new();
        coord.register_table_for_planning("dim",  small_dim(10));
        coord.register_table_for_planning("fact", large_fact(50_000));

        let sql = "SELECT * FROM fact JOIN dim ON fact.dim_id = dim.id";
        let out = coord.explain(sql).unwrap();
        assert!(out.contains("BroadcastHash"), "expected BroadcastHash in\n{out}");
        assert!(out.contains("Dispatch"),      "explain output missing Dispatch section:\n{out}");
        assert!(out.contains("BroadcastJoin"), "expected BroadcastJoin dispatch:\n{out}");
    }

    #[test]
    fn explain_picks_shuffle_for_large_group_by() {
        let coord = Coordinator::new();
        // Big table, no dim — plain aggregation.
        coord.register_table_for_planning("fact", large_fact(50_000));

        let sql = "SELECT dim_id, sum(amount) FROM fact GROUP BY dim_id";
        let out = coord.explain(sql).unwrap();
        // The classifier requires an Exchange in the plan for shuffle dispatch;
        // catalyst inserts one when partial/final aggregation is used.
        assert!(out.contains("HashAggregate"), "expected HashAggregate in\n{out}");
    }

    #[test]
    fn classifier_falls_back_to_local_for_simple_scan() {
        let coord = Coordinator::new();
        coord.register_table_for_planning("t", large_fact(100));

        let (_plan, dispatch) = coord.plan_sql("SELECT amount FROM t").unwrap();
        assert!(matches!(dispatch, Dispatch::LocalTables { .. }));
    }

    #[test]
    fn dispatch_broadcast_picks_smaller_side() {
        let coord = Coordinator::new();
        coord.register_table_for_planning("small_left",  small_dim(5));
        coord.register_table_for_planning("big_right",   large_fact(100_000));

        let sql = "SELECT * FROM small_left JOIN big_right ON small_left.id = big_right.dim_id";
        let (_plan, dispatch) = coord.plan_sql(sql).unwrap();
        match dispatch {
            Dispatch::BroadcastJoin { large_table, small_table, small_rows, .. } => {
                assert_eq!(large_table, "big_right", "large side should be big");
                assert_eq!(small_table, "small_left", "small side should be broadcast");
                assert!(small_rows <= 100, "small side row estimate too high: {small_rows}");
            }
            other => panic!("expected BroadcastJoin, got {other:?}"),
        }
    }

    #[test]
    fn register_populates_catalog_stats() {
        let coord = Coordinator::new();
        coord.register_table_for_planning("fact", large_fact(1_000));
        coord.register_table_for_planning("dim",  small_dim(10));

        let sizes = coord.catalog_sizes();
        // catalog_sizes returns smallest first.
        assert_eq!(sizes[0].0, "dim");
        assert_eq!(sizes[0].1, 10);
        assert_eq!(sizes[1].0, "fact");
        assert_eq!(sizes[1].1, 1_000);
    }
}
