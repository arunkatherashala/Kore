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

// ---------------------------------------------------------------------------------------------------------------
// Join keys pushed into a UNION ALL derived table.
//
//   FROM (SELECT .. FROM a UNION ALL SELECT .. FROM b) u, d WHERE u.k = d.dk AND <filters on d only>
//
// Every arm row whose k-expression is not a `dk` of a qualifying `d` row is dropped by the inner join anyway, so each arm
// may carry `<k-expression> IN (SELECT dk FROM d WHERE <filters>)` and shrink before the arms are concatenated.

fn out_name(p: &Projection) -> Option<String> {
    match p {
        Projection::Expr { alias: Some(a), .. } => Some(a.clone()),
        Projection::Expr { expr: Expr::Col(c), .. } => Some(c.rsplit('.').next().unwrap_or(c).to_string()),
        Projection::Expr { expr: Expr::QualCol(_, c), .. } => Some(c.clone()),
        _ => None,
    }
}

fn plain_arm(s: &SelectStmt, ncols: usize) -> bool {
    !s.distinct && s.group_by.is_empty() && s.group_exprs.is_empty() && s.having.is_none() && s.qualify.is_none()
        && s.limit.is_none() && s.offset.is_none() && s.windows.is_empty() && s.pivot.is_none() && s.unpivot.is_none()
        && s.lateral_views.is_empty() && s.grouping == Grouping::Plain && s.order_by.is_empty()
        && s.projections.len() == ncols
        && s.projections.iter().all(|p| match p {
            Projection::Expr { expr, .. } => !crate::rewrite::contains_agg(expr) && !crate::ast_walk::any_node(expr, &|x| matches!(x, Expr::Window { .. } | Expr::AggX { .. })),
            Projection::Star => false,
        })
}

/// `col` as (qualifier, bare name).
fn split_col(e: &Expr) -> Option<(Option<&str>, &str)> {
    match e {
        Expr::Col(c) => Some((None, c.as_str())),
        Expr::QualCol(t, c) => Some((Some(t.as_str()), c.as_str())),
        _ => None,
    }
}

fn dbgp(s: String) { if crate::executor::prof_on() { eprintln!("[prof] {s}"); } }

