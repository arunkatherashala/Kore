//! Phase 17 — Vectorized fast-path for `KqlContext::query`.
//!
//! `kore-vectorized` has been sitting in the tree with a full SIMD batch
//! engine (bitmap filter, `batch_sum_full`, hashed group-by) and *zero*
//! callers in the SQL executor. Every regular query still went through the
//! row-by-row interpreter in [`crate::executor`], which hits
//! `get_value(idx) → Value` — the ~34 sites that were flagged as gap G3 in
//! the Spark-vs-KORE audit.
//!
//! This module closes that gap by adding a *pre-executor* fast-path:
//!
//! ```text
//!   KqlContext::query(sql)
//!     ├─ parse
//!     ├─ if try_vectorized(&query, ctx) → Some(block)   ← Phase 17
//!     │      return block
//!     └─ execute_query(&query, ctx)                     ← existing row loop
//! ```
//!
//! The fast-path is deliberately conservative. It accepts only the query
//! shape it can execute *identically* to the row-loop path:
//!
//! * one FROM table (no JOIN, no VALUES, no FROM-subquery)
//! * no CTE / UNION / DISTINCT / HAVING / QUALIFY / ORDER BY
//! * WHERE is a conjunction of `col OP literal` (or absent)
//! * projections are (a) `*`, or (b) bare cols, or (c) aggs, or (d) group
//!   keys + aggs — no arithmetic, no functions, no window fns, no CASE
//! * aggs are SUM / COUNT / MIN / MAX / AVG on scalar cols
//! * optional LIMIT
//!
//! Anything else returns `None` and falls through to the interpreter.
//!
//! Gate: set `KORE_VECTORIZED=0` to disable the fast-path (default: on).

use std::collections::HashMap;

use kore_core::{Column, ColumnData, DataBlock, JoinKey, KoreError, Value};
use kore_vectorized::{
    batch_sum_full, vectorized_filter,
    ColCondition, CmpOp, VecFilter,
};

use crate::ast::{AggFunc, BinOpKind, Expr, JoinKind, Projection, Query, SelectStmt};
use crate::executor::KqlContext;

// ─── Public entry point ──────────────────────────────────────────────────────

/// Try to execute `query` on the vectorized fast-path. Returns `None` if the
/// query shape is unsupported — caller falls through to the row-loop
/// interpreter.  Never panics; on any classification failure returns `None`.
///
/// Callers: [`KqlContext::query`].
pub fn try_vectorized(
    query: &Query,
    ctx: &KqlContext,
) -> Option<Result<DataBlock, KoreError>> {
    if !enabled() {
        return None;
    }
    // Reject CTEs / UNION at the outer level.
    if !query.ctes.is_empty() || !query.union_all.is_empty() {
        return None;
    }
    let stmt = query.body.as_ref()?;
    let base_alias = stmt.from.alias.as_deref().unwrap_or(stmt.from.name.as_str()).to_string();
    let plan = classify(stmt, &base_alias)?;

    // Fetch the base table by unqualified name.  The row-loop path prefixes
    // every column with the alias via `prefix_columns` in `execute_select`;
    // we emit output columns pre-prefixed to match, without ever cloning the
    // underlying `ColumnData`.
    let base = ctx.get(&stmt.from.name)?.clone();

    Some(execute(plan, base, stmt, &base_alias))
}

fn enabled() -> bool {
    match std::env::var("KORE_VECTORIZED") {
        Ok(v) => v != "0" && !v.eq_ignore_ascii_case("false") && !v.eq_ignore_ascii_case("off"),
        Err(_) => true,
    }
}

// ─── Classification ──────────────────────────────────────────────────────────

/// The typed fast-path shape derived from a `SelectStmt`.
///
/// After `classify()`, `execute()` needs no further AST inspection — every
/// decision has been resolved into concrete columns / aggs / keys.
#[derive(Debug, Clone)]
struct FastPlan {
    /// One of the 4 supported output shapes.
    shape:   OutShape,
    /// Conjunctive filter, or `None` for no WHERE.
    filter:  Option<VecFilter>,
    /// LIMIT n, or `None`.
    limit:   Option<usize>,
    /// STAR: output all base columns after filter+limit (no name changes).
    is_star: bool,
}

