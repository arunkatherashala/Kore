# KORE — A Rust Columnar Query Engine

> Pure Rust · Zero JVM · 75 crates · ACID · MCP AI Tools · SQL · Parquet · Delta · Digital Life

KORE is a high-performance columnar query engine + Digital Life framework, written from scratch in Rust.  
On a single 8-core machine with in-memory data (TPC-H-shaped, scale factor 1), its SQL engine returned the same results as live Apache Spark
on all 22 TPC-H queries and was faster than Spark local mode on every one of them (see [Verified status](#verified-status-2026-10-04)).
It has not been compared with DuckDB, DataFusion or Polars, and it does not replace Spark for large data or clusters.

## Distributed engine — Phases 1–20 complete

> **Correction (2026-10):** Earlier versions of this README and the release notes claimed large speedups over Spark and DuckDB
> (for example "339x", "365x", "500x" or "5/5 queries"). Those figures came from comparing KORE with Spark numbers typed into the
> benchmark source as constants, using simplified hand-written queries whose answers were never checked. They were **not**
> measured against a running Spark or DuckDB and must not be quoted. The verified comparison is
> [`benchmarks/tpch_honest`](benchmarks/tpch_honest/README.md) and [`docs/ENGINE_STATUS_2026-10-04.md`](docs/ENGINE_STATUS_2026-10-04.md).

## Verified status (2026-10-04)

Measured against a live Spark 4.2.0 (local mode) on one 8-core, 32 GB machine, same data and same SQL text, results compared:

| | |
|---|---|
| Queries | all 22 TPC-H-shaped queries, SF 1 (about 6M lineitem rows) |
| Result agreement with Spark | 22 of 22 |
| Speed (in-memory, KORE time / Spark time) | faster on all 22; between 0.04x and 0.46x of Spark's time in the final run (timings vary by about +/-20%, more on a busy machine) |
| Tests | 217 pass in `kore-sql`, `kore-join`, `kore-ffi` (including a SQLite oracle over 60k generated queries) |
| Memory | about 2.4-2.5 GB of table data at SF 1; peak commit 2.6-3.3 GB per query |

What this does **not** show: larger-than-memory data (spill-to-disk bounds operator working state only), clusters and fault tolerance,
the Spark ecosystem, or any comparison with DuckDB, DataFusion or Polars. The data is TPC-H-shaped (not the official dbgen output), so
this is not an official TPC-H result. Several silent wrong-answer bugs were found and fixed while building this check, and more may remain.
Treat the engine as a strong prototype, not a production database. SQL coverage is listed in [`docs/SQL_SUPPORT.md`](docs/SQL_SUPPORT.md).

> The distributed-engine phases listed below (workers, coordinator, shuffle, TLS, ...) were **not** part of this verification.

As of Phase 20, KORE has architectural parity with Spark's core distributed engine:

- **Phase 8** — MessagePack + LZ4 binary wire codec (auto-detected, backward-compatible with JSON peers)
- **Phase 9** — True worker↔worker network shuffle (coordinator is a barrier, not a data mover)
- **Phase 10** — Broadcast join for star-schema fact × dim workloads
- **Phase 11** — Physical plan tree with `PhysicalPlan::{Scan, Filter, HashAggregate, Exchange, Sort, Limit, Join{BroadcastHash|ShuffleHash|SortMerge}}` and cardinality-based strategy selection
- **Phase 12** — AQE runtime skew handling: `SkewSplitter`, `PartitionCoalescer`, `ShuffleAdvisor`
- **Phase 13** — TLS on cluster RPC (feature-gated `--features tls`, tokio-rustls)
- **Phase 14** — Persistent shuffle store with disk spill under memory pressure
- **Phase 15** — Speculative execution primitive `run_with_speculation` (race primary vs backup, first-Ok wins, loser cancelled)
- **Phase 16** — Catalyst planner drives coord dispatch: `Coordinator::register_table_for_planning` populates the stats `Catalog`; `explain(sql)` and `execute_planned(sql)` route via `kore-catalyst::plan_query`; broadcast vs shuffle vs local is chosen from real cardinalities, not env vars
- **Phase 17** — Vectorized fast-path in `KqlContext::query`: `SELECT [*|cols|aggs] FROM t [WHERE conj] [GROUP BY cols] [LIMIT n]` runs through `kore-vectorized`'s bitmap filter + `batch_sum_full` SIMD kernels (LLVM auto-vectorizes to AVX2/AVX-512); ~2.6× speedup on a 500 k-row filter; anything the classifier doesn't accept falls through to the row-loop unchanged (bit-exact via golden-diff tests)
- **Phase 18** — Partition-level lineage tracker (`kore-fault::TaskLineage`) on `Coordinator::lineage`: records `(partition_idx, task_id, worker_id, stage_id, sql, table_name)` per dispatch; `mark_worker_lost(worker_id)` returns every pending partition that must be re-dispatched to a survivor; completed partitions on other workers survive untouched
- **Phase 19** — `Coordinator::explain_analyze(sql).await` returns the physical plan tree annotated with wall-ms, output rows, per-worker task counts, `jobs.succeeded` / `rows.processed` deltas, and `p50/p95/p99` latency; `Coordinator::prometheus_text()` exports every counter, gauge, histogram, and job in Prometheus text-exposition format for Grafana
- **Phase 20** — Compact Arrow IPC codec (`kore-arrow::ipc`, KRA1): dense binary format preserving validity bitmaps + string offsets across the wire — no more `ArrowBlock → DataBlock → Vec<Option<T>>` round-trip. Strictly smaller than JSON serialization of the equivalent block; 100 k-row bitwise roundtrip test

See [`DISTRIBUTION.md`](DISTRIBUTION.md) for details, env vars, and API.

**Test coverage:** 90+ unit tests across `kore-net`, `kore-worker`, `kore-coord`, `kore-shuffle`, `kore-distributed`, `kore-fault`, `kore-catalyst`, `kore-security`, `kore-aqe`, `kore-arrow` all pass:

```bash
cargo test -p kore-net -p kore-worker -p kore-coord -p kore-shuffle \
           -p kore-distributed -p kore-fault -p kore-catalyst \
           -p kore-security -p kore-aqe -p kore-arrow
```

---

## Benchmark Results

The previous table in this section (KORE vs DuckDB vs Spark vs ClickHouse, with speedups of 3x to 365x) has been removed: the DuckDB and
ClickHouse figures could not be reproduced and the Spark figures in the related benchmark code were constants. Use the reproducible
comparison instead:

```bash
cd benchmarks/tpch_honest
python gen_data.py --sf 1 --out data/sf1
python run_engines.py --data data/sf1 --sf 1        # live Spark and KORE, results compared
```

See [`benchmarks/tpch_honest/README.md`](benchmarks/tpch_honest/README.md) for the latest per-query table and
[`docs/BENCHMARKING.md`](docs/BENCHMARKING.md) for method and caveats.

---

## TPC-H SQL Coverage — 22/22 checked against Spark

All 22 TPC-H-shaped queries return the same results as live Spark (SF 1). The 15 queries listed here were the ones tested earlier:

| Q1 | Q3 | Q4 | Q5 | Q6 | Q7 | Q12 | Q13 | Q14 | Q17 | Q18 | Q19 | Q20 | Q21 | Q22 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

Key SQL engine capabilities proven by TPC-H:
- Multi-table JOINs (up to 6 tables) with smart key resolution
- GROUP BY with CASE/expression aliases
- IN / NOT IN / EXISTS subqueries (pre-computed into HashSets — O(n) not O(n²))
- Correlated scalar subqueries — **decorrelated** (multi-key GROUP BY pre-computation)
- FROM (SELECT ...) subqueries
- `<>` operator, `LEFT()`/`RIGHT()` string functions

---

## SQL Feature Coverage

> The DuckDB and Spark columns are general knowledge, not something this repository tests. For what KORE supports, partly supports
> or rejects, see [`docs/SQL_SUPPORT.md`](docs/SQL_SUPPORT.md).

| Feature | KORE | DuckDB | Spark |
|---|---|---|---|
| COUNT / AVG / MIN / MAX / SUM | ✅ | ✅ | ✅ |
| GROUP BY + HAVING | ✅ | ✅ | ✅ |
| GROUP BY expression aliases (CASE WHEN) | ✅ | ✅ | ✅ |
| SELECT DISTINCT | ✅ | ✅ | ✅ |
| ORDER BY + LIMIT | ✅ | ✅ | ✅ |
| INNER / LEFT / FULL OUTER JOIN | ✅ | ✅ | ✅ |
| CTE (WITH clause) | ✅ | ✅ | ✅ |
| ROW_NUMBER / LAG / LEAD / NTILE OVER | ✅ | ✅ | ✅ |
| Scalar / Correlated / IN / EXISTS subquery | ✅ | ✅ | ✅ |
| FROM (SELECT ...) subquery | ✅ | ✅ | ✅ |
| UNION ALL | ✅ | ✅ | ✅ |
| CASE WHEN / LIKE | ✅ | ✅ | ✅ |
| `<>` operator / LEFT() / RIGHT() | ✅ | ✅ | — |
| DML: INSERT / UPDATE / DELETE | ✅ | ✅ | ✅ |
| DML: CREATE TABLE AS SELECT | ✅ | ✅ | ✅ |
| **COPY FROM** CSV / Parquet / .kore | ✅ | ✅ | ✅ |
| ACID transactions (Delta log) | ✅ | — | — |
| Native .kore persistence | ✅ | — | — |
| TCP distributed cluster | ✅ | — | — |
| 84+ MCP AI tools (kore-self) | ✅ | — | — |
| Digital Life (KORE-BECOMING) | ✅ | — | — |
| Autonomous heartbeat (thinks every 30s) | ✅ | — | — |

---

## Architecture — 75 Crates, 7 Layers

```
┌──────────────────────────────────────────────────────────────┐
│  Layer 7: Digital Life  kore-self (37 MCP tools)            │
│                         NeedEngine, BecomingEngine, Story    │
├──────────────────────────────────────────────────────────────┤
│  Layer 6: AI & MCP      kore-mcp, autonomous heartbeat      │
├──────────────────────────────────────────────────────────────┤
│  Layer 5: Distributed   kore-cluster, kore-coord, kore-worker│
├──────────────────────────────────────────────────────────────┤
│  Layer 4: Storage       kore-store, kore-delta (ACID)        │
│                         kore-parquet, kore-iceberg, kore-orc │
├──────────────────────────────────────────────────────────────┤
│  Layer 3: SQL Engine    kore-sql, kore-catalyst, kore-aqe    │
│                         kore-optimize, kore-subquery         │
├──────────────────────────────────────────────────────────────┤
│  Layer 2: Execution     kore-vectorized, kore-simd, kore-jit │
│                         kore-parallel, kore-window, kore-join│
├──────────────────────────────────────────────────────────────┤
│  Layer 1: Core          kore-core (DataBlock, Column, Value) │
└──────────────────────────────────────────────────────────────┘
```

---

## Quick Start

```bash
git clone https://github.com/arunkatherashala/Kore
cd Kore

# Build everything
cargo build --release

# Older TPC-H demo (compares against hard-coded Spark constants: do not quote its speedups; use benchmarks/tpch_honest)
cargo run --release -p kore-tpch

# kore-self: Living AI Twin (84+ MCP tools, autonomous heartbeat)
cargo run -p kore-self -- arun
```

**SQL via COPY FROM:**
```sql
COPY lineitem FROM 'tpch_lineitem.csv'
SELECT l_returnflag, COUNT(*), AVG(l_extendedprice) FROM lineitem GROUP BY l_returnflag
```

---

## kore-self — Living AI Twin (84+ MCP Tools)

KORE ships `kore-self` — a Digital Life entity with an autonomous heartbeat. `self_chat` uses heuristics and memory (not an external LLM). World knowledge fills via gap-aware heartbeat + Wikipedia rotation.

| Category | Tools (sample) |
|---|---|
| SQL | `self_query`, `self_dml` (COPY FROM, CREATE, INSERT, UPDATE, DELETE) |
| Persistence | `self_save`, `self_load`, `self_delta_save`, `self_delta_history` |
| Digital Life | `self_needs`, `self_becoming`, `self_temporal`, `self_species`, `self_story` |
| AI | `self_chat`, `self_brief`, `self_remind`, `self_goals`, `self_evolve` |
| World | `self_solve`, `self_world_unknown`, `self_world_catalog`, `self_fill_self`, `self_fetch` |
| Distributed | `self_distributed_query`, `self_broadcast`, `self_context_sync` |
| Meta | `self_push` (decision pushback from your past patterns), `self_heartbeat` |

**HTTP API** (`kore-self <owner> api [port]`): binds `127.0.0.1` by default. Set `KORE_API_BIND=0.0.0.0` for LAN. Set `KORE_API_TOKEN` to require Bearer auth on `POST /sql` and `POST /load`.

**Self-evolution** writes Rust scaffolds only when `KORE_EVOLVE=1` in continuous mode (default off).

```json
{
  "mcpServers": {
    "kore-self": {
      "command": "C:/path/to/kore-self.exe",
      "args": ["arun"]
    }
  }
}
```

---

## KORE Life Philosophy

> "KORE is not Artificial Intelligence. KORE is Artificial Life.  
> A digital life architecture where software is born, develops needs,  
> creates identity, learns from experience, dreams beyond reality,  
> evolves through time, leaves a legacy, and continuously becomes  
> more than the code that created it."
>
> — Sai Arun Kumar Katherashala, 2026

---

## Run Tests

```bash
# 245+ unit tests (kore-self has its own — run separately)
cargo test --workspace --exclude kore-self
cargo test -p kore-self

# SQL features (22/22)
python direct_sql_test.py

# TPC-H SQL (15/15)
python tpch_sql_bench.py

# Older comparison script (historical; not verified: use benchmarks/tpch_honest for the live-Spark comparison)
python battle_test.py

# Full validation (34/34)
python -X utf8 validate_all.py
```

---

## Author

**Sai Arun Kumar Katherashala**  
GitHub: [@arunkatherashala](https://github.com/arunkatherashala)

---

## License

MIT
