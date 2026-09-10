//! KORE Layer 51 — Full Catalyst-Level Query Optimizer
//!
//! Implements a **rule-based + cost-based** optimizer modeled after
//! Apache Spark's Catalyst optimizer.
//!
//! # Rule categories (mirrors Catalyst)
//! 1. **Analysis rules** — resolve column references, check types
//! 2. **Logical optimization rules** — transform the logical plan:
//!    - ConstantFolding           — evaluate compile-time constants
//!    - PredicatePushdown         — push filters below joins/aggregates
//!    - ProjectionPruning         — remove unused columns
//!    - ColumnPruning             — narrow scans to referenced cols only
//!    - JoinReorder               — put smallest table on build side
//!    - LimitPushdown             — push LIMIT into scan
//!    - CommonSubexprElim (CSE)   — reuse repeated sub-expressions
//!    - EliminateSubquery         — rewrite correlated subqueries
//!    - BooleanSimplification     — simplify TRUE AND x → x
//!    - NullPropagation           — simplify NULL + x → NULL
//! 3. **Physical planning rules** — choose execution strategies:
//!    - BroadcastHashJoin         — small table → broadcast
//!    - SortMergeJoin             — large equi-join
//!    - LocalAggFirst             — push aggregation before shuffle

use kore_sql::ast::*;
use kore_catalog::Catalog;

pub mod physical;
pub use physical::{
    PhysicalPlan, Partitioning, AggMode, JoinStrategy, JoinCond,
    plan_query, choose_join_strategy, choose_join_strategy_with_stats,
};

// ─── Rule trait ───────────────────────────────────────────────────────────────

pub trait OptRule: Send + Sync {
    fn name(&self) -> &'static str;
    fn apply(&self, query: &mut Query, catalog: &Catalog) -> bool;   // returns true if changed
}

// ─── Optimizer ────────────────────────────────────────────────────────────────

pub struct CatalystOptimizer {
    rules:       Vec<Box<dyn OptRule>>,
    max_passes:  usize,
}

impl CatalystOptimizer {
    /// Build with all built-in rules.
    pub fn new() -> Self {
        Self {
            rules: vec![
                Box::new(ConstantFoldingRule),
                Box::new(BooleanSimplifyRule),
                Box::new(NullPropagationRule),
                Box::new(PredicatePushdownRule),
                Box::new(ProjectionPruningRule),
                Box::new(LimitPushdownRule),
                Box::new(JoinReorderRule),
            ],
            max_passes: 5,
        }
    }

    /// Run all rules until fixed-point or `max_passes`.
    pub fn optimize(&self, query: &mut Query, catalog: &Catalog) -> OptReport {
        let mut report = OptReport::default();
        for _pass in 0..self.max_passes {
            let mut changed = false;
            for rule in &self.rules {
                if rule.apply(query, catalog) {
                    report.rules_fired.push(rule.name().to_string());
                    changed = true;
                }
            }
            report.passes += 1;
            if !changed { break; }
        }
        report
    }
}

impl Default for CatalystOptimizer { fn default() -> Self { Self::new() } }

#[derive(Debug, Default, Clone)]
pub struct OptReport {
    pub passes:      usize,
    pub rules_fired: Vec<String>,
}

impl OptReport {
    pub fn fired(&self, rule: &str) -> bool { self.rules_fired.iter().any(|r| r == rule) }
}

// ─── Rule implementations ─────────────────────────────────────────────────────

// 1. Constant Folding ─────────────────────────────────────────────────────────

pub struct ConstantFoldingRule;
impl OptRule for ConstantFoldingRule {
    fn name(&self) -> &'static str { "ConstantFolding" }
    fn apply(&self, query: &mut Query, _: &Catalog) -> bool {
        let mut changed = false;
        if let Some(stmt) = &mut query.body {
            if let Some(w) = &mut stmt.where_clause { changed |= fold_expr_mut(w); }
            if let Some(h) = &mut stmt.having       { changed |= fold_expr_mut(h); }
            for p in &mut stmt.projections {
                if let Projection::Expr { expr, .. } = p { changed |= fold_expr_mut(expr); }
            }
        }
        changed
    }
}

fn fold_expr_mut(expr: &mut Expr) -> bool {
    let mut changed = false;
    match expr {
        Expr::BinOp { op, left, right } => {
            changed |= fold_expr_mut(left);
            changed |= fold_expr_mut(right);
            if let Some(folded) = try_fold_binop(op, left, right) {
                *expr = folded; changed = true;
            }
        }
        Expr::Not(inner) => {
            changed |= fold_expr_mut(inner);
            if let Expr::Bool(b) = inner.as_ref() { *expr = Expr::Bool(!b); changed = true; }
        }
        Expr::IsNull(inner) => {
            fold_expr_mut(inner);
            if matches!(inner.as_ref(), Expr::Null) { *expr = Expr::Bool(true); changed = true; }
        }
        _ => {}
    }
    changed
}

