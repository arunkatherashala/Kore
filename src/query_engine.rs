/// KORE Quick MVP Query Engine
/// Minimal viable SQL query processor with basic SELECT, WHERE, and LIMIT support

use std::collections::HashMap;

/// SQL token types
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Select,
    From,
    Where,
    And,
    Or,
    Limit,
    Join,
    Inner,
    Left,
    Right,
    On,
    Star,
    Comma,
    Equal,
    Greater,
    Less,
    Dot,
    Identifier(String),
    Number(f64),
    String(String),
    Eof,
}

/// SQL Query
#[derive(Debug, Clone)]
pub struct Query {
    pub select_cols: Vec<String>,
    pub table: String,
    pub joins: Vec<JoinClause>,
    pub filters: Vec<Filter>,
    pub limit: Option<usize>,
}

/// JOIN clause
#[derive(Debug, Clone)]
pub struct JoinClause {
    pub join_type: JoinType,
    pub table: String,
    pub left_col: String,
    pub right_col: String,
}

/// JOIN types
#[derive(Debug, Clone, PartialEq)]
pub enum JoinType {
    Inner,
    Left,
    Right,
}

/// Filter condition
#[derive(Debug, Clone)]
pub struct Filter {
    pub column: String,
    pub operator: FilterOp,
    pub value: Value,
}

/// Filter operators
#[derive(Debug, Clone, PartialEq)]
pub enum FilterOp {
    Equal,
    Greater,
    Less,
    GreaterEqual,
    LessEqual,
    NotEqual,
}

/// Value types
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Null,
}

impl Value {
    /// Compare values
    pub fn compare(&self, other: &Self, op: &FilterOp) -> bool {
        match (self, other, op) {
            (Value::Int(a), Value::Int(b), FilterOp::Equal) => a == b,
            (Value::Int(a), Value::Int(b), FilterOp::Greater) => a > b,
            (Value::Int(a), Value::Int(b), FilterOp::Less) => a < b,
            (Value::Int(a), Value::Int(b), FilterOp::GreaterEqual) => a >= b,
            (Value::Int(a), Value::Int(b), FilterOp::LessEqual) => a <= b,
            (Value::Int(a), Value::Int(b), FilterOp::NotEqual) => a != b,
            
            (Value::Float(a), Value::Float(b), FilterOp::Equal) => (a - b).abs() < f64::EPSILON,
            (Value::Float(a), Value::Float(b), FilterOp::Greater) => a > b,
            (Value::Float(a), Value::Float(b), FilterOp::Less) => a < b,
            (Value::Float(a), Value::Float(b), FilterOp::GreaterEqual) => a >= b,
            (Value::Float(a), Value::Float(b), FilterOp::LessEqual) => a <= b,
            (Value::Float(a), Value::Float(b), FilterOp::NotEqual) => (a - b).abs() > f64::EPSILON,
            
            (Value::String(a), Value::String(b), FilterOp::Equal) => a == b,
            (Value::String(a), Value::String(b), FilterOp::NotEqual) => a != b,
            
            _ => false,
        }
    }
}