pub(crate) fn push_union_keys(stmt: &SelectStmt, ctx: &KqlContext) -> Option<SelectStmt> {
    if crate::testing::no_union_push() || stmt.joins.is_empty() || stmt.where_clause.is_none() { return None; }
    if !stmt.joins.iter().all(|j| matches!(j.join_type, JoinKind::Implicit)) || stmt.pivot.is_some() || stmt.unpivot.is_some() || !stmt.set_ops.is_empty() { return None; }
    let tables: Vec<&TableExpr> = std::iter::once(&stmt.from).chain(stmt.joins.iter().map(|j| &j.table)).collect();
    let alias_of = |t: &TableExpr| t.alias.clone().unwrap_or_else(|| t.name.clone());
    // the derived UNION ALL table
    let di = tables.iter().position(|t| t.subquery.as_ref().map_or(false, |q| !q.set_ops.is_empty()));
    dbgp(format!("union push: derived idx {:?} of {}", di, tables.len()));
    let di = di?;
    let dt = tables[di];
    let sq = dt.subquery.as_ref()?;
    if !sq.set_ops.iter().all(|(k, a)| matches!(k, SetOpKind::UnionAll) && a.set_ops.is_empty()) || !sq.order_by.is_empty() || sq.limit.is_some() { return None; }
    let ncols = sq.projections.len();
    let names: Vec<Option<String>> = if dt.col_aliases.is_empty() { sq.projections.iter().map(out_name).collect() } else { dt.col_aliases.iter().cloned().map(Some).collect() };
    if names.len() != ncols { return None; }
    let mut first = (**sq).clone();
    first.set_ops = Vec::new();
    if !plain_arm(&first, ncols) || !sq.set_ops.iter().all(|(_, a)| plain_arm(a, ncols)) { return None; }
    let da = alias_of(dt);
    // the other tables must be plain catalog tables with known columns
    let mut cols: Vec<(String, Vec<String>)> = Vec::new();
    let mut others: Vec<&TableExpr> = Vec::new();
    for (i, t) in tables.iter().enumerate() {
        if i == di { continue; }
        if t.subquery.is_some() || t.values.is_some() { return None; }
        let b = ctx.get(&t.name)?;
        cols.push((alias_of(t), b.columns.iter().map(|c| c.name.rsplit('.').next().unwrap_or(&c.name).to_string()).collect()));
        others.push(t);
    }
    let derived_pos = |q: Option<&str>, c: &str| -> Option<usize> {
        if let Some(q) = q { if !q.eq_ignore_ascii_case(&da) { return None; } }
        else if cols.iter().any(|(_, cs)| cs.iter().any(|x| x == c)) { return None; }
        names.iter().position(|n| n.as_deref().map_or(false, |n| n.eq_ignore_ascii_case(c)))
    };
    // the table a column belongs to
    let owner = |q: Option<&str>, c: &str| -> Option<usize> {
        let hits: Vec<usize> = cols.iter().enumerate().filter(|(_, (a, cs))| q.map_or(true, |q| q.eq_ignore_ascii_case(a)) && cs.iter().any(|x| x == c)).map(|(i, _)| i).collect();
        if hits.len() == 1 && !(q.is_none() && names.iter().any(|n| n.as_deref() == Some(c))) { Some(hits[0]) } else { None }
    };
    dbgp(format!("union push: arms ok, cols={}", cols.len()));
    let mut conj = Vec::new();
    crate::rewrite::split_conjuncts(stmt.where_clause.as_ref()?, &mut conj);
    if !crate::testing::no_cast_fold() { for c in conj.iter_mut() { *c = crate::executor::fold_date_casts(c); } }
    // links: (derived column position, table index, table column)
    let mut links: Vec<(usize, usize, String)> = Vec::new();
    for c in &conj {
        if let Expr::BinOp { op: BinOpKind::Eq, left, right } = c {
            if let (Some((lq, lc)), Some((rq, rc))) = (split_col(left), split_col(right)) {
                if let (Some(p), Some(t)) = (derived_pos(lq, lc), owner(rq, rc)) { links.push((p, t, rc.to_string())); }
                else if let (Some(p), Some(t)) = (derived_pos(rq, rc), owner(lq, lc)) { links.push((p, t, lc.to_string())); }
            }
        }
    }
    let mut sets: Vec<(usize, SelectStmt)> = Vec::new();
    for (pos, ti, tcol) in links {
        // filters that read this table only
        let mut fs = Vec::new();
        for c in &conj {
            let mut refs = Vec::new();
            if !crate::rewrite::referenced_cols(c, &mut refs) || refs.is_empty() { continue; }
            let all_t = refs.iter().all(|r| {
                let (q, n) = match r.split_once('.') { Some((q, n)) => (Some(q), n), None => (None, r.as_str()) };
                owner(q, n) == Some(ti)
            });
            if all_t { fs.push(c.clone()); }
        }
        let Some(w) = crate::rewrite::and_all(fs) else { continue };
        let mut sub = SelectStmt::star_from(others[ti].clone());
        sub.projections = vec![Projection::Expr { expr: Expr::Col(tcol), alias: None }];
        sub.where_clause = Some(w);
        sets.push((pos, sub));
    }
    dbgp(format!("union push: links sets={}", sets.len()));
    if sets.is_empty() { return None; }
    let mut out = stmt.clone();
    let dsub = out.joins.iter_mut().map(|j| &mut j.table).chain(std::iter::once(&mut out.from)).find(|t| t.subquery.as_ref().map_or(false, |q| !q.set_ops.is_empty()) && alias_of(t) == da)?;
    let q = dsub.subquery.as_mut()?;
    let mut changed = false;
    let mut apply = |arm: &mut SelectStmt| {
        for (pos, sub) in &sets {
            let Projection::Expr { expr, .. } = &arm.projections[*pos] else { continue };
            let inn = Expr::InSubquery { expr: Box::new(expr.clone()), subquery: Box::new(sub.clone()), negated: false };
            let mut have = Vec::new();
            if let Some(w) = &arm.where_clause { crate::rewrite::split_conjuncts(w, &mut have); }
            if have.contains(&inn) { continue; }
            have.push(inn);
            arm.where_clause = crate::rewrite::and_all(have);
            changed = true;
        }
    };
    apply(q.as_mut());
    for (_, a) in q.set_ops.iter_mut() { apply(a); }
    if changed { Some(out) } else { None }
}
