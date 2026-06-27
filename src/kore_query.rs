/// KORE ∞ Layer 5 — KoreQuery: Pure Rust SQL Engine
///
/// Run SQL queries directly on .kore files — no external deps, no DuckDB,
/// no Pandas. Zero-copy columnar execution in pure Rust.
///
/// Supported SQL:
///   SELECT col1, col2, COUNT(*), SUM(col), AVG(col), MIN(col), MAX(col)
///   FROM "file.kore"
///   WHERE col > 100 AND col2 = 'value'
///   GROUP BY col
///   ORDER BY col ASC|DESC
///   LIMIT N

use crate::kore_v2::{KoreReader, KVal};

// --- Token -------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Select, From, Where, Group, By, Order, Asc, Desc, Limit,
    And, Or, Not, As,
    Star, Comma,
    Eq, Neq, Lt, Lte, Gt, Gte,
    LParen, RParen,
    Ident(String), Str(String), Num(f64),
    Count, Sum, Avg, Min, Max,
}

fn tokenize(sql: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = sql.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' | '\n' | '\r' => { i += 1; }
            '*'  => { tokens.push(Token::Star);   i += 1; }
            ','  => { tokens.push(Token::Comma);  i += 1; }
            '('  => { tokens.push(Token::LParen); i += 1; }
            ')'  => { tokens.push(Token::RParen); i += 1; }
            '='  => { tokens.push(Token::Eq);     i += 1; }
            '<'  => {
                if i+1 < chars.len() && chars[i+1] == '=' { tokens.push(Token::Lte); i += 2; }
                else if i+1 < chars.len() && chars[i+1] == '>' { tokens.push(Token::Neq); i += 2; }
                else { tokens.push(Token::Lt); i += 1; }
            }
            '>' => {
                if i+1 < chars.len() && chars[i+1] == '=' { tokens.push(Token::Gte); i += 2; }
                else { tokens.push(Token::Gt); i += 1; }
            }
            '!' => {
                if i+1 < chars.len() && chars[i+1] == '=' { tokens.push(Token::Neq); i += 2; }
                else { i += 1; }
            }
            '\'' | '"' => {
                let q = chars[i]; i += 1;
                let mut s = String::new();
                while i < chars.len() && chars[i] != q { s.push(chars[i]); i += 1; }
                i += 1;
                tokens.push(Token::Str(s));
            }
            c if c.is_ascii_digit() || (c == '-' && i+1 < chars.len() && chars[i+1].is_ascii_digit()) => {
                let mut s = String::new();
                if c == '-' { s.push(c); i += 1; }
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    s.push(chars[i]); i += 1;
                }
                tokens.push(Token::Num(s.parse().unwrap_or(0.0)));
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut s = String::new();
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.') {
                    s.push(chars[i]); i += 1;
                }
                tokens.push(match s.to_uppercase().as_str() {
                    "SELECT" => Token::Select, "FROM"  => Token::From,
                    "WHERE"  => Token::Where,  "GROUP" => Token::Group,
                    "BY"     => Token::By,     "ORDER" => Token::Order,
                    "ASC"    => Token::Asc,    "DESC"  => Token::Desc,
                    "LIMIT"  => Token::Limit,  "AND"   => Token::And,
                    "OR"     => Token::Or,     "NOT"   => Token::Not,
                    "AS"     => Token::As,     "COUNT" => Token::Count,
                    "SUM"    => Token::Sum,    "AVG"   => Token::Avg,
                    "MIN"    => Token::Min,    "MAX"   => Token::Max,
                    _        => Token::Ident(s),
                });
            }
            _ => { i += 1; }
        }
    }
    tokens
}

// --- AST ---------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Expr {
    Column(String),
    Literal(KVal),
    Agg(AggFn, Option<String>),
    BinOp(Box<Expr>, CmpOp, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Star,
}

#[derive(Debug, Clone, PartialEq)]
enum AggFn { Count, Sum, Avg, Min, Max }

#[derive(Debug, Clone, PartialEq)]
enum CmpOp { Eq, Neq, Lt, Lte, Gt, Gte }

#[derive(Debug, Clone)]
struct SelectItem { expr: Expr, alias: Option<String> }

