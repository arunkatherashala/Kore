# Engine sync: master-kore-engine (236ae86c) into the Digital Life workspace

Branch `engine-sync`, started from local `dev` = 8589a219 (= pushed `main-Digital-Life-AI`).
Source of the new engine: `origin/master-kore-engine` = 236ae86c (read only; nothing was written there).
Common ancestor of the two lines: e89640d0 (2026-06-30). Since then master changed the engine crates on its own
(TPC-H / TPC-DS work, 211 commits) and dev changed a few of them for kore-self (270 commits, most outside the engine).

## 1. Inventory

### What kore-self uses from the engine side

| crate | used by kore-self for | API used |
|---|---|---|
| kore-sql | every SQL call: `kore_query.rs` (memory queries), `versioned/sql.rs` (versioned log, `AS OF` is parsed and resolved in kore-self, not in the engine), `being/world_t/tools.rs::guarded_sql` (World T and World M3 SQL plans, result digest = sha256 of `versioned::sql::block_strings`), `main.rs` | `KqlContext::new`, `register`, `register_mut`, `query`, `execute_dml`, `table_names` |
| kore-core | data blocks for all of the above | `DataBlock`, `Column`, `ColumnData` (`Int64`, `Float64`, `Bool`, `Str`, `StrDict`), `Value`, `KoreError::InvalidArgument` |
| kore-parquet | `main.rs` file import; World M3 source (`being/world_m3/source.rs`) | `ParquetReader::new(..).read()`; **dev-only** `meta` and `read_batches` (capped, batch-wise reader that keeps timestamps as Int64, added by M3 in a8774cdc) |
| kore-io | `main.rs` CSV import | `CsvReader::new` |
| kore-delta | dependency only (no direct call found) | - |
| kore-distributed | `main.rs` | `distributed_query`, `cluster_query` |
| kore-store | `.kore` files everywhere (memory, versioned log, World T/M3 files) | `KoreReader`, `KoreWriter`, `SharedDict`, `DictSet`, `CompactOptions`, `rowgroups` |

`kore-sql` itself depends on kore-core, kore-join, kore-window, kore-spill, kore-store, kore-delta, kore-parquet,
kore-io; master's kore-sql adds kore-catalog, kore-vectorized and the `regex` crate (1.13.1, in the local cargo cache).
Master's kore-sql no longer calls kore-window (its window code is `kore-sql/src/window.rs`); on dev only kore-sql and
kore-tpch use kore-window.

### Which engine crates differ (dev vs master, since the common ancestor e89640d0)

| crate | changed on master | changed on dev | what dev changed |
|---|---|---|---|
| kore-sql | 72 files, +32,263 / -1,393 | 9 files, +3,607 / -111 | Jul 2026 SQL features (most also on master in its own form), LIKE iterative matcher (802b446e), master NULL semantics port (abd8de1e), stable multi-key ORDER BY (a80d3e86) + DuckDB oracle (a064aa5e) |
| kore-core | 4 files, +561 / -30 | none | - |
| kore-window | 2 files, +350 / -16 | 1 file, +82 / -140 | NULL semantics, stable sorts (abd8de1e, a80d3e86) |
| kore-parquet | 2 files, +468 / -20 | 1 file, +60 | capped reader `meta` / `read_batches` (M3) |
| kore-join | 3 files, +642 / -43 | none | - |
| kore-spill | 2 files, +438 / -3 | none | - |
| kore-delta | 2 files, +1,494 / -272 | none | - |
| kore-io | 2 files, +4 / -4 | none | - |
| kore-catalog | 2 files, +638 / -1 | none | - |
| kore-vectorized | 2 files, +977 / -3 | none | - (dev's copy depends on kore-sql; master's does not, master's kore-sql depends on it) |
| kore-store | 4 files, +303 / -66 | 44 files, +6,461 / -104 | the KORE file format work (v3/v4, codecs, dictionaries, row groups, compaction) |
| kore-distributed | 4 files, +621 / -64 | 2 files, +257 / -62 | TCP cluster via kore-coord / kore-worker |

Crates on master but not on dev (not engine, not used by kore-self, not brought): kore-automl, kore-autonomy,
kore-benchmark, kore-causality, kore-continual-learning, kore-demo, kore-deploy, kore-explainability, kore-finetune,
kore-flight, kore-gnn, kore-graph, kore-hdfs, kore-knowledge-graph, kore-llm, kore-logic, kore-meta-learning, kore-ml,
kore-mlops, kore-multimodal, kore-neurosymbolic, kore-privacy, kore-pyo3, kore-qa, kore-quantum, kore-rag, kore-rl,
kore-sdk, kore-server, kore-streaming, kore-submit, kore-timeseries, kore-vectors.
Crates on dev but not in master's workspace: kore-self (it lives on the Digital Life line by design).

### Public API differences that matter to kore-self

* `kore_core::Value` gains `Array(Vec<Value>)` and `Map(Vec<(Value, Value)>)`; `KoreError` gains 11 variants
  (ParseError, NetworkError, Timeout, ...). Exhaustive matches on these must take the new variants.
* `ColumnData::push` on a `StrDict` with 255 values now turns the column into plain text instead of wrapping the u8 code.
* `KqlContext`: everything kore-self calls exists on master with the same signature; master adds `set_memory_limit`,
  `create_view`, `spill_stats` and the `testing` module (switches for optimised paths).
* kore-parquet: master has no `meta` / `read_batches`; they must be kept (they merge cleanly into master's file).
