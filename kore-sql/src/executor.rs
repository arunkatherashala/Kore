//! KQL executor — runs a `SelectStmt` against named `DataBlock` tables.

use std::collections::HashMap;
use kore_core::{Column, ColumnData, DataBlock, KoreError, Value};
use kore_join::{HashJoin, JoinConfig};
use kore_core::JoinType;
use kore_window::{WindowFn as WinFn, WinOrder, apply_window};
use crate::ast::*;

/// Registry of named tables.
#[derive(Default, Clone)]
pub struct KqlContext {
    tables: HashMap<String, DataBlock>,
}

impl KqlContext {
    pub fn new() -> Self { Self::default() }

    /// Register a named table (replaces if already registered).
    pub fn register(&mut self, name: impl Into<String>, block: DataBlock) {
        self.tables.insert(name.into(), block);
    }

    /// Parse + execute a KQL query (supports CTEs and UNION ALL).
    pub fn query(&self, sql: &str) -> Result<DataBlock, KoreError> {
        let query = crate::parser::parse_query(sql)?;
        execute_query(&query, self)
    }

    pub fn get(&self, name: &str) -> Option<&DataBlock> {
        self.tables.get(name)
    }

    pub fn table_names(&self) -> Vec<String> {
        self.tables.keys().cloned().collect()
    }
}

pub fn execute(sql: &str, ctx: &KqlContext) -> Result<DataBlock, KoreError> {
    ctx.query(sql)
}

/// Execute a full Query (with CTEs and UNION ALL).
pub fn execute_query(query: &Query, ctx: &KqlContext) -> Result<DataBlock, KoreError> {
    // 1. Register CTEs in an extended context
    let mut local = ctx.clone();
    for cte in &query.ctes {
        let result = execute_select(&cte.body, &local)?;
        local.register(cte.name.clone(), result);
    }

    // 2. Execute main body
    let body = query.body.as_ref()
        .ok_or_else(|| KoreError::InvalidArgument("empty query body".into()))?;
    let mut result = execute_select(body, &local)?;

    // 3. UNION ALL
    for stmt in &query.union_all {
        let other = execute_select(stmt, &local)?;
        result = DataBlock::concat(vec![result, other])?;
    }

    Ok(result)
}

pub fn execute_select(stmt: &SelectStmt, ctx: &KqlContext) -> Result<DataBlock, KoreError> {
    // 1. Resolve FROM table
    let base_name   = &stmt.from.name;
    let base_alias  = stmt.from.alias.as_deref().unwrap_or(base_name.as_str());
    let base_block  = ctx.get(base_name)
        .ok_or_else(|| KoreError::InvalidArgument(format!("unknown table: {base_name}")))?
        .clone();

    // Prefix column names with alias
    let mut result = prefix_columns(base_block, base_alias);

    // 2. Process JOINs
    for join in &stmt.joins {
        let right_name  = &join.table.name;
        let right_alias = join.table.alias.as_deref().unwrap_or(right_name.as_str());
        let right_block = ctx.get(right_name)
            .ok_or_else(|| KoreError::InvalidArgument(format!("unknown table: {right_name}")))?
            .clone();
        let right_block = prefix_columns(right_block, right_alias);

        let jtype = match join.join_type {
            JoinKind::Inner => JoinType::Inner,
            JoinKind::Left  => JoinType::Left,
            JoinKind::Right => JoinType::Left,   // swap tables for right join
            JoinKind::Full  => JoinType::Full,
        };

        // Resolve join keys (may be qualified "alias.col" or bare "col")
        let lk = resolve_col_name(&join.on.left_col,  base_alias);
        let rk = resolve_col_name(&join.on.right_col, right_alias);

        let cfg = JoinConfig { left_key: lk.clone(), right_key: rk, join_type: jtype };

        if join.join_type == JoinKind::Right {
            result = HashJoin::join(&right_block, &result, &cfg)?;
        } else {
            result = HashJoin::join(&result, &right_block, &cfg)?;
        }
    }

    // 3. WHERE filter
    if let Some(pred) = &stmt.where_clause {
        result = filter_block(result, pred)?;
    }

    // 4. GROUP BY  (or global aggregation if no GROUP BY but has aggregates)
    let has_agg = stmt.projections.iter().any(|p| matches!(p, Projection::Expr { expr: Expr::Agg { .. }, .. }));
    if !stmt.group_by.is_empty() {
        result = group_by_agg(result, &stmt.group_by, &stmt.projections)?;
    } else if has_agg {
        result = global_agg(result, &stmt.projections)?;
    }

    // 4.1 HAVING — filter on aggregated result
    if let Some(having) = &stmt.having {
        result = filter_block(result, having)?;
    }

    // 4.5 Window functions — applied AFTER WHERE/GROUP BY, BEFORE ORDER BY
    let win_projs: Vec<(usize, &Expr, Option<&String>)> = stmt.projections.iter()
        .enumerate()
        .filter_map(|(i, p)| match p {
            Projection::Expr { expr: e @ Expr::Window { .. }, alias } => Some((i, e, alias.as_ref())),
            _ => None,
        })
        .collect();

    if !win_projs.is_empty() {
        for (idx, expr, alias) in &win_projs {
            if let Expr::Window { func, spec } = expr {
                let out_name = alias.map(|a| a.as_str())
                    .unwrap_or("__win")
                    .to_string();
                let win_fn   = ast_to_win_fn(func);
                let part_by  = spec.partition_by.iter()
                    .filter_map(|e| match e { Expr::Col(n) | Expr::QualCol(_, n) => Some(n.clone()), _ => None })
                    .collect::<Vec<_>>();
                let order_by = spec.order_by.iter()
                    .map(|o| WinOrder { col: o.col.clone(), desc: o.desc })
                    .collect::<Vec<_>>();
                result = apply_window(&result, &part_by, &order_by, &win_fn, &out_name)?;
            }
        }
    }

    // 5. ORDER BY
    for item in stmt.order_by.iter().rev() {
        let col = resolve_col_name(&item.col, "");
        result = sort_block(result, &col, item.desc)?;
    }

    // 6. LIMIT
    if let Some(n) = stmt.limit {
        result = limit_block(result, n as usize);
    }

    // 7. Projection
    result = project(result, &stmt.projections)?;

    Ok(result)
}

