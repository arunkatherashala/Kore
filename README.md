# KORE — A Pure Rust In-Memory Analytics Engine

**KORE** is a high-performance distributed analytics engine built entirely in Rust, designed to surpass Apache Spark in speed, memory efficiency, and safety.

## Architecture — 48 Layers

| Layer | Crate | Purpose |
|-------|-------|---------|
| 1–20  | `kore-core`, `kore-join`, `kore-cache`, `kore-ml2`, `kore-pipeline`, `kore-cluster`, `kore-bench`, `kore-store`, `kore-ml3`, `kore-ffi` | Core engine, joins, caching, ML, pipeline, clustering, benchmarking, storage, FFI |
| 21    | `kore-sql`      | KQL query engine — full SQL dialect (SELECT, WHERE, GROUP BY, HAVING, ORDER BY, LIMIT, UNION ALL, WITH/CTEs, JOINs, window functions, 24 scalar functions, COUNT DISTINCT, CASE WHEN, LIKE, IN, BETWEEN) |
| 22    | `kore-store`    | Columnar storage engine |
| 25    | `kore-api`      | Axum REST API server (port 8080) |
| 27    | `kore-window`   | Window functions (ROW_NUMBER, RANK, DENSE_RANK, LAG, LEAD, SUM/AVG OVER, NTILE, FIRST_VALUE, LAST_VALUE, CUMSUM) |
| 28    | `kore-io`       | CSV / NDJSON file I/O |
| 29    | `kore-shuffle`  | Distributed shuffle (hash, range, round-robin partitioning) |
| 30    | `kore-spill`    | Out-of-core execution (spill-to-disk, external sort) |
| 31    | `kore-stream`   | Structured streaming (micro-batch, tumbling/sliding windows, watermarks) |
| 32    | `kore-parquet`  | Apache Parquet file I/O |
| 33    | `kore-optimize` | Static query optimizer (constant folding, predicate pushdown, projection pruning) |
| 35    | `kore-parallel` | Rayon-powered parallel filter, aggregate, sort, group-by |
| 36    | `kore-bloom`    | Bloom filter joins (mirrors Spark DynamicBroadcastHashJoin) |
| 37    | `kore-net`      | TCP network transport protocol (length-framed JSON messages) |
| 38    | `kore-worker`   | Distributed worker node (registers, executes SQL tasks over TCP) |
| 39    | `kore-coord`    | Cluster coordinator — partitions data, dispatches tasks, two-phase aggregation |
| 40    | `kore-fault`    | Fault tolerance — lineage DAG, task retry (exponential backoff), checkpointing, speculative execution |
| 41    | `kore-aqe`      | Adaptive Query Execution — broadcast join promotion, skew detection, partition coalescing |
| 42    | `kore-simd`     | 8-wide SIMD vectorized column operations (auto-vectorized to AVX2/SSE4.2) |
| 43    | `kore-delta`    | ACID Delta Lake — transaction log, time travel (`read_at_version`), delete, vacuum |
| 44    | `kore-catalog`  | Column histograms (equi-depth), NDV, cardinality estimation, join row estimates |
| 45    | `kore-compress` | Column compression — dictionary encoding, RLE, bit-packing (auto-selected) |
| 46    | `kore-codegen`  | Compiled query predicates — column-at-a-time evaluation, fused filter+project pipeline |
| 47    | `kore-mv`       | Materialized views — full/incremental refresh, auto-invalidation |
| 48    | `kore-prune`    | Zone-map partition pruning — skip irrelevant partitions before reading |

## KORE vs Apache Spark

| Feature | Apache Spark | KORE |
|---------|-------------|------|
| Language | Scala/JVM | **Rust (zero-cost abstractions)** |
| Startup time | 10–30 seconds | **~0ms** |
| GC pauses | Yes (JVM GC) | **Never** |
| Memory overhead | ~2–4× data size | **~1× data size** |
| SQL | Full ANSI + Catalyst | KQL (70% ANSI coverage) |
| Distribution | YARN/K8s cluster | **TCP coordinator + workers** |
| Fault tolerance | RDD lineage | **Lineage DAG + retry + checkpoint** |
| Adaptive execution | Spark AQE (3.0+) | **kore-aqe** |
| Delta Lake / ACID | Delta Lake OSS | **kore-delta (built-in)** |
| Column compression | Parquet codecs | **Dict + RLE + bit-packing** |
| Compiled queries | Tungsten/whole-stage codegen | **kore-codegen (column-at-a-time)** |
| Materialized views | Via external tools | **kore-mv (built-in)** |
| Partition pruning | Dynamic partition pruning | **Zone-map pruning (kore-prune)** |
| Bloom filter joins | DynamicBroadcastHashJoin | **kore-bloom** |
| SIMD execution | Unsafe Java intrinsics | **Rust auto-vectorized (AVX2)** |
| Window functions | Full SQL | ROW_NUMBER, RANK, LAG, LEAD, SUM/AVG OVER, NTILE, CUMSUM |
| ML | MLlib | Linear/Logistic Reg, KNN, SVM, K-Means |

