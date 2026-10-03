//! The general query tail: everything after FROM / JOIN / WHERE for statements the specialised fast paths
//! in `executor.rs` do not cover (expression GROUP BY, GROUPING SETS, FILTER / DISTINCT / statistical
//! aggregates, window functions anywhere in an expression, QUALIFY, subqueries in the select list,
//! ordinal and aggregate ORDER BY, DISTINCT with LIMIT, ...). It is row-oriented and favours being
//! right over being fast; the TPC-H shaped fast paths never come through here.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use kore_core::{Column, ColumnData, DataBlock, KoreError};

use crate::aggs::{self, row_key, total_cmp};
use crate::ast::*;
use crate::ast_walk::{any_node, map_expr, walk_expr};
use crate::executor::{eval_expr, execute_select, ExprVal, KqlContext};
use crate::scalar::{cmp_vals, num, to_str};
use crate::window::{self, cmp_keys};

type V = ExprVal;

fn err(msg: impl Into<String>) -> KoreError { KoreError::InvalidArgument(msg.into()) }

// ─── routing ──────────────────────────────────────────────────────────────────

fn is_subq(e: &Expr) -> bool {
    matches!(e, Expr::ScalarSubquery(_) | Expr::InSubquery { .. } | Expr::Exists { .. } | Expr::QuantSubquery { .. })
}

fn is_agg(e: &Expr) -> bool { matches!(e, Expr::Agg { .. } | Expr::AggX { .. }) }

pub fn has_subq(e: &Expr) -> bool { any_node(e, &is_subq) }

fn is_grouping_fn(e: &Expr) -> bool {
    matches!(e, Expr::FuncCall { name, .. } if name == "GROUPING" || name == "GROUPING_ID")
}

/// Statements (syntactically) outside what the fast paths in the executor handle.
pub fn needs_general(s: &SelectStmt) -> bool {
    if !s.group_exprs.is_empty() || !s.windows.is_empty() || s.qualify.is_some() { return true; }
    if s.grouping != Grouping::Plain && !s.group_by.is_empty() { return true; }
    if s.distinct && (s.limit.is_some() || s.offset.is_some()) { return true; }
    if ungrouped_projection(s) { return true; }
    let special = |e: &Expr| any_node(e, &|x| matches!(x, Expr::AggX { .. } | Expr::Window { .. }) || is_grouping_fn(x) || is_subq(x));
    for p in &s.projections {
        if let Projection::Expr { expr, .. } = p {
            if special(expr) || matches!(expr, Expr::QualCol(_, c) if c == "*") { return true; }
        }
    }
    if let Some(h) = &s.having {
        // a HAVING with aggregates *and* subqueries is handled by lift_aggregates (outer WHERE with a context)
        let only_agg_x = any_node(h, &|x| matches!(x, Expr::AggX { .. } | Expr::Window { .. }) || is_grouping_fn(x));
        let subq_without_agg = any_node(h, &is_subq) && !any_node(h, &is_agg);
        if only_agg_x || subq_without_agg { return true; }
    }
    for o in &s.order_by {
        if matches!(o.expr, Expr::Int(_)) { return true; }
        if special(&o.expr) || any_node(&o.expr, &is_agg) { return true; }
        // an expression over a select alias (ORDER BY -x) can only be evaluated against the output
        if !matches!(o.expr, Expr::Col(_) | Expr::QualCol(..)) {
            let aliases: Vec<&String> = s.projections.iter().filter_map(|p| match p { Projection::Expr { alias: Some(a), .. } => Some(a), _ => None }).collect();
            if any_node(&o.expr, &|x| matches!(x, Expr::Col(c) if aliases.iter().any(|a| a.eq_ignore_ascii_case(c)))) { return true; }
        }
    }
    false
}

/// Bare names of the columns an expression reads (aggregate arguments and subqueries excluded).
fn plain_columns(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Col(c) | Expr::QualCol(_, c) => out.push(c.clone()),
        Expr::Agg { .. } | Expr::AggX { .. } | Expr::Window { .. } | Expr::ScalarSubquery(_) | Expr::InSubquery { .. }
        | Expr::Exists { .. } | Expr::QuantSubquery { .. } => {}
        other => {
            // children only
            crate::ast_walk::map_expr(other, &mut |x| {
                if std::ptr::eq(x, other) { return None; }
                plain_columns(x, out);
                Some(x.clone())
            });
        }
    }
}

/// A select item that is neither aggregated nor grouped (`SELECT id, COUNT(*) FROM t`, `SELECT g, v ... GROUP BY g`):
/// Spark rejects it; the fast paths would silently drop or invent a value. Items that merely alias a GROUP BY name
/// are the fast path's own convention and stay where they are.
fn ungrouped_projection(s: &SelectStmt) -> bool {
    let has_agg = s.projections.iter().any(|p| matches!(p, Projection::Expr { expr, .. } if any_node(expr, &is_agg)));
    if !has_agg && s.group_by.is_empty() { return false; }
    let grouped = |c: &str| s.group_by.iter().any(|g| bare_name(g).eq_ignore_ascii_case(c));
    for p in &s.projections {
        let Projection::Expr { expr, alias } = p else { continue };
        if any_node(expr, &is_agg) { continue; }
        if alias.as_ref().map_or(false, |a| s.group_by.iter().any(|g| g.eq_ignore_ascii_case(a))) { continue; }
        let mut cols = Vec::new();
        plain_columns(expr, &mut cols);
        if cols.iter().all(|c| grouped(c)) { continue; }
        return true;
    }
    // HAVING may only use grouped columns, aggregates and select aliases
    if let Some(h) = &s.having {
        let aliases: Vec<&String> = s.projections.iter().filter_map(|p| match p { Projection::Expr { alias: Some(a), .. } => Some(a), _ => None }).collect();
        let mut cols = Vec::new();
        plain_columns(h, &mut cols);
        if cols.iter().any(|c| !grouped(c) && !aliases.iter().any(|a| a.eq_ignore_ascii_case(c))) { return true; }
    }
    false
}