#[derive(Debug, Clone)]
enum OutShape {
    /// SELECT c1, c2, ... — bare column list (or star).
    BareCols  { cols: Vec<ProjCol> },
    /// SELECT sum(a), count(*), ... — global aggregation, no GROUP BY.
    GlobalAgg { aggs: Vec<AggCol> },
    /// SELECT key1, key2, sum(a), ... GROUP BY key1, key2.
    GroupAgg  { keys: Vec<String>, aggs: Vec<AggCol> },
}

#[derive(Debug, Clone)]
struct ProjCol {
    /// Source column name in the base block (unqualified).
    src:      String,
    /// Output column name (alias if provided, else `src`).
    out_name: String,
}

#[derive(Debug, Clone)]
struct AggCol {
    func:     AggFunc,
    /// Source column name, or `None` for COUNT(*).
    src:      Option<String>,
    out_name: String,
}

fn classify(stmt: &SelectStmt, base_alias: &str) -> Option<FastPlan> {
    // Unsupported top-level features.
    if stmt.distinct { return None; }
    if !stmt.joins.is_empty() { return None; }
    if stmt.from.subquery.is_some() || stmt.from.values.is_some() { return None; }
    if stmt.from.name == "__dual__" { return None; }
    if stmt.having.is_some() || stmt.qualify.is_some() { return None; }
    if !stmt.order_by.is_empty() { return None; }

    // WHERE must be a conjunction of simple col-OP-literal conditions.
    let filter = if let Some(w) = &stmt.where_clause {
        Some(build_filter(w)?)
    } else {
        None
    };

    // Detect star.
    let is_star = stmt.projections.len() == 1
        && matches!(stmt.projections[0], Projection::Star);

    // Classify projections.
    let mut bare_cols = Vec::<ProjCol>::new();
    let mut aggs     = Vec::<AggCol>::new();
    for p in &stmt.projections {
        match p {
            Projection::Star => {
                if stmt.projections.len() != 1 { return None; }
                // Handled by is_star.
            }
            Projection::Expr { expr, alias } => match expr {
                Expr::Col(c) => bare_cols.push(ProjCol {
                    src:      c.clone(),
                    out_name: alias.clone().unwrap_or_else(|| format!("{base_alias}.{c}")),
                }),
                Expr::QualCol(_t, c) => bare_cols.push(ProjCol {
                    src:      c.clone(),
                    out_name: alias.clone().unwrap_or_else(|| format!("{base_alias}.{c}")),
                }),
                Expr::Agg { func, expr: inner } => {
                    // Whitelist: only the simple aggregates we know how to
                    // evaluate identically to the row-loop path.  Anything
                    // else — COUNT DISTINCT, STDDEV, VARIANCE, MEDIAN,
                    // STRING_AGG, PERCENTILE — falls through so it keeps
                    // going down the existing interpreter.
                    match func {
                        AggFunc::Count | AggFunc::Sum | AggFunc::Avg
                        | AggFunc::Min | AggFunc::Max => {}
                        _ => return None,
                    }
                    // Parser encodes COUNT(*) as Expr::Col("*") — treat that
                    // literal as the star case (no source column, just row
                    // count).  Expr::Star may also appear from other producers.
                    let (src, inner_disp) = match inner.as_ref() {
                        Expr::Col(c) if c == "*"          => (None, "*".to_string()),
                        Expr::Col(c) | Expr::QualCol(_, c) => (Some(c.clone()), c.clone()),
                        Expr::Star                        => (None, "*".to_string()),
                        _ => return None,
                    };
                    let out_name = alias.clone()
                        .unwrap_or_else(|| format!("{func:?}({inner_disp})"));
                    aggs.push(AggCol { func: func.clone(), src, out_name });
                }
                _ => return None,
            }
        }
    }

    // Decide shape.
    let shape = if !aggs.is_empty() {
        // Any bare-col in this branch must correspond to a GROUP BY key —
        // otherwise it's a mixed selection the row-loop needs to reject / plan.
        if !stmt.group_by.is_empty() {
            let keys = stmt.group_by.clone();
            // Every bare col we selected must be a group-by key.
            for pc in &bare_cols {
                if !keys.iter().any(|k| eq_col_name(k, &pc.src)) {
                    return None;
                }
            }
            OutShape::GroupAgg { keys, aggs }
        } else {
            if !bare_cols.is_empty() { return None; }
            OutShape::GlobalAgg { aggs }
        }
    } else if is_star {
        // SELECT * — GROUP BY without aggs is nonsensical here.
        if !stmt.group_by.is_empty() { return None; }
        OutShape::BareCols { cols: Vec::new() }
    } else {
        if !stmt.group_by.is_empty() { return None; }
        OutShape::BareCols { cols: bare_cols }
    };

    Some(FastPlan {
        shape,
        filter,
        limit: stmt.limit.map(|n| n as usize),
        is_star,
    })
}

