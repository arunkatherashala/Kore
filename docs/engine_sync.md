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

## 2. Bug probe: master's engine tests against dev's old engine copy (before the sync)

How: master's 28 kore-sql integration test files (236ae86c) were copied into an uncommitted scratch state of this
branch at 8589a219 and run against dev's kore-sql (debug, `--test-threads=1`, 900 s per file). Master's
`kore_sql::testing` module does not exist on dev: a stand-in made its switches no-ops (the optimised paths they turn
off do not exist on dev), and `set_memory_limit` a no-op (dev cannot spill; the query runs in memory and its answer is
still checked). Path counters ("the X path ran") were then made to tick, and the tests that had failed only on such an
assert were run again, so that their answer checks behind the assert ran too. `spill.rs` does not compile on dev
(no memory limit, no spill statistics: feature absent) and is not counted.

Result per file (first run; "path only" = the failures were only path asserts, every answer check passed in the rerun):

| test file | passed / failed on dev | what failed |
|---|---|---|
| ds_regress | 0 / 7 | **all 7 TPC-DS bugs of master's README are present** (see below) |
| sql_correctness | 6 / 20 | wrong answers (joins, subqueries, windows, NULL logic) and errors |
| sql_coverage | 1 / 28 | 252 wrong answers, 318 errors, 13 accepted-invalid in the first failing check of each test |
| sql_coverage2 | 0 / 6 | 8 wrong, 52 errors (generators, maps, higher-order functions, recursive CTE) |
| sql_gaps3 | 0 / 11 | 22 wrong (dates/timestamps, decimals), 96 errors |
| fuzz_regress | 1 / 15 | double / bigint precision, NULL logic of NOT IN / ALL, comma-join columns, GROUPING() |
| m_null_semantics (master's `null_semantics.rs`) | 1 / 5 | COUNT window, HAVING COUNT(col), LAG with default, unknown function in ORDER BY |
| differential | 0 / 1 | 38 of 1,600 random queries: dictionary-encoded text differs from plain text |
| deep_input | 2 / 2 | process abort (stack overflow) at nesting 5,000 for 12 of 14 shapes; `NOT NOT a = 1` wrong |
| like_robust | 1 / 1 | `ILIKE` finds nothing |
| cliff_join | 2 / 2 | LEFT JOIN with residual ON condition; theta join refused |
| cliff_window | 4 / 1 | 4,212 of 6,335 random window queries refused (errors; the answered ones agree) |
| p1_join | 3 / 1 | composite key Float64 = Int64 finds no rows |
| p1_plan | 0 / 8 | GROUP BY over comma joins: "column not found"; a star next to other items refused |
| p4_bind | 0 / 7 | no binder: unknown columns are NULL instead of an error; UPDATE of an unknown column accepted; no CREATE VIEW |
| p5_exprjoin | 3 / 3 (2 after rerun) | offset self join returns 0 rows; COUNT(*) typed Float |
| p5_rollup | 3 / 1 | `GROUPING()` unknown |
| p2_ovh | 1 / 1 | `CREATE TABLE t (..)` without AS refused |
| p1_agg, p2_str, p2_subq | 6/2, 2/3, 0/3 | path only (all answers right in the rerun) |
| p5_cte, date_accel | 7/1, 0/1 | path only / stand-in artefact (a "switch disables the path" check) |
| fused_agg, fuzz_oracle | 1/0, 1/0 (2 ignored) | pass |
| p3_gap, semi_join_perf | killed at 900 s | nested IN subquery / EXISTS too slow on dev (debug build) |

Totals of the first run: 45 passed, 130 failed (p3_gap, semi_join_perf killed; spill.rs not compiled).
Every failed case of the four case-list files (787 cases) is listed in `docs/engine_sync_probe_cases.md`.

### Master fixes the old copy lacked (silent wrong answers first)

| # | bug on dev's old copy (silent unless marked) | minimal repro | dev | master | test |
|---|---|---|---|---|---|
| 1 | two aggregates of the same function in one SELECT collapse to the first | `select sum(v*2), sum(v*3) from t`; `select count(1), count(null) from t` | 340,340; 6,6 | 340,510; 6,0 | sql_coverage |
| 2 | aggregates inside expressions | `select max(v) - min(v) from t`; `select -sum(v) from t` | NULL per row | 50; -170 | sql_coverage |
| 3 | `COUNT(col) OVER ()` counts rows | 5 rows, 1 non-NULL b: `COUNT(b) OVER ()` | 5 | 1 | m_null_semantics |
| 4 | window aggregate with ORDER BY is the whole-partition total, not a running frame | `sum(v) over (partition by g order by id)` | 30,30,30,.. | 10,30,30,.. | sql_correctness, sql_coverage |
| 5 | HAVING on an aggregate together with ORDER BY drops every group | `.. group by g having count(x) >= 1 order by g nulls first`; TPC-DS Q6 shape | no rows | the groups | m_null_semantics, ds_regress |
| 6 | `SELECT expr, *` drops the expression | `select id*10 as x, * from t` | x missing | x present | ds_regress |
| 7 | constant / computed select item next to GROUP BY becomes TEXT rounded to 4 digits | `select 0 as g, k, sum(v) .. group by k`; `select id % 2 as par .. group by par` | `'0'`, `'0.0000'`, `'1.5002'` | 0, 0, 1.50015 | ds_regress |
| 8 | GROUP BY / aggregates over dictionary-encoded text differ from plain text | random queries | 38 of 1,600 differ | 0 | differential |
| 9 | double equality and grouping are approximate | `x = 0.3` with 0.1+0.2 present; distinct of 1e-12 / 2e-12 | matches both; merged | exact | fuzz_regress |
| 10 | BIGINT above 2^53 merged in semi joins and scalar subqueries | 9007199254740993 vs ..992 | merged | exact | fuzz_regress |
| 11 | `NOT NOT a = 1` | nested derived tables | 3 rows | 1 row | deep_input |
| 12 | LEFT JOIN whose ON has an extra non-equality condition drops unmatched left rows | `ta left join tb on a.k = b.k and a.v < b.v` | 2 rows | 3 rows | cliff_join |
| 13 | composite join key Float64 = Int64 finds nothing; offset self join finds nothing | `a.k2 = b.k2` (1.0 vs 1); `v1.rn = l.rn + 1` | 0 rows | the matches | p1_join, p5_exprjoin |
| 14 | cross join / comma join without equality not a cross product | `select count(*) from t a cross join t b` | 6 | 36 | sql_coverage, sql_correctness |
| 15 | correlated scalar subqueries return NULL / nothing | `(select count(*) from u where u.id = t.id)` | NULL | counts | sql_coverage, sql_correctness |
| 16 | EXISTS / NOT EXISTS with an outer filter; `IN (subquery)` as a value | `id in (select id from u)` in the select list | all false | true/false | sql_correctness, sql_coverage |
| 17 | INTERSECT keeps duplicates; INTERSECT ALL wrong; NTILE wrong | `select g from t intersect select g from t` | duplicates | distinct | sql_coverage |
| 18 | NOT IN / ALL with a NULL in the set; NOT IN over an empty set for a NULL key | fuzz shapes | wrong or refused | SQL three-valued logic | fuzz_regress |
| 19 | ILIKE matches nothing | `s ILIKE '%NEEDLE%'` | 0 rows | 1 row | like_robust |
| 20 | trailing tokens silently dropped; unknown column reads as NULL | `select c_id from cust garbage junk`; `select nosuch + 1 from t` | answered | error | sql_correctness, p4_bind |
| 21 | DESC sort puts NULL first (dev's legacy NULL placement; master: NULLs last on DESC, first on ASC) | `select f from t order by f desc` | NULL first | NULL last | sql_coverage |
| 22 | casts and dates | `cast(123.456 as decimal(10,2))`; `to_date('2024-02-30')`; `cast(2.0 as string)` | 123.456; 2024-02-30; 2 | 123.46; NULL; 2.0 | sql_coverage |
| 23 | (crash) deep nesting aborts the process (stack overflow) | 5,000 nested parentheses / NOT / CASE / subqueries; 100,000-term AND chain | abort | clean error | deep_input |
| 24 | (error) comma join after an explicit join; EXISTS over a multi-table subquery; expression join keys; theta joins; GROUP BY over comma joins | TPC-DS Q35/Q40/Q49/Q80/Q93 shapes | parse / column errors | answered | ds_regress, p1_plan, cliff_join |
| 25 | (error) missing features | LAG/LEAD with a default, `GROUPING()`, `>= ALL (..)`, `PERCENTILE_CONT .. WITHIN GROUP`, `REGEXP_LIKE`, CREATE VIEW, `CREATE TABLE t (..)`, arrays, maps, structs, generators, higher-order functions, spill under a memory limit | error | answered | several |
| 26 | (error) CONCAT of dictionary and plain text above 255 distinct values | | TypeMismatch | answered | fuzz_regress |

Which of these the World T / World M3 SQL plans can reach: their query shapes (single table `t`, COUNT / SUM / AVG /
MIN / MAX / MEDIAN, `COUNT(DISTINCT ..)`, GROUP BY one column with ORDER BY .. LIMIT, `LAG(x) OVER (ORDER BY d)`,
`ORDER BY .. LIMIT 1 OFFSET k`, a refused CROSS JOIN, unknown functions) were run on dev's old copy on a 7-row table
with NULLs; all answered as expected except PERCENTILE_CONT .. WITHIN GROUP, REGEXP_LIKE and the self CROSS JOIN
(errors). Bug 1 showed with same-function aggregates over expressions or literals (`sum(v*2), sum(v*3)`,
`count(1), count(null)`); the World T shape `COUNT(x), COUNT(*), AVG(x)` came out right (5, 7, 1.05). Whether any being outcome changes is settled by the
golden tests in step 4, not by this list.
