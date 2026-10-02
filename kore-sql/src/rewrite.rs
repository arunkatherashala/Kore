//! Pure AST rewrites used by the executor: conjunct handling for implicit (comma) joins and lifting of
//! aggregates that are nested inside larger expressions (`100 * sum(a) / sum(b)`, `sum(x) / 7.0`, ...).

use crate::ast::*;

/// Flatten `a AND b AND c` into `[a, b, c]`.
pub fn split_conjuncts(e: &Expr, out: &mut Vec<Expr>) {
    match e {
        Expr::BinOp { op: BinOpKind::And, left, right } => {
            split_conjuncts(left, out);
            split_conjuncts(right, out);
        }
        other => out.push(other.clone()),
    }
}

/// `(k AND a) OR (k AND b)` implies `k`: append the conjuncts shared by every OR branch so that a
/// join key hidden inside a disjunction (TPC-H Q19) can still drive a hash join. The OR itself stays.
pub fn factor_common_from_or(conjuncts: &mut Vec<Expr>) {
    fn branches(e: &Expr, out: &mut Vec<Expr>) {
        match e {
            Expr::BinOp { op: BinOpKind::Or, left, right } => { branches(left, out); branches(right, out); }
            other => out.push(other.clone()),
        }
    }
    let mut extra = Vec::new();
    for c in conjuncts.iter() {
        if !matches!(c, Expr::BinOp { op: BinOpKind::Or, .. }) { continue; }
        let mut bs = Vec::new();
        branches(c, &mut bs);
        let parts: Vec<Vec<Expr>> = bs.iter().map(|b| { let mut v = Vec::new(); split_conjuncts(b, &mut v); v }).collect();
        for cand in &parts[0] {
            if parts.iter().all(|p| p.contains(cand)) && !conjuncts.contains(cand) && !extra.contains(cand) {
                extra.push(cand.clone());
            }
        }
    }
    conjuncts.extend(extra);
}

pub fn and_all(mut v: Vec<Expr>) -> Option<Expr> {
    let first = if v.is_empty() { return None } else { v.remove(0) };
    Some(v.into_iter().fold(first, |acc, e| Expr::BinOp { op: BinOpKind::And, left: Box::new(acc), right: Box::new(e) }))
}

/// Rewrite `alias.col` references to plain `col` so the expression can be evaluated on a table whose
/// columns are not prefixed yet. Other qualifiers are left alone.
pub fn unqualify(e: &Expr, alias: &str) -> Expr {
    let u = |x: &Expr| Box::new(unqualify(x, alias));
    match e {
        Expr::QualCol(t, c) if t == alias => Expr::Col(c.clone()),
        Expr::BinOp { op, left, right } => Expr::BinOp { op: op.clone(), left: u(left), right: u(right) },
        Expr::Not(x) => Expr::Not(u(x)),
        Expr::IsNull(x) => Expr::IsNull(u(x)),
        Expr::IsNotNull(x) => Expr::IsNotNull(u(x)),
        Expr::Case { operand, branches, else_val } => Expr::Case {
            operand: operand.as_ref().map(|o| u(o)),
            branches: branches.iter().map(|(c, v)| (u(c), u(v))).collect(),
            else_val: else_val.as_ref().map(|x| u(x)),
        },
        Expr::In { expr, values, negated } => Expr::In {
            expr: u(expr), values: values.iter().map(|v| unqualify(v, alias)).collect(), negated: *negated,
        },
        Expr::Between { expr, low, high, negated } => Expr::Between { expr: u(expr), low: u(low), high: u(high), negated: *negated },
        Expr::Like { expr, pattern, negated } => Expr::Like { expr: u(expr), pattern: u(pattern), negated: *negated },
        Expr::ILike { expr, pattern, negated } => Expr::ILike { expr: u(expr), pattern: u(pattern), negated: *negated },
        Expr::FuncCall { name, args } => Expr::FuncCall { name: name.clone(), args: args.iter().map(|a| unqualify(a, alias)).collect() },
        other => other.clone(),
    }
}

/// `Col(x)` -> "x", `QualCol(t, c)` -> "t.c".
pub fn col_ref(e: &Expr) -> Option<String> {
    match e {
        Expr::Col(c) => Some(c.clone()),
        Expr::QualCol(t, c) => Some(format!("{t}.{c}")),
        _ => None,
    }
}