// ─── Column prefix helper ─────────────────────────────────────────────────────

fn prefix_columns(mut block: DataBlock, alias: &str) -> DataBlock {
    for col in &mut block.columns {
        if !col.name.contains('.') {
            col.name = format!("{}.{}", alias, col.name);
        }
    }
    block
}

fn resolve_col_name(name: &str, default_alias: &str) -> String {
    if name.contains('.') {
        name.to_string()
    } else if default_alias.is_empty() {
        name.to_string()
    } else {
        format!("{}.{}", default_alias, name)
    }
}

// ─── Filter (WHERE) ───────────────────────────────────────────────────────────

fn filter_block(block: DataBlock, pred: &Expr) -> Result<DataBlock, KoreError> {
    let n = block.num_rows;
    let keep: Vec<bool> = (0..n).map(|row| eval_bool(pred, &block, row)).collect();
    let indices: Vec<usize> = keep.iter().enumerate()
        .filter(|(_, &k)| k).map(|(i, _)| i).collect();
    Ok(block.select_rows(&indices))
}

fn eval_bool(expr: &Expr, block: &DataBlock, row: usize) -> bool {
    match eval_expr(expr, block, row) {
        ExprVal::Bool(b) => b,
        _                => false,
    }
}

#[derive(Debug, Clone)]
enum ExprVal {
    Int(i64), Float(f64), Str(String), Bool(bool), Null,
}

fn eval_expr(expr: &Expr, block: &DataBlock, row: usize) -> ExprVal {
    match expr {
        Expr::Int(n)   => ExprVal::Int(*n),
        Expr::Float(f) => ExprVal::Float(*f),
        Expr::Str(s)   => ExprVal::Str(s.clone()),
        Expr::Bool(b)  => ExprVal::Bool(*b),
        Expr::Not(e)   => match eval_expr(e, block, row) {
            ExprVal::Bool(b) => ExprVal::Bool(!b),
            _                => ExprVal::Bool(false),
        },
        Expr::Col(_) | Expr::QualCol(_, _) => {
            let full = match expr {
                Expr::QualCol(t, c) => format!("{}.{}", t, c),
                Expr::Col(n)        => n.clone(),
                _                   => unreachable!(),
            };
            get_cell(block, &full, row)
        }
        Expr::BinOp { op, left, right } => {
            let lv = eval_expr(left,  block, row);
            let rv = eval_expr(right, block, row);
            eval_binop(op, lv, rv)
        }
        Expr::IsNull(e) => match eval_expr(e, block, row) {
            ExprVal::Null => ExprVal::Bool(true),
            _             => ExprVal::Bool(false),
        },
        Expr::IsNotNull(e) => match eval_expr(e, block, row) {
            ExprVal::Null => ExprVal::Bool(false),
            _             => ExprVal::Bool(true),
        },
        Expr::Agg { .. }    => ExprVal::Null,
        Expr::Window { .. } => ExprVal::Null,
        Expr::Star          => ExprVal::Null,
        Expr::Null          => ExprVal::Null,
        // ── CASE WHEN ─────────────────────────────────────────────────────
        Expr::Case { operand, branches, else_val } => {
            match operand {
                None => {
                    // Searched: CASE WHEN cond THEN val ...
                    for (cond, val) in branches {
                        if eval_bool(cond, block, row) {
                            return eval_expr(val, block, row);
                        }
                    }
                }
                Some(op_expr) => {
                    // Simple: CASE expr WHEN literal THEN val ...
                    let lhs = eval_expr(op_expr, block, row);
                    for (cond, val) in branches {
                        let rhs = eval_expr(cond, block, row);
                        let eq = match (&lhs, &rhs) {
                            (ExprVal::Int(a),   ExprVal::Int(b))   => a == b,
                            (ExprVal::Float(a), ExprVal::Float(b)) => (a-b).abs() < 1e-10,
                            (ExprVal::Str(a),   ExprVal::Str(b))   => a == b,
                            (ExprVal::Bool(a),  ExprVal::Bool(b))  => a == b,
                            _ => false,
                        };
                        if eq { return eval_expr(val, block, row); }
                    }
                }
            }
            else_val.as_ref().map(|e| eval_expr(e, block, row)).unwrap_or(ExprVal::Null)
        }
        // ── LIKE ──────────────────────────────────────────────────────────
        Expr::Like { expr: e, pattern, negated } => {
            let sv = eval_expr(e, block, row);
            let pv = eval_expr(pattern, block, row);
            let matches = match (sv, pv) {
                (ExprVal::Str(s), ExprVal::Str(p)) => like_match(&s, &p),
                _ => false,
            };
            ExprVal::Bool(if *negated { !matches } else { matches })
        }
        // ── IN ────────────────────────────────────────────────────────────
        Expr::In { expr: e, values, negated } => {
            let lv = eval_expr(e, block, row);
            let found = values.iter().any(|v| {
                let rv = eval_expr(v, block, row);
                match (&lv, &rv) {
                    (ExprVal::Int(a),   ExprVal::Int(b))   => a == b,
                    (ExprVal::Float(a), ExprVal::Float(b)) => (a-b).abs() < 1e-10,
                    (ExprVal::Str(a),   ExprVal::Str(b))   => a == b,
                    (ExprVal::Bool(a),  ExprVal::Bool(b))  => a == b,
                    _ => false,
                }
            });
            ExprVal::Bool(if *negated { !found } else { found })
        }
        // ── BETWEEN ───────────────────────────────────────────────────────
        Expr::Between { expr: e, low, high, negated } => {
            let v  = eval_expr(e, block, row);
            let lo = eval_expr(low, block, row);
            let hi = eval_expr(high, block, row);
            let in_range = match (&v, &lo, &hi) {
                (ExprVal::Int(v),   ExprVal::Int(lo),   ExprVal::Int(hi))   => v >= lo && v <= hi,
                (ExprVal::Float(v), ExprVal::Float(lo), ExprVal::Float(hi)) => v >= lo && v <= hi,
                (ExprVal::Str(v),   ExprVal::Str(lo),   ExprVal::Str(hi))   => v.as_str() >= lo.as_str() && v.as_str() <= hi.as_str(),
                _ => false,
            };
            ExprVal::Bool(if *negated { !in_range } else { in_range })
        }
        // ── SCALAR FUNCTIONS ──────────────────────────────────────────────
        Expr::FuncCall { name, args } => eval_func(name, args, block, row),
    }
}

