# KORE SQL engine: status and honest assessment

**Date:** 2026-10-04
**Branch / commit:** `master-kore-engine` @ `cfbb7575`
**Scope:** the `kore-sql` engine (parser, executor, joins, spill-to-disk) as measured on one machine.

## Verdict in one paragraph

KORE's SQL engine is real: a Rust parser and executor whose answers were checked against live Apache Spark on
all 22 TPC-H-shaped queries, with 0 mismatches. On one machine, with data that fits in memory (SF1, about 6M
lineitem rows), it was faster than Spark local mode on every query. It is a strong prototype. It is not
production-ready, it does not replace Spark for large data, clusters or fault tolerance, and **DuckDB is still
faster than KORE**: in the final same-day comparison (below) DuckDB was about 1.8x faster than KORE in geometric
mean, while KORE was faster than DataFusion and Polars overall and about 13x faster than Spark local mode.

## What was wrong before and was corrected

| Earlier claim | Reality |
|---|---|
| "339x faster than Spark" (README, release notes, `kore-tpch`) | The Spark numbers were constants typed into the code, never measured, and the queries were simplified. Banners were added to the affected documents. Do not quote those figures. |
| 22 queries supported | At the start of this work 2 of 22 queries returned correct results. Roughly 15 silent wrong-answer bugs were found and fixed (parser dropping tokens, nested aggregates, NULL handling in joins and `GROUP BY`, correlated subqueries, unstable sort, integer typing, `UPDATE` that changed nothing, and others). |

## How the measurement works

- Both engines read identical data and run identical SQL text (`benchmarks/tpch_honest`).
- Spark 4.2.0 in local mode on all cores, tables cached in memory before timing. KORE has tables loaded in memory before timing.
- Each query runs once as a warm-up, then the minimum of the timed runs is reported.
- KORE's results are compared with Spark's by column name (floats to 1e-6 relative). A difference is reported as `MISMATCH`.
- Machine: 8 cores, 32 GB RAM, Windows. Timings vary by about +/-20% between runs, and more when other
  programs are using the machine.

## Results at SF1 (final run on this commit, milliseconds)

All 22 results agree with Spark (22 ok, 0 mismatch, 0 error, 0 timeout).

| Query | Spark | KORE | KORE / Spark | | Query | Spark | KORE | KORE / Spark |
|---|---|---|---|---|---|---|---|---|
| Q1 | 1088 | 497 | 0.46 | | Q12 | 1548 | 205 | 0.13 |
| Q2 | 1251 | 47 | 0.04 | | Q13 | 2111 | 868 | 0.41 |
| Q3 | 1503 | 413 | 0.27 | | Q14 | 605 | 87 | 0.14 |
| Q4 | 1429 | 163 | 0.11 | | Q15 | 1159 | 84 | 0.07 |
| Q5 | 1846 | 282 | 0.15 | | Q16 | 1928 | 88 | 0.05 |
| Q6 | 410 | 124 | 0.30 | | Q17 | 1216 | 152 | 0.13 |
| Q7 | 1791 | 586 | 0.33 | | Q18 | 3116 | 611 | 0.20 |
| Q8 | 1567 | 295 | 0.19 | | Q19 | 608 | 110 | 0.18 |
| Q9 | 2765 | 667 | 0.24 | | Q20 | 1372 | 555 | 0.40 |
| Q10 | 1755 | 247 | 0.14 | | Q21 | 4156 | 354 | 0.09 |
| Q11 | 1026 | 63 | 0.06 | | Q22 | 1216 | 212 | 0.17 |

Notes on the table:
- This was a single full run on a machine that was still busy at times. Re-running a few queries alone gave
  Q1 217-236 ms, Q6 68-94 ms and Q9 561-616 ms, so some entries above are inflated by load.
- Spark local mode has fixed JVM and planning overhead that is large for queries that take about a second. The
  ratios say little about 100 GB+ data.
- Spark times are from a cached live Spark run on this machine, not from published benchmarks.

## Comparison with other single-node engines (added 2026-10-04, after network access to conda-forge)

Same machine, same Parquet data, same SQL text, answers compared with Spark. Times in ms; best of 3 after a warm-up.

| Engine | Answers agreeing with Spark | Geometric mean over the 18 queries all engines answered correctly |
|---|---|---|
| DuckDB 1.5.6 | 22 of 22 | 60 ms |
| **KORE** | 22 of 22 | 106 ms |
| Polars 1.44.2 | 19 of 22 (SQL layer rejects Q2, Q13, Q17) | 164 ms |
| DataFusion 54.0.0 | 21 of 22 (Q15 returns no rows: float-equality artifact of the query) | 166 ms |
| Spark 4.2.0 local | 22 of 22 (reference) | 1425 ms |

