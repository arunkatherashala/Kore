// Canonical crate root: expose distinct modules for KORE v1/v2/query/txn.
// Keep the legacy lightweight shim behind the feature flag `kore_lite_compat`.

#![allow(
    clippy::empty_line_after_doc_comments,
    clippy::new_without_default,
    clippy::unnecessary_cast,
    clippy::manual_div_ceil,
    clippy::manual_is_multiple_of,
    clippy::needless_range_loop,
    clippy::for_kv_map,
    clippy::needless_return,
    clippy::collapsible_if,
    clippy::len_zero,
    clippy::iter_nth_zero,
    clippy::manual_range_contains,
    clippy::manual_checked_ops,
    clippy::maybe_infinite_iter,
    clippy::redundant_closure,
    clippy::useless_vec,
    clippy::identity_op,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]

pub mod kore;
pub mod kore_v2;
pub mod kore_query;
pub mod kore_txn;
pub mod gorilla;
pub mod benchmarks;
pub mod binary_format;
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
pub mod decompression;
pub mod kore_reader;
pub mod codec_selector;
pub mod compression_validator;
pub mod roundtrip_validator;
pub mod compression;
pub mod roundtrip_integration;
pub mod fileio_validator;
pub mod integration_tests;
pub mod parametric_tests;
pub mod production_validator;
pub mod kore_writer;

// Cloud storage connectors (API stubs enabled for v1.0, full implementation in v1.1)
#[cfg(feature = "s3")]
pub mod s3_reader;

#[cfg(feature = "azure")]
pub mod azure_reader;

#[cfg(feature = "gcs")]
pub mod gcs_reader;

// Python bindings for Kore core functionality
#[cfg(feature = "pyo3")]
pub mod python_bindings;

// Java/JNI bindings (function implementations conditioned by cloud features)
#[cfg(feature = "java")]
pub mod java_bindings;

// JavaScript/NAPI bindings (function implementations conditioned by cloud features)
#[cfg(feature = "napi")]
pub mod napi_bindings;

#[cfg(feature = "kore_lite_compat")]
pub mod kore_lite;

// Canonical top-level API points to v2 to avoid ambiguous re-exports.
pub use kore_query::{AggFunc, FilterOp, KoreQuery, QueryResult, SelectCol, WhereClause};
pub use kore_v2::{KColumn, KType, KVal, KoreReader, KoreWriter, KORE_MAGIC, KORE_V2};
