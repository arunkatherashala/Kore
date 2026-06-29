//! KORE Layer 50 — Subquery Support
//!
//! Enables SQL subqueries in three forms:
//!
//! 1. **Scalar subquery** — `WHERE price > (SELECT AVG(price) FROM products)`
//!    Returns a single value; used anywhere a literal can appear.
//!
//! 2. **IN subquery** — `WHERE id IN (SELECT id FROM active_users)`
//!    Returns a set of values; used with IN / NOT IN.
//!
//! 3. **EXISTS subquery** — `WHERE EXISTS (SELECT 1 FROM orders WHERE orders.user_id = u.id)`
//!    Returns true if the subquery produces ≥1 row.
//!
//! 4. **Derived table (FROM subquery)** — `SELECT * FROM (SELECT ...) AS t`
//!    A full SELECT as the FROM source.

use kore_core::{DataBlock, KoreError};
use kore_sql::executor::KqlContext;

// ─── Subquery evaluator ───────────────────────────────────────────────────────

/// Result of evaluating a subquery.
#[derive(Debug, Clone)]
pub enum SubqueryResult {
    /// Scalar: a single f64 value (or None if empty/null).
    Scalar(Option<f64>),
    /// Set: a list of string-encoded values for IN checks.
    Set(Vec<String>),
    /// Exists: true if the subquery produced ≥1 row.
    Exists(bool),
    /// Table: a full DataBlock result (for derived tables).
    Table(DataBlock),
}