/// Collect referenced column names. Returns false when the expression holds something that must stay
/// where it is (aggregate, window, subquery), so the caller must not move or reorder it.
pub fn referenced_cols(e: &Expr, out: &mut Vec<String>) -> bool {
    match e {
        Expr::Col(_) | Expr::QualCol(..) => { out.extend(col_ref(e)); true }
        Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Null | Expr::Star => true,
        Expr::BinOp { left, right, .. } => referenced_cols(left, out) && referenced_cols(right, out),
        Expr::Not(x) | Expr::IsNull(x) | Expr::IsNotNull(x) => referenced_cols(x, out),
        Expr::Case { operand, branches, else_val } => {
            operand.as_ref().map_or(true, |o| referenced_cols(o, out))
                && branches.iter().all(|(c, r)| referenced_cols(c, out) && referenced_cols(r, out))
                && else_val.as_ref().map_or(true, |x| referenced_cols(x, out))
        }
        Expr::In { expr, values, .. } => referenced_cols(expr, out) && values.iter().all(|v| referenced_cols(v, out)),
        Expr::Between { expr, low, high, .. } => referenced_cols(expr, out) && referenced_cols(low, out) && referenced_cols(high, out),
        Expr::Like { expr, pattern, .. } | Expr::ILike { expr, pattern, .. } => referenced_cols(expr, out) && referenced_cols(pattern, out),
        Expr::FuncCall { args, .. } => args.iter().all(|a| referenced_cols(a, out)),
        Expr::Array(items) => items.iter().all(|a| referenced_cols(a, out)),
        Expr::Agg { .. } | Expr::Window { .. } | Expr::ScalarSubquery(_) | Expr::InSubquery { .. }
        | Expr::Exists { .. } | Expr::Explode(_) => false,
    }
}

/// True when an aggregate call appears anywhere in `e` (subqueries and windows excluded).
pub fn contains_agg(e: &Expr) -> bool {
    match e {
        Expr::Agg { .. } => true,
        Expr::BinOp { left, right, .. } => contains_agg(left) || contains_agg(right),
        Expr::Not(x) | Expr::IsNull(x) | Expr::IsNotNull(x) | Expr::Explode(x) => contains_agg(x),
        Expr::Case { operand, branches, else_val } => {
            operand.as_ref().map_or(false, |o| contains_agg(o))
                || branches.iter().any(|(c, r)| contains_agg(c) || contains_agg(r))
                || else_val.as_ref().map_or(false, |x| contains_agg(x))
        }
        Expr::In { expr, values, .. } => contains_agg(expr) || values.iter().any(contains_agg),
        Expr::Between { expr, low, high, .. } => contains_agg(expr) || contains_agg(low) || contains_agg(high),
        Expr::Like { expr, pattern, .. } | Expr::ILike { expr, pattern, .. } => contains_agg(expr) || contains_agg(pattern),
        Expr::FuncCall { args, .. } => args.iter().any(contains_agg),
        Expr::Array(items) => items.iter().any(contains_agg),
        _ => false,
    }
}

/// Replace every aggregate in `e` by a column reference to a hidden aggregate output.
fn replace_aggs(e: &Expr, hidden: &mut Vec<(String, Expr)>) -> Expr {
    let mut r = |x: &Expr, h: &mut Vec<(String, Expr)>| Box::new(replace_aggs(x, h));
    match e {
        Expr::Agg { .. } => {
            if let Some((name, _)) = hidden.iter().find(|(_, a)| a == e) {
                return Expr::Col(name.clone());
            }
            let name = format!("__agg{}", hidden.len());
            hidden.push((name.clone(), e.clone()));
            Expr::Col(name)
        }
        Expr::BinOp { op, left, right } => Expr::BinOp { op: op.clone(), left: r(left, hidden), right: r(right, hidden) },
        Expr::Not(x) => Expr::Not(r(x, hidden)),
        Expr::IsNull(x) => Expr::IsNull(r(x, hidden)),
        Expr::IsNotNull(x) => Expr::IsNotNull(r(x, hidden)),
        Expr::Case { operand, branches, else_val } => Expr::Case {
            operand: operand.as_ref().map(|o| r(o, hidden)),
            branches: branches.iter().map(|(c, v)| (r(c, hidden), r(v, hidden))).collect(),
            else_val: else_val.as_ref().map(|x| r(x, hidden)),
        },
        Expr::In { expr, values, negated } => Expr::In {
            expr: r(expr, hidden),
            values: values.iter().map(|v| replace_aggs(v, hidden)).collect(),
            negated: *negated,
        },
        Expr::Between { expr, low, high, negated } => Expr::Between {
            expr: r(expr, hidden), low: r(low, hidden), high: r(high, hidden), negated: *negated,
        },
        Expr::Like { expr, pattern, negated } => Expr::Like { expr: r(expr, hidden), pattern: r(pattern, hidden), negated: *negated },
        Expr::ILike { expr, pattern, negated } => Expr::ILike { expr: r(expr, hidden), pattern: r(pattern, hidden), negated: *negated },
        Expr::FuncCall { name, args } => Expr::FuncCall { name: name.clone(), args: args.iter().map(|a| replace_aggs(a, hidden)).collect() },
        other => other.clone(),
    }
}