/// Needs the general path because of the column types the classic numeric aggregators cannot handle
/// (MIN / MAX / DISTINCT / SUM over text or boolean columns).
pub fn block_needs_general(s: &SelectStmt, block: &DataBlock) -> bool {
    // GROUP BY name that is the alias of a computed select item *and* an input column: SQL groups by the
    // input column, the fast path would group by the alias' expression
    if !s.group_by.is_empty() {
        for p in &s.projections {
            if let Projection::Expr { expr, alias: Some(a) } = p {
                if !matches!(expr, Expr::Col(_) | Expr::QualCol(..)) && !is_agg(expr)
                    && s.group_by.iter().any(|g| bare_name(g).eq_ignore_ascii_case(a))
                    && block.columns.iter().any(|c| bare_name(&c.name).eq_ignore_ascii_case(a)) {
                    return true;
                }
            }
        }
    }
    let textual = |e: &Expr| -> bool {
        match e {
            Expr::Col(_) | Expr::QualCol(..) => {
                let name = match e { Expr::QualCol(t, c) => format!("{t}.{c}"), Expr::Col(c) => c.clone(), _ => unreachable!() };
                block.columns.iter().find(|c| c.name == name || c.name.ends_with(&format!(".{name}")))
                    .map_or(false, |c| matches!(c.data, ColumnData::Str(_) | ColumnData::StrDict { .. } | ColumnData::Bool(_)))
            }
            Expr::Int(_) | Expr::Float(_) | Expr::Star => false,
            other => (0..block.num_rows.min(32)).any(|r| matches!(eval_expr(other, block, r), V::Str(_) | V::Bool(_))),
        }
    };
    let mut hit = false;
    let mut check = |e: &Expr| {
        if let Expr::Agg { func, expr } = e {
            if matches!(func, AggFunc::Count) { return; }
            if textual(expr) { hit = true; }
        }
    };
    for p in &s.projections { if let Projection::Expr { expr, .. } = p { walk_expr(expr, false, &mut check); } }
    if let Some(h) = &s.having { walk_expr(h, false, &mut check); }
    hit
}

// ─── values <-> columns ───────────────────────────────────────────────────────

pub fn column_values(c: &Column) -> Vec<V> {
    match &c.data {
        ColumnData::Int64(v) => v.iter().map(|x| x.map(V::Int).unwrap_or(V::Null)).collect(),
        ColumnData::Float64(v) => v.iter().map(|x| x.map(V::Float).unwrap_or(V::Null)).collect(),
        ColumnData::Bool(v) => v.iter().map(|x| x.map(V::Bool).unwrap_or(V::Null)).collect(),
        ColumnData::Str(v) => v.iter().map(|x| x.as_ref().map(|s| V::Str(s.clone())).unwrap_or(V::Null)).collect(),
        ColumnData::StrDict { codes, dict } => codes.iter().map(|&c| if c == u8::MAX { V::Null } else { dict.get(c as usize).map(|s| V::Str(s.clone())).unwrap_or(V::Null) }).collect(),
    }
}

/// A typed column from values: all-int -> Int64, any float -> Float64, any text -> Str, all-bool -> Bool.
/// An all-NULL column keeps the type of `hint` when there is one.
pub fn vals_to_column(name: String, vals: Vec<V>, hint: Option<&ColumnData>) -> Column {
    let (mut int, mut flt, mut s, mut b) = (false, false, false, false);
    for v in &vals {
        match v { V::Int(_) => int = true, V::Float(_) => flt = true, V::Str(_) => s = true, V::Bool(_) => b = true, V::Null => {} }
    }
    let data = if s {
        ColumnData::Str(vals.iter().map(to_str).collect())
    } else if flt {
        ColumnData::Float64(vals.iter().map(|v| num(v).filter(|_| !matches!(v, V::Null))).collect())
    } else if int {
        ColumnData::Int64(vals.iter().map(|v| match v { V::Int(i) => Some(*i), V::Bool(x) => Some(*x as i64), _ => None }).collect())
    } else if b {
        ColumnData::Bool(vals.iter().map(|v| match v { V::Bool(x) => Some(*x), _ => None }).collect())
    } else {
        // only NULLs
        match hint {
            Some(ColumnData::Int64(_)) => ColumnData::Int64(vec![None; vals.len()]),
            Some(ColumnData::Float64(_)) => ColumnData::Float64(vec![None; vals.len()]),
            Some(ColumnData::Bool(_)) => ColumnData::Bool(vec![None; vals.len()]),
            _ => ColumnData::Str(vec![None; vals.len()]),
        }
    };
    Column { name, data }
}

fn lit(v: &V) -> Expr {
    match v {
        V::Int(i) => Expr::Int(*i),
        V::Float(f) => Expr::Float(*f),
        V::Str(s) => Expr::Str(s.clone()),
        V::Bool(b) => Expr::Bool(*b),
        V::Null => Expr::Null,
    }
}

fn find_col_idx(block: &DataBlock, name: &str) -> Option<usize> {
    block.columns.iter().position(|c| c.name == name).or_else(|| {
        let suffix = format!(".{name}");
        block.columns.iter().position(|c| c.name.ends_with(&suffix))
    }).or_else(|| {
        // identifiers are case-insensitive
        let lower = name.to_ascii_lowercase();
        block.columns.iter().position(|c| {
            let cl = c.name.to_ascii_lowercase();
            cl == lower || cl.ends_with(&format!(".{lower}"))
        })
    })
}

/// Result column name without a table qualifier (`t.id` -> `id`); computed names such as `Sum(t.v)` are kept.
pub fn bare_name(name: &str) -> String {
    if name.contains('(') || name.contains(' ') { return name.to_string(); }
    name.rsplit('.').next().unwrap_or(name).to_string()
}

/// Drop table qualifiers from column names: a derived table / CTE exposes bare column names.
pub fn strip_qualifiers(mut b: DataBlock) -> DataBlock {
    for c in &mut b.columns { c.name = bare_name(&c.name); }
    b
}

