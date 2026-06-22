use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Select,
    From,
    Where,
    Join,
    On,
    Left,
    Right,
    Outer,
    As,
    Full,
    Cross,
    Using,
    Comma,
    Star,
    Ident(String),
    Number(String),
    Op(String),
    Eof,
}

pub struct Tokenizer {
    tokens: Vec<Token>,
    pos: usize,
}

impl Tokenizer {
    pub fn new(input: &str) -> Self {
        let mut tokens = Vec::new();
        let re = Regex::new(r"(?xi)
            (\s+)|
            (,) |
            (\*) |
            (\() |
            (\)) |
            (<=|>=|<>|!=|=|<|>) |
            \b(AND|OR)\b |
            \b(LEFT|RIGHT|OUTER|AS|FULL|CROSS)\b |
            \b(SELECT)\b |
            \b(FROM)\b |
            \b(WHERE)\b |
            \b(JOIN)\b |
            \b(ON)\b |
            \b(USING)\b |
            ([A-Za-z_][A-Za-z0-9_\.]*) |
            ([0-9]+(?:\.[0-9]+)?)
        ").unwrap();

        let mut idx = 0;
        while idx < input.len() {
            if let Some(m) = re.find(&input[idx..]) {
                if m.start() != 0 {
                    panic!("Unexpected token at: {}", &input[idx..]);
                }
                let s = m.as_str();
                let s_trim = s.trim();
                if s_trim.is_empty() {
                    // skip whitespace
                } else if s_trim.eq_ignore_ascii_case("SELECT") {
                    tokens.push(Token::Select)
                } else if s_trim.eq_ignore_ascii_case("FROM") {
                    tokens.push(Token::From)
                } else if s_trim.eq_ignore_ascii_case("LEFT") {
                    tokens.push(Token::Left)
                } else if s_trim.eq_ignore_ascii_case("RIGHT") {
                    tokens.push(Token::Right)
                } else if s_trim.eq_ignore_ascii_case("OUTER") {
                    tokens.push(Token::Outer)
                } else if s_trim.eq_ignore_ascii_case("AS") {
                    tokens.push(Token::As)
                } else if s_trim.eq_ignore_ascii_case("FULL") {
                    tokens.push(Token::Full)
                } else if s_trim.eq_ignore_ascii_case("CROSS") {
                    tokens.push(Token::Cross)
                } else if s_trim.eq_ignore_ascii_case("JOIN") {
                    tokens.push(Token::Join)
                } else if s_trim.eq_ignore_ascii_case("ON") {
                    tokens.push(Token::On)
                } else if s_trim.eq_ignore_ascii_case("USING") {
                    tokens.push(Token::Using)
                } else if s_trim.eq_ignore_ascii_case("WHERE") {
                    tokens.push(Token::Where)
                } else if s_trim.eq_ignore_ascii_case("AND") {
                    tokens.push(Token::Op("AND".to_string()))
                } else if s_trim.eq_ignore_ascii_case("OR") {
                    tokens.push(Token::Op("OR".to_string()))
                } else if s_trim == "," {
                    tokens.push(Token::Comma)
                } else if s_trim == "*" {
                    tokens.push(Token::Star)
                } else if s_trim == "(" {
                    tokens.push(Token::Op("(".to_string()))
                } else if s_trim == ")" {
                    tokens.push(Token::Op(")".to_string()))
                } else if ["<=","=>","!=","=","<",">"]
                    .iter()
                    .any(|&op| op == s_trim)
                {
                    tokens.push(Token::Op(s_trim.to_string()));
                } else if s_trim.chars().next().unwrap().is_ascii_digit() {
                    tokens.push(Token::Number(s_trim.to_string()))
                } else {
                    tokens.push(Token::Ident(s_trim.to_string()))
                }
                idx += m.end();
            } else {
                break;
            }
        }
        tokens.push(Token::Eof);
        Self { tokens, pos: 0 }
    }

    pub fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    pub fn next(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        t
    }
}
