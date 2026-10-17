# How we benchmarked KORE honestly (and what that found)

*Written 2026-10-17. Everything here is measured on one 8-core, 32 GB Windows machine, on TPC-H-shaped data (our generator, not the official dbgen) and TPC-DS data
(DuckDB's dsdgen). See `ENGINE_STATUS_2026-10-04.md` for the current numbers and caveats.*

## 1. The starting point was a false claim

The repository said KORE was "339x faster than Spark". Reading the benchmark code showed why that could not be true: the Spark times were constants typed into the source,
the queries were simplified hand-written versions, and KORE's answers were never checked. We marked the old documents with a correction and replaced the comparison.

## 2. The method that replaced it

- The same data (CSV and Parquet), the same SQL text, run on every engine on the same machine.
- Spark 4.2 (local mode), DuckDB, DataFusion and Polars were run live; each engine's rows were compared with a reference engine with a tolerance for floats.
  A faster time only counts if the answer matches.
- Warm-up run, then the best of several runs; the time includes turning the result into Python objects for every engine.
- Because the machine is shared and noisy, speed comparisons between builds use interleaved A/B runs, and comparisons with DuckDB alternate DuckDB, KORE, DuckDB so a
  slow stretch shows up on both sides. Several "results" were discarded because a stray process or a busy desktop skewed them (DuckDB's own timing moved by 2x).
- Every optimisation sits behind an on/off switch, and every merge had to pass a correctness gate: the optimised plan versus every switch-off variant, plus a SQLite oracle,
  on randomly generated queries at sizes that cross the engine's thresholds.

## 3. What the method found

| Stage | Finding |
|---|---|
| First live comparison | Only 2 of 22 TPC-H queries returned correct answers. The earlier "speedups" were measuring wrong results. |
| Fixing TPC-H | Silent wrong answers: parser dropped tokens after `FROM a, b`, nested aggregates, `COUNT(col)`, CSV dictionary overflow, correlated scalar subqueries pre-evaluated as constants, unstable sort, mixed int/float comparisons. |
| Fuzzing (SQLite oracle) | 14 more wrong answers, e.g. a `LIMIT` pushed below a join, `NOT IN` dropping rows, `EXISTS` inside `OR`, doubles merged within 1e-10, BIGINT keys above 2^53 merged, a spill-path corruption of dictionary columns. |
| Hash-collision review | A fast `DISTINCT` that trusted a 128-bit hash would have dropped rows on a collision; it now confirms equality cell by cell. |
| TPC-DS (99 queries) | 7 more bugs, e.g. `SELECT expr, *` silently dropped the expression, constants next to `GROUP BY` came back as text. Before the fixes 82 of 99 matched DuckDB. |
| Hazard review | A reference to a nonexistent column evaluated to NULL instead of an error; a static binding check now raises `column not found`. |
| Test hygiene | One test appeared to hang in the full suite: the optimisation switches are process-wide, so a test that turned the hash join off made another test's join quadratic. Tests that flip switches now take a shared lock. |

## 4. What the numbers say now (see the status doc for details and caveats)

- On 22 TPC-H-shaped queries (SF 1 and SF 3, warm, in memory) KORE returns the same answers as Spark and DuckDB, is about 30x faster than Spark local mode, and is level with DuckDB
  (in the latest alternating runs somewhat ahead; the margin is not established because the machine was noisy).
- On the 99 TPC-DS queries (SF 1) KORE returns 97 identical answers (2 time out) and is about 2.8x slower than DuckDB, so the TPC-H result does not generalise.
- Not tested: data beyond SF 3, concurrency, cold starts, larger-than-memory data, production features. "One of the strongest engines" is not something these runs establish.

## 5. Lessons

1. Check the answers before timing anything: every speed claim that skipped this was wrong.
2. A second benchmark family finds a different set of bugs and slow spots than the first; one benchmark is not evidence of generality.
3. Fuzz against an independent oracle at the sizes where optimisations switch on, not only on tiny tables.
4. On a shared machine, report ranges and alternate the engines; do not quote a ratio from one run.
5. A switch for every optimisation made both testing and bisecting cheap.
6. Process-wide switches and parallel tests interact: lock them.

## 6. Reproduce

```
cd benchmarks/tpch_honest        # TPC-H: gen_data.py, run_engines.py, run_other_engines.py, compare_all.py
cd benchmarks/tpcds_honest       # TPC-DS: gen_data.py (DuckDB dsdgen), run_ds.py
bash kore-sql/tests/oracle/run_gate.sh   # correctness gate (SQLite oracle + switch differential)
```
See `docs/BENCHMARKING.md` and the READMEs in those folders.
