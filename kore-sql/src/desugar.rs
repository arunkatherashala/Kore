//! AST-to-AST rewrites that turn SQL forms the executor has no direct code for into forms it does:
//! `x op ANY|ALL (subquery)`, `LEFT SEMI|ANTI JOIN`, `JOIN .. USING (..)` and `NATURAL JOIN`.

use std::cell::RefCell;

use kore_core::KoreError;

use crate::ast::*;
use crate::ast_walk::{map_expr, map_stmt_exprs, walk_own_exprs};
use crate::executor::{execute_select, KqlContext};

fn err(msg: impl Into<String>) -> KoreError { KoreError::InvalidArgument(msg.into()) }

fn bare(name: &str) -> String { name.rsplit('.').next().unwrap_or(name).to_string() }

fn and(a: Option<Expr>, b: Expr) -> Expr {
    match a { None => b, Some(a) => Expr::BinOp { op: BinOpKind::And, left: Box::new(a), right: Box::new(b) } }
}

/// `t.c` / `c` as a column expression.
fn col_expr(name: &str) -> Expr {
    match name.split_once('.') {
        Some((t, c)) => Expr::QualCol(t.to_string(), c.to_string()),
        None => Expr::Col(name.to_string()),
    }
}

fn alias_of(t: &TableExpr) -> String { t.alias.clone().unwrap_or_else(|| t.name.clone()) }

/// Column names a FROM item produces.
fn table_columns(t: &TableExpr, ctx: &KqlContext) -> Result<Vec<String>, KoreError> {
    if !t.col_aliases.is_empty() { return Ok(t.col_aliases.clone()); }
    if let Some(rows) = &t.values {
        return Ok((0..rows.first().map_or(0, |r| r.len())).map(|i| format!("col{}", i + 1)).collect());
    }
    if let Some(sub) = &t.subquery {
        // run it with LIMIT 0 to learn the output names (set operations and SELECT * included)
        let mut probe = (**sub).clone();
        probe.limit = Some(0);
        probe.offset = None;
        let b = execute_select(&probe, ctx)?;
        return Ok(b.columns.iter().map(|c| bare(&c.name)).collect());
    }
    if let Some(b) = ctx.get(&t.name) { return Ok(b.columns.iter().map(|c| bare(&c.name)).collect()); }
    let q = ctx.query(&format!("SELECT * FROM {} LIMIT 0", t.name))
        .map_err(|_| err(format!("unknown table: {}", t.name)))?;
    Ok(q.columns.iter().map(|c| bare(&c.name)).collect())
}

/// `Some(rewritten)` when the statement uses a form this module desugars.
pub fn rewrite_stmt(stmt: &SelectStmt, ctx: &KqlContext) -> Result<Option<SelectStmt>, KoreError> {
    let mut cur = stmt.clone();
    let mut changed = false;

    // quantified comparisons
    let mut has_quant = false;
    walk_own_exprs(stmt, &mut |e| {
        if matches!(e, Expr::QuantSubquery { .. }) { has_quant = true; }
    });
    if has_quant {
        let failure: RefCell<Option<KoreError>> = RefCell::new(None);
        cur = map_stmt_exprs(&cur, &mut |e| match e {
            Expr::QuantSubquery { expr, op, all, subquery } => match quant_to_plain(expr, op, *all, subquery) {
                Ok(r) => Some(r),
                Err(er) => { *failure.borrow_mut() = Some(er); Some(Expr::Null) }
            },
            _ => None,
        });
        if let Some(e) = failure.into_inner() { return Err(e); }
        changed = true;
    }

    // joins
    if cur.joins.iter().any(|j| matches!(j.join_type, JoinKind::Semi | JoinKind::Anti) || !j.using.is_empty() || j.natural) {
        cur = rewrite_joins(cur, ctx)?;
        changed = true;
    }
    Ok(if changed { Some(cur) } else { None })
}