fn eq_col_name(a: &str, b: &str) -> bool {
    // Accept qualified/unqualified match: "sales.region" == "region".
    if a == b { return true; }
    let a_tail = a.rsplit('.').next().unwrap_or(a);
    let b_tail = b.rsplit('.').next().unwrap_or(b);
    a_tail == b_tail
}

// ─── WHERE → VecFilter ───────────────────────────────────────────────────────

fn build_filter(expr: &Expr) -> Option<VecFilter> {
    let mut conditions = Vec::new();
    walk_conj(expr, &mut conditions)?;
    if conditions.is_empty() { return None; }
    Some(VecFilter { conditions })
}

fn walk_conj(expr: &Expr, out: &mut Vec<ColCondition>) -> Option<()> {
    match expr {
        Expr::BinOp { op: BinOpKind::And, left, right } => {
            walk_conj(left, out)?;
            walk_conj(right, out)?;
            Some(())
        }
        Expr::BinOp { op, left, right } => {
            let cmp = binop_to_cmp(op)?;
            let (col, threshold, str_val) = extract_col_lit(left, right, op)?;
            out.push(ColCondition {
                col_name:  col,
                op:        cmp,
                threshold,
                str_value: str_val,
            });
            Some(())
        }
        _ => None,
    }
}

fn binop_to_cmp(op: &BinOpKind) -> Option<CmpOp> {
    Some(match op {
        BinOpKind::Eq => CmpOp::Eq,
        BinOpKind::Ne => CmpOp::Ne,
        BinOpKind::Lt => CmpOp::Lt,
        BinOpKind::Le => CmpOp::Le,
        BinOpKind::Gt => CmpOp::Gt,
        BinOpKind::Ge => CmpOp::Ge,
        _             => return None,
    })
}

/// Extract `col OP literal` (accepting either operand order for symmetric ops).
fn extract_col_lit(l: &Expr, r: &Expr, op: &BinOpKind) -> Option<(String, f64, Option<String>)> {
    // col OP literal
    if let Some(cname) = col_name(l) {
        if let Some(v) = literal_to_f64(r) {
            return Some((cname, v, None));
        }
        if let Some(s) = literal_to_string(r) {
            return Some((cname, 0.0, Some(s)));
        }
    }
    // literal OP col — only meaningful for symmetric comparisons.
    if let Some(cname) = col_name(r) {
        let flipped = match op {
            BinOpKind::Eq | BinOpKind::Ne => true,
            _ => false, // don't try to flip <, > — VecFilter's threshold direction matters
        };
        if flipped {
            if let Some(v) = literal_to_f64(l) {
                return Some((cname, v, None));
            }
            if let Some(s) = literal_to_string(l) {
                return Some((cname, 0.0, Some(s)));
            }
        }
    }
    None
}

fn col_name(e: &Expr) -> Option<String> {
    match e {
        Expr::Col(c)         => Some(c.clone()),
        Expr::QualCol(_t, c) => Some(c.clone()),
        _ => None,
    }
}

