//! KQL lexer — converts raw SQL text into a flat Vec<Token>.

use kore_core::KoreError;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // ─── Keywords ─────────────────────────────────────────────────
    Select, From, Where, Join, On, Inner, Left, Full, Outer, Right,
    As, Group, By, Order, Asc, Desc, Limit, And, Or, Not, Distinct,
    Count, Sum, Avg, Min, Max, Is, Null, Having, Union, All, With,
    Like, In, Case, When, Then, Else, End, Between,
    Intersect, Except, Merge, Extract,
    // ─── Window function keywords ─────────────────────────────────
    Over, Partition, Rows, Range, Unbounded, Preceding, Following, Current,
    // ─── Punctuation / operators ──────────────────────────────────
    Star, Comma, Dot, LParen, RParen, Semicolon,
    Eq, Ne, Lt, Le, Gt, Ge, Plus, Minus, Slash, Percent,
    // ─── Literals ─────────────────────────────────────────────────
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    // ─── Meta ─────────────────────────────────────────────────────
    Eof,
}

pub struct Lexer {
    chars: Vec<char>,
    pos:   usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self { chars: input.chars().collect(), pos: 0 }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, KoreError> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next()?;
            let done = tok == Token::Eof;
            tokens.push(tok);
            if done { break; }
        }
        Ok(tokens)
    }

    fn peek(&self) -> Option<char> { self.chars.get(self.pos).copied() }
    fn peek2(&self) -> Option<char> { self.chars.get(self.pos + 1).copied() }
    fn advance(&mut self) -> char {
        let c = self.chars[self.pos]; self.pos += 1; c
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // whitespace
            while self.peek().map_or(false, |c| c.is_whitespace()) { self.pos += 1; }
            // single-line comment `--`
            if self.peek() == Some('-') && self.peek2() == Some('-') {
                while self.peek().map_or(false, |c| c != '\n') { self.pos += 1; }
            } else {
                break;
            }
        }
    }

    fn next(&mut self) -> Result<Token, KoreError> {
        self.skip_whitespace_and_comments();
        let Some(c) = self.peek() else { return Ok(Token::Eof); };
        Ok(match c {
            '*'  => { self.pos += 1; Token::Star }
            ','  => { self.pos += 1; Token::Comma }
            '.'  => { self.pos += 1; Token::Dot }
            '('  => { self.pos += 1; Token::LParen }
            ')'  => { self.pos += 1; Token::RParen }
            ';'  => { self.pos += 1; Token::Semicolon }
            '+'  => { self.pos += 1; Token::Plus }
            '-'  => { self.pos += 1; Token::Minus }
            '/'  => { self.pos += 1; Token::Slash }
            '%'  => { self.pos += 1; Token::Percent }
            '='  => { self.pos += 1; Token::Eq }
            '!'  if self.peek2() == Some('=') => { self.pos += 2; Token::Ne }
            '<'  => { if self.peek2() == Some('=') { self.pos += 2; Token::Le }
                      else if self.peek2() == Some('>') { self.pos += 2; Token::Ne }  // <> = !=
                      else { self.pos += 1; Token::Lt } }
            '>'  => { if self.peek2() == Some('=') { self.pos += 2; Token::Ge }
                      else { self.pos += 1; Token::Gt } }
            '\'' => self.read_str()?,
            c if c.is_ascii_digit() => self.read_number()?,
            c if c.is_alphabetic() || c == '_' => self.read_ident(),
            other => return Err(KoreError::InvalidArgument(format!("unexpected char {:?} at pos {}", other, self.pos))),
        })
    }

    fn read_str(&mut self) -> Result<Token, KoreError> {
        self.pos += 1; // skip opening '
        let mut s = String::new();
        loop {
            match self.peek() {
                None => return Err(KoreError::InvalidArgument("unterminated string".into())),
                Some('\'') => { self.pos += 1; break; }
                Some(c) => { s.push(c); self.pos += 1; }
            }
        }
        Ok(Token::Str(s))
    }

    fn read_number(&mut self) -> Result<Token, KoreError> {
        let start = self.pos;
        let mut is_float = false;
        while self.peek().map_or(false, |c| c.is_ascii_digit() || c == '.') {
            if self.peek() == Some('.') { is_float = true; }
            self.pos += 1;
        }
        // scientific notation
        if self.peek() == Some('e') || self.peek() == Some('E') {
            is_float = true;
            self.pos += 1;
            if self.peek() == Some('+') || self.peek() == Some('-') { self.pos += 1; }
            while self.peek().map_or(false, |c| c.is_ascii_digit()) { self.pos += 1; }
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        if is_float {
            s.parse::<f64>()
             .map(Token::Float)
             .map_err(|_| KoreError::InvalidArgument(format!("bad float: {s}")))
        } else {
            s.parse::<i64>()
             .map(Token::Int)
             .map_err(|_| KoreError::InvalidArgument(format!("bad int: {s}")))
        }
    }

    fn read_ident(&mut self) -> Token {
        let start = self.pos;
        while self.peek().map_or(false, |c| c.is_alphanumeric() || c == '_') {
            self.pos += 1;
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        match s.to_ascii_uppercase().as_str() {
            "SELECT"   => Token::Select,
            "FROM"     => Token::From,
            "WHERE"    => Token::Where,
            "JOIN"     => Token::Join,
            "ON"       => Token::On,
            "INNER"    => Token::Inner,
            "LEFT"     => Token::Left,
            "RIGHT"    => Token::Right,
            "FULL"     => Token::Full,
            "OUTER"    => Token::Outer,
            "AS"       => Token::As,
            "GROUP"    => Token::Group,
            "BY"       => Token::By,
            "ORDER"    => Token::Order,
            "ASC"      => Token::Asc,
            "DESC"     => Token::Desc,
            "LIMIT"    => Token::Limit,
            "AND"      => Token::And,
            "OR"       => Token::Or,
            "NOT"      => Token::Not,
            "DISTINCT" => Token::Distinct,
            "COUNT"    => Token::Count,
            "SUM"      => Token::Sum,
            "AVG"      => Token::Avg,
            "MIN"      => Token::Min,
            "MAX"      => Token::Max,
            "IS"       => Token::Is,
            "NULL"     => Token::Null,
            "HAVING"   => Token::Having,
            "UNION"    => Token::Union,
            "ALL"      => Token::All,
            "WITH"     => Token::With,
            "LIKE"     => Token::Like,
            "IN"       => Token::In,
            "CASE"     => Token::Case,
            "WHEN"     => Token::When,
            "THEN"     => Token::Then,
            "ELSE"     => Token::Else,
            "END"      => Token::End,
            "BETWEEN"  => Token::Between,
            // window
            "OVER"      => Token::Over,
            "PARTITION" => Token::Partition,
            "ROWS"      => Token::Rows,
            "RANGE"     => Token::Range,
            "UNBOUNDED" => Token::Unbounded,
            "PRECEDING" => Token::Preceding,
            "FOLLOWING" => Token::Following,
            "CURRENT"   => Token::Current,
            "INTERSECT" => Token::Intersect,
            "EXCEPT"    => Token::Except,
            "MERGE"     => Token::Merge,
            "EXTRACT"   => Token::Extract,
            _           => Token::Ident(s),
        }
    }
}