/// Evaluate a subquery SQL string and return the appropriate result type.
pub fn eval_subquery(
    sql:  &str,
    ctx:  &KqlContext,
    kind: SubqueryKind,
) -> Result<SubqueryResult, KoreError> {
    let result = ctx.query(sql)?;
    match kind {
        SubqueryKind::Scalar => {
            if result.num_rows == 0 || result.columns.is_empty() {
                return Ok(SubqueryResult::Scalar(None));
            }
            let val = result.columns[0].data.get_value(0).as_f64();
            Ok(SubqueryResult::Scalar(val))
        }
        SubqueryKind::Set => {
            if result.columns.is_empty() {
                return Ok(SubqueryResult::Set(vec![]));
            }
            let col = &result.columns[0];
            let vals: Vec<String> = (0..result.num_rows).filter_map(|r| {
                Some(match &col.data {
                    kore_core::ColumnData::Int64(v)   => v.get(r).and_then(|x| *x)?.to_string(),
                    kore_core::ColumnData::Float64(v) => v.get(r).and_then(|x| *x).map(|f| format!("{f:.10}"))?,
                    kore_core::ColumnData::Str(v)     => v.get(r).and_then(|x| x.clone())?,
                    kore_core::ColumnData::Bool(v)    => v.get(r).and_then(|x| *x)?.to_string(),
                })
            }).collect();
            Ok(SubqueryResult::Set(vals))
        }
        SubqueryKind::Exists => Ok(SubqueryResult::Exists(result.num_rows > 0)),
        SubqueryKind::DerivedTable => Ok(SubqueryResult::Table(result)),
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SubqueryKind {
    Scalar,
    Set,
    Exists,
    DerivedTable,
}

// ─── Subquery-aware query execution ──────────────────────────────────────────

/// Execute a query that may contain subquery placeholders.
///
/// Subqueries are pre-evaluated and registered as temporary tables,
/// then the outer query uses those tables by name.
///
/// # Supported syntax extensions
/// ```sql
/// -- Scalar subquery in WHERE
/// SELECT * FROM t WHERE price > (SELECT AVG(price) FROM t) -- not yet inline-parsed
///
/// -- IN subquery (rewritten as semi-join)
/// SELECT * FROM orders WHERE user_id IN (SELECT id FROM premium_users)
///
/// -- Derived table
/// SELECT * FROM (SELECT id, SUM(amount) AS total FROM sales GROUP BY id) AS agg
/// WHERE total > 100
/// ```
///
/// Since inline subquery parsing requires modifying the kore-sql parser,
/// this module provides the helper infrastructure; the host application
/// can pre-evaluate subqueries and inject results as named tables.
pub struct SubqueryContext<'a> {
    pub ctx: &'a mut KqlContext,
}

impl<'a> SubqueryContext<'a> {
    pub fn new(ctx: &'a mut KqlContext) -> Self { Self { ctx } }

    /// Register the result of a subquery as a named table so the outer
    /// query can reference it.
    pub fn materialize(&mut self, alias: &str, sql: &str) -> Result<usize, KoreError> {
        let result = self.ctx.query(sql)?;
        let n = result.num_rows;
        self.ctx.register(alias.to_string(), result);
        Ok(n)
    }

    /// Execute a scalar subquery and return its value.
    pub fn scalar(&self, sql: &str) -> Result<Option<f64>, KoreError> {
        match eval_subquery(sql, self.ctx, SubqueryKind::Scalar)? {
            SubqueryResult::Scalar(v) => Ok(v),
            _ => Err(KoreError::InvalidArgument("expected scalar subquery".into())),
        }
    }

    /// Execute a set subquery for IN checks.
    pub fn set_values(&self, sql: &str) -> Result<Vec<String>, KoreError> {
        match eval_subquery(sql, self.ctx, SubqueryKind::Set)? {
            SubqueryResult::Set(v) => Ok(v),
            _ => Err(KoreError::InvalidArgument("expected set subquery".into())),
        }
    }

    /// Check EXISTS.
    pub fn exists(&self, sql: &str) -> Result<bool, KoreError> {
        match eval_subquery(sql, self.ctx, SubqueryKind::Exists)? {
            SubqueryResult::Exists(b) => Ok(b),
            _ => Err(KoreError::InvalidArgument("expected exists subquery".into())),
        }
    }

    /// Run the outer query after subqueries have been materialized.
    pub fn query(&self, sql: &str) -> Result<DataBlock, KoreError> {
        self.ctx.query(sql)
    }
}

// ─── Semi-join (IN subquery rewrite) ─────────────────────────────────────────

/// Apply a semi-join: keep only rows in `block` whose `key_col` value
/// appears in `set`. Equivalent to `WHERE key_col IN (subquery)`.
pub fn semi_join(block: &DataBlock, key_col: &str, set: &[String]) -> DataBlock {
    use std::collections::HashSet;
    let set_hs: HashSet<&str> = set.iter().map(|s| s.as_str()).collect();
    let indices: Vec<usize> = if let Some(col) = block.columns.iter().find(|c| c.name == key_col) {
        (0..block.num_rows).filter(|&r| {
            let k = match &col.data {
                kore_core::ColumnData::Int64(v)   => v.get(r).and_then(|x| *x).map(|i| i.to_string()),
                kore_core::ColumnData::Str(v)     => v.get(r).and_then(|x| x.clone()),
                _ => None,
            };
            k.map(|s| set_hs.contains(s.as_str())).unwrap_or(false)
        }).collect()
    } else { vec![] };
    block.select_rows(&indices)
}

/// Anti-join: keep rows NOT in the set (NOT IN subquery).
pub fn anti_join(block: &DataBlock, key_col: &str, set: &[String]) -> DataBlock {
    use std::collections::HashSet;
    let set_hs: HashSet<&str> = set.iter().map(|s| s.as_str()).collect();
    let indices: Vec<usize> = if let Some(col) = block.columns.iter().find(|c| c.name == key_col) {
        (0..block.num_rows).filter(|&r| {
            let k = match &col.data {
                kore_core::ColumnData::Int64(v)   => v.get(r).and_then(|x| *x).map(|i| i.to_string()),
                kore_core::ColumnData::Str(v)     => v.get(r).and_then(|x| x.clone()),
                _ => None,
            };
            k.map(|s| !set_hs.contains(s.as_str())).unwrap_or(true)
        }).collect()
    } else { (0..block.num_rows).collect() };
    block.select_rows(&indices)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};
    use kore_sql::executor::KqlContext;

    fn make_data() -> DataBlock {
        DataBlock {
            num_rows: 5,
            columns: vec![
                Column { name: "id".into(),    data: ColumnData::Int64(vec![Some(1),Some(2),Some(3),Some(4),Some(5)]) },
                Column { name: "score".into(), data: ColumnData::Float64(vec![Some(10.0),Some(50.0),Some(30.0),Some(80.0),Some(20.0)]) },
            ],
        }
    }

    #[test]
    fn test_scalar_subquery() {
        let mut ctx = KqlContext::new();
        ctx.register("t", make_data());
        let sq = SubqueryContext::new(&mut ctx);
        // AVG(score) = (10+50+30+80+20)/5 = 38
        let avg = sq.scalar("SELECT AVG(score) AS mean_sc FROM t").unwrap().unwrap();
        assert!((avg - 38.0).abs() < 0.001, "avg={avg}");
    }

    #[test]
    fn test_set_subquery_semi_join() {
        let mut ctx = KqlContext::new();
        ctx.register("t", make_data());
        let sq = SubqueryContext::new(&mut ctx);
        // IDs with score > 30
        let ids = sq.set_values("SELECT id FROM t WHERE score > 30").unwrap();
        // ids = [2, 4] (score 50 and 80)
        assert_eq!(ids.len(), 2);
        let data = make_data();
        let filtered = semi_join(&data, "id", &ids);
        assert_eq!(filtered.num_rows, 2);
    }

    #[test]
    fn test_exists_subquery() {
        let mut ctx = KqlContext::new();
        ctx.register("t", make_data());
        let sq = SubqueryContext::new(&mut ctx);
        assert!(sq.exists("SELECT 1 FROM t WHERE score > 70").unwrap());
        assert!(!sq.exists("SELECT 1 FROM t WHERE score > 1000").unwrap());
    }

    #[test]
    fn test_derived_table_pattern() {
        let mut ctx = KqlContext::new();
        ctx.register("t", make_data());
        let mut sq = SubqueryContext::new(&mut ctx);
        // Materialize derived table: high scorers
        sq.materialize("high", "SELECT * FROM t WHERE score > 30").unwrap();
        let result = sq.query("SELECT * FROM high WHERE id > 1").unwrap();
        // score>30 AND id>1 → id=2(50), id=4(80) → 2 rows
        assert_eq!(result.num_rows, 2);
    }

    #[test]
    fn test_anti_join() {
        let data = make_data();
        let exclude = vec!["2".into(), "4".into()];
        let result = anti_join(&data, "id", &exclude);
        // Keep ids 1, 3, 5
        assert_eq!(result.num_rows, 3);
    }
}
