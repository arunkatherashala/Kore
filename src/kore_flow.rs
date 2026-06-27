/// KORE ∞ Layer 6 — KoreFlow: Advanced SQL Engine
///
/// Multi-table JOINs, Window Functions, HAVING, Subqueries — pure Rust.
/// 
/// Supported SQL:
///   SELECT a.col, b.col, COUNT(*), SUM(a.amount),
///          ROW_NUMBER() OVER (PARTITION BY a.cat ORDER BY a.amount DESC)
///   FROM "left.kore" a
///   INNER JOIN "right.kore" b ON a.id = b.id
///   LEFT  JOIN "extra.kore" e ON a.cat = e.cat
///   WHERE a.amount > 100 AND b.active = 'true'
///   GROUP BY a.cat, b.name
///   HAVING COUNT(*) > 5
///   ORDER BY COUNT(*) DESC
///   LIMIT 20

use std::collections::HashMap;
use crate::kore_v2::{KoreReader, KVal};

// ============================================================================
// Tokens
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Select, From, Where, Group, By, Order, Asc, Desc, Limit,
    And, Or, Not, As, On, Having,
    Inner, Left, Right, Full, Outer, Join,
    Over, Partition,
    Comma, Star, Dot, LParen, RParen,
    Eq, Neq, Lt, Lte, Gt, Gte,
    Ident(String), Str(String), Num(f64),
    Count, Sum, Avg, Min, Max,
    RowNumber, Rank, DenseRank, Lag, Lead,
    IsKw, NullKw,
}

fn tokenize(sql: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let ch: Vec<char> = sql.chars().collect();
    let mut i = 0;
    while i < ch.len() {
        match ch[i] {
            ' ' | '\t' | '\n' | '\r' => { i += 1; }
            '*' => { out.push(Tok::Star);   i += 1; }
            ',' => { out.push(Tok::Comma);  i += 1; }
            '(' => { out.push(Tok::LParen); i += 1; }
            ')' => { out.push(Tok::RParen); i += 1; }
            '.' => { out.push(Tok::Dot);    i += 1; }
            '=' => { out.push(Tok::Eq);     i += 1; }
            '!' => { if i+1 < ch.len() && ch[i+1]=='=' { out.push(Tok::Neq); i+=2; } else { i+=1; } }
            '<' => { if i+1<ch.len()&&ch[i+1]=='='{ out.push(Tok::Lte);i+=2; }
                     else if i+1<ch.len()&&ch[i+1]=='>'{ out.push(Tok::Neq);i+=2; }
                     else{ out.push(Tok::Lt);i+=1; } }
            '>' => { if i+1<ch.len()&&ch[i+1]=='='{ out.push(Tok::Gte);i+=2; }
                     else{ out.push(Tok::Gt);i+=1; } }
            '\'' | '"' => {
                let q=ch[i]; i+=1;
                let mut s=String::new();
                while i<ch.len()&&ch[i]!=q { s.push(ch[i]); i+=1; }
                i+=1; out.push(Tok::Str(s));
            }
            c if c.is_ascii_digit() || (c=='-'&&i+1<ch.len()&&ch[i+1].is_ascii_digit()) => {
                let mut s=String::new();
                if c=='-'{ s.push(c); i+=1; }
                while i<ch.len()&&(ch[i].is_ascii_digit()||ch[i]=='.'){ s.push(ch[i]); i+=1; }
                out.push(Tok::Num(s.parse().unwrap_or(0.0)));
            }
            c if c.is_alphabetic() || c=='_' => {
                let mut s=String::new();
                while i<ch.len()&&(ch[i].is_alphanumeric()||ch[i]=='_'){ s.push(ch[i]); i+=1; }
                out.push(match s.to_uppercase().as_str() {
                    "SELECT"=>Tok::Select, "FROM"=>Tok::From, "WHERE"=>Tok::Where,
                    "GROUP"=>Tok::Group, "BY"=>Tok::By, "ORDER"=>Tok::Order,
                    "ASC"=>Tok::Asc, "DESC"=>Tok::Desc, "LIMIT"=>Tok::Limit,
                    "AND"=>Tok::And, "OR"=>Tok::Or, "NOT"=>Tok::Not,
                    "AS"=>Tok::As, "ON"=>Tok::On, "HAVING"=>Tok::Having,
                    "INNER"=>Tok::Inner, "LEFT"=>Tok::Left, "RIGHT"=>Tok::Right,
                    "FULL"=>Tok::Full, "OUTER"=>Tok::Outer, "JOIN"=>Tok::Join,
                    "OVER"=>Tok::Over, "PARTITION"=>Tok::Partition,
                    "COUNT"=>Tok::Count, "SUM"=>Tok::Sum, "AVG"=>Tok::Avg,
                    "MIN"=>Tok::Min, "MAX"=>Tok::Max,
                    "ROW_NUMBER"=>Tok::RowNumber, "RANK"=>Tok::Rank,
                    "DENSE_RANK"=>Tok::DenseRank, "LAG"=>Tok::Lag, "LEAD"=>Tok::Lead,
                    "IS"=>Tok::IsKw, "NULL"=>Tok::NullKw,
                    _ => Tok::Ident(s),
                });
            }
            _ => { i+=1; }
        }
    }
    out
}

// ============================================================================
// AST
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
enum CmpOp { Eq, Neq, Lt, Lte, Gt, Gte }

#[derive(Debug, Clone)]
enum AggFn { Count, Sum, Avg, Min, Max }

#[derive(Debug, Clone)]
enum WinFn { RowNumber, Rank, DenseRank, Lag(String, usize), Lead(String, usize) }

#[derive(Debug, Clone)]
struct WinSpec { partition_by: Vec<String>, order_by: Vec<(String, bool)> }

