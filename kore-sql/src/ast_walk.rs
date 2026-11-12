//! Generic traversal and rewriting of expression trees. `map_expr` rebuilds a tree bottom-up-or-stop:
//! the callback sees every node first and may replace it (the replacement is not descended into).

use crate::ast::*;

type Bx = Box<Expr>;

fn bx(e: &Expr, f: &mut dyn FnMut(&Expr) -> Option<Expr>) -> Bx { Box::new(map_expr(e, f)) }

fn map_spec(s: &WindowSpec, f: &mut dyn FnMut(&Expr) -> Option<Expr>) -> WindowSpec {
    WindowSpec {
        base: s.base.clone(),
        partition_by: s.partition_by.iter().map(|e| map_expr(e, f)).collect(),
        order_by: s.order_by.iter().map(|o| map_order(o, f)).collect(),
        frame: s.frame.as_ref().map(|fr| WindowFrame {
            mode: fr.mode.clone(),
            start: map_bound(&fr.start, f),
            end: map_bound(&fr.end, f),
        }),
    }
}

fn map_bound(b: &FrameBound, f: &mut dyn FnMut(&Expr) -> Option<Expr>) -> FrameBound {
    match b {
        FrameBound::Preceding(e) => FrameBound::Preceding(bx(e, f)),
        FrameBound::Following(e) => FrameBound::Following(bx(e, f)),
        other => other.clone(),
    }
}

pub fn map_order(o: &OrderByItem, f: &mut dyn FnMut(&Expr) -> Option<Expr>) -> OrderByItem {
    let expr = map_expr(&o.expr, f);
    // `col` is the plain-column spelling of `expr` that the fast sort uses; it must follow a rewrite
    let col = if expr == o.expr {
        o.col.clone()
    } else {
        match &expr { Expr::Col(c) => c.clone(), Expr::QualCol(t, c) => format!("{t}.{c}"), _ => String::new() }
    };
    OrderByItem { expr, col, desc: o.desc, nulls_first: o.nulls_first }
}

fn map_wfn(w: &WindowFn, f: &mut dyn FnMut(&Expr) -> Option<Expr>) -> WindowFn {
    match w {
        WindowFn::Ntile(x) => WindowFn::Ntile(bx(x, f)),
        WindowFn::FirstValue(x) => WindowFn::FirstValue(bx(x, f)),
        WindowFn::LastValue(x) => WindowFn::LastValue(bx(x, f)),
        WindowFn::FirstValueIgnoreNulls(x) => WindowFn::FirstValueIgnoreNulls(bx(x, f)),
        WindowFn::LastValueIgnoreNulls(x) => WindowFn::LastValueIgnoreNulls(bx(x, f)),
        WindowFn::CumSum(x) => WindowFn::CumSum(bx(x, f)),
        WindowFn::Lag { expr, offset, default } => WindowFn::Lag { expr: bx(expr, f), offset: bx(offset, f), default: default.as_ref().map(|d| bx(d, f)) },
        WindowFn::Lead { expr, offset, default } => WindowFn::Lead { expr: bx(expr, f), offset: bx(offset, f), default: default.as_ref().map(|d| bx(d, f)) },
        WindowFn::Agg { func, expr } => WindowFn::Agg { func: func.clone(), expr: bx(expr, f) },
        WindowFn::NthValue { expr, n } => WindowFn::NthValue { expr: bx(expr, f), n: bx(n, f) },
        WindowFn::AggX { name, args, filter } => WindowFn::AggX {
            name: name.clone(),
            args: args.iter().map(|a| map_expr(a, f)).collect(),
            filter: filter.as_ref().map(|x| bx(x, f)),
        },
        other => other.clone(),
    }
}

