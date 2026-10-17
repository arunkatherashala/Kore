//! Semi-structure for the TPC-DS Q95 CTE: `SELECT a.k, a.x, b.x FROM t a, t b WHERE a.k = b.k AND a.x <> b.x`
//! materialises one row per ordered pair of rows of a key with different `x` (quadratic in the group size).
//! When the CTE is only consumed as a set of keys (`expr IN (SELECT k FROM cte ..)`, no aggregate, no other
//! column of the CTE read), the CTE is replaced by the distinct qualifying keys,
//! `SELECT k FROM t WHERE k IS NOT NULL GROUP BY k HAVING MIN(x) <> MAX(x)`: a key has two rows with different
//! non-NULL `x` exactly when its smallest and largest non-NULL `x` differ (NULL `x` never matches).
//! Every other shape is left alone.

use crate::ast::*;
use crate::ast_walk::{walk_own_exprs, walk_stmt};
use crate::executor::KqlContext;
use kore_core::ColumnData;

fn ident_ok(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !s.chars().next().unwrap().is_ascii_digit()
}

fn conjuncts<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
    match e {
        Expr::BinOp { op: BinOpKind::And, left, right } => { conjuncts(left, out); conjuncts(right, out); }
        other => out.push(other),
    }
}

/// `(alias, column)` of a qualified column.
fn qc(e: &Expr) -> Option<(&str, &str)> { if let Expr::QualCol(a, c) = e { Some((a.as_str(), c.as_str())) } else { None } }

struct PairCte { table: String, key: String, x: String, key_name: String, other_names: Vec<String> }

fn pair_shape(s: &SelectStmt) -> Option<PairCte> {
    if s.distinct || !s.group_by.is_empty() || !s.group_exprs.is_empty() || s.having.is_some() || s.qualify.is_some()
        || !s.order_by.is_empty() || s.limit.is_some() || s.offset.is_some() || !s.set_ops.is_empty() || !s.windows.is_empty()
        || !s.lateral_views.is_empty() || s.pivot.is_some() || s.unpivot.is_some() || s.grouping != Grouping::Plain { return None; }
    let (f, j) = (&s.from, s.joins.first()?);
    if s.joins.len() != 1 || !matches!(j.join_type, JoinKind::Implicit) || j.push_filter.is_some() || j.on.expr.is_some() || !j.using.is_empty() || j.natural { return None; }
    for t in [f, &j.table] {
        if t.subquery.is_some() || t.values.is_some() || t.push_filter.is_some() || !t.col_aliases.is_empty() { return None; }
    }
    if !f.name.eq_ignore_ascii_case(&j.table.name) || !ident_ok(&f.name) { return None; }
    let a = f.alias.clone().unwrap_or_else(|| f.name.clone());
    let b = j.table.alias.clone().unwrap_or_else(|| j.table.name.clone());
    if a.eq_ignore_ascii_case(&b) { return None; }
    let mut cs = Vec::new();
    conjuncts(s.where_clause.as_ref()?, &mut cs);
    if cs.len() != 2 { return None; }
    let (mut key, mut x) = (None, None);
    for c in cs {
        if let Expr::BinOp { op, left, right } = c {
            let (l, r) = (qc(left)?, qc(right)?);
            if !l.1.eq_ignore_ascii_case(r.1) { return None; }
            let sides = (l.0.eq_ignore_ascii_case(&a) && r.0.eq_ignore_ascii_case(&b)) || (l.0.eq_ignore_ascii_case(&b) && r.0.eq_ignore_ascii_case(&a));
            if !sides { return None; }
            match op { BinOpKind::Eq => key = Some(l.1.to_string()), BinOpKind::Ne => x = Some(l.1.to_string()), _ => return None }
        } else { return None; }
    }
    let (key, x) = (key?, x?);
    if !ident_ok(&key) || !ident_ok(&x) || key.eq_ignore_ascii_case(&x) { return None; }
    // projections: plain columns of either alias, named by alias or column name
    let (mut key_name, mut others) = (None::<String>, Vec::new());
    for p in &s.projections {
        let Projection::Expr { expr, alias } = p else { return None };
        let (q, c) = qc(expr)?;
        if !(q.eq_ignore_ascii_case(&a) || q.eq_ignore_ascii_case(&b)) { return None; }
        let name = alias.clone().unwrap_or_else(|| c.to_string());
        if !ident_ok(&name) { return None; }
        if c.eq_ignore_ascii_case(&key) && key_name.is_none() { key_name = Some(name); }
        else if c.eq_ignore_ascii_case(&key) || c.eq_ignore_ascii_case(&x) { others.push(name); }
        else { return None; }
    }
    let key_name = key_name?;
    if others.iter().any(|n| n.eq_ignore_ascii_case(&key_name)) { return None; }
    Some(PairCte { table: f.name.clone(), key, x, key_name, other_names: others })
}