#[derive(Debug, Clone)]
enum Expr {
    Col(String),
    QCol(String, String),
    Lit(KVal),
    Agg(AggFn, Option<String>),
    Win(WinFn, WinSpec),
    BinOp(Box<Expr>, CmpOp, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    IsNull(Box<Expr>),
    Star,
}

#[derive(Debug, Clone)]
struct SelItem { expr: Expr, alias: Option<String> }

#[derive(Debug, Clone)]
enum JoinKind { Inner, Left }

#[derive(Debug, Clone)]
struct JoinClause { kind: JoinKind, path: String, alias: String, on: Expr }

#[derive(Debug, Clone)]
struct Query {
    select:     Vec<SelItem>,
    from_path:  String,
    from_alias: String,
    joins:      Vec<JoinClause>,
    where_:     Option<Expr>,
    group_by:   Vec<String>,
    having:     Option<Expr>,
    order_by:   Vec<(String, bool)>,
    limit:      Option<usize>,
}

// ============================================================================
// Parser
// ============================================================================

struct Parser { toks: Vec<Tok>, pos: usize }

impl Parser {
    fn new(toks: Vec<Tok>) -> Self { Parser { toks, pos: 0 } }
    fn peek(&self) -> Option<&Tok> { self.toks.get(self.pos) }
    fn next(&mut self) -> Option<Tok> {
        if self.pos < self.toks.len() { let t = self.toks[self.pos].clone(); self.pos += 1; Some(t) }
        else { None }
    }
    fn eat(&mut self, t: &Tok) { if self.peek() == Some(t) { self.next(); } }

    fn parse(&mut self) -> Result<Query, String> {
        self.eat(&Tok::Select);
        let select = self.parse_select_list()?;
        self.eat(&Tok::From);
        let from_path = self.parse_path();
        let from_alias = self.parse_opt_alias(&from_path);

        let mut joins = Vec::new();
        loop {
            let kind = match self.peek() {
                Some(Tok::Inner)       => { self.next(); self.eat(&Tok::Join); JoinKind::Inner }
                Some(Tok::Left)        => { self.next(); self.eat(&Tok::Outer); self.eat(&Tok::Join); JoinKind::Left }
                Some(Tok::Join)        => { self.next(); JoinKind::Inner }
                _ => break,
            };
            let path = self.parse_path();
            let alias = self.parse_opt_alias_nkw(&path);
            self.eat(&Tok::On);
            let on = self.parse_expr()?;
            joins.push(JoinClause { kind, path, alias, on });
        }

        let where_ = if self.peek() == Some(&Tok::Where) { self.next(); Some(self.parse_expr()?) } else { None };

        let group_by = if self.peek() == Some(&Tok::Group) {
            self.next(); self.eat(&Tok::By);
            let mut cols = vec![self.parse_colref()];
            while self.peek() == Some(&Tok::Comma) { self.next(); cols.push(self.parse_colref()); }
            cols
        } else { Vec::new() };

        let having = if self.peek() == Some(&Tok::Having) { self.next(); Some(self.parse_expr()?) } else { None };

        let order_by = if self.peek() == Some(&Tok::Order) {
            self.next(); self.eat(&Tok::By);
            let mut cols = Vec::new();
            loop {
                let col = self.parse_colref();
                let asc = match self.peek() { Some(Tok::Asc) => { self.next(); true } Some(Tok::Desc) => { self.next(); false } _ => true };
                cols.push((col, asc));
                if self.peek() == Some(&Tok::Comma) { self.next(); } else { break; }
            }
            cols
        } else { Vec::new() };

        let limit = if self.peek() == Some(&Tok::Limit) {
            self.next(); match self.next() { Some(Tok::Num(n)) => Some(n as usize), _ => None }
        } else { None };

        Ok(Query { select, from_path, from_alias, joins, where_, group_by, having, order_by, limit })
    }

    fn parse_path(&mut self) -> String {
        match self.next() {
            Some(Tok::Str(s)) => s,
            Some(Tok::Ident(s)) => {
                // handle "name.kore" without quotes
                if self.peek() == Some(&Tok::Dot) {
                    self.next();
                    match self.next() { Some(Tok::Ident(ext)) => format!("{}.{}", s, ext), _ => s }
                } else { s }
            }
            _ => String::new()
        }
    }

    fn parse_opt_alias(&mut self, path: &str) -> String {
        if self.peek() == Some(&Tok::As) { self.next(); }
        match self.peek() {
            Some(Tok::Ident(_)) => match self.next() { Some(Tok::Ident(s)) => s, _ => stem(path) },
            _ => stem(path),
        }
    }

    fn parse_opt_alias_nkw(&mut self, path: &str) -> String {
        // Only consume if next is Ident and NOT a keyword
        match self.peek() {
            Some(Tok::As) => { self.next(); match self.next() { Some(Tok::Ident(s)) => s, _ => stem(path) } }
            Some(Tok::Ident(_)) => match self.next() { Some(Tok::Ident(s)) => s, _ => stem(path) },
            _ => stem(path),
        }
    }

    fn parse_colref(&mut self) -> String {
        match self.next() {
            Some(Tok::Ident(s)) => {
                if self.peek() == Some(&Tok::Dot) {
                    self.next();
                    match self.next() { Some(Tok::Ident(c)) => format!("{}.{}", s, c), _ => s }
                } else { s }
            }
            Some(Tok::Num(n)) => n.to_string(),
            _ => String::new(),
        }
    }

    fn parse_select_list(&mut self) -> Result<Vec<SelItem>, String> {
        let mut items = Vec::new();
        loop {
            let expr = self.parse_select_expr()?;
            let alias = if self.peek() == Some(&Tok::As) {
                self.next(); match self.next() { Some(Tok::Ident(s)) => Some(s), _ => None }
            } else { None };
            items.push(SelItem { expr, alias });
            if self.peek() == Some(&Tok::Comma) { self.next(); } else { break; }
        }
        Ok(items)
    }