// ─── subqueries inside expressions ────────────────────────────────────────────

#[derive(Clone)]
enum SubResult {
    Scalar(V),
    Set(Vec<V>),
    Exists(bool),
}

struct Evaluator<'a> {
    ctx: &'a KqlContext,
    cache: RefCell<HashMap<String, SubResult>>,
    error: RefCell<Option<KoreError>>,
}

impl<'a> Evaluator<'a> {
    fn new(ctx: &'a KqlContext) -> Self { Evaluator { ctx, cache: RefCell::new(HashMap::new()), error: RefCell::new(None) } }

    fn fail(&self, e: KoreError) { let mut slot = self.error.borrow_mut(); if slot.is_none() { *slot = Some(e); } }

    fn check(&self) -> Result<(), KoreError> {
        match self.error.borrow_mut().take() { Some(e) => Err(e), None => Ok(()) }
    }

    /// Evaluate `e` for `row`, running any subqueries it contains.
    fn eval(&self, e: &Expr, block: &DataBlock, row: usize) -> V {
        if has_subq(e) {
            let bound = self.materialize(e, block, row);
            eval_expr(&bound, block, row)
        } else {
            eval_expr(e, block, row)
        }
    }

    fn run_sub(&self, sub: &SelectStmt, block: &DataBlock, row: usize) -> Option<SubResult> {
        let bound = bind_outer(sub, block, row, self.ctx);
        let key = format!("{:?}", bound);
        let want = key.clone();
        if let Some(hit) = self.cache.borrow().get(&want) { return Some(hit.clone()); }
        let res = match execute_select(&bound, self.ctx) {
            Ok(r) => r,
            Err(e) => { self.fail(e); return None; }
        };
        let vals: Vec<V> = res.columns.first().map(column_values).unwrap_or_default();
        let out = SubResult::Set(vals);
        self.cache.borrow_mut().insert(key, out.clone());
        Some(out)
    }

    /// Replace subquery nodes of `e` by their values for this row.
    fn materialize(&self, e: &Expr, block: &DataBlock, row: usize) -> Expr {
        map_expr(e, &mut |x| match x {
            Expr::ScalarSubquery(sub) => {
                let Some(SubResult::Set(vals)) = self.run_sub(sub, block, row) else { return Some(Expr::Null) };
                if vals.len() > 1 {
                    self.fail(err("scalar subquery returned more than one row"));
                    return Some(Expr::Null);
                }
                Some(vals.first().map(lit).unwrap_or(Expr::Null))
            }
            Expr::Exists { subquery, negated } => {
                let rows = match self.run_sub_rows(subquery, block, row) { Some(n) => n, None => return Some(Expr::Null) };
                Some(Expr::Bool((rows > 0) != *negated))
            }
            Expr::InSubquery { expr, subquery, negated } => {
                let lhs = self.eval(expr, block, row);
                let Some(SubResult::Set(vals)) = self.run_sub(subquery, block, row) else { return Some(Expr::Null) };
                Some(match quantified(&lhs, &vals, &BinOpKind::Eq, false) {
                    Some(b) => Expr::Bool(b != *negated),
                    None => Expr::Null,
                })
            }
            Expr::QuantSubquery { expr, op, all, subquery } => {
                let lhs = self.eval(expr, block, row);
                let Some(SubResult::Set(vals)) = self.run_sub(subquery, block, row) else { return Some(Expr::Null) };
                Some(match quantified(&lhs, &vals, op, *all) { Some(b) => Expr::Bool(b), None => Expr::Null })
            }
            _ => None,
        })
    }

    /// Number of rows a subquery returns (EXISTS only needs that).
    fn run_sub_rows(&self, sub: &SelectStmt, block: &DataBlock, row: usize) -> Option<usize> {
        match self.run_sub(sub, block, row)? {
            SubResult::Set(v) => {
                // a subquery with no columns would still have rows, but the cache holds the first column only
                Some(v.len())
            }
            SubResult::Exists(b) => Some(b as usize),
            SubResult::Scalar(_) => Some(1),
        }
    }
}

/// `x <op> ANY|ALL (set)` and `x IN (set)` with SQL three-valued logic.
fn quantified(lhs: &V, set: &[V], op: &BinOpKind, all: bool) -> Option<bool> {
    if set.is_empty() { return Some(all); }
    if matches!(lhs, V::Null) { return None; }
    let mut unknown = false;
    for v in set {
        let r = match cmp_vals(lhs, v) {
            None => None,
            Some(o) => Some(match op {
                BinOpKind::Eq => o.is_eq(), BinOpKind::Ne => o.is_ne(), BinOpKind::Lt => o.is_lt(),
                BinOpKind::Le => o.is_le(), BinOpKind::Gt => o.is_gt(), _ => o.is_ge(),
            }),
        };
        match (r, all) {
            (Some(true), false) => return Some(true),
            (Some(false), true) => return Some(false),
            (None, _) => unknown = true,
            _ => {}
        }
    }
    if unknown { None } else { Some(all) }
}

/// Substitute references to the outer row inside a correlated subquery by literals.
fn bind_outer(sub: &SelectStmt, block: &DataBlock, row: usize, ctx: &KqlContext) -> SelectStmt {
    let mut inner_aliases: HashSet<String> = HashSet::new();
    let mut inner_cols: HashSet<String> = HashSet::new();
    let mut unknown_inner = false;
    let mut scope = |t: &TableExpr| {
        inner_aliases.insert(t.alias.clone().unwrap_or_else(|| t.name.clone()));
        inner_aliases.insert(t.name.clone());
        if let Some(b) = ctx.get(&t.name) {
            for c in &b.columns { inner_cols.insert(bare_name(&c.name)); }
        } else if let Some(sq) = &t.subquery {
            if sq.projections.iter().any(|p| matches!(p, Projection::Star)) { unknown_inner = true; }
            for p in &sq.projections {
                if let Projection::Expr { expr, alias } = p {
                    match (alias, expr) {
                        (Some(a), _) => { inner_cols.insert(a.clone()); }
                        (None, Expr::Col(c)) | (None, Expr::QualCol(_, c)) => { inner_cols.insert(c.clone()); }
                        _ => {}
                    }
                }
            }
        } else if t.values.is_none() {
            unknown_inner = true; // a view or file: schema not known here
        }
        for c in &t.col_aliases { inner_cols.insert(c.clone()); }
    };
    scope(&sub.from);
    for j in &sub.joins { scope(&j.table); }

    let lookup = |name: &str| -> Option<V> {
        let i = find_col_idx(block, name)?;
        column_values_at(&block.columns[i], row)
    };
    let mut f = |x: &Expr| -> Option<Expr> {
        match x {
            Expr::QualCol(q, c) if !inner_aliases.contains(q) => lookup(&format!("{q}.{c}")).map(|v| lit(&v)),
            Expr::Col(c) if !unknown_inner && !inner_cols.contains(c) => lookup(c).map(|v| lit(&v)),
            _ => None,
        }
    };
    let mut bound = crate::ast_walk::map_stmt_exprs(sub, &mut f);
    // ON expressions and the FROM subquery bodies of the subquery are left as written
    bound.joins = sub.joins.iter().zip(bound.joins.iter()).map(|(orig, b)| {
        let mut j = b.clone();
        j.on.left_col = orig.on.left_col.clone();
        j.on.right_col = orig.on.right_col.clone();
        j
    }).collect();
    bound
}

fn column_values_at(c: &Column, row: usize) -> Option<V> {
    Some(match &c.data {
        ColumnData::Int64(v) => v.get(row)?.map(V::Int).unwrap_or(V::Null),
        ColumnData::Float64(v) => v.get(row)?.map(V::Float).unwrap_or(V::Null),
        ColumnData::Bool(v) => v.get(row)?.map(V::Bool).unwrap_or(V::Null),
        ColumnData::Str(v) => v.get(row)?.as_ref().map(|s| V::Str(s.clone())).unwrap_or(V::Null),
        ColumnData::StrDict { codes, dict } => {
            let c = *codes.get(row)?;
            if c == u8::MAX { V::Null } else { V::Str(dict.get(c as usize)?.clone()) }
        }
    })
}

// ─── name resolution ──────────────────────────────────────────────────────────

/// Rewrite column references to the exact column names of `block` so structural equality means semantic equality.
fn resolve_cols(e: &Expr, block: &DataBlock) -> Expr {
    map_expr(e, &mut |x| match x {
        Expr::Col(c) => find_col_idx(block, c).map(|i| Expr::Col(block.columns[i].name.clone())),
        Expr::QualCol(t, c) => find_col_idx(block, &format!("{t}.{c}")).map(|i| Expr::Col(block.columns[i].name.clone())),
        // subquery bodies resolve against their own scope
        _ => None,
    })
}

struct Item {
    expr: Expr,
    name: String,
    /// plain column of the input (type preserved when gathered)
    src_col: Option<usize>,
}

fn default_name(expr: &Expr, block: &DataBlock) -> String {
    match expr {
        Expr::Col(c) | Expr::QualCol(_, c) => {
            let full = match expr { Expr::QualCol(t, c) => format!("{t}.{c}"), _ => c.clone() };
            // like the fast path, a plain column keeps the name it has in the input (`t.id`)
            find_col_idx(block, &full).map(|i| block.columns[i].name.clone()).unwrap_or_else(|| c.clone())
        }
        Expr::Agg { func, expr: inner } => {
            let col = match inner.as_ref() { Expr::Col(c) => c.clone(), Expr::QualCol(_, c) => c.clone(), _ => String::new() };
            format!("{:?}({})", func, col)
        }
        Expr::AggX { name, args, .. } => {
            let col = args.first().map(|a| match a { Expr::Col(c) => c.clone(), Expr::QualCol(_, c) => c.clone(), _ => String::new() }).unwrap_or_default();
            format!("{}({})", name.to_ascii_lowercase(), col)
        }
        _ => "expr".to_string(),
    }
}

// ─── the pipeline ─────────────────────────────────────────────────────────────

struct GroupSpec {
    exprs: Vec<Expr>,
    sets: Vec<Vec<usize>>,
}

fn subsets(n: usize) -> Vec<Vec<usize>> {
    (0..(1usize << n)).rev().map(|mask| (0..n).filter(|i| mask & (1 << (n - 1 - i)) != 0).collect()).collect()
}

/// Run projections / grouping / windows / ordering over `input` (the rows left after FROM, JOIN and WHERE).
pub fn run(stmt: &SelectStmt, input: DataBlock, ctx: &KqlContext) -> Result<DataBlock, KoreError> {
    let n_in = input.num_rows;
    let evaluator = Evaluator::new(ctx);

    // ── select items ──
    let mut items: Vec<Item> = Vec::new();
    let mut proj_alias: Vec<(String, usize)> = Vec::new(); // alias -> item index
    for p in &stmt.projections {
        match p {
            Projection::Star => {
                for (i, c) in input.columns.iter().enumerate() {
                    items.push(Item { expr: Expr::Col(c.name.clone()), name: c.name.clone(), src_col: Some(i) });
                }
            }
            Projection::Expr { expr: Expr::QualCol(t, c), .. } if c == "*" => {
                let prefix = format!("{t}.");
                let mut any = false;
                for (i, col) in input.columns.iter().enumerate() {
                    if col.name.starts_with(&prefix) {
                        any = true;
                        items.push(Item { expr: Expr::Col(col.name.clone()), name: col.name.clone(), src_col: Some(i) });
                    }
                }
                if !any { return Err(err(format!("cannot expand '{t}.*': no such table or alias"))); }
            }
            Projection::Expr { expr, alias } => {
                let e = resolve_cols(expr, &input);
                let name = alias.clone().unwrap_or_else(|| default_name(&e, &input));
                let src_col = match &e { Expr::Col(c) if alias.is_none() || true => input.columns.iter().position(|x| &x.name == c), _ => None };
                if let Some(a) = alias { proj_alias.push((a.clone(), items.len())); }
                items.push(Item { expr: e, name, src_col });
            }
        }
    }

    // alias substitution for HAVING / ORDER BY / GROUP BY: names that are not input columns but select aliases
    let alias_expr = |e: &Expr| -> Expr {
        map_expr(e, &mut |x| match x {
            Expr::Col(c) if find_col_idx(&input, c).is_none() => {
                proj_alias.iter().find(|(a, _)| a.eq_ignore_ascii_case(c)).map(|(_, i)| items[*i].expr.clone())
            }
            _ => None,
        })
    };

    let has_agg_in = |e: &Expr| any_node(e, &is_agg);
    let having = stmt.having.as_ref().map(|h| alias_expr(&resolve_cols(h, &input)));
    let order_exprs: Vec<Expr> = stmt.order_by.iter().map(|o| resolve_cols(&o.expr, &input)).collect();
    let qualify = stmt.qualify.as_ref().map(|q| alias_expr(&resolve_cols(q, &input)));

    let grouped = !stmt.group_by.is_empty() || !stmt.group_exprs.is_empty()
        || items.iter().any(|i| has_agg_in(&i.expr))
        || having.as_ref().map_or(false, |h| has_agg_in(h))
        || order_exprs.iter().any(|e| has_agg_in(e))
        || qualify.as_ref().map_or(false, |q| has_agg_in(q));

    // ── window definitions resolved ──
    let resolve_windows = |e: &Expr| -> Result<Expr, KoreError> {
        let mut failure: Option<KoreError> = None;
        let out = map_expr(e, &mut |x| match x {
            Expr::Window { func, spec } => match window::resolve_spec(spec, &stmt.windows) {
                Ok(s) => Some(Expr::Window { func: func.clone(), spec: s }),
                Err(er) => { failure = Some(er); Some(x.clone()) }
            },
            _ => None,
        });
        match failure { Some(er) => Err(er), None => Ok(out) }
    };

    let mut items_exprs: Vec<Expr> = Vec::new();
    for it in &items { items_exprs.push(resolve_windows(&it.expr)?); }
    let having = having.map(|h| resolve_windows(&h)).transpose()?;
    let qualify = qualify.map(|q| resolve_windows(&q)).transpose()?;
    let order_exprs: Vec<Expr> = order_exprs.iter().map(|e| resolve_windows(e)).collect::<Result<_, _>>()?;

    // ── grouping ──
    let mut g_block: DataBlock;
    let mut rewrite: Box<dyn Fn(&Expr) -> Result<Expr, KoreError>>;
    if grouped {
        // grouping expressions and sets
        let spec: GroupSpec = if !stmt.group_exprs.is_empty() {
            GroupSpec {
                exprs: stmt.group_exprs.iter().map(|e| alias_expr(&resolve_cols(e, &input))).collect(),
                sets: if stmt.group_sets.is_empty() { vec![(0..stmt.group_exprs.len()).collect()] } else { stmt.group_sets.clone() },
            }
        } else if !stmt.group_by.is_empty() {
            let exprs: Vec<Expr> = stmt.group_by.iter().map(|g| {
                let e = match g.split_once('.') { Some((t, c)) => Expr::QualCol(t.into(), c.into()), None => Expr::Col(g.clone()) };
                alias_expr(&resolve_cols(&e, &input))
            }).collect();
            let k = exprs.len();
            let sets = match stmt.grouping {
                Grouping::Plain => vec![(0..k).collect()],
                Grouping::Rollup => (0..=k).rev().map(|m| (0..m).collect()).collect(),
                Grouping::Cube => subsets(k),
            };
            GroupSpec { exprs, sets }
        } else {
            GroupSpec { exprs: Vec::new(), sets: vec![Vec::new()] }
        };
        for e in &spec.exprs {
            if has_agg_in(e) { return Err(err("aggregate functions are not allowed in GROUP BY")); }
        }

        // aggregate calls, deduplicated
        let mut aggs_list: Vec<Expr> = Vec::new();
        {
            let mut collect = |e: &Expr| {
                walk_expr(e, false, &mut |x| {
                    if is_agg(x) && !aggs_list.contains(x) { aggs_list.push(x.clone()); }
                });
            };
            for e in &items_exprs { collect(e); }
            if let Some(h) = &having { collect(h); }
            if let Some(q) = &qualify { collect(q); }
            for e in &order_exprs { collect(e); }
        }
        // nested aggregates are an error
        for a in &aggs_list {
            let nested = match a {
                Expr::Agg { expr, .. } => has_agg_in(expr),
                Expr::AggX { args, filter, .. } => args.iter().any(|x| has_agg_in(x)) || filter.as_ref().map_or(false, |f| has_agg_in(f)),
                _ => false,
            };
            if nested { return Err(err("aggregate function calls cannot be nested")); }
        }

        // key columns and aggregate inputs, evaluated once over the input
        let key_vals: Vec<Vec<V>> = spec.exprs.iter().map(|e| (0..n_in).map(|r| evaluator.eval(e, &input, r)).collect()).collect();
        let mut agg_inputs = Vec::with_capacity(aggs_list.len());
        for a in &aggs_list {
            let mut ev = |e: &Expr, r: usize| evaluator.eval(e, &input, r);
            agg_inputs.push(aggs::prepare(a, n_in, &mut ev).ok_or_else(|| err("bad aggregate"))?);
        }
        evaluator.check()?;

        let nk = spec.exprs.len();
        let mut out_keys: Vec<Vec<V>> = vec![Vec::new(); nk];
        let mut out_flags: Vec<Vec<V>> = vec![Vec::new(); nk];
        let mut out_aggs: Vec<Vec<V>> = vec![Vec::new(); aggs_list.len()];
        for set in &spec.sets {
            // rows grouped by the key values of this set, in order of first appearance
            let mut groups: Vec<Vec<usize>> = Vec::new();
            if set.is_empty() {
                groups.push((0..n_in).collect());
            } else {
                let mut index: HashMap<String, usize> = HashMap::new();
                for r in 0..n_in {
                    let mut key = String::new();
                    for &k in set { aggs::key_of(&key_vals[k][r], &mut key); }
                    let gi = *index.entry(key).or_insert_with(|| { groups.push(Vec::new()); groups.len() - 1 });
                    groups[gi].push(r);
                }
            }
            for rows in &groups {
                for k in 0..nk {
                    if set.contains(&k) {
                        out_keys[k].push(key_vals[k][rows[0]].clone());
                        out_flags[k].push(V::Int(0));
                    } else {
                        out_keys[k].push(V::Null);
                        out_flags[k].push(V::Int(1));
                    }
                }
                for (j, inp) in agg_inputs.iter().enumerate() { out_aggs[j].push(aggs::aggregate(inp, rows)?); }
            }
        }
        let mut cols: Vec<Column> = Vec::new();
        for k in 0..nk {
            let hint = column_hint(&spec.exprs[k], &input);
            cols.push(vals_to_column(format!("__k{k}"), std::mem::take(&mut out_keys[k]), hint));
            cols.push(vals_to_column(format!("__gf{k}"), std::mem::take(&mut out_flags[k]), None));
        }
        for (j, v) in out_aggs.into_iter().enumerate() { cols.push(vals_to_column(format!("__a{j}"), v, None)); }
        let num_rows = cols.first().map(|c| c.data.len()).unwrap_or_else(|| if spec.sets.is_empty() { 0 } else { 1 });
        g_block = DataBlock { columns: cols, num_rows };

        // expression rewriter: group expressions and aggregates become references to G's columns
        let gexprs = spec.exprs.clone();
        let alist = aggs_list.clone();
        rewrite = Box::new(move |e: &Expr| -> Result<Expr, KoreError> {
            let mut failure: Option<String> = None;
            let out = map_expr(e, &mut |x| {
                if let Some(i) = gexprs.iter().position(|g| g == x) { return Some(Expr::Col(format!("__k{i}"))); }
                if is_agg(x) {
                    return alist.iter().position(|a| a == x).map(|j| Expr::Col(format!("__a{j}")));
                }
                if let Expr::FuncCall { name, args } = x {
                    if name == "GROUPING" && args.len() == 1 {
                        return match gexprs.iter().position(|g| g == &args[0]) {
                            Some(i) => Some(Expr::Col(format!("__gf{i}"))),
                            None => { failure = Some("GROUPING() argument is not a grouping expression".into()); Some(Expr::Null) }
                        };
                    }
                    if name == "GROUPING_ID" {
                        let mut acc: Option<Expr> = None;
                        let k = args.len();
                        for (pos, a) in args.iter().enumerate() {
                            match gexprs.iter().position(|g| g == a) {
                                Some(i) => {
                                    let term = Expr::BinOp { op: BinOpKind::Mul, left: Box::new(Expr::Col(format!("__gf{i}"))), right: Box::new(Expr::Int(1i64 << (k - 1 - pos))) };
                                    acc = Some(match acc { None => term, Some(p) => Expr::BinOp { op: BinOpKind::Add, left: Box::new(p), right: Box::new(term) } });
                                }
                                None => failure = Some("GROUPING_ID() argument is not a grouping expression".into()),
                            }
                        }
                        return Some(acc.unwrap_or(Expr::Int(0)));
                    }
                }
                // window specs are rewritten by map_expr's descent; a bare input column left over is an error
                None
            });
            if let Some(m) = failure { return Err(err(m)); }
            // any input column that survived the rewrite is neither grouped nor aggregated
            let mut bad: Option<String> = None;
            walk_expr(&out, false, &mut |x| {
                if let Expr::Col(c) = x {
                    if !c.starts_with("__") && c != "*" { bad = Some(c.clone()); }
                }
                if let Expr::QualCol(t, c) = x { bad = Some(format!("{t}.{c}")); }
            });
            if let Some(c) = bad {
                return Err(err(format!("expression '{c}' is neither present in the GROUP BY nor an aggregate function")));
            }
            Ok(out)
        });
    } else {
        g_block = input.clone();
        rewrite = Box::new(|e: &Expr| Ok(e.clone()));
    }
    let n = g_block.num_rows;

    // ── rewritten expressions ──
    let mut items_g: Vec<Expr> = items_exprs.iter().map(|e| rewrite(e)).collect::<Result<_, _>>()?;
    let having_g = having.as_ref().map(|h| rewrite(h)).transpose()?;
    let mut qualify_g = qualify.as_ref().map(|q| rewrite(q)).transpose()?;
    let order_pre: Vec<Option<Expr>> = {
        // ordinals and output-column references are resolved against the output later
        let mut v = Vec::new();
        for (i, e) in order_exprs.iter().enumerate() {
            let direct = matches!(stmt.order_by[i].expr, Expr::Int(_))
                || matches!(&stmt.order_by[i].expr, Expr::Col(c) | Expr::QualCol(_, c) if items.iter().any(|it| bare_name(&it.name).eq_ignore_ascii_case(c)))
                || matches!(&stmt.order_by[i].expr, Expr::Col(c) if proj_alias.iter().any(|(a, _)| a.eq_ignore_ascii_case(c)));
            v.push(if direct { None } else { Some(alias_expr(e)) });
        }
        v
    };
    let mut order_g: Vec<Option<Expr>> = Vec::new();
    for o in &order_pre {
        order_g.push(match o { Some(e) => Some(rewrite(&resolve_windows(e)?)?), None => None });
    }

    // ── HAVING ──
    if let Some(h) = &having_g {
        let keep: Vec<usize> = (0..n).filter(|&r| matches!(evaluator.eval(h, &g_block, r), V::Bool(true))).collect();
        evaluator.check()?;
        g_block = g_block.select_rows(&keep);
    }

    // ── windows ──
    let mut windows: Vec<Expr> = Vec::new();
    {
        let mut collect = |e: &Expr| {
            walk_expr(e, false, &mut |x| { if matches!(x, Expr::Window { .. }) && !windows.contains(x) { windows.push(x.clone()); } });
        };
        for e in &items_g { collect(e); }
        if let Some(q) = &qualify_g { collect(q); }
        for e in order_g.iter().flatten() { collect(e); }
    }
    if !windows.is_empty() {
        let nrows = g_block.num_rows;
        let mut new_cols: Vec<Column> = Vec::new();
        for (k, w) in windows.iter().enumerate() {
            let Expr::Window { func, spec } = w else { unreachable!() };
            let mut ev = |e: &Expr, r: usize| evaluator.eval(e, &g_block, r);
            let mut vals = window::evaluate(func, spec, nrows, &mut ev)?;
            // ranking columns have always been exposed as DOUBLE by this engine; callers (and tests) depend on it
            if matches!(func, WindowFn::RowNumber | WindowFn::Rank | WindowFn::DenseRank | WindowFn::Ntile(_)) {
                for v in vals.iter_mut() { if let V::Int(i) = v { *v = V::Float(*i as f64); } }
            }
            new_cols.push(vals_to_column(format!("__w{k}"), vals, None));
        }
        evaluator.check()?;
        g_block.columns.extend(new_cols);
        let wlist = windows.clone();
        let replace = |e: &Expr| -> Expr {
            map_expr(e, &mut |x| if matches!(x, Expr::Window { .. }) { wlist.iter().position(|w| w == x).map(|k| Expr::Col(format!("__w{k}"))) } else { None })
        };
        items_g = items_g.iter().map(&replace).collect();
        qualify_g = qualify_g.map(|q| replace(&q));
        order_g = order_g.into_iter().map(|o| o.map(|e| replace(&e))).collect();
    }

    // ── QUALIFY ──
    if let Some(q) = &qualify_g {
        let keep: Vec<usize> = (0..g_block.num_rows).filter(|&r| matches!(evaluator.eval(q, &g_block, r), V::Bool(true))).collect();
        evaluator.check()?;
        g_block = g_block.select_rows(&keep);
    }
    let n = g_block.num_rows;

    // ── projection ──
    let mut out_cols: Vec<Column> = Vec::with_capacity(items.len());
    for (it, e) in items.iter().zip(items_g.iter()) {
        // a plain input column is copied as is so its type survives (also when every value is NULL)
        if let (Some(ci), false) = (it.src_col, grouped) {
            if let Expr::Col(c) = e {
                if let Some(gi) = g_block.columns.iter().position(|x| &x.name == c) {
                    let mut col = g_block.columns[gi].clone();
                    let _ = ci;
                    col.name = it.name.clone();
                    out_cols.push(col);
                    continue;
                }
            }
        }
        let vals: Vec<V> = (0..n).map(|r| evaluator.eval(e, &g_block, r)).collect();
        let hint = column_hint_g(e, &g_block);
        out_cols.push(vals_to_column(it.name.clone(), vals, hint));
    }
    evaluator.check()?;

    // ── ORDER BY keys ──
    let mut keys: Vec<Vec<V>> = Vec::new();
    for (i, o) in stmt.order_by.iter().enumerate() {
        let col_vals = |idx: usize| column_values(&out_cols[idx]);
        let k: Vec<V> = match (&o.expr, &order_g[i]) {
            (Expr::Int(p), _) => {
                if *p < 1 || *p as usize > out_cols.len() { return Err(err(format!("ORDER BY position {p} is not in the select list"))); }
                col_vals(*p as usize - 1)
            }
            (Expr::Col(c) | Expr::QualCol(_, c), None) => {
                // output alias or output column name
                let by_alias = proj_alias.iter().find(|(a, _)| a.eq_ignore_ascii_case(c)).map(|(_, idx)| *idx);
                let by_name = items.iter().position(|it| bare_name(&it.name).eq_ignore_ascii_case(c));
                col_vals(by_alias.or(by_name).unwrap_or(0))
            }
            (_, Some(e)) => (0..n).map(|r| evaluator.eval(e, &g_block, r)).collect(),
            _ => vec![V::Null; n],
        };
        keys.push(k);
    }
    evaluator.check()?;

    // ── sort, DISTINCT, OFFSET / LIMIT ──
    let mut order: Vec<usize> = (0..n).collect();
    if !stmt.order_by.is_empty() {
        order.sort_by(|&a, &b| {
            for (k, item) in stmt.order_by.iter().enumerate() {
                let o = cmp_keys(&keys[k][a], &keys[k][b], item);
                if o != std::cmp::Ordering::Equal { return o; }
            }
            std::cmp::Ordering::Equal
        });
    }
    if stmt.distinct {
        let out_vals: Vec<Vec<V>> = out_cols.iter().map(column_values).collect();
        let mut seen = HashSet::new();
        order.retain(|&r| {
            let row: Vec<V> = out_vals.iter().map(|c| c[r].clone()).collect();
            seen.insert(row_key(&row))
        });
    }
    let start = stmt.offset.unwrap_or(0) as usize;
    let order: Vec<usize> = match stmt.limit {
        Some(l) => order.into_iter().skip(start).take(l as usize).collect(),
        None => order.into_iter().skip(start).collect(),
    };
    let result = DataBlock { columns: out_cols, num_rows: n }.select_rows(&order);
    Ok(result)
}

fn column_hint<'a>(e: &Expr, block: &'a DataBlock) -> Option<&'a ColumnData> {
    match e { Expr::Col(c) => block.columns.iter().find(|x| &x.name == c).map(|x| &x.data), _ => None }
}

