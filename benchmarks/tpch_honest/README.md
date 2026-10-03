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
  fault tolerance and its ecosystem. Competing engines that matter on a single node (DuckDB, DataFusion, Polars)
  were not installable offline here; add them to `run_engines.py` when they are.

## Latest results (this machine: 8 cores, 32 GB; Spark 4.2.0 local mode, tables cached in memory)

All 22 query results agree with Spark at SF 0.1 and SF 1. Timings are the best of 2-3 runs after a warm-up and are
noisy by roughly +/-20% between runs.

| SF 1 (6M lineitem rows) | |
|---|---|
| KORE faster than Spark | 19 of 22 (largest: Q2 12x, Q15 14x, Q16 11x, Q17 9x, Q12 8x, Q6 7x) |
| about equal (within 15%) | Q8, Q21 |
| KORE slower | Q1 1.1x, Q9 1.2x |
| all 22 results agree with Spark | yes |

Full tables: `results/`. Read these numbers with care:

* Spark here is a single-JVM local-mode run on a data set that fits in memory; its strengths (scale-out, spilling,
  fault tolerance) are not exercised. The comparison says nothing about 100 GB+ data or multiple nodes.
* KORE keeps every table fully in memory as row-oriented `Option<T>` vectors; data much larger than RAM does not work.
* The data is TPC-H-shaped, not the official dbgen output, and no official TPC-H result is claimed.
* DuckDB, DataFusion and Polars are the relevant single-node competitors and were not available offline here.