/// `x op ANY|ALL (sub)` as IN / NOT IN / a comparison with a MIN or MAX scalar subquery.
fn quant_to_plain(lhs: &Expr, op: &BinOpKind, all: bool, sub: &SelectStmt) -> Result<Expr, KoreError> {
    use BinOpKind::*;
    match (op, all) {
        (Eq, false) => return Ok(Expr::InSubquery { expr: Box::new(lhs.clone()), subquery: Box::new(sub.clone()), negated: false }),
        (Ne, true) => return Ok(Expr::InSubquery { expr: Box::new(lhs.clone()), subquery: Box::new(sub.clone()), negated: true }),
        (Ne, false) => return Err(err("<> ANY (subquery) is not supported")),
        _ => {}
    }
    // name the subquery's single output column so an aggregate can refer to it
    let mut inner = sub.clone();
    match inner.projections.first_mut() {
        Some(Projection::Expr { alias, .. }) => *alias = Some("__q0".into()),
        _ => return Err(err("ANY / ALL needs a subquery with an explicit select list")),
    }
    inner.projections.truncate(1);
    let scalar = |func: &str| {
        let mut s = SelectStmt::star_from(TableExpr {
            name: "__q".into(), alias: Some("__q".into()), subquery: Some(Box::new(inner.clone())),
            values: None, push_filter: None, col_aliases: Vec::new(),
        });
        s.projections = vec![Projection::Expr {
            expr: Expr::AggX { name: func.into(), args: vec![Expr::Col("__q0".into())], distinct: false, filter: None },
            alias: None,
        }];
        Expr::ScalarSubquery(Box::new(s))
    };
    let use_max = match (op, all) {
        (Gt | Ge, true) | (Lt | Le, false) => true,
        (Gt | Ge, false) | (Lt | Le, true) => false,
        (Eq, true) => {
            // x = ALL: every value equals x
            let eq_min = Expr::BinOp { op: Eq, left: Box::new(lhs.clone()), right: Box::new(scalar("MIN")) };
            let eq_max = Expr::BinOp { op: Eq, left: Box::new(lhs.clone()), right: Box::new(scalar("MAX")) };
            let both = Expr::BinOp { op: And, left: Box::new(eq_min), right: Box::new(eq_max) };
            return Ok(Expr::BinOp { op: Or, left: Box::new(Expr::Exists { subquery: Box::new(sub.clone()), negated: true }), right: Box::new(both) });
        }
        _ => return Err(err("unsupported quantified comparison")),
    };
    let cmp = Expr::BinOp { op: op.clone(), left: Box::new(lhs.clone()), right: Box::new(scalar(if use_max { "MAX" } else { "MIN" })) };
    if all {
        // ALL over an empty set is true
        Ok(Expr::BinOp { op: Or, left: Box::new(Expr::Exists { subquery: Box::new(sub.clone()), negated: true }), right: Box::new(cmp) })
    } else {
        Ok(cmp)
    }
}