    fn parse_select_expr(&mut self) -> Result<Expr, String> {
        match self.peek().cloned() {
            Some(Tok::Star) => { self.next(); Ok(Expr::Star) }
            Some(Tok::Count) => { self.next(); self.parse_agg(AggFn::Count) }
            Some(Tok::Sum)   => { self.next(); self.parse_agg(AggFn::Sum) }
            Some(Tok::Avg)   => { self.next(); self.parse_agg(AggFn::Avg) }
            Some(Tok::Min)   => { self.next(); self.parse_agg(AggFn::Min) }
            Some(Tok::Max)   => { self.next(); self.parse_agg(AggFn::Max) }
            Some(Tok::RowNumber)  => { self.next(); self.parse_win(WinFn::RowNumber) }
            Some(Tok::Rank)       => { self.next(); self.parse_win(WinFn::Rank) }
            Some(Tok::DenseRank)  => { self.next(); self.parse_win(WinFn::DenseRank) }
            Some(Tok::Lag)        => { self.next(); self.parse_lag_lead(false) }
            Some(Tok::Lead)       => { self.next(); self.parse_lag_lead(true) }
            _ => self.parse_expr(),
        }
    }

    fn parse_agg(&mut self, f: AggFn) -> Result<Expr, String> {
        self.eat(&Tok::LParen);
        let col = match self.peek() {
            Some(Tok::Star) => { self.next(); None }
            Some(Tok::Ident(_)) => Some(self.parse_colref()),
            _ => { self.next(); None }
        };
        self.eat(&Tok::RParen);
        Ok(Expr::Agg(f, col))
    }

    fn parse_win(&mut self, f: WinFn) -> Result<Expr, String> {
        self.eat(&Tok::LParen); self.eat(&Tok::RParen);
        self.eat(&Tok::Over); self.eat(&Tok::LParen);
        let part = if self.peek() == Some(&Tok::Partition) {
            self.next(); self.eat(&Tok::By);
            let mut c = vec![self.parse_colref()];
            while self.peek() == Some(&Tok::Comma) { self.next(); c.push(self.parse_colref()); }
            c
        } else { Vec::new() };
        let ord = if self.peek() == Some(&Tok::Order) {
            self.next(); self.eat(&Tok::By);
            let mut c = Vec::new();
            loop {
                let col = self.parse_colref();
                let asc = match self.peek() { Some(Tok::Asc) => { self.next(); true } Some(Tok::Desc) => { self.next(); false } _ => true };
                c.push((col, asc));
                if self.peek() == Some(&Tok::Comma) { self.next(); } else { break; }
            }
            c
        } else { Vec::new() };
        self.eat(&Tok::RParen);
        Ok(Expr::Win(f, WinSpec { partition_by: part, order_by: ord }))
    }

    fn parse_lag_lead(&mut self, is_lead: bool) -> Result<Expr, String> {
        self.eat(&Tok::LParen);
        let col = self.parse_colref();
        let offset = if self.peek() == Some(&Tok::Comma) {
            self.next();
            match self.next() { Some(Tok::Num(n)) => n as usize, _ => 1 }
        } else { 1 };
        self.eat(&Tok::RParen);
        self.eat(&Tok::Over); self.eat(&Tok::LParen);
        let part = if self.peek() == Some(&Tok::Partition) {
            self.next(); self.eat(&Tok::By);
            let mut c = vec![self.parse_colref()];
            while self.peek() == Some(&Tok::Comma) { self.next(); c.push(self.parse_colref()); }
            c
        } else { Vec::new() };
        let ord = if self.peek() == Some(&Tok::Order) {
            self.next(); self.eat(&Tok::By);
            let mut c = Vec::new();
            loop {
                let col2 = self.parse_colref();
                let asc = match self.peek() { Some(Tok::Asc) => { self.next(); true } Some(Tok::Desc) => { self.next(); false } _ => true };
                c.push((col2, asc));
                if self.peek() == Some(&Tok::Comma) { self.next(); } else { break; }
            }
            c
        } else { Vec::new() };
        self.eat(&Tok::RParen);
        let wf = if is_lead { WinFn::Lead(col, offset) } else { WinFn::Lag(col, offset) };
        Ok(Expr::Win(wf, WinSpec { partition_by: part, order_by: ord }))
    }

    fn parse_expr(&mut self) -> Result<Expr, String> {
        let l = self.parse_and()?;
        if self.peek() == Some(&Tok::Or) { self.next(); let r = self.parse_and()?; Ok(Expr::Or(Box::new(l), Box::new(r))) }
        else { Ok(l) }
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let l = self.parse_not()?;
        if self.peek() == Some(&Tok::And) { self.next(); let r = self.parse_not()?; Ok(Expr::And(Box::new(l), Box::new(r))) }
        else { Ok(l) }
    }

    fn parse_not(&mut self) -> Result<Expr, String> {
        if self.peek() == Some(&Tok::Not) { self.next(); Ok(Expr::Not(Box::new(self.parse_cmp()?))) }
        else { self.parse_cmp() }
    }