fn column_hint_g<'a>(e: &Expr, block: &'a DataBlock) -> Option<&'a ColumnData> { column_hint(e, block) }

// ─── set operations, ORDER BY / LIMIT on a finished result ────────────────────

fn rows_of(b: &DataBlock) -> Vec<Vec<V>> {
    let cols: Vec<Vec<V>> = b.columns.iter().map(column_values).collect();
    (0..b.num_rows).map(|r| cols.iter().map(|c| c[r].clone()).collect()).collect()
}

fn block_of(names: &[String], rows: Vec<Vec<V>>, hints: &[ColumnData]) -> DataBlock {
    let n = rows.len();
    let cols: Vec<Column> = names.iter().enumerate().map(|(k, name)| {
        vals_to_column(name.clone(), rows.iter().map(|r| r[k].clone()).collect(), hints.get(k))
    }).collect();
    DataBlock { columns: cols, num_rows: n }
}

/// UNION / INTERSECT / EXCEPT (with ALL variants); column names come from the left side.
pub fn apply_set_op(left: DataBlock, right: DataBlock, kind: &SetOpKind) -> Result<DataBlock, KoreError> {
    if left.columns.len() != right.columns.len() {
        return Err(err(format!("set operation arms have different column counts ({} vs {})", left.columns.len(), right.columns.len())));
    }
    // identical column types: plain concatenation keeps the representation (UNION ALL is common and can be large)
    let same_types = left.columns.iter().zip(&right.columns).all(|(a, b)| std::mem::discriminant(&a.data) == std::mem::discriminant(&b.data)
        && !matches!(a.data, ColumnData::StrDict { .. }));
    if matches!(kind, SetOpKind::UnionAll) && same_types && left.num_rows > 0 && right.num_rows > 0 {
        let mut r = right.clone();
        for (c, l) in r.columns.iter_mut().zip(&left.columns) { c.name = l.name.clone(); }
        return DataBlock::concat(vec![left, r]);
    }
    let names: Vec<String> = left.columns.iter().map(|c| c.name.clone()).collect();
    let hints: Vec<ColumnData> = left.columns.iter().map(|c| c.data.clone()).collect();
    let (lrows, rrows) = (rows_of(&left), rows_of(&right));
    let key = |r: &Vec<V>| row_key(r);
    let rows: Vec<Vec<V>> = match kind {
        SetOpKind::UnionAll => lrows.into_iter().chain(rrows).collect(),
        SetOpKind::Union => {
            let mut seen = HashSet::new();
            lrows.into_iter().chain(rrows).filter(|r| seen.insert(key(r))).collect()
        }
        SetOpKind::Intersect => {
            let rset: HashSet<String> = rrows.iter().map(key).collect();
            let mut seen = HashSet::new();
            lrows.into_iter().filter(|r| rset.contains(&key(r)) && seen.insert(key(r))).collect()
        }
        SetOpKind::Except => {
            let rset: HashSet<String> = rrows.iter().map(key).collect();
            let mut seen = HashSet::new();
            lrows.into_iter().filter(|r| !rset.contains(&key(r)) && seen.insert(key(r))).collect()
        }
        SetOpKind::IntersectAll | SetOpKind::ExceptAll => {
            let mut counts: HashMap<String, i64> = HashMap::new();
            for r in &rrows { *counts.entry(key(r)).or_insert(0) += 1; }
            let intersect = matches!(kind, SetOpKind::IntersectAll);
            lrows.into_iter().filter(|r| {
                let c = counts.entry(key(r)).or_insert(0);
                if *c > 0 { *c -= 1; intersect } else { !intersect }
            }).collect()
        }
    };
    Ok(block_of(&names, rows, &hints))
}

