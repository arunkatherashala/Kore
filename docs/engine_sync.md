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

## 3. Sync: what was brought, and every deviation from master

Commits on `engine-sync` (not merged into `dev`): cf0795d5 (mechanical copy), 1ee55dea (kore-self match arms),
6ff789f9 (dev's ORDER BY / NULL tests on the new engine), facf3c4b (one ORDER BY fix), plus the docs commits.

Identical to master 236ae86c: kore-core, kore-sql (every file of src, tests, examples) except `src/executor.rs`
(deviation 1), kore-join, kore-spill, kore-delta, kore-io, kore-catalog, kore-vectorized, kore-parquet/Cargo.toml,
`benchmarks/tpcds_honest/queries.json` (fixture read by `kore-sql/tests/p4_bind.rs`), and six crates that dev never
changed and that no longer compiled against the new engine: kore-optimize, kore-ffi, kore-python, kore-kafka,
kore-dml, kore-node (`cargo check --workspace` is clean afterwards).

Deviations from master (every file where the branch differs from 236ae86c inside the synced crates, and the crates
deliberately kept from dev):

| # | file / crate | what differs | why |
|---|---|---|---|
| 1 | `kore-sql/src/executor.rs` | `sql_cmp_f64`: the NULL-aware ORDER BY sort and its top-N path compare doubles with -0.0 = 0.0 (master: `f64::total_cmp`, -0.0 < 0.0); 13 lines | dev's DuckDB ORDER BY oracle on the synced engine: 12 of 2,366 cases disagreed (group `floats`, keys with a NULLS clause), 0 after |
| 2 | `kore-parquet/src/lib.rs` | master's file plus dev's `meta` and `read_batches` (capped, timestamp-keeping reader of World M3) | kore-self's World M3 source needs them; clean 3-way merge (ancestor e89640d0), nothing of master's file changed |
| 3 | `kore-window` (whole crate) | dev's copy kept (stable sorts, NULL semantics); master's adds `apply_window_framed` / `WindowFrame` and still sorts with `par_sort_unstable_by` | master's kore-sql no longer calls kore-window (its windows are `kore-sql/src/window.rs`); only kore-tpch uses it; no synced crate calls the framed API |
| 4 | `kore-store` (whole crate) | dev's copy kept (the KORE file format work: v3/v4, codecs, dictionaries, row groups); master's 4-file change (mmap reader, readable writer) not brought | kore-self's `.kore` files depend on dev's format; master's kore-sql / kore-spill / kore-io compile and pass against it (they use `KoreReader::read_file`, `KoreWriter::write_file`) |
| 5 | `kore-distributed` (whole crate) | dev's copy kept (TCP cluster via kore-coord / kore-worker) | used by kore-self `main.rs`; compiles against the new engine |
| 6 | `kore-sql/tests/null_semantics.rs` | master's file; dev's file of the same name renamed to `null_semantics_dev.rs` | name clash; both kept |
| 7 | `kore-sql/tests/null_semantics_dev.rs` (dev-only) | two expectations changed: REGEXP_LIKE removed from the unknown-function list; `LAST_VALUE(x) OVER (PARTITION BY g ORDER BY d)` expects `[1, NULL, 3, 4]` | master implements REGEXP_LIKE; the old copy returned the partition's last value, the SQL default frame ends at the current row (DuckDB gives `[1, NULL, 3, 4]`) |
| 8 | `kore-sql/tests/order_by_multikey.rs`, `kore-sql/examples/order_by_oracle_gen.rs` (dev-only) | without a NULLS clause the reference / the DuckDB SQL places NULL as the smallest value (ASC NULLS FIRST, DESC NULLS LAST); oracle group `rand_orderby_legacy` renamed `rand_orderby_default` | the old copy's per-type legacy NULL placement (Int64 i64::MIN, Float64 f64::MAX, text "") does not exist in master's engine, which follows the Spark default |
| 9 | `kore-sql/tests/oracle_duckdb/compare_order_by.py` (dev-only) | unchanged | - |
| 10 | kore-self `kore_query.rs` (3 matches), `versioned/sql.rs` (1 match) | arms for `Value::Array` / `Value::Map` | new enum variants; they never come out of a DataBlock column, so printed values and the World T result digest are unchanged for every column type |
| 11 | workspace `Cargo.toml` | dev's (master's 33 extra members not added) | not engine, not used by kore-self |
| 12 | every other crate | dev's copy | compiles against the new engine unchanged |

## 4. Tests on the synced engine

Debug builds, `--offline`, `--no-fail-fast`, `CARGO_TARGET_DIR=C:/Users/skathera/kw/target-dev`, sums of the
`test result` lines (ignored tests not counted):