    fn parse_cmp(&mut self) -> Result<Expr, String> {
        let l = self.parse_primary()?;
        if self.peek() == Some(&Tok::IsKw) {
            self.next();
            let neg = if self.peek() == Some(&Tok::Not) { self.next(); true } else { false };
            self.eat(&Tok::NullKw);
            return Ok(if neg { Expr::Not(Box::new(Expr::IsNull(Box::new(l)))) } else { Expr::IsNull(Box::new(l)) });
        }
        let op = match self.peek() {
            Some(Tok::Eq)  => CmpOp::Eq,  Some(Tok::Neq) => CmpOp::Neq,
            Some(Tok::Lt)  => CmpOp::Lt,  Some(Tok::Lte) => CmpOp::Lte,
            Some(Tok::Gt)  => CmpOp::Gt,  Some(Tok::Gte) => CmpOp::Gte,
            _ => return Ok(l),
        };
        self.next();
        let r = self.parse_primary()?;
        Ok(Expr::BinOp(Box::new(l), op, Box::new(r)))
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Tok::Ident(s)) => {
                if self.peek() == Some(&Tok::Dot) {
                    self.next();
                    match self.next() {
                        Some(Tok::Ident(col)) => Ok(Expr::QCol(s, col)),
                        Some(Tok::Star)       => Ok(Expr::Star),
                        _                     => Ok(Expr::Col(s)),
                    }
                } else { Ok(Expr::Col(s)) }
            }
            Some(Tok::Str(s))   => Ok(Expr::Lit(KVal::Str(s))),
            Some(Tok::Num(n))   => Ok(Expr::Lit(KVal::Float(n))),
            Some(Tok::NullKw)   => Ok(Expr::Lit(KVal::Null)),
            Some(Tok::LParen)   => { let e = self.parse_expr()?; self.eat(&Tok::RParen); Ok(e) }
            // Allow aggregate functions inside HAVING expressions
            Some(Tok::Count) => self.parse_agg(AggFn::Count),
            Some(Tok::Sum)   => self.parse_agg(AggFn::Sum),
            Some(Tok::Avg)   => self.parse_agg(AggFn::Avg),
            Some(Tok::Min)   => self.parse_agg(AggFn::Min),
            Some(Tok::Max)   => self.parse_agg(AggFn::Max),
            t => Err(format!("Unexpected token: {:?}", t)),
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn stem(path: &str) -> String {
    std::path::Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("t").to_string()
}

fn kv_f64(v: &KVal) -> f64 {
    match v { KVal::Int(x) => *x as f64, KVal::Float(x) => *x,
              KVal::Str(s) => s.parse().unwrap_or(0.0), _ => 0.0 }
}

fn kv_key(v: &KVal) -> String {
    match v { KVal::Int(x) => x.to_string(), KVal::Float(x) => format!("{}", x),
              KVal::Str(s) => s.to_lowercase(), KVal::Bool(b) => b.to_string(),
              KVal::Null => "\x00".to_string(), _ => format!("{:?}", v) }
}

fn kv_eq(a: &KVal, b: &KVal) -> bool {
    match (a, b) {
        (KVal::Int(x), KVal::Int(y))     => x == y,
        (KVal::Float(x), KVal::Float(y)) => (x - y).abs() < 1e-9,
        (KVal::Int(x), KVal::Float(y))   => (*x as f64 - y).abs() < 1e-9,
        (KVal::Float(x), KVal::Int(y))   => (x - *y as f64).abs() < 1e-9,
        (KVal::Str(x), KVal::Str(y))     => x.eq_ignore_ascii_case(y),
        (KVal::Bool(x), KVal::Bool(y))   => x == y,
        (KVal::Null, KVal::Null)         => true,
        (KVal::Str(s), KVal::Float(f)) | (KVal::Float(f), KVal::Str(s)) =>
            s.parse::<f64>().map(|sf| (sf - f).abs() < 1e-9).unwrap_or(false),
        (KVal::Str(s), KVal::Int(i)) | (KVal::Int(i), KVal::Str(s)) =>
            s.parse::<i64>().map(|si| si == *i).unwrap_or(false),
        _ => false,
    }
}

fn kv_cmp(a: &KVal, b: &KVal) -> i32 {
    let af = kv_f64(a); let bf = kv_f64(b);
    if af < bf { -1 } else if af > bf { 1 } else { 0 }
}

fn fmt_kv(v: &KVal) -> String {
    match v {
        KVal::Int(x)   => x.to_string(),
        KVal::Float(x) => fmt_f64(*x),
        KVal::Str(s)   => s.clone(),
        KVal::Bool(b)  => b.to_string(),
        KVal::Null     => "NULL".to_string(),
        _              => format!("{:?}", v),
    }
}

fn fmt_f64(f: f64) -> String {
    if f == f.floor() && f.abs() < 1e15 { format!("{}", f as i64) }
    else { format!("{:.4}", f).trim_end_matches('0').trim_end_matches('.').to_string() }
}

// ============================================================================
// Flat row context (all columns from all joined tables, prefixed alias.col)
// ============================================================================

fn load_table(path: &str) -> Result<(Vec<String>, Vec<Vec<KVal>>), String> {
    let reader = KoreReader::open(path)?;
    let col_names: Vec<String> = reader.columns.iter().map(|c| c.name.clone()).collect();
    let all_cols = reader.read_all_columns();
    let nrows = all_cols.first().map(|c| c.len()).unwrap_or(0);
    let rows: Vec<Vec<KVal>> = (0..nrows).map(|i| {
        all_cols.iter().map(|col| col.get(i).cloned().unwrap_or(KVal::Null)).collect()
    }).collect();
    Ok((col_names, rows))
}

/// Resolve a column name in a flat prefixed row.
/// Cols are stored as "alias.colname". Lookup by:
///   1. Exact match  2. "alias.col"  3. suffix ".col"  4. bare "col" anywhere
fn flat_resolve(cols: &[String], row: &[KVal], name: &str) -> KVal {
    if let Some(i) = cols.iter().position(|c| c.eq_ignore_ascii_case(name)) {
        return row.get(i).cloned().unwrap_or(KVal::Null);
    }
    let sfx = format!(".{}", name.to_lowercase());
    if let Some(i) = cols.iter().position(|c| c.to_lowercase().ends_with(&sfx)) {
        return row.get(i).cloned().unwrap_or(KVal::Null);
    }
    KVal::Null
}

fn flat_resolve_q(cols: &[String], row: &[KVal], alias: &str, col: &str) -> KVal {
    let full = format!("{}.{}", alias, col).to_lowercase();
    if let Some(i) = cols.iter().position(|c| c.to_lowercase() == full) {
        return row.get(i).cloned().unwrap_or(KVal::Null);
    }
    flat_resolve(cols, row, col)
}

fn eval_expr(expr: &Expr, cols: &[String], row: &[KVal]) -> KVal {
    match expr {
        Expr::Col(n)      => flat_resolve(cols, row, n),
        Expr::QCol(a, c)  => flat_resolve_q(cols, row, a, c),
        Expr::Lit(v)      => v.clone(),
        _ => KVal::Null,
    }
}

fn eval_filter(expr: &Expr, cols: &[String], row: &[KVal]) -> bool {
    match expr {
        Expr::BinOp(l, op, r) => {
            let lv = eval_expr(l, cols, row);
            let rv = eval_expr(r, cols, row);
            match op {
                CmpOp::Eq  => kv_eq(&lv, &rv),
                CmpOp::Neq => !kv_eq(&lv, &rv),
                CmpOp::Lt  => kv_cmp(&lv, &rv) < 0,
                CmpOp::Lte => kv_cmp(&lv, &rv) <= 0,
                CmpOp::Gt  => kv_cmp(&lv, &rv) > 0,
                CmpOp::Gte => kv_cmp(&lv, &rv) >= 0,
            }
        }
        Expr::And(l, r)  => eval_filter(l, cols, row) && eval_filter(r, cols, row),
        Expr::Or(l, r)   => eval_filter(l, cols, row) || eval_filter(r, cols, row),
        Expr::Not(e)     => !eval_filter(e, cols, row),
        Expr::IsNull(e)  => matches!(eval_expr(e, cols, row), KVal::Null),
        _ => true,
    }
}

// ============================================================================
// Extract join key column names from ON clause  (e.g. ON a.id = b.id)
// ============================================================================

fn extract_key(on: &Expr, alias: &str) -> String {
    if let Expr::BinOp(l, _, r) = on {
        for side in &[l.as_ref(), r.as_ref()] {
            if let Expr::QCol(a, c) = side { if a.eq_ignore_ascii_case(alias) { return c.clone(); } }
            if let Expr::Col(c) = side { return c.clone(); }
        }
    }
    String::new()
}

// ============================================================================
// KoreFlow public API
// ============================================================================

/// Layer 6: Advanced SQL engine — JOINs, HAVING, Window Functions
pub struct KoreFlow;

impl KoreFlow {
    /// Execute SQL. Returns (column_headers, rows_as_strings).
    pub fn sql(query: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
        let mut p = Parser::new(tokenize(query));
        let q = p.parse()?;
        execute(q)
    }

