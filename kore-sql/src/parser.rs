//! KQL recursive-descent parser.
//!
//! Turns a token stream into a `SelectStmt` AST.

use crate::ast::*;
use crate::lexer::{Lexer, Token};
use kore_core::KoreError;

pub fn parse(sql: &str) -> Result<SelectStmt, KoreError> {
    parse_query(sql)?.body
        .ok_or_else(|| KoreError::InvalidArgument("empty query".into()))
}

/// Parse a full query: WITH ..., SELECT ..., UNION ALL SELECT ...
pub fn parse_query(sql: &str) -> Result<Query, KoreError> {
    let mut lexer = Lexer::new(sql);
    let tokens = lexer.tokenize()?;
    let mut p = Parser::new(tokens);

    // WITH clause (CTEs)
    let ctes = if p.peek() == &Token::With {
        p.pos += 1;
        p.parse_cte_list()?
    } else { vec![] };

    // Main SELECT
    let body = Some(p.parse_select()?);

    // UNION ALL / UNION
    let mut union_all = vec![];
    while p.peek() == &Token::Union {
        p.pos += 1;
        p.consume_if(&Token::All);   // UNION ALL or UNION (dedup not implemented)
        union_all.push(p.parse_select()?);
    }

    Ok(Query { ctes, body, union_all })
}