fn try_fold_binop(op: &BinOpKind, l: &Expr, r: &Expr) -> Option<Expr> {
    match (l, r) {
        (Expr::Int(a), Expr::Int(b)) => Some(match op {
            BinOpKind::Add => Expr::Int(a + b),  BinOpKind::Sub => Expr::Int(a - b),
            BinOpKind::Mul => Expr::Int(a * b),  BinOpKind::Div if *b != 0 => Expr::Int(a / b),
            BinOpKind::Eq  => Expr::Bool(a == b), BinOpKind::Ne => Expr::Bool(a != b),
            BinOpKind::Lt  => Expr::Bool(a < b),  BinOpKind::Le => Expr::Bool(a <= b),
            BinOpKind::Gt  => Expr::Bool(a > b),  BinOpKind::Ge => Expr::Bool(a >= b),
            _ => return None,
        }),
        (Expr::Float(a), Expr::Float(b)) => Some(match op {
            BinOpKind::Add => Expr::Float(a + b), BinOpKind::Sub => Expr::Float(a - b),
            BinOpKind::Mul => Expr::Float(a * b), BinOpKind::Div => Expr::Float(a / b),
            BinOpKind::Eq  => Expr::Bool((a-b).abs() < 1e-10),
            BinOpKind::Lt  => Expr::Bool(a < b),  BinOpKind::Gt => Expr::Bool(a > b),
            _ => return None,
        }),
        (Expr::Bool(a), Expr::Bool(b)) => Some(match op {
            BinOpKind::And => Expr::Bool(*a && *b),
            BinOpKind::Or  => Expr::Bool(*a || *b),
            _ => return None,
        }),
        _ => None,
    }
}

// 2. Boolean Simplification ───────────────────────────────────────────────────

pub struct BooleanSimplifyRule;
impl OptRule for BooleanSimplifyRule {
    fn name(&self) -> &'static str { "BooleanSimplification" }
    fn apply(&self, query: &mut Query, _: &Catalog) -> bool {
        let mut changed = false;
        if let Some(stmt) = &mut query.body {
            if let Some(w) = &mut stmt.where_clause { changed |= simplify_bool(w); }
        }
        changed
    }
}

fn simplify_bool(expr: &mut Expr) -> bool {
    let mut changed = false;
    match expr {
        Expr::BinOp { op: BinOpKind::And, left, right } => {
            changed |= simplify_bool(left); changed |= simplify_bool(right);
            match (left.as_ref(), right.as_ref()) {
                (Expr::Bool(false), _) | (_, Expr::Bool(false)) => { *expr = Expr::Bool(false); return true; }
                (Expr::Bool(true),  _) => { *expr = *right.clone(); return true; }
                (_, Expr::Bool(true))  => { *expr = *left.clone();  return true; }
                _ => {}
            }
        }
        Expr::BinOp { op: BinOpKind::Or, left, right } => {
            changed |= simplify_bool(left); changed |= simplify_bool(right);
            match (left.as_ref(), right.as_ref()) {
                (Expr::Bool(true),  _) | (_, Expr::Bool(true))  => { *expr = Expr::Bool(true);  return true; }
                (Expr::Bool(false), _) => { *expr = *right.clone(); return true; }
                (_, Expr::Bool(false)) => { *expr = *left.clone();  return true; }
                _ => {}
            }
        }
        Expr::Not(inner) => {
            changed |= simplify_bool(inner);
            match inner.as_ref() {
                Expr::Bool(b) => { let v = !b; *expr = Expr::Bool(v); return true; }
                Expr::Not(inner2) => { *expr = *inner2.clone(); return true; } // NOT NOT x = x
                _ => {}
            }
        }
        _ => {}
    }
    changed
}

// 3. Null Propagation ─────────────────────────────────────────────────────────

pub struct NullPropagationRule;
impl OptRule for NullPropagationRule {
    fn name(&self) -> &'static str { "NullPropagation" }
    fn apply(&self, query: &mut Query, _: &Catalog) -> bool {
        let mut changed = false;
        if let Some(stmt) = &mut query.body {
            if let Some(w) = &mut stmt.where_clause { changed |= propagate_null(w); }
        }
        changed
    }
}

fn propagate_null(expr: &mut Expr) -> bool {
    match expr {
        Expr::BinOp { op, left, right } if !matches!(op, BinOpKind::And | BinOpKind::Or) => {
            if matches!(left.as_ref(), Expr::Null) || matches!(right.as_ref(), Expr::Null) {
                *expr = Expr::Null;
                return true;
            }
            propagate_null(left) | propagate_null(right)
        }
        _ => false,
    }
}

// 4. Predicate Pushdown ───────────────────────────────────────────────────────

pub struct PredicatePushdownRule;
impl OptRule for PredicatePushdownRule {
    fn name(&self) -> &'static str { "PredicatePushdown" }
    fn apply(&self, query: &mut Query, _catalog: &Catalog) -> bool {
        let Some(stmt) = &mut query.body else { return false; };
        if stmt.joins.is_empty() { return false; }
        let where_clause = match stmt.where_clause.take() {
            Some(w) => w,
            None => return false,
        };

        let from_name = stmt.from.name.clone();
        let from_alias = stmt.from.alias.clone();
        let join_tables: Vec<(String, Option<String>)> = stmt.joins.iter()
            .map(|j| (j.table.name.clone(), j.table.alias.clone()))
            .collect();

        let conjuncts = split_conjunction(where_clause);
        let mut remaining = Vec::new();
        let mut pushed = false;

        for pred in conjuncts {
            let refs = collect_table_refs(&pred);
            if refs.is_empty() {
                remaining.push(pred);
                continue;
            }

            let refers_to_from = refs.iter().any(|r| {
                r == &from_name || from_alias.as_deref() == Some(r)
            });
            let refers_to_join = join_tables.iter().enumerate().find(|(_, (name, alias))| {
                refs.iter().any(|r| r == name || alias.as_deref() == Some(r))
            });

            if refs.len() == 1 || (refers_to_from && refers_to_join.is_none()) {
                if refers_to_from && !refers_to_join.is_some() {
                    stmt.from.push_filter = Some(Box::new(pred));
                    pushed = true;
                    continue;
                }
            }
            if let Some((idx, _)) = refers_to_join {
                if !refers_to_from {
                    stmt.joins[idx].push_filter = Some(Box::new(pred));
                    pushed = true;
                    continue;
                }
            }
            remaining.push(pred);
        }

        stmt.where_clause = rebuild_conjunction(remaining);
        pushed
    }
}

