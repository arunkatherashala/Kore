use crate::ast::*;
use crate::tokenizer::{Token, Tokenizer};

pub struct SQLParser<'a> {
    tokenizer: Tokenizer,
    _input: &'a str,
}

#[derive(Debug)]
pub enum ParseError {
    UnexpectedToken(String),
    Eof,
}

impl<'a> SQLParser<'a> {
    pub fn new(input: &'a str) -> Self {
        Self { tokenizer: Tokenizer::new(input), _input: input }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.tokenizer.next() {
            Token::Ident(s) => Ok(s),
            t => Err(ParseError::UnexpectedToken(format!("expected ident, got {:?}", t))),
        }
    }

    pub fn parse(&mut self) -> Result<Stmt, ParseError> {
        match self.tokenizer.next() {
            Token::Select => self.parse_select(),
            _ => Err(ParseError::UnexpectedToken("expected SELECT".to_string())),
        }
    }

    fn parse_select(&mut self) -> Result<Stmt, ParseError> {
        let mut cols = Vec::new();
        match self.tokenizer.peek() {
            Token::Star => { self.tokenizer.next(); cols.push("*".to_string()); }
            _ => {
                loop {
                    let ident = self.expect_ident()?;
                    cols.push(ident);
                    match self.tokenizer.peek() {
                        Token::Comma => { self.tokenizer.next(); continue }
                        _ => break,
                    }
                }
            }
        }

        match self.tokenizer.next() {
            Token::From => {}
            t => return Err(ParseError::UnexpectedToken(format!("expected FROM, got {:?}", t))),
        }

        let table = self.expect_ident()?;
        // optional alias for base table
        if let Token::As = self.tokenizer.peek() { self.tokenizer.next(); let _ = self.expect_ident()?; }
        else if let Token::Ident(_) = self.tokenizer.peek() { let _ = self.expect_ident()?; }

        let mut joins = Vec::new();
        loop {
            match self.tokenizer.peek() {
                Token::Join | Token::Left | Token::Right | Token::Full | Token::Cross => {
                    // determine join kind
                    let kind = match self.tokenizer.peek() {
                        Token::Left => { self.tokenizer.next(); JoinKind::Left }
                        Token::Right => { self.tokenizer.next(); JoinKind::Right }
                        Token::Full => { self.tokenizer.next(); JoinKind::Full }
                        Token::Cross => { self.tokenizer.next(); JoinKind::Cross }
                        _ => JoinKind::Inner,
                    };
                    // consume any modifier tokens until we hit JOIN
                    loop {
                        match self.tokenizer.peek() {
                            Token::Outer | Token::As => { self.tokenizer.next(); continue }
                            Token::Join => { break }
                            _ => break,
                        }
                    }
                    // now expect JOIN
                    match self.tokenizer.next() {
                        Token::Join => {}
                        t => return Err(ParseError::UnexpectedToken(format!("expected JOIN, got {:?}", t))),
                    }

                    let jt = self.expect_ident()?;
                    // optional alias (AS? ident)
                    let mut alias = None;
                    if let Token::As = self.tokenizer.peek() { self.tokenizer.next(); alias = Some(self.expect_ident()?); }
                    else if let Token::Ident(_) = self.tokenizer.peek() { alias = Some(self.expect_ident()?); }

                    // decide how to parse the join condition: prefer ON, but allow end-of-statement for CROSS/FULL
                    match self.tokenizer.peek() {
                        Token::On => {
                            self.tokenizer.next();
                            let on_expr = self.parse_expr()?;
                            joins.push(Join { kind, table: jt, alias, on: on_expr });
                        }
                        Token::Using => {
                            // parse USING (col1, col2)
                            self.tokenizer.next();
                            // expect '('
                            match self.tokenizer.next() {
                                Token::Op(s) if s == "(" => {}
                                t => return Err(ParseError::UnexpectedToken(format!("expected '(', got {:?}", t))),
                            }
                            let mut cols = Vec::new();
                            loop {
                                let c = self.expect_ident()?;
                                cols.push(c);
                                match self.tokenizer.peek() {
                                    Token::Comma => { self.tokenizer.next(); continue }
                                    _ => break,
                                }
                            }
                            // expect ')'
                            match self.tokenizer.next() {
                                Token::Op(s) if s == ")" => {}
                                t => return Err(ParseError::UnexpectedToken(format!("expected ')', got {:?}", t))),
                            }
                            // create a placeholder Expr storing columns as comma-joined right side
                            let rhs = cols.join(",");
                            joins.push(Join { kind, table: jt, alias, on: Expr::Binary { left: "USING".to_string(), op: "".to_string(), right: rhs } });
                        }
                        Token::Where | Token::Eof | Token::Join | Token::Comma => {
                            // no condition provided; treat as empty
                            joins.push(Join { kind, table: jt, alias, on: Expr::Binary { left: "".to_string(), op: "".to_string(), right: "".to_string() } });
                        }
                        _ => {
                            if let JoinKind::Cross = kind {
                                joins.push(Join { kind, table: jt, alias, on: Expr::Binary { left: "".to_string(), op: "".to_string(), right: "".to_string() } });
                            } else {
                                return Err(ParseError::UnexpectedToken(format!("expected ON or USING, got {:?}", self.tokenizer.peek())));
                            }
                        }
                    }
                }
                _ => break,
            }
        }

        let where_clause = match self.tokenizer.peek() {
            Token::Where => {
                self.tokenizer.next();
                Some(self.parse_expr()?)
            }
            _ => None,
        };

        Ok(Stmt { kind: StmtKind::Select, select: Some(SelectStmt { columns: cols, from: table, joins, where_clause }) })
    }
}

impl<'a> SQLParser<'a> {
    // parse expression with precedence: OR (low) -> AND -> primary
    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and()?;
        loop {
            if let Token::Op(op) = self.tokenizer.peek().clone() {
                if op.eq_ignore_ascii_case("OR") {
                    self.tokenizer.next();
                    let right = self.parse_and()?;
                    left = Expr::Logical { left: Box::new(left), op: "OR".to_string(), right: Box::new(right) };
                    continue;
                }
            }
            break;
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_primary()?;
        loop {
            if let Token::Op(op) = self.tokenizer.peek().clone() {
                if op.eq_ignore_ascii_case("AND") {
                    self.tokenizer.next();
                    let right = self.parse_primary()?;
                    left = Expr::Logical { left: Box::new(left), op: "AND".to_string(), right: Box::new(right) };
                    continue;
                }
            }
            break;
        }
        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.tokenizer.peek().clone() {
            Token::Op(ref s) if s == "(" => {
                self.tokenizer.next();
                let inner = self.parse_expr()?;
                // expect ')'
                match self.tokenizer.next() {
                    Token::Op(ref s2) if s2 == ")" => Ok(Expr::Paren(Box::new(inner))),
                    t => Err(ParseError::UnexpectedToken(format!("expected ), got {:?}", t))),
                }
            }
            Token::Ident(_) => {
                // binary: IDENT OP (IDENT|NUMBER)
                let left = self.expect_ident()?;
                let op = match self.tokenizer.next() {
                    Token::Op(s) => s,
                    t => return Err(ParseError::UnexpectedToken(format!("expected op, got {:?}", t))),
                };
                let right = match self.tokenizer.next() {
                    Token::Ident(s) => s,
                    Token::Number(n) => n,
                    t => return Err(ParseError::UnexpectedToken(format!("expected literal, got {:?}", t))),
                };
                Ok(Expr::Binary { left, op, right })
            }
            t => Err(ParseError::UnexpectedToken(format!("unexpected token in expression: {:?}", t))),
        }
    }
}
