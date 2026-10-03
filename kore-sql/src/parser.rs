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

    // Main statement: SELECT arms joined by UNION / INTERSECT / EXCEPT / MINUS, optional trailing ORDER BY / LIMIT
    let mut body = p.parse_compound()?;
    let set_ops = std::mem::take(&mut body.set_ops);
    let (q_order, q_limit, q_offset) = if set_ops.is_empty() {
        (Vec::new(), None, None)
    } else {
        (std::mem::take(&mut body.order_by), body.limit.take(), body.offset.take())
    };

    // Never silently drop the tail of a statement: whatever the grammar did not consume is an error.
    p.consume_if(&Token::Semicolon);
    if p.peek() != &Token::Eof {
        return Err(KoreError::InvalidArgument(format!(
            "unexpected {:?} after the end of the statement", p.peek())));
    }

    Ok(Query { ctes, body: Some(body), set_ops, order_by: q_order, limit: q_limit, offset: q_offset })
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
            Token::Str(s)    => Ok(s),
            ref t if non_reserved_word(t).is_some() => Ok(non_reserved_word(t).unwrap().to_string()),
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
            TableExpr { name: "__dual__".to_string(), alias: None, subquery: None, values: None, push_filter: None, col_aliases: vec![] }
        };

        // FROM a, b, c — comma-separated tables are implicit inner joins; the join keys are taken
        // from equality predicates in WHERE when the statement is executed.
        let mut joins = Vec::new();
        while self.consume_if(&Token::Comma) {
            let table = self.parse_table_expr()?;
            joins.push(JoinClause {
                join_type: JoinKind::Implicit,
                table,
                on: JoinOn { left_col: String::new(), right_col: String::new(), expr: None },
                push_filter: None,
                using: Vec::new(),
                natural: false,
            });
        }

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
        while self.is_join_keyword() {
            joins.push(self.parse_join()?);
        }

        // WHERE
        let where_clause = if self.consume_if(&Token::Where) {
            Some(self.parse_expr(0)?)
        } else {
            None
        };

        // GROUP BY [plain list | expressions | ordinals | ROLLUP | CUBE | GROUPING SETS]
        let (group_by, grouping, group_exprs, group_sets) = if self.peek() == &Token::Group && self.peek2() == &Token::By {
            self.pos += 2; // consume GROUP BY
            self.parse_group_by(&projections)?
        } else {
            (Vec::new(), Grouping::Plain, Vec::new(), Vec::new())
        };

        // HAVING / QUALIFY / WINDOW w AS (..) in any order
        let mut having = None;
        let mut qualify = None;
        let mut windows: Vec<(String, WindowSpec)> = Vec::new();
        loop {
            if self.peek() == &Token::Having {
                self.pos += 1;
                having = Some(self.parse_expr(0)?);
            } else if self.peek_ident_upper() == "QUALIFY" {
                self.pos += 1;
                qualify = Some(self.parse_expr(0)?);
            } else if self.peek_ident_upper() == "WINDOW" {
                self.pos += 1;
                loop {
                    let name = self.expect_ident()?;
                    self.expect(&Token::As)?;
                    windows.push((name, self.parse_window_spec()?));
                    if !self.consume_if(&Token::Comma) { break; }
                }
            } else {
                break;
            }
        }

        // ORDER BY
        let order_by = if self.peek() == &Token::Order && self.peek2() == &Token::By {
            self.pos += 2;
            self.parse_order_by_list()?
        } else {
            Vec::new()
        };

        let (limit, offset) = self.parse_limit_offset()?;

        Ok(SelectStmt { distinct, projections, from, joins, where_clause,
                         group_by, grouping, group_exprs, group_sets, windows, having, qualify, order_by, limit, offset, scan_limit: None,
                         lateral_views, pivot, unpivot, hints, parenthesized: false, set_ops: Vec::new() })
    }

    /// `arm [UNION|INTERSECT|EXCEPT|MINUS [ALL|DISTINCT] arm]* [ORDER BY ..] [LIMIT ..]`. With set operations
    /// the first arm carries them in `set_ops` and the trailing ORDER BY / LIMIT / OFFSET apply to the whole.
    fn parse_compound(&mut self) -> Result<SelectStmt, KoreError> {
        let mut first = self.parse_select_arm()?;
        let mut arms: Vec<(SetOpKind, SelectStmt)> = Vec::new();
        loop {
            let is_minus = self.peek_ident_upper() == "MINUS";
            if !(matches!(self.peek(), Token::Union | Token::Intersect | Token::Except) || is_minus) { break; }
            let kind = match self.peek().clone() {
                Token::Union => {
                    self.pos += 1;
                    if self.consume_if(&Token::All) { SetOpKind::UnionAll } else { self.consume_if(&Token::Distinct); SetOpKind::Union }
                }
                Token::Intersect => {
                    self.pos += 1;
                    if self.consume_if(&Token::All) { SetOpKind::IntersectAll } else { self.consume_if(&Token::Distinct); SetOpKind::Intersect }
                }
                _ => {
                    self.pos += 1; // EXCEPT / MINUS
                    if self.consume_if(&Token::All) { SetOpKind::ExceptAll } else { self.consume_if(&Token::Distinct); SetOpKind::Except }
                }
            };
            arms.push((kind, self.parse_select_arm()?));
        }
        let (mut order, mut limit, mut offset) = (Vec::new(), None, None);
        // ORDER BY / LIMIT written after a bare last arm belong to the whole result
        if let Some((_, last)) = arms.last_mut() {
            if !last.parenthesized {
                order = std::mem::take(&mut last.order_by);
                limit = last.limit.take();
                offset = last.offset.take();
            }
        }
        // ... as do clauses following a parenthesised arm
        if self.peek() == &Token::Order && self.peek2() == &Token::By {
            self.pos += 2;
            order = self.parse_order_by_list()?;
        }
        if self.peek() == &Token::Limit || matches!(self.peek_ident_upper().as_str(), "OFFSET" | "FETCH") {
            let (l, o) = self.parse_limit_offset()?;
            if l.is_some() { limit = l; }
            if o.is_some() { offset = o; }
        }
        if arms.is_empty() {
            if !order.is_empty() { first.order_by = order; }
            if limit.is_some() { first.limit = limit; }
            if offset.is_some() { first.offset = offset; }
            return Ok(first);
        }
        // the first arm keeps its own ORDER BY / LIMIT only when it is parenthesised: isolate it
        if !first.order_by.is_empty() || first.limit.is_some() || first.offset.is_some() || !first.set_ops.is_empty() {
            first = SelectStmt::wrap(first);
        }
        first.set_ops = arms;
        first.order_by = order;
        first.limit = limit;
        first.offset = offset;
        Ok(first)
    }

    /// `SELECT ..` or `( SELECT .. )` as one arm of a set operation / the whole statement.
    fn parse_select_arm(&mut self) -> Result<SelectStmt, KoreError> {
        if self.peek() == &Token::LParen {
            self.pos += 1;
            let mut inner = self.parse_compound()?;
            self.expect(&Token::RParen)?;
            inner.parenthesized = true;
            return Ok(inner);
        }
        self.parse_select()
    }

    /// `LIMIT n`, `OFFSET m [ROWS]`, `FETCH FIRST n ROWS ONLY` in any sensible order.
    fn parse_limit_offset(&mut self) -> Result<(Option<u64>, Option<u64>), KoreError> {
        let (mut limit, mut offset) = (None, None);
        loop {
            if self.consume_if(&Token::Limit) {
                match self.advance() {
                    Token::Int(n) if n >= 0 => limit = Some(n as u64),
                    Token::All => {}
                    other => return Err(KoreError::InvalidArgument(format!("LIMIT expects a non-negative integer, got {:?}", other))),
                }
            } else if self.peek_ident_upper() == "FETCH" {
                // FETCH FIRST n ROWS ONLY
                self.pos += 2; // FETCH FIRST|NEXT
                let n = match self.peek().clone() { Token::Int(n) => { self.pos += 1; n as u64 } _ => 1 };
                while !matches!(self.peek(), Token::Eof) {
                    if self.peek_ident_upper() == "ONLY" { self.pos += 1; break; }
                    self.pos += 1;
                }
                limit = Some(n);
            } else if self.peek_ident_upper() == "OFFSET" {
                self.pos += 1;
                match self.advance() {
                    Token::Int(n) if n >= 0 => offset = Some(n as u64),
                    other => return Err(KoreError::InvalidArgument(format!("OFFSET expects a non-negative integer, got {:?}", other))),
                }
                if self.peek_ident_upper() == "ROWS" || self.peek() == &Token::Rows { self.pos += 1; }
            } else {
                break;
            }
        }
        Ok((limit, offset))
    }

    /// GROUP BY list. Plain column lists and a lone ROLLUP/CUBE over columns keep the original
    /// representation (fast executor paths); anything else becomes expressions + grouping sets.
    fn parse_group_by(&mut self, projections: &[Projection]) -> Result<(Vec<String>, Grouping, Vec<Expr>, Vec<Vec<usize>>), KoreError> {
        // every GROUP BY element contributes a list of alternative sets; the final sets are their product
        let mut elements: Vec<Vec<Vec<Expr>>> = Vec::new();
        let mut kinds: Vec<Option<Grouping>> = Vec::new();
        // GROUP BY ALL: every select item that is not an aggregate
        let all_mode = self.peek() == &Token::All;
        if all_mode {
            self.pos += 1;
            for p in projections {
                if let Projection::Expr { expr, .. } = p {
                    let has_agg = crate::ast_walk::any_node(expr, &|x| matches!(x, Expr::Agg { .. } | Expr::AggX { .. } | Expr::Window { .. }));
                    if !has_agg { elements.push(vec![vec![expr.clone()]]); kinds.push(None); }
                }
            }
            if elements.is_empty() { return Ok((Vec::new(), Grouping::Plain, Vec::new(), vec![Vec::new()])); }
        }
        loop {
            if all_mode { break; }
            let kw = self.peek_ident_upper();
            let next_is_paren = self.peek2() == &Token::LParen;
            if (kw == "ROLLUP" || kw == "CUBE") && next_is_paren {
                self.pos += 2;
                let mut items = vec![self.parse_group_item(projections)?];
                while self.consume_if(&Token::Comma) { items.push(self.parse_group_item(projections)?); }
                self.expect(&Token::RParen)?;
                elements.push(rollup_cube_sets(&items, kw == "ROLLUP"));
                kinds.push(Some(if kw == "ROLLUP" { Grouping::Rollup } else { Grouping::Cube }));
            } else if kw == "GROUPING" && matches!(self.peek2(), Token::Ident(s) if s.eq_ignore_ascii_case("SETS")) {
                self.pos += 2;
                self.expect(&Token::LParen)?;
                let mut sets: Vec<Vec<Expr>> = Vec::new();
                loop {
                    if self.consume_if(&Token::LParen) {
                        let mut set = Vec::new();
                        if self.peek() != &Token::RParen {
                            set.push(self.parse_group_item(projections)?);
                            while self.consume_if(&Token::Comma) { set.push(self.parse_group_item(projections)?); }
                        }
                        self.expect(&Token::RParen)?;
                        sets.push(set);
                    } else {
                        sets.push(vec![self.parse_group_item(projections)?]);
                    }
                    if !self.consume_if(&Token::Comma) { break; }
                }
                self.expect(&Token::RParen)?;
                elements.push(sets);
                kinds.push(None);
            } else {
                let e = self.parse_group_item(projections)?;
                elements.push(vec![vec![e]]);
                kinds.push(None);
            }
            if !self.consume_if(&Token::Comma) { break; }
        }
        // GROUP BY a, b WITH ROLLUP / WITH CUBE
        if self.peek() == &Token::With {
            let w = match self.tokens.get(self.pos + 1) { Some(Token::Ident(s)) => s.to_ascii_uppercase(), _ => String::new() };
            if w == "ROLLUP" || w == "CUBE" {
                self.pos += 2;
                let items: Vec<Expr> = elements.drain(..).flat_map(|e| e.into_iter().flatten()).collect();
                elements.push(rollup_cube_sets(&items, w == "ROLLUP"));
                kinds = vec![Some(if w == "ROLLUP" { Grouping::Rollup } else { Grouping::Cube })];
            }
        }

        let is_col = |e: &Expr| matches!(e, Expr::Col(_) | Expr::QualCol(..));
        let col_name = |e: &Expr| match e { Expr::Col(c) => c.clone(), Expr::QualCol(t, c) => format!("{t}.{c}"), _ => String::new() };
        // classic shapes: all single plain columns, or one ROLLUP/CUBE over plain columns
        if elements.iter().all(|e| e.len() == 1 && e[0].len() == 1 && is_col(&e[0][0])) {
            let cols: Vec<String> = elements.iter().map(|e| col_name(&e[0][0])).collect();
            return Ok((cols, Grouping::Plain, Vec::new(), Vec::new()));
        }
        if elements.len() == 1 {
            if let Some(kind) = kinds[0] {
                let all_cols = elements[0].iter().flatten().all(is_col);
                // the longest set lists every column in order
                if all_cols {
                    if let Some(full) = elements[0].iter().max_by_key(|s| s.len()) {
                        return Ok((full.iter().map(col_name).collect(), kind, Vec::new(), Vec::new()));
                    }
                }
            }
        }

        // general form
        let mut exprs: Vec<Expr> = Vec::new();
        let mut index_of = |e: &Expr| -> usize {
            match exprs.iter().position(|x| x == e) {
                Some(i) => i,
                None => { exprs.push(e.clone()); exprs.len() - 1 }
            }
        };
        let mut sets: Vec<Vec<usize>> = vec![Vec::new()];
        for el in &elements {
            let mut next = Vec::new();
            for base in &sets {
                for alt in el {
                    let mut s = base.clone();
                    for e in alt {
                        let i = index_of(e);
                        if !s.contains(&i) { s.push(i); }
                    }
                    next.push(s);
                }
            }
            sets = next;
        }
        let placeholders = (0..exprs.len()).map(|i| format!("__gexpr{i}")).collect();
        Ok((placeholders, Grouping::Plain, exprs, sets))
    }

    /// One GROUP BY item; an integer is a 1-based position in the select list.
    fn parse_group_item(&mut self, projections: &[Projection]) -> Result<Expr, KoreError> {
        let e = self.parse_expr(0)?;
        if let Expr::Int(n) = e {
            return match projections.get((n - 1).max(0) as usize) {
                Some(Projection::Expr { expr, .. }) if n >= 1 && !matches!(expr, Expr::Agg { .. } | Expr::AggX { .. }) => Ok(expr.clone()),
                _ => Err(KoreError::InvalidArgument(format!("GROUP BY position {n} is not in the select list"))),
            };
        }
        Ok(e)
    }

    // ── Window spec helpers ────────────────────────────────────────────────

    /// Parse list of CTEs: `name AS (select), ...`
    pub fn parse_cte_list(&mut self) -> Result<Vec<CteClause>, KoreError> {
        let mut ctes = vec![];
        loop {
            let name = self.expect_ident()?;
            let col_aliases = self.parse_col_aliases()?;
            self.expect(&Token::As)?;
            self.expect(&Token::LParen)?;
            let mut body = self.parse_compound()?;
            self.expect(&Token::RParen)?;
            if !col_aliases.is_empty() {
                // WITH c(a, b) AS (..): rename the body's output columns
                body = SelectStmt::star_from(TableExpr {
                    name: name.clone(), alias: Some(name.clone()), subquery: Some(Box::new(body)),
                    values: None, push_filter: None, col_aliases,
                });
            }
            ctes.push(CteClause { name, body });
            if !self.consume_if(&Token::Comma) { break; }
        }
        Ok(ctes)
    }

    /// Argument list and trailers of an aggregate call: `NAME([DISTINCT] args) [WITHIN GROUP (ORDER BY x)]
    /// [FILTER (WHERE c)] [OVER (..)]`. Calls the classic fast paths cover become `Expr::Agg`; everything
    /// else (DISTINCT over several columns, FILTER, population statistics, ...) becomes `Expr::AggX`.
    fn parse_aggregate(&mut self, name: &str) -> Result<Expr, KoreError> {
        self.expect(&Token::LParen)?;
        let distinct = self.consume_if(&Token::Distinct);
        if !distinct && self.peek() == &Token::All && self.peek2() != &Token::RParen { self.pos += 1; }
        let mut args: Vec<Expr> = Vec::new();
        if self.peek() == &Token::Star {
            self.pos += 1;
            args.push(Expr::Col("*".into()));
        } else if self.peek() != &Token::RParen {
            args.push(self.parse_expr(0)?);
            while self.consume_if(&Token::Comma) { args.push(self.parse_expr(0)?); }
        }
        self.expect(&Token::RParen)?;
        if args.is_empty() {
            return Err(KoreError::InvalidArgument(format!("{}() requires at least one argument", name.to_ascii_lowercase())));
        }

        // IGNORE NULLS / RESPECT NULLS on FIRST / LAST
        let mut ignore_nulls = false;
        if matches!(self.peek_ident_upper().as_str(), "IGNORE" | "RESPECT") {
            ignore_nulls = self.peek_ident_upper() == "IGNORE";
            self.pos += 1;
            if self.peek_ident_upper() == "NULLS" { self.pos += 1; }
        }
        // PERCENTILE_CONT(p) WITHIN GROUP (ORDER BY x [DESC])
        let mut within: Option<(Expr, bool)> = None;
        if self.peek_ident_upper() == "WITHIN" {
            self.pos += 1;
            self.expect(&Token::Group)?;
            self.expect(&Token::LParen)?;
            self.expect(&Token::Order)?;
            self.expect(&Token::By)?;
            let e = self.parse_expr(0)?;
            let desc = self.consume_if(&Token::Desc);
            if !desc { self.consume_if(&Token::Asc); }
            self.expect(&Token::RParen)?;
            within = Some((e, desc));
        }
        // FILTER (WHERE cond)
        let mut filter: Option<Box<Expr>> = None;
        if self.peek_ident_upper() == "FILTER" && self.peek2() == &Token::LParen {
            self.pos += 2;
            self.expect(&Token::Where)?;
            filter = Some(Box::new(self.parse_expr(0)?));
            self.expect(&Token::RParen)?;
        }

        // canonical names
        let mut nm = match name {
            "STD" | "STDDEV_SAMP" => "STDDEV",
            "VAR_SAMP" => "VARIANCE",
            "LISTAGG" | "GROUP_CONCAT" => "STRING_AGG",
            "ARRAY_AGG" => "COLLECT_LIST",
            "EVERY" => "BOOL_AND",
            "SOME" | "ANY" => "BOOL_OR",
            "APPROX_PERCENTILE" => "PERCENTILE_APPROX",
            other => other,
        }.to_string();
        if let Some((e, desc)) = within {
            // the percentile argument stays, the ordering expression becomes the first argument
            let mut a = vec![e];
            for x in args.drain(..) {
                a.push(match (&x, desc) {
                    (Expr::Float(p), true) => Expr::Float(1.0 - p),
                    (Expr::Int(p), true) => Expr::Float(1.0 - *p as f64),
                    _ => x,
                });
            }
            args = a;
        }
        if ignore_nulls && matches!(nm.as_str(), "FIRST" | "LAST") { args.push(Expr::Bool(true)); }
        if nm == "PERCENTILE_CONT" || nm == "PERCENTILE_DISC" { /* args = [x, p] already */ }

        // classic form?
        let classic = filter.is_none() && args.len() == 1
            && match nm.as_str() {
                "COUNT" => true,
                "SUM" | "AVG" | "MIN" | "MAX" => !distinct,
                "STDDEV" | "VARIANCE" | "MEDIAN" => !distinct,
                _ => false,
            };
        let func = match (nm.as_str(), distinct) {
            ("COUNT", true) => AggFunc::CountDistinct,
            ("COUNT", false) => AggFunc::Count,
            ("SUM", _) => AggFunc::Sum,
            ("AVG", _) => AggFunc::Avg,
            ("MIN", _) => AggFunc::Min,
            ("MAX", _) => AggFunc::Max,
            ("STDDEV", _) => AggFunc::Stddev,
            ("VARIANCE", _) => AggFunc::Variance,
            _ => AggFunc::Median,
        };

        if self.peek() == &Token::Over {
            if distinct { return Err(KoreError::InvalidArgument("DISTINCT is not supported in window functions".into())); }
            self.pos += 1;
            let spec = self.parse_window_spec()?;
            let wf = if classic {
                WindowFn::Agg { func, expr: Box::new(args.remove(0)) }
            } else {
                WindowFn::AggX { name: std::mem::take(&mut nm), args, filter }
            };
            return Ok(Expr::Window { func: wf, spec });
        }
        if classic {
            return Ok(Expr::Agg { func, expr: Box::new(args.remove(0)) });
        }
        Ok(Expr::AggX { name: nm, args, distinct, filter })
    }

    /// `OVER name`, `OVER (name? PARTITION BY .. ORDER BY .. frame?)` or a `WINDOW w AS (..)` body.
    fn parse_window_spec(&mut self) -> Result<WindowSpec, KoreError> {
        let mut spec = WindowSpec::default();
        if let Token::Ident(w) = self.peek().clone() {
            // OVER w
            self.pos += 1;
            spec.base = Some(w);
            return Ok(spec);
        }
        self.expect(&Token::LParen)?;
        // (w ORDER BY ..) refines a named window
        if let Token::Ident(w) = self.peek().clone() {
            if !matches!(w.to_ascii_uppercase().as_str(), "PARTITION" | "ORDER" | "ROWS" | "RANGE") {
                self.pos += 1;
                spec.base = Some(w);
            }
        }
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
            let mode = if self.peek() == &Token::Rows { FrameMode::Rows } else { FrameMode::Range };
            self.pos += 1;
            // BETWEEN a AND b, or a single bound (the end then defaults to CURRENT ROW)
            let between = match self.peek() {
                Token::Between => { self.pos += 1; true }
                Token::Ident(s) if s.eq_ignore_ascii_case("BETWEEN") => { self.pos += 1; true }
                _ => false,
            };
            let start = self.parse_frame_bound()?;
            let end = if between {
                self.expect(&Token::And)?;
                self.parse_frame_bound()?
            } else {
                FrameBound::CurrentRow
            };
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
                let n = self.parse_expr(10)?;
                Ok(if self.peek() == &Token::Preceding { self.pos += 1; FrameBound::Preceding(Box::new(n)) }
                   else { self.expect(&Token::Following)?; FrameBound::Following(Box::new(n)) })
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
            up @ ("LAG" | "LEAD") => {
                let e = self.parse_expr(0)?;
                let o = if self.consume_if(&Token::Comma) { self.parse_expr(0)? } else { Expr::Int(1) };
                let d = if self.consume_if(&Token::Comma) { Some(Box::new(self.parse_expr(0)?)) } else { None };
                if up == "LAG" { WindowFn::Lag { expr: Box::new(e), offset: Box::new(o), default: d } }
                else { WindowFn::Lead { expr: Box::new(e), offset: Box::new(o), default: d } }
            }
            up @ ("FIRST_VALUE" | "LAST_VALUE") => {
                let e = Box::new(self.parse_expr(0)?);
                // FIRST_VALUE(x, true) ignores NULLs
                let ignore = if self.consume_if(&Token::Comma) { matches!(self.parse_expr(0)?, Expr::Bool(true)) || false } else { false };
                match (up, ignore) {
                    ("FIRST_VALUE", false) => WindowFn::FirstValue(e),
                    ("FIRST_VALUE", true) => WindowFn::FirstValueIgnoreNulls(e),
                    (_, false) => WindowFn::LastValue(e),
                    (_, true) => WindowFn::LastValueIgnoreNulls(e),
                }
            }
            "NTH_VALUE" => {
                let e = self.parse_expr(0)?;
                self.expect(&Token::Comma)?;
                let n = self.parse_expr(0)?;
                WindowFn::NthValue { expr: Box::new(e), n: Box::new(n) }
            }
            "CUMSUM"|"CUM_SUM" => WindowFn::CumSum(Box::new(self.parse_expr(0)?)),
            other => return Err(KoreError::InvalidArgument(format!("unknown window fn: {other}"))),
        })
    }

    fn is_join_keyword(&self) -> bool {
        match self.peek() {
            Token::Join | Token::Inner | Token::Left | Token::Right | Token::Full | Token::Cross => true,
            Token::Ident(w) => w.eq_ignore_ascii_case("NATURAL")
                || ((w.eq_ignore_ascii_case("SEMI") || w.eq_ignore_ascii_case("ANTI")) && self.peek2() == &Token::Join),
            _ => false,
        }
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
        // alias.* expands to every column of one table
        if let (Token::Ident(t), Some(Token::Dot), Some(Token::Star)) = (self.peek().clone(), self.tokens.get(self.pos + 1), self.tokens.get(self.pos + 2)) {
            self.pos += 3;
            return Ok(Projection::Expr { expr: Expr::QualCol(t, "*".into()), alias: None });
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

    /// Optional `(c1, c2, ..)` column alias list after a table alias.
    fn parse_col_aliases(&mut self) -> Result<Vec<String>, KoreError> {
        let mut cols = Vec::new();
        if self.peek() == &Token::LParen {
            self.pos += 1;
            loop {
                cols.push(self.expect_alias()?);
                if !self.consume_if(&Token::Comma) { break; }
            }
            self.expect(&Token::RParen)?;
        }
        Ok(cols)
    }

    /// `VALUES (..), (..)` rows (the VALUES keyword already consumed).
    fn parse_values_rows(&mut self) -> Result<Vec<Vec<Expr>>, KoreError> {
        let mut rows: Vec<Vec<Expr>> = Vec::new();
        loop {
            let paren = self.consume_if(&Token::LParen);
            let mut row = vec![self.parse_expr(0)?];
            while self.consume_if(&Token::Comma) { row.push(self.parse_expr(0)?); }
            if paren { self.expect(&Token::RParen)?; }
            rows.push(row);
            // VALUES 1, 2 (bare) is one row per expression; VALUES (1), (2) has parentheses
            if !paren || !self.consume_if(&Token::Comma) { break; }
        }
        if rows.iter().any(|r| r.len() != rows[0].len()) {
            return Err(KoreError::InvalidArgument("VALUES rows must all have the same number of columns".into()));
        }
        Ok(rows)
    }

    /// Alias after a derived table or VALUES: `[AS] name [(c1, c2)]`.
    fn parse_derived_alias(&mut self, default: &str) -> Result<(String, Vec<String>), KoreError> {
        let alias = if self.consume_if(&Token::As) {
            self.expect_alias()?
        } else if matches!(self.peek(), Token::Ident(s) if !is_clause_word(s)) {
            self.expect_ident()?
        } else {
            default.to_string()
        };
        Ok((alias, self.parse_col_aliases()?))
    }

    fn parse_table_expr(&mut self) -> Result<TableExpr, KoreError> {
        // VALUES (r1c1, r1c2), (r2c1, r2c2) AS t(a, b) — inline table
        if self.peek_ident_upper() == "VALUES" {
            self.pos += 1;
            let rows = self.parse_values_rows()?;
            let (alias, col_aliases) = self.parse_derived_alias("_values")?;
            return Ok(TableExpr { name: alias.clone(), alias: Some(alias), subquery: None, values: Some(rows), push_filter: None, col_aliases });
        }

        // FROM (SELECT ...) alias — subquery as FROM table; also FROM (VALUES ...) and FROM ((SELECT ..) UNION ..)
        if self.peek() == &Token::LParen {
            self.pos += 1; // consume (
            if self.peek_ident_upper() == "VALUES" {
                self.pos += 1;
                let rows = self.parse_values_rows()?;
                self.expect(&Token::RParen)?;
                let (alias, col_aliases) = self.parse_derived_alias("_values")?;
                return Ok(TableExpr { name: alias.clone(), alias: Some(alias), subquery: None, values: Some(rows), push_filter: None, col_aliases });
            }
            let subq = self.parse_compound()?;
            self.expect(&Token::RParen)?;
            let (alias, col_aliases) = self.parse_derived_alias("_subq")?;
            return Ok(TableExpr { name: alias.clone(), alias: Some(alias), subquery: Some(Box::new(subq)), values: None, push_filter: None, col_aliases });
        }

        // Accept a string literal as table name (e.g. FROM 'data/file.parquet')
        let name = if matches!(self.peek(), Token::Str(_)) {
            match self.advance() { Token::Str(s) => s, _ => unreachable!() }
        } else if self.peek() == &Token::Range {
            // `range` is a window-frame keyword for the lexer; as a FROM item it is the table function
            self.pos += 1;
            "range".to_string()
        } else {
            self.expect_ident()?
        };
        // table-valued function: range(n) / range(start, end[, step])
        if name.eq_ignore_ascii_case("range") && self.peek() == &Token::LParen {
            self.pos += 1;
            let mut nums: Vec<i64> = Vec::new();
            loop {
                let neg = self.consume_if(&Token::Minus);
                match self.advance() {
                    Token::Int(n) => nums.push(if neg { -n } else { n }),
                    other => return Err(KoreError::InvalidArgument(format!("range() takes integer literals, got {:?}", other))),
                }
                if !self.consume_if(&Token::Comma) { break; }
            }
            self.expect(&Token::RParen)?;
            let (start, end, step) = match nums.as_slice() {
                [n] => (0, *n, 1),
                [a, b] => (*a, *b, 1),
                [a, b, c] => (*a, *b, *c),
                _ => return Err(KoreError::InvalidArgument("range() takes 1 to 3 arguments".into())),
            };
            if step == 0 { return Err(KoreError::InvalidArgument("range(): step cannot be 0".into())); }
            let count = if (step > 0 && end > start) || (step < 0 && end < start) { ((end - start).abs() + step.abs() - 1) / step.abs() } else { 0 };
            if count > 10_000_000 { return Err(KoreError::InvalidArgument("range() is limited to 10 million rows".into())); }
            let rows: Vec<Vec<Expr>> = (0..count).map(|i| vec![Expr::Int(start + i * step)]).collect();
            let (alias, mut col_aliases) = self.parse_derived_alias("range")?;
            if col_aliases.is_empty() { col_aliases = vec!["id".into()]; }
            return Ok(TableExpr { name: alias.clone(), alias: Some(alias), subquery: None, values: Some(rows), push_filter: None, col_aliases });
        }
        // schema-qualified names: db.table
        let name = if self.peek() == &Token::Dot && matches!(self.tokens.get(self.pos + 1), Some(Token::Ident(_))) {
            self.pos += 1;
            let t = self.expect_ident()?;
            format!("{name}.{t}")
        } else { name };
        let alias = if self.consume_if(&Token::As) {
            Some(self.expect_alias()?)
        } else if matches!(self.peek(), Token::Ident(s) if !is_clause_word(s))
               && !self.is_join_keyword()
               && self.peek() != &Token::Pivot
               && self.peek() != &Token::Unpivot
               && self.peek() != &Token::Lateral {
            Some(self.expect_ident()?)
        } else {
            None
        };
        Ok(TableExpr { name, alias, subquery: None, values: None, push_filter: None, col_aliases: vec![] })
    }

    // ─── JOIN clause ───────────────────────────────────────────────────────

    fn parse_join(&mut self) -> Result<JoinClause, KoreError> {
        let natural = if self.peek_ident_upper() == "NATURAL" { self.pos += 1; true } else { false };
        let join_type = match self.peek().clone() {
            Token::Inner => { self.pos += 1; self.expect(&Token::Join)?; JoinKind::Inner }
            Token::Left  => {
                self.pos += 1;
                let kind = match self.peek_ident_upper().as_str() {
                    "SEMI" => { self.pos += 1; JoinKind::Semi }
                    "ANTI" => { self.pos += 1; JoinKind::Anti }
                    _ => { self.consume_if(&Token::Outer); JoinKind::Left }
                };
                self.expect(&Token::Join)?;
                kind
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
            Token::Ident(w) if w.eq_ignore_ascii_case("SEMI") || w.eq_ignore_ascii_case("ANTI") => {
                self.pos += 1;
                self.expect(&Token::Join)?;
                if w.eq_ignore_ascii_case("SEMI") { JoinKind::Semi } else { JoinKind::Anti }
            }
            Token::Join  => { self.pos += 1; JoinKind::Inner }
            _ => return Err(KoreError::InvalidArgument("expected JOIN keyword".into())),
        };

        let table = self.parse_table_expr()?;
        let no_on = JoinOn { left_col: String::new(), right_col: String::new(), expr: None };

        // CROSS JOIN and NATURAL JOIN have no ON clause
        if join_type == JoinKind::Cross || natural {
            return Ok(JoinClause { join_type, table, on: no_on, push_filter: None, using: Vec::new(), natural });
        }

        // JOIN .. USING (a, b)
        if self.peek_ident_upper() == "USING" {
            self.pos += 1;
            self.expect(&Token::LParen)?;
            let mut using = vec![self.expect_alias()?];
            while self.consume_if(&Token::Comma) { using.push(self.expect_alias()?); }
            self.expect(&Token::RParen)?;
            return Ok(JoinClause { join_type, table, on: no_on, push_filter: None, using, natural: false });
        }

        // a bare inner JOIN without ON is a cross join
        if self.peek() != &Token::On && join_type == JoinKind::Inner {
            return Ok(JoinClause { join_type: JoinKind::Cross, table, on: no_on, push_filter: None, using: Vec::new(), natural: false });
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

        Ok(JoinClause { join_type, table, on: JoinOn { left_col, right_col, expr }, push_filter: None, using: Vec::new(), natural: false })
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
            // array subscript: Spark's `arr[i]` is 0-based, ELEMENT_AT is 1-based
            if self.peek() == &Token::LBracket {
                self.pos += 1;
                let idx = self.parse_expr(0)?;
                self.expect(&Token::RBracket)?;
                let one_based = Expr::BinOp { op: BinOpKind::Add, left: Box::new(idx), right: Box::new(Expr::Int(1)) };
                lhs = Expr::FuncCall { name: "ELEMENT_AT".to_string(), args: vec![lhs, one_based] };
                continue;
            }
            // Predicate operators (IS, [NOT] LIKE/IN/BETWEEN/RLIKE) bind tighter than AND/OR/NOT but
            // looser than arithmetic, so they only apply when the caller is not inside an arithmetic operand.
            if min_prec <= 3 {
                // IS [NOT] NULL / TRUE / FALSE / DISTINCT FROM
                if self.peek() == &Token::Is {
                    self.pos += 1;
                    let negated = self.consume_if(&Token::Not);
                    lhs = match self.peek().clone() {
                        Token::Null => {
                            self.pos += 1;
                            if negated { Expr::IsNotNull(Box::new(lhs)) } else { Expr::IsNull(Box::new(lhs)) }
                        }
                        Token::Distinct => {
                            self.pos += 1;
                            self.expect(&Token::From)?;
                            let rhs = self.parse_expr(4)?;
                            let eq = null_safe_eq(lhs, rhs);
                            // IS DISTINCT FROM = NOT null-safe-equal
                            if negated { eq } else { Expr::Not(Box::new(eq)) }
                        }
                        Token::Ident(w) if w.eq_ignore_ascii_case("TRUE") || w.eq_ignore_ascii_case("FALSE") || w.eq_ignore_ascii_case("UNKNOWN") => {
                            self.pos += 1;
                            let w = w.to_ascii_uppercase();
                            let coalesce = |e: Expr| Expr::FuncCall { name: "COALESCE".into(), args: vec![e, Expr::Bool(false)] };
                            match (w.as_str(), negated) {
                                ("TRUE", false) => coalesce(lhs),
                                ("TRUE", true) => Expr::Not(Box::new(coalesce(lhs))),
                                ("FALSE", false) => coalesce(Expr::Not(Box::new(lhs))),
                                ("FALSE", true) => Expr::Not(Box::new(coalesce(Expr::Not(Box::new(lhs))))),
                                (_, false) => Expr::IsNull(Box::new(lhs)),
                                (_, true) => Expr::IsNotNull(Box::new(lhs)),
                            }
                        }
                        other => return Err(KoreError::InvalidArgument(format!("expected NULL, TRUE, FALSE or DISTINCT after IS, got {:?}", other))),
                    };
                    continue;
                }
                // optional NOT before LIKE / ILIKE / IN / BETWEEN / RLIKE / REGEXP
                let mut negated = false;
                if self.peek() == &Token::Not {
                    let next = self.tokens.get(self.pos + 1).cloned().unwrap_or(Token::Eof);
                    let is_pred = match &next {
                        Token::In | Token::Like | Token::ILike | Token::Between => true,
                        Token::Ident(w) => w.eq_ignore_ascii_case("RLIKE") || w.eq_ignore_ascii_case("REGEXP"),
                        _ => false,
                    };
                    if is_pred { self.pos += 1; negated = true; }
                }
                match self.peek().clone() {
                    Token::Like => {
                        self.pos += 1;
                        let mut pat = self.parse_expr(5)?;
                        // LIKE 'x' ESCAPE 'c': rewritten to the default backslash escape
                        if self.peek_ident_upper() == "ESCAPE" {
                            self.pos += 1;
                            let esc = match self.advance() {
                                Token::Str(e) if e.chars().count() == 1 => e.chars().next().unwrap(),
                                other => return Err(KoreError::InvalidArgument(format!("ESCAPE needs a one-character string, got {:?}", other))),
                            };
                            pat = match pat {
                                Expr::Str(p) => {
                                    let mut out = String::new();
                                    let mut it = p.chars().peekable();
                                    while let Some(c) = it.next() {
                                        if c == esc {
                                            if let Some(n) = it.next() { out.push('\\'); out.push(n); }
                                        } else if c == '\\' { out.push_str("\\\\"); }
                                        else { out.push(c); }
                                    }
                                    Expr::Str(out)
                                }
                                _ => return Err(KoreError::InvalidArgument("LIKE ... ESCAPE needs a literal pattern".into())),
                            };
                        }
                        lhs = Expr::Like { expr: Box::new(lhs), pattern: Box::new(pat), negated };
                        continue;
                    }
                    Token::ILike => {
                        self.pos += 1;
                        let pat = self.parse_expr(5)?;
                        lhs = Expr::ILike { expr: Box::new(lhs), pattern: Box::new(pat), negated };
                        continue;
                    }
                    Token::Ident(w) if w.eq_ignore_ascii_case("RLIKE") || w.eq_ignore_ascii_case("REGEXP") => {
                        self.pos += 1;
                        let pat = self.parse_expr(5)?;
                        let call = Expr::FuncCall { name: "REGEXP_LIKE".into(), args: vec![lhs, pat] };
                        lhs = if negated { Expr::Not(Box::new(call)) } else { call };
                        continue;
                    }
                    Token::In => {
                        self.pos += 1;
                        self.expect(&Token::LParen)?;
                        if self.peek() == &Token::Select {
                            let stmt = self.parse_compound()?;
                            self.expect(&Token::RParen)?;
                            lhs = Expr::InSubquery { expr: Box::new(lhs), subquery: Box::new(stmt), negated };
                        } else {
                            let values = self.parse_expr_list()?;
                            self.expect(&Token::RParen)?;
                            lhs = Expr::In { expr: Box::new(lhs), values, negated };
                        }
                        continue;
                    }
                    Token::Between => {
                        self.pos += 1;
                        let low  = self.parse_expr(5)?;
                        self.expect(&Token::And)?;
                        let high = self.parse_expr(5)?;
                        lhs = Expr::Between { expr: Box::new(lhs), low: Box::new(low), high: Box::new(high), negated };
                        continue;
                    }
                    _ => {}
                }
            }
            // integer division operator
            if self.peek_ident_upper() == "DIV" && min_prec <= 9 {
                self.pos += 1;
                let rhs = self.parse_expr(10)?;
                lhs = Expr::FuncCall { name: "DIV".into(), args: vec![lhs, rhs] };
                continue;
            }
            let prec = infix_precedence(self.peek());
            if prec == 0 || prec < min_prec { break; }
            let op_tok = self.advance();
            // x <op> ANY|SOME|ALL (SELECT ..)
            if matches!(op_tok, Token::Eq | Token::Ne | Token::Lt | Token::Le | Token::Gt | Token::Ge) {
                let quant = match self.peek() {
                    Token::All => Some(true),
                    Token::Ident(w) if w.eq_ignore_ascii_case("ANY") || w.eq_ignore_ascii_case("SOME") => Some(false),
                    _ => None,
                };
                if let (Some(all), Token::LParen, Some(Token::Select)) = (quant, self.peek2().clone(), self.tokens.get(self.pos + 2)) {
                    self.pos += 2;
                    let sub = self.parse_compound()?;
                    self.expect(&Token::RParen)?;
                    lhs = Expr::QuantSubquery { expr: Box::new(lhs), op: tok_to_binop(&op_tok)?, all, subquery: Box::new(sub) };
                    continue;
                }
            }
            let rhs = self.parse_expr(prec + 1)?;
            if op_tok == Token::NullSafeEq {
                lhs = null_safe_eq(lhs, rhs);
                continue;
            }
            if let Some(f) = match op_tok { Token::Ampersand => Some("BITAND"), Token::Pipe => Some("BITOR"), Token::Caret => Some("BITXOR"), _ => None } {
                lhs = Expr::FuncCall { name: f.into(), args: vec![lhs, rhs] };
                continue;
            }
            let op = tok_to_binop(&op_tok)?;
            lhs = fold_decimal_literals(Expr::BinOp { op, left: Box::new(lhs), right: Box::new(rhs) });
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, KoreError> {
        // EXISTS (SELECT ...)
        if let Token::Ident(ref s) = self.peek().clone() {
            if s.eq_ignore_ascii_case("EXISTS") {
                self.pos += 1;
                self.expect(&Token::LParen)?;
                let stmt = self.parse_compound()?;
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
                    let stmt = self.parse_compound()?;
                    self.expect(&Token::RParen)?;
                    return Ok(Expr::Exists { subquery: Box::new(stmt), negated: true });
                }
            }
            // NOT binds looser than comparison and predicates: NOT a > 1 is NOT (a > 1)
            return Ok(Expr::Not(Box::new(self.parse_expr(3)?)));
        }
        // Unary minus / plus
        if self.peek() == &Token::Minus {
            self.pos += 1;
            let inner = self.parse_unary()?;
            return Ok(match inner {
                Expr::Int(n) => Expr::Int(n.wrapping_neg()),
                Expr::Float(f) => Expr::Float(-f),
                other => Expr::BinOp { left: Box::new(Expr::Int(0)), op: BinOpKind::Sub, right: Box::new(other) },
            });
        }
        if self.peek() == &Token::Plus {
            self.pos += 1;
            return self.parse_unary();
        }
        if self.peek() == &Token::Tilde {
            self.pos += 1;
            let inner = self.parse_unary()?;
            return Ok(Expr::FuncCall { name: "BITNOT".into(), args: vec![inner] });
        }
        self.parse_primary()
    }

    /// Function calls with their own syntax (CAST .. AS, TRIM(BOTH .. FROM ..), SUBSTRING(.. FROM .. FOR ..),
    /// POSITION(.. IN ..), OVERLAY(.. PLACING ..)). Called right after the opening parenthesis; returns
    /// None for ordinary calls with nothing consumed.
    fn parse_special_call(&mut self, up: &str) -> Result<Option<Expr>, KoreError> {
        match up {
            "CAST" | "TRY_CAST" => {
                let e = self.parse_expr(0)?;
                self.expect(&Token::As)?;
                let ty = self.expect_alias()?.to_ascii_uppercase();
                let mut args = vec![e, Expr::Col(ty)];
                if self.consume_if(&Token::LParen) {
                    loop {
                        match self.advance() {
                            Token::Int(n) => args.push(Expr::Int(n)),
                            other => return Err(KoreError::InvalidArgument(format!("bad type parameter {:?}", other))),
                        }
                        if !self.consume_if(&Token::Comma) { break; }
                    }
                    self.expect(&Token::RParen)?;
                }
                self.expect(&Token::RParen)?;
                Ok(Some(Expr::FuncCall { name: up.to_string(), args }))
            }
            "TRIM" => {
                let mode = self.peek_ident_upper();
                if matches!(mode.as_str(), "BOTH" | "LEADING" | "TRAILING") {
                    self.pos += 1;
                    let chars = if self.peek() == &Token::From { Expr::Str(" ".into()) } else { self.parse_expr(0)? };
                    self.expect(&Token::From)?;
                    let s = self.parse_expr(0)?;
                    self.expect(&Token::RParen)?;
                    return Ok(Some(Expr::FuncCall { name: format!("__TRIM_{mode}"), args: vec![s, chars] }));
                }
                if self.peek() == &Token::RParen { return Ok(None); }
                let first = self.parse_expr(0)?;
                if self.consume_if(&Token::From) {
                    let s = self.parse_expr(0)?;
                    self.expect(&Token::RParen)?;
                    return Ok(Some(Expr::FuncCall { name: "__TRIM_BOTH".into(), args: vec![s, first] }));
                }
                let mut args = vec![first];
                while self.consume_if(&Token::Comma) { args.push(self.parse_expr(0)?); }
                self.expect(&Token::RParen)?;
                Ok(Some(Expr::FuncCall { name: up.to_string(), args }))
            }
            "SUBSTRING" | "SUBSTR" => {
                let s = self.parse_expr(0)?;
                if self.consume_if(&Token::From) {
                    let a = self.parse_expr(0)?;
                    let mut args = vec![s, a];
                    if self.consume_if(&Token::For) { args.push(self.parse_expr(0)?); }
                    self.expect(&Token::RParen)?;
                    return Ok(Some(Expr::FuncCall { name: up.to_string(), args }));
                }
                let mut args = vec![s];
                while self.consume_if(&Token::Comma) { args.push(self.parse_expr(0)?); }
                self.expect(&Token::RParen)?;
                Ok(Some(Expr::FuncCall { name: up.to_string(), args }))
            }
            "POSITION" => {
                let sub = self.parse_expr(4)?;
                if self.consume_if(&Token::In) {
                    let s = self.parse_expr(0)?;
                    self.expect(&Token::RParen)?;
                    return Ok(Some(Expr::FuncCall { name: "LOCATE".into(), args: vec![sub, s] }));
                }
                let mut args = vec![sub];
                while self.consume_if(&Token::Comma) { args.push(self.parse_expr(0)?); }
                self.expect(&Token::RParen)?;
                Ok(Some(Expr::FuncCall { name: up.to_string(), args }))
            }
            "OVERLAY" => {
                let s = self.parse_expr(0)?;
                if self.peek_ident_upper() == "PLACING" {
                    self.pos += 1;
                    let rep = self.parse_expr(0)?;
                    self.expect(&Token::From)?;
                    let from = self.parse_expr(0)?;
                    let mut args = vec![s, rep, from];
                    if self.consume_if(&Token::For) { args.push(self.parse_expr(0)?); }
                    self.expect(&Token::RParen)?;
                    return Ok(Some(Expr::FuncCall { name: "OVERLAY".into(), args }));
                }
                let mut args = vec![s];
                while self.consume_if(&Token::Comma) { args.push(self.parse_expr(0)?); }
                self.expect(&Token::RParen)?;
                Ok(Some(Expr::FuncCall { name: up.to_string(), args }))
            }
            _ => Ok(None),
        }
    }

    /// `INTERVAL 1 DAY`, `INTERVAL '1' MONTH`, `INTERVAL '3 days'` -> INTERVAL(n, 'unit').
    fn parse_interval(&mut self) -> Result<Expr, KoreError> {
        let (n, unit): (Expr, String) = match self.advance() {
            Token::Str(s) => {
                let t = s.trim().to_string();
                match t.split_once(char::is_whitespace) {
                    Some((a, u)) => {
                        let n = a.parse::<i64>().map(Expr::Int).or_else(|_| a.parse::<f64>().map(Expr::Float))
                            .map_err(|_| KoreError::InvalidArgument(format!("bad interval '{s}'")))?;
                        (n, u.trim().to_string())
                    }
                    None => {
                        let n = t.parse::<i64>().map(Expr::Int).or_else(|_| t.parse::<f64>().map(Expr::Float))
                            .map_err(|_| KoreError::InvalidArgument(format!("bad interval '{s}'")))?;
                        (n, self.expect_alias()?)
                    }
                }
            }
            Token::Int(n) => (Expr::Int(n), self.expect_alias()?),
            Token::Float(f) => (Expr::Float(f), self.expect_alias()?),
            Token::Minus => {
                match self.advance() {
                    Token::Int(n) => (Expr::Int(-n), self.expect_alias()?),
                    other => return Err(KoreError::InvalidArgument(format!("bad interval value {:?}", other))),
                }
            }
            other => return Err(KoreError::InvalidArgument(format!("bad interval value {:?}", other))),
        };
        if crate::datetime::norm_unit(&unit).is_none() {
            return Err(KoreError::InvalidArgument(format!("unsupported interval unit '{unit}'")));
        }
        Ok(Expr::FuncCall { name: "INTERVAL".into(), args: vec![n, Expr::Str(unit)] })
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
            Token::Array if self.peek() == &Token::LParen => {
                self.expect(&Token::LParen)?;
                let elements = if self.peek() != &Token::RParen {
                    self.parse_expr_list()?
                } else { vec![] };
                self.expect(&Token::RParen)?;
                return Ok(Expr::Array(elements));
            }
            // MAP('a', 1, 'b', 2) → FuncCall("MAP", ...)
            Token::Map if self.peek() == &Token::LParen => {
                self.expect(&Token::LParen)?;
                let args = if self.peek() != &Token::RParen {
                    self.parse_expr_list()?
                } else { vec![] };
                self.expect(&Token::RParen)?;
                return Ok(Expr::FuncCall { name: "MAP".to_string(), args });
            }
            // EXPLODE(expr)
            Token::Explode if self.peek() == &Token::LParen => {
                self.expect(&Token::LParen)?;
                let inner = self.parse_expr(0)?;
                self.expect(&Token::RParen)?;
                return Ok(Expr::Explode(Box::new(inner)));
            }
            Token::LParen => {
                // If next token is SELECT → scalar subquery: (SELECT ...)
                if self.peek() == &Token::Select {
                    let stmt = self.parse_compound()?;
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
            // Aggregate functions (COUNT/SUM/AVG/MIN/MAX keywords and the named ones), optionally windowed
            Token::Count | Token::Sum | Token::Avg | Token::Min | Token::Max => {
                let nm = match &self.tokens[self.pos - 1] {
                    Token::Count => "COUNT", Token::Sum => "SUM", Token::Avg => "AVG", Token::Min => "MIN", _ => "MAX",
                };
                // not followed by '(': a column that happens to be called count / sum / avg ...
                if self.peek() != &Token::LParen {
                    return Ok(Expr::Col(nm.to_ascii_lowercase()));
                }
                self.parse_aggregate(nm)
            }
            Token::Ident(ref name) if self.peek() == &Token::LParen && is_agg_name(&name.to_ascii_uppercase()) => {
                let up = name.to_ascii_uppercase();
                self.parse_aggregate(&up)
            }
            // Identifier: plain column OR window function name
            Token::Ident(name) => {
                match name.to_ascii_uppercase().as_str() {
                    "ROW_NUMBER" | "RANK" | "DENSE_RANK" | "PERCENT_RANK" | "CUME_DIST" | "NTILE" |
                    "LAG" | "LEAD" | "FIRST_VALUE" | "LAST_VALUE" | "NTH_VALUE" | "CUMSUM" | "CUM_SUM"
                        if self.peek() == &Token::LParen => {
                        self.expect(&Token::LParen)?;
                        let mut wfn = self.parse_window_fn_args(&name)?;
                        self.expect(&Token::RParen)?;
                        // FIRST_VALUE(x) IGNORE NULLS
                        if matches!(self.peek_ident_upper().as_str(), "IGNORE" | "RESPECT") {
                            let ignore = self.peek_ident_upper() == "IGNORE";
                            self.pos += 1;
                            if self.peek_ident_upper() == "NULLS" { self.pos += 1; }
                            if ignore {
                                wfn = match wfn {
                                    WindowFn::FirstValue(e) => WindowFn::FirstValueIgnoreNulls(e),
                                    WindowFn::LastValue(e) => WindowFn::LastValueIgnoreNulls(e),
                                    other => other,
                                };
                            }
                        }
                        let spec = if self.peek() == &Token::Over {
                            self.pos += 1;
                            self.parse_window_spec()?
                        } else {
                            WindowSpec::default()
                        };
                        Ok(Expr::Window { func: wfn, spec })
                    }
                    _ => {
                        let up = name.to_ascii_uppercase();
                        if self.peek() == &Token::LParen {
                            self.pos += 1;
                            if let Some(special) = self.parse_special_call(&up)? { return Ok(special); }
                            // Scalar function call: UPPER(x), ROUND(x,2), etc.
                            let mut args = if self.peek() != &Token::RParen {
                                let first = self.parse_expr(0)?;
                                let mut a = vec![first];
                                while self.consume_if(&Token::Comma) {
                                    a.push(self.parse_expr(0)?);
                                }
                                a
                            } else { vec![] };
                            self.expect(&Token::RParen)?;
                            // CONCAT_WS(sep, collect_list(x)) / ARRAY_JOIN(collect_list(x), sep): string aggregation
                            let collected = |e: &Expr| matches!(e, Expr::AggX { name, .. } if name == "COLLECT_LIST" || name == "COLLECT_SET");
                            let join_args = match up.as_str() {
                                "CONCAT_WS" if args.len() == 2 && collected(&args[1]) => Some((args[1].clone(), args[0].clone())),
                                "ARRAY_JOIN" if args.len() == 2 && collected(&args[0]) => Some((args[0].clone(), args[1].clone())),
                                _ => None,
                            };
                            if let Some((Expr::AggX { name, args: inner, filter, .. }, sep)) = join_args {
                                let mut a = inner;
                                a.push(sep);
                                let agg = Expr::AggX { name: "STRING_AGG".into(), args: a, distinct: name == "COLLECT_SET", filter };
                                // an empty list joins to the empty string, not NULL
                                return Ok(Expr::FuncCall { name: "COALESCE".into(), args: vec![agg, Expr::Str(String::new())] });
                            }
                            // DATEADD(day, 5, d) / TIMESTAMPDIFF(month, a, b): the unit is a bare word
                            if args.len() == 3 && matches!(up.as_str(), "DATEADD" | "DATE_ADD" | "TIMESTAMPADD" | "DATEDIFF" | "DATE_DIFF" | "TIMESTAMPDIFF") {
                                if let Expr::Col(c) = &args[0] {
                                    if crate::datetime::norm_unit(c).is_some() { args[0] = Expr::Str(c.clone()); }
                                }
                            }
                            Ok(Expr::FuncCall { name: up, args })
                        } else if (up == "TRUE" || up == "FALSE") && self.peek() != &Token::Dot {
                            Ok(Expr::Bool(up == "TRUE"))
                        } else if matches!(up.as_str(), "CURRENT_DATE" | "CURRENT_TIMESTAMP" | "LOCALTIMESTAMP" | "CURRENT_USER") && self.peek() != &Token::Dot {
                            Ok(Expr::FuncCall { name: up, args: vec![] })
                        } else if (up == "DATE" || up == "TIMESTAMP") && matches!(self.peek(), Token::Str(_)) {
                            // DATE '2024-01-31' / TIMESTAMP '2024-01-31 10:00:00'
                            let lit = self.parse_primary()?;
                            Ok(Expr::FuncCall { name: if up == "DATE" { "TO_DATE".into() } else { "TO_TIMESTAMP".into() }, args: vec![lit] })
                        } else if up == "INTERVAL" && matches!(self.peek(), Token::Str(_) | Token::Int(_) | Token::Float(_) | Token::Minus) {
                            self.parse_interval()
                        } else if self.peek() == &Token::Dot {
                            self.pos += 1;
                            let col = self.expect_alias()?;
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
            ref t if non_reserved_word(t).is_some() && self.peek() != &Token::LParen => {
                let n = non_reserved_word(t).unwrap();
                if self.peek() == &Token::Dot {
                    // keyword-named table alias: rows.x
                    self.pos += 1;
                    let col = self.expect_alias()?;
                    return Ok(Expr::QualCol(n.to_string(), col));
                }
                Ok(Expr::Col(n.to_string()))
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
                let word = self.peek_ident_upper();
                if word != "FIRST" && word != "LAST" {
                    return Err(KoreError::InvalidArgument(format!("expected FIRST or LAST after NULLS, got {:?}", self.peek())));
                }
                self.pos += 1;
                Some(word == "FIRST")
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

/// Keywords that Spark does not reserve: they stay usable as column names.
fn non_reserved_word(t: &Token) -> Option<&'static str> {
    Some(match t {
        Token::Over => "over", Token::Partition => "partition", Token::Rows => "rows", Token::Range => "range",
        Token::Unbounded => "unbounded", Token::Preceding => "preceding", Token::Following => "following",
        Token::Current => "current", Token::Array => "array", Token::Map => "map", Token::Pivot => "pivot",
        Token::Unpivot => "unpivot", Token::Lateral => "lateral", Token::View => "view", Token::Temp => "temp",
        Token::Merge => "merge", Token::Explode => "explode", Token::Count => "count", Token::Sum => "sum",
        Token::Avg => "avg", Token::Min => "min", Token::Max => "max", Token::Left => "left", Token::Right => "right",
        Token::Create => "create", Token::Drop => "drop", Token::For => "for", Token::Extract => "extract",
        _ => return None,
    })
}

/// Aggregate functions that are not spelled with a dedicated keyword.
fn is_agg_name(up: &str) -> bool {
    matches!(up,
        "STDDEV" | "STDDEV_SAMP" | "STDDEV_POP" | "STD" | "VARIANCE" | "VAR_SAMP" | "VAR_POP" | "MEDIAN"
        | "PERCENTILE" | "PERCENTILE_APPROX" | "APPROX_PERCENTILE" | "PERCENTILE_CONT" | "PERCENTILE_DISC"
        | "STRING_AGG" | "LISTAGG" | "GROUP_CONCAT" | "COLLECT_LIST" | "COLLECT_SET" | "ARRAY_AGG"
        | "FIRST" | "LAST" | "ANY_VALUE" | "MODE" | "COUNT_IF" | "BOOL_AND" | "BOOL_OR" | "EVERY" | "SOME" | "ANY"
        | "MAX_BY" | "MIN_BY" | "APPROX_COUNT_DISTINCT" | "CORR" | "COVAR_POP" | "COVAR_SAMP"
        | "SKEWNESS" | "KURTOSIS" | "SUM_DISTINCT")
}

/// Words that end a table reference and so cannot be an implicit table alias.
fn is_clause_word(s: &str) -> bool {
    matches!(s.to_ascii_uppercase().as_str(),
        "WHERE" | "ORDER" | "GROUP" | "LIMIT" | "HAVING" | "QUALIFY" | "UNION" | "INTERSECT" | "EXCEPT" | "MINUS" | "FETCH"
        | "OFFSET" | "ON" | "SET" | "INTO" | "USING" | "NATURAL" | "WINDOW" | "SEMI" | "ANTI" | "TABLESAMPLE")
}

fn infix_precedence(tok: &Token) -> u8 {
    match tok {
        Token::Or              => 1,
        Token::And             => 2,
        Token::Eq | Token::Ne | Token::NullSafeEq => 3,
        Token::Lt | Token::Le
        | Token::Gt | Token::Ge => 4,
        Token::Pipe            => 5,
        Token::Caret           => 6,
        Token::Ampersand       => 7,
        Token::Plus | Token::Minus | Token::Concat => 8,
        Token::Star | Token::Slash | Token::Percent => 9,
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

/// Spark types `0.1`, `19.99` as DECIMAL, so `0.1 + 0.2` is exactly `0.3` there. Literal-only `+ - *` are therefore
/// evaluated in decimal arithmetic here and stored as the nearest double.
fn fold_decimal_literals(e: Expr) -> Expr {
    fn parts(e: &Expr) -> Option<(i128, u32)> {
        match e {
            Expr::Int(i) => Some((*i as i128, 0)),
            Expr::Float(f) if f.is_finite() => {
                let t = format!("{}", f);
                if t.contains('e') || t.contains('E') { return None; }
                let (ip, fp) = t.split_once('.').unwrap_or((&t, ""));
                if ip.trim_start_matches('-').len() + fp.len() > 17 { return None; }
                let m: i128 = format!("{ip}{fp}").parse().ok()?;
                Some((m, fp.len() as u32))
            }
            _ => None,
        }
    }
    let Expr::BinOp { op, left, right } = &e else { return e };
    if !matches!(op, BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul) { return e; }
    if !matches!((left.as_ref(), right.as_ref()), (Expr::Float(_), Expr::Float(_)) | (Expr::Float(_), Expr::Int(_)) | (Expr::Int(_), Expr::Float(_))) { return e; }
    let (Some((am, asc)), Some((bm, bsc))) = (parts(left), parts(right)) else { return e };
    let (mant, scale) = match op {
        BinOpKind::Mul => (am.checked_mul(bm), asc + bsc),
        _ => {
            let sc = asc.max(bsc);
            let (a2, b2) = (am.checked_mul(10i128.pow(sc - asc)), bm.checked_mul(10i128.pow(sc - bsc)));
            match (a2, b2) {
                (Some(a2), Some(b2)) => (if matches!(op, BinOpKind::Add) { a2.checked_add(b2) } else { a2.checked_sub(b2) }, sc),
                _ => (None, sc),
            }
        }
    };
    match mant {
        Some(m) if m.abs() < (1i128 << 100) && scale <= 30 => Expr::Float(m as f64 / 10f64.powi(scale as i32)),
        _ => e,
    }
}

/// ROLLUP(a, b, c) -> [a b c], [a b], [a], []; CUBE(a, b) -> every subset.
fn rollup_cube_sets(items: &[Expr], rollup: bool) -> Vec<Vec<Expr>> {
    if rollup {
        (0..=items.len()).rev().map(|n| items[..n].to_vec()).collect()
    } else {
        let n = items.len();
        (0..(1usize << n)).rev().map(|mask| (0..n).filter(|i| mask & (1 << (n - 1 - i)) != 0).map(|i| items[i].clone()).collect()).collect()
    }
}

/// `a <=> b`: equality that treats NULL = NULL as true and NULL = x as false.
fn null_safe_eq(a: Expr, b: Expr) -> Expr {
    let both_null = Expr::BinOp {
        op: BinOpKind::And,
        left: Box::new(Expr::IsNull(Box::new(a.clone()))),
        right: Box::new(Expr::IsNull(Box::new(b.clone()))),
    };
    let eq = Expr::BinOp { op: BinOpKind::Eq, left: Box::new(a), right: Box::new(b) };
    Expr::FuncCall { name: "COALESCE".into(), args: vec![eq, both_null] }
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