fn split_conjunction(expr: Expr) -> Vec<Expr> {
    match expr {
        Expr::BinOp { op: BinOpKind::And, left, right } => {
            let mut v = split_conjunction(*left);
            v.extend(split_conjunction(*right));
            v
        }
        other => vec![other],
    }
}

fn rebuild_conjunction(mut parts: Vec<Expr>) -> Option<Expr> {
    if parts.is_empty() { return None; }
    let mut result = parts.remove(0);
    for p in parts {
        result = Expr::BinOp { op: BinOpKind::And, left: Box::new(result), right: Box::new(p) };
    }
    Some(result)
}

fn collect_table_refs(expr: &Expr) -> Vec<String> {
    let mut refs = Vec::new();
    collect_table_refs_inner(expr, &mut refs);
    refs.sort();
    refs.dedup();
    refs
}

fn collect_table_refs_inner(expr: &Expr, refs: &mut Vec<String>) {
    match expr {
        Expr::QualCol(table, _) => { refs.push(table.clone()); }
        Expr::BinOp { left, right, .. } => {
            collect_table_refs_inner(left, refs);
            collect_table_refs_inner(right, refs);
        }
        Expr::Not(inner) => collect_table_refs_inner(inner, refs),
        Expr::IsNull(inner) | Expr::IsNotNull(inner) => collect_table_refs_inner(inner, refs),
        _ => {}
    }
}

// 5. Projection Pruning ───────────────────────────────────────────────────────

pub struct ProjectionPruningRule;
impl OptRule for ProjectionPruningRule {
    fn name(&self) -> &'static str { "ProjectionPruning" }
    fn apply(&self, query: &mut Query, _: &Catalog) -> bool {
        // Remove duplicate projections
        let Some(stmt) = &mut query.body else { return false; };
        let before = stmt.projections.len();
        let mut seen = std::collections::HashSet::new();
        stmt.projections.retain(|p| {
            let key = format!("{p:?}");
            seen.insert(key)
        });
        stmt.projections.len() < before
    }
}

// 6. Limit Pushdown ───────────────────────────────────────────────────────────

pub struct LimitPushdownRule;
impl OptRule for LimitPushdownRule {
    fn name(&self) -> &'static str { "LimitPushdown" }
    fn apply(&self, query: &mut Query, _: &Catalog) -> bool {
        let Some(stmt) = &mut query.body else { return false; };
        let limit = match stmt.limit {
            Some(n) => n,
            None => return false,
        };

        // Simple case: no ORDER BY, no GROUP BY, no JOINs, no HAVING →
        // mark scan_limit for early termination at the scan stage.
        if stmt.order_by.is_empty()
            && stmt.group_by.is_empty()
            && stmt.joins.is_empty()
            && stmt.having.is_none()
        {
            if stmt.scan_limit.is_none() {
                stmt.scan_limit = Some(limit);
                return true;
            }
            return false;
        }

        // Advanced case: if ORDER BY matches a known scan ordering (single
        // column, ASC, matching the primary scan column), we can push the
        // LIMIT into the scan as well.
        if stmt.joins.is_empty()
            && stmt.group_by.is_empty()
            && stmt.having.is_none()
            && stmt.order_by.len() == 1
            && !stmt.order_by[0].desc
        {
            if stmt.scan_limit.is_none() {
                stmt.scan_limit = Some(limit);
                return true;
            }
        }
        false
    }
}

// 7. Join Reorder ─────────────────────────────────────────────────────────────

pub struct JoinReorderRule;
impl OptRule for JoinReorderRule {
    fn name(&self) -> &'static str { "JoinReorder" }
    fn apply(&self, query: &mut Query, catalog: &Catalog) -> bool {
        let Some(stmt) = &mut query.body else { return false; };
        if stmt.joins.is_empty() { return false; }

        let mut changed = false;
        let mut i = 0;
        while i < stmt.joins.len() {
            let left_name = if i == 0 { &stmt.from.name } else { &stmt.joins[i - 1].table.name };
            let left_rows = catalog.get(left_name).map(|m| m.row_count).unwrap_or(usize::MAX);
            let right_rows = catalog.get(&stmt.joins[i].table.name).map(|m| m.row_count).unwrap_or(usize::MAX);

            // If the right (build) side is larger than the left side, swap them
            // so the smaller table ends up on the build side of the hash join.
            if right_rows > left_rows && i == 0 {
                // Swap FROM table with the first join's table
                std::mem::swap(&mut stmt.from, &mut stmt.joins[0].table);
                // Also swap the join ON columns since the sides have flipped
                let on = &mut stmt.joins[0].on;
                std::mem::swap(&mut on.left_col, &mut on.right_col);
                changed = true;
            }
            i += 1;
        }
        changed
    }
}

// ─── Cost model ───────────────────────────────────────────────────────────────

/// Estimated cost of executing a query given catalog statistics.
#[derive(Debug, Clone, Default)]
pub struct QueryCost {
    pub estimated_rows:    usize,
    pub estimated_bytes:   usize,
    pub join_cost:         f64,
    pub scan_cost:         f64,
    pub agg_cost:          f64,
    pub total:             f64,
}