// ─── Scalar function evaluation ───────────────────────────────────────────────

fn eval_func(name: &str, args: &[Expr], block: &DataBlock, row: usize) -> ExprVal {
    // Helper macro: unwrap Option or return Null
    macro_rules! need {
        ($e:expr) => { match $e { Some(v) => v, None => return ExprVal::Null } };
    }
    let arg = |i: usize| args.get(i).map(|e| eval_expr(e, block, row)).unwrap_or(ExprVal::Null);
    let arg_str = |i: usize| match arg(i) { ExprVal::Str(s) => Some(s), _ => None };
    let arg_f64 = |i: usize| to_f64(&arg(i));

    match name {
        // ── String functions ────────────────────────────────────────────────
        "UPPER" => arg_str(0).map(|s| ExprVal::Str(s.to_uppercase())).unwrap_or(ExprVal::Null),
        "LOWER" => arg_str(0).map(|s| ExprVal::Str(s.to_lowercase())).unwrap_or(ExprVal::Null),
        "TRIM"  => arg_str(0).map(|s| ExprVal::Str(s.trim().to_string())).unwrap_or(ExprVal::Null),
        "LTRIM" => arg_str(0).map(|s| ExprVal::Str(s.trim_start().to_string())).unwrap_or(ExprVal::Null),
        "RTRIM" => arg_str(0).map(|s| ExprVal::Str(s.trim_end().to_string())).unwrap_or(ExprVal::Null),
        "LENGTH" | "LEN" | "CHAR_LENGTH" => {
            arg_str(0).map(|s| ExprVal::Int(s.chars().count() as i64)).unwrap_or(ExprVal::Null)
        }
        "REVERSE" => arg_str(0).map(|s| ExprVal::Str(s.chars().rev().collect())).unwrap_or(ExprVal::Null),
        "SUBSTR" | "SUBSTRING" => {
            let s = need!(arg_str(0));
            let start = (arg_f64(1).unwrap_or(1.0) as i64 - 1).max(0) as usize;
            let len   = args.get(2).map(|_| arg_f64(2).unwrap_or(0.0) as usize);
            let chars: Vec<char> = s.chars().collect();
            let slice: String = match len {
                Some(l) => chars.iter().skip(start).take(l).collect(),
                None    => chars.iter().skip(start).collect(),
            };
            ExprVal::Str(slice)
        }
        "REPLACE" => {
            let s    = need!(arg_str(0));
            let from = arg_str(1).unwrap_or_default();
            let to   = arg_str(2).unwrap_or_default();
            ExprVal::Str(s.replace(&from, &to))
        }
        "CONCAT" => {
            let parts: String = args.iter()
                .map(|a| match eval_expr(a, block, row) { ExprVal::Str(s) => s, v => format!("{:?}", v) })
                .collect();
            ExprVal::Str(parts)
        }
        "REPEAT" => {
            let s = arg_str(0).unwrap_or_default();
            let n = arg_f64(1).unwrap_or(0.0) as usize;
            ExprVal::Str(s.repeat(n))
        }
        "LPAD" => {
            let s   = arg_str(0).unwrap_or_default();
            let len = arg_f64(1).unwrap_or(0.0) as usize;
            let pad = arg_str(2).unwrap_or_else(|| " ".into());
            if s.len() >= len { return ExprVal::Str(s); }
            let fill: String = pad.chars().cycle().take(len - s.len()).collect();
            ExprVal::Str(format!("{fill}{s}"))
        }
        "RPAD" => {
            let s   = arg_str(0).unwrap_or_default();
            let len = arg_f64(1).unwrap_or(0.0) as usize;
            let pad = arg_str(2).unwrap_or_else(|| " ".into());
            if s.len() >= len { return ExprVal::Str(s); }
            let fill: String = pad.chars().cycle().take(len - s.len()).collect();
            ExprVal::Str(format!("{s}{fill}"))
        }
        // ── Math functions ──────────────────────────────────────────────────
        "ABS"   => match arg(0) {
            ExprVal::Int(i)   => ExprVal::Int(i.abs()),
            ExprVal::Float(f) => ExprVal::Float(f.abs()),
            _ => ExprVal::Null,
        },
        "ROUND" => {
            let f = need!(arg_f64(0));
            let dp = arg_f64(1).unwrap_or(0.0) as u32;
            let m  = 10f64.powi(dp as i32);
            ExprVal::Float((f * m).round() / m)
        }
        "FLOOR" => arg_f64(0).map(|f| ExprVal::Float(f.floor())).unwrap_or(ExprVal::Null),
        "CEIL" | "CEILING" => arg_f64(0).map(|f| ExprVal::Float(f.ceil())).unwrap_or(ExprVal::Null),
        "SQRT"  => arg_f64(0).map(|f| ExprVal::Float(f.sqrt())).unwrap_or(ExprVal::Null),
        "POWER" | "POW" => {
            let b = need!(arg_f64(0));
            let e = need!(arg_f64(1));
            ExprVal::Float(b.powf(e))
        }
        "LOG"   => arg_f64(0).map(|f| ExprVal::Float(f.ln())).unwrap_or(ExprVal::Null),
        "LOG10" => arg_f64(0).map(|f| ExprVal::Float(f.log10())).unwrap_or(ExprVal::Null),
        "EXP"   => arg_f64(0).map(|f| ExprVal::Float(f.exp())).unwrap_or(ExprVal::Null),
        "MOD"   => {
            let a = need!(arg_f64(0));
            let b = need!(arg_f64(1));
            ExprVal::Float(a % b)
        }
        // ── Null-handling ───────────────────────────────────────────────────
        "COALESCE" | "NVL" | "IFNULL" | "ISNULL" => {
            for a in args {
                let v = eval_expr(a, block, row);
                if !matches!(v, ExprVal::Null) { return v; }
            }
            ExprVal::Null
        }
        "NULLIF" => {
            let a = arg(0);
            let b = arg(1);
            let eq = match (&a, &b) {
                (ExprVal::Int(x),   ExprVal::Int(y))   => x == y,
                (ExprVal::Float(x), ExprVal::Float(y)) => (x - y).abs() < 1e-10,
                (ExprVal::Str(x),   ExprVal::Str(y))   => x == y,
                (ExprVal::Bool(x),  ExprVal::Bool(y))  => x == y,
                _ => false,
            };
            if eq { ExprVal::Null } else { a }
        }
        // ── Cast ────────────────────────────────────────────────────────────
        "CAST" => {
            let val = arg(0);
            // arg(1) is the type keyword parsed as a Col
            let ty  = match args.get(1) {
                Some(Expr::Col(t)) => t.to_ascii_uppercase(),
                _                  => return val,
            };
            match ty.as_str() {
                "INT" | "INTEGER" | "BIGINT" => match val {
                    ExprVal::Float(f) => ExprVal::Int(f as i64),
                    ExprVal::Str(s)   => s.trim().parse::<i64>().map(ExprVal::Int).unwrap_or(ExprVal::Null),
                    other             => other,
                },
                "FLOAT" | "DOUBLE" | "REAL" | "NUMERIC" | "DECIMAL" => match val {
                    ExprVal::Int(i)   => ExprVal::Float(i as f64),
                    ExprVal::Str(s)   => s.trim().parse::<f64>().map(ExprVal::Float).unwrap_or(ExprVal::Null),
                    other             => other,
                },
                "VARCHAR" | "TEXT" | "STRING" | "CHAR" => ExprVal::Str(match val {
                    ExprVal::Int(i)   => i.to_string(),
                    ExprVal::Float(f) => f.to_string(),
                    ExprVal::Bool(b)  => b.to_string(),
                    ExprVal::Str(s)   => s,
                    ExprVal::Null     => return ExprVal::Null,
                }),
                "BOOLEAN" | "BOOL" => ExprVal::Bool(match val {
                    ExprVal::Int(i)   => i != 0,
                    ExprVal::Float(f) => f != 0.0,
                    ExprVal::Str(s)   => matches!(s.to_lowercase().as_str(), "true" | "1" | "yes"),
                    ExprVal::Bool(b)  => b,
                    ExprVal::Null     => return ExprVal::Null,
                }),
                _ => val,
            }
        }
        // ── Type-check predicates ────────────────────────────────────────────
        "ISNUMERIC" => ExprVal::Bool(to_f64(&arg(0)).is_some()),
        "IIF" => {
            if eval_bool(&args[0], block, row) { arg(1) } else { arg(2) }
        }
        // ── Fallthrough ─────────────────────────────────────────────────────
        _ => ExprVal::Null,
    }
}

