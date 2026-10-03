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
pub mod rewrite;
pub mod ast_walk;
pub mod general;
pub mod aggs;
pub mod desugar;
pub mod hashes;
pub mod arrays;
pub mod testing;
pub mod vecexpr;
pub mod window;
pub mod scalar;
pub mod datetime;
pub mod vec_path;

pub use ast::*;
pub use executor::{KqlContext, ExprVal, execute, execute_query};
pub use parser::{parse, parse_query};

use kore_core::KoreError;

/// One-shot: parse + execute SQL against a context.
pub fn query(sql: &str, ctx: &KqlContext) -> Result<kore_core::DataBlock, KoreError> {
    // Same entry point as every other caller, so WITH clauses, UNION/INTERSECT/EXCEPT and the
    // vectorized fast path are not silently skipped.
    ctx.query(sql)
}