#[derive(Debug)]
struct Query {
    select:   Vec<SelectItem>,
    from:     String,
    where_:   Option<Expr>,
    group_by: Vec<String>,
    order_by: Vec<(String, bool)>,
    limit:    Option<usize>,
}

// --- Parser ------------------------------------------------------------------

struct Parser { tokens: Vec<Token>, pos: usize }

impl Parser {
    fn new(t: Vec<Token>) -> Self { Parser { tokens: t, pos: 0 } }
    fn peek(&self) -> Option<&Token> { self.tokens.get(self.pos) }
    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned(); self.pos += 1; t
    }
    fn eat(&mut self, t: &Token) -> bool {
        if self.peek() == Some(t) { self.pos += 1; true } else { false }
    }

    fn parse(&mut self) -> Result<Query, String> {
        self.eat(&Token::Select);
        let mut select = Vec::new();
        loop {
            select.push(self.parse_select_item()?);
            if !self.eat(&Token::Comma) { break; }
        }
        if !self.eat(&Token::From) { return Err("Expected FROM".into()); }
        let from = match self.next() {
            Some(Token::Str(s)) | Some(Token::Ident(s)) => s,
            _ => return Err("Expected table name after FROM".into()),
        };
        let where_ = if self.peek() == Some(&Token::Where) { self.next(); Some(self.parse_expr()?) } else { None };
        let mut group_by = Vec::new();
        if self.peek() == Some(&Token::Group) {
            self.next(); self.eat(&Token::By);
            loop {
                match self.next() { Some(Token::Ident(c)) => group_by.push(c), _ => break }
                if !self.eat(&Token::Comma) { break; }
            }
        }
        let mut order_by = Vec::new();
        if self.peek() == Some(&Token::Order) {
            self.next(); self.eat(&Token::By);
            loop {
                match self.next() {
                    Some(Token::Ident(c)) => {
                        let asc = self.peek() != Some(&Token::Desc);
                        if !asc { self.next(); } else if self.peek() == Some(&Token::Asc) { self.next(); }
                        order_by.push((c, asc));
                    }
                    _ => break,
                }
                if !self.eat(&Token::Comma) { break; }
            }
        }
        let limit = if self.peek() == Some(&Token::Limit) {
            self.next(); match self.next() { Some(Token::Num(n)) => Some(n as usize), _ => None }
        } else { None };
        Ok(Query { select, from, where_, group_by, order_by, limit })
    }

    fn parse_select_item(&mut self) -> Result<SelectItem, String> {
        let expr = self.parse_expr_or_agg()?;
        let alias = if self.peek() == Some(&Token::As) {
            self.next(); match self.next() { Some(Token::Ident(s)) => Some(s), _ => None }
        } else { None };
        Ok(SelectItem { expr, alias })
    }

    fn parse_expr_or_agg(&mut self) -> Result<Expr, String> {
        match self.peek().cloned() {
            Some(Token::Star)  => { self.next(); Ok(Expr::Star) }
            Some(Token::Count) => { self.next(); self.parse_agg(AggFn::Count) }
            Some(Token::Sum)   => { self.next(); self.parse_agg(AggFn::Sum) }
            Some(Token::Avg)   => { self.next(); self.parse_agg(AggFn::Avg) }
            Some(Token::Min)   => { self.next(); self.parse_agg(AggFn::Min) }
            Some(Token::Max)   => { self.next(); self.parse_agg(AggFn::Max) }
            _ => self.parse_expr(),
        }
    }

    fn parse_agg(&mut self, f: AggFn) -> Result<Expr, String> {
        self.eat(&Token::LParen);
        let col = match self.next() {
            Some(Token::Star) => None, Some(Token::Ident(c)) => Some(c), _ => None,
        };
        self.eat(&Token::RParen);
        Ok(Expr::Agg(f, col))
    }

    fn parse_expr(&mut self) -> Result<Expr, String> {
        let l = self.parse_and()?;
        if self.peek() == Some(&Token::Or) { self.next(); let r = self.parse_and()?; Ok(Expr::Or(Box::new(l), Box::new(r))) }
        else { Ok(l) }
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let l = self.parse_cmp()?;
        if self.peek() == Some(&Token::And) { self.next(); let r = self.parse_cmp()?; Ok(Expr::And(Box::new(l), Box::new(r))) }
        else { Ok(l) }
    }

    fn parse_cmp(&mut self) -> Result<Expr, String> {
        if self.peek() == Some(&Token::Not) { self.next(); let e = self.parse_primary()?; return Ok(Expr::Not(Box::new(e))); }
        let l = self.parse_primary()?;
        let op = match self.peek() {
            Some(Token::Eq) => CmpOp::Eq, Some(Token::Neq) => CmpOp::Neq,
            Some(Token::Lt) => CmpOp::Lt, Some(Token::Lte) => CmpOp::Lte,
            Some(Token::Gt) => CmpOp::Gt, Some(Token::Gte) => CmpOp::Gte,
            _ => return Ok(l),
        };
        self.next();
        let r = self.parse_primary()?;
        Ok(Expr::BinOp(Box::new(l), op, Box::new(r)))
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Token::Ident(s)) => Ok(Expr::Column(s)),
            Some(Token::Str(s))   => Ok(Expr::Literal(KVal::Str(s))),
            Some(Token::Num(n))   => Ok(Expr::Literal(KVal::Float(n))),
            Some(Token::LParen)   => { let e = self.parse_expr()?; self.eat(&Token::RParen); Ok(e) }
            t => Err(format!("Unexpected: {:?}", t)),
        }
    }
}