/// Number of table references named `name` in `s` (FROM / JOIN, at any nesting depth).
fn count_refs(s: &SelectStmt, name: &str) -> usize {
    let tab = |t: &TableExpr| usize::from(t.subquery.is_none() && t.values.is_none() && t.name.eq_ignore_ascii_case(name));
    let mut n = tab(&s.from);
    for j in &s.joins { n += tab(&j.table); }
    if let Some(q) = &s.from.subquery { n += count_refs(q, name); }
    for j in &s.joins { if let Some(q) = &j.table.subquery { n += count_refs(q, name); } }
    for (_, arm) in &s.set_ops { n += count_refs(arm, name); }
    walk_own_exprs(s, &mut |e| match e {
        Expr::ScalarSubquery(x) | Expr::Exists { subquery: x, .. } | Expr::InSubquery { subquery: x, .. } | Expr::QuantSubquery { subquery: x, .. } => n += count_refs(x, name),
        _ => {}
    });
    n
}

/// An IN subquery whose result set only depends on the set of key values of the CTE: number of CTE references in it.
fn set_consumer(sub: &SelectStmt, name: &str, banned: &[String]) -> Option<usize> {
    if !sub.group_by.is_empty() || !sub.group_exprs.is_empty() || sub.having.is_some() || sub.qualify.is_some() || sub.limit.is_some()
        || sub.offset.is_some() || !sub.set_ops.is_empty() || !sub.windows.is_empty() || sub.pivot.is_some() || sub.unpivot.is_some()
        || !sub.lateral_views.is_empty() || sub.grouping != Grouping::Plain { return None; }
    if sub.projections.len() != 1 || matches!(sub.projections[0], Projection::Star) { return None; }
    if sub.joins.iter().any(|j| !matches!(j.join_type, JoinKind::Implicit | JoinKind::Inner | JoinKind::Cross) || !j.using.is_empty() || j.natural) { return None; }
    let mut direct = 0;
    for t in std::iter::once(&sub.from).chain(sub.joins.iter().map(|j| &j.table)) {
        if t.subquery.is_none() && t.values.is_none() && t.name.eq_ignore_ascii_case(name) {
            if !t.col_aliases.is_empty() { return None; }
            direct += 1;
        }
    }
    if direct == 0 { return None; }
    let mut ok = true;
    walk_stmt(sub, true, &mut |e| match e {
        Expr::Agg { .. } | Expr::AggX { .. } | Expr::Window { .. } | Expr::Star => ok = false,
        Expr::Col(c) | Expr::QualCol(_, c) => { if banned.iter().any(|b| b.eq_ignore_ascii_case(c)) { ok = false; } }
        _ => {}
    });
    if ok { Some(direct) } else { None }
}

fn rewritten_body(p: &PairCte) -> Option<SelectStmt> {
    let sql = format!(
        "SELECT {k} AS {n} FROM {t} WHERE {k} IS NOT NULL GROUP BY {k} HAVING MIN({x}) <> MAX({x})",
        k = p.key, n = p.key_name, t = p.table, x = p.x);
    crate::parser::parse_query(&sql).ok()?.body
}

/// Replace qualifying pair-join CTEs by their distinct key sets. Returns None when nothing applies.
pub(crate) fn rewrite(query: &Query, ctx: &KqlContext) -> Option<Query> {
    if query.ctes.is_empty() || crate::testing::no_semi_cte() { return None; }
    let mut out: Option<Query> = None;
    for (i, cte) in query.ctes.iter().enumerate() {
        if cte.recursive { continue; }
        let Some(p) = pair_shape(&cte.body) else { continue };
        // the base table must be a real table with integer key and x
        let Some(tb) = ctx.get(&p.table) else { continue };
        let int_col = |n: &str| tb.columns.iter().find(|c| c.name.eq_ignore_ascii_case(n)).map_or(false, |c| matches!(c.data, ColumnData::Int64(_)));
        if !int_col(&p.key) || !int_col(&p.x) { continue; }
        let q = out.as_ref().unwrap_or(query);
        // every other statement: all references of the CTE must be set consumers
        let (mut total, mut approved) = (0, 0);
        let mut stmts: Vec<&SelectStmt> = Vec::new();
        for (j, c) in q.ctes.iter().enumerate() { if j != i { stmts.push(&c.body); } }
        if let Some(b) = &q.body { stmts.push(b); }
        stmts.extend(q.set_ops.iter().map(|(_, s)| s));
        for s in &stmts {
            total += count_refs(s, &cte.name);
            walk_stmt(s, true, &mut |e| if let Expr::InSubquery { subquery, .. } = e {
                if let Some(d) = set_consumer(subquery, &cte.name, &p.other_names) { approved += d; }
            });
        }
        if total == 0 || total != approved { continue; }
        let Some(body) = rewritten_body(&p) else { continue };
        let mut nq = q.clone();
        nq.ctes[i].body = body;
        out = Some(nq);
    }
    out
}