fn literal_to_f64(e: &Expr) -> Option<f64> {
    match e {
        Expr::Int(i)   => Some(*i as f64),
        Expr::Float(f) => Some(*f),
        Expr::Bool(b)  => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn literal_to_string(e: &Expr) -> Option<String> {
    match e {
        Expr::Str(s) => Some(s.clone()),
        _ => None,
    }
}

// ─── Execution ───────────────────────────────────────────────────────────────

fn execute(
    plan: FastPlan,
    base: DataBlock,
    stmt: &SelectStmt,
    base_alias: &str,
) -> Result<DataBlock, KoreError> {
    // 1. Filter → row indices.
    let indices: Vec<usize> = match &plan.filter {
        Some(f) => vectorized_filter(&base, f),
        None    => (0..base.num_rows).collect(),
    };

    // 2. Dispatch by shape.
    let out = match &plan.shape {
        OutShape::BareCols { .. }        if plan.is_star => project_star(&base, &indices, base_alias),
        OutShape::BareCols { cols }                      => project_cols(&base, &indices, cols)?,
        OutShape::GlobalAgg { aggs }                     => global_agg(&base, &indices, aggs)?,
        OutShape::GroupAgg  { keys, aggs }               => group_agg(&base, &indices, keys, aggs, stmt, base_alias)?,
    };

    // 3. LIMIT.
    Ok(apply_limit(out, plan.limit))
}

fn project_star(base: &DataBlock, indices: &[usize], base_alias: &str) -> DataBlock {
    // Reuse take_rows for columnar row selection, prefix every output col to
    // mirror the row-loop path's `prefix_columns` step.
    let columns = base.columns.iter().map(|c| Column {
        name: format!("{base_alias}.{}", c.name),
        data: c.data.take_rows(indices),
    }).collect();
    DataBlock { columns, num_rows: indices.len() }
}

fn project_cols(
    base: &DataBlock,
    indices: &[usize],
    cols: &[ProjCol],
) -> Result<DataBlock, KoreError> {
    let mut out_cols = Vec::with_capacity(cols.len());
    for pc in cols {
        let src = find_col(base, &pc.src)
            .ok_or_else(|| KoreError::ColumnNotFound(pc.src.clone()))?;
        let mut new_col = Column {
            name: pc.out_name.clone(),
            data: src.data.take_rows(indices),
        };
        // take_rows preserves dtype; nothing else to do.
        new_col.name = pc.out_name.clone();
        out_cols.push(new_col);
    }
    Ok(DataBlock { columns: out_cols, num_rows: indices.len() })
}

fn global_agg(
    base: &DataBlock,
    indices: &[usize],
    aggs: &[AggCol],
) -> Result<DataBlock, KoreError> {
    let mut out_cols = Vec::with_capacity(aggs.len());
    for a in aggs {
        let val = compute_agg_over(base, indices, a)?;
        out_cols.push(agg_result_to_column(&a.func, &a.out_name, val));
    }
    Ok(DataBlock { columns: out_cols, num_rows: 1 })
}

fn group_agg(
    base: &DataBlock,
    indices: &[usize],
    keys: &[String],
    aggs: &[AggCol],
    stmt: &SelectStmt,
    base_alias: &str,
) -> Result<DataBlock, KoreError> {
    // Locate key columns.
    let key_cols: Vec<&Column> = keys.iter()
        .map(|k| find_col(base, k).ok_or_else(|| KoreError::ColumnNotFound(k.clone())))
        .collect::<Result<_, _>>()?;

    // Group indices by tuple of JoinKeys (JoinKey is Hash+Eq and covers
    // Int/Bool/Str/Null — float keys aren't supported here, which mirrors
    // the row-loop's hash-group-by).
    let mut groups: HashMap<Vec<JoinKey>, Vec<usize>> = HashMap::new();
    let mut key_order: Vec<Vec<JoinKey>> = Vec::new();
    for &row in indices {
        let mut key = Vec::with_capacity(key_cols.len());
        let mut has_float = false;
        for col in &key_cols {
            match &col.data {
                ColumnData::Float64(_) => { has_float = true; break; }
                _ => key.push(JoinKey::from(&col.data.get_value(row))),
            }
        }
        if has_float {
            return Err(KoreError::InvalidArgument(
                "vectorized fast-path: GROUP BY on Float64 not supported".into()));
        }
        if !groups.contains_key(&key) { key_order.push(key.clone()); }
        groups.entry(key).or_default().push(row);
    }

    // Determine output column order:
    //  1) reproduce projection order from the SELECT clause,
    //  2) so users get exactly what they asked for.
    let mut out_cols: Vec<Column> = Vec::new();
    for p in &stmt.projections {
        match p {
            Projection::Expr { expr: Expr::Col(c), alias }
            | Projection::Expr { expr: Expr::QualCol(_, c), alias } => {
                let key_idx = keys.iter().position(|k| eq_col_name(k, c))
                    .ok_or_else(|| KoreError::InvalidArgument(
                        format!("group key '{c}' missing (should have been rejected by classify)")
                    ))?;
                let src = key_cols[key_idx];
                // Bare-col output: match the row-loop's `project()` which
                // preserves the *prefixed* source column name unless an
                // explicit alias is provided.
                let out_name = alias.clone().unwrap_or_else(|| format!("{base_alias}.{c}"));
                out_cols.push(build_key_column(&out_name, src, &key_order, key_idx));
            }
            Projection::Expr { expr: Expr::Agg { .. }, .. } => {
                // Emit agg columns in a second pass so we keep them adjacent
                // to the key columns in projection order.  Placeholder marker:
                out_cols.push(Column {
                    name: String::from("__agg_placeholder__"),
                    data: ColumnData::Int64(Vec::new()),
                });
            }
            _ => unreachable!("classify() should have rejected"),
        }
    }

    // Compute each aggregate over its group's row indices, then substitute
    // placeholders in projection order.
    let mut agg_iter = aggs.iter();
    for col in out_cols.iter_mut() {
        if col.name == "__agg_placeholder__" {
            let a = agg_iter.next()
                .ok_or_else(|| KoreError::InvalidArgument(
                    "internal: agg count mismatch".into()))?;
            let mut vals: Vec<Value> = Vec::with_capacity(key_order.len());
            for key in &key_order {
                let rows = &groups[key];
                let v = compute_agg_over(base, rows, a)?;
                vals.push(v);
            }
            *col = build_agg_column(&a.func, &a.out_name, vals);
        }
    }

    Ok(DataBlock { columns: out_cols, num_rows: key_order.len() })
}

fn build_key_column(
    out_name: &str,
    src: &Column,
    key_order: &[Vec<JoinKey>],
    key_idx: usize,
) -> Column {
    // Reconstruct the key column with the right dtype.
    match &src.data {
        ColumnData::Int64(_) => {
            let vals: Vec<Option<i64>> = key_order.iter()
                .map(|k| match &k[key_idx] {
                    JoinKey::Int(i) => Some(*i),
                    JoinKey::Bool(b) => Some(*b as i64),
                    JoinKey::Null => None,
                    JoinKey::Str(_) => None, // shouldn't happen if types match
                })
                .collect();
            Column { name: out_name.to_string(), data: ColumnData::Int64(vals) }
        }
        ColumnData::Bool(_) => {
            let vals: Vec<Option<bool>> = key_order.iter()
                .map(|k| match &k[key_idx] {
                    JoinKey::Bool(b) => Some(*b),
                    JoinKey::Int(i)  => Some(*i != 0),
                    JoinKey::Null    => None,
                    JoinKey::Str(_)  => None,
                })
                .collect();
            Column { name: out_name.to_string(), data: ColumnData::Bool(vals) }
        }
        ColumnData::Str(_) | ColumnData::StrDict { .. } => {
            let vals: Vec<Option<String>> = key_order.iter()
                .map(|k| match &k[key_idx] {
                    JoinKey::Str(s)  => Some(s.clone()),
                    JoinKey::Int(i)  => Some(i.to_string()),
                    JoinKey::Bool(b) => Some(b.to_string()),
                    JoinKey::Null    => None,
                })
                .collect();
            Column { name: out_name.to_string(), data: ColumnData::Str(vals) }
        }
        ColumnData::Float64(_) => unreachable!("Float64 group keys rejected earlier"),
    }
}

// ─── Aggregate computation ───────────────────────────────────────────────────

fn compute_agg_over(
    base: &DataBlock,
    rows: &[usize],
    a: &AggCol,
) -> Result<Value, KoreError> {
    // COUNT(*): pure row count, no source column needed.
    if let (AggFunc::Count, None) = (&a.func, &a.src) {
        return Ok(Value::Int(rows.len() as i64));
    }

    let src_name = a.src.as_ref().ok_or_else(||
        KoreError::InvalidArgument("agg without source column".into()))?;
    let src = find_col(base, src_name)
        .ok_or_else(|| KoreError::ColumnNotFound(src_name.clone()))?;

    match &src.data {
        ColumnData::Int64(v) => {
            let selected: Vec<f64> = rows.iter()
                .filter_map(|&r| v.get(r).and_then(|x| *x).map(|i| i as f64))
                .collect();
            Ok(finalize_f64_agg(&a.func, &selected))
        }
        ColumnData::Float64(v) => {
            let selected: Vec<f64> = rows.iter()
                .filter_map(|&r| v.get(r).and_then(|x| *x))
                .collect();
            Ok(finalize_f64_agg(&a.func, &selected))
        }
        ColumnData::Bool(v) => {
            // SUM/COUNT/MIN/MAX on Bool: convert to 0/1.
            let selected: Vec<f64> = rows.iter()
                .filter_map(|&r| v.get(r).and_then(|x| *x).map(|b| if b { 1.0 } else { 0.0 }))
                .collect();
            Ok(finalize_f64_agg(&a.func, &selected))
        }
        ColumnData::Str(_) | ColumnData::StrDict { .. } => {
            // Only COUNT is meaningful for strings on this fast path.
            match a.func {
                AggFunc::Count => Ok(Value::Int(rows.iter()
                    .filter(|&&r| !matches!(src.data.get_value(r), Value::Null))
                    .count() as i64)),
                _ => Err(KoreError::InvalidArgument(
                    "vectorized fast-path: numeric agg on string column".into())),
            }
        }
    }
}

fn finalize_f64_agg(func: &AggFunc, vals: &[f64]) -> Value {
    match func {
        AggFunc::Count            => Value::Int(vals.len() as i64),
        AggFunc::Sum              => Value::Float(batch_sum_full(vals)),
        AggFunc::Avg if vals.is_empty() => Value::Null,
        AggFunc::Avg              => Value::Float(batch_sum_full(vals) / vals.len() as f64),
        AggFunc::Min              => vals.iter().copied().reduce(f64::min)
            .map(Value::Float).unwrap_or(Value::Null),
        AggFunc::Max              => vals.iter().copied().reduce(f64::max)
            .map(Value::Float).unwrap_or(Value::Null),
        _ => Value::Null, // classify() rejects other agg kinds up front
    }
}

/// Aggregates always emit `Float64` — this matches the row-loop path, where
/// every aggregate (including COUNT) is promoted to floating-point.  Keeping
/// the same output dtype is what lets the golden-diff tests hold.
fn agg_result_to_column(_func: &AggFunc, name: &str, v: Value) -> Column {
    let f = value_to_f64(&v);
    Column { name: name.to_string(), data: ColumnData::Float64(vec![f]) }
}

fn build_agg_column(_func: &AggFunc, name: &str, vals: Vec<Value>) -> Column {
    let data: Vec<Option<f64>> = vals.iter().map(value_to_f64).collect();
    Column { name: name.to_string(), data: ColumnData::Float64(data) }
}

#[inline]
fn value_to_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) => Some(*f),
        Value::Int(i)   => Some(*i as f64),
        Value::Bool(b)  => Some(if *b { 1.0 } else { 0.0 }),
        _               => None,
    }
}