// --- Evaluation helpers ------------------------------------------------------

fn eval_filter(expr: &Expr, row: &[(&str, &KVal)]) -> bool {
    match expr {
        Expr::BinOp(l, op, r) => {
            let lv = eval_val(l, row); let rv = eval_val(r, row);
            match op {
                CmpOp::Eq  => kval_eq(&lv, &rv),   CmpOp::Neq => !kval_eq(&lv, &rv),
                CmpOp::Lt  => kval_cmp(&lv, &rv) < 0, CmpOp::Lte => kval_cmp(&lv, &rv) <= 0,
                CmpOp::Gt  => kval_cmp(&lv, &rv) > 0, CmpOp::Gte => kval_cmp(&lv, &rv) >= 0,
            }
        }
        Expr::And(l, r) => eval_filter(l, row) && eval_filter(r, row),
        Expr::Or(l, r)  => eval_filter(l, row) || eval_filter(r, row),
        Expr::Not(e)    => !eval_filter(e, row),
        _ => true,
    }
}

fn eval_val(expr: &Expr, row: &[(&str, &KVal)]) -> KVal {
    match expr {
        Expr::Column(name) => row.iter().find(|(n,_)| n.eq_ignore_ascii_case(name))
            .map(|(_,v)| (*v).clone()).unwrap_or(KVal::Null),
        Expr::Literal(v) => v.clone(),
        _ => KVal::Null,
    }
}

fn kval_eq(a: &KVal, b: &KVal) -> bool {
    match (a, b) {
        (KVal::Int(x), KVal::Int(y))     => x == y,
        (KVal::Float(x), KVal::Float(y)) => (x - y).abs() < 1e-9,
        (KVal::Int(x), KVal::Float(y))   => (*x as f64 - y).abs() < 1e-9,
        (KVal::Float(x), KVal::Int(y))   => (x - *y as f64).abs() < 1e-9,
        (KVal::Str(x), KVal::Str(y))     => x.eq_ignore_ascii_case(y),
        (KVal::Str(s), KVal::Float(y))   => s.parse::<f64>().map(|x| (x - y).abs() < 1e-9).unwrap_or(false),
        (KVal::Float(x), KVal::Str(s))   => s.parse::<f64>().map(|y| (x - y).abs() < 1e-9).unwrap_or(false),
        (KVal::Str(s), KVal::Int(y))     => s.parse::<i64>().map(|x| x == *y).unwrap_or(false),
        (KVal::Int(x), KVal::Str(s))     => s.parse::<i64>().map(|y| *x == y).unwrap_or(false),
        (KVal::Bool(x), KVal::Bool(y))   => x == y,
        (KVal::Null, KVal::Null)         => true,
        _ => false,
    }
}