DuckDB was fastest on 16 of 22 queries, Polars on 5, KORE on 1 (Q1: 74 ms vs DuckDB 113 ms). Per-query table:
`benchmarks/tpch_honest/README.md`. KORE's geometric mean improved from 238 ms to 106 ms during the day, after the aggregation,
join and subquery work below (fused filter+aggregate, late-materialised joins, sideways information passing, per-key
correlated aggregates). Its weakest queries relative to DuckDB are Q13 (850 vs 87 ms), Q17 (82 vs 24), Q10 (309 vs 76) and
Q3/Q4/Q12. Caveats: one machine, SF 1, in-memory, TPC-H-shaped data, about +/-20% noise. Engines were re-run back to back on a
quiet machine (Spark's times are from its earlier cached run on the same machine).

## What was added in this round

- Join planning: filters derived from `OR` branches, unique-key (lookup) tables joined first.
- Execution speed: parallel flat hash join, parallel group-by, semi-join rewrite for `EXISTS`, faster `LIKE`,
  shared `IN` sets, top-N for `ORDER BY ... LIMIT`, hash-based `DISTINCT` (exact: hash-equal rows are confirmed
  cell by cell).
- Correctness and coverage: window frames with correct NULLs, `ROLLUP`/`CUBE`/`GROUPING SETS`, MAP values, lambdas
  and higher-order functions, `WITH RECURSIVE`, working `INSERT`/`UPDATE`/`DELETE`/`CREATE`/`DROP`, many string,
  date and JSON functions. See `SQL_SUPPORT.md` for the full matrix.
- Memory: peak commit at SF1 down 22-30% per query (for example Q9 4.44 GB to 3.32 GB). The table data itself is
  about 2.4-2.5 GB at SF1.
- Spill to disk: `KqlContext::set_memory_limit` makes `ORDER BY`, `GROUP BY` and hash joins partition to temporary
  files when their working state exceeds the limit. Default is unlimited, so normal speed is unaffected.

## Test evidence

- 223 tests pass across `kore-sql`, `kore-join` and `kore-ffi`, including about 960 hand-derived regression
  cases, a differential test across execution paths, and a SQLite oracle over 60k generated queries.
- A mutation fuzzer ran about 1.6M mutants with no panics (reported by the SQL-coverage work, not re-run here).
- Not run: the whole workspace, because a Python-binding crate does not build against Python 3.14 here.

## Known limits (do not claim otherwise)

| Area | Limit |
|---|---|
| Scale | Everything is in memory as row vectors of `Option<T>`. Spilling bounds operator working state only, not input tables or results. A skewed key can still exceed the budget. |
| SF3 (18M lineitem rows) | Earlier run needed 8-10 GB per query process and one run failed with an out-of-memory error while other programs were running. Not re-run on the final commit. |
| Strings | `Option<String>` columns that cannot be dictionary-encoded (dictionary holds at most 254 values) dominate memory and are slow to sort and group. |
| Other engines | DuckDB is about 1.8x faster than KORE (geometric mean at SF1) and won 16 of 22 queries. KORE is faster than DataFusion and Polars overall. |
| Distributed | No cluster execution, no fault tolerance, no connectors (S3, Hive, JDBC, Kafka), no streaming or ML. |
| SQL | No STRUCT, TABLESAMPLE or native DECIMAL (DECIMAL is a double). A DATE compared with a TIMESTAMP is a string comparison. `EXPLAIN`/`DESCRIBE` output differs from Spark. |
| Data | TPC-H-shaped data from `gen_data.py`, not the official dbgen. This is not an official TPC-H result. |
| Output order | Inner joins can return rows in a different order, so unordered `LIMIT` queries may return a different set of tied rows. Spark behaves the same way. |
| Unverified | `format_number(0.5, 0)` now rounds half-even, taken from Spark's documentation, not from a Spark run. |

## Maturity assessment

| Question | Answer |
|---|---|
| Is it a real SQL engine? | Yes. |
| Are the results correct? | On the 22 TPC-H-shaped queries, yes, checked against live Spark. In general, no guarantee: bugs are still being found at a rate of several per round. |
| Faster than Spark on one machine, in-memory? | Yes at SF1, by 2x to 25x in these runs. |
| Faster than DuckDB? | Not overall: DuckDB is about 1.8x faster in geometric mean and won 16 of 22 queries. KORE was faster on Q1 only. |
| Replacement for Spark? | No. |
| Production ready? | No. |
| Best description | Strong research-grade prototype. |

## What would be needed next, in order

1. Close the remaining gap to DuckDB (about 1.8x): Q13 (left join + count), Q17, Q10, Q3/Q4/Q12 and the date-string filters. A compact date/integer column representation would help most.
2. Run SF3 and SF10 on a machine with enough RAM; reduce string memory (compact string type or per-column loading).
3. Parallelise the remaining single-threaded paths (Q20 inner aggregation, string-key group-by, string sort).
4. Keep widening the differential and oracle tests: every round so far found new silent wrong answers.

## How to reproduce

```
cd kore
cargo build --release -p kore-ffi --offline
cd benchmarks/tpch_honest
python gen_data.py --sf 1 --out data/sf1
python run_engines.py --data data/sf1 --sf 1          # Spark and KORE, with result comparison
python run_other_engines.py --engine duckdb --data data/sf1 --sf 1   # also polars, datafusion (needs those installed, e.g. from conda-forge)
python compare_all.py --sf 1                          # combined table
python run_engines.py --data data/sf1 --sf 1 --engines kore --only Q1,Q9
```

Needs Python with `pyspark`, `pyarrow`, `numpy`, and Java 17+ for Spark. See `BENCHMARKING.md` for details.