/// Lexer for SQL tokenization
pub struct Lexer {
    input: Vec<char>,
    pos: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            pos: 0,
        }
    }

    fn current_char(&self) -> Option<char> {
        if self.pos < self.input.len() {
            Some(self.input[self.pos])
        } else {
            None
        }
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.current_char() {
            if ch.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn read_identifier(&mut self) -> String {
        let mut result = String::new();
        while let Some(ch) = self.current_char() {
            if ch.is_alphanumeric() || ch == '_' {
                result.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        result
    }

    fn read_number(&mut self) -> f64 {
        let mut result = String::new();
        while let Some(ch) = self.current_char() {
            if ch.is_numeric() || ch == '.' {
                result.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        result.parse().unwrap_or(0.0)
    }

    pub fn next_token(&mut self) -> Token {
        self.skip_whitespace();

        match self.current_char() {
            None => Token::Eof,
            Some('*') => {
                self.advance();
                Token::Star
            }
            Some(',') => {
                self.advance();
                Token::Comma
            }
            Some('=') => {
                self.advance();
                Token::Equal
            }
            Some('>') => {
                self.advance();
                Token::Greater
            }
            Some('<') => {
                self.advance();
                Token::Less
            }
            Some('.') => {
                self.advance();
                Token::Dot
            }
            Some('"') | Some('\'') => {
                self.advance();
                let quote = self.input[self.pos - 1];
                let mut result = String::new();
                while let Some(ch) = self.current_char() {
                    if ch == quote {
                        self.advance();
                        break;
                    }
                    result.push(ch);
                    self.advance();
                }
                Token::String(result)
            }
            Some(ch) if ch.is_numeric() => Token::Number(self.read_number()),
            Some(ch) if ch.is_alphabetic() => {
                let ident = self.read_identifier();
                match ident.to_uppercase().as_str() {
                    "SELECT" => Token::Select,
                    "FROM" => Token::From,
                    "WHERE" => Token::Where,
                    "AND" => Token::And,
                    "OR" => Token::Or,
                    "LIMIT" => Token::Limit,
                    "JOIN" => Token::Join,
                    "INNER" => Token::Inner,
                    "LEFT" => Token::Left,
                    "RIGHT" => Token::Right,
                    "ON" => Token::On,
                    _ => Token::Identifier(ident),
                }
            }
            Some(_) => {
                self.advance();
                self.next_token()
            }
        }
    }
}

/// SQL Parser
pub struct Parser {
    lexer: Lexer,
    current_token: Token,
}

impl Parser {
    pub fn new(input: &str) -> Self {
        let mut lexer = Lexer::new(input);
        let current_token = lexer.next_token();
        Self { lexer, current_token }
    }

    fn advance(&mut self) {
        self.current_token = self.lexer.next_token();
    }

    pub fn parse(&mut self) -> Result<Query, String> {
        // Expect SELECT
        if self.current_token != Token::Select {
            return Err("Expected SELECT".to_string());
        }
        self.advance();

        // Parse columns
        let select_cols = self.parse_select_list()?;

        // Expect FROM
        if self.current_token != Token::From {
            return Err("Expected FROM".to_string());
        }
        self.advance();

        // Parse table
        let table = match &self.current_token {
            Token::Identifier(name) => { let name = name.clone();
                let t = name.clone();
                self.advance();
                t
            }
            _ => return Err("Expected table name".to_string()),
        };

        // Parse JOIN clauses (optional)
        let joins = self.parse_joins()?;

        // Parse WHERE clause (optional)
        let filters = if self.current_token == Token::Where {
            self.advance();
            self.parse_where_clause()?
        } else {
            Vec::new()
        };

        // Parse LIMIT clause (optional)
        let limit = if self.current_token == Token::Limit {
            self.advance();
            match &self.current_token {
                Token::Number(n) => {
                    let l = Some(*n as usize);
                    self.advance();
                    l
                }
                _ => return Err("Expected number after LIMIT".to_string()),
            }
        } else {
            None
        };

        Ok(Query {
            select_cols,
            table,
            joins,
            filters,
            limit,
        })
    }

    fn parse_select_list(&mut self) -> Result<Vec<String>, String> {
        let mut cols = Vec::new();

        loop {
            match &self.current_token {
                Token::Star => {
                    cols.push("*".to_string());
                    self.advance();
                }
                Token::Identifier(name) => { let name = name.clone();
                    cols.push(name.clone());
                    self.advance();
                }
                _ => return Err("Expected column name".to_string()),
            }

            if self.current_token == Token::Comma {
                self.advance();
            } else {
                break;
            }
        }

        Ok(cols)
    }

    fn parse_where_clause(&mut self) -> Result<Vec<Filter>, String> {
        let mut filters = Vec::new();

        loop {
            let col = match &self.current_token {
                Token::Identifier(name) => { let name = name.clone();
                    let c = name.clone();
                    self.advance();
                    c
                }
                _ => return Err("Expected column name in WHERE".to_string()),
            };

            let op = match &self.current_token {
                Token::Equal => FilterOp::Equal,
                Token::Greater => FilterOp::Greater,
                Token::Less => FilterOp::Less,
                _ => return Err("Expected operator".to_string()),
            };
            self.advance();

            let val = match &self.current_token {
                Token::Number(n) => {
                    let v = Value::Int(*n as i64);
                    self.advance();
                    v
                }
                Token::String(s) => {
                    let v = Value::String(s.clone());
                    self.advance();
                    v
                }
                _ => return Err("Expected value".to_string()),
            };

            filters.push(Filter {
                column: col,
                operator: op,
                value: val,
            });

            if self.current_token == Token::And {
                self.advance();
            } else {
                break;
            }
        }

        Ok(filters)
    }

    fn parse_joins(&mut self) -> Result<Vec<JoinClause>, String> {
        let mut joins = Vec::new();

        while self.current_token == Token::Join 
            || self.current_token == Token::Inner 
            || self.current_token == Token::Left 
            || self.current_token == Token::Right {
            
            let join_type = if self.current_token == Token::Inner {
                self.advance();
                if self.current_token != Token::Join {
                    return Err("Expected JOIN after INNER".to_string());
                }
                JoinType::Inner
            } else if self.current_token == Token::Left {
                self.advance();
                if self.current_token != Token::Join {
                    return Err("Expected JOIN after LEFT".to_string());
                }
                JoinType::Left
            } else if self.current_token == Token::Right {
                self.advance();
                if self.current_token != Token::Join {
                    return Err("Expected JOIN after RIGHT".to_string());
                }
                JoinType::Right
            } else {
                JoinType::Inner
            };

            self.advance(); // Skip JOIN token

            // Parse table name
            let join_table = match &self.current_token {
                Token::Identifier(name) => { let name = name.clone();
                    let t = name.clone();
                    self.advance();
                    t
                }
                _ => return Err("Expected table name after JOIN".to_string()),
            };

            // Expect ON
            if self.current_token != Token::On {
                return Err("Expected ON after JOIN table".to_string());
            }
            self.advance();

            // Parse ON condition (e.g., table1.id = table2.id)
            let left_col = self.parse_qualified_column()?;
            
            if self.current_token != Token::Equal {
                return Err("Expected = in JOIN ON condition".to_string());
            }
            self.advance();

            let right_col = self.parse_qualified_column()?;

            joins.push(JoinClause {
                join_type,
                table: join_table,
                left_col,
                right_col,
            });
        }

        Ok(joins)
    }

    fn parse_qualified_column(&mut self) -> Result<String, String> {
        let col = match &self.current_token {
            Token::Identifier(name) => name.clone(),
            _ => return Err("Expected column name".to_string()),
        };
        self.advance();

        // Handle table.column format
        if self.current_token == Token::Dot {
            self.advance();
            match &self.current_token {
                Token::Identifier(name) => { let name = name.clone();
                    self.advance();
                    Ok(format!("{}.{}", col, name))
                }
                _ => Err("Expected column name after dot".to_string()),
            }
        } else {
            Ok(col)
        }
    }
}

/// Query executor
pub struct QueryExecutor;

impl QueryExecutor {
    /// Execute query on data
    pub fn execute(
        query: &Query,
        data: Vec<HashMap<String, Value>>,
    ) -> Vec<HashMap<String, Value>> {
        let mut results = Vec::new();
        let mut count = 0;

        for row in data {
            // Apply filters
            let mut passes = true;
            for filter in &query.filters {
                if let Some(col_val) = row.get(&filter.column) {
                    if !col_val.compare(&filter.value, &filter.operator) {
                        passes = false;
                        break;
                    }
                }
            }

            if !passes {
                continue;
            }

            // Apply column selection
            let mut result_row = HashMap::new();
            if query.select_cols.contains(&"*".to_string()) {
                result_row = row.clone();
            } else {
                for col in &query.select_cols {
                    if let Some(val) = row.get(col) {
                        result_row.insert(col.clone(), val.clone());
                    }
                }
            }

            results.push(result_row);
            count += 1;

            // Apply limit
            if let Some(l) = query.limit {
                if count >= l {
                    break;
                }
            }
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lexer_select() {
        let mut lexer = Lexer::new("SELECT col1 FROM table1");
        assert_eq!(lexer.next_token(), Token::Select);
        assert_eq!(
            lexer.next_token(),
            Token::Identifier("col1".to_string())
        );
        assert_eq!(lexer.next_token(), Token::From);
    }

    #[test]
    fn test_parser_simple_select() {
        let mut parser = Parser::new("SELECT col1 FROM table1");
        let query = parser.parse().unwrap();
        assert_eq!(query.select_cols, vec!["col1".to_string()]);
        assert_eq!(query.table, "table1");
    }

    #[test]
    fn test_parser_with_where() {
        let mut parser = Parser::new("SELECT col1 FROM table1 WHERE col2 = 100");
        let query = parser.parse().unwrap();
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.filters[0].column, "col2");
    }

    #[test]
    fn test_parser_with_limit() {
        let mut parser = Parser::new("SELECT * FROM table1 LIMIT 10");
        let query = parser.parse().unwrap();
        assert_eq!(query.limit, Some(10));
    }

    #[test]
    fn test_value_comparison() {
        let v1 = Value::Int(100);
        let v2 = Value::Int(50);
        assert!(v1.compare(&v2, &FilterOp::Greater));
        assert!(!v1.compare(&v2, &FilterOp::Less));
    }

    #[test]
    fn test_query_executor() {
        let mut data = Vec::new();
        let mut row1 = HashMap::new();
        row1.insert("id".to_string(), Value::Int(1));
        row1.insert("name".to_string(), Value::String("Alice".to_string()));
        data.push(row1);

        let mut row2 = HashMap::new();
        row2.insert("id".to_string(), Value::Int(2));
        row2.insert("name".to_string(), Value::String("Bob".to_string()));
        data.push(row2);

        let query = Query {
            select_cols: vec!["name".to_string()],
            table: "users".to_string(),
            joins: vec![],
            filters: vec![],
            limit: None,
        };

        let results = QueryExecutor::execute(&query, data);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_parser_with_join() {
        let mut parser = Parser::new("SELECT col1 FROM table1 INNER JOIN table2 ON table1.id = table2.id");
        let query = parser.parse().unwrap();
        assert_eq!(query.joins.len(), 1);
        assert_eq!(query.joins[0].table, "table2");
        assert_eq!(query.joins[0].join_type, JoinType::Inner);
    }

    #[test]
    fn test_parser_with_left_join() {
        let mut parser = Parser::new("SELECT * FROM users LEFT JOIN orders ON users.id = orders.user_id");
        let query = parser.parse().unwrap();
        assert_eq!(query.joins.len(), 1);
        assert_eq!(query.joins[0].join_type, JoinType::Left);
        assert_eq!(query.joins[0].left_col, "users.id");
        assert_eq!(query.joins[0].right_col, "orders.user_id");
    }

    #[test]
    fn test_lexer_join_tokens() {
        let mut lexer = Lexer::new("JOIN INNER LEFT RIGHT ON");
        assert_eq!(lexer.next_token(), Token::Join);
        assert_eq!(lexer.next_token(), Token::Inner);
        assert_eq!(lexer.next_token(), Token::Left);
        assert_eq!(lexer.next_token(), Token::Right);
        assert_eq!(lexer.next_token(), Token::On);
    }
}
