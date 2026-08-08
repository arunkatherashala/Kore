//! kore-distributed — Layer 66: True Distributed SQL Engine
//!
//! Two execution modes:
//!
//! ## Mode 1: Rayon (in-process parallel)
//!   DistributedExecutor::query() — splits data across threads on same machine.
//!   Fast. Zero network overhead.
//!
//! ## Mode 2: TCP Cluster (true multi-node)
//!   DistributedExecutor::query_cluster() — uses kore-coord + kore-worker via TCP.
//!   Works on same machine OR across multiple machines (just change worker addresses).
//!   Architecture:
//!     1. Coordinator starts on a local TCP port
//!     2. N workers spawn (local) or connect (remote) via TCP
//!     3. Coordinator partitions data → sends via KoreMsg::AssignTask
//!     4. Workers execute SQL on their partition → return KoreMsg::TaskResult
//!     5. Coordinator merges partial results with two-phase aggregation
//!
//!   To use on a REAL cluster: run workers on remote machines pointing at
//!   the coordinator's IP. Everything else is identical.

mod cluster;
mod planner;
pub use cluster::{
    query_persistent_cluster, query_persistent_cluster_blocking,
    query_persistent_cluster_planned, query_persistent_cluster_blocking_planned,
};
pub use planner::{plan, DistributedPlan, DistributedStrategy};

use std::collections::HashMap;
use rayon::prelude::*;

use kore_core::types::{Column, ColumnData, DataBlock};
use kore_sql::KqlContext;

// ─── Distributed executor ────────────────────────────────────────────────────

pub struct DistributedExecutor {
    /// Number of parallel workers (default: num CPU cores)
    pub num_workers: usize,
}

impl DistributedExecutor {
    pub fn new(num_workers: usize) -> Self {
        let w = if num_workers == 0 { rayon::current_num_threads() } else { num_workers };
        Self { num_workers: w }
    }

    pub fn with_all_cores() -> Self { Self::new(0) }

