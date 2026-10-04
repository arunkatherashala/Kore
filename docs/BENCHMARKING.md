# Benchmarking KORE SQL against Spark

The only benchmark in this repository that is meant to be quoted is `benchmarks/tpch_honest`. The older harnesses
(`kore-tpch`, `kore_vs_spark.py`) compare against hard-coded Spark numbers and do not verify KORE's answers; do not
quote them.

## What `benchmarks/tpch_honest` does

* `gen_data.py` generates **TPC-H-shaped** tables (CSV and Parquet). It is not the official `dbgen`; row counts of
  query results differ from the published answers and nothing here is an official TPC-H result.
* `run_engines.py` runs the 22 queries from `queries.py` on live PySpark (local mode) and on KORE, with the same SQL
  text over identical files, then compares KORE's result set with Spark's (by lower-cased column name, floats to
  1e-6 relative). Each query is reported as `ok`, `MISMATCH` (both ran, results differ: a bug in one engine),
  `error` (KORE rejected or failed it) or `timeout`.

## How to run

Requirements: Python with `pyspark`, `pyarrow`, `numpy`; Java 17+; a Rust toolchain.

```
cargo build --release -p kore-ffi --offline            # the KORE engine the harness loads
cd benchmarks/tpch_honest
python gen_data.py --sf 1 --out data/sf1               # data/ is gitignored
python run_engines.py --data data/sf1 --sf 1                      # Spark + KORE, all 22 queries
python run_engines.py --data data/sf1 --sf 1 --engines kore       # KORE only (reuses the cached Spark answers)
python run_engines.py --data data/sf1 --sf 1 --only Q1,Q6         # subset
```

`results/spark_cache_sf<N>.json` caches Spark's answers so that KORE-only runs still verify correctness; copy it next to
a fresh checkout if you do not want to run Spark. A good regression gate after touching shared execution code is
`--engines kore` on SF 1 and expecting 22 `ok` and 0 `MISMATCH`.

## Methodology

* Both engines read the same data; dates are ISO strings in both, so neither has a native-date advantage.
* Spark: local mode on all cores, tables cached in memory before timing. KORE: tables loaded into its in-memory
  session before timing.
* One warm-up run, then `--repeats` timed runs; the minimum is reported. Timings include materialising rows in Python
  (`collect()` for Spark, the JSON the KORE FFI returns for KORE).

## Caveats (read before quoting any number)

* **Not official TPC-H.** Generated data is TPC-H-shaped only.
* **Single machine, in memory.** The data fits in RAM. Spark's strengths (scale-out, spilling, fault tolerance, an
  ecosystem) are not exercised, and KORE keeps whole tables in memory as row-oriented `Option<T>` vectors, so data
  larger than RAM is not a supported scenario.
* **Spark local mode** is one JVM with its startup, planning and codegen costs; at small scale factors those dominate
  and flatter any native engine. Spark in a cluster is a different system.
* **Small scale factors** (0.1 to 1). Results at SF 100 are unknown.
* **Noise:** roughly +/-20% between runs on a shared workstation; other processes (including other builds) change the
  numbers. Re-run before drawing conclusions from a difference smaller than that.
* **Correctness is checked only for the 22 queries.** Broader SQL conformance is tracked separately in
  `docs/SQL_SUPPORT.md` and the `kore-sql/tests` suites.
* DuckDB, DataFusion and Polars are the relevant single-node competitors and are not part of the harness yet.
