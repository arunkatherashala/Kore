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

**Earlier note, written on 2026-10-12 while the machine was busy (superseded for the KORE-vs-DuckDB ordering by the 2026-10-15 idle-machine results below/above, but still true about how noisy a shared machine is): run-to-run variance.** The 48-50 ms vs 47 ms result above came from the quietest runs. Later repeated runs of the same code on the same shared machine gave KORE geometric means of 53, 86, 112, 137 and 141 ms (over the 22 queries the figures were 53-141 ms) and DuckDB 47-67 ms, mainly because other programs on the machine (Teams, Edge WebView, and three runaway helper processes that were found and stopped) took CPU in the middle of runs; in one run Q1-Q10 were unchanged and Q11-Q22 were 4-25x slower. An interleaved A/B of the previous and the current build (two rounds each) gave 56 / 84 ms and 66 / 70 ms: no consistent difference. So "about level with DuckDB" is a best case on a quiet machine. A fair summary is: between roughly level with and about 2x slower than DuckDB at SF1, much faster than Spark local mode, and the ordering with DuckDB is not stable across runs.

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

## Final results after Phase 1 and Phase 2 (2026-10-15, idle machine)

Merged: Phase 1 (wider fused group-by, parallel join mechanics, SIP bitmaps, estimate-ordered joins, lazy base table, packed multi-column keys) and
Phase 2 (text group keys gathered once per group, HAVING threshold inside the aggregation, extended top-N, direct correlated-scalar path, fused LEFT JOIN +
COUNT, parse cache, faster FFI JSON writer). 295 tests pass; correctness gate (`kore-sql/tests/oracle/run_gate.sh`: in-process differential with every switch
on/off + SQLite oracle, small, mid and five threshold sizes): GATE PASS, 0 disagreements, 0 oracle mismatches. All 22 answers match Spark (SF 1) and
DuckDB (SF 3) in every run.

Alternating runs, DuckDB measured before and after each KORE run, nothing else running (geometric mean over the 22 queries, ms):

| | DuckDB before | KORE | DuckDB after |
|---|---|---|---|
| SF 1 round 1 | 43.0 | 49.1 | 43.9 |
| SF 1 round 2 | 42.2 | 41.3 | 49.9 |
| SF 1 round 3 | 43.2 | 49.1 | 42.9 |
| SF 3 round 1 | 121.1 | 130.1 | 117.4 |
| SF 3 round 2 | 124.0 | 128.9 | 114.6 |
| SF 3 round 3 | 138.5 | 109.8 | 113.9 |

Means: SF 1 KORE 46.5 ms vs DuckDB 44.2 ms; SF 3 KORE 122.9 ms vs DuckDB 121.6 ms. **KORE and DuckDB are at parity on this benchmark within run-to-run noise
(about +/-10%): this is a tie, not a win.** The goal "KORE faster than DuckDB at SF 3" was not met in a way these runs can distinguish.

SF 3, round 3 per query (single round: not a stable ranking; KORE faster on 11 queries, DuckDB on 11):

| Query | KORE | DuckDB |
|---|---|---|
| Q1 | 155 | 202 |
| Q2 | 26 | 48 |
| Q3 | 136 | 138 |
| Q4 | 171 | 160 |
| Q5 | 85 | 78 |
| Q6 | 36 | 69 |
| Q7 | 86 | 133 |
| Q8 | 65 | 57 |
| Q9 | 430 | 223 |
| Q10 | 136 | 184 |
| Q11 | 33 | 16 |
| Q12 | 166 | 207 |
| Q13 | 189 | 201 |
| Q14 | 89 | 84 |
| Q15 | 128 | 90 |
| Q16 | 230 | 228 |
| Q17 | 28 | 41 |
| Q18 | 171 | 236 |
| Q19 | 42 | 184 |
| Q20 | 290 | 114 |
| Q21 | 627 | 524 |
| Q22 | 81 | 41 |

KORE's clearest remaining weak spots versus DuckDB: Q9 (430 vs 223 ms), Q20 (290 vs 114), Q11 (33 vs 16), Q22 (81 vs 41), Q15 (128 vs 90), Q21 (627 vs 524).
KORE's clearest strengths: Q19 (42 vs 184), Q6, Q7, Q1, Q10, Q17, Q18.

Scope of this claim: one machine (8 cores, 32 GB), TPC-H-shaped data (not the official dbgen), warm in-memory runs, 22 queries, SF 1 and SF 3 only. It says nothing
about other workloads (TPC-DS, ClickBench), larger scales, cold data, concurrency or production features.

## After Phase 3 (TPC-DS fixes, remaining TPC-H gaps), 2026-10-16