fn kval_cmp(a: &KVal, b: &KVal) -> i32 {
    let af = match a { KVal::Int(x) => *x as f64, KVal::Float(x) => *x, KVal::Str(s) => s.parse::<f64>().unwrap_or(0.0), _ => 0.0 };
    let bf = match b { KVal::Int(x) => *x as f64, KVal::Float(x) => *x, KVal::Str(s) => s.parse::<f64>().unwrap_or(0.0), _ => 0.0 };
    if af < bf { -1 } else if af > bf { 1 } else { 0 }
}

fn kval_f64(v: &KVal) -> f64 {
    match v { KVal::Int(x) => *x as f64, KVal::Float(x) => *x, KVal::Str(s) => s.parse::<f64>().unwrap_or(0.0), _ => 0.0 }
}

fn fmt_num(f: f64) -> String {
    if f == f.floor() && f.abs() < 1e15 { format!("{}", f as i64) }
    else { format!("{:.4}", f).trim_end_matches('0').trim_end_matches('.').to_string() }
}

// --- KoreQuery ---------------------------------------------------------------

pub struct KoreQuery { path: String }

impl KoreQuery {
    pub fn new(path: &str) -> Self { KoreQuery { path: path.to_string() } }

    pub fn sql(&self, query: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
        let mut parser = Parser::new(tokenize(query));
        let q = parser.parse()?;

        let file_path = if q.from.ends_with(".kore") { &q.from } else { &self.path };
        let reader = KoreReader::open(file_path)?;
        let all_cols = reader.read_all_columns();
        let col_names: Vec<&str> = reader.columns.iter().map(|c| c.name.as_str()).collect();

        let has_agg = q.select.iter().any(|s| matches!(s.expr, Expr::Agg(_, _)));

        if has_agg && !q.group_by.is_empty() { self.exec_group_agg(&q, &col_names, &all_cols) }
        else if has_agg                       { self.exec_agg_only(&q, &col_names, &all_cols) }
        else                                  { self.exec_select(&q, &col_names, &all_cols) }
    }

    fn exec_select(&self, q: &Query, col_names: &[&str], all_cols: &[Vec<KVal>]) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
        let nrows = all_cols.first().map(|c| c.len()).unwrap_or(0);
        let out_cols: Vec<(String, usize)> = if q.select.len() == 1 && matches!(q.select[0].expr, Expr::Star) {
            col_names.iter().enumerate().map(|(i,n)| (n.to_string(), i)).collect()
        } else {
            q.select.iter().filter_map(|s| {
                if let Expr::Column(ref name) = s.expr {
                    let alias = s.alias.clone().unwrap_or_else(|| name.clone());
                    col_names.iter().position(|n| n.eq_ignore_ascii_case(name)).map(|idx| (alias, idx))
                } else { None }
            }).collect()
        };
        let headers: Vec<String> = out_cols.iter().map(|(h,_)| h.clone()).collect();
        let mut rows: Vec<Vec<String>> = Vec::new();

        for row_i in 0..nrows {
            let ctx: Vec<(&str,&KVal)> = col_names.iter().enumerate()
                .filter_map(|(ci,n)| all_cols.get(ci)?.get(row_i).map(|v| (*n,v))).collect();
            if let Some(ref w) = q.where_ { if !eval_filter(w, &ctx) { continue; } }
            rows.push(out_cols.iter().map(|(_,ci)| all_cols.get(*ci).and_then(|c|c.get(row_i)).map(|v|v.display()).unwrap_or_default()).collect());
        }