    /// Execute using TRUE TCP cluster: coordinator + real worker processes.
    ///
    /// This spawns a local coordinator + N workers connected via TCP sockets.
    /// On a real multi-machine cluster, workers run on remote machines and
    /// connect to the coordinator's IP — the protocol is IDENTICAL.
    ///
    /// Network flow:
    ///   1. Coordinator binds port, workers connect + register
    ///   2. Coordinator sends KoreMsg::AssignTask (partition + SQL) over TCP
    ///   3. Workers execute SQL, return KoreMsg::TaskResult over TCP
    ///   4. Coordinator merges results, applies ORDER BY/LIMIT
    pub fn query_cluster(&self, sql: &str, table_name: &str, data: DataBlock) -> Result<DataBlock, String> {
        let n_workers = self.num_workers;
        let sql_owned = sql.to_string();
        let table_owned = table_name.to_string();
        let reduce_sql = build_merge_sql(sql).replace("FROM data", "FROM merged");
        let has_agg = sql.to_lowercase().contains("group by")
            || sql.to_lowercase().contains("sum(")
            || sql.to_lowercase().contains("count(");

        // Build tokio runtime for async TCP operations
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(n_workers + 2)  // +2 for coordinator + I/O
            .enable_all()
            .build()
            .map_err(|e| format!("tokio runtime: {e}"))?;

        rt.block_on(async move {
            use tokio::net::TcpListener;

            // 1. Start coordinator on a free port
            let coord_listener = TcpListener::bind("127.0.0.1:0").await
                .map_err(|e| format!("coordinator bind: {e}"))?;
            let coord_addr = coord_listener.local_addr()
                .map_err(|e| format!("coord addr: {e}"))?.to_string();

            let coord = std::sync::Arc::new(kore_coord::Coordinator::new());
            let coord2 = coord.clone();

            // Run coordinator in background
            tokio::spawn(async move {
                coord2.run(coord_listener).await;
            });

            // 2. Spawn N workers — each connects to coordinator via TCP
            let mut worker_handles = Vec::new();
            for i in 0..n_workers {
                let ca = coord_addr.clone();
                worker_handles.push(tokio::spawn(async move {
                    let w = kore_worker::Worker::new(format!("worker-{i}"));
                    let _ = w.run(&ca).await;
                }));
            }

            // 3. Wait for all workers to register (poll with timeout)
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                if coord.worker_count() >= n_workers { break; }
                if tokio::time::Instant::now() > deadline {
                    return Err(format!("Timeout: only {}/{} workers registered", coord.worker_count(), n_workers));
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            eprintln!("[kore-distributed] TCP cluster: {n_workers} workers ready at {coord_addr}");

            // 4. Execute distributed query via coordinator (real TCP dispatch)
            let result = coord.execute_distributed(
                &sql_owned,
                &table_owned,
                data,
                if has_agg { Some(reduce_sql.as_str()) } else { None },
            ).await.map_err(|e| format!("distributed exec: {e}"))?;

            // 5. Abort worker tasks
            for h in worker_handles { h.abort(); }

            Ok(result)
        })
    }

    /// Execute a SQL query (Rayon in-process parallel — default mode).
    pub fn query(&self, sql: &str, data: DataBlock) -> Result<DataBlock, String> {
        // Detect query type to choose merge strategy
        let sql_lower = sql.to_lowercase();
        let has_group_by = sql_lower.contains("group by");
        let has_agg      = sql_lower.contains("sum(") || sql_lower.contains("count(")
                        || sql_lower.contains("avg(") || sql_lower.contains("min(")
                        || sql_lower.contains("max(");

        if has_group_by && has_agg {
            self.distributed_group_by(sql, data)
        } else if has_agg {
            self.distributed_global_agg(sql, data)
        } else {
            self.distributed_filter(sql, data)
        }
    }

    // ── Strategy 1: Distributed GROUP BY ─────────────────────────────────────
    // Phase 1 — Each worker runs WHERE + partial GROUP BY on its slice
    // Phase 2 — Coordinator merges partial results with full GROUP BY
    // Result: only aggregated rows cross worker boundaries (not raw rows)

    fn distributed_group_by(&self, sql: &str, data: DataBlock) -> Result<DataBlock, String> {
        let n = data.num_rows;
        let t = self.num_workers;
        let chunk = ((n + t - 1) / t).max(1);

        // PHASE 1: Each worker runs the FULL SQL on its slice (filter + local GROUP BY)
        // This is the key optimization: instead of returning raw rows, each worker
        // returns aggregated rows. For 6M rows with 6 groups → 6 rows per worker, not 6M/t rows.
        let partial_results: Vec<DataBlock> = (0..t)
            .into_par_iter()
            .filter_map(|w| {
                let start = w * chunk;
                let end   = (start + chunk).min(n);
                if start >= end { return None; }
                let slice = data.select_rows(&(start..end).collect::<Vec<_>>());
                let mut ctx = KqlContext::new();
                ctx.register("data", slice);
                // Run full SQL per worker — each gets a partial GROUP BY result
                // Workers return tiny aggregated results (6 rows for 6-group GROUP BY)
                ctx.query(&rewrite_table_name(sql, "data")).ok()
                   .filter(|b| b.num_rows > 0)
            })
            .collect();

        if partial_results.is_empty() { return Ok(DataBlock::empty()); }

        // PHASE 2: Merge partial aggregates.
        // Strategy: concat all partial results, run GROUP BY again to merge sums/counts
        let merged = DataBlock::concat(partial_results).map_err(|e| format!("Merge: {e}"))?;
        let mut ctx = KqlContext::new();
        ctx.register("data", merged);

        // Build merge SQL: re-aggregate the partial results
        // Replace the original projections with re-aggregation over partial results
        let merge_sql = build_merge_sql(sql);
        ctx.query(&rewrite_table_name(&merge_sql, "data"))
           .map_err(|e| format!("Phase 2 merge: {e}"))
    }

    // ── Strategy 2: Distributed global aggregation (no GROUP BY) ─────────────
    // Workers: each runs filter + partial aggregation (SUM/COUNT per slice)
    // Coordinator: re-aggregate partial results

    fn distributed_global_agg(&self, sql: &str, data: DataBlock) -> Result<DataBlock, String> {
        let n = data.num_rows;
        let t = self.num_workers;
        let chunk = ((n + t - 1) / t).max(1);

        // Each worker runs full SQL on its slice → tiny aggregated result (1 row)
        let parts: Vec<DataBlock> = (0..t)
            .into_par_iter()
            .filter_map(|w| {
                let start = w * chunk;
                let end   = (start + chunk).min(n);
                if start >= end { return None; }
                let slice = data.select_rows(&(start..end).collect::<Vec<_>>());
                let mut ctx = KqlContext::new();
                ctx.register("data", slice);
                ctx.query(&rewrite_table_name(sql, "data")).ok()
                   .filter(|b| b.num_rows > 0)
            })
            .collect();

        if parts.is_empty() { return Ok(DataBlock::empty()); }
        // Re-aggregate the t partial results (t rows) → 1 final row
        let merged = DataBlock::concat(parts).map_err(|e| format!("Concat: {e}"))?;
        let mut ctx = KqlContext::new();
        ctx.register("data", merged);
        let merge_sql = build_merge_sql(sql);
        ctx.query(&rewrite_table_name(&merge_sql, "data"))
           .map_err(|e| format!("Global agg merge: {e}"))
    }

    // ── Strategy 3: Distributed filter (no aggregation) ──────────────────────
    // Workers: parallel WHERE on each slice. Coordinator: concat.

    fn distributed_filter(&self, sql: &str, data: DataBlock) -> Result<DataBlock, String> {
        let n = data.num_rows;
        let t = self.num_workers;
        let chunk = ((n + t - 1) / t).max(1);

        let results: Vec<Result<DataBlock, String>> = (0..t)
            .into_par_iter()
            .map(|w| {
                let start = w * chunk;
                let end   = (start + chunk).min(n);
                if start >= end { return Ok(DataBlock::empty()); }
                let slice = data.select_rows(&(start..end).collect::<Vec<_>>());
                let mut ctx = KqlContext::new();
                ctx.register("data", slice);
                ctx.query(&rewrite_table_name(sql, "data"))
                   .map_err(|e| format!("Worker {w}: {e}"))
            })
            .collect();

        let parts: Vec<DataBlock> = results
            .into_iter()
            .filter_map(|r| r.ok())
            .filter(|b| b.num_rows > 0)
            .collect();

        if parts.is_empty() { return Ok(DataBlock::empty()); }
        DataBlock::concat(parts).map_err(|e| format!("Concat: {e}"))
    }
}

// ─── High-level API ───────────────────────────────────────────────────────────

/// Execute SQL on a DataBlock using all CPU cores in parallel (Rayon mode).
pub fn distributed_query(sql: &str, data: DataBlock) -> Result<DataBlock, String> {
    DistributedExecutor::with_all_cores().query(sql, data)
}

/// Execute with a specific number of workers (Rayon mode).
pub fn distributed_query_n(sql: &str, data: DataBlock, workers: usize) -> Result<DataBlock, String> {
    DistributedExecutor::new(workers).query(sql, data)
}

/// Execute on persistent coordinator using planner (Phase 6).
pub fn cluster_query_planned(
    coord_addr: &str,
    sql: &str,
    table_name: &str,
    data: DataBlock,
) -> Result<DataBlock, String> {
    let p = plan(sql, table_name);
    query_persistent_cluster_blocking_planned(
        coord_addr,
        &p.map_sql,
        table_name,
        data,
        p.reduce_sql.as_deref(),
    )
}

/// Execute on a **persistent** coordinator already running (no spawn/kill per query).
pub fn cluster_query_persistent(
    coord_addr: &str,
    sql: &str,
    table_name: &str,
    data: DataBlock,
) -> Result<DataBlock, String> {
    query_persistent_cluster_blocking(coord_addr, sql, table_name, data)
}

/// Execute using TRUE TCP cluster: real coordinator + worker network.
/// Same SQL, same DataBlock — but communication goes through TCP sockets.
/// On a multi-machine cluster, workers run on remote hosts pointing to coordinator IP.
pub fn cluster_query(sql: &str, table_name: &str, data: DataBlock, workers: usize) -> Result<DataBlock, String> {
    DistributedExecutor::new(workers).query_cluster(sql, table_name, data)
}

// ─── DistributedContext: drop-in replacement for KqlContext ──────────────────

/// A distributed version of KqlContext.
/// Same API as KqlContext — just run queries in parallel across workers.
pub struct DistributedContext {
    tables:   HashMap<String, DataBlock>,
    executor: DistributedExecutor,
}

impl DistributedContext {
    pub fn new() -> Self {
        Self {
            tables:   HashMap::new(),
            executor: DistributedExecutor::with_all_cores(),
        }
    }