    /// Execute SQL and return a pretty ASCII table string.
    pub fn table(query: &str) -> String {
        match Self::sql(query) { Ok((h, r)) => render(&h, &r), Err(e) => format!("ERROR: {}", e) }
    }
}

// ============================================================================
// Execution
// ============================================================================

fn execute(q: Query) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    // ── Load tables ──────────────────────────────────────────────────────────
    let (lcols, lrows) = load_table(&q.from_path)?;
    let mut join_tables: Vec<(Vec<String>, Vec<Vec<KVal>>)> = Vec::new();
    for j in &q.joins { join_tables.push(load_table(&j.path)?); }

    // ── Build flat column list (alias.col) ────────────────────────────────────
    let mut flat_cols: Vec<String> = lcols.iter().map(|c| format!("{}.{}", q.from_alias, c)).collect();
    for (ji, j) in q.joins.iter().enumerate() {
        for c in &join_tables[ji].0 { flat_cols.push(format!("{}.{}", j.alias, c)); }
    }

    // ── Hash-join: build index on each right table ────────────────────────────
    let mut hmaps: Vec<HashMap<String, Vec<usize>>> = Vec::new();
    for (ji, j) in q.joins.iter().enumerate() {
        let rkey = extract_key(&j.on, &j.alias);
        let ri = join_tables[ji].0.iter().position(|c| c.eq_ignore_ascii_case(&rkey)).unwrap_or(0);
        let mut hm: HashMap<String, Vec<usize>> = HashMap::new();
        for (row_i, row) in join_tables[ji].1.iter().enumerate() {
            hm.entry(kv_key(row.get(ri).unwrap_or(&KVal::Null))).or_default().push(row_i);
        }
        hmaps.push(hm);
    }

    // ── Probe: build flat joined rows ─────────────────────────────────────────
    let mut flat_rows: Vec<Vec<KVal>> = Vec::new();
    for lrow in &lrows {
        let mut cur: Vec<Vec<KVal>> = vec![lrow.clone()];
        for (ji, j) in q.joins.iter().enumerate() {
            let lkey = extract_key(&j.on, &q.from_alias);
            let li = lcols.iter().position(|c| c.eq_ignore_ascii_case(&lkey)).unwrap_or(0);
            let lk = kv_key(lrow.get(li).unwrap_or(&KVal::Null));
            let mut nxt: Vec<Vec<KVal>> = Vec::new();
            for cr in &cur {
                if let Some(idxs) = hmaps[ji].get(&lk) {
                    for &ri in idxs {
                        let mut r = cr.clone();
                        r.extend_from_slice(&join_tables[ji].1[ri]);
                        nxt.push(r);
                    }
                } else if matches!(j.kind, JoinKind::Left) {
                    let mut r = cr.clone();
                    r.extend(vec![KVal::Null; join_tables[ji].0.len()]);
                    nxt.push(r);
                }
            }
            cur = nxt;
        }
        flat_rows.extend(cur);
    }

    // ── WHERE filter ──────────────────────────────────────────────────────────
    let filtered: Vec<Vec<KVal>> = flat_rows.into_iter()
        .filter(|r| q.where_.as_ref().map_or(true, |w| eval_filter(w, &flat_cols, r)))
        .collect();

    // ── Aggregate or plain select ─────────────────────────────────────────────
    let has_agg = q.select.iter().any(|s| matches!(s.expr, Expr::Agg(_, _)));
    let has_win = q.select.iter().any(|s| matches!(s.expr, Expr::Win(_, _)));

    let (mut headers, mut rows) = if has_agg && !q.group_by.is_empty() {
        exec_group_agg(&q, &flat_cols, &filtered)?
    } else if has_agg {
        exec_agg_only(&q, &flat_cols, &filtered)?
    } else {
        exec_select(&q, &flat_cols, &filtered)?
    };

    // ── HAVING ────────────────────────────────────────────────────────────────
    if let Some(hav) = &q.having {
        rows.retain(|row| eval_having(hav, &headers, row));
    }

    // ── Window functions ──────────────────────────────────────────────────────
    if has_win { apply_windows(&q, &mut headers, &mut rows); }

    // ── ORDER BY + LIMIT ──────────────────────────────────────────────────────
    apply_order_by(&q.order_by, &headers, &mut rows);
    if let Some(n) = q.limit { rows.truncate(n); }