/// Rebuild `e`, letting `f` replace any node. Subquery bodies are left alone (`f` can still replace the node).
pub fn map_expr(e: &Expr, f: &mut dyn FnMut(&Expr) -> Option<Expr>) -> Expr {
    if let Some(r) = f(e) { return r; }
    match e {
        Expr::BinOp { op, left, right } => Expr::BinOp { op: op.clone(), left: bx(left, f), right: bx(right, f) },
        Expr::Not(x) => Expr::Not(bx(x, f)),
        Expr::IsNull(x) => Expr::IsNull(bx(x, f)),
        Expr::IsNotNull(x) => Expr::IsNotNull(bx(x, f)),
        Expr::Explode(x) => Expr::Explode(bx(x, f)),
        Expr::Agg { func, expr } => Expr::Agg { func: func.clone(), expr: bx(expr, f) },
        Expr::AggX { name, args, distinct, filter } => Expr::AggX {
            name: name.clone(),
            args: args.iter().map(|a| map_expr(a, f)).collect(),
            distinct: *distinct,
            filter: filter.as_ref().map(|x| bx(x, f)),
        },
        Expr::Window { func, spec } => Expr::Window { func: map_wfn(func, f), spec: map_spec(spec, f) },
        Expr::Case { operand, branches, else_val } => Expr::Case {
            operand: operand.as_ref().map(|o| bx(o, f)),
            branches: branches.iter().map(|(c, v)| (bx(c, f), bx(v, f))).collect(),
            else_val: else_val.as_ref().map(|x| bx(x, f)),
        },
        Expr::In { expr, values, negated } => Expr::In { expr: bx(expr, f), values: values.iter().map(|v| map_expr(v, f)).collect(), negated: *negated },
        Expr::Between { expr, low, high, negated } => Expr::Between { expr: bx(expr, f), low: bx(low, f), high: bx(high, f), negated: *negated },
        Expr::Like { expr, pattern, negated } => Expr::Like { expr: bx(expr, f), pattern: bx(pattern, f), negated: *negated },
        Expr::ILike { expr, pattern, negated } => Expr::ILike { expr: bx(expr, f), pattern: bx(pattern, f), negated: *negated },
        Expr::FuncCall { name, args } => Expr::FuncCall { name: name.clone(), args: args.iter().map(|a| map_expr(a, f)).collect() },
        Expr::Array(items) => Expr::Array(items.iter().map(|a| map_expr(a, f)).collect()),
        Expr::InSubquery { expr, subquery, negated } => Expr::InSubquery { expr: bx(expr, f), subquery: subquery.clone(), negated: *negated },
        Expr::QuantSubquery { expr, op, all, subquery } => Expr::QuantSubquery { expr: bx(expr, f), op: op.clone(), all: *all, subquery: subquery.clone() },
        Expr::Col(_) | Expr::QualCol(..) | Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Null | Expr::Star
        | Expr::ScalarSubquery(_) | Expr::Exists { .. } => e.clone(),
    }
}

/// Visit `e` and all of its descendants (pre-order). With `into_subqueries`, subquery statements are entered too.
pub fn walk_expr(e: &Expr, into_subqueries: bool, f: &mut dyn FnMut(&Expr)) {
    f(e);
    macro_rules! w { ($x:expr) => { walk_expr($x, into_subqueries, f) } }
    match e {
        Expr::BinOp { left, right, .. } => { w!(left); w!(right); }
        Expr::Not(x) | Expr::IsNull(x) | Expr::IsNotNull(x) | Expr::Explode(x) | Expr::Agg { expr: x, .. } => w!(x),
        Expr::AggX { args, filter, .. } => {
            for a in args { w!(a); }
            if let Some(x) = filter { w!(x); }
        }
        Expr::Window { func, spec } => {
            match func {
                WindowFn::Ntile(x) | WindowFn::FirstValue(x) | WindowFn::LastValue(x) | WindowFn::CumSum(x)
                | WindowFn::FirstValueIgnoreNulls(x) | WindowFn::LastValueIgnoreNulls(x) | WindowFn::Agg { expr: x, .. } => w!(x),
                WindowFn::Lag { expr, offset, default } | WindowFn::Lead { expr, offset, default } => {
                    w!(expr); w!(offset);
                    if let Some(d) = default { w!(d); }
                }
                WindowFn::NthValue { expr, n } => { w!(expr); w!(n); }
                WindowFn::AggX { args, filter, .. } => {
                    for a in args { w!(a); }
                    if let Some(x) = filter { w!(x); }
                }
                WindowFn::RowNumber | WindowFn::Rank | WindowFn::DenseRank | WindowFn::PercentRank | WindowFn::CumeDist => {}
            }
            for p in &spec.partition_by { w!(p); }
            for o in &spec.order_by { w!(&o.expr); }
            if let Some(fr) = &spec.frame {
                for b in [&fr.start, &fr.end] {
                    if let FrameBound::Preceding(x) | FrameBound::Following(x) = b { w!(x); }
                }
            }
        }
        Expr::Case { operand, branches, else_val } => {
            if let Some(o) = operand { w!(o); }
            for (c, v) in branches { w!(c); w!(v); }
            if let Some(x) = else_val { w!(x); }
        }
        Expr::In { expr, values, .. } => { w!(expr); for v in values { w!(v); } }
        Expr::Between { expr, low, high, .. } => { w!(expr); w!(low); w!(high); }
        Expr::Like { expr, pattern, .. } | Expr::ILike { expr, pattern, .. } => { w!(expr); w!(pattern); }
        Expr::FuncCall { args, .. } => { for a in args { w!(a); } }
        Expr::Array(items) => { for a in items { w!(a); } }
        Expr::ScalarSubquery(s) | Expr::Exists { subquery: s, .. } => if into_subqueries { walk_stmt(s, true, f); },
        Expr::InSubquery { expr, subquery, .. } | Expr::QuantSubquery { expr, subquery, .. } => {
            w!(expr);
            if into_subqueries { walk_stmt(subquery, true, f); }
        }
        Expr::Col(_) | Expr::QualCol(..) | Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Null | Expr::Star => {}
    }
}

