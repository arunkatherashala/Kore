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
| Q1 | 1088 | 293 | 86 | 166 | 321 |
| Q2 | 1251 | 41 | 22 | 103 | unsupported |
| Q3 | 1503 | 240 | 57 | 252 | 80 |
| Q4 | 1429 | 152 | 64 | 163 | 104 |
| Q5 | 1846 | 233 | 31 | 294 | 149 |
| Q6 | 410 | 73 | 24 | 116 | 41 |
| Q7 | 1791 | 452 | 58 | 337 | 456 |
| Q8 | 1566 | 238 | 28 | 268 | 717 |
| Q9 | 2765 | 698 | 98 | 467 | 3171 |
| Q10 | 1754 | 271 | 66 | 299 | 126 |
| Q11 | 1026 | 120 | 13 | 75 | 34 |
| Q12 | 1548 | 171 | 78 | 254 | 154 |
| Q13 | 2110 | 770 | 73 | 202 | unsupported |
| Q14 | 605 | 89 | 36 | 146 | 50 |
| Q15 | 1159 | 79 | 30 | result differs* | 44 |
| Q16 | 1928 | 97 | 52 | 181 | 79 |
| Q17 | 1216 | 228 | 17 | 424 | unsupported |
| Q18 | 3116 | 756 | 78 | 772 | 2337 |
| Q19 | 608 | 106 | 78 | 235 | 105 |
| Q20 | 1372 | 618 | 47 | 218 | 284 |
| Q21 | 4156 | 685 | 203 | 432 | 3520 |
| Q22 | 1216 | 282 | 26 | 71 | 31 |

Answers agreeing with Spark: Spark 22/22 (reference), KORE 22/22, DuckDB 22/22, DataFusion 21/22, Polars 19/22.
Geometric mean over the 18 queries every engine answered correctly: Spark 1425 ms, KORE 238 ms, DuckDB 52 ms, DataFusion 223 ms, Polars 203 ms.
DuckDB was fastest on all 22 queries, about 4.6x faster than KORE in geometric mean.

\* DataFusion returned no rows for Q15: the query compares a sum with `max()` of the same sum using float equality, and a different summation
order gives a slightly different float. This is a property of the query, not necessarily a DataFusion bug. Polars' SQL layer rejects Q2, Q13 and Q17
(correlated subquery / `NOT LIKE` in a join condition).

Read this with care: one machine (8 cores, 32 GB, Windows), SF 1, in-memory, TPC-H-shaped data (not dbgen), best of 3 after a warm-up, roughly
+/-20% noise (more for KORE in this run, which was timed while other programs were running: earlier quiet runs gave Q1 217-236 ms and
Q9 561-616 ms). Engines were installed from conda-forge: DuckDB 1.5.6, DataFusion 54.0.0, Polars 1.44.2, Spark 4.2.0 (local mode).

Full tables: `results/`. Read these numbers with care:

* Spark here is a single-JVM local-mode run on a data set that fits in memory; its strengths (scale-out, spilling,
  fault tolerance) are not exercised. The comparison says nothing about 100 GB+ data or multiple nodes.
* DuckDB is clearly faster than KORE on this workload. KORE is in the same range as DataFusion and Polars, and faster than Spark local mode.
* KORE keeps every table fully in memory as row-oriented `Option<T>` vectors; data much larger than RAM does not work.
* The data is TPC-H-shaped, not the official dbgen output, and no official TPC-H result is claimed.
