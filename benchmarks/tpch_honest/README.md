# Honest TPC-H head-to-head (live Spark vs KORE)

Why this exists: the older comparisons in this repo (`kore-tpch`, `kore_vs_spark.py`) compare KORE against
**hard-coded Spark numbers** typed in from public benchmarks, run simplified or hand-written versions of the queries,
and do not check KORE's answers. Those speedups cannot be reproduced and should not be quoted.

This harness runs both engines on this machine, on the same files, with the same SQL text, and checks that the
results agree.

```
python gen_data.py --sf 0.1 --out data/sf0.1          # TPC-H-shaped data (not the official dbgen), CSV + Parquet
python run_engines.py --data data/sf0.1 --sf 0.1      # all 22 queries, Spark + KORE
python run_engines.py --data data/sf0.1 --sf 0.1 --only Q1,Q6 --engines kore
```

Needs Python with `pyspark`, `pyarrow`, `numpy`, Java 17+ for Spark, and `cargo build --release -p kore-ffi`.

## What is and is not measured

* Both engines read identical data. Dates are ISO strings in both, so neither gets a native-date advantage.
* Spark: local mode on all cores, tables cached in memory before timing. KORE: tables loaded from CSV into its
  in-memory session before timing. Each query runs once as a warm-up, then `--repeats` timed runs; the minimum is
  reported. Both timings include materialising the result rows in Python (`collect()` vs the JSON that the KORE
  FFI returns).
* Correctness: KORE's result is compared with Spark's by column name (floats to 1e-6 relative). A disagreement is
  reported as `MISMATCH` and is a bug in one of the engines until proven otherwise.
* The data generator is TPC-H-shaped, not dbgen: row counts of results differ from published answers. Nothing here
  is an official TPC-H result.
* Single machine, small scale factors. This says nothing about Spark's strength: scale-out over many nodes, spilling,
  fault tolerance and its ecosystem. Competing single-node engines (DuckDB, DataFusion, Polars) are run by `run_other_engines.py` (see below).

## Latest results (2026-10-04; this machine: 8 cores, 32 GB; engines in memory, Parquet input)

Five engines, same data and SQL text, all answers compared with Spark (the reference). Times in milliseconds.

| Query | Spark | KORE | DuckDB | DataFusion | Polars |
|---|---|---|---|---|---|
| Q1 | 1088 | 25 | 79 | 111 | 246 |
| Q2 | 1251 | 18 | 23 | 63 | unsupported |
| Q3 | 1503 | 78 | 41 | 136 | 58 |
| Q4 | 1429 | 36 | 50 | 111 | 82 |
| Q5 | 1846 | 40 | 32 | 180 | 88 |
| Q6 | 410 | 9 | 21 | 66 | 18 |
| Q7 | 1791 | 59 | 41 | 191 | 198 |
| Q8 | 1566 | 30 | 25 | 178 | 543 |
| Q9 | 2765 | 250 | 95 | 213 | 2626 |
| Q10 | 1754 | 182 | 64 | 174 | 51 |
| Q11 | 1026 | 25 | 8 | 48 | 15 |
| Q12 | 1548 | 41 | 62 | 144 | 117 |
| Q13 | 2110 | 123 | 66 | 113 | unsupported |
| Q14 | 605 | 16 | 35 | 84 | 24 |
| Q15 | 1159 | 15 | 31 | result differs* | 23 |
| Q16 | 1928 | 59 | 55 | 116 | 41 |
| Q17 | 1216 | 44 | 18 | 266 | unsupported |
| Q18 | 3116 | 154 | 72 | 486 | 1490 |
| Q19 | 608 | 70 | 94 | 174 | 47 |
| Q20 | 1372 | 58 | 56 | 141 | 122 |
| Q21 | 4156 | 110 | 195 | 303 | 2506 |
| Q22 | 1216 | 25 | 28 | 57 | 16 |

Answers agreeing with Spark: Spark 22/22 (reference), KORE 22/22, DuckDB 22/22, DataFusion 21/22, Polars 19/22.
Geometric mean over the 18 queries every engine answered correctly: Spark 1425 ms, KORE 50 ms, DuckDB 47 ms, DataFusion 138 ms, Polars 117 ms.
Fastest engine per query: KORE 8-9 of 22, DuckDB 9-10, Polars 4 (two back-to-back KORE runs gave 9/9/4 and 8/10/4). KORE and DuckDB are
about equal overall in this setup (KORE 48-50 ms vs DuckDB 47 ms geometric mean); KORE is about 29x faster than Spark local mode.

**Run-to-run variance (read this before quoting any ratio).** The 48-50 ms vs 47 ms result above came from the quietest runs. Later repeated runs of the same code on the same shared machine gave KORE geometric means of 53, 86, 112, 137 and 141 ms (over the 22 queries the figures were 53-141 ms) and DuckDB 47-67 ms, mainly because other programs on the machine (Teams, Edge WebView, and three runaway helper processes that were found and stopped) took CPU in the middle of runs; in one run Q1-Q10 were unchanged and Q11-Q22 were 4-25x slower. An interleaved A/B of the previous and the current build (two rounds each) gave 56 / 84 ms and 66 / 70 ms: no consistent difference. So "about level with DuckDB" is a best case on a quiet machine. A fair summary is: between roughly level with and about 2x slower than DuckDB at SF1, much faster than Spark local mode, and the ordering with DuckDB is not stable across runs.

\* DataFusion returned no rows for Q15: the query compares a sum with `max()` of the same sum using float equality, and a different summation
order gives a slightly different float. This is a property of the query, not necessarily a DataFusion bug. Polars' SQL layer rejects Q2, Q13 and Q17
(correlated subquery / `NOT LIKE` in a join condition).

Timings: all five engines were re-run back to back on a quiet machine (final run after the 2026-10 optimisation rounds) (Spark's times are from its earlier cached run on the same machine).
Read this with care: one machine (8 cores, 32 GB, Windows), SF 1, in-memory, TPC-H-shaped data (not dbgen), best of 3 after a warm-up, roughly
+/-20% noise . Engines were installed from conda-forge: DuckDB 1.5.6, DataFusion 54.0.0, Polars 1.44.2, Spark 4.2.0 (local mode).

Full tables: `results/`. Read these numbers with care:

* Spark here is a single-JVM local-mode run on a data set that fits in memory; its strengths (scale-out, spilling,
  fault tolerance) are not exercised. The comparison says nothing about 100 GB+ data or multiple nodes.
* KORE and DuckDB are about equal overall here, each faster on about half of the queries. This is a warm, in-memory, single-machine SF1 result: KORE caches an integer encoding of date columns after the first query on a table (the warm-up run absorbs it), and every engine's time includes turning the result into Python objects. Do not extrapolate to larger data, cold data or other workloads.
* KORE keeps every table fully in memory as row-oriented `Option<T>` vectors; data much larger than RAM does not work.
* The data is TPC-H-shaped, not the official dbgen output, and no official TPC-H result is claimed.