/// Visit every expression of a statement (projections, joins, WHERE, GROUP BY, HAVING, QUALIFY, ORDER BY,
/// window definitions, FROM subqueries / VALUES, set-operation arms).
pub fn walk_stmt(s: &SelectStmt, into_subqueries: bool, f: &mut dyn FnMut(&Expr)) {
    for p in &s.projections { if let Projection::Expr { expr, .. } = p { walk_expr(expr, into_subqueries, f); } }
    walk_table(&s.from, into_subqueries, f);
    for j in &s.joins {
        walk_table(&j.table, into_subqueries, f);
        if let Some(e) = &j.on.expr { walk_expr(e, into_subqueries, f); }
    }
    for e in [&s.where_clause, &s.having, &s.qualify].into_iter().flatten() { walk_expr(e, into_subqueries, f); }
    for g in &s.group_exprs { walk_expr(g, into_subqueries, f); }
    for o in &s.order_by { walk_expr(&o.expr, into_subqueries, f); }
    for (_, w) in &s.windows {
        for p in &w.partition_by { walk_expr(p, into_subqueries, f); }
        for o in &w.order_by { walk_expr(&o.expr, into_subqueries, f); }
    }
    for (_, arm) in &s.set_ops { walk_stmt(arm, into_subqueries, f); }
}

fn walk_table(t: &TableExpr, into_subqueries: bool, f: &mut dyn FnMut(&Expr)) {
    if let Some(sub) = &t.subquery { walk_stmt(sub, into_subqueries, f); }
    if let Some(rows) = &t.values { for r in rows { for e in r { walk_expr(e, into_subqueries, f); } } }
}

/// Visit every expression of a whole query (CTE bodies, main body, set-operation arms).
pub fn walk_query(q: &Query, into_subqueries: bool, f: &mut dyn FnMut(&Expr)) {
    for c in &q.ctes { walk_stmt(&c.body, into_subqueries, f); }
    if let Some(b) = &q.body { walk_stmt(b, into_subqueries, f); }
    for (_, s) in &q.set_ops { walk_stmt(s, into_subqueries, f); }
    for o in &q.order_by { walk_expr(&o.expr, into_subqueries, f); }
}

/// True when `pred` holds for some node of `e`.
pub fn any_node(e: &Expr, pred: &dyn Fn(&Expr) -> bool) -> bool {
    let mut found = false;
    walk_expr(e, false, &mut |x| { if pred(x) { found = true; } });
    found
}

/// Expressions of a statement's own clauses mapped through `f` (FROM subqueries and set arms are not entered).
pub fn map_stmt_exprs(s: &SelectStmt, f: &mut dyn FnMut(&Expr) -> Option<Expr>) -> SelectStmt {
    let mut out = s.clone();
    out.projections = s.projections.iter().map(|p| match p {
        Projection::Expr { expr, alias } => Projection::Expr { expr: map_expr(expr, f), alias: alias.clone() },
        other => other.clone(),
    }).collect();
    out.where_clause = s.where_clause.as_ref().map(|e| map_expr(e, f));
    out.having = s.having.as_ref().map(|e| map_expr(e, f));
    out.qualify = s.qualify.as_ref().map(|e| map_expr(e, f));
    out.group_exprs = s.group_exprs.iter().map(|e| map_expr(e, f)).collect();
    out.order_by = s.order_by.iter().map(|o| map_order(o, f)).collect();
    out.windows = s.windows.iter().map(|(n, w)| (n.clone(), map_spec(w, f))).collect();
    for j in &mut out.joins {
        if let Some(e) = &j.on.expr { j.on.expr = Some(map_expr(e, f)); }
    }
    out
}

