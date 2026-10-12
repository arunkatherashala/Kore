//! `EXPLAIN [EXTENDED | FORMATTED | COST | CODEGEN] query`: a plan tree in Spark's text shape (`== Physical Plan ==`,
//! operators nested with `+- `). It is derived from the parsed statement, not from the executor's actual choices, so
//! it shows the logical shape (scan, filter, join, aggregate, window, sort, limit) rather than exchange / codegen
//! details.

use crate::ast::*;
use crate::ast_walk::expr_sql;

struct Node { text: String, children: Vec<Node> }

fn node(text: impl Into<String>, children: Vec<Node>) -> Node { Node { text: text.into(), children } }

fn render_child(n: &Node, indent: &str, branch: &str, out: &mut Vec<String>) {
    out.push(format!("{indent}{branch}{}", n.text));
    for (i, c) in n.children.iter().enumerate() {
        let b = if n.children.len() > 1 && i + 1 < n.children.len() { ":- " } else { "+- " };
        let cont = if branch == ":- " { ":  " } else { "   " };
        render_child(c, &format!("{indent}{cont}"), b, out);
    }
}

fn list(items: &[String]) -> String { items.join(", ") }

fn table_node(t: &TableExpr) -> Node {
    if let Some(sub) = &t.subquery {
        return node(format!("SubqueryAlias {}", t.alias.clone().unwrap_or_else(|| t.name.clone())), vec![select_node(sub)]);
    }
    if t.values.is_some() { return node(format!("LocalTableScan [{}]", list(&t.col_aliases)), vec![]); }
    node(format!("Scan {}", t.name), vec![])
}

fn order_text(o: &OrderByItem) -> String {
    let nulls = match o.nulls_first { Some(true) => " NULLS FIRST", Some(false) => " NULLS LAST", None => if o.desc { " NULLS LAST" } else { " NULLS FIRST" } };
    format!("{} {}{}", if matches!(o.expr, Expr::Int(_)) { o.col.clone() } else { expr_sql(&o.expr) }, if o.desc { "DESC" } else { "ASC" }, nulls)
}

fn proj_text(p: &Projection) -> String {
    match p {
        Projection::Star => "*".into(),
        Projection::Expr { expr, alias } => match alias {
            Some(a) => format!("{} AS {}", expr_sql(expr), a),
            None => expr_sql(expr),
        },
    }
}

