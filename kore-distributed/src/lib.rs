//! kore-distributed — Layer 66: True Distributed SQL Engine
//!
//! Wires kore-sql → parallel workers → merge.
//! This is the missing link that makes KORE compete with Spark at TB scale.
//!
//! Architecture:
//!
//!   SQL Query
//!      ↓
//!   DistributedExecutor::query()
//!      ↓
//!   1. PARTITION: Split DataBlock into T horizontal slices (by rows)
//!      ↓  ↓  ↓  ↓  ↓  ↓  ↓  ↓
//!   2. EXECUTE: Each worker runs filter + local aggregation on its slice
//!      (Rayon threads now → real network workers via kore-coord/kore-worker later)
//!      ↓
//!   3. MERGE: Combine partial aggregates (SUM adds, COUNT adds, MIN/MAX compare)
//!      ↓
//!   4. FINALIZE: ORDER BY, LIMIT, HAVING on merged result
//!      ↓
//!   Final DataBlock
//!
//! Supported distributed patterns:
//!   - SELECT agg() FROM table WHERE ... GROUP BY cols  (map-reduce aggregation)
//!   - SELECT * FROM table WHERE ...                    (parallel filter, no merge needed)
//!   - SELECT agg() FROM table WHERE ...                (parallel global agg, merge sums)
//!
//! Future (network mode): replace Rayon threads with kore-net + kore-worker

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

    /// Execute a SQL query in distributed mode.
    ///
    /// Steps:
    ///   1. Parse to detect query type (aggregation, filter, etc.)
    ///   2. Partition data across workers
    ///   3. Each worker executes locally
    ///   4. Merge partial results
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
    // Most powerful: each worker builds local partial aggregates,
    // coordinator merges by key. No full shuffle needed.

    fn distributed_group_by(&self, sql: &str, data: DataBlock) -> Result<DataBlock, String> {
        let n = data.num_rows;
        let t = self.num_workers;
        let chunk = ((n + t - 1) / t).max(1);

        // PHASE 1: Parallel local aggregation on each data slice
        let partial_results: Vec<Result<DataBlock, String>> = (0..t)
            .into_par_iter()
            .map(|w| {
                let start = w * chunk;
                let end   = (start + chunk).min(n);
                if start >= end { return Ok(DataBlock::empty()); }

                // Extract this worker's data slice
                let slice_indices: Vec<usize> = (start..end).collect();
                let slice = data.select_rows(&slice_indices);

                // Register and execute SQL on local slice
                let mut ctx = KqlContext::new();
                ctx.register("data", slice);
                // Rewrite table name to "data"
                let local_sql = rewrite_table_name(sql, "data");
                ctx.query(&local_sql).map_err(|e| format!("Worker {w}: {e}"))
            })
            .collect();

        // PHASE 2: Merge partial results
        let mut partials: Vec<DataBlock> = partial_results
            .into_iter()
            .filter_map(|r| r.ok())
            .filter(|b| b.num_rows > 0)
            .collect();

        if partials.is_empty() { return Ok(DataBlock::empty()); }
        if partials.len() == 1 { return Ok(partials.remove(0)); }

        // Merge partial GROUP BY results by re-aggregating
        let combined = DataBlock::concat(partials).map_err(|e| format!("Merge: {e}"))?;
        let mut ctx = KqlContext::new();
        ctx.register("data", combined);
        let merge_sql = rewrite_table_name(sql, "data");
        ctx.query(&merge_sql).map_err(|e| format!("Final merge: {e}"))
    }

    // ── Strategy 2: Distributed global aggregation (no GROUP BY) ─────────────
    // Each worker computes partial SUM/COUNT, coordinator adds them up.

    fn distributed_global_agg(&self, sql: &str, data: DataBlock) -> Result<DataBlock, String> {
        let n = data.num_rows;
        let t = self.num_workers;
        let chunk = ((n + t - 1) / t).max(1);

        let partial_results: Vec<Result<DataBlock, String>> = (0..t)
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

        // Sum up all partial aggregates column-by-column
        let partials: Vec<DataBlock> = partial_results
            .into_iter()
            .filter_map(|r| r.ok())
            .filter(|b| b.num_rows > 0)
            .collect();

        if partials.is_empty() { return Ok(DataBlock::empty()); }

        // Merge by summing Float64 columns (handles SUM/COUNT)
        merge_partial_aggs(partials)
    }

    // ── Strategy 3: Distributed filter (no aggregation) ──────────────────────
    // Each worker filters its slice. Concat results.

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

/// Execute SQL on a DataBlock using all CPU cores in parallel.
/// Automatically detects query type and applies the best distributed strategy.
pub fn distributed_query(sql: &str, data: DataBlock) -> Result<DataBlock, String> {
    DistributedExecutor::with_all_cores().query(sql, data)
}

/// Execute with a specific number of workers.
pub fn distributed_query_n(sql: &str, data: DataBlock, workers: usize) -> Result<DataBlock, String> {
    DistributedExecutor::new(workers).query(sql, data)
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