    Ok((headers, rows))
}

// ── Plain SELECT ──────────────────────────────────────────────────────────────

fn exec_select(q: &Query, cols: &[String], rows: &[Vec<KVal>]) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    // Build output column definitions: (display_header, resolver_fn)
    let out_defs: Vec<(String, Expr)> = if q.select.len() == 1 && matches!(q.select[0].expr, Expr::Star) {
        // SELECT * — emit all columns (strip alias prefix for display)
        cols.iter().map(|c| {
            let disp = c.rfind('.').map(|i| c[i+1..].to_string()).unwrap_or(c.clone());
            (disp, Expr::Col(c.clone()))
        }).collect()
    } else {
        q.select.iter().filter(|s| !matches!(s.expr, Expr::Win(_, _))).flat_map(|s| -> Vec<(String, Expr)> {
            match &s.expr {
                Expr::Star => cols.iter().map(|c| {
                    let disp = c.rfind('.').map(|i| c[i+1..].to_string()).unwrap_or(c.clone());
                    (disp, Expr::Col(c.clone()))
                }).collect(),
                Expr::Col(n)     => vec![(s.alias.clone().unwrap_or_else(|| short(n)), s.expr.clone())],
                Expr::QCol(_, c) => vec![(s.alias.clone().unwrap_or_else(|| c.clone()), s.expr.clone())],
                Expr::Lit(_)     => vec![(s.alias.clone().unwrap_or("?".to_string()), s.expr.clone())],
                _ => vec![],
            }
        }).collect()
    };

    let headers: Vec<String> = out_defs.iter().map(|(h,_)| h.clone()).collect();
    let result: Vec<Vec<String>> = rows.iter().map(|row| {
        out_defs.iter().map(|(_, expr)| fmt_kv(&eval_expr(expr, cols, row))).collect()
    }).collect();
    Ok((headers, result))
}

// ── GROUP BY + AGG ────────────────────────────────────────────────────────────

fn exec_group_agg(q: &Query, cols: &[String], rows: &[Vec<KVal>]) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    // Resolve group-by column indices
    let gi: Vec<usize> = q.group_by.iter().map(|g| {
        let s = short(g);
        cols.iter().position(|c| c.eq_ignore_ascii_case(g) || c.to_lowercase().ends_with(&format!(".{}", s.to_lowercase()))).unwrap_or(usize::MAX)
    }).collect();

    // Collect agg definitions
    struct AggDef { f: AggFn, ci: Option<usize>, hdr: String }
    let agg_defs: Vec<AggDef> = q.select.iter().filter_map(|s| {
        if let Expr::Agg(f, col) = &s.expr {
            let ci = col.as_ref().and_then(|n| {
                let sn = short(n);
                cols.iter().position(|c| c.eq_ignore_ascii_case(n) || c.to_lowercase().ends_with(&format!(".{}", sn.to_lowercase())))
            });
            let h = s.alias.clone().unwrap_or_else(|| agg_header(f, col));
            Some(AggDef { f: f.clone(), ci, hdr: h })
        } else { None }
    }).collect();

    // Group accumulation
    struct Acc { key_vals: Vec<KVal>, cnt: Vec<usize>, sum: Vec<f64>, mn: Vec<f64>, mx: Vec<f64> }
    let mut map: HashMap<String, Acc> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for row in rows {
        let kv: Vec<KVal> = gi.iter().map(|&i| row.get(i).cloned().unwrap_or(KVal::Null)).collect();
        let k: String = kv.iter().map(kv_key).collect::<Vec<_>>().join("\x00");
        let acc = map.entry(k.clone()).or_insert_with(|| {
            order.push(k.clone());
            Acc { key_vals: kv.clone(), cnt: vec![0; agg_defs.len()], sum: vec![0.0; agg_defs.len()],
                  mn: vec![f64::MAX; agg_defs.len()], mx: vec![f64::MIN; agg_defs.len()] }
        });
        for (ai, ad) in agg_defs.iter().enumerate() {
            let v = ad.ci.and_then(|ci| row.get(ci)).unwrap_or(&KVal::Null);
            let f = kv_f64(v);
            acc.cnt[ai] += 1; acc.sum[ai] += f;
            if f < acc.mn[ai] { acc.mn[ai] = f; }
            if f > acc.mx[ai] { acc.mx[ai] = f; }
        }
    }

    // Group-by column headers from SELECT
    let grp_hdrs: Vec<String> = q.select.iter().filter_map(|s| match &s.expr {
        Expr::Agg(_, _) | Expr::Win(_, _) | Expr::Star => None,
        Expr::Col(n)     => Some(s.alias.clone().unwrap_or_else(|| short(n))),
        Expr::QCol(_, c) => Some(s.alias.clone().unwrap_or_else(|| c.clone())),
        _ => None,
    }).collect();
    let agg_hdrs: Vec<String> = agg_defs.iter().map(|a| a.hdr.clone()).collect();
    let headers: Vec<String> = grp_hdrs.into_iter().chain(agg_hdrs).collect();

    let result: Vec<Vec<String>> = order.iter().map(|k| {
        let acc = &map[k];
        let mut row: Vec<String> = acc.key_vals.iter().map(fmt_kv).collect();
        for (ai, ad) in agg_defs.iter().enumerate() {
            row.push(match ad.f {
                AggFn::Count => fmt_f64(acc.cnt[ai] as f64),
                AggFn::Sum   => fmt_f64(acc.sum[ai]),
                AggFn::Avg   => if acc.cnt[ai]>0 { fmt_f64(acc.sum[ai]/acc.cnt[ai] as f64) } else { "0".into() },
                AggFn::Min   => if acc.mn[ai]<f64::MAX { fmt_f64(acc.mn[ai]) } else { "0".into() },
                AggFn::Max   => if acc.mx[ai]>f64::MIN { fmt_f64(acc.mx[ai]) } else { "0".into() },
            });
        }
        row
    }).collect();

    Ok((headers, result))
}

// ── AGG only (no GROUP BY) ────────────────────────────────────────────────────