struct Parser {
    tokens: Vec<Token>,
    pos:    usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self { Self { tokens, pos: 0 } }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }
    fn peek2(&self) -> &Token {
        self.tokens.get(self.pos + 1).unwrap_or(&Token::Eof)
    }
    fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        t
    }
    fn expect(&mut self, expected: &Token) -> Result<(), KoreError> {
        if self.peek() == expected {
            self.pos += 1;
            Ok(())
        } else {
            Err(KoreError::InvalidArgument(format!("expected {:?}, got {:?}", expected, self.peek())))
        }
    }
    fn expect_ident(&mut self) -> Result<String, KoreError> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            other => Err(KoreError::InvalidArgument(format!("expected identifier, got {:?}", other))),
        }
    }

    /// Like expect_ident but also accepts SQL keywords as alias names (e.g. AVG, COUNT, avg).
    /// Used after AS keyword in projections and CTEs.
    fn expect_alias(&mut self) -> Result<String, KoreError> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            // Allow common keywords used as alias names
            Token::Avg       => Ok("avg".to_string()),
            Token::Count     => Ok("count".to_string()),
            Token::Sum       => Ok("sum".to_string()),
            Token::Min       => Ok("min".to_string()),
            Token::Max       => Ok("max".to_string()),
            Token::Group     => Ok("group".to_string()),
            Token::Order     => Ok("order".to_string()),
            Token::From      => Ok("from".to_string()),
            Token::Where     => Ok("where".to_string()),
            Token::Asc       => Ok("asc".to_string()),
            Token::Desc      => Ok("desc".to_string()),
            Token::Distinct  => Ok("distinct".to_string()),
            other => Err(KoreError::InvalidArgument(format!("expected alias name, got {:?}", other))),
        }
    }
    fn consume_if(&mut self, tok: &Token) -> bool {
        if self.peek() == tok { self.pos += 1; true } else { false }
    }

    // ─── SELECT statement ──────────────────────────────────────────────────

    fn parse_select(&mut self) -> Result<SelectStmt, KoreError> {
        self.expect(&Token::Select)?;
        let distinct = self.consume_if(&Token::Distinct);

        // projections
        let projections = self.parse_projections()?;

        // FROM
        self.expect(&Token::From)?;
        let from = self.parse_table_expr()?;

        // JOINs
        let mut joins = Vec::new();
        while self.is_join_keyword() {
            joins.push(self.parse_join()?);
        }

        // WHERE
        let where_clause = if self.consume_if(&Token::Where) {
            Some(self.parse_expr(0)?)
        } else {
            None
        };

        // GROUP BY
        let group_by = if self.peek() == &Token::Group && self.peek2() == &Token::By {
            self.pos += 2; // consume GROUP BY
            self.parse_ident_list()?
        } else {
            Vec::new()
        };

        // HAVING
        let having = if self.consume_if(&Token::Having) {
            Some(self.parse_expr(0)?)
        } else {
            None
        };

        // ORDER BY
        let order_by = if self.peek() == &Token::Order && self.peek2() == &Token::By {
            self.pos += 2;
            self.parse_order_by_list()?
        } else {
            Vec::new()
        };

        // LIMIT
        let limit = if self.consume_if(&Token::Limit) {
            match self.advance() {
                Token::Int(n) => Some(n as u64),
                other => return Err(KoreError::InvalidArgument(format!("LIMIT expects integer, got {:?}", other))),
            }
        } else {
            None
        };

        Ok(SelectStmt { distinct, projections, from, joins, where_clause,
                         group_by, having, order_by, limit })
    }

    // ── Window spec helpers ────────────────────────────────────────────────

    /// Parse list of CTEs: `name AS (select), ...`
    pub fn parse_cte_list(&mut self) -> Result<Vec<CteClause>, KoreError> {
        let mut ctes = vec![];
        loop {
            let name = self.expect_ident()?;
            self.expect(&Token::As)?;
            self.expect(&Token::LParen)?;
            let body = self.parse_select()?;
            self.expect(&Token::RParen)?;
            ctes.push(CteClause { name, body });
            if !self.consume_if(&Token::Comma) { break; }
        }
        Ok(ctes)
    }

    fn maybe_window(&mut self, agg: Expr, func: AggFunc) -> Result<Expr, KoreError> {
        if self.peek() != &Token::Over { return Ok(agg); }
        self.pos += 1;
        let spec  = self.parse_window_spec()?;
        let inner = match &agg {
            Expr::Agg { expr, .. } => expr.as_ref().clone(),
            _ => agg.clone(),
        };
        Ok(Expr::Window { func: WindowFn::Agg { func, expr: Box::new(inner) }, spec })
    }

    fn parse_window_spec(&mut self) -> Result<WindowSpec, KoreError> {
        self.expect(&Token::LParen)?;
        let mut spec = WindowSpec::default();
        if self.peek() == &Token::Partition {
            self.pos += 1;
            self.expect(&Token::By)?;
            spec.partition_by.push(self.parse_expr(0)?);
            while self.consume_if(&Token::Comma) { spec.partition_by.push(self.parse_expr(0)?); }
        }
        if self.peek() == &Token::Order {
            self.pos += 1;
            self.expect(&Token::By)?;
            spec.order_by = self.parse_order_by_list()?;
        }
        if matches!(self.peek(), Token::Rows | Token::Range) {
            let mode = if self.peek() == &Token::Rows { self.pos += 1; FrameMode::Rows }
                       else { self.pos += 1; FrameMode::Range };
            if let Token::Ident(s) = self.peek() { if s.eq_ignore_ascii_case("BETWEEN") { self.pos += 1; } }
            let start = self.parse_frame_bound()?;
            if self.peek() == &Token::And { self.pos += 1; }
            let end = self.parse_frame_bound()?;
            spec.frame = Some(WindowFrame { mode, start, end });
        }
        self.expect(&Token::RParen)?;
        Ok(spec)
    }

    fn parse_frame_bound(&mut self) -> Result<FrameBound, KoreError> {
        match self.peek() {
            Token::Unbounded => {
                self.pos += 1;
                Ok(if self.peek() == &Token::Preceding { self.pos += 1; FrameBound::UnboundedPreceding }
                   else { self.consume_if(&Token::Following); FrameBound::UnboundedFollowing })
            }
            Token::Current => {
                self.pos += 1;
                if let Token::Ident(s) = self.peek() { if s.eq_ignore_ascii_case("ROW") { self.pos += 1; } }
                Ok(FrameBound::CurrentRow)
            }
            _ => {
                let n = self.parse_expr(0)?;
                Ok(if self.peek() == &Token::Preceding { self.pos += 1; FrameBound::Preceding(Box::new(n)) }
                   else { self.consume_if(&Token::Following); FrameBound::Following(Box::new(n)) })
            }
        }
    }

    fn parse_window_fn_args(&mut self, name: &str) -> Result<WindowFn, KoreError> {
        Ok(match name.to_ascii_uppercase().as_str() {
            "ROW_NUMBER" => WindowFn::RowNumber,
            "RANK"       => WindowFn::Rank,
            "DENSE_RANK" => WindowFn::DenseRank,
            "NTILE"      => WindowFn::Ntile(Box::new(self.parse_expr(0)?)),
            "LAG"  => { let e = self.parse_expr(0)?;
                        let o = if self.consume_if(&Token::Comma) { self.parse_expr(0)? } else { Expr::Int(1) };
                        WindowFn::Lag  { expr: Box::new(e), offset: Box::new(o) } }
            "LEAD" => { let e = self.parse_expr(0)?;
                        let o = if self.consume_if(&Token::Comma) { self.parse_expr(0)? } else { Expr::Int(1) };
                        WindowFn::Lead { expr: Box::new(e), offset: Box::new(o) } }
            "FIRST_VALUE" => WindowFn::FirstValue(Box::new(self.parse_expr(0)?)),
            "LAST_VALUE"  => WindowFn::LastValue (Box::new(self.parse_expr(0)?)),
            "CUMSUM"|"CUM_SUM" => WindowFn::CumSum(Box::new(self.parse_expr(0)?)),
            other => return Err(KoreError::InvalidArgument(format!("unknown window fn: {other}"))),
        })
    }

    fn is_join_keyword(&self) -> bool {
        matches!(self.peek(),
            Token::Join | Token::Inner | Token::Left | Token::Right | Token::Full)
    }

    // ─── Projections ───────────────────────────────────────────────────────

    fn parse_projections(&mut self) -> Result<Vec<Projection>, KoreError> {
        let mut projs = vec![self.parse_one_projection()?];
        while self.consume_if(&Token::Comma) {
            projs.push(self.parse_one_projection()?);
        }
        Ok(projs)
    }

    fn parse_one_projection(&mut self) -> Result<Projection, KoreError> {
        if self.peek() == &Token::Star {
            self.pos += 1;
            return Ok(Projection::Star);
        }
        let expr = self.parse_expr(0)?;
        let alias = if self.consume_if(&Token::As) {
            Some(self.expect_alias()?)
        } else if matches!(self.peek(), Token::Ident(_)) {
            Some(self.expect_ident()?)
        } else if matches!(self.peek(),
            Token::Avg | Token::Count | Token::Sum | Token::Min | Token::Max |
            Token::Asc | Token::Desc | Token::Group | Token::Order | Token::Where |
            Token::Distinct
        ) && !matches!(self.peek(), Token::From) {
            // Keyword used as implicit alias without AS (e.g. SELECT AVG(x) avg ...)
            // Only consume if it looks like an alias (not a clause keyword)
            let next_tok = self.peek().clone();
            match next_tok {
                // These can be aliases
                Token::Avg | Token::Count | Token::Sum | Token::Min | Token::Max |
                Token::Asc | Token::Desc | Token::Distinct => Some(self.expect_alias()?),
                // These could be aliases but are risky — only take if followed by comma or FROM
                _ => None,
            }
        } else {
            None
        };
        Ok(Projection::Expr { expr, alias })
    }

    // ─── Table reference ───────────────────────────────────────────────────

    fn parse_table_expr(&mut self) -> Result<TableExpr, KoreError> {
        let name = self.expect_ident()?;
        let alias = if self.consume_if(&Token::As) {
            Some(self.expect_ident()?)
        } else if matches!(self.peek(), Token::Ident(_)) && !self.is_join_keyword()
               && self.peek() != &Token::Where
               && self.peek() != &Token::Order
               && self.peek() != &Token::Group
               && self.peek() != &Token::Limit {
            Some(self.expect_ident()?)
        } else {
            None
        };
        Ok(TableExpr { name, alias })
    }

    // ─── JOIN clause ───────────────────────────────────────────────────────

    fn parse_join(&mut self) -> Result<JoinClause, KoreError> {
        let join_type = match self.peek() {
            Token::Inner => { self.pos += 1; self.expect(&Token::Join)?; JoinKind::Inner }
            Token::Left  => {
                self.pos += 1;
                self.consume_if(&Token::Outer);
                self.expect(&Token::Join)?;
                JoinKind::Left
            }
            Token::Right => {
                self.pos += 1;
                self.consume_if(&Token::Outer);
                self.expect(&Token::Join)?;
                JoinKind::Right
            }
            Token::Full  => {
                self.pos += 1;
                self.consume_if(&Token::Outer);
                self.expect(&Token::Join)?;
                JoinKind::Full
            }
            Token::Join  => { self.pos += 1; JoinKind::Inner }
            _ => return Err(KoreError::InvalidArgument("expected JOIN keyword".into())),
        };

        let table = self.parse_table_expr()?;
        self.expect(&Token::On)?;

        // Parse equi-join: col = col  (may be qualified)
        let left_col  = self.parse_qualified_col()?;
        self.expect(&Token::Eq)?;
        let right_col = self.parse_qualified_col()?;

        Ok(JoinClause { join_type, table, on: JoinOn { left_col, right_col } })
    }

    fn parse_qualified_col(&mut self) -> Result<String, KoreError> {
        // Accept both identifiers and SQL keywords used as column names (e.g. avg, count, sum)
        let name = self.expect_alias()?;
        if self.peek() == &Token::Dot {
            self.pos += 1;
            let col = self.expect_alias()?;
            Ok(format!("{}.{}", name, col))
        } else {
            Ok(name)
        }
    }

    // ─── Expression parser (Pratt/precedence climbing) ────────────────────

    fn parse_expr(&mut self, min_prec: u8) -> Result<Expr, KoreError> {
        let mut lhs = self.parse_unary()?;
        loop {
            // IS NULL / IS NOT NULL
            if self.peek() == &Token::Is {
                self.pos += 1;
                lhs = if self.peek() == &Token::Not {
                    self.pos += 1; self.expect(&Token::Null)?;
                    Expr::IsNotNull(Box::new(lhs))
                } else {
                    self.expect(&Token::Null)?;
                    Expr::IsNull(Box::new(lhs))
                };
                continue;
            }
            // LIKE / NOT LIKE
            if self.peek() == &Token::Like {
                self.pos += 1;
                let pat = self.parse_unary()?;
                lhs = Expr::Like { expr: Box::new(lhs), pattern: Box::new(pat), negated: false };
                continue;
            }
            // IN (...) or IN (SELECT ...)
            if self.peek() == &Token::In {
                self.pos += 1;
                self.expect(&Token::LParen)?;
                // Distinguish IN (SELECT ...) from IN (literal, ...)
                if self.peek() == &Token::Select {
                    let stmt = self.parse_select()?;
                    self.expect(&Token::RParen)?;
                    lhs = Expr::InSubquery { expr: Box::new(lhs), subquery: Box::new(stmt), negated: false };
                } else {
                    let values = self.parse_expr_list()?;
                    self.expect(&Token::RParen)?;
                    lhs = Expr::In { expr: Box::new(lhs), values, negated: false };
                }
                continue;
            }
            // NOT IN / NOT LIKE / NOT IN (SELECT ...)
            if self.peek() == &Token::Not {
                let next = self.tokens.get(self.pos + 1).cloned().unwrap_or(Token::Eof);
                match next {
                    Token::In => {
                        self.pos += 2;
                        self.expect(&Token::LParen)?;
                        if self.peek() == &Token::Select {
                            let stmt = self.parse_select()?;
                            self.expect(&Token::RParen)?;
                            lhs = Expr::InSubquery { expr: Box::new(lhs), subquery: Box::new(stmt), negated: true };
                        } else {
                            let values = self.parse_expr_list()?;
                            self.expect(&Token::RParen)?;
                            lhs = Expr::In { expr: Box::new(lhs), values, negated: true };
                        }
                        continue;
                    }
                    Token::Like => {
                        self.pos += 2;
                        let pat = self.parse_unary()?;
                        lhs = Expr::Like { expr: Box::new(lhs), pattern: Box::new(pat), negated: true };
                        continue;
                    }
                    _ => {}
                }
            }
            // BETWEEN low AND high
            if self.peek() == &Token::Between {
                self.pos += 1;
                let low  = self.parse_expr(5)?;
                self.expect(&Token::And)?;
                let high = self.parse_expr(5)?;
                lhs = Expr::Between { expr: Box::new(lhs), low: Box::new(low), high: Box::new(high), negated: false };
                continue;
            }
            let prec = infix_precedence(self.peek());
            if prec == 0 || prec < min_prec { break; }
            let op_tok = self.advance();
            let op = tok_to_binop(&op_tok)?;
            let rhs = self.parse_expr(prec + 1)?;
            lhs = Expr::BinOp { op, left: Box::new(lhs), right: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, KoreError> {
        // EXISTS (SELECT ...)
        if let Token::Ident(ref s) = self.peek().clone() {
            if s.eq_ignore_ascii_case("EXISTS") {
                self.pos += 1;
                self.expect(&Token::LParen)?;
                let stmt = self.parse_select()?;
                self.expect(&Token::RParen)?;
                return Ok(Expr::Exists { subquery: Box::new(stmt), negated: false });
            }
        }
        if self.consume_if(&Token::Not) {
            // NOT EXISTS (SELECT ...)
            if let Token::Ident(ref s) = self.peek().clone() {
                if s.eq_ignore_ascii_case("EXISTS") {
                    self.pos += 1;
                    self.expect(&Token::LParen)?;
                    let stmt = self.parse_select()?;
                    self.expect(&Token::RParen)?;
                    return Ok(Expr::Exists { subquery: Box::new(stmt), negated: true });
                }
            }
            // NOT IN / NOT LIKE / NOT BETWEEN
            if self.peek() == &Token::In {
                self.pos += 1;
                self.expect(&Token::LParen)?;
                // Check if it's IN (SELECT ...) or IN (literal, ...)
                if self.peek() == &Token::Select {
                    let stmt = self.parse_select()?;
                    self.expect(&Token::RParen)?;
                    return Ok(Expr::InSubquery { expr: Box::new(Expr::Null), subquery: Box::new(stmt), negated: true });
                }
                let values = self.parse_expr_list()?;
                self.expect(&Token::RParen)?;
                return Ok(Expr::In { expr: Box::new(Expr::Null), values, negated: true });
            }
            return Ok(Expr::Not(Box::new(self.parse_unary()?)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, KoreError> {
        match self.advance() {
            Token::LParen => {
                // If next token is SELECT → scalar subquery: (SELECT ...)
                if self.peek() == &Token::Select {
                    let stmt = self.parse_select()?;
                    self.expect(&Token::RParen)?;
                    return Ok(Expr::ScalarSubquery(Box::new(stmt)));
                }
                let e = self.parse_expr(0)?;
                self.expect(&Token::RParen)?;
                Ok(e)
            }
            Token::Int(n)   => Ok(Expr::Int(n)),
            Token::Float(f) => Ok(Expr::Float(f)),
            Token::Str(s)   => Ok(Expr::Str(s)),
            Token::Null     => Ok(Expr::Null),
            // CASE WHEN ... THEN ... [ELSE ...] END
            Token::Case => self.parse_case(),
            // Aggregate functions — check for OVER (window)
            Token::Count => {
                if self.peek() != &Token::LParen {
                    return Ok(Expr::Col("count".to_string()));
                }
                self.expect(&Token::LParen)?;
                let distinct = self.consume_if(&Token::Distinct);
                let inner = if self.peek() == &Token::Star {
                    self.pos += 1; Expr::Col("*".into())
                } else { self.parse_expr(0)? };
                self.expect(&Token::RParen)?;
                let func = if distinct { AggFunc::CountDistinct } else { AggFunc::Count };
                self.maybe_window(Expr::Agg { func: func.clone(), expr: Box::new(inner) }, func)
            }
            Token::Sum => {
                if self.peek() != &Token::LParen {
                    return Ok(Expr::Col("sum".to_string()));
                }
                self.expect(&Token::LParen)?;
                let inner = self.parse_expr(0)?;
                self.expect(&Token::RParen)?;
                self.maybe_window(Expr::Agg { func: AggFunc::Sum, expr: Box::new(inner.clone()) }, AggFunc::Sum)
            }
            Token::Avg => {
                // If NOT followed by '(', treat as column reference (e.g. CTE alias named "avg")
                if self.peek() != &Token::LParen {
                    return Ok(Expr::Col("avg".to_string()));
                }
                self.expect(&Token::LParen)?;
                let inner = self.parse_expr(0)?;
                self.expect(&Token::RParen)?;
                self.maybe_window(Expr::Agg { func: AggFunc::Avg, expr: Box::new(inner) }, AggFunc::Avg)
            }
            Token::Min => {
                if self.peek() != &Token::LParen {
                    return Ok(Expr::Col("min".to_string()));
                }
                self.expect(&Token::LParen)?;
                let inner = self.parse_expr(0)?;
                self.expect(&Token::RParen)?;
                self.maybe_window(Expr::Agg { func: AggFunc::Min, expr: Box::new(inner) }, AggFunc::Min)
            }
            Token::Max => {
                self.expect(&Token::LParen)?;
                let inner = self.parse_expr(0)?;
                self.expect(&Token::RParen)?;
                self.maybe_window(Expr::Agg { func: AggFunc::Max, expr: Box::new(inner) }, AggFunc::Max)
            }
            // Identifier: plain column OR window function name
            Token::Ident(name) => {
                match name.to_ascii_uppercase().as_str() {
                    "ROW_NUMBER" | "RANK" | "DENSE_RANK" | "NTILE" |
                    "LAG" | "LEAD" | "FIRST_VALUE" | "LAST_VALUE" | "CUMSUM" | "CUM_SUM" => {
                        self.expect(&Token::LParen)?;
                        let wfn = self.parse_window_fn_args(&name)?;
                        self.expect(&Token::RParen)?;
                        let spec = if self.peek() == &Token::Over {
                            self.pos += 1;
                            self.parse_window_spec()?
                        } else {
                            WindowSpec::default()
                        };
                        Ok(Expr::Window { func: wfn, spec })
                    }
                    _ => {
                        if self.peek() == &Token::LParen {
                            // Scalar function call: UPPER(x), ROUND(x,2), etc.
                            self.pos += 1;
                            let args = if self.peek() != &Token::RParen {
                                // CAST(expr AS type) — treat AS as arg separator
                                let first = self.parse_expr(0)?;
                                let mut a = vec![first];
                                while self.consume_if(&Token::Comma) || self.consume_if(&Token::As) {
                                    a.push(self.parse_expr(0)?);
                                }
                                a
                            } else { vec![] };
                            self.expect(&Token::RParen)?;
                            Ok(Expr::FuncCall { name: name.to_ascii_uppercase(), args })
                        } else if self.peek() == &Token::Dot {
                            self.pos += 1;
                            let col = self.expect_ident()?;
                            Ok(Expr::QualCol(name, col))
                        } else {
                            Ok(Expr::Col(name))
                        }
                    }
                }
            }
            // SQL keywords that can also be function names: LEFT(str, n), RIGHT(str, n)
            Token::Left | Token::Right => {
                let fname = match &self.tokens[self.pos - 1] { Token::Left => "LEFT", _ => "RIGHT" };
                if self.peek() == &Token::LParen {
                    self.pos += 1; // consume LParen
                    let s    = self.parse_expr(0)?;
                    self.expect(&Token::Comma)?;
                    let n    = self.parse_expr(0)?;
                    self.expect(&Token::RParen)?;
                    Ok(Expr::FuncCall { name: fname.to_string(), args: vec![s, n] })
                } else {
                    Ok(Expr::Col(fname.to_lowercase()))
                }
            }
            other => Err(KoreError::InvalidArgument(format!("unexpected token in expr: {:?}", other))),
        }
    }

    // ── List helpers ───────────────────────────────────────────────────────

    fn parse_expr_list(&mut self) -> Result<Vec<Expr>, KoreError> {
        let mut list = vec![self.parse_expr(0)?];
        while self.consume_if(&Token::Comma) { list.push(self.parse_expr(0)?); }
        Ok(list)
    }

    // ── CASE WHEN ─────────────────────────────────────────────────────────

    fn parse_case(&mut self) -> Result<Expr, KoreError> {
        // Simple CASE: operand is present; Searched CASE: WHEN comes right after CASE
        let operand = if self.peek() != &Token::When {
            Some(Box::new(self.parse_expr(0)?))
        } else { None };

        let mut branches = vec![];
        while self.peek() == &Token::When {
            self.pos += 1;
            let cond = self.parse_expr(0)?;
            self.expect(&Token::Then)?;
            let val  = self.parse_expr(0)?;
            branches.push((Box::new(cond), Box::new(val)));
        }

        let else_val = if self.peek() == &Token::Else {
            self.pos += 1;
            Some(Box::new(self.parse_expr(0)?))
        } else { None };

        self.expect(&Token::End)?;
        Ok(Expr::Case { operand, branches, else_val })
    }

    // ── List helpers (ident/col) ───────────────────────────────────────────

    fn parse_ident_list(&mut self) -> Result<Vec<String>, KoreError> {
        let mut list = vec![self.parse_qualified_col()?];
        while self.consume_if(&Token::Comma) {
            list.push(self.parse_qualified_col()?);
        }
        Ok(list)
    }

    fn parse_order_by_list(&mut self) -> Result<Vec<OrderByItem>, KoreError> {
        let mut list = Vec::new();
        loop {
            let col = self.parse_qualified_col()?;
            let desc = if self.consume_if(&Token::Desc) { true }
                       else { self.consume_if(&Token::Asc); false };
            list.push(OrderByItem { col, desc });
            if !self.consume_if(&Token::Comma) { break; }
        }
        Ok(list)
    }
}

// ─── Operator helpers ─────────────────────────────────────────────────────────

fn infix_precedence(tok: &Token) -> u8 {
    match tok {
        Token::Or              => 1,
        Token::And             => 2,
        Token::Eq | Token::Ne  => 3,
        Token::Lt | Token::Le
        | Token::Gt | Token::Ge => 4,
        Token::Plus | Token::Minus => 5,
        Token::Star | Token::Slash | Token::Percent => 6,
        _ => 0,
    }
}

fn tok_to_binop(tok: &Token) -> Result<BinOpKind, KoreError> {
    Ok(match tok {
        Token::Eq     => BinOpKind::Eq,
        Token::Ne     => BinOpKind::Ne,
        Token::Lt     => BinOpKind::Lt,
        Token::Le     => BinOpKind::Le,
        Token::Gt     => BinOpKind::Gt,
        Token::Ge     => BinOpKind::Ge,
        Token::And    => BinOpKind::And,
        Token::Or     => BinOpKind::Or,
        Token::Plus   => BinOpKind::Add,
        Token::Minus  => BinOpKind::Sub,
        Token::Star   => BinOpKind::Mul,
        Token::Slash  => BinOpKind::Div,
        Token::Percent => BinOpKind::Mod,
        other => return Err(KoreError::InvalidArgument(format!("not a binary op: {:?}", other))),
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_select() {
        let stmt = parse("SELECT id, name FROM users").unwrap();
        assert_eq!(stmt.projections.len(), 2);
        assert_eq!(stmt.from.name, "users");
        assert!(stmt.joins.is_empty());
    }

    #[test]
    fn parse_inner_join() {
        let stmt = parse(
            "SELECT a.id, b.name FROM orders AS a INNER JOIN customers AS b ON a.cust_id = b.id"
        ).unwrap();
        assert_eq!(stmt.joins.len(), 1);
        assert_eq!(stmt.joins[0].join_type, JoinKind::Inner);
    }

    #[test]
    fn parse_where_order_limit() {
        let stmt = parse(
            "SELECT * FROM scores WHERE score > 80 ORDER BY score DESC LIMIT 10"
        ).unwrap();
        assert!(stmt.where_clause.is_some());
        assert_eq!(stmt.order_by.len(), 1);
        assert!(stmt.order_by[0].desc);
        assert_eq!(stmt.limit, Some(10));
    }
}