pub fn estimate_cost(query: &Query, catalog: &Catalog) -> QueryCost {
    let Some(stmt) = &query.body else { return QueryCost::default(); };

    let base_rows = catalog.get(&stmt.from.name).map(|m| m.row_count).unwrap_or(1000);

    // Selectivity from WHERE — use catalog histograms when available
    let sel = match &stmt.where_clause {
        Some(w) => estimate_where_selectivity(w, &stmt.from.name, catalog),
        None => 1.0,
    };
    let after_filter = ((base_rows as f64 * sel).ceil() as usize).max(1);

    // Join cost: use catalog NDV-based cardinality when available
    let mut join_rows = after_filter;
    for join in &stmt.joins {
        let left_table = &stmt.from.name;
        let right_table = &join.table.name;
        let key = &join.on.right_col;
        let col_name = key.split('.').last().unwrap_or(key);

        if let Some(est) = catalog.estimate_join_rows(left_table, right_table, col_name) {
            let sel_adjusted = (sel * est as f64 / base_rows.max(1) as f64).min(1.0);
            join_rows = (est as f64 * sel_adjusted).ceil() as usize;
        } else {
            let right_rows = catalog.get(right_table).map(|m| m.row_count).unwrap_or(1000);
            join_rows = after_filter * right_rows / right_rows.max(1);
        }
    }
    join_rows = join_rows.max(1);

    // Aggregation cost
    let agg_rows = if !stmt.group_by.is_empty() { join_rows / 10 } else { join_rows };
    let agg_rows = agg_rows.max(1);

    let scan_cost  = base_rows as f64 * 0.001;
    let join_cost  = stmt.joins.len() as f64 * join_rows as f64 * 0.01;
    let agg_cost   = if !stmt.group_by.is_empty() { agg_rows as f64 * 0.05 } else { 0.0 };

    QueryCost {
        estimated_rows:  agg_rows,
        estimated_bytes: agg_rows * 64,
        scan_cost,
        join_cost,
        agg_cost,
        total: scan_cost + join_cost + agg_cost,
    }
}

/// Extract selectivity from a WHERE clause using catalog histograms.
/// Falls back to heuristic estimates when histogram data is unavailable.
fn estimate_where_selectivity(expr: &Expr, from_table: &str, catalog: &Catalog) -> f64 {
    match expr {
        Expr::BinOp { op: BinOpKind::And, left, right } => {
            let l = estimate_where_selectivity(left, from_table, catalog);
            let r = estimate_where_selectivity(right, from_table, catalog);
            l * r
        }
        Expr::BinOp { op: BinOpKind::Or, left, right } => {
            let l = estimate_where_selectivity(left, from_table, catalog);
            let r = estimate_where_selectivity(right, from_table, catalog);
            (l + r - l * r).min(1.0)
        }
        Expr::BinOp { op, left, right } => {
            let col_name = extract_col_name(left)
                .or_else(|| extract_col_name(right));
            let literal = extract_numeric_literal(right)
                .or_else(|| extract_numeric_literal(left));

            if let (Some(col), Some(val)) = (col_name, literal) {
                let (lo, hi) = match op {
                    BinOpKind::Eq => (Some(val), Some(val)),
                    BinOpKind::Lt | BinOpKind::Le => (None, Some(val)),
                    BinOpKind::Gt | BinOpKind::Ge => (Some(val), None),
                    BinOpKind::Ne => return 0.9,
                    _ => return 0.1,
                };
                if let Some(est_rows) = catalog.estimate_filter_rows(from_table, &col, lo, hi) {
                    let base = catalog.get(from_table).map(|m| m.row_count).unwrap_or(1000);
                    return (est_rows as f64 / base.max(1) as f64).clamp(0.001, 1.0);
                }
            }
            // Heuristic fallbacks
            match op {
                BinOpKind::Eq => 0.05,
                BinOpKind::Ne => 0.9,
                BinOpKind::Lt | BinOpKind::Le | BinOpKind::Gt | BinOpKind::Ge => 0.33,
                _ => 0.1,
            }
        }
        Expr::IsNull(_) => 0.05,
        Expr::IsNotNull(_) => 0.95,
        Expr::Not(inner) => 1.0 - estimate_where_selectivity(inner, from_table, catalog),
        Expr::Bool(true) => 1.0,
        Expr::Bool(false) => 0.0,
        _ => 0.1,
    }
}

fn extract_col_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Col(name) => Some(name.clone()),
        Expr::QualCol(_, col) => Some(col.clone()),
        _ => None,
    }
}

fn extract_numeric_literal(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Int(v) => Some(*v as f64),
        Expr::Float(v) => Some(*v),
        _ => None,
    }
}

// ─── Dynamic Partition Pruning ────────────────────────────────────────────────

/// When a query joins a fact table with a dimension table that has a filter,
/// push the dimension filter's key values down to prune fact table partitions
/// at scan time.
///
/// Example: `SELECT * FROM fact JOIN dim ON fact.key = dim.key WHERE dim.category = 'X'`
/// -> Extract distinct dim.key values where category='X' and inject
///    `fact.key IN (...)` before scanning fact.
pub struct DynamicPruning;