fn exec_agg_only(q: &Query, cols: &[String], rows: &[Vec<KVal>]) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let items: Vec<(AggFn, Option<usize>, String)> = q.select.iter().filter_map(|s| {
        if let Expr::Agg(f, col) = &s.expr {
            let ci = col.as_ref().and_then(|n| {
                let sn = short(n);
                cols.iter().position(|c| c.eq_ignore_ascii_case(n) || c.to_lowercase().ends_with(&format!(".{}", sn.to_lowercase())))
            });
            Some((f.clone(), ci, s.alias.clone().unwrap_or_else(|| agg_header(f, col))))
        } else { None }
    }).collect();

    let mut cnt = vec![0usize; items.len()];
    let mut sum = vec![0f64; items.len()];
    let mut mn  = vec![f64::MAX; items.len()];
    let mut mx  = vec![f64::MIN; items.len()];

    for row in rows {
        for (i, (_, ci, _)) in items.iter().enumerate() {
            let v = ci.and_then(|c| row.get(c)).unwrap_or(&KVal::Null);
            let f = kv_f64(v);
            cnt[i] += 1; sum[i] += f;
            if f < mn[i] { mn[i] = f; }
            if f > mx[i] { mx[i] = f; }
        }
    }

    let headers: Vec<String> = items.iter().map(|(_, _, h)| h.clone()).collect();
    let row: Vec<String> = items.iter().enumerate().map(|(i, (f, _, _))| match f {
        AggFn::Count => fmt_f64(cnt[i] as f64),
        AggFn::Sum   => fmt_f64(sum[i]),
        AggFn::Avg   => if cnt[i]>0 { fmt_f64(sum[i]/cnt[i] as f64) } else { "0".into() },
        AggFn::Min   => if mn[i]<f64::MAX { fmt_f64(mn[i]) } else { "0".into() },
        AggFn::Max   => if mx[i]>f64::MIN { fmt_f64(mx[i]) } else { "0".into() },
    }).collect();
    Ok((headers, vec![row]))
}

// ── HAVING ────────────────────────────────────────────────────────────────────

fn eval_having(expr: &Expr, hdrs: &[String], row: &[String]) -> bool {
    let get = |e: &Expr| -> String {
        match e {
            Expr::Col(n) | Expr::QCol(_, n) =>
                hdrs.iter().position(|h| h.eq_ignore_ascii_case(n))
                    .and_then(|i| row.get(i)).cloned().unwrap_or_default(),
            Expr::Agg(f, col) => {
                let h = agg_header(f, col);
                hdrs.iter().position(|hh| hh.eq_ignore_ascii_case(&h))
                    .and_then(|i| row.get(i)).cloned().unwrap_or_default()
            }
            Expr::Lit(KVal::Float(f)) => f.to_string(),
            Expr::Lit(KVal::Int(i))   => i.to_string(),
            Expr::Lit(KVal::Str(s))   => s.clone(),
            _ => String::new(),
        }
    };
    match expr {
        Expr::BinOp(l, op, r) => {
            let lv = get(l); let rv = get(r);
            let lf = lv.parse::<f64>().unwrap_or(0.0);
            let rf = rv.parse::<f64>().unwrap_or(0.0);
            match op { CmpOp::Eq=>lv==rv, CmpOp::Neq=>lv!=rv,
                       CmpOp::Lt=>lf<rf,  CmpOp::Lte=>lf<=rf,
                       CmpOp::Gt=>lf>rf,  CmpOp::Gte=>lf>=rf }
        }
        Expr::And(l, r) => eval_having(l, hdrs, row) && eval_having(r, hdrs, row),
        Expr::Or(l, r)  => eval_having(l, hdrs, row) || eval_having(r, hdrs, row),
        Expr::Not(e)    => !eval_having(e, hdrs, row),
        _ => true,
    }
}

// ── Window Functions ──────────────────────────────────────────────────────────