/// Replace every aggregate call in `e` by a column `__aggN`; returns the rewritten expression and the
/// aggregates it replaced, in order.
pub fn extract_aggs(e: &Expr) -> (Expr, Vec<(String, Expr)>) {
    let mut hidden = Vec::new();
    let rewritten = replace_aggs(e, &mut hidden);
    (rewritten, hidden)
}

/// `SELECT 100 * SUM(a) / SUM(b) ... GROUP BY k` is executed as
/// `SELECT 100 * __agg0 / __agg1 ... FROM (SELECT k, SUM(a) AS __agg0, SUM(b) AS __agg1 ... GROUP BY k)`.
/// Returns None when the statement has no aggregate nested inside a larger expression.
pub fn lift_aggregates(stmt: &SelectStmt) -> Option<SelectStmt> {
    let nested = |p: &Projection| matches!(p, Projection::Expr { expr, .. }
        if !matches!(expr, Expr::Agg { .. } | Expr::Window { .. }) && contains_agg(expr));
    let having_aggs = stmt.having.as_ref().map_or(false, contains_agg);
    if !stmt.projections.iter().any(nested) && !having_aggs { return None; }
    if stmt.projections.iter().any(|p| matches!(p, Projection::Star | Projection::Expr { expr: Expr::Window { .. }, .. })) {
        return None;
    }

    let mut hidden: Vec<(String, Expr)> = Vec::new();
    let mut inner_projs = Vec::new();
    let mut outer_projs = Vec::new();
    for (i, p) in stmt.projections.iter().enumerate() {
        let Projection::Expr { expr, alias } = p else { continue };
        if contains_agg(expr) {
            let rewritten = replace_aggs(expr, &mut hidden);
            // an un-aliased bare aggregate keeps the name the executor would have given it
            let default_name = match expr {
                Expr::Agg { func, expr: inner } => {
                    let col = match inner.as_ref() { Expr::Col(c) => c.clone(), Expr::QualCol(t, c) => format!("{t}.{c}"), _ => String::new() };
                    format!("{func:?}({col})")
                }
                _ => format!("expr{i}"),
            };
            outer_projs.push(Projection::Expr { expr: rewritten, alias: Some(alias.clone().unwrap_or(default_name)) });
        } else {
            let name = alias.clone()
                .or_else(|| col_ref(expr).map(|c| c.rsplit('.').next().unwrap_or(&c).to_string()))
                .unwrap_or_else(|| format!("__k{i}"));
            inner_projs.push(Projection::Expr { expr: expr.clone(), alias: Some(name.clone()) });
            outer_projs.push(Projection::Expr { expr: Expr::Col(name.clone()), alias: Some(name) });
        }
    }

    // HAVING that mentions aggregates is evaluated on the outer statement over the hidden outputs
    let (inner_having, outer_where) = match &stmt.having {
        Some(h) if contains_agg(h) => (None, Some(replace_aggs(h, &mut hidden))),
        other => (other.clone(), None),
    };
    for (name, agg) in &hidden {
        inner_projs.push(Projection::Expr { expr: agg.clone(), alias: Some(name.clone()) });
    }

    let inner = SelectStmt {
        distinct: false,
        projections: inner_projs,
        from: stmt.from.clone(),
        joins: stmt.joins.clone(),
        where_clause: stmt.where_clause.clone(),
        group_by: stmt.group_by.clone(),
        having: inner_having,
        qualify: stmt.qualify.clone(),
        order_by: Vec::new(),
        limit: None,
        offset: None,
        scan_limit: None,
        lateral_views: stmt.lateral_views.clone(),
        pivot: stmt.pivot.clone(),
        unpivot: stmt.unpivot.clone(),
        hints: stmt.hints.clone(),
    };
    Some(SelectStmt {
        distinct: stmt.distinct,
        projections: outer_projs,
        from: TableExpr { name: "__lifted".into(), alias: Some("__lifted".into()), subquery: Some(Box::new(inner)), values: None, push_filter: None },
        joins: Vec::new(),
        where_clause: outer_where,
        group_by: Vec::new(),
        having: None,
        qualify: None,
        order_by: stmt.order_by.clone(),
        limit: stmt.limit,
        offset: stmt.offset,
        scan_limit: None,
        lateral_views: Vec::new(),
        pivot: None,
        unpivot: None,
        hints: stmt.hints.clone(),
    })
}
