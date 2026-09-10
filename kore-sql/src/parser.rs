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

    // UNION ALL / UNION / INTERSECT / EXCEPT
    let mut set_ops = vec![];
    while matches!(p.peek(), Token::Union | Token::Intersect | Token::Except) {
        let kind = match p.peek() {
            Token::Union => {
                p.pos += 1;
                if p.consume_if(&Token::All) {
                    SetOpKind::UnionAll
                } else {
                    p.consume_if(&Token::Distinct);
                    SetOpKind::Union
                }
            }
            Token::Intersect => { p.pos += 1; p.consume_if(&Token::All); SetOpKind::Intersect }
            Token::Except    => { p.pos += 1; p.consume_if(&Token::All); SetOpKind::Except }
            _ => unreachable!(),
        };
        set_ops.push((kind, p.parse_select()?));
    }

    Ok(Query { ctes, body, set_ops })
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
    /// Returns the uppercase string if the next token is an Ident, else "".
    fn peek_ident_upper(&self) -> String {
        match self.tokens.get(self.pos) {
            Some(Token::Ident(s)) => s.to_ascii_uppercase(),
            _ => String::new(),
        }
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
            Token::Left      => Ok("left".to_string()),
            Token::Right     => Ok("right".to_string()),
            Token::Inner     => Ok("inner".to_string()),
            Token::Full      => Ok("full".to_string()),
            Token::Join      => Ok("join".to_string()),
            Token::On        => Ok("on".to_string()),
            Token::By        => Ok("by".to_string()),
            Token::Array     => Ok("array".to_string()),
            Token::Map       => Ok("map".to_string()),
            Token::Explode   => Ok("explode".to_string()),
            Token::Pivot     => Ok("pivot".to_string()),
            Token::Unpivot   => Ok("unpivot".to_string()),
            Token::Lateral   => Ok("lateral".to_string()),
            Token::For       => Ok("for".to_string()),
            other => Err(KoreError::InvalidArgument(format!("expected alias name, got {:?}", other))),
        }
    }
    fn consume_if(&mut self, tok: &Token) -> bool {
        if self.peek() == tok { self.pos += 1; true } else { false }
    }

    // ─── SELECT statement ──────────────────────────────────────────────────

    fn parse_select(&mut self) -> Result<SelectStmt, KoreError> {
        self.expect(&Token::Select)?;

        // Parse query hints: /*+ BROADCAST(t) */ or /*+ REPARTITION(n) */
        let hints = if let Token::Hint(_) = self.peek() {
            match self.advance() {
                Token::Hint(h) => parse_hints(&h),
                _ => vec![],
            }
        } else { vec![] };

        let distinct = self.consume_if(&Token::Distinct);

        // projections
        let projections = self.parse_projections()?;

        // FROM (optional — allows SELECT 1+1, SELECT NOW() etc.)
        let from = if self.peek() == &Token::From {
            self.pos += 1;
            self.parse_table_expr()?
        } else {
            TableExpr { name: "__dual__".to_string(), alias: None, subquery: None, values: None, push_filter: None }
        };

        // PIVOT / UNPIVOT (after FROM, before JOINs)
        let pivot = if self.peek() == &Token::Pivot {
            Some(self.parse_pivot()?)
        } else { None };
        let unpivot = if self.peek() == &Token::Unpivot {
            Some(self.parse_unpivot()?)
        } else { None };

        // LATERAL VIEW EXPLODE(col) alias AS col_alias
        let mut lateral_views = Vec::new();
        while self.peek() == &Token::Lateral {
            lateral_views.push(self.parse_lateral_view()?);
        }

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

        // GROUP BY [ROLLUP(...) | CUBE(...) | plain list]
        let (group_by, rollup, cube) = if self.peek() == &Token::Group && self.peek2() == &Token::By {
            self.pos += 2; // consume GROUP BY
            // Check for ROLLUP or CUBE
            let upper = self.peek().clone();
            if let Token::Ident(kw) = &upper {
                let kw = kw.to_uppercase();
                if kw == "ROLLUP" || kw == "CUBE" {
                    let is_rollup = kw == "ROLLUP";
                    self.pos += 1; // consume ROLLUP/CUBE
                    self.expect(&Token::LParen)?;
                    let cols = self.parse_ident_list()?;
                    self.expect(&Token::RParen)?;
                    (cols, is_rollup, !is_rollup)
                } else {
                    (self.parse_ident_list()?, false, false)
                }
            } else {
                (self.parse_ident_list()?, false, false)
            }
        } else {
            (Vec::new(), false, false)
        };
        // Ignore rollup/cube flags for now — treat same as plain GROUP BY
        let _ = (rollup, cube);

        // HAVING
        let having = if self.consume_if(&Token::Having) {
            Some(self.parse_expr(0)?)
        } else {
            None
        };

        // QUALIFY (filter on window function results)
        let qualify = if self.peek_ident_upper() == "QUALIFY" {
            self.pos += 1;
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
        } else if self.peek_ident_upper() == "FETCH" {
            // FETCH FIRST n ROWS ONLY
            self.pos += 1; // consume FETCH
            self.pos += 1; // consume FIRST (or NEXT)
            let n = match self.advance() { Token::Int(n) => n as u64, _ => 1 };
            // consume ROWS ONLY
            while !matches!(self.peek(), Token::Eof) {
                if self.peek_ident_upper() == "ONLY" { self.pos += 1; break; }
                self.pos += 1;
            }
            Some(n)
        } else {
            None
        };

        // OFFSET n ROWS
        let offset = if self.peek_ident_upper() == "OFFSET" {
            self.pos += 1;
            let n = match self.advance() { Token::Int(n) => n as u64, _ => 0 };
            // skip optional ROWS
            if self.peek_ident_upper() == "ROWS" { self.pos += 1; }
            Some(n)
        } else {
            None
        };

        Ok(SelectStmt { distinct, projections, from, joins, where_clause,
                         group_by, having, qualify, order_by, limit, offset, scan_limit: None,
                         lateral_views, pivot, unpivot, hints })
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
            // Consume optional BETWEEN keyword (Token::Between or Ident "BETWEEN")
            match self.peek() {
                Token::Between => { self.pos += 1; }
                Token::Ident(s) if s.eq_ignore_ascii_case("BETWEEN") => { self.pos += 1; }
                _ => {}
            }
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
            "ROW_NUMBER"   => WindowFn::RowNumber,
            "RANK"         => WindowFn::Rank,
            "DENSE_RANK"   => WindowFn::DenseRank,
            "PERCENT_RANK" => WindowFn::PercentRank,
            "CUME_DIST"    => WindowFn::CumeDist,
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
            Token::Join | Token::Inner | Token::Left | Token::Right | Token::Full | Token::Cross)
    }

    // ─── LATERAL VIEW ──────────────────────────────────────────────────────
    fn parse_lateral_view(&mut self) -> Result<LateralView, KoreError> {
        self.expect(&Token::Lateral)?;
        // Consume VIEW (as Ident since it might be Token::View or an ident)
        match self.peek() {
            Token::View => { self.pos += 1; }
            Token::Ident(s) if s.eq_ignore_ascii_case("VIEW") => { self.pos += 1; }
            _ => return Err(KoreError::InvalidArgument("expected VIEW after LATERAL".into())),
        }
        // EXPLODE(expr)
        self.expect(&Token::Explode)?;
        self.expect(&Token::LParen)?;
        let expr = self.parse_expr(0)?;
        self.expect(&Token::RParen)?;
        // table_alias
        let table_alias = self.expect_ident()?;
        // AS col_alias
        self.expect(&Token::As)?;
        let col_alias = self.expect_alias()?;
        Ok(LateralView { expr: Expr::Explode(Box::new(expr)), table_alias, col_alias })
    }

    // ─── PIVOT ─────────────────────────────────────────────────────────────
    fn parse_pivot(&mut self) -> Result<PivotClause, KoreError> {
        self.expect(&Token::Pivot)?;
        self.expect(&Token::LParen)?;
        // agg_func(agg_col)
        let agg_func = self.parse_agg_func_name()?;
        self.expect(&Token::LParen)?;
        let agg_col = self.expect_ident()?;
        self.expect(&Token::RParen)?;
        // FOR for_col
        self.expect(&Token::For)?;
        let for_col = self.expect_ident()?;
        // IN (val1, val2, ...)
        self.expect(&Token::In)?;
        self.expect(&Token::LParen)?;
        let in_values = self.parse_expr_list()?;
        self.expect(&Token::RParen)?;
        self.expect(&Token::RParen)?;
        Ok(PivotClause { agg_func, agg_col, for_col, in_values })
    }

    fn parse_agg_func_name(&mut self) -> Result<AggFunc, KoreError> {
        match self.advance() {
            Token::Sum   => Ok(AggFunc::Sum),
            Token::Avg   => Ok(AggFunc::Avg),
            Token::Count => Ok(AggFunc::Count),
            Token::Min   => Ok(AggFunc::Min),
            Token::Max   => Ok(AggFunc::Max),
            Token::Ident(s) => match s.to_ascii_uppercase().as_str() {
                "SUM"   => Ok(AggFunc::Sum),
                "AVG"   => Ok(AggFunc::Avg),
                "COUNT" => Ok(AggFunc::Count),
                "MIN"   => Ok(AggFunc::Min),
                "MAX"   => Ok(AggFunc::Max),
                other => Err(KoreError::InvalidArgument(format!("unsupported PIVOT aggregate: {other}"))),
            },
            other => Err(KoreError::InvalidArgument(format!("expected aggregate function, got {:?}", other))),
        }
    }

    // ─── UNPIVOT ───────────────────────────────────────────────────────────
    fn parse_unpivot(&mut self) -> Result<UnpivotClause, KoreError> {
        self.expect(&Token::Unpivot)?;
        self.expect(&Token::LParen)?;
        // value_col FOR key_col IN (col1, col2, ...)
        let value_col = self.expect_ident()?;
        self.expect(&Token::For)?;
        let key_col = self.expect_ident()?;
        self.expect(&Token::In)?;
        self.expect(&Token::LParen)?;
        let in_cols = self.parse_ident_list()?;
        self.expect(&Token::RParen)?;
        self.expect(&Token::RParen)?;
        Ok(UnpivotClause { value_col, key_col, in_cols })
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
        // VALUES (r1c1, r1c2), (r2c1, r2c2) AS t — inline table
        if self.peek_ident_upper() == "VALUES" {
            self.pos += 1;
            let mut rows: Vec<Vec<Expr>> = Vec::new();
            loop {
                self.expect(&Token::LParen)?;
                let mut row = vec![self.parse_expr(0)?];
                while self.consume_if(&Token::Comma) { row.push(self.parse_expr(0)?); }
                self.expect(&Token::RParen)?;
                rows.push(row);
                if !self.consume_if(&Token::Comma) { break; }
            }
            let alias = if self.consume_if(&Token::As) { Some(self.expect_alias()?) }
                        else if matches!(self.peek(), Token::Ident(_)) { Some(self.expect_alias()?) }
                        else { Some("_values".to_string()) };
            let name = alias.clone().unwrap_or_else(|| "_values".to_string());
            return Ok(TableExpr { name, alias, subquery: None, values: Some(rows), push_filter: None });
        }

        // Handle FROM (SELECT ...) alias — subquery as FROM table
        if self.peek() == &Token::LParen {
            self.pos += 1; // consume (
            // Could be VALUES inside parens too
            let subq = self.parse_select()?;
            self.expect(&Token::RParen)?;
            let alias = if self.consume_if(&Token::As) {
                Some(self.expect_ident()?)
            } else if matches!(self.peek(), Token::Ident(_)) {
                Some(self.expect_ident()?)
            } else {
                Some("_subq".to_string())
            };
            let name = alias.clone().unwrap_or_else(|| "_subq".to_string());
            return Ok(TableExpr { name, alias, subquery: Some(Box::new(subq)), values: None, push_filter: None });
        }

        // Accept a string literal as table name (e.g. FROM 'data/file.parquet')
        let name = if matches!(self.peek(), Token::Str(_)) {
            match self.advance() { Token::Str(s) => s, _ => unreachable!() }
        } else {
            self.expect_ident()?
        };
        let alias = if self.consume_if(&Token::As) {
            Some(self.expect_ident()?)
        } else if matches!(self.peek(), Token::Ident(s) if !["WHERE","ORDER","GROUP","LIMIT","HAVING","QUALIFY","UNION","INTERSECT","EXCEPT","FETCH","OFFSET","ON","SET","INTO"].contains(&s.to_ascii_uppercase().as_str()))
               && !self.is_join_keyword()
               && self.peek() != &Token::Where
               && self.peek() != &Token::Order
               && self.peek() != &Token::Group
               && self.peek() != &Token::Limit
               && self.peek() != &Token::Pivot
               && self.peek() != &Token::Unpivot
               && self.peek() != &Token::Lateral {
            Some(self.expect_ident()?)
        } else {
            None
        };
        Ok(TableExpr { name, alias, subquery: None, values: None, push_filter: None })
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
            Token::Cross => {
                self.pos += 1;
                self.expect(&Token::Join)?;
                JoinKind::Cross
            }
            Token::Join  => { self.pos += 1; JoinKind::Inner }
            _ => return Err(KoreError::InvalidArgument("expected JOIN keyword".into())),
        };

        let table = self.parse_table_expr()?;

        // CROSS JOIN has no ON clause
        if join_type == JoinKind::Cross {
            return Ok(JoinClause {
                join_type,
                table,
                on: JoinOn { left_col: String::new(), right_col: String::new(), expr: None },
                push_filter: None,
            });
        }

        self.expect(&Token::On)?;

        // Parse the ON expression (supports non-equi joins)
        let on_expr = self.parse_expr(0)?;

        // Try to extract equi-join columns for hash join optimization
        let (left_col, right_col) = match &on_expr {
            Expr::BinOp { op: BinOpKind::Eq, left, right } => {
                let lc = expr_to_col_name(left);
                let rc = expr_to_col_name(right);
                (lc.unwrap_or_default(), rc.unwrap_or_default())
            }
            _ => (String::new(), String::new()),
        };

        let expr = if left_col.is_empty() || right_col.is_empty() {
            Some(on_expr)
        } else {
            None
        };

        Ok(JoinClause { join_type, table, on: JoinOn { left_col, right_col, expr }, push_filter: None })
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
            // array[index] → ELEMENT_AT(array, index)
            if self.peek() == &Token::LBracket {
                self.pos += 1;
                let idx = self.parse_expr(0)?;
                self.expect(&Token::RBracket)?;
                lhs = Expr::FuncCall {
                    name: "ELEMENT_AT".to_string(),
                    args: vec![lhs, idx],
                };
                continue;
            }
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
            // ILIKE (case-insensitive LIKE)
            if self.peek() == &Token::ILike {
                self.pos += 1;
                let pat = self.parse_unary()?;
                lhs = Expr::ILike { expr: Box::new(lhs), pattern: Box::new(pat), negated: false };
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
            // NOT IN / NOT LIKE / NOT ILIKE / NOT BETWEEN / NOT IN (SELECT ...)
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
                    Token::ILike => {
                        self.pos += 2;
                        let pat = self.parse_unary()?;
                        lhs = Expr::ILike { expr: Box::new(lhs), pattern: Box::new(pat), negated: true };
                        continue;
                    }
                    Token::Between => {
                        self.pos += 2;
                        let low  = self.parse_expr(5)?;
                        self.expect(&Token::And)?;
                        let high = self.parse_expr(5)?;
                        lhs = Expr::Between { expr: Box::new(lhs), low: Box::new(low), high: Box::new(high), negated: true };
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
        // Unary minus: -expr → (0 - expr)
        if self.peek() == &Token::Minus {
            self.pos += 1;
            let inner = self.parse_primary()?;
            return Ok(Expr::BinOp {
                left: Box::new(Expr::Int(0)),
                op:   BinOpKind::Sub,
                right: Box::new(inner),
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, KoreError> {
        match self.advance() {
            // [1, 2, 3] → Array literal
            Token::LBracket => {
                let elements = if self.peek() != &Token::RBracket {
                    self.parse_expr_list()?
                } else { vec![] };
                self.expect(&Token::RBracket)?;
                return Ok(Expr::Array(elements));
            }
            // ARRAY(1, 2, 3) → Array literal
            Token::Array => {
                self.expect(&Token::LParen)?;
                let elements = if self.peek() != &Token::RParen {
                    self.parse_expr_list()?
                } else { vec![] };
                self.expect(&Token::RParen)?;
                return Ok(Expr::Array(elements));
            }
            // MAP('a', 1, 'b', 2) → FuncCall("MAP", ...)
            Token::Map => {
                self.expect(&Token::LParen)?;
                let args = if self.peek() != &Token::RParen {
                    self.parse_expr_list()?
                } else { vec![] };
                self.expect(&Token::RParen)?;
                return Ok(Expr::FuncCall { name: "MAP".to_string(), args });
            }
            // EXPLODE(expr)
            Token::Explode => {
                self.expect(&Token::LParen)?;
                let inner = self.parse_expr(0)?;
                self.expect(&Token::RParen)?;
                return Ok(Expr::Explode(Box::new(inner)));
            }
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
            // ── Extended aggregate functions ──────────────────────────────────────
            Token::Ident(ref name) if matches!(name.to_ascii_uppercase().as_str(),
                "STDDEV" | "STDEV" | "STDDEV_POP" | "STDDEV_SAMP" | "STD" |
                "VARIANCE" | "VAR_POP" | "VAR_SAMP" |
                "MEDIAN" | "STRING_AGG" | "GROUP_CONCAT" | "LISTAGG" |
                "PERCENTILE_CONT" | "PERCENTILE_DISC"
            ) => {
                let fname = name.to_ascii_uppercase();
                // token already consumed by parse_primary's advance()
                if self.peek() != &Token::LParen {
                    return Ok(Expr::Col(fname.to_lowercase()));
                }
                self.expect(&Token::LParen)?;
                let inner = self.parse_expr(0)?;
                let func = match fname.as_str() {
                    "STDDEV" | "STDEV" | "STDDEV_SAMP" | "STDDEV_POP" | "STD" => AggFunc::Stddev,
                    "VARIANCE" | "VAR_POP" | "VAR_SAMP" => AggFunc::Variance,
                    "MEDIAN" => AggFunc::Median,
                    "STRING_AGG" | "LISTAGG" | "GROUP_CONCAT" => {
                        let sep = if self.consume_if(&Token::Comma) {
                            match self.advance() { Token::Str(s) => s, _ => ",".to_string() }
                        } else { ",".to_string() };
                        AggFunc::StringAgg { sep }
                    }
                    "PERCENTILE_CONT" | "PERCENTILE_DISC" => {
                        let p_str = match &inner { Expr::Float(f) => format!("{}", f), Expr::Int(i) => format!("{}", i), _ => "0.5".to_string() };
                        self.expect(&Token::RParen)?;
                        if self.peek_ident_upper() == "WITHIN" { self.pos += 1; }
                        if self.peek_ident_upper() == "GROUP"  { self.pos += 1; }
                        if self.peek() == &Token::LParen { self.pos += 1; }
                        if self.peek_ident_upper() == "ORDER"  { self.pos += 1; }
                        if let Token::Ident(s) = self.peek().clone() { if s.eq_ignore_ascii_case("BY") { self.pos += 1; } }
                        let order_col = self.parse_expr(0)?;
                        if self.peek() == &Token::RParen { self.pos += 1; }
                        return Ok(Expr::Agg { func: AggFunc::Percentile { p: p_str }, expr: Box::new(order_col) });
                    }
                    _ => AggFunc::Avg,
                };
                self.expect(&Token::RParen)?;
                Ok(Expr::Agg { func, expr: Box::new(inner) })
            }
            // Identifier: plain column OR window function name
            Token::Ident(name) => {
                match name.to_ascii_uppercase().as_str() {
                    "ROW_NUMBER" | "RANK" | "DENSE_RANK" | "PERCENT_RANK" | "CUME_DIST" | "NTILE" |
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
            // EXTRACT(field FROM date) → FuncCall("EXTRACT", [field_str, date])
            Token::Extract => {
                self.expect(&Token::LParen)?;
                // Field name: YEAR, MONTH, DAY, etc. (parsed as Ident or keyword)
                let field = self.expect_alias()?;
                // consume FROM keyword
                if let Token::From = self.peek() { self.pos += 1; }
                else if let Token::Ident(s) = self.peek() { if s.eq_ignore_ascii_case("FROM") { self.pos += 1; } }
                let date_expr = self.parse_expr(0)?;
                self.expect(&Token::RParen)?;
                Ok(Expr::FuncCall { name: "EXTRACT".to_string(), args: vec![Expr::Str(field), date_expr] })
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
            let expr = self.parse_expr(0)?;
            let col = match &expr {
                Expr::Col(c) => c.clone(),
                Expr::QualCol(t, c) => format!("{}.{}", t, c),
                _ => String::new(),
            };
            let desc = if self.consume_if(&Token::Desc) { true }
                       else { self.consume_if(&Token::Asc); false };
            // NULLS FIRST / NULLS LAST
            let nulls_first = if self.peek_ident_upper() == "NULLS" {
                self.pos += 1;
                let first = self.peek_ident_upper() == "FIRST";
                self.pos += 1; // consume FIRST or LAST
                Some(first)
            } else { None };
            list.push(OrderByItem { expr, col, desc, nulls_first });
            if !self.consume_if(&Token::Comma) { break; }
        }
        Ok(list)
    }
}

// ─── Hint parser ──────────────────────────────────────────────────────────────

fn parse_hints(hint_text: &str) -> Vec<QueryHint> {
    let mut hints = Vec::new();
    for part in hint_text.split(',') {
        let part = part.trim();
        if let Some(name) = part.strip_prefix("BROADCAST(").or_else(|| part.strip_prefix("broadcast(")) {
            if let Some(table) = name.strip_suffix(')') {
                hints.push(QueryHint::Broadcast(table.trim().to_string()));
            }
        } else if let Some(n_str) = part.strip_prefix("REPARTITION(").or_else(|| part.strip_prefix("repartition(")) {
            if let Some(n) = n_str.strip_suffix(')') {
                if let Ok(n) = n.trim().parse::<usize>() {
                    hints.push(QueryHint::Repartition(n));
                }
            }
        }
    }
    hints
}

// ─── Operator helpers ─────────────────────────────────────────────────────────

fn infix_precedence(tok: &Token) -> u8 {
    match tok {
        Token::Or              => 1,
        Token::And             => 2,
        Token::Eq | Token::Ne  => 3,
        Token::Lt | Token::Le
        | Token::Gt | Token::Ge => 4,
        Token::Plus | Token::Minus | Token::Concat => 5,
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
        Token::Concat => BinOpKind::Concat,
        other => return Err(KoreError::InvalidArgument(format!("not a binary op: {:?}", other))),
    })
}

fn expr_to_col_name(e: &Expr) -> Option<String> {
    match e {
        Expr::Col(c) => Some(c.clone()),
        Expr::QualCol(t, c) => Some(format!("{}.{}", t, c)),
        _ => None,
    }
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

    // ── LATERAL VIEW ─────────────────────────────────────────────────────────

    #[test]
    fn parse_lateral_view_explode() {
        let stmt = parse(
            "SELECT id, val FROM test LATERAL VIEW EXPLODE(tags) t AS val"
        ).unwrap();
        assert_eq!(stmt.lateral_views.len(), 1);
        assert_eq!(stmt.lateral_views[0].table_alias, "t");
        assert_eq!(stmt.lateral_views[0].col_alias, "val");
    }

    #[test]
    fn parse_lateral_view_with_array() {
        let stmt = parse(
            "SELECT id, v FROM t LATERAL VIEW EXPLODE(ARRAY(1, 2, 3)) ex AS v"
        ).unwrap();
        assert_eq!(stmt.lateral_views.len(), 1);
    }

    // ── PIVOT / UNPIVOT ──────────────────────────────────────────────────────

    #[test]
    fn parse_pivot() {
        let stmt = parse(
            "SELECT * FROM sales PIVOT (SUM(amount) FOR product IN ('A', 'B', 'C'))"
        ).unwrap();
        assert!(stmt.pivot.is_some());
        let p = stmt.pivot.unwrap();
        assert_eq!(p.agg_col, "amount");
        assert_eq!(p.for_col, "product");
        assert_eq!(p.in_values.len(), 3);
    }

    #[test]
    fn parse_unpivot() {
        let stmt = parse(
            "SELECT * FROM wide UNPIVOT (value FOR key IN (col1, col2, col3))"
        ).unwrap();
        assert!(stmt.unpivot.is_some());
        let u = stmt.unpivot.unwrap();
        assert_eq!(u.value_col, "value");
        assert_eq!(u.key_col, "key");
        assert_eq!(u.in_cols.len(), 3);
    }

    // ── Complex types ────────────────────────────────────────────────────────

    #[test]
    fn parse_array_literal() {
        let stmt = parse("SELECT ARRAY(1, 2, 3) AS arr FROM t").unwrap();
        if let Projection::Expr { expr: Expr::Array(elems), .. } = &stmt.projections[0] {
            assert_eq!(elems.len(), 3);
        } else {
            panic!("expected Expr::Array");
        }
    }

    #[test]
    fn parse_bracket_array() {
        let stmt = parse("SELECT [1, 2, 3] AS arr FROM t").unwrap();
        if let Projection::Expr { expr: Expr::Array(elems), .. } = &stmt.projections[0] {
            assert_eq!(elems.len(), 3);
        } else {
            panic!("expected Expr::Array");
        }
    }

    #[test]
    fn parse_map_function() {
        let stmt = parse("SELECT MAP('a', 1, 'b', 2) AS m FROM t").unwrap();
        if let Projection::Expr { expr: Expr::FuncCall { name, args }, .. } = &stmt.projections[0] {
            assert_eq!(name, "MAP");
            assert_eq!(args.len(), 4);
        } else {
            panic!("expected Expr::FuncCall MAP");
        }
    }

    #[test]
    fn parse_explode_function() {
        let stmt = parse("SELECT EXPLODE(tags) AS val FROM t").unwrap();
        if let Projection::Expr { expr: Expr::Explode(_), .. } = &stmt.projections[0] {
            // OK
        } else {
            panic!("expected Expr::Explode");
        }
    }

    #[test]
    fn parse_element_at_bracket_syntax() {
        let stmt = parse("SELECT arr[1] AS elem FROM t").unwrap();
        if let Projection::Expr { expr: Expr::FuncCall { name, args }, .. } = &stmt.projections[0] {
            assert_eq!(name, "ELEMENT_AT");
            assert_eq!(args.len(), 2);
        } else {
            panic!("expected ELEMENT_AT function from bracket syntax");
        }
    }

    // ── Query hints ──────────────────────────────────────────────────────────

    #[test]
    fn parse_broadcast_hint() {
        let stmt = parse("SELECT /*+ BROADCAST(orders) */ * FROM orders").unwrap();
        assert_eq!(stmt.hints.len(), 1);
        assert!(matches!(&stmt.hints[0], QueryHint::Broadcast(t) if t == "orders"));
    }

    #[test]
    fn parse_repartition_hint() {
        let stmt = parse("SELECT /*+ REPARTITION(16) */ * FROM orders").unwrap();
        assert_eq!(stmt.hints.len(), 1);
        assert!(matches!(&stmt.hints[0], QueryHint::Repartition(16)));
    }

    #[test]
    fn parse_no_hint() {
        let stmt = parse("SELECT * FROM orders").unwrap();
        assert!(stmt.hints.is_empty());
    }
}

