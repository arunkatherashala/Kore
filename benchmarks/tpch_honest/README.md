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

## Scale check: SF 3 (18M lineitem rows), 2026-10-12

KORE and DuckDB only (Spark does not fit in the memory left on the test machine next to them, so **DuckDB is the correctness reference at this scale**;
DuckDB agreed with Spark on 22 of 22 queries at SF 1). Same Parquet/CSV data and SQL text, best of 2 after a warm-up, times in milliseconds.

| Query | KORE | DuckDB |
|---|---|---|
| Q1 | 220 | 208 |
| Q2 | 33 | 49 |
| Q3 | 486 | 129 |
| Q4 | 215 | 179 |
| Q5 | 255 | 83 |
| Q6 | 57 | 72 |
| Q7 | 306 | 155 |
| Q8 | 138 | 66 |
| Q9 | 1018 | 270 |
| Q10 | 814 | 145 |
| Q11 | 76 | 17 |
| Q12 | 203 | 240 |
| Q13 | 622 | 231 |
| Q14 | 85 | 97 |
| Q15 | 206 | 75 |
| Q16 | 268 | 223 |
| Q17 | 131 | 45 |
| Q18 | 766 | 209 |
| Q19 | 274 | 192 |
| Q20 | 492 | 133 |
| Q21 | 498 | 468 |
| Q22 | 137 | 49 |

Answers agreeing with DuckDB: KORE 22/22. Geometric mean over the 22 queries: KORE 236 ms, DuckDB 119 ms (KORE about 2.0x slower).
Fastest per query: kore 4, duckdb 18.

What this shows: KORE completes all 22 queries at 3x the SF1 data and returns the same answers as DuckDB; an earlier SF3 attempt had failed with an
out-of-memory error, before the memory-footprint work. The gap to DuckDB is wider than at SF 1 (best quiet SF 1 runs were about level), mostly on
join-heavy queries (Q3 486 vs 129 ms, Q5 255 vs 83, Q9 1018 vs 270, Q10 814 vs 145, Q18 766 vs 209), so KORE does not scale as well as DuckDB here.
Not measured: KORE's peak memory at SF 3 (it ran in the roughly 9.7 GB of RAM that was available, with other programs open), Spark at SF 3, anything larger.
Same caveats as above: one shared machine with large run-to-run variance, TPC-H-shaped data, warm in-memory runs.

## After the Phase 1 optimisation round (2026-10-14)

Changes in this round (all merged behind on/off switches, each with a differential test): wider fused group-by (any number of key columns, text keys
verified against numeric keys, sorted-run mode, SUBSTR as dictionary column), parallel partitioned hash build / probe concatenation / in-column gather,
SIP key sets as bitmaps (no 400k cap), estimate-ordered joins, lazy base table and one late payload gather, exact packed multi-column Int64 join keys.
Aggregate push-down below joins was investigated and deliberately not implemented: the join output is already small after filters and SIP, so
pre-aggregating would cost more than it saves and adds wrong-answer risk.

Interleaved A/B on the same machine (previous build vs merged build, geometric mean over all 22 queries, ms, two rounds each):

| Scale | Previous build | Merged build |
|---|---|---|
| SF 1 | 98 and 75 | 61 and 57 |
| SF 3 | 246 and 212 | 150 and 149 |

That is roughly 30-40% faster. All answers still match (22/22 vs Spark at SF 1, 22/22 vs DuckDB at SF 3).

KORE vs DuckDB, alternating runs (DuckDB measured before and after each KORE run; geometric mean over 22 queries, ms):

| | DuckDB before | KORE | DuckDB after |
|---|---|---|---|
| SF 1 round 1 | 65 | 52 | 45 |
| SF 1 round 2 | 45 | 69 | 85 |
| SF 3 round 1 | 176 | 165 | 126 |
| SF 3 round 2 | 127 | 174 | 121 |

Reading: at SF 1 the two are indistinguishable (DuckDB alone swings between 45 and 85 ms on this machine). At SF 3, comparing against DuckDB's quiet runs
(120-127 ms), KORE is about 1.4x slower (165-174 ms), down from about 2x before this round. **The target "KORE faster than DuckDB at SF 3" was not reached.**
Remaining gap by area: string-heavy group-by and gathers (Q10, Q18, Q9), subquery and outer-join queries (Q4, Q11, Q13, Q15, Q17, Q20, Q21, Q22),
fixed per-query overhead on the 10-60 ms queries.

Correctness gate on the merged code (`kore-sql/tests/oracle/run_gate.sh`: in-process differential with every optimisation switch on/off plus the
SQLite oracle, small, mid and five threshold sizes): GATE PASS, 0 disagreements, 0 oracle mismatches; 284 unit/integration tests pass.

Full tables: `results/`. Read these numbers with care:

* Spark here is a single-JVM local-mode run on a data set that fits in memory; its strengths (scale-out, spilling,
  fault tolerance) are not exercised. The comparison says nothing about 100 GB+ data or multiple nodes.
* KORE and DuckDB are about equal overall here, each faster on about half of the queries. This is a warm, in-memory, single-machine SF1 result: KORE caches an integer encoding of date columns after the first query on a table (the warm-up run absorbs it), and every engine's time includes turning the result into Python objects. Do not extrapolate to larger data, cold data or other workloads.
* KORE keeps every table fully in memory as row-oriented `Option<T>` vectors; data much larger than RAM does not work.
* The data is TPC-H-shaped, not the official dbgen output, and no official TPC-H result is claimed.