/// ORDER BY / OFFSET / LIMIT on an already computed result (set operations): columns are found by name,
/// ordinals are positions, anything else is evaluated against the result's columns.
pub fn order_limit(block: DataBlock, order_by: &[OrderByItem], limit: Option<u64>, offset: Option<u64>) -> Result<DataBlock, KoreError> {
    let n = block.num_rows;
    let mut order: Vec<usize> = (0..n).collect();
    if !order_by.is_empty() {
        let mut keys: Vec<Vec<V>> = Vec::new();
        for o in order_by {
            let k = match &o.expr {
                Expr::Int(p) => {
                    if *p < 1 || *p as usize > block.columns.len() { return Err(err(format!("ORDER BY position {p} is not in the select list"))); }
                    column_values(&block.columns[*p as usize - 1])
                }
                Expr::Col(c) | Expr::QualCol(_, c) => {
                    let full = match &o.expr { Expr::QualCol(t, c) => format!("{t}.{c}"), _ => c.clone() };
                    match find_col_idx(&block, &full).or_else(|| find_col_idx(&block, c)) {
                        Some(i) => column_values(&block.columns[i]),
                        None => return Err(err(format!("ORDER BY column not found: {full}"))),
                    }
                }
                other => (0..n).map(|r| eval_expr(other, &block, r)).collect(),
            };
            keys.push(k);
        }
        order.sort_by(|&a, &b| {
            for (k, item) in order_by.iter().enumerate() {
                let o = cmp_keys(&keys[k][a], &keys[k][b], item);
                if o != std::cmp::Ordering::Equal { return o; }
            }
            std::cmp::Ordering::Equal
        });
    }
    let start = offset.unwrap_or(0) as usize;
    let order: Vec<usize> = match limit {
        Some(l) => order.into_iter().skip(start).take(l as usize).collect(),
        None => order.into_iter().skip(start).collect(),
    };
    Ok(block.select_rows(&order))
}

/// A statement chained to further arms with UNION / INTERSECT / EXCEPT: run the arms, combine them left to
/// right, then apply the statement's own ORDER BY / LIMIT / OFFSET to the combined result.
pub fn execute_compound(stmt: &SelectStmt, ctx: &KqlContext) -> Result<DataBlock, KoreError> {
    let mut head = stmt.clone();
    head.set_ops = Vec::new();
    head.order_by = Vec::new();
    head.limit = None;
    head.offset = None;
    let mut acc = execute_select(&head, ctx)?;
    for (kind, arm) in &stmt.set_ops {
        let other = execute_select(arm, ctx)?;
        acc = apply_set_op(acc, other, kind)?;
    }
    order_limit(acc, &stmt.order_by, stmt.limit, stmt.offset)
}

#[allow(dead_code)]
fn unused(_: &V, _: &V) -> std::cmp::Ordering { total_cmp(&V::Null, &V::Null) }
