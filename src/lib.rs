// Canonical crate root: expose distinct modules for KORE v1/v2/query/txn.
// Keep the legacy lightweight shim behind the feature flag `kore_lite_compat`.

pub mod kore;
pub mod kore_v2;
pub mod kore_query;
pub mod kore_txn;

#[cfg(feature = "kore_lite_compat")]
pub mod kore_lite;

// Canonical top-level API points to v2 to avoid ambiguous re-exports.
pub use kore_query::{AggFunc, FilterOp, KoreQuery, QueryResult, SelectCol, WhereClause};
pub use kore_v2::{KColumn, KType, KVal, KoreReader, KoreWriter, KORE_MAGIC, KORE_V2};
