pub mod tokenizer;
pub mod ast;
pub mod parser;

pub use parser::SQLParser;
pub use ast::*;

#[cfg(test)]
mod tests {
    use crate::parser::SQLParser;
    use crate::StmtKind;

    #[test]
    fn simple_select() {
        let sql = "SELECT id, name FROM users WHERE id = 123";
        let mut p = SQLParser::new(sql);
        let ast = p.parse().expect("parse");
        assert_eq!(ast.kind, StmtKind::Select);
    }
}