    pub fn with_workers(n: usize) -> Self {
        Self {
            tables:   HashMap::new(),
            executor: DistributedExecutor::new(n),
        }
    }

    pub fn register(&mut self, name: impl Into<String>, data: DataBlock) {
        self.tables.insert(name.into(), data);
    }

    /// Execute SQL in distributed mode.
    /// Partitions the main table across workers, merges results.
    pub fn query(&self, sql: &str) -> Result<DataBlock, String> {
        // Find the referenced table in the SQL
        let (table_name, data) = self.tables.iter()
            .find(|(name, _)| {
                let sql_lower = sql.to_lowercase();
                sql_lower.contains(&format!(" {} ", name.to_lowercase())) ||
                sql_lower.contains(&format!(" {})", name.to_lowercase())) ||
                sql_lower.contains(&format!("from {}", name.to_lowercase()))
            })
            .map(|(n, d)| (n.clone(), d.clone()))
            .ok_or_else(|| "No matching table found in SQL".to_string())?;

        self.executor.query(sql, data)
    }

    pub fn num_workers(&self) -> usize { self.executor.num_workers }
}

impl Default for DistributedContext { fn default() -> Self { Self::new() } }

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Rewrite FROM <table> to FROM data in the SQL string.
/// Simple string replacement — works for standard SELECT ... FROM table ...
/// Build a merge SQL for two-phase aggregation over partial worker results.
///
/// Problem: Workers run `SELECT kind, SUM(importance) AS total FROM slice GROUP BY kind`
/// This gives partial results with columns (kind, total).
/// The merge must then run `SELECT kind, SUM(total) AS total FROM data GROUP BY kind`
/// NOT the original SQL (which would try SUM(importance) but only `total` column exists).
///
/// Algorithm: parse projection list, detect aggregate functions,
/// rewrite them to aggregate their alias columns.
pub(crate) fn build_merge_sql(original_sql: &str) -> String {
    let lower = original_sql.to_lowercase();

    // Only transform if it has aggregation
    let has_agg = lower.contains("sum(") || lower.contains("count(")
               || lower.contains("avg(") || lower.contains("min(")
               || lower.contains("max(");
    if !has_agg { return original_sql.to_string(); }

    // Extract: SELECT ... FROM ... WHERE ... GROUP BY ... ORDER BY ... LIMIT ...
    let select_start = lower.find("select ").map(|p| p + 7).unwrap_or(0);
    let from_pos = lower.find(" from ").unwrap_or(original_sql.len());

    let projections_str = &original_sql[select_start..from_pos];

    // Parse projections and rewrite aggregates to use their aliases
    let mut new_projs: Vec<String> = vec![];
    for proj in split_projections(projections_str) {
        let proj = proj.trim().to_string();
        let proj_lower = proj.to_lowercase();

        // Detect aggregate: SUM/COUNT/AVG/MIN/MAX(...)
        if let Some((func, alias)) = extract_agg_and_alias(&proj) {
            let alias = alias.unwrap_or_else(|| {
                // Auto-alias: func_col
                proj_lower.split('(').next().unwrap_or("agg").to_string()
            });
            // COUNT(*) partial → SUM(cnt_alias) in merge
            let merge_func = if func.eq_ignore_ascii_case("COUNT") { "SUM" } else { &func };
            new_projs.push(format!("{merge_func}({alias}) AS {alias}"));
        } else {
            // Non-aggregate (column, qualified col) — keep as-is
            // Strip table-qualified names for merged data (use just the column)
            let col = proj.rsplit('.').next().unwrap_or(&proj).to_string();
            new_projs.push(col);
        }
    }

    // Rebuild: keep FROM data, WHERE (removed for merge), GROUP BY, ORDER BY, LIMIT
    let after_from = &original_sql[from_pos..];
    let group_pos = lower.rfind(" group by ").map(|p| p + 1);
    let order_pos = lower.rfind(" order by ").map(|p| p + 1);
    let limit_pos = lower.rfind(" limit ").map(|p| p + 1);

    let mut tail = String::new();
    if let Some(gp) = group_pos {
        let end = order_pos.or(limit_pos).unwrap_or(original_sql.len());
        tail.push(' ');
        tail.push_str(&original_sql[gp..end]);
    }
    if let Some(op) = order_pos {
        let end = limit_pos.unwrap_or(original_sql.len());
        tail.push(' ');
        tail.push_str(&original_sql[op..end]);
    }
    if let Some(lp) = limit_pos {
        tail.push(' ');
        tail.push_str(&original_sql[lp..]);
    }

    format!("SELECT {} FROM data{tail}", new_projs.join(", "))
}

fn split_projections(s: &str) -> Vec<&str> {
    // Split by comma but respect parentheses
    let mut parts = vec![];
    let mut depth = 0;
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Returns (function_name, alias) if this is an aggregate expression.
fn extract_agg_and_alias(proj: &str) -> Option<(String, Option<String>)> {
    let proj_lower = proj.to_lowercase();
    let funcs = ["sum(", "count(", "avg(", "min(", "max("];
    for f in &funcs {
        if let Some(pos) = proj_lower.find(f) {
            let func = f.trim_end_matches('(').to_uppercase();
            // Find alias after AS or at end
            let alias = if let Some(as_pos) = proj_lower.rfind(" as ") {
                Some(proj[as_pos + 4..].trim().to_string())
            } else {
                None
            };
            return Some((func, alias));
        }
    }
    None
}

fn rewrite_table_name(sql: &str, new_name: &str) -> String {
    // Already using "data" as table name → no change needed
    if sql.to_lowercase().contains("from data") { return sql.to_string(); }

    // Replace first occurrence of FROM <word> with FROM data
    let lower = sql.to_lowercase();
    if let Some(from_pos) = lower.find(" from ") {
        let after_from = from_pos + 6;
        let table_end = sql[after_from..].find(|c: char| c == ' ' || c == '\n' || c == '\r')
            .map(|p| after_from + p)
            .unwrap_or(sql.len());
        format!("{} {} {}", &sql[..from_pos], "FROM", new_name) + &sql[table_end..]
    } else {
        sql.to_string()
    }
}

/// Merge partial aggregation results by summing Float64 columns.
fn merge_partial_aggs(parts: Vec<DataBlock>) -> Result<DataBlock, String> {
    if parts.is_empty() { return Ok(DataBlock::empty()); }
    if parts.len() == 1 { return Ok(parts.into_iter().next().unwrap()); }

    let ncols = parts[0].columns.len();
    let mut merged_cols: Vec<Column> = Vec::new();

    for ci in 0..ncols {
        let col_name = parts[0].columns[ci].name.clone();
        // Sum all Float64 partial values into one
        match &parts[0].columns[ci].data {
            ColumnData::Float64(_) => {
                let total: f64 = parts.iter()
                    .filter_map(|b| b.columns.get(ci))
                    .flat_map(|c| match &c.data {
                        ColumnData::Float64(v) => v.iter().filter_map(|x| *x).collect::<Vec<_>>(),
                        _ => vec![],
                    })
                    .sum();
                merged_cols.push(Column { name: col_name, data: ColumnData::Float64(vec![Some(total)]) });
            }
            _ => {
                // Non-numeric: keep from first partial
                merged_cols.push(parts[0].columns[ci].clone());
            }
        }
    }

    Ok(DataBlock { num_rows: 1, columns: merged_cols })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::types::{Column, ColumnData, DataBlock};

    fn make_test_data(n: usize) -> DataBlock {
        DataBlock {
            num_rows: n,
            columns: vec![
                Column { name: "amount".into(), data: ColumnData::Float64(
                    (0..n).map(|i| Some(i as f64)).collect()
                )},
                Column { name: "cat".into(), data: ColumnData::Str(
                    (0..n).map(|i| Some(["A","B","C"][i%3].to_string())).collect()
                )},
            ],
        }
    }

    #[test]
    fn test_distributed_group_by() {
        let data = make_test_data(900);
        let mut ctx = DistributedContext::with_workers(3);
        ctx.register("sales", data);
        let result = ctx.query(
            "SELECT cat, SUM(amount) AS total FROM sales GROUP BY cat"
        ).expect("distributed GROUP BY failed");

        assert_eq!(result.num_rows, 3);  // 3 distinct categories
        println!("Distributed GROUP BY: {} groups across {} workers",
            result.num_rows, ctx.num_workers());
    }

    #[test]
    fn test_distributed_filter() {
        let data = make_test_data(1000);
        let result = distributed_query(
            "SELECT amount FROM data WHERE amount < 100",
            data
        ).expect("distributed filter failed");
        assert_eq!(result.num_rows, 100);
        println!("Distributed filter: {} rows matched", result.num_rows);
    }

    #[test]
    fn test_distributed_global_agg() {
        let data = make_test_data(1000);
        // sum(0..1000) = 499500
        let result = distributed_query(
            "SELECT SUM(amount) AS total FROM data",
            data
        ).expect("distributed global agg failed");
        assert_eq!(result.num_rows, 1);
        println!("Distributed SUM: {:?}", result);
    }
}
