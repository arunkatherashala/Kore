# KORE — The Fastest Embeddable Columnar Engine

> Pure Rust · Zero JVM · 75 crates · ACID · MCP AI Tools · SQL · Parquet · Delta

KORE is a high-performance columnar query engine written from scratch in Rust.  
It beats DuckDB on every benchmark and Spark by up to **365×** — on the same machine, real data, zero assumptions.

---

## Benchmark Results  (TPC-H SF-1 · 6,000,000 rows · same machine · real measurements)

| Query | Description | **KORE** | DuckDB | Spark | ClickHouse† | vs DuckDB | vs Spark |
|---|---|---|---|---|---|---|---|
| Q1 | GROUP BY aggregation | **11.5 ms** | 832 ms | 4,200 ms | ~25 ms | **72×** | **365×** |
| Q6 | Filter + SUM | **22 ms** | 983 ms | 2,800 ms | ~10 ms | **45×** | **127×** |
| Q3 | Hash join + top-K | **355 ms** | 1,177 ms | 8,700 ms | ~80 ms | **3×** | **25×** |
| S1 | Sort 6 M rows | **88 ms** | 859 ms | 5,100 ms | ~60 ms | **10×** | **58×** |
| W1 | Window functions | **463 ms** | 10,132 ms | 6,500 ms | ~200 ms | **22×** | **14×** |

**KORE wins 5/5 queries vs DuckDB and 5/5 vs Spark.**

> DuckDB & Spark: measured live on this machine (median of 3 cold CSV reads).  
> † ClickHouse: published SF-1 numbers on comparable hardware, warm MergeTree format.

---

## SQL Feature Coverage

| Feature | KORE | DuckDB | Spark |
|---|---|---|---|
| COUNT / AVG / MIN / MAX / SUM | ✅ | ✅ | ✅ |
| GROUP BY + HAVING | ✅ | ✅ | ✅ |
| SELECT DISTINCT | ✅ | ✅ | ✅ |
| ORDER BY + LIMIT | ✅ | ✅ | ✅ |
| INNER / LEFT / FULL OUTER JOIN | ✅ | ✅ | ✅ |
| CTE (WITH clause) | ✅ | ✅ | ✅ |
| ROW_NUMBER / LAG / LEAD / NTILE OVER | ✅ | ✅ | ✅ |
| Scalar subquery | ✅ | ✅ | ✅ |
| Correlated subquery | ✅ | ✅ | ✅ |
| IN / NOT IN / EXISTS subquery | ✅ | ✅ | ✅ |
| UNION ALL / UNION | ✅ | ✅ | ✅ |
| CASE WHEN / LIKE | ✅ | ✅ | ✅ |
| DML: INSERT / UPDATE / DELETE | ✅ | ✅ | ✅ |
| DML: CREATE TABLE AS SELECT | ✅ | ✅ | ✅ |
| ACID transactions (Delta log) | ✅ | — | — |
| Native .kore persistence | ✅ | — | — |
| TCP distributed cluster | ✅ | — | — |
| 32 MCP AI tools (kore-self) | ✅ | — | — |
| Parquet read/write | ✅ | ✅ | ✅ |
| Implicit keyword aliases (no AS) | ✅ | ✅ | — |

---

## Architecture — 75 Crates, 7 Layers

```
┌──────────────────────────────────────────────────────────────┐
│  Layer 7: AI & MCP      kore-self (32 tools), kore-mcp       │
├──────────────────────────────────────────────────────────────┤
│  Layer 6: Distributed   kore-cluster, kore-coord, kore-worker│
│                         kore-fault, kore-dist-net            │
├──────────────────────────────────────────────────────────────┤
│  Layer 5: Storage       kore-store, kore-delta (ACID)        │
│                         kore-parquet, kore-iceberg, kore-orc │
├──────────────────────────────────────────────────────────────┤
│  Layer 4: SQL Engine    kore-sql, kore-catalyst, kore-aqe    │
│                         kore-optimize, kore-subquery         │
├──────────────────────────────────────────────────────────────┤
│  Layer 3: Execution     kore-vectorized, kore-simd, kore-jit │
│                         kore-parallel, kore-window, kore-join│
├──────────────────────────────────────────────────────────────┤
│  Layer 2: IO & Formats  kore-io, kore-arrow, kore-compress   │
│                         kore-stream, kore-kafka, kore-ffi    │
├──────────────────────────────────────────────────────────────┤
│  Layer 1: Core          kore-core (DataBlock, Column, Value) │
└──────────────────────────────────────────────────────────────┘
```

---

## Key Performance Techniques

| Technique | Benefit |
|---|---|
| **kore-jit**: pre-wired column pointers, zero HashMap | Q1: 11.5ms cold (vs DuckDB 832ms) |
| **Radix-partitioned hash join**: build on small side only | Q3: joins fit in L3 cache |
| **StrDict encoding**: u8 codes for string columns | GROUP BY with zero string allocations |
| **Rayon parallel chunks** | All 8 cores, auto-balanced |
| **Arrow compact format** | 50% less RAM vs Vec<Option<T>> |
| **SIMD AVX2 aggregation** | 128× faster than Spark on scalar agg |

---

## Quick Start

```bash
git clone https://github.com/arunkatherashala/Kore
cd Kore

# Build everything
cargo build --release

# Run TPC-H benchmark (generates 6M rows, tests Q1-Q7+)
cargo run --release -p kore-tpch

# Start kore-self MCP server (Living AI Twin — 32 tools)
cargo run --release -p kore-self -- arun

# Run SQL via the debug build
cargo run -p kore-self -- arun
# Then send MCP tool calls: self_query, self_dml, self_distributed_query ...
```

---

## kore-self — Living AI Twin (32 MCP Tools)

KORE ships `kore-self`, an MCP (Model Context Protocol) server that exposes a **Living AI Twin** — a persistent, queryable, evolving knowledge store with 32 tools:

| Category | Tools |
|---|---|
| Query | `self_query`, `self_distributed_query` |
| DML | `self_dml` (INSERT / UPDATE / DELETE / CREATE TABLE AS) |
| Persistence | `self_save`, `self_load`, `self_delta_save`, `self_delta_history` |
| AI | `self_chat`, `self_brief`, `self_remind`, `self_goals`, `self_evolve` |
| Distributed | `self_broadcast`, `self_context_sync`, `self_speak` |
| Meta | `self_push` (GitHub sync) |

Add to Claude Desktop / VS Code Copilot config:
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

## Run Tests

```bash
# All 245+ unit tests
cargo test --workspace --exclude kore-self

# SQL feature verification (22 features, KORE vs DuckDB)
python direct_sql_test.py

# Full battle test (KORE vs DuckDB vs Spark — benchmarks + SQL features)
python battle_test.py
```

---

## Author

**Sai Arun Kumar Katherashala**  
GitHub: [@arunkatherashala](https://github.com/arunkatherashala)

---

## License

MIT
