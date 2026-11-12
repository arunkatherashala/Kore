//! Static column binding: every column reference of a statement must resolve against the schema visible at that
//! point, otherwise the statement fails with `column not found: <name>` instead of evaluating the reference to NULL.
//!
//! One pass over the parsed statement per execution (microseconds, nothing per row). The checker never changes the
//! statement and never runs anything; it only answers "can this name possibly resolve". Whenever it cannot tell
//! (a table it has no schema for, a derived table with unnamed expression outputs, PIVOT output, a generator with
//! unnamed columns) the source is "open" and every name is accepted: a false error on a valid query is worse than
//! the hazard it removes.
//!
//! Scoping follows Spark: FROM/JOIN sources of the statement itself, then the enclosing statements (correlated
//! subqueries), SELECT aliases (accepted in every clause, which is more lenient than Spark), lambda parameters,
//! CTE names (earlier CTEs are visible to later ones), views (their own query is analysed for its output names).

use kore_core::KoreError;

use crate::ast::*;
use crate::executor::KqlContext;

/// Output / table column names; `None` = unknown (accept every name).
type Cols = Option<Vec<String>>;

struct Source { alias: String, real: String, cols: Cols }

#[derive(Default)]
struct Scope { sources: Vec<Source>, aliases: Vec<String>, open: bool }

struct Binder<'a> {
    ctx: &'a KqlContext,
    ctes: Vec<(String, Cols)>,
    stack: Vec<Scope>,
    lambdas: Vec<String>,
    depth: usize,
}

type R<T> = Result<T, KoreError>;

fn eq(a: &str, b: &str) -> bool { a.eq_ignore_ascii_case(b) }
fn bare(n: &str) -> &str { n.rsplit('.').next().unwrap_or(n) }
fn has_name(cols: &[String], c: &str) -> bool { cols.iter().any(|n| eq(n, c) || eq(bare(n), c)) }

/// Words that parse as identifiers but are values.
fn special_word(c: &str) -> bool {
    matches!(c.to_ascii_uppercase().as_str(),
        "CURRENT_TIME" | "CURRENT_DATABASE" | "CURRENT_CATALOG" | "CURRENT_SCHEMA" | "SESSION_USER" | "USER" | "LOCALTIME"
        | "CURRENT_DATE" | "CURRENT_TIMESTAMP" | "LOCALTIMESTAMP" | "CURRENT_USER" | "NULL" | "TRUE" | "FALSE")
}

/// Check one parsed statement against the catalog of `ctx`. `Err` names the first unresolvable column.
pub(crate) fn check_query(q: &Query, ctx: &KqlContext) -> R<()> {
    let mut b = Binder { ctx, ctes: Vec::new(), stack: Vec::new(), lambdas: Vec::new(), depth: 0 };
    b.query(q).map(|_| ())
}