fn get_cell(block: &DataBlock, col_name: &str, row: usize) -> ExprVal {    // Try exact match, then suffix match (for qualified names)
    let col = block.columns.iter().find(|c| c.name == col_name)
        .or_else(|| block.columns.iter().find(|c| c.name.ends_with(&format!(".{}", col_name))));
    match col {
        None => ExprVal::Null,
        Some(c) => match &c.data {
            ColumnData::Int64(v)   => v.get(row).and_then(|x| x.as_ref()).map(|&i| ExprVal::Int(i)).unwrap_or(ExprVal::Null),
            ColumnData::Float64(v) => v.get(row).and_then(|x| x.as_ref()).map(|&f| ExprVal::Float(f)).unwrap_or(ExprVal::Null),
            ColumnData::Bool(v)    => v.get(row).and_then(|x| x.as_ref()).map(|&b| ExprVal::Bool(b)).unwrap_or(ExprVal::Null),
            ColumnData::Str(v)     => v.get(row).and_then(|x| x.as_ref()).map(|s| ExprVal::Str(s.clone())).unwrap_or(ExprVal::Null),
        }
    }
}

fn eval_binop(op: &BinOpKind, l: ExprVal, r: ExprVal) -> ExprVal {
    // Boolean short-circuits
    if let (BinOpKind::And, ExprVal::Bool(lb), ExprVal::Bool(rb)) = (op, &l, &r) {
        return ExprVal::Bool(*lb && *rb);
    }
    if let (BinOpKind::Or, ExprVal::Bool(lb), ExprVal::Bool(rb)) = (op, &l, &r) {
        return ExprVal::Bool(*lb || *rb);
    }

    // Numeric comparison / arithmetic
    let lf = to_f64(&l);
    let rf = to_f64(&r);

    if let (Some(lv), Some(rv)) = (lf, rf) {
        return match op {
            BinOpKind::Eq  => ExprVal::Bool((lv - rv).abs() < 1e-10),
            BinOpKind::Ne  => ExprVal::Bool((lv - rv).abs() >= 1e-10),
            BinOpKind::Lt  => ExprVal::Bool(lv < rv),
            BinOpKind::Le  => ExprVal::Bool(lv <= rv),
            BinOpKind::Gt  => ExprVal::Bool(lv > rv),
            BinOpKind::Ge  => ExprVal::Bool(lv >= rv),
            BinOpKind::Add => ExprVal::Float(lv + rv),
            BinOpKind::Sub => ExprVal::Float(lv - rv),
            BinOpKind::Mul => ExprVal::Float(lv * rv),
            BinOpKind::Div => ExprVal::Float(lv / rv),
            BinOpKind::Mod => ExprVal::Float(lv % rv),
            _ => ExprVal::Bool(false),
        };
    }

    // String comparison
    if let (ExprVal::Str(ls), ExprVal::Str(rs)) = (&l, &r) {
        return match op {
            BinOpKind::Eq => ExprVal::Bool(ls == rs),
            BinOpKind::Ne => ExprVal::Bool(ls != rs),
            BinOpKind::Lt => ExprVal::Bool(ls < rs),
            BinOpKind::Le => ExprVal::Bool(ls <= rs),
            BinOpKind::Gt => ExprVal::Bool(ls > rs),
            BinOpKind::Ge => ExprVal::Bool(ls >= rs),
            _ => ExprVal::Null,
        };
    }

    ExprVal::Null
}