        for (col, asc) in q.order_by.iter().rev() {
            if let Some(ci) = headers.iter().position(|h| h.eq_ignore_ascii_case(col)) {
                rows.sort_by(|a,b| { let o=a[ci].partial_cmp(&b[ci]).unwrap_or(std::cmp::Ordering::Equal); if *asc{o}else{o.reverse()} });
            }
        }
        if let Some(n) = q.limit { rows.truncate(n); }
        Ok((headers, rows))
    }

    fn exec_agg_only(&self, q: &Query, col_names: &[&str], all_cols: &[Vec<KVal>]) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
        let nrows = all_cols.first().map(|c| c.len()).unwrap_or(0);
        let items: Vec<(AggFn, Option<usize>, String)> = q.select.iter().filter_map(|s| {
            if let Expr::Agg(f, col) = &s.expr {
                let ci = col.as_ref().and_then(|n| col_names.iter().position(|c| c.eq_ignore_ascii_case(n)));
                let h = s.alias.clone().unwrap_or_else(|| match f {
                    AggFn::Count=>"COUNT(*)".into(), AggFn::Sum=>format!("SUM({})",col.as_deref().unwrap_or("*")),
                    AggFn::Avg=>format!("AVG({})",col.as_deref().unwrap_or("*")), AggFn::Min=>format!("MIN({})",col.as_deref().unwrap_or("*")),
                    AggFn::Max=>format!("MAX({})",col.as_deref().unwrap_or("*")),
                });
                Some((f.clone(), ci, h))
            } else { None }
        }).collect();

        let mut cnt = vec![0usize; items.len()];
        let mut sum = vec![0f64; items.len()];
        let mut mn  = vec![f64::MAX; items.len()];
        let mut mx  = vec![f64::MIN; items.len()];

        for row_i in 0..nrows {
            let ctx: Vec<(&str,&KVal)> = col_names.iter().enumerate()
                .filter_map(|(ci,n)| all_cols.get(ci)?.get(row_i).map(|v|(*n,v))).collect();
            if let Some(ref w) = q.where_ { if !eval_filter(w, &ctx) { continue; } }
            for (ai,(f,ci,_)) in items.iter().enumerate() {
                let v = ci.and_then(|idx| all_cols.get(idx)?.get(row_i));
                let n = v.map(kval_f64).unwrap_or(0.0);
                match f {
                    AggFn::Count => cnt[ai]+=1,
                    AggFn::Sum   => { sum[ai]+=n; cnt[ai]+=1; }
                    AggFn::Avg   => { sum[ai]+=n; cnt[ai]+=1; }
                    AggFn::Min   => { if n<mn[ai]{mn[ai]=n;} cnt[ai]+=1; }
                    AggFn::Max   => { if n>mx[ai]{mx[ai]=n;} cnt[ai]+=1; }
                }
            }
        }
        let headers: Vec<String> = items.iter().map(|(_,_,h)| h.clone()).collect();
        let row: Vec<String> = items.iter().enumerate().map(|(ai,(f,_,_))| match f {
            AggFn::Count => cnt[ai].to_string(),
            AggFn::Sum   => fmt_num(sum[ai]),
            AggFn::Avg   => fmt_num(if cnt[ai]>0{sum[ai]/cnt[ai] as f64}else{0.0}),
            AggFn::Min   => fmt_num(if mn[ai]==f64::MAX{0.0}else{mn[ai]}),
            AggFn::Max   => fmt_num(if mx[ai]==f64::MIN{0.0}else{mx[ai]}),
        }).collect();
        Ok((headers, vec![row]))
    }

    fn exec_group_agg(&self, q: &Query, col_names: &[&str], all_cols: &[Vec<KVal>]) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
        use std::collections::HashMap;
        let nrows = all_cols.first().map(|c| c.len()).unwrap_or(0);
        let gb_idx: Vec<usize> = q.group_by.iter()
            .filter_map(|n| col_names.iter().position(|c| c.eq_ignore_ascii_case(n))).collect();

        let agg_items: Vec<(AggFn, Option<usize>, String)> = q.select.iter().filter_map(|s| {
            if let Expr::Agg(f, col) = &s.expr {
                let ci = col.as_ref().and_then(|n| col_names.iter().position(|c| c.eq_ignore_ascii_case(n)));
                let h = s.alias.clone().unwrap_or_else(|| match f {
                    AggFn::Count=>"COUNT(*)".into(), AggFn::Sum=>format!("SUM({})",col.as_deref().unwrap_or("*")),
                    AggFn::Avg=>format!("AVG({})",col.as_deref().unwrap_or("*")), AggFn::Min=>format!("MIN({})",col.as_deref().unwrap_or("*")),
                    AggFn::Max=>format!("MAX({})",col.as_deref().unwrap_or("*")),
                });
                Some((f.clone(), ci, h))
            } else { None }
        }).collect();

        let mut groups: HashMap<Vec<String>, Vec<(usize,f64,f64,f64)>> = HashMap::new();

        for row_i in 0..nrows {
            let ctx: Vec<(&str,&KVal)> = col_names.iter().enumerate()
                .filter_map(|(ci,n)| all_cols.get(ci)?.get(row_i).map(|v|(*n,v))).collect();
            if let Some(ref w) = q.where_ { if !eval_filter(w, &ctx) { continue; } }
            let key: Vec<String> = gb_idx.iter()
                .map(|&ci| all_cols.get(ci).and_then(|c|c.get(row_i)).map(|v|v.display()).unwrap_or_default()).collect();
            let e = groups.entry(key).or_insert_with(|| vec![(0,0.0,f64::MAX,f64::MIN); agg_items.len()]);
            for (ai,(f,ci,_)) in agg_items.iter().enumerate() {
                let v = ci.and_then(|idx| all_cols.get(idx)?.get(row_i));
                let n = v.map(kval_f64).unwrap_or(0.0);
                match f {
                    AggFn::Count => e[ai].0+=1,
                    AggFn::Sum   => { e[ai].1+=n; e[ai].0+=1; }
                    AggFn::Avg   => { e[ai].1+=n; e[ai].0+=1; }
                    AggFn::Min   => { if n<e[ai].2{e[ai].2=n;} e[ai].0+=1; }
                    AggFn::Max   => { if n>e[ai].3{e[ai].3=n;} e[ai].0+=1; }
                }
            }
        }

        let mut headers: Vec<String> = q.group_by.clone();
        for (_,_,h) in &agg_items { headers.push(h.clone()); }

        let mut rows: Vec<Vec<String>> = groups.into_iter().map(|(key, accum)| {
            let mut row = key;
            for (ai,(f,_,_)) in agg_items.iter().enumerate() {
                let (cnt,sum,mn,mx) = accum[ai];
                row.push(match f {
                    AggFn::Count => cnt.to_string(),
                    AggFn::Sum   => fmt_num(sum),
                    AggFn::Avg   => fmt_num(if cnt>0{sum/cnt as f64}else{0.0}),
                    AggFn::Min   => fmt_num(if mn==f64::MAX{0.0}else{mn}),
                    AggFn::Max   => fmt_num(if mx==f64::MIN{0.0}else{mx}),
                });
            }
            row
        }).collect();

        for (col,asc) in q.order_by.iter().rev() {
            if let Some(ci) = headers.iter().position(|h| h.eq_ignore_ascii_case(col)) {
                rows.sort_by(|a,b| { let o=a[ci].partial_cmp(&b[ci]).unwrap_or(std::cmp::Ordering::Equal); if *asc{o}else{o.reverse()} });
            }
        }
        if let Some(n) = q.limit { rows.truncate(n); }
        Ok((headers, rows))
    }

    /// Pretty-print results as ASCII table
    pub fn table(&self, query: &str) -> String {
        match self.sql(query) {
            Err(e) => format!("ERROR: {}", e),
            Ok((headers, rows)) => {
                if rows.is_empty() { return "  (no rows)".into(); }
                let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
                for row in &rows {
                    for (i,cell) in row.iter().enumerate() {
                        if i < widths.len() { widths[i] = widths[i].max(cell.len()); }
                    }
                }
                let bar: String = widths.iter().map(|w| "-".repeat(w+2)).collect::<Vec<_>>().join("+");
                let header = headers.iter().enumerate().map(|(i,h)| format!(" {:w$} ", h, w=widths[i])).collect::<Vec<_>>().join("|");
                let body = rows.iter().map(|row| {
                    format!("|{}|", row.iter().enumerate().map(|(i,c)| format!(" {:w$} ", c, w=widths.get(i).copied().unwrap_or(0))).collect::<Vec<_>>().join("|"))
                }).collect::<Vec<_>>().join("\n");
                format!("+{}+\n|{}|\n+{}+\n{}\n+{}+\n  {} row{}", bar, header, bar, body, bar, rows.len(), if rows.len()==1{""} else{"s"})
            }
        }
    }
}
