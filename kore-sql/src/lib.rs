//! KORE Layer 21 — KQL Query Language
//!
//! A SQL-like query language that compiles to KORE join/filter/sort ops.
//!
//! ```sql
//! SELECT a.id, b.name, a.score
//! FROM   orders  AS a
//! INNER JOIN customers AS b ON a.cust_id = b.id
//! WHERE  a.score > 80
//! ORDER  BY a.score DESC
//! LIMIT  100
//! ```

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod executor;

pub use ast::*;
pub use executor::{KqlContext, execute, execute_query};
pub use parser::{parse, parse_query};

use kore_core::KoreError;

/// One-shot: parse + execute SQL against a context.
pub fn query(sql: &str, ctx: &KqlContext) -> Result<kore_core::DataBlock, KoreError> {
    let stmt = parse(sql)?;
    executor::execute_select(&stmt, ctx)
}