impl<'a> Binder<'a> {
    fn query(&mut self, q: &Query) -> R<Cols> {
        for c in &q.ctes {
            if c.recursive { self.ctes.push((c.name.clone(), None)); }
            let out = self.stmt(&c.body)?;
            if c.recursive { if let Some(l) = self.ctes.last_mut() { l.1 = out; } } else { self.ctes.push((c.name.clone(), out)); }
        }
        let body = match &q.body { Some(b) => b, None => return Ok(None) };
        let out = self.stmt(body)?;
        for (_, arm) in &q.set_ops { self.stmt(arm)?; }
        // ORDER BY after a set operation sorts the combined result: plain names must be output columns
        if !q.set_ops.is_empty() {
            if let Some(cols) = &out {
                for o in &q.order_by {
                    if let Expr::Col(c) = &o.expr {
                        if !has_name(cols, c) && !c.starts_with("__") {
                            return Err(self.not_found(c, Some(cols)));
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    fn top(&mut self) -> &mut Scope { self.stack.last_mut().expect("scope") }

    fn stmt(&mut self, s: &SelectStmt) -> R<Cols> {
        self.stack.push(Scope::default());
        let r = self.stmt_inner(s);
        self.stack.pop();
        let out = r?;
        for (_, arm) in &s.set_ops { self.stmt(arm)?; }
        Ok(out)
    }

    fn stmt_inner(&mut self, s: &SelectStmt) -> R<Cols> {
        for t in std::iter::once(&s.from).chain(s.joins.iter().map(|j| &j.table)) {
            let src = self.source(t)?;
            self.top().sources.push(src);
        }
        if s.pivot.is_some() || s.unpivot.is_some() { self.top().open = true; }
        for lv in &s.lateral_views {
            let cols = if lv.col_alias.is_empty() { None } else { Some(lv.col_alias.split(',').map(|x| x.trim().to_string()).collect()) };
            self.top().sources.push(Source { alias: lv.table_alias.clone(), real: lv.table_alias.clone(), cols });
        }
        for p in &s.projections {
            if let Projection::Expr { alias: Some(a), .. } = p {
                for part in a.split(',') { self.top().aliases.push(part.trim().to_string()); }
            }
        }
        for lv in &s.lateral_views { self.expr(&lv.expr)?; }
        for p in &s.projections { if let Projection::Expr { expr, .. } = p { self.expr(expr)?; } }
        for j in &s.joins {
            if let Some(e) = &j.on.expr {
                self.expr(e)?;
            } else {
                for n in [&j.on.left_col, &j.on.right_col] { if !n.is_empty() { self.name(n)?; } }
            }
            for u in &j.using { self.check_using(u, &j.table)?; }
        }
        if let Some(w) = &s.where_clause { self.expr(w)?; }
        if s.group_exprs.is_empty() {
            for g in &s.group_by { if !g.chars().all(|c| c.is_ascii_digit()) { self.name(g)?; } }
        } else {
            for g in &s.group_exprs { self.expr(g)?; }
        }
        for e in [&s.having, &s.qualify].into_iter().flatten() { self.expr(e)?; }
        for (_, w) in &s.windows { self.spec(w)?; }
        for o in &s.order_by { self.expr(&o.expr)?; }
        Ok(self.outputs(s))
    }

    /// `JOIN .. USING (c)`: the column must exist on the right side and on the left.
    fn check_using(&mut self, c: &str, right: &TableExpr) -> R<()> {
        let ralias = right.alias.as_deref().unwrap_or(right.name.as_str()).to_string();
        let scope = self.stack.last().expect("scope");
        let mut left_known = true;
        let mut left_has = false;
        for s in &scope.sources {
            match &s.cols {
                None => left_known = false,
                Some(v) => {
                    if eq(&s.alias, &ralias) {
                        if !has_name(v, c) { return Err(self.not_found(c, Some(v))); }
                    } else if has_name(v, c) { left_has = true; }
                }
            }
        }
        if left_known && !left_has && !scope.open && scope.sources.len() > 1 { return Err(self.not_found(c, None)); }
        Ok(())
    }

    fn outputs(&self, s: &SelectStmt) -> Cols {
        let scope = self.stack.last()?;
        if scope.open { return None; }
        let mut out: Vec<String> = Vec::new();
        for p in &s.projections {
            match p {
                Projection::Star => {
                    for src in &scope.sources { out.extend(src.cols.as_ref()?.iter().cloned()); }
                }
                Projection::Expr { expr: Expr::Col(c), .. } if c == "*" => {
                    for src in &scope.sources { out.extend(src.cols.as_ref()?.iter().cloned()); }
                }
                Projection::Expr { expr: Expr::QualCol(q, c), .. } if c == "*" => {
                    let src = scope.sources.iter().find(|x| eq(&x.alias, q) || eq(&x.real, q))?;
                    out.extend(src.cols.as_ref()?.iter().cloned());
                }
                Projection::Expr { alias: Some(a), .. } => out.extend(a.split(',').map(|x| x.trim().to_string())),
                Projection::Expr { expr: Expr::Col(c) | Expr::QualCol(_, c), alias: None } => out.push(bare(c).to_string()),
                _ => return None,
            }
        }
        Some(out)
    }

    /// Columns of one FROM / JOIN item.
    fn source(&mut self, t: &TableExpr) -> R<Source> {
        let alias = t.alias.clone().unwrap_or_else(|| t.name.clone());
        let rename = |cols: Cols, ca: &[String]| -> Cols {
            let mut v = cols?;
            for (i, n) in ca.iter().enumerate() { if i < v.len() { v[i] = n.clone(); } }
            Some(v)
        };
        let cols: Cols = if let Some(rows) = &t.values {
            let n = rows.first().map_or(0, |r| r.len());
            if n == 0 { None } else {
                Some((0..n).map(|i| t.col_aliases.get(i).cloned().unwrap_or_else(|| format!("col{}", i + 1))).collect())
            }
        } else if let Some(sub) = &t.subquery {
            let out = self.stmt(sub)?;
            rename(out, &t.col_aliases)
        } else {
            self.table_cols(&t.name)
        };
        Ok(Source { alias, real: t.name.clone(), cols })
    }

    fn table_cols(&mut self, name: &str) -> Cols {
        if name == "__dual__" { return Some(vec!["__dual__".into()]); }
        if name.contains(".parquet") { return None; }
        if let Some((_, c)) = self.ctes.iter().rev().find(|(n, _)| eq(n, name)) { return c.clone(); }
        if let Some(b) = self.ctx.get(name) {
            if b.columns.is_empty() { return None; }
            return Some(b.columns.iter().map(|c| c.name.clone()).collect());
        }
        if let Some(sql) = self.ctx.view_sql(name) {
            if self.depth >= 6 { return None; }
            let q = crate::executor::parse_cached(sql.trim()).ok()?;
            let mut inner = Binder { ctx: self.ctx, ctes: Vec::new(), stack: Vec::new(), lambdas: Vec::new(), depth: self.depth + 1 };
            return inner.query(&q).ok().flatten();
        }
        None
    }

    // ── expressions ──────────────────────────────────────────────────────────

    fn spec(&mut self, w: &WindowSpec) -> R<()> {
        for p in &w.partition_by { self.expr(p)?; }
        for o in &w.order_by { self.expr(&o.expr)?; }
        if let Some(f) = &w.frame {
            for b in [&f.start, &f.end] {
                if let FrameBound::Preceding(x) | FrameBound::Following(x) = b { self.expr(x)?; }
            }
        }
        Ok(())
    }

    fn exprs(&mut self, v: &[Expr]) -> R<()> { for e in v { self.expr(e)?; } Ok(()) }

    fn expr(&mut self, e: &Expr) -> R<()> {
        match e {
            Expr::Col(c) => self.col(None, c),
            Expr::QualCol(q, c) => self.col(Some(q), c),
            Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Null | Expr::Star => Ok(()),
            Expr::BinOp { left, right, .. } => { self.expr(left)?; self.expr(right) }
            Expr::Not(x) | Expr::IsNull(x) | Expr::IsNotNull(x) | Expr::Explode(x) | Expr::Agg { expr: x, .. } => self.expr(x),
            Expr::AggX { args, filter, .. } => {
                self.exprs(args)?;
                if let Some(f) = filter { self.expr(f)?; }
                Ok(())
            }
            Expr::Window { func, spec } => {
                match func {
                    WindowFn::RowNumber | WindowFn::Rank | WindowFn::DenseRank | WindowFn::PercentRank | WindowFn::CumeDist => {}
                    WindowFn::Ntile(x) | WindowFn::FirstValue(x) | WindowFn::LastValue(x) | WindowFn::CumSum(x)
                    | WindowFn::FirstValueIgnoreNulls(x) | WindowFn::LastValueIgnoreNulls(x) | WindowFn::Agg { expr: x, .. } => self.expr(x)?,
                    WindowFn::Lag { expr, offset, default } | WindowFn::Lead { expr, offset, default } => {
                        self.expr(expr)?; self.expr(offset)?;
                        if let Some(d) = default { self.expr(d)?; }
                    }
                    WindowFn::NthValue { expr, n } => { self.expr(expr)?; self.expr(n)?; }
                    WindowFn::AggX { args, filter, .. } => {
                        self.exprs(args)?;
                        if let Some(f) = filter { self.expr(f)?; }
                    }
                }
                self.spec(spec)
            }
            Expr::Case { operand, branches, else_val } => {
                if let Some(o) = operand { self.expr(o)?; }
                for (c, v) in branches { self.expr(c)?; self.expr(v)?; }
                if let Some(x) = else_val { self.expr(x)?; }
                Ok(())
            }
            Expr::In { expr, values, .. } => { self.expr(expr)?; self.exprs(values) }
            Expr::Between { expr, low, high, .. } => { self.expr(expr)?; self.expr(low)?; self.expr(high) }
            Expr::Like { expr, pattern, .. } | Expr::ILike { expr, pattern, .. } => { self.expr(expr)?; self.expr(pattern) }
            Expr::Array(items) => self.exprs(items),
            Expr::FuncCall { name, args } => self.call(name, args),
            Expr::ScalarSubquery(s) | Expr::Exists { subquery: s, .. } => self.stmt(s).map(|_| ()),
            Expr::InSubquery { expr, subquery, .. } | Expr::QuantSubquery { expr, subquery, .. } => {
                self.expr(expr)?;
                self.stmt(subquery).map(|_| ())
            }
        }
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> R<()> {
        if name == "__LAMBDA" && !args.is_empty() {
            let n = self.lambdas.len();
            for a in &args[..args.len() - 1] { if let Expr::Str(p) = a { self.lambdas.push(p.clone()); } }
            let r = self.expr(&args[args.len() - 1]);
            self.lambdas.truncate(n);
            return r;
        }
        // the type of CAST(x AS type) is parsed as a bare word
        if matches!(name, "CAST" | "TRY_CAST" | "CONVERT") {
            return match args.first() { Some(a) => self.expr(a), None => Ok(()) };
        }
        for a in args {
            // a unit word (DATEADD(day, ..)) that is not a column
            if matches!(a, Expr::Col(c) if crate::datetime::norm_unit(c).is_some()) { let _ = self.expr(a); continue; }
            self.expr(a)?;
        }
        Ok(())
    }

    /// A name written as text (`a.k` / `k`): join keys and plain GROUP BY lists.
    fn name(&mut self, n: &str) -> R<()> {
        match n.split_once('.') {
            Some((q, c)) if !q.is_empty() && !c.is_empty() => self.col(Some(q), c),
            _ => self.col(None, n),
        }
    }

    /// Can `c` be found unqualified: a source column, an alias, a table name (whole-row struct), in any visible scope?
    fn resolves(&self, c: &str, whole_row: bool) -> bool {
        for scope in self.stack.iter().rev() {
            if scope.open { return true; }
            for s in &scope.sources {
                match &s.cols {
                    None => return true,
                    Some(v) => if has_name(v, c) || (whole_row && eq(&s.alias, c)) { return true; },
                }
            }
            if scope.aliases.iter().any(|a| eq(a, c)) { return true; }
        }
        false
    }

    fn col(&mut self, q: Option<&str>, c: &str) -> R<()> {
        if c == "*" {
            return match q {
                Some(q) if !self.qualifier_ok(q) => Err(self.not_found(&format!("{q}.*"), None)),
                _ => Ok(()),
            };
        }
        if c.starts_with("__") { return Ok(()); }
        match q {
            None => {
                if self.lambdas.iter().any(|p| eq(p, c)) || special_word(c) || self.resolves(c, true) { return Ok(()); }
                if let Some((q2, c2)) = c.split_once('.') {
                    if !q2.is_empty() && !c2.is_empty() { return self.col(Some(q2), c2); }
                }
                Err(self.not_found(c, None))
            }
            Some(q) => {
                if self.lambdas.iter().any(|p| eq(p, q)) || eq(q, "window") || q.starts_with("__") { return Ok(()); }
                // table alias first: the nearest scope that owns the qualifier decides
                for scope in self.stack.iter().rev() {
                    let owners: Vec<&Source> = scope.sources.iter().filter(|s| eq(&s.alias, q) || eq(&s.real, q)).collect();
                    if owners.is_empty() { continue; }
                    if scope.open || owners.iter().any(|s| s.cols.as_ref().map_or(true, |v| has_name(v, c))) { return Ok(()); }
                    break;
                }
                // `s.field` of a struct column `s`; or a column whose name itself contains the dot
                if self.resolves(q, false) { return Ok(()); }
                let full = format!("{q}.{c}");
                if self.stack.iter().any(|sc| sc.sources.iter().any(|s| s.cols.as_ref().map_or(false, |v| v.iter().any(|n| eq(n, &full))))) { return Ok(()); }
                let cand = self.stack.iter().rev().flat_map(|sc| sc.sources.iter())
                    .find(|s| eq(&s.alias, q) || eq(&s.real, q)).and_then(|s| s.cols.clone());
                Err(self.not_found(&full, cand.as_deref()))
            }
        }
    }

    fn qualifier_ok(&self, q: &str) -> bool {
        self.lambdas.iter().any(|p| eq(p, q)) || self.resolves(q, false)
            || self.stack.iter().any(|sc| sc.sources.iter().any(|s| eq(&s.alias, q) || eq(&s.real, q)))
    }

    /// Spark style: the name and the columns that could have been meant.
    fn not_found(&self, name: &str, cand: Option<&[String]>) -> KoreError {
        let mut list: Vec<String> = match cand {
            Some(c) => c.to_vec(),
            None => self.stack.last().map(|sc| sc.sources.iter().filter_map(|s| s.cols.as_ref().map(|v| (s, v)))
                .flat_map(|(s, v)| v.iter().map(move |c| format!("{}.{}", s.alias, bare(c)))).collect()).unwrap_or_default(),
        };
        list.truncate(12);
        if list.is_empty() {
            KoreError::InvalidArgument(format!("column not found: {name}"))
        } else {
            KoreError::InvalidArgument(format!("column not found: {name}; available columns: {}", list.join(", ")))
        }
    }
}
