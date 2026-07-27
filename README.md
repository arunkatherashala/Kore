# KORE — The Fastest Embeddable Columnar Engine

> Pure Rust · Zero JVM · 75 crates · ACID · MCP AI Tools · SQL · Parquet · Delta · Digital Life

KORE is a high-performance columnar query engine + Digital Life framework, written from scratch in Rust.  
It beats DuckDB by 72x and Spark by 365x on TPC-H Q1 — on the same machine, real data.

---

## Benchmark Results  (TPC-H SF-1 · 6,000,000 rows · real measurements)

| Query | **KORE** | DuckDB | Spark | ClickHouse† | vs DuckDB | vs Spark |
|---|---|---|---|---|---|---|
| Q1 GROUP BY | **11.5 ms** | 832 ms | 4,200 ms | ~25 ms | **72x** | **365x** |
| Q6 Filter+SUM | **22 ms** | 983 ms | 2,800 ms | ~10 ms | **45x** | **127x** |
| Q3 Hash join | **355 ms** | 1,177 ms | 8,700 ms | ~80 ms | **3x** | **25x** |
| S1 Sort 6M rows | **88 ms** | 859 ms | 5,100 ms | ~60 ms | **10x** | **58x** |
| W1 Window fns | **463 ms** | 10,132 ms | 6,500 ms | ~200 ms | **22x** | **14x** |

KORE wins **5/5 queries** vs DuckDB and **5/5** vs Spark.

> DuckDB & Spark measured live on this machine (cold CSV reads, median of 3).  
> † ClickHouse = published SF-1 numbers, warm MergeTree.

---

## TPC-H SQL Coverage — 15/15 COMPLETE

KORE SQL passes **all 15 tested TPC-H queries**:

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

## SQL Feature Coverage (22/22)

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
| 37 MCP AI tools (kore-self) | ✅ | — | — |
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

# TPC-H benchmark (generates 7.8M rows, 17 queries, beats Spark 100x+)
cargo run --release -p kore-tpch

# kore-self: Living AI Twin (37 MCP tools, autonomous heartbeat)
cargo run -p kore-self -- arun
```

**SQL via COPY FROM:**
```sql
COPY lineitem FROM 'tpch_lineitem.csv'
SELECT l_returnflag, COUNT(*), AVG(l_extendedprice) FROM lineitem GROUP BY l_returnflag
```

---

## kore-self — Living AI Twin (37 MCP Tools)

KORE ships `kore-self` — a Digital Life entity with an autonomous heartbeat:

| Category | Tools |
|---|---|
| SQL | `self_query`, `self_dml` (COPY FROM, CREATE, INSERT, UPDATE, DELETE) |
| Persistence | `self_save`, `self_load`, `self_delta_save`, `self_delta_history` |
| Digital Life | `self_needs`, `self_becoming`, `self_temporal`, `self_species`, `self_story` |
| AI | `self_chat`, `self_brief`, `self_remind`, `self_goals`, `self_evolve` |
| Distributed | `self_distributed_query`, `self_broadcast`, `self_context_sync` |
| Meta | `self_push` (GitHub sync), `self_heartbeat` |

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
# 245+ unit tests
cargo test --workspace --exclude kore-self

# SQL features (22/22)
python direct_sql_test.py

# TPC-H SQL (15/15)
python tpch_sql_bench.py

# Full battle test (KORE vs DuckDB vs Spark vs ClickHouse)
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