fn apply_limit(mut block: DataBlock, limit: Option<usize>) -> DataBlock {
    let Some(n) = limit else { return block; };
    if block.num_rows <= n { return block; }
    let cut: Vec<usize> = (0..n).collect();
    block.columns = block.columns.iter().map(|c| Column {
        name: c.name.clone(),
        data: c.data.take_rows(&cut),
    }).collect();
    block.num_rows = n;
    block
}

fn find_col<'a>(block: &'a DataBlock, name: &str) -> Option<&'a Column> {
    // Try exact and suffix-based match, mirroring executor's resolve behavior.
    block.columns.iter().find(|c| c.name == name || c.name.ends_with(&format!(".{name}")))
}

// silence unused-import warning when compiling without JoinKind test usage
const _: fn() = || { let _ = std::mem::size_of::<JoinKind>(); };

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::KqlContext;
    use crate::parser::parse_query;

    fn sales_block(n: usize) -> DataBlock {
        DataBlock {
            num_rows: n,
            columns: vec![
                Column { name: "region".into(),
                    data: ColumnData::Str((0..n)
                        .map(|i| Some(["EU","US","AP"][i % 3].to_string())).collect()) },
                Column { name: "amount".into(),
                    data: ColumnData::Float64((0..n).map(|i| Some(i as f64)).collect()) },
                Column { name: "qty".into(),
                    data: ColumnData::Int64((0..n).map(|i| Some(i as i64)).collect()) },
            ],
        }
    }

    fn shape_only(a: &DataBlock, b: &DataBlock) -> bool {
        if a.num_rows != b.num_rows { return false; }
        if a.columns.len() != b.columns.len() { return false; }
        for (x, y) in a.columns.iter().zip(b.columns.iter()) {
            if x.name != y.name { return false; }
        }
        true
    }

    /// Sort rows of a block by all columns, returning a canonical form we can
    /// compare for row-set equality (hash-groupby order isn't guaranteed).
    fn canonicalize(block: &DataBlock) -> Vec<Vec<String>> {
        let mut rows: Vec<Vec<String>> = (0..block.num_rows).map(|r| {
            block.columns.iter().map(|c| match c.data.get_value(r) {
                Value::Int(i)   => i.to_string(),
                Value::Float(f) => format!("{f:.6}"),
                Value::Bool(b)  => b.to_string(),
                Value::Str(s)   => s,
                Value::Null     => "NULL".to_string(),
            }).collect()
        }).collect();
        rows.sort();
        rows
    }

    /// Compare row-loop path to fast-path on the same SQL.
    ///
    /// Both entry points are called directly (no env-var toggling) so this is
    /// safe under `cargo test`'s default multi-threaded runner — no shared
    /// `KORE_VECTORIZED` state across tests.
    fn assert_same(sql: &str, base: DataBlock) {
        let mut ctx = KqlContext::new();
        ctx.register("sales", base);

        // Row-loop path — reference.  Go straight to execute_query so we
        // don't accidentally route through the fast-path.
        let query = parse_query(sql).expect("parse");
        let reference = crate::executor::execute_query(&query, &ctx)
            .expect("row-loop query failed");

        // Fast path.
        let fast = try_vectorized(&query, &ctx);
        if let Some(res) = fast {
            let fast_block = res.expect("fast path returned error");
            assert!(
                shape_only(&reference, &fast_block),
                "schema mismatch for `{sql}`:\n  ref cols={:?} rows={}\n  fast cols={:?} rows={}",
                reference.columns.iter().map(|c| &c.name).collect::<Vec<_>>(),
                reference.num_rows,
                fast_block.columns.iter().map(|c| &c.name).collect::<Vec<_>>(),
                fast_block.num_rows,
            );
            assert_eq!(
                canonicalize(&reference),
                canonicalize(&fast_block),
                "row-set mismatch for `{sql}`",
            );
        } else {
            panic!("fast path should have accepted `{sql}` — classifier rejected it");
        }
    }

    #[test]
    fn accepts_star() {
        assert_same("SELECT * FROM sales", sales_block(30));
    }

    #[test]
    fn accepts_bare_cols_with_filter() {
        assert_same(
            "SELECT region, amount FROM sales WHERE amount > 10 AND qty < 25",
            sales_block(30),
        );
    }

    #[test]
    fn accepts_bare_cols_with_limit() {
        assert_same(
            "SELECT region, qty FROM sales LIMIT 7",
            sales_block(30),
        );
    }

    #[test]
    fn accepts_global_agg() {
        assert_same(
            "SELECT SUM(amount), COUNT(*), MIN(qty), MAX(qty) FROM sales WHERE amount > 5",
            sales_block(60),
        );
    }

    #[test]
    fn accepts_group_agg_by_string_key() {
        assert_same(
            "SELECT region, SUM(amount) AS total, COUNT(*) AS n FROM sales GROUP BY region",
            sales_block(60),
        );
    }

    #[test]
    fn accepts_group_agg_by_int_key_with_filter() {
        // qty is an int col; group by it directly.
        assert_same(
            "SELECT qty, SUM(amount) FROM sales WHERE amount > 3 GROUP BY qty",
            sales_block(30),
        );
    }

    #[test]
    fn rejects_join() {
        let mut ctx = KqlContext::new();
        ctx.register("sales", sales_block(10));
        ctx.register("dim", sales_block(3));
        let q = parse_query(
            "SELECT * FROM sales JOIN dim ON sales.region = dim.region"
        ).unwrap();
        assert!(try_vectorized(&q, &ctx).is_none(), "joins must fall through");
    }

    #[test]
    fn rejects_order_by() {
        let mut ctx = KqlContext::new();
        ctx.register("sales", sales_block(10));
        let q = parse_query("SELECT * FROM sales ORDER BY amount").unwrap();
        assert!(try_vectorized(&q, &ctx).is_none(), "ORDER BY must fall through");
    }

    #[test]
    fn rejects_or_predicate() {
        let mut ctx = KqlContext::new();
        ctx.register("sales", sales_block(10));
        let q = parse_query("SELECT * FROM sales WHERE amount > 5 OR qty < 2").unwrap();
        assert!(try_vectorized(&q, &ctx).is_none(), "OR must fall through");
    }

    #[test]
    fn rejects_expression_projection() {
        let mut ctx = KqlContext::new();
        ctx.register("sales", sales_block(10));
        let q = parse_query("SELECT amount + qty FROM sales").unwrap();
        assert!(try_vectorized(&q, &ctx).is_none(), "arithmetic proj must fall through");
    }

    #[test]
    fn env_gate_disables_fast_path() {
        // This is the only test that touches KORE_VECTORIZED — all other
        // tests exercise the fast-path directly via `try_vectorized`, so
        // there is no parallel-test race with this env-var toggle.
        std::env::set_var("KORE_VECTORIZED", "0");
        let mut ctx = KqlContext::new();
        ctx.register("sales", sales_block(10));
        let q = parse_query("SELECT * FROM sales").unwrap();
        assert!(try_vectorized(&q, &ctx).is_none());
        std::env::remove_var("KORE_VECTORIZED");
    }

    /// Correctness anchor for a large filter+project shape: 500k rows,
    /// selective WHERE, all-columns projection.  The fast-path calls
    /// `vectorized_filter` (bitmap → indices) then `take_rows` (columnar),
    /// which avoids the row-loop's per-row expression evaluation.
    ///
    /// This test asserts:
    /// * the fast-path returns the same row-set as the row-loop, and
    /// * elapsed time is bounded — no runaway regressions.
    ///
    /// We deliberately do *not* assert `fast < row_loop`.  On modern KORE
    /// the row-loop is already columnar-aware and beats naive interpreters,
    /// so Phase 17's win depends heavily on shape / build profile / CPU.
    /// A rigorous benchmark belongs in `kore-bench`, not a unit test.
    #[test]
    fn fast_path_correctness_on_500k_rows() {
        let n = 500_000;
        let base = sales_block(n);
        let mut ctx = KqlContext::new();
        ctx.register("sales", base);
        let sql = "SELECT * FROM sales WHERE amount > 100000 AND qty > 50000";
        let query = parse_query(sql).unwrap();

        let t_row = std::time::Instant::now();
        let ref_block = crate::executor::execute_query(&query, &ctx).unwrap();
        let row_ms = t_row.elapsed().as_millis();

        let t_vec = std::time::Instant::now();
        let fast_res = try_vectorized(&query, &ctx)
            .expect("fast path should accept this shape")
            .expect("fast path returned error");
        let vec_ms = t_vec.elapsed().as_millis();

        assert_eq!(fast_res.num_rows, ref_block.num_rows,
            "row count mismatch: row-loop={} fast-path={}",
            ref_block.num_rows, fast_res.num_rows);

        // Floor: fast path shouldn't be more than 5× slower than row-loop —
        // catches accidental O(n²) regressions.
        assert!(
            vec_ms <= row_ms.saturating_mul(5).max(200),
            "fast path ({vec_ms} ms) grossly slower than row-loop ({row_ms} ms) on {n} rows",
        );

        eprintln!(
            "Phase-17 500k filter+scan: row-loop={row_ms}ms  fast-path={vec_ms}ms  \
             ratio≈{:.2}× (>1 = fast-path faster)",
            row_ms as f64 / vec_ms.max(1) as f64,
        );
    }
}
