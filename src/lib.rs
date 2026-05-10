// Canonical crate root: expose distinct modules for KORE v1/v2/query/txn.
// Keep the legacy lightweight shim behind the feature flag `kore_lite_compat`.

pub mod kore;
pub mod kore_v2;
pub mod kore_query;
pub mod kore_txn;
pub mod gorilla;
pub mod benchmarks;
pub mod query_engine;
pub mod query_cache;
pub mod index_manager;
pub mod distributed_engine;
pub mod query_parallelization;
pub mod memory_pooling;
pub mod join_optimization;
pub mod baseline_benchmarking;
pub mod query_optimization_engine;
pub mod realworld_benchmarking;
pub mod deployment;
pub mod comprehensive_testing;
pub mod performance_profiling;
pub mod advanced_features;
pub mod documentation;

#[cfg(feature = "kore_lite_compat")]
pub mod kore_lite;

// Canonical top-level API points to v2 to avoid ambiguous re-exports.
pub use kore_query::{AggFunc, FilterOp, KoreQuery, QueryResult, SelectCol, WhereClause};
pub use kore_v2::{KColumn, KType, KVal, KoreReader, KoreWriter, KORE_MAGIC, KORE_V2};