fn rewrite_joins(mut s: SelectStmt, ctx: &KqlContext) -> Result<SelectStmt, KoreError> {
    // visible tables so far: (alias, columns)
    let mut scopes: Vec<(String, Vec<String>)> = vec![(alias_of(&s.from), table_columns(&s.from, ctx)?)];
    // columns of `SELECT *` in output order (qualifier, expression, name)
    let mut star: Vec<(Expr, String)> = scopes[0].1.iter().map(|c| (Expr::QualCol(scopes[0].0.clone(), c.clone()), c.clone())).collect();
    // unqualified references to a USING / NATURAL column
    let mut merged: Vec<(String, Expr)> = Vec::new();
    let mut used_star = false;
    let mut new_joins: Vec<JoinClause> = Vec::new();
    let mut extra_where: Option<Expr> = None;

    for j in std::mem::take(&mut s.joins) {
        let right_alias = alias_of(&j.table);
        match j.join_type {
            JoinKind::Semi | JoinKind::Anti => {
                let cond = match (&j.on.expr, j.on.left_col.is_empty()) {
                    (Some(e), _) => e.clone(),
                    (None, false) => Expr::BinOp { op: BinOpKind::Eq, left: Box::new(col_expr(&j.on.left_col)), right: Box::new(col_expr(&j.on.right_col)) },
                    _ => return Err(err("SEMI / ANTI JOIN needs an ON condition")),
                };
                let mut sub = SelectStmt::star_from(j.table.clone());
                sub.projections = vec![Projection::Expr { expr: Expr::Int(1), alias: None }];
                sub.where_clause = Some(cond);
                let ex = Expr::Exists { subquery: Box::new(sub), negated: j.join_type == JoinKind::Anti };
                extra_where = Some(and(extra_where, ex));
            }
            _ if !j.using.is_empty() || j.natural => {
                let right_cols = table_columns(&j.table, ctx)?;
                let using: Vec<String> = if j.natural {
                    let mut common = Vec::new();
                    for (_, cols) in &scopes {
                        for c in cols {
                            if right_cols.iter().any(|r| r.eq_ignore_ascii_case(c)) && !common.iter().any(|x: &String| x.eq_ignore_ascii_case(c)) { common.push(c.clone()); }
                        }
                    }
                    common
                } else { j.using.clone() };
                let mut on: Option<Expr> = None;
                for c in &using {
                    let owner = scopes.iter().find(|(_, cols)| cols.iter().any(|x| x.eq_ignore_ascii_case(c)))
                        .ok_or_else(|| err(format!("USING column '{c}' not found on the left side of the join")))?;
                    if !right_cols.iter().any(|x| x.eq_ignore_ascii_case(c)) {
                        return Err(err(format!("USING column '{c}' not found on the right side of the join")));
                    }
                    let l = Expr::QualCol(owner.0.clone(), c.clone());
                    let r = Expr::QualCol(right_alias.clone(), c.clone());
                    on = Some(and(on, Expr::BinOp { op: BinOpKind::Eq, left: Box::new(l.clone()), right: Box::new(r.clone()) }));
                    let prev = merged.iter().position(|(m, _)| m.eq_ignore_ascii_case(c));
                    let prev_expr = prev.map(|i| merged[i].1.clone()).unwrap_or(l);
                    let value = match j.join_type {
                        JoinKind::Right => r,
                        JoinKind::Full => Expr::FuncCall { name: "COALESCE".into(), args: vec![prev_expr, r] },
                        _ => prev_expr,
                    };
                    match prev { Some(i) => merged[i].1 = value, None => merged.push((c.clone(), value)) }
                }
                // SELECT *: the joined columns once and first, then what is left of both sides
                let is_used = |n: &str| using.iter().any(|u| u.eq_ignore_ascii_case(n));
                let mut new_star: Vec<(Expr, String)> = using.iter().map(|c| (merged.iter().find(|(m, _)| m.eq_ignore_ascii_case(c)).unwrap().1.clone(), c.clone())).collect();
                new_star.extend(star.iter().filter(|(_, n)| !is_used(n)).cloned());
                new_star.extend(right_cols.iter().filter(|c| !is_used(c)).map(|c| (Expr::QualCol(right_alias.clone(), c.clone()), c.clone())));
                star = new_star;
                used_star = true;
                let mut nj = j.clone();
                nj.using = Vec::new();
                nj.natural = false;
                match on {
                    Some(e) => { nj.on = JoinOn { left_col: String::new(), right_col: String::new(), expr: Some(e) }; }
                    None => { nj.join_type = JoinKind::Cross; }
                }
                scopes.push((right_alias, right_cols));
                new_joins.push(nj);
            }
            _ => {
                let right_cols = table_columns(&j.table, ctx)?;
                star.extend(right_cols.iter().map(|c| (Expr::QualCol(right_alias.clone(), c.clone()), c.clone())));
                scopes.push((right_alias, right_cols));
                new_joins.push(j);
            }
        }
    }
    s.joins = new_joins;
    if let Some(w) = extra_where {
        s.where_clause = Some(and(s.where_clause.take(), w));
    }

    // unqualified references to merged columns
    if !merged.is_empty() {
        let mut f = |e: &Expr| -> Option<Expr> {
            if let Expr::Col(c) = e {
                return merged.iter().find(|(m, _)| m.eq_ignore_ascii_case(c)).map(|(_, v)| v.clone());
            }
            None
        };
        // SELECT-list items keep their visible name
        let projections: Vec<Projection> = s.projections.iter().map(|p| match p {
            Projection::Expr { expr, alias } => {
                let alias = alias.clone().or_else(|| if let Expr::Col(c) = expr { Some(c.clone()) } else { None });
                Projection::Expr { expr: map_expr(expr, &mut f), alias }
            }
            other => other.clone(),
        }).collect();
        let mut mapped = map_stmt_exprs(&s, &mut f);
        mapped.projections = projections;
        s = mapped;
    }
    if used_star {
        let mut out = Vec::new();
        for p in std::mem::take(&mut s.projections) {
            match p {
                Projection::Star => out.extend(star.iter().map(|(e, n)| Projection::Expr { expr: e.clone(), alias: Some(n.clone()) })),
                other => out.push(other),
            }
        }
        s.projections = out;
    }
    Ok(s)
}