fn to_f64(v: &ExprVal) -> Option<f64> {
    match v {
        ExprVal::Int(i)   => Some(*i as f64),
        ExprVal::Float(f) => Some(*f),
        ExprVal::Bool(b)  => Some(if *b { 1.0 } else { 0.0 }),
        _                 => None,
    }
}

// ─── Sort ─────────────────────────────────────────────────────────────────────

fn sort_block(block: DataBlock, col: &str, desc: bool) -> Result<DataBlock, KoreError> {
    // Find the column (exact or suffix match)
    let col_name = block.columns.iter()
        .find(|c| c.name == col || c.name.ends_with(&format!(".{}", col)))
        .map(|c| c.name.clone())
        .ok_or_else(|| KoreError::InvalidArgument(format!("ORDER BY column not found: {col}")))?;

    let resolved = col_name.as_str();
    let n = block.num_rows;
    let mut indices: Vec<usize> = (0..n).collect();
    indices.sort_by(|&a, &b| {
        let av = get_cell(&block, resolved, a);
        let bv = get_cell(&block, resolved, b);
        let ord = match (&av, &bv) {
            (ExprVal::Int(x),   ExprVal::Int(y))   => x.cmp(y),
            (ExprVal::Float(x), ExprVal::Float(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
            (ExprVal::Str(x),   ExprVal::Str(y))   => x.cmp(y),
            _ => std::cmp::Ordering::Equal,
        };
        if desc { ord.reverse() } else { ord }
    });
    Ok(block.select_rows(&indices))
}

// ─── Limit ────────────────────────────────────────────────────────────────────

fn limit_block(block: DataBlock, n: usize) -> DataBlock {
    let take = n.min(block.num_rows);
    let indices: Vec<usize> = (0..take).collect();
    block.select_rows(&indices)
}

fn project(block: DataBlock, projections: &[Projection]) -> Result<DataBlock, KoreError> {
    // Star = keep all
    if projections.iter().any(|p| matches!(p, Projection::Star)) {
        return Ok(block);
    }

    let mut new_cols: Vec<Column> = Vec::new();
    for proj in projections {
        match proj {
            Projection::Star => { new_cols.extend(block.columns.iter().cloned()); }
            Projection::Expr { expr, alias } => {
                let out_name = || alias.clone().unwrap_or_else(|| "expr".into());

                match expr {
                    Expr::Col(c) | Expr::QualCol(_, c) => {
                        let col_name = match expr {
                            Expr::QualCol(t, c2) => format!("{}.{}", t, c2),
                            _                    => c.clone(),
                        };
                        let src = block.columns.iter().find(|col| {
                            col.name == col_name || col.name.ends_with(&format!(".{}", col_name))
                        }).ok_or_else(|| KoreError::InvalidArgument(format!("column not found: {col_name}")))?;
                        let mut nc = src.clone();
                        if let Some(a) = alias { nc.name = a.clone(); }
                        new_cols.push(nc);
                    }
                    // Window function columns are already materialized under alias name
                    Expr::Window { .. } => {
                        let win_col = alias.clone().unwrap_or_else(|| "__win".into());
                        let src = block.columns.iter().find(|c| c.name == win_col)
                            .ok_or_else(|| KoreError::InvalidArgument(format!("window col not found: {win_col}")))?;
                        new_cols.push(src.clone());
                    }
                    // Aggregate results are already in block (from group_by_agg)
                    Expr::Agg { .. } => {
                        let col_name = out_name();
                        if let Some(src) = block.columns.iter().find(|c| c.name == col_name) {
                            new_cols.push(src.clone());
                        }
                        // else silently skip (shouldn't happen after group_by_agg)
                    }
                    // Everything else: evaluate row-by-row
                    _ => {
                        let n = block.num_rows;
                        let vals: Vec<ExprVal> = (0..n).map(|r| eval_expr(expr, &block, r)).collect();
                        new_cols.push(exprvals_to_column(out_name(), vals));
                    }
                }
            }
        }
    }
    let num_rows = block.num_rows;
    Ok(DataBlock { columns: new_cols, num_rows })
}

/// Convert a Vec<ExprVal> into a typed Column.
fn exprvals_to_column(name: String, vals: Vec<ExprVal>) -> Column {
    // Determine type from first non-null value
    match vals.iter().find(|v| !matches!(v, ExprVal::Null)) {
        Some(ExprVal::Int(_)) | Some(ExprVal::Bool(_)) if matches!(vals.iter().find(|v| !matches!(v, ExprVal::Null)), Some(ExprVal::Int(_))) =>
            Column { name, data: ColumnData::Int64(vals.into_iter().map(|v| match v {
                ExprVal::Int(i) => Some(i), ExprVal::Float(f) => Some(f as i64), _ => None,
            }).collect()) },
        Some(ExprVal::Float(_)) =>
            Column { name, data: ColumnData::Float64(vals.into_iter().map(|v| match v {
                ExprVal::Float(f) => Some(f), ExprVal::Int(i) => Some(i as f64), _ => None,
            }).collect()) },
        Some(ExprVal::Bool(_)) =>
            Column { name, data: ColumnData::Bool(vals.into_iter().map(|v| match v {
                ExprVal::Bool(b) => Some(b), _ => None,
            }).collect()) },
        // Str and Null fall here
        _ =>
            Column { name, data: ColumnData::Str(vals.into_iter().map(|v| match v {
                ExprVal::Str(s) => Some(s), ExprVal::Int(i) => Some(i.to_string()),
                ExprVal::Float(f) => Some(f.to_string()), ExprVal::Bool(b) => Some(b.to_string()),
                ExprVal::Null => None,
            }).collect()) },
    }
}

// ─── Global aggregation (no GROUP BY) ────────────────────────────────────────

/// Aggregate the entire block into a single row.
fn global_agg(block: DataBlock, projections: &[Projection]) -> Result<DataBlock, KoreError> {
    let all_rows: Vec<usize> = (0..block.num_rows).collect();
    let mut new_cols: Vec<Column> = Vec::new();
    for proj in projections {
        if let Projection::Expr { expr: Expr::Agg { func, expr: inner }, alias } = proj {
            let col_name = match inner.as_ref() {
                Expr::Col(c)        => c.clone(),
                Expr::QualCol(t, c) => format!("{}.{}", t, c),
                _ => String::new(),
            };
            let vals: Vec<f64> = all_rows.iter()
                .filter_map(|&r| to_f64(&get_cell(&block, &col_name, r)))
                .collect();
            let v: Option<f64> = match func {
                AggFunc::Count => Some(all_rows.len() as f64),
                AggFunc::CountDistinct => {
                    use std::collections::HashSet;
                    let mut seen = HashSet::new();
                    let n = all_rows.iter().filter(|&&r| {
                        let cell = get_cell(&block, &col_name, r);
                        let key = match &cell {
                            ExprVal::Int(i)   => format!("i{i}"),
                            ExprVal::Float(f) => format!("f{f:.10}"),
                            ExprVal::Str(s)   => format!("s{s}"),
                            ExprVal::Bool(b)  => format!("b{b}"),
                            ExprVal::Null     => return false,
                        };
                        seen.insert(key)
                    }).count();
                    Some(n as f64)
                }
                AggFunc::Sum => if vals.is_empty() { None } else { Some(vals.iter().sum()) },
                AggFunc::Avg => if vals.is_empty() { None } else { Some(vals.iter().sum::<f64>() / vals.len() as f64) },
                AggFunc::Min => vals.iter().copied().reduce(f64::min),
                AggFunc::Max => vals.iter().copied().reduce(f64::max),
            };
            let name = alias.clone().unwrap_or_else(|| format!("{:?}({})", func, col_name));
            new_cols.push(Column { name, data: ColumnData::Float64(vec![v]) });
        }
    }
    Ok(DataBlock { columns: new_cols, num_rows: 1 })
}

// ─── GROUP BY (aggregate) ─────────────────────────────────────────────────────

fn group_by_agg(
    block: DataBlock,
    group_cols: &[String],
    projections: &[Projection],
) -> Result<DataBlock, KoreError> {
    // Build group key → row indices
    let mut groups: Vec<(Vec<ExprVal>, Vec<usize>)> = Vec::new();
    'outer: for row in 0..block.num_rows {
        let key: Vec<ExprVal> = group_cols.iter()
            .map(|c| get_cell(&block, c, row))
            .collect();
        for (k, idxs) in &mut groups {
            if expr_vals_eq(k, &key) { idxs.push(row); continue 'outer; }
        }
        groups.push((key, vec![row]));
    }

    // Build result block from aggregated groups
    // For now, just emit first row of each group (full agg = TODO for complex cases)
    let first_rows: Vec<usize> = groups.iter().map(|(_, idxs)| idxs[0]).collect();
    let agg_block = block.select_rows(&first_rows);

    // Handle SUM/COUNT/AVG/MIN/MAX in projections
    let has_agg = projections.iter().any(|p| matches!(p, Projection::Expr { expr: Expr::Agg { .. }, .. }));
    if !has_agg { return Ok(agg_block); }

    let mut new_cols: Vec<Column> = Vec::new();
    for proj in projections {
        match proj {
            Projection::Star => {
                new_cols.extend(agg_block.columns.iter().cloned());
            }
            Projection::Expr { expr, alias } => {
                match expr {
                    Expr::Agg { func, expr: inner } => {
                        let col_name = match inner.as_ref() {
                            Expr::Col(c)       => c.clone(),
                            Expr::QualCol(t, c) => format!("{}.{}", t, c),
                            _ => String::new(),
                        };
                        let mut agg_vals: Vec<Option<f64>> = Vec::new();
                        for (_, idxs) in &groups {
                            let vals: Vec<f64> = idxs.iter()
                                .filter_map(|&r| to_f64(&get_cell(&block, &col_name, r)))
                                .collect();
                            let v = match func {
                                AggFunc::Count => Some(idxs.len() as f64),
                                AggFunc::CountDistinct => {
                                    use std::collections::HashSet;
                                    let mut seen = HashSet::new();
                                    let n = idxs.iter().filter(|&&r| {
                                        let cell = get_cell(&block, &col_name, r);
                                        let key = match &cell {
                                            ExprVal::Int(i)   => format!("i{i}"),
                                            ExprVal::Float(f) => format!("f{f:.10}"),
                                            ExprVal::Str(s)   => format!("s{s}"),
                                            ExprVal::Bool(b)  => format!("b{b}"),
                                            ExprVal::Null     => return false,
                                        };
                                        seen.insert(key)
                                    }).count();
                                    Some(n as f64)
                                }
                                AggFunc::Sum   => if vals.is_empty() { None } else { Some(vals.iter().sum()) },
                                AggFunc::Avg   => if vals.is_empty() { None } else { Some(vals.iter().sum::<f64>() / vals.len() as f64) },
                                AggFunc::Min   => vals.iter().copied().reduce(f64::min),
                                AggFunc::Max   => vals.iter().copied().reduce(f64::max),
                            };
                            agg_vals.push(v);
                        }
                        let name = alias.clone().unwrap_or_else(|| format!("{:?}({})", func, col_name));
                        new_cols.push(Column {
                            name,
                            data: ColumnData::Float64(agg_vals.into_iter().map(|v| v).collect()),
                        });
                    }
                    other => {
                        let col_name = match other {
                            Expr::Col(c)       => c.clone(),
                            Expr::QualCol(t, c) => format!("{}.{}", t, c),
                            _ => continue,
                        };
                        if let Some(src) = agg_block.columns.iter().find(|c| {
                            c.name == col_name || c.name.ends_with(&format!(".{}", col_name))
                        }) {
                            let mut nc = src.clone();
                            if let Some(a) = alias { nc.name = a.clone(); }
                            new_cols.push(nc);
                        }
                    }
                }
            }
        }
    }

    let num_rows = groups.len();
    Ok(DataBlock { columns: new_cols, num_rows })
}

fn expr_vals_eq(a: &[ExprVal], b: &[ExprVal]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| match (x, y) {
        (ExprVal::Int(x),   ExprVal::Int(y))   => x == y,
        (ExprVal::Float(x), ExprVal::Float(y)) => (x - y).abs() < 1e-10,
        (ExprVal::Str(x),   ExprVal::Str(y))   => x == y,
        (ExprVal::Bool(x),  ExprVal::Bool(y))  => x == y,
        (ExprVal::Null,     ExprVal::Null)      => true,
        _ => false,
    })
}

// ── Map AST WindowFn → kore-window WindowFn ───────────────────────────────────

// ── LIKE pattern matching ─────────────────────────────────────────────────────

/// SQL LIKE: `%` = any chars, `_` = single char, `\` = escape char.
fn like_match(value: &str, pattern: &str) -> bool {
    like_recursive(value.as_bytes(), pattern.as_bytes())
}

fn like_recursive(s: &[u8], p: &[u8]) -> bool {
    match (s, p) {
        (_, [])           => s.is_empty(),
        (_, [b'%', rest @ ..]) => {
            // % matches 0 or more characters
            if like_recursive(s, rest) { return true; }
            if let [_, tail @ ..] = s { return like_recursive(tail, p); }
            false
        }
        ([], _) => false,
        ([sc, st @ ..], [b'_', pt @ ..]) => like_recursive(st, pt),  // _ matches any one
        ([sc, st @ ..], [pc, pt @ ..]) if sc == pc => like_recursive(st, pt),
        _ => false,
    }
}

fn col_name_from_expr(e: &Expr) -> String {    match e {
        Expr::Col(n)        => n.clone(),
        Expr::QualCol(_, n) => n.clone(),
        _ => "__expr__".into(),
    }
}

fn ast_to_win_fn(ast: &WindowFn) -> WinFn {
    match ast {
        WindowFn::RowNumber   => WinFn::RowNumber,
        WindowFn::Rank        => WinFn::Rank,
        WindowFn::DenseRank   => WinFn::DenseRank,
        WindowFn::Ntile(n)    => WinFn::Ntile(match n.as_ref() { Expr::Int(i) => *i as usize, _ => 4 }),
        WindowFn::Lag  { expr, offset } => WinFn::Lag  { col: col_name_from_expr(expr), offset: match offset.as_ref() { Expr::Int(i) => *i as usize, _ => 1 } },
        WindowFn::Lead { expr, offset } => WinFn::Lead { col: col_name_from_expr(expr), offset: match offset.as_ref() { Expr::Int(i) => *i as usize, _ => 1 } },
        WindowFn::Agg { func, expr } => match func {
            AggFunc::Sum   => WinFn::Sum  (col_name_from_expr(expr)),
            AggFunc::Avg   => WinFn::Avg  (col_name_from_expr(expr)),
            AggFunc::Count | AggFunc::CountDistinct => WinFn::Count(col_name_from_expr(expr)),
            AggFunc::Min   => WinFn::Min  (col_name_from_expr(expr)),
            AggFunc::Max   => WinFn::Max  (col_name_from_expr(expr)),
        },
        WindowFn::CumSum(e)    => WinFn::CumSum    (col_name_from_expr(e)),
        WindowFn::FirstValue(e) => WinFn::FirstValue(col_name_from_expr(e)),
        WindowFn::LastValue(e)  => WinFn::LastValue (col_name_from_expr(e)),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn make_orders() -> DataBlock {
        DataBlock {
            num_rows: 4,
            columns: vec![
                Column { name: "id".into(),      data: ColumnData::Int64(vec![Some(1),Some(2),Some(3),Some(4)]) },
                Column { name: "cust_id".into(),  data: ColumnData::Int64(vec![Some(10),Some(20),Some(10),Some(30)]) },
                Column { name: "score".into(),    data: ColumnData::Float64(vec![Some(90.0),Some(70.0),Some(85.0),Some(60.0)]) },
            ],
        }
    }

    fn make_customers() -> DataBlock {
        DataBlock {
            num_rows: 3,
            columns: vec![
                Column { name: "id".into(),   data: ColumnData::Int64(vec![Some(10),Some(20),Some(30)]) },
                Column { name: "name".into(), data: ColumnData::Str(vec![Some("Alice".into()),Some("Bob".into()),Some("Carol".into())]) },
            ],
        }
    }

    #[test]
    fn test_simple_where_limit() {
        let mut ctx = KqlContext::new();
        ctx.register("orders", make_orders());
        let result = ctx.query(
            "SELECT * FROM orders WHERE score > 80 ORDER BY score DESC LIMIT 2"
        ).unwrap();
        assert_eq!(result.num_rows, 2);
    }

    #[test]
    fn test_inner_join() {
        let mut ctx = KqlContext::new();
        ctx.register("orders",    make_orders());
        ctx.register("customers", make_customers());
        let result = ctx.query(
            "SELECT * FROM orders AS a INNER JOIN customers AS b ON a.cust_id = b.id"
        ).unwrap();
        // All 4 orders have valid cust_id in customers table
        assert_eq!(result.num_rows, 4);
    }

    #[test]
    fn test_aggregate() {
        let mut ctx = KqlContext::new();
        ctx.register("orders", make_orders());
        let result = ctx.query(
            "SELECT cust_id, SUM(score) AS total FROM orders GROUP BY cust_id"
        ).unwrap();
        assert_eq!(result.num_rows, 3); // 3 distinct cust_ids
    }

    // ─── Layer 34: Scalar functions ─────────────────────────────────────────

    fn make_strings() -> DataBlock {
        DataBlock {
            num_rows: 3,
            columns: vec![
                Column { name: "id".into(),  data: ColumnData::Int64(vec![Some(1),Some(2),Some(3)]) },
                Column { name: "tag".into(), data: ColumnData::Str(vec![
                    Some("hello".into()), Some("  World  ".into()), Some("Rust".into())
                ]) },
                Column { name: "val".into(), data: ColumnData::Float64(vec![Some(3.7), Some(-1.5), Some(2.0)]) },
            ],
        }
    }

    #[test]
    fn test_string_functions() {
        let mut ctx = KqlContext::new();
        ctx.register("t", make_strings());
        // UPPER, LOWER, TRIM, LENGTH
        let r = ctx.query(
            "SELECT UPPER(tag) AS u, LOWER(tag) AS l, TRIM(tag) AS tr, LENGTH(tag) AS n FROM t WHERE id = 2"
        ).unwrap();
        assert_eq!(r.num_rows, 1);
        if let ColumnData::Str(v) = &r.columns.iter().find(|c| c.name=="u").unwrap().data {
            assert_eq!(v[0], Some("  WORLD  ".into()));
        }
        if let ColumnData::Str(v) = &r.columns.iter().find(|c| c.name=="tr").unwrap().data {
            assert_eq!(v[0], Some("World".into()));
        }
    }

    #[test]
    fn test_math_functions() {
        let mut ctx = KqlContext::new();
        ctx.register("t", make_strings());
        let r = ctx.query("SELECT ABS(val) AS a, ROUND(val, 0) AS r, CEIL(val) AS c FROM t").unwrap();
        assert_eq!(r.num_rows, 3);
        if let ColumnData::Float64(v) = &r.columns.iter().find(|c| c.name=="a").unwrap().data {
            assert!((v[0].unwrap() - 3.7).abs() < 0.001);
            assert!((v[1].unwrap() - 1.5).abs() < 0.001); // ABS(-1.5)
        }
    }

    #[test]
    fn test_count_distinct() {
        let mut ctx = KqlContext::new();
        ctx.register("orders", make_orders());
        let r = ctx.query(
            "SELECT COUNT(DISTINCT cust_id) AS uniq FROM orders"
        ).unwrap();
        // 3 distinct cust_ids (10, 20, 30) in 4 rows
        if let ColumnData::Float64(v) = &r.columns.iter().find(|c| c.name=="uniq").unwrap().data {
            assert_eq!(v[0], Some(3.0));
        }
    }

    #[test]
    fn test_having_clause() {
        let mut ctx = KqlContext::new();
        ctx.register("orders", make_orders());
        // cust_id=10 appears twice (scores 90, 85) → sum=175; others appear once
        let r = ctx.query(
            "SELECT cust_id, SUM(score) AS total FROM orders GROUP BY cust_id HAVING total > 100"
        ).unwrap();
        assert_eq!(r.num_rows, 1);
    }

    #[test]
    fn test_coalesce_cast() {
        let mut ctx = KqlContext::new();
        ctx.register("t", make_strings());
        let r = ctx.query("SELECT COALESCE(id, 0) AS cid, CAST(val AS VARCHAR) AS sv FROM t LIMIT 1").unwrap();
        assert_eq!(r.num_rows, 1);
    }
}