fn apply_windows(q: &Query, headers: &mut Vec<String>, rows: &mut Vec<Vec<String>>) {
    let win_items: Vec<(&SelItem, &WinFn, &WinSpec)> = q.select.iter().filter_map(|s| {
        if let Expr::Win(wf, ws) = &s.expr { Some((s, wf, ws)) } else { None }
    }).collect();

    for (si, wf, ws) in win_items {
        let col_name = si.alias.clone().unwrap_or_else(|| match wf {
            WinFn::RowNumber        => "ROW_NUMBER()".into(),
            WinFn::Rank             => "RANK()".into(),
            WinFn::DenseRank        => "DENSE_RANK()".into(),
            WinFn::Lag(c, _)        => format!("LAG({})", c),
            WinFn::Lead(c, _)       => format!("LEAD({})", c),
        });
        headers.push(col_name);

        let hl = headers.len() - 1;
        let part_ci: Vec<usize> = ws.partition_by.iter().map(|p| {
            let sn = short(p);
            headers[..hl].iter().position(|h| h.eq_ignore_ascii_case(p) || h.eq_ignore_ascii_case(&sn)).unwrap_or(usize::MAX)
        }).collect();
        let ord_ci: Vec<(usize, bool)> = ws.order_by.iter().map(|(o, asc)| {
            let sn = short(o);
            let ci = headers[..hl].iter().position(|h| h.eq_ignore_ascii_case(o) || h.eq_ignore_ascii_case(&sn)).unwrap_or(usize::MAX);
            (ci, *asc)
        }).collect();

        // Group row indices by partition key
        let mut parts: HashMap<String, Vec<usize>> = HashMap::new();
        for (ri, row) in rows.iter().enumerate() {
            let k: String = part_ci.iter().map(|&ci| row.get(ci).cloned().unwrap_or_default()).collect::<Vec<_>>().join("\x00");
            parts.entry(k).or_default().push(ri);
        }

        let mut win_vals: Vec<String> = vec![String::new(); rows.len()];

        for (_key, mut idxs) in parts {
            // Sort within partition by order_by
            if !ord_ci.is_empty() {
                let rows_ref: &Vec<Vec<String>> = rows;
                idxs.sort_by(|&a, &b| {
                    for &(ci, asc) in &ord_ci {
                        if ci == usize::MAX { continue; }
                        let av = rows_ref[a].get(ci).map(|s| s.as_str()).unwrap_or("");
                        let bv = rows_ref[b].get(ci).map(|s| s.as_str()).unwrap_or("");
                        let af = av.parse::<f64>().ok();
                        let bf = bv.parse::<f64>().ok();
                        let ord = match (af, bf) {
                            (Some(af), Some(bf)) => af.partial_cmp(&bf).unwrap_or(std::cmp::Ordering::Equal),
                            _ => av.cmp(bv),
                        };
                        let ord = if asc { ord } else { ord.reverse() };
                        if ord != std::cmp::Ordering::Equal { return ord; }
                    }
                    std::cmp::Ordering::Equal
                });
            }

            match wf {
                WinFn::RowNumber => {
                    for (rank, &ri) in idxs.iter().enumerate() { win_vals[ri] = (rank+1).to_string(); }
                }
                WinFn::Rank => {
                    let mut rank = 1usize;
                    let mut prev = String::new();
                    for (k, &ri) in idxs.iter().enumerate() {
                        let curr: String = ord_ci.iter().map(|&(ci,_)| rows[ri].get(ci).cloned().unwrap_or_default()).collect::<Vec<_>>().join("\x00");
                        if k > 0 && curr != prev { rank = k+1; }
                        win_vals[ri] = rank.to_string();
                        prev = curr;
                    }
                }
                WinFn::DenseRank => {
                    let mut rank = 1usize;
                    let mut prev = String::new();
                    for (k, &ri) in idxs.iter().enumerate() {
                        let curr: String = ord_ci.iter().map(|&(ci,_)| rows[ri].get(ci).cloned().unwrap_or_default()).collect::<Vec<_>>().join("\x00");
                        if k > 0 && curr != prev { rank += 1; }
                        win_vals[ri] = rank.to_string();
                        prev = curr;
                    }
                }
                WinFn::Lag(col, off) => {
                    let ci = headers[..headers.len()-1].iter().position(|h| h.eq_ignore_ascii_case(col)).unwrap_or(usize::MAX);
                    for (k, &ri) in idxs.iter().enumerate() {
                        win_vals[ri] = if k >= *off { rows[idxs[k-off]].get(ci).cloned().unwrap_or("NULL".into()) }
                                       else { "NULL".into() };
                    }
                }
                WinFn::Lead(col, off) => {
                    let ci = headers[..headers.len()-1].iter().position(|h| h.eq_ignore_ascii_case(col)).unwrap_or(usize::MAX);
                    for (k, &ri) in idxs.iter().enumerate() {
                        win_vals[ri] = if k+off < idxs.len() { rows[idxs[k+off]].get(ci).cloned().unwrap_or("NULL".into()) }
                                       else { "NULL".into() };
                    }
                }
            }
        }
        for (ri, row) in rows.iter_mut().enumerate() { row.push(win_vals[ri].clone()); }
    }
}

// ── ORDER BY ──────────────────────────────────────────────────────────────────

fn apply_order_by(order_by: &[(String, bool)], headers: &[String], rows: &mut Vec<Vec<String>>) {
    if order_by.is_empty() { return; }
    rows.sort_by(|a, b| {
        for (col, asc) in order_by {
            let ci = if let Ok(n) = col.parse::<usize>() { n.saturating_sub(1) }
                     else { headers.iter().position(|h| h.eq_ignore_ascii_case(col)).unwrap_or(usize::MAX) };
            if ci == usize::MAX { continue; }
            let av = a.get(ci).map(|s| s.as_str()).unwrap_or("");
            let bv = b.get(ci).map(|s| s.as_str()).unwrap_or("");
            let ord = match (av.parse::<f64>(), bv.parse::<f64>()) {
                (Ok(af), Ok(bf)) => af.partial_cmp(&bf).unwrap_or(std::cmp::Ordering::Equal),
                _ => av.cmp(bv),
            };
            let ord = if *asc { ord } else { ord.reverse() };
            if ord != std::cmp::Ordering::Equal { return ord; }
        }
        std::cmp::Ordering::Equal
    });
}

// ── Table renderer ────────────────────────────────────────────────────────────

fn render(hdrs: &[String], rows: &[Vec<String>]) -> String {
    if rows.is_empty() { return format!("  (no rows)\n  0 rows"); }
    let mut w: Vec<usize> = hdrs.iter().map(|h| h.len()).collect();
    for row in rows { for (i, c) in row.iter().enumerate() { if i<w.len() { w[i]=w[i].max(c.len()); } } }
    let sep: String = w.iter().map(|&ww| format!("+{}", "-".repeat(ww+2))).collect::<String>() + "+";
    let mut out = format!("{}\n", sep);
    out += &format!("| {} |\n", hdrs.iter().zip(&w).map(|(h,&ww)| format!("{:<ww$}", h, ww=ww)).collect::<Vec<_>>().join(" | "));
    out += &format!("{}\n", sep);
    for row in rows {
        out += &format!("| {} |\n", (0..hdrs.len()).map(|i| {
            let c = row.get(i).map(|s| s.as_str()).unwrap_or("");
            format!("{:<ww$}", c, ww=w[i])
        }).collect::<Vec<_>>().join(" | "));
    }
    out += &format!("{}\n", sep);
    out += &format!("  {} rows", rows.len());
    out
}

// ── Misc helpers ──────────────────────────────────────────────────────────────

fn short(name: &str) -> String {
    name.rfind('.').map(|i| name[i+1..].to_string()).unwrap_or_else(|| name.to_string())
}

fn agg_header(f: &AggFn, col: &Option<String>) -> String {
    match f {
        AggFn::Count => "COUNT(*)".into(),
        AggFn::Sum   => format!("SUM({})", col.as_deref().unwrap_or("*")),
        AggFn::Avg   => format!("AVG({})", col.as_deref().unwrap_or("*")),
        AggFn::Min   => format!("MIN({})", col.as_deref().unwrap_or("*")),
        AggFn::Max   => format!("MAX({})", col.as_deref().unwrap_or("*")),
    }
}