Merged: fixes found by running all 99 TPC-DS queries (see `benchmarks/tpcds_honest/README.md`), plus Q9/Q20/Q21/Q15/Q4/Q22 work (correlated-scalar scoping fix for Q20,
identity-move gather for lookup joins, direct integer slot table for EXISTS, selective fused aggregation, SUBSTR pushed to the source rows). 309 tests pass;
correctness gate (`run_gate.sh`): GATE PASS, 0 disagreements, 0 oracle mismatches; 22/22 answers match Spark (SF 1) and DuckDB (SF 3) in every run.

Alternating runs, DuckDB measured before and after each KORE run (geometric mean over 22 queries, ms):

| | DuckDB before | KORE | DuckDB after |
|---|---|---|---|
| SF 1 round 1 | 47.5 | 36.4 | 46.0 |
| SF 1 round 2 | 43.0 | 34.3 | 56.8 |
| SF 1 round 3 | 56.1 | 39.5 | 77.6 |
| SF 3 round 1 | 190.2 | 130.5 | 373.4 |
| SF 3 round 2 | 311.9 | 148.7 | 162.2 |
| SF 3 round 3 | 153.9 | 124.1 | 161.0 |

KORE was at or below DuckDB in all six rounds. **Do not read a precise ratio from this:** the machine was not quiet (three leftover test processes of mine were running,
one of them busy), and DuckDB's own before/after readings differ by up to 2x within one round (SF 3 round 1: 190 vs 373). On quieter days DuckDB measured 43-45 ms (SF 1) and
114-127 ms (SF 3) and KORE was level with it (see the section above). A fair summary: KORE is level with, and in these runs somewhat ahead of, DuckDB on TPC-H-shaped data
at SF 1 and SF 3; confirm on an idle machine before quoting a margin.

TPC-DS (99 queries, SF 1, DuckDB as the reference; run before the Phase 3 TPC-H work; superseded by the final TPC-DS results in `benchmarks/tpcds_honest/README.md`): 97 answers identical to DuckDB, 2 timeouts (Q47, Q95), 0 wrong. **KORE is about 2.8x slower
than DuckDB on TPC-DS** (geometric mean 125 ms vs 44 ms over the 97 agreeing queries; KORE faster on 32 of 97). Worst: Q70 and Q22 (ROLLUP), Q5, Q47 (offset self-join), Q95.
So the TPC-H result does not carry over to TPC-DS.

## Idle-machine alternating run, 2026-10-17 (commit ea542abf, includes the column-binding check)

DuckDB measured before and after each KORE run, nothing else running on purpose (geometric mean over the 22 queries, ms). 22/22 answers match Spark (SF 1) and DuckDB (SF 3) in every run.

| | DuckDB before | KORE | DuckDB after |
|---|---|---|---|
| SF 1 round 1 | 42.1 | 38.3 | 46.0 |
| SF 1 round 2 | 39.9 | 41.0 | 68.2 |
| SF 1 round 3 | 70.7 | 34.5 | 44.6 |
| SF 3 round 1 | 107.3 | 91.3 | 104.2 |
| SF 3 round 2 | 105.6 | 88.5 | 162.0 |
| SF 3 round 3 | 160.0 | 67.7 | 127.6 |

KORE was at or below DuckDB's reading in all six rounds (SF 1: 34.5-41.0 ms vs 39.9-70.7; SF 3: 67.7-91.3 ms vs 104.2-162.0). DuckDB's own readings still varied by up to 1.6x within a round, so no single ratio
is claimed. Comparing with DuckDB's quietest readings (SF 1 about 40-46 ms, SF 3 about 104-108 ms), KORE is roughly 10-25% lower on this benchmark. The earlier idle-looking runs on 2026-10-15 were at parity
(KORE 109.8-130.1 ms vs DuckDB 113.9-138.5 ms at SF 3) before the Phase 3 work. One caveat: the timing script counted 3 matching processes at its start (python, cargo or test processes of the project's tooling
were not all confirmed gone), so treat this as a strong indication, not a precise margin. Scope: TPC-H-shaped data, one machine, warm in-memory runs, SF 1 and SF 3 only. On TPC-DS KORE is still slower than DuckDB (see below).

Full tables: `results/`. Read these numbers with care:

* Spark here is a single-JVM local-mode run on a data set that fits in memory; its strengths (scale-out, spilling,
  fault tolerance) are not exercised. The comparison says nothing about 100 GB+ data or multiple nodes.
* KORE and DuckDB are about equal overall here, each faster on about half of the queries. This is a warm, in-memory, single-machine SF1 result: KORE caches an integer encoding of date columns after the first query on a table (the warm-up run absorbs it), and every engine's time includes turning the result into Python objects. Do not extrapolate to larger data, cold data or other workloads.
* KORE keeps every table fully in memory as row-oriented `Option<T>` vectors; data much larger than RAM does not work.
* The data is TPC-H-shaped, not the official dbgen output, and no official TPC-H result is claimed.