/// Visit the expressions of a statement's own clauses only: not FROM subqueries, not set-operation arms.
pub fn walk_own_exprs(s: &SelectStmt, f: &mut dyn FnMut(&Expr)) {
    for p in &s.projections { if let Projection::Expr { expr, .. } = p { walk_expr(expr, false, f); } }
    for j in &s.joins { if let Some(e) = &j.on.expr { walk_expr(e, false, f); } }
    for e in [&s.where_clause, &s.having, &s.qualify].into_iter().flatten() { walk_expr(e, false, f); }
    for g in &s.group_exprs { walk_expr(g, false, f); }
    for o in &s.order_by { walk_expr(&o.expr, false, f); }
    for (_, w) in &s.windows {
        for p in &w.partition_by { walk_expr(p, false, f); }
        for o in &w.order_by { walk_expr(&o.expr, false, f); }
    }
}

/// Spark-flavoured text of an expression, used to name unaliased result columns (`sum((v * 2))`).
pub fn expr_sql(e: &Expr) -> String {
    let list = |v: &[Expr]| v.iter().map(expr_sql).collect::<Vec<_>>().join(", ");
    match e {
        Expr::Col(c) | Expr::QualCol(_, c) => c.clone(),
        Expr::Int(i) => i.to_string(),
        Expr::Float(f) => crate::scalar::fmt_f64(*f),
        Expr::Str(s) => format!("'{s}'"),
        Expr::Bool(b) => b.to_string(),
        Expr::Null => "NULL".into(),
        Expr::Star => "*".into(),
        Expr::BinOp { op, left, right } => {
            let o = match op {
                BinOpKind::Eq => "=", BinOpKind::Ne => "<>", BinOpKind::Lt => "<", BinOpKind::Le => "<=",
                BinOpKind::Gt => ">", BinOpKind::Ge => ">=", BinOpKind::And => "AND", BinOpKind::Or => "OR",
                BinOpKind::Add => "+", BinOpKind::Sub => "-", BinOpKind::Mul => "*", BinOpKind::Div => "/",
                BinOpKind::Mod => "%", BinOpKind::Concat => "||",
            };
            format!("({} {} {})", expr_sql(left), o, expr_sql(right))
        }
        Expr::Not(x) => format!("(NOT {})", expr_sql(x)),
        Expr::IsNull(x) => format!("({} IS NULL)", expr_sql(x)),
        Expr::IsNotNull(x) => format!("({} IS NOT NULL)", expr_sql(x)),
        Expr::Agg { func, expr } => {
            let inner = match expr.as_ref() { Expr::Col(c) if c == "*" => "1".to_string(), other => expr_sql(other) };
            match func {
                AggFunc::CountDistinct => format!("count(DISTINCT {inner})"),
                other => {
                    let n = format!("{:?}", other);
                    let n = n.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("agg").to_ascii_lowercase();
                    format!("{n}({inner})")
                }
            }
        }
        Expr::AggX { name, args, distinct, .. } => {
            let a: Vec<String> = args.iter().map(|x| match x { Expr::Col(c) if c == "*" => "1".to_string(), o => expr_sql(o) }).collect();
            format!("{}({}{})", name.to_ascii_lowercase(), if *distinct { "DISTINCT " } else { "" }, a.join(", "))
        }
        Expr::FuncCall { name, args } => format!("{}({})", name.to_ascii_lowercase(), list(args)),
        Expr::Case { operand, branches, else_val } => {
            let mut t = String::from("CASE");
            if let Some(o) = operand { t.push_str(&format!(" {}", expr_sql(o))); }
            for (c, v) in branches { t.push_str(&format!(" WHEN {} THEN {}", expr_sql(c), expr_sql(v))); }
            if let Some(x) = else_val { t.push_str(&format!(" ELSE {}", expr_sql(x))); }
            t.push_str(" END");
            t
        }
        Expr::In { expr, values, negated } => format!("({} {}IN ({}))", expr_sql(expr), if *negated { "NOT " } else { "" }, list(values)),
        Expr::Between { expr, low, high, negated } => format!("({} {}BETWEEN {} AND {})", expr_sql(expr), if *negated { "NOT " } else { "" }, expr_sql(low), expr_sql(high)),
        Expr::Like { expr, pattern, negated } => format!("({} {}LIKE {})", expr_sql(expr), if *negated { "NOT " } else { "" }, expr_sql(pattern)),
        Expr::ILike { expr, pattern, negated } => format!("({} {}ILIKE {})", expr_sql(expr), if *negated { "NOT " } else { "" }, expr_sql(pattern)),
        Expr::Window { .. } => "window_expr".into(),
        Expr::Array(items) => format!("array({})", list(items)),
        Expr::Explode(x) => format!("explode({})", expr_sql(x)),
        Expr::ScalarSubquery(_) | Expr::InSubquery { .. } | Expr::Exists { .. } | Expr::QuantSubquery { .. } => "subquery".into(),
    }
}
