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
| Q1 | 1088 | 74 | 113 | 131 | 298 |
| Q2 | 1251 | 31 | 30 | 73 | unsupported |
| Q3 | 1503 | 160 | 58 | 163 | 56 |
| Q4 | 1429 | 155 | 70 | 129 | 96 |
| Q5 | 1846 | 86 | 43 | 221 | 126 |
| Q6 | 410 | 45 | 29 | 89 | 28 |
| Q7 | 1791 | 112 | 58 | 244 | 222 |
| Q8 | 1566 | 54 | 33 | 207 | 571 |
| Q9 | 2765 | 339 | 114 | 283 | 2904 |
| Q10 | 1754 | 309 | 76 | 236 | 115 |
| Q11 | 1026 | 20 | 9 | 61 | 40 |
| Q12 | 1548 | 170 | 95 | 162 | 164 |
| Q13 | 2110 | 850 | 87 | 137 | unsupported |
| Q14 | 605 | 66 | 50 | 115 | 49 |
| Q15 | 1159 | 49 | 38 | result differs* | 59 |
| Q16 | 1928 | 79 | 65 | 154 | 78 |
| Q17 | 1216 | 82 | 24 | 291 | unsupported |
| Q18 | 3116 | 215 | 88 | 541 | 1906 |
| Q19 | 608 | 132 | 106 | 141 | 59 |
| Q20 | 1372 | 128 | 54 | 159 | 197 |
| Q21 | 4156 | 278 | 227 | 410 | 2941 |
| Q22 | 1216 | 36 | 29 | 59 | 19 |

Answers agreeing with Spark: Spark 22/22 (reference), KORE 22/22, DuckDB 22/22, DataFusion 21/22, Polars 19/22.
Geometric mean over the 18 queries every engine answered correctly: Spark 1425 ms, KORE 106 ms, DuckDB 60 ms, DataFusion 166 ms, Polars 164 ms.
DuckDB was fastest on 16 of 22 queries, Polars on 5, KORE on 1 (Q1). KORE is about 1.8x slower than DuckDB in geometric mean, faster than
DataFusion and Polars overall, and about 13x faster than Spark local mode.

\* DataFusion returned no rows for Q15: the query compares a sum with `max()` of the same sum using float equality, and a different summation
order gives a slightly different float. This is a property of the query, not necessarily a DataFusion bug. Polars' SQL layer rejects Q2, Q13 and Q17
(correlated subquery / `NOT LIKE` in a join condition).

Timings: all five engines were re-run back to back on a quiet machine on 2026-10-04 (Spark's times are from its earlier cached run on the same machine).
Read this with care: one machine (8 cores, 32 GB, Windows), SF 1, in-memory, TPC-H-shaped data (not dbgen), best of 3 after a warm-up, roughly
+/-20% noise . Engines were installed from conda-forge: DuckDB 1.5.6, DataFusion 54.0.0, Polars 1.44.2, Spark 4.2.0 (local mode).

Full tables: `results/`. Read these numbers with care:

* Spark here is a single-JVM local-mode run on a data set that fits in memory; its strengths (scale-out, spilling,
  fault tolerance) are not exercised. The comparison says nothing about 100 GB+ data or multiple nodes.
* DuckDB is still faster than KORE on most queries (about 1.8x in geometric mean). KORE is faster than DataFusion and Polars overall and faster than Spark local mode.
* KORE keeps every table fully in memory as row-oriented `Option<T>` vectors; data much larger than RAM does not work.
* The data is TPC-H-shaped, not the official dbgen output, and no official TPC-H result is claimed.
