#[derive(Debug, PartialEq, Eq)]
pub enum StmtKind {
    Select,
}

#[derive(Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub select: Option<SelectStmt>,
}

#[derive(Debug)]
pub struct SelectStmt {
    pub columns: Vec<String>,
    pub from: String,
    pub joins: Vec<Join>,
    pub where_clause: Option<Expr>,
}

#[derive(Debug)]
pub struct Join {
    pub kind: JoinKind,
    pub table: String,
    pub alias: Option<String>,
    pub on: Expr,
}

#[derive(Debug)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Outer,
    Full,
    Cross,
}

#[derive(Debug)]
pub enum Expr {
    Binary { left: String, op: String, right: String },
    Logical { left: Box<Expr>, op: String, right: Box<Expr> },
    Paren(Box<Expr>),
}