| crate | passed | failed |
|---|---|---|
| kore-sql (master's 28 test files incl. spill.rs, plus order_by_multikey, null_semantics_dev, unit tests) | 351 | 0 |
| kore-core / kore-join / kore-spill / kore-delta / kore-io | 50 / 13 / 17 / 20 / 3 | 0 |
| kore-catalog / kore-vectorized / kore-parquet / kore-window | 28 / 28 / 15 / 4 | 0 |
| **engine crates, total** | **529** | **0** |
| kore-store | 63 | 0 |
| kore-self | 700 | **2** (World T golden, World T engine-behaviour pins; below) |
| **all** | **1,292** | **2** |

kore-tpch: builds, has no tests (0 / 0). `cargo check --workspace --offline`: clean.

DuckDB ORDER BY oracle (DuckDB 1.5.6 from the existing `engines` env, `python -I`, scratch data in
`C:/Users/skathera/kw/engine-sync-oracle`, deleted afterwards): 2,366 queries, 0 disagreements (12 before
deviation 1, all in group `floats`, -0.0 vs 0.0).

### Golden and World T / World M3 / being outcome tests

* `tests_b13_golden.rs` (via `tests_b13.rs`, `with_the_b13_switch_off_every_chooser_is_byte_identical_to_e9ed7d1d`):
  **passes**, unchanged.
* `world_t/tests_golden.rs::existing_worlds_are_byte_identical_with_world_t_off`: **passes**.
* `world_t/tests_golden.rs::world_t_default_being_is_byte_identical`: **FAILS** (file not edited). Got
  `["a0170c83ccaade2f", "44d774c20f505838:23548:6000"]`, pinned `["091ce2aa622226d4", "beb34713af0726b8:22847:6000"]`:
  the logged life (seed 11) and the measurement life (seed 101, budget 6000; reward 22,847 -> 23,548) both changed.
* `world_t/tests_engine.rs::engine_behaviour_the_world_t_tools_work_around`: **FAILS** (file not edited). It pins the
  old engine's behaviour on purpose ("if one of these starts failing, the engine changed: revisit `tools.rs`"). On
  master's engine: `REGEXP_LIKE(g, 'a')` is a function (true, false, true, NULL) instead of an error;
  `SELECT "d" FROM t` returns the text `'d'` on every row (a double-quoted string literal, Spark style) instead of an
  error; `PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY x)` parses (3.0); `ORDER BY s DESC` puts the NULL sum last
  (Spark) instead of first (PostgreSQL), so both `ORDER BY s DESC` and `.. NULLS LAST` start with `a`; the self CROSS
  JOIN `SELECT a.x, b.x FROM t a CROSS JOIN t b` works (16 rows) instead of an error. Items 1 (COUNT), 2 (unknown
  functions other than REGEXP_LIKE) and 5 (LAG NULLs) are unchanged.
* World M3 tests: 14 of 14 pass. Every other kore-self test passes.

Why the World T golden changed (exactly which SQL answers, and which engine is wrong): every query the World T being
ran was traced (input table digest, SQL, outcome, result) in both builds for the two lives of the golden test (old
copy 8589a219: 1,582 queries; synced: 1,608). The trace was a temporary, uncommitted wrapper around
`world_t::tools::guarded_sql`, removed afterwards; with it the old build still passed its golden. The first query of
the logged life already differs. Over the (input, SQL) pairs present in both traces, 296 differ, all on tables the
**naive loader** read wrongly, so that numbers are text or the whole header is one tab-joined column name:

| class | pairs | example | old copy | synced engine | DuckDB 1.5.6 |
|---|---|---|---|---|---|
| a column that does not exist (naive load: the header became one column `\ufeffsku\twarehouse\t...`) | 210 | `SELECT COUNT(*) AS v FROM t WHERE stock > 18` | `0` (an unknown column reads as NULL: probe bug 20) | error `column not found: stock` | error `Referenced column "stock" not found` |
| text column compared with a number, SUM over a text column | 81 | `SELECT COUNT(*) AS v FROM t WHERE tries > 14` (`tries` is text); `SELECT SUM(amount) AS v FROM t WHERE method = 'card'` | `0`; `NULL` | `25`; `6023.24` (text read as a number, Spark style) | error without a cast; `25`; `6023.24` with `TRY_CAST(.. AS DOUBLE)` |
| MAX over a text column | 5 | `SELECT MAX(amount) AS v FROM t` | `NULL` | `'98.27'` (the text maximum) | `'98.27'` |

In all 296 pairs the synced engine agrees with DuckDB, as written or, in the 81, with the text column read as a
number where DuckDB demands an explicit cast (in 3 of the 81 only the SUM was checked: my TRY_CAST rewrite changed
the COUNT(amount) next to it, which is the same in both KORE builds). In none of the 296 does the old copy agree with
DuckDB either way. So the golden changed because **the old engine was wrong**: it answered queries on a column that
does not exist (0 / NULL instead of an error) and silently returned 0 / NULL for comparisons and sums over text.
With the synced engine the measurement life of the golden earns more (23,548 vs 22,847 over 6,000 steps). The golden
files were not edited; whether to re-pin them (and to update `tests_engine.rs` and the "engine needs" list in
docs/world_t.md) is the owner's decision.

## 5. What this means for earlier results

Results measured before this sync (World T, W2, W2b, S1, M3) used the old engine copy; runs after the sync are
compared only on fresh seeds. No measurement run and no report seed was run for this sync.

Limits: the probe ran master's tests on dev's copy with stand-ins for master's test switches (a no-op memory limit:
spill paths not exercised on dev); case-list tests stop at the first failing check per test, so the 787 listed cases
are a lower bound; p3_gap and semi_join_perf were killed at 900 s on dev (debug), not completed; the World T analysis
covers the two lives of the golden test (seeds 11 and 101), not the report seeds; DuckDB was the referee with one
stated rewrite (TRY_CAST) where it refuses implicit casts; master's kore-store change was not evaluated; kore-window
and kore-store are dev's copies, so the branch is not a byte copy of master's engine; whether master's single-key
external sort above 256 MB keeps ties stable was not checked.