## Quick Start

### Build

```bash
# Debug build
cargo build

# Release (optimized)
cargo build --release
```

### Start the REST API server

```bash
./target/release/kore-api.exe
# Listening on http://127.0.0.1:8080
```

### Start a distributed cluster

```bash
# Terminal 1 — Coordinator
./target/release/kore-coord.exe 127.0.0.1:7878

# Terminal 2 — Worker 1
./target/release/kore-worker.exe 127.0.0.1:7878 worker-1

# Terminal 3 — Worker 2
./target/release/kore-worker.exe 127.0.0.1:7878 worker-2
```

### SQL via REST API

```bash
# Upload CSV
curl -X POST http://localhost:8080/api/v1/tables/sales \
  -H "Content-Type: text/csv" \
  --data-binary @sales.csv

# Query
curl -X POST http://localhost:8080/api/v1/query \
  -H "Content-Type: application/json" \
  -d '{"sql": "SELECT region, SUM(amount) AS total FROM sales GROUP BY region ORDER BY total DESC"}'
```

### KQL (KORE Query Language) — Supported SQL

```sql
-- Aggregation + HAVING
SELECT region, COUNT(DISTINCT user_id) AS users, SUM(revenue) AS total
FROM events
GROUP BY region
HAVING total > 10000
ORDER BY total DESC
LIMIT 20;

-- Window functions
SELECT user_id, amount,
  SUM(amount) OVER (PARTITION BY region ORDER BY ts) AS running_total,
  ROW_NUMBER() OVER (PARTITION BY region ORDER BY amount DESC) AS rank
FROM transactions;

-- CTEs + UNION ALL
WITH top_users AS (
  SELECT user_id, SUM(amount) AS total FROM orders GROUP BY user_id
)
SELECT * FROM top_users WHERE total > 500
UNION ALL
SELECT user_id, 0.0 AS total FROM inactive WHERE last_seen < '2025-01-01';

-- Scalar functions
SELECT UPPER(name), ROUND(price, 2), COALESCE(discount, 0.0),
       CAST(age AS VARCHAR), SUBSTR(code, 1, 3)
FROM products
WHERE price BETWEEN 10.0 AND 500.0
  AND category IN ('A', 'B', 'C')
  AND name LIKE '%Pro%';
```

### Delta Table (ACID transactions + time travel)

```rust
use kore_delta::{DeltaTable, SchemaField};

let mut table = DeltaTable::create("./my_table", vec![
    SchemaField { name: "id".into(), dtype: "INT64".into(), nullable: false },
    SchemaField { name: "value".into(), dtype: "FLOAT64".into(), nullable: true },
])?;

table.insert(data_block)?;       // version 1
table.insert(more_data)?;        // version 2

let v1 = table.read_at_version(1)?;  // time travel
let current = table.read()?;         // latest
```

### Partition Pruning

```rust
use kore_prune::{PruningEngine, PrunePred};

let mut engine = PruningEngine::new();
engine.add_partition_from_block(0, &partition_a);
engine.add_partition_from_block(1, &partition_b);

let pred = PrunePred::ColBetweenF64 { col: "price".into(), lo: 100.0, hi: 500.0 };
let read_partitions = engine.surviving_ids(&pred);
// Skip partitions where price range doesn't overlap [100, 500]
println!("Pruned {:.0}% of partitions", engine.prune_ratio(&pred) * 100.0);
```

## Test Suite

```
33 crates  ·  66 test suites  ·  0 failures
```

```bash
cargo test
```

## Project Structure

```
kore/
├── kore-core/       # Column types (Int64, Float64, Bool, Str), DataBlock, KoreError
├── kore-sql/        # KQL parser, AST, executor
├── kore-api/        # Axum REST server (binary: kore-api)
├── kore-coord/      # Cluster coordinator (binary: kore-coord)
├── kore-worker/     # Worker node (binary: kore-worker)
├── kore-delta/      # ACID delta table
├── kore-catalog/    # Column statistics + histograms
├── kore-compress/   # Column compression
├── kore-codegen/    # Compiled query predicates
├── kore-mv/         # Materialized views
├── kore-prune/      # Partition pruning
├── kore-aqe/        # Adaptive Query Execution
├── kore-fault/      # Fault tolerance
├── kore-simd/       # SIMD vectorized operations
├── kore-net/        # TCP transport protocol
├── kore-parallel/   # Rayon parallel execution
├── kore-bloom/      # Bloom filter joins
└── ...              # 16 more crates
```

## License

MIT