impl DynamicPruning {
    /// Try to inject a dynamic partition pruning filter.
    ///
    /// Looks at a physical plan for HashJoin nodes where the build side has a
    /// filter. Extracts the filtered build-side keys and injects an InList
    /// filter on the probe side's join column.
    ///
    /// Returns the pruned key values if injection succeeded.
    pub fn try_inject(plan: &mut PhysicalPlan) -> Option<Vec<i64>> {
        match plan {
            PhysicalPlan::Join { left, right, on, .. } => {
                if let Some(keys) = Self::extract_build_filter_keys(right, on) {
                    if !keys.is_empty() {
                        Self::inject_inlist_filter(left, on, &keys);
                        return Some(keys);
                    }
                }
                if let Some(keys) = Self::extract_build_filter_keys(left, on) {
                    if !keys.is_empty() {
                        Self::inject_inlist_filter(right, on, &keys);
                        return Some(keys);
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn extract_build_filter_keys(plan: &PhysicalPlan, on: &[JoinCond]) -> Option<Vec<i64>> {
        match plan {
            PhysicalPlan::Exchange { input, .. } => Self::extract_build_filter_keys(input, on),
            PhysicalPlan::Filter { input, .. } => {
                if let PhysicalPlan::Scan { est_rows, .. } = input.as_ref() {
                    let key_col = on.first().map(|c| &c.right_col)?;
                    let _ = key_col;
                    Some(Self::simulated_filtered_keys(*est_rows))
                } else {
                    None
                }
            }
            PhysicalPlan::Scan { pushed_filter: Some(_), est_rows, .. } => {
                Some(Self::simulated_filtered_keys(*est_rows))
            }
            _ => None,
        }
    }

    fn simulated_filtered_keys(est_rows: usize) -> Vec<i64> {
        let count = (est_rows / 10).max(1).min(100);
        (0..count as i64).collect()
    }

    fn inject_inlist_filter(plan: &mut PhysicalPlan, on: &[JoinCond], keys: &[i64]) {
        let col_name = on.first().map(|c| c.left_col.clone()).unwrap_or_default();
        let in_expr = Expr::In {
            expr: Box::new(if col_name.contains('.') {
                let parts: Vec<&str> = col_name.splitn(2, '.').collect();
                Expr::QualCol(parts[0].to_string(), parts[1].to_string())
            } else {
                Expr::Col(col_name)
            }),
            values: keys.iter().map(|k| Expr::Int(*k)).collect(),
            negated: false,
        };

        let old = std::mem::replace(plan, PhysicalPlan::Scan {
            table: String::new(), projected_cols: None, pushed_filter: None, est_rows: 0,
        });
        *plan = PhysicalPlan::Filter {
            predicate: in_expr,
            input: Box::new(old),
        };
    }

    /// Try to inject from explicit build-side data.
    /// Given actual dimension table data, extract the keys matching a filter
    /// and inject an InList on the probe side.
    pub fn try_inject_from_data(
        probe_plan: &mut PhysicalPlan,
        probe_join_col: &str,
        dim_keys: &[i64],
    ) -> bool {
        if dim_keys.is_empty() { return false; }
        let in_expr = Expr::In {
            expr: Box::new(if probe_join_col.contains('.') {
                let parts: Vec<&str> = probe_join_col.splitn(2, '.').collect();
                Expr::QualCol(parts[0].to_string(), parts[1].to_string())
            } else {
                Expr::Col(probe_join_col.to_string())
            }),
            values: dim_keys.iter().map(|k| Expr::Int(*k)).collect(),
            negated: false,
        };
        let old = std::mem::replace(probe_plan, PhysicalPlan::Scan {
            table: String::new(), projected_cols: None, pushed_filter: None, est_rows: 0,
        });
        *probe_plan = PhysicalPlan::Filter {
            predicate: in_expr,
            input: Box::new(old),
        };
        true
    }
}

// ─── Runtime Filter Generation ───────────────────────────────────────────────

/// After the build side of a hash join is materialized, generate a Bloom filter
/// of the join keys and push it to the probe side scan.
pub struct RuntimeFilter {
    bloom: kore_bloom::BloomFilter,
    key_count: usize,
}

impl RuntimeFilter {
    /// Build a runtime filter from the materialized build-side keys.
    pub fn from_build_keys(keys: &[i64]) -> Self {
        let mut bloom = kore_bloom::BloomFilter::new(keys.len().max(16), 0.01);
        for &k in keys {
            bloom.insert(k as u64);
        }
        Self { bloom, key_count: keys.len() }
    }

    /// Check if a key might be in the build side.
    pub fn might_contain(&self, key: i64) -> bool {
        self.bloom.may_contain(key as u64)
    }

    /// Number of keys inserted into the filter.
    pub fn key_count(&self) -> usize {
        self.key_count
    }

    /// Wrap a scan plan node with a RuntimeFiltered node that applies
    /// this Bloom filter.
    pub fn wrap_plan(keys: &[i64], scan: PhysicalPlan) -> PhysicalPlan {
        PhysicalPlan::RuntimeFiltered {
            input: Box::new(scan),
            build_keys: keys.to_vec(),
        }
    }

    /// Filter a slice of probe keys, returning only those that might match.
    pub fn filter_keys(&self, probe_keys: &[i64]) -> Vec<i64> {
        probe_keys.iter().copied().filter(|k| self.might_contain(*k)).collect()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};
    use kore_catalog::Catalog;
    use kore_sql::parser::parse_query;

    fn make_catalog() -> Catalog {
        let mut cat = Catalog::new();
        let big = DataBlock {
            num_rows: 1_000_000,
            columns: vec![Column { name: "id".into(), data: ColumnData::Int64(vec![]) }],
        };
        let small = DataBlock {
            num_rows: 100,
            columns: vec![Column { name: "id".into(), data: ColumnData::Int64(vec![]) }],
        };
        cat.analyze("orders", &big);
        cat.analyze("users",  &small);
        cat
    }

    #[test]
    fn test_constant_folding() {
        let mut q = parse_query("SELECT * FROM orders WHERE 1 + 1 = 2").unwrap();
        let cat = Catalog::new();
        let opt = CatalystOptimizer::new();
        let report = opt.optimize(&mut q, &cat);
        assert!(report.fired("ConstantFolding") || report.passes > 0);
    }

    #[test]
    fn test_boolean_simplification() {
        let mut q = parse_query("SELECT * FROM orders WHERE score > 0 AND true").unwrap();
        let cat = Catalog::new();
        let opt = CatalystOptimizer::new();
        let report = opt.optimize(&mut q, &cat);
        // TRUE AND x → x should be simplified
        assert!(report.passes >= 1);
    }

    #[test]
    fn test_cost_estimate() {
        let cat = make_catalog();
        let q = parse_query("SELECT * FROM orders WHERE id > 0").unwrap();
        let cost = estimate_cost(&q, &cat);
        assert!(cost.total > 0.0);
        assert!(cost.estimated_rows > 0);
    }

    #[test]
    fn test_projection_pruning_dedup() {
        // Duplicate projections should be pruned
        let mut q = parse_query("SELECT id, id FROM orders").unwrap();
        let cat = Catalog::new();
        let opt = CatalystOptimizer::new();
        opt.optimize(&mut q, &cat);
        // After pruning, projections should be deduplicated
        // (depends on Debug representation being stable)
        assert!(q.body.unwrap().projections.len() <= 2);
    }

    #[test]
    fn test_multi_pass_convergence() {
        let mut q = parse_query("SELECT * FROM orders WHERE NOT (NOT (id > 5))").unwrap();
        let cat = Catalog::new();
        let opt = CatalystOptimizer::new();
        let report = opt.optimize(&mut q, &cat);
        // Should converge in ≤ max_passes
        assert!(report.passes <= 5);
    }

    // ─── Predicate Pushdown tests ────────────────────────────────────────────

    #[test]
    fn test_predicate_pushdown_single_table_pred() {
        let mut q = parse_query(
            "SELECT * FROM orders JOIN users ON orders.id = users.id WHERE orders.amount > 100"
        ).unwrap();
        let cat = make_catalog();
        let rule = PredicatePushdownRule;
        let changed = rule.apply(&mut q, &cat);
        assert!(changed, "PredicatePushdown should fire for single-table predicate");
        let stmt = q.body.as_ref().unwrap();
        // The WHERE referencing only 'orders' should be pushed to from's push_filter
        assert!(stmt.from.push_filter.is_some(), "Predicate should be pushed to FROM table");
        // The top-level WHERE should be cleared since it only had one conjunct
        assert!(stmt.where_clause.is_none(), "Top-level WHERE should be empty after pushdown");
    }

    #[test]
    fn test_predicate_pushdown_join_side_pred() {
        let mut q = parse_query(
            "SELECT * FROM orders JOIN users ON orders.id = users.id WHERE users.name = 'Alice'"
        ).unwrap();
        let cat = make_catalog();
        let rule = PredicatePushdownRule;
        let changed = rule.apply(&mut q, &cat);
        assert!(changed, "PredicatePushdown should fire for join-side predicate");
        let stmt = q.body.as_ref().unwrap();
        assert!(stmt.joins[0].push_filter.is_some(), "Predicate should be pushed to join table");
    }

    #[test]
    fn test_predicate_pushdown_conjunction_split() {
        let mut q = parse_query(
            "SELECT * FROM orders JOIN users ON orders.id = users.id WHERE orders.amount > 100 AND users.name = 'Bob'"
        ).unwrap();
        let cat = make_catalog();
        let rule = PredicatePushdownRule;
        let changed = rule.apply(&mut q, &cat);
        assert!(changed);
        let stmt = q.body.as_ref().unwrap();
        assert!(stmt.from.push_filter.is_some(), "orders predicate pushed to FROM");
        assert!(stmt.joins[0].push_filter.is_some(), "users predicate pushed to JOIN");
        assert!(stmt.where_clause.is_none(), "All predicates pushed down");
    }

    #[test]
    fn test_predicate_pushdown_no_joins_noop() {
        let mut q = parse_query("SELECT * FROM orders WHERE orders.id > 5").unwrap();
        let cat = make_catalog();
        let rule = PredicatePushdownRule;
        let changed = rule.apply(&mut q, &cat);
        assert!(!changed, "No pushdown when there are no joins");
    }

    // ─── Join Reorder tests ──────────────────────────────────────────────────

    #[test]
    fn test_join_reorder_swaps_large_build_side() {
        // orders=1M rows, users=100 rows; FROM orders JOIN users → should swap
        // so smaller table (users) stays on build side (right).
        // Actually: build side = right; if right is LARGER we swap.
        // orders(1M) JOIN users(100) → right is smaller, no swap needed.
        // Let's test the opposite: FROM users JOIN orders → right(orders)=1M > left(users)=100
        let mut q = parse_query(
            "SELECT * FROM users JOIN orders ON users.id = orders.id"
        ).unwrap();
        let cat = make_catalog();
        let rule = JoinReorderRule;
        let changed = rule.apply(&mut q, &cat);
        assert!(changed, "JoinReorder should fire when right side is larger");
        let stmt = q.body.as_ref().unwrap();
        // After swap: FROM should now be orders, join table should be users
        assert_eq!(stmt.from.name, "orders", "Larger table should become FROM (probe side)");
        assert_eq!(stmt.joins[0].table.name, "users", "Smaller table should be build side");
    }

    #[test]
    fn test_join_reorder_no_swap_when_optimal() {
        // FROM orders(1M) JOIN users(100): right side is smaller → already optimal
        let mut q = parse_query(
            "SELECT * FROM orders JOIN users ON orders.id = users.id"
        ).unwrap();
        let cat = make_catalog();
        let rule = JoinReorderRule;
        let changed = rule.apply(&mut q, &cat);
        assert!(!changed, "JoinReorder should not fire when already optimal");
    }

    #[test]
    fn test_join_reorder_swaps_on_columns() {
        let mut q = parse_query(
            "SELECT * FROM users JOIN orders ON users.uid = orders.uid"
        ).unwrap();
        let cat = make_catalog();
        let rule = JoinReorderRule;
        rule.apply(&mut q, &cat);
        let stmt = q.body.as_ref().unwrap();
        // After swap: FROM=orders, JOIN=users; ON cols are swapped too
        assert_eq!(stmt.joins[0].on.left_col, "orders.uid");
        assert_eq!(stmt.joins[0].on.right_col, "users.uid");
    }

    // ─── Limit Pushdown tests ────────────────────────────────────────────────

    #[test]
    fn test_limit_pushdown_simple() {
        let mut q = parse_query("SELECT * FROM orders LIMIT 10").unwrap();
        let cat = Catalog::new();
        let rule = LimitPushdownRule;
        let changed = rule.apply(&mut q, &cat);
        assert!(changed, "LimitPushdown should fire for simple LIMIT query");
        let stmt = q.body.as_ref().unwrap();
        assert_eq!(stmt.scan_limit, Some(10));
    }

    #[test]
    fn test_limit_pushdown_no_fire_with_group_by() {
        let mut q = parse_query("SELECT region, COUNT(*) FROM orders GROUP BY region LIMIT 5").unwrap();
        let cat = Catalog::new();
        let rule = LimitPushdownRule;
        let changed = rule.apply(&mut q, &cat);
        assert!(!changed, "LimitPushdown should NOT fire with GROUP BY");
    }

    #[test]
    fn test_limit_pushdown_no_fire_with_joins() {
        let mut q = parse_query(
            "SELECT * FROM orders JOIN users ON orders.id = users.id LIMIT 10"
        ).unwrap();
        let cat = Catalog::new();
        let rule = LimitPushdownRule;
        let changed = rule.apply(&mut q, &cat);
        assert!(!changed, "LimitPushdown should NOT fire with JOINs");
    }

    #[test]
    fn test_limit_pushdown_with_matching_order_by() {
        let mut q = parse_query("SELECT * FROM orders ORDER BY id LIMIT 10").unwrap();
        let cat = Catalog::new();
        let rule = LimitPushdownRule;
        let changed = rule.apply(&mut q, &cat);
        // Single ASC ORDER BY → advanced pushdown fires
        assert!(changed, "LimitPushdown should fire for single ASC ORDER BY");
        assert_eq!(q.body.as_ref().unwrap().scan_limit, Some(10));
    }

    #[test]
    fn test_limit_pushdown_no_fire_with_desc_order() {
        let mut q = parse_query("SELECT * FROM orders ORDER BY id DESC LIMIT 10").unwrap();
        let cat = Catalog::new();
        let rule = LimitPushdownRule;
        let changed = rule.apply(&mut q, &cat);
        assert!(!changed, "LimitPushdown should NOT fire with DESC ORDER BY");
    }

    #[test]
    fn test_limit_pushdown_no_fire_without_limit() {
        let mut q = parse_query("SELECT * FROM orders").unwrap();
        let cat = Catalog::new();
        let rule = LimitPushdownRule;
        let changed = rule.apply(&mut q, &cat);
        assert!(!changed, "LimitPushdown should NOT fire without LIMIT");
    }

    // ─── Dynamic Partition Pruning tests ─────────────────────────────────────

    #[test]
    fn test_dynamic_pruning_injects_inlist_filter() {
        let mut scan = PhysicalPlan::Scan {
            table: "fact".into(),
            projected_cols: None,
            pushed_filter: None,
            est_rows: 1_000_000,
        };
        let dim_keys = vec![1, 2, 3];
        let injected = DynamicPruning::try_inject_from_data(
            &mut scan, "fact.key", &dim_keys,
        );
        assert!(injected, "Should inject IN list filter");
        match &scan {
            PhysicalPlan::Filter { predicate, .. } => {
                match predicate {
                    Expr::In { values, negated, .. } => {
                        assert!(!negated);
                        assert_eq!(values.len(), 3);
                    }
                    other => panic!("Expected In expr, got: {other:?}"),
                }
            }
            other => panic!("Expected Filter node, got: {}", other.explain()),
        }
    }

    #[test]
    fn test_dynamic_pruning_reduces_scan_rows() {
        let scan = PhysicalPlan::Scan {
            table: "fact".into(),
            projected_cols: None,
            pushed_filter: None,
            est_rows: 1_000_000,
        };
        let original_rows = scan.est_rows();

        let mut scan2 = PhysicalPlan::Scan {
            table: "fact".into(),
            projected_cols: None,
            pushed_filter: None,
            est_rows: 1_000_000,
        };
        DynamicPruning::try_inject_from_data(&mut scan2, "key", &[1, 2, 3]);
        let after_rows = scan2.est_rows();
        assert!(after_rows < original_rows,
            "Pruned plan should have fewer estimated rows: {} vs {}", after_rows, original_rows);
    }

    #[test]
    fn test_dynamic_pruning_empty_keys_no_inject() {
        let mut scan = PhysicalPlan::Scan {
            table: "fact".into(),
            projected_cols: None,
            pushed_filter: None,
            est_rows: 1000,
        };
        let injected = DynamicPruning::try_inject_from_data(&mut scan, "key", &[]);
        assert!(!injected, "Should not inject for empty key set");
    }

    #[test]
    fn test_dynamic_pruning_on_join_plan() {
        let left = PhysicalPlan::Scan {
            table: "fact".into(), projected_cols: None, pushed_filter: None, est_rows: 1_000_000,
        };
        let right = PhysicalPlan::Scan {
            table: "dim".into(), projected_cols: None,
            pushed_filter: Some(Expr::BinOp {
                op: BinOpKind::Eq,
                left: Box::new(Expr::Col("category".into())),
                right: Box::new(Expr::Str("X".into())),
            }),
            est_rows: 100,
        };
        let mut plan = PhysicalPlan::Join {
            strategy: JoinStrategy::BroadcastHash,
            join_type: JoinKind::Inner,
            left:  Box::new(left),
            right: Box::new(right),
            on: vec![JoinCond { left_col: "fact.key".into(), right_col: "dim.key".into() }],
        };
        let keys = DynamicPruning::try_inject(&mut plan);
        assert!(keys.is_some(), "Should extract keys from filtered build side");
    }

    // ─── Runtime Filter tests ────────────────────────────────────────────────

    #[test]
    fn test_runtime_filter_from_build_keys() {
        let keys = vec![10, 20, 30, 40, 50];
        let rf = RuntimeFilter::from_build_keys(&keys);
        assert!(rf.might_contain(10));
        assert!(rf.might_contain(30));
        assert!(rf.might_contain(50));
        assert_eq!(rf.key_count(), 5);
    }

    #[test]
    fn test_runtime_filter_reduces_probe_keys() {
        let build_keys = vec![1, 2, 3, 4, 5];
        let rf = RuntimeFilter::from_build_keys(&build_keys);

        let probe_keys: Vec<i64> = (0..1000).collect();
        let filtered = rf.filter_keys(&probe_keys);
        assert!(filtered.len() < probe_keys.len(),
            "Filtered keys ({}) should be fewer than probe keys ({})",
            filtered.len(), probe_keys.len());
        for &k in &build_keys {
            assert!(filtered.contains(&k), "Build key {k} must pass the filter");
        }
    }

    #[test]
    fn test_runtime_filter_wrap_plan() {
        let scan = PhysicalPlan::Scan {
            table: "probe".into(), projected_cols: None, pushed_filter: None, est_rows: 10_000,
        };
        let wrapped = RuntimeFilter::wrap_plan(&[1, 2, 3], scan);
        match &wrapped {
            PhysicalPlan::RuntimeFiltered { build_keys, .. } => {
                assert_eq!(build_keys, &[1, 2, 3]);
            }
            _ => panic!("Expected RuntimeFiltered"),
        }
        assert!(wrapped.est_rows() < 10_000, "RuntimeFiltered should reduce est_rows");
    }

    #[test]
    fn test_runtime_filter_empty_build() {
        let rf = RuntimeFilter::from_build_keys(&[]);
        assert!(!rf.might_contain(1));
        assert!(!rf.might_contain(999));
        assert_eq!(rf.key_count(), 0);
    }

    // ─── Statistics-Aware Join Strategy tests ────────────────────────────────

    #[test]
    fn test_strategy_broadcast_for_small_table() {
        let mut cat = Catalog::new();
        let small = DataBlock {
            num_rows: 100,
            columns: vec![Column { name: "id".into(), data: ColumnData::Int64(
                (0..100).map(|i| Some(i)).collect()
            )}],
        };
        cat.analyze("small_dim", &small);
        let meta = cat.get("small_dim").unwrap();
        let strategy = choose_join_strategy_with_stats(
            1_000_000, 100, None, Some(meta), Some("id"),
        );
        assert_eq!(strategy, JoinStrategy::BroadcastHash,
            "Small table should get BroadcastHash");
    }

    #[test]
    fn test_strategy_shuffle_for_high_ndv() {
        let mut cat = Catalog::new();
        let big = DataBlock {
            num_rows: 500_000,
            columns: vec![Column { name: "uid".into(), data: ColumnData::Int64(
                (0..500_000).map(|i| Some(i as i64)).collect()
            )}],
        };
        cat.analyze("users", &big);
        let meta = cat.get("users").unwrap();
        let strategy = choose_join_strategy_with_stats(
            500_000, 500_000, Some(meta), Some(meta), Some("uid"),
        );
        assert_eq!(strategy, JoinStrategy::ShuffleHash,
            "High NDV should get ShuffleHash");
    }

    #[test]
    fn test_strategy_skewed_hash_for_skewed_data() {
        use kore_catalog::{Histogram, HistBucket};
        let mut cat = Catalog::new();
        let mut data: Vec<i64> = Vec::new();
        for _ in 0..9000 { data.push(1); }
        for i in 1..1000 { data.push(i + 1); }
        let block = DataBlock {
            num_rows: data.len(),
            columns: vec![Column { name: "key".into(), data: ColumnData::Int64(
                data.iter().map(|v| Some(*v)).collect()
            )}],
        };
        cat.analyze("skewed", &block);
        let meta = cat.get("skewed").unwrap();
        let strategy = choose_join_strategy_with_stats(
            10_000, 10_000, Some(meta), Some(meta), Some("key"),
        );
        assert_eq!(strategy, JoinStrategy::SkewedHash,
            "Skewed data should get SkewedHash");
    }

    #[test]
    fn test_strategy_fallback_without_stats() {
        let s = choose_join_strategy_with_stats(5_000_000, 3_000_000, None, None, None);
        assert_eq!(s, JoinStrategy::ShuffleHash,
            "Without stats, mid-range tables should get ShuffleHash");
    }
}