fn select_node(s: &SelectStmt) -> Node {
    // bottom-up: scan -> joins -> filter -> aggregate -> window -> having -> project -> distinct -> sort -> limit
    let mut cur = table_node(&s.from);
    for j in &s.joins {
        let kind = match j.join_type {
            JoinKind::Inner | JoinKind::Implicit => "Inner", JoinKind::Left => "LeftOuter", JoinKind::Right => "RightOuter",
            JoinKind::Full => "FullOuter", JoinKind::Cross => "Cross", JoinKind::Semi => "LeftSemi", JoinKind::Anti => "LeftAnti",
        };
        let cond = match &j.on.expr {
            Some(e) => expr_sql(e),
            None if !j.on.left_col.is_empty() => format!("({} = {})", j.on.left_col, j.on.right_col),
            None if !j.using.is_empty() => format!("USING ({})", list(&j.using)),
            None => String::new(),
        };
        let name = if matches!(j.join_type, JoinKind::Cross) || cond.is_empty() { "CartesianProduct" } else { "SortMergeJoin" };
        cur = node(format!("{name} {kind}{}", if cond.is_empty() { String::new() } else { format!(", {cond}") }), vec![cur, table_node(&j.table)]);
    }
    for lv in &s.lateral_views {
        cur = node(format!("Generate {}, {}", expr_sql(&lv.expr), lv.col_alias), vec![cur]);
    }
    if let Some(w) = &s.where_clause { cur = node(format!("Filter {}", expr_sql(w)), vec![cur]); }
    let has_agg = s.projections.iter().any(|p| matches!(p, Projection::Expr { expr, .. } if crate::rewrite::contains_agg(expr)))
        || !s.group_by.is_empty() || !s.group_exprs.is_empty();
    if has_agg {
        let keys: Vec<String> = if !s.group_exprs.is_empty() { s.group_exprs.iter().map(expr_sql).collect() } else { s.group_by.clone() };
        let funcs: Vec<String> = s.projections.iter().filter_map(|p| match p {
            Projection::Expr { expr, .. } if crate::rewrite::contains_agg(expr) => Some(expr_sql(expr)),
            _ => None,
        }).collect();
        cur = node(format!("HashAggregate(keys=[{}], functions=[{}])", list(&keys), list(&funcs)), vec![cur]);
    }
    let windows: Vec<String> = s.projections.iter().filter_map(|p| match p {
        Projection::Expr { expr: Expr::Window { spec, .. }, alias } => Some(format!("{} windowspecdefinition({})", alias.clone().unwrap_or_else(|| "window_expr".into()),
            list(&spec.partition_by.iter().map(expr_sql).chain(spec.order_by.iter().map(order_text)).collect::<Vec<_>>()))),
        _ => None,
    }).collect();
    if !windows.is_empty() { cur = node(format!("Window [{}]", list(&windows)), vec![cur]); }
    if let Some(h) = &s.having { cur = node(format!("Filter {}", expr_sql(h)), vec![cur]); }
    if let Some(q) = &s.qualify { cur = node(format!("Filter {}", expr_sql(q)), vec![cur]); }
    let projs: Vec<String> = s.projections.iter().map(proj_text).collect();
    if !(s.projections.len() == 1 && matches!(s.projections[0], Projection::Star)) || has_agg {
        cur = node(format!("Project [{}]", list(&projs)), vec![cur]);
    }
    if s.distinct { cur = node(format!("HashAggregate(keys=[{}], functions=[])", list(&projs)), vec![cur]); }
    for (kind, arm) in &s.set_ops {
        let name = match kind {
            SetOpKind::UnionAll | SetOpKind::Union => "Union", SetOpKind::Intersect | SetOpKind::IntersectAll => "Intersect", _ => "Except",
        };
        let all = matches!(kind, SetOpKind::UnionAll | SetOpKind::IntersectAll | SetOpKind::ExceptAll);
        cur = node(format!("{name}{}", if all && name != "Union" { " All" } else { "" }), vec![cur, select_node(arm)]);
    }
    if !s.order_by.is_empty() {
        let o: Vec<String> = s.order_by.iter().map(order_text).collect();
        match s.limit {
            Some(n) => return node(format!("TakeOrderedAndProject(limit={n}, orderBy=[{}], output=[{}])", list(&o), list(&projs)), vec![cur]),
            None => cur = node(format!("Sort [{}], true", list(&o)), vec![cur]),
        }
    }
    if let Some(n) = s.limit { cur = node(format!("GlobalLimit {n}"), vec![cur]); }
    cur
}

fn query_node(q: &Query) -> Option<Node> {
    let body = q.body.as_ref()?;
    let mut n = select_node(body);
    for (kind, arm) in &q.set_ops {
        let name = match kind { SetOpKind::UnionAll | SetOpKind::Union => "Union", SetOpKind::Intersect | SetOpKind::IntersectAll => "Intersect", _ => "Except" };
        n = node(name, vec![n, select_node(arm)]);
    }
    if !q.order_by.is_empty() {
        n = node(format!("Sort [{}], true", list(&q.order_by.iter().map(order_text).collect::<Vec<_>>())), vec![n]);
    }
    if let Some(l) = q.limit { n = node(format!("GlobalLimit {l}"), vec![n]); }
    for cte in q.ctes.iter().rev() {
        n = node(format!("CTE {}", cte.name), vec![select_node(&cte.body), n]);
    }
    Some(n)
}

fn tree_text(n: &Node) -> Vec<String> {
    let mut out = vec![n.text.clone()];
    for (i, c) in n.children.iter().enumerate() {
        let b = if n.children.len() > 1 && i + 1 < n.children.len() { ":- " } else { "+- " };
        render_child(c, "", b, &mut out);
    }
    out
}

/// Text of `EXPLAIN [mode] <query>`; `mode` is the upper-case keyword (empty for plain EXPLAIN).
pub fn explain(mode: &str, q: &Query) -> Option<String> {
    let root = query_node(q)?;
    let lines = tree_text(&root);
    let physical = format!("== Physical Plan ==\n{}", lines.join("\n"));
    Some(match mode {
        "EXTENDED" | "COST" => {
            let logical = lines.join("\n");
            format!("== Parsed Logical Plan ==\n{logical}\n\n== Analyzed Logical Plan ==\n{logical}\n\n== Optimized Logical Plan ==\n{logical}\n\n{physical}")
        }
        _ => physical,
    })
}
