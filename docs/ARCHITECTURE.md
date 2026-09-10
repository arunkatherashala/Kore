# KORE Engine — Architecture

KORE is a distributed SQL analytics engine written in Rust, designed as a
from-scratch alternative to Apache Spark. It processes columnar data through a
pipeline of parsing, optimization, and parallel execution — either on a single
machine or across a cluster of worker nodes.

The workspace contains **68 crates** organized in numbered layers. This
document describes the high-level architecture and how the pieces fit together.

---

## 1. Architecture Overview

```
                        ┌──────────────┐
                        │  Client API  │  REST / PG wire / Python / MCP
                        └──────┬───────┘
                               │
                        ┌──────▼───────┐
                        │   SQL Parse  │  kore-sql (lexer → AST)
                        └──────┬───────┘
                               │
                   ┌───────────▼───────────┐
                   │   Query Optimization  │  kore-optimize, kore-catalyst
                   └───────────┬───────────┘
                               │
              ┌────────────────▼────────────────┐
              │         Execution Engine         │
              │  kore-parallel · kore-vectorized │
              │  kore-simd · kore-codegen · JIT  │
              │  kore-gpu                        │
              └────────┬──────────────┬──────────┘
                       │              │
            ┌──────────▼──┐    ┌──────▼──────────┐
            │  Single-node │    │  Distributed     │
            │  (DataBlock) │    │  kore-coord      │
            └──────────────┘    │  kore-worker     │
                                │  kore-net        │
                                │  kore-shuffle    │
                                └─────────────────┘
                                        │
                               ┌────────▼────────┐
                               │  Storage Layer   │
                               │  Parquet · Delta │
                               │  Iceberg · S3    │
                               └─────────────────┘
```

### Crate Groups

| Group | Crates | Purpose |
|---|---|---|
| **Core** | `kore-core` | `DataBlock`, `Column`, `ColumnData`, `Value`, `KoreError` — the fundamental data types shared by every other crate |
| **SQL Engine** | `kore-sql`, `kore-sql-v2` | SQL lexer, parser, AST, and row-at-a-time executor. v2 adds `DISTINCT`, `EXCEPT`, `INTERSECT`, `ROLLUP`, `CUBE` |
| **Storage** | `kore-store`, `kore-parquet`, `kore-io`, `kore-delta`, `kore-iceberg` | Binary columnar format, Apache Parquet I/O, CSV/JSON file I/O, ACID Delta Lake, Apache Iceberg table format |
| **Query Optimization** | `kore-optimize`, `kore-catalyst`, `kore-catalog`, `kore-codegen`, `kore-jit` | Rule-based optimizer, cost-based Catalyst optimizer, column statistics/histograms, compiled predicates, JIT-specialized execution |
| **Execution** | `kore-parallel`, `kore-simd`, `kore-vectorized`, `kore-gpu` | Rayon parallel execution, SIMD-accelerated column ops, batch-vectorized (1024-row) processing, GPU compute (wgpu/CUDA) |
| **Joins** | `kore-join`, `kore-sortmerge`, `kore-bloom` | Hash join, sort-merge join, broadcast join, Bloom-filter-based join pruning |
| **Window / Subquery** | `kore-window`, `kore-subquery` | SQL window functions (`ROW_NUMBER`, `RANK`, `LAG`, etc.), scalar/`IN`/`EXISTS` subquery evaluation |
| **DML** | `kore-dml`, `kore-prune`, `kore-mv` | `INSERT`/`UPDATE`/`DELETE`/`MERGE`/`CTAS`, zone-map partition pruning, materialized views with incremental refresh |
| **Distributed** | `kore-net`, `kore-coord`, `kore-worker`, `kore-shuffle`, `kore-dist-net`, `kore-node`, `kore-fault`, `kore-aqe`, `kore-rm` | TCP wire protocol, cluster coordinator, worker nodes, hash-partition shuffle, true multi-node networking, fault tolerance (lineage + retry + speculative exec), adaptive query execution, resource manager |
| **Connectors** | `kore-object-store`, `kore-hdfs`, `kore-kafka`, `kore-connect`, `kore-arrow`, `kore-flight` | S3/GCS/Azure object store, HDFS (WebHDFS REST), Kafka streaming source/sink, Arrow IPC/JSON/HTTP connectors, Arrow compact columnar format, Arrow Flight service |
| **APIs** | `kore-api`, `kore-server`, `kore-python`, `kore-pyo3`, `kore-ffi`, `kore-mcp` | Axum REST/WebSocket API, PostgreSQL wire protocol server, Python ctypes C ABI bridge, PyO3 native Python bindings, C FFI for 7+ languages, MCP server for AI assistants |
| **Security** | `kore-security` | Token authentication, RBAC (Reader/Writer/Admin/Worker), TLS configuration, audit logging, IP allowlisting |
| **ML** | `kore-ml2`, `kore-ml3`, `kore-mlx`, `kore-distml` | Random forest, gradient boosting, linear regression, KNN, SVM, logistic regression, K-means, distributed ML across workers |
| **Ops** | `kore-metrics`, `kore-deploy`, `kore-submit` | Prometheus metrics + job history, Kubernetes/standalone deployment, spark-submit-style CLI job submission |
| **Infra** | `kore-cache`, `kore-compress`, `kore-spill`, `kore-shuffle-store`, `kore-stream`, `kore-pipeline`, `kore-cluster`, `kore-bench`, `kore-tpch`, `kore-demo`, `kore-distributed` | Query caching, column compression (dict/RLE/bit-pack/LZ4), out-of-core spill, persistent shuffle store, structured streaming, pipeline execution, cluster management, benchmarking, TPC-H suite, demo binary, distributed SQL wire-up |

---

## 2. Query Execution Flow

When a SQL query reaches KORE, it passes through these stages:

### 2.1 Parse

```
SQL text  ──►  kore-sql/lexer.rs  ──►  Token stream
                                           │
                                   kore-sql/parser.rs
                                           │
                                        AST (Query)
```

The lexer tokenizes SQL into keywords, identifiers, operators, and literals.
The parser builds a typed AST with `SelectStmt`, `Expr`, `JoinClause`,
`Projection`, `OrderByItem`, etc. Supports JOINs, GROUP BY, HAVING, ORDER BY,
LIMIT, CTEs (`WITH`), and set operations.

### 2.2 Optimize

```
AST  ──►  kore-optimize::Optimizer  ──►  Optimized AST
                    │
            ┌───────┴────────┐
            │  Rule passes:  │
            │  1. ConstantFolding    (evaluate `1+1` at compile time)
            │  2. PredicatePushdown  (move WHERE before JOINs)
            │  3. ProjectionPruning  (drop unreferenced columns)
            │  4. JoinReorder        (smaller table on build side)
            │  5. LimitPushdown      (push LIMIT into scan)
            └────────────────┘
```

For advanced queries, `kore-catalyst` provides a full Catalyst-level optimizer
with cost-based planning:
- **Column histograms** from `kore-catalog` feed selectivity estimation
- **Physical planning** chooses BroadcastHashJoin vs SortMergeJoin based on
  real table statistics
- **Multi-pass convergence** — rules fire repeatedly until a fixed point

### 2.3 Execute (Single-Node)

```
Optimized AST  ──►  kore-sql/executor.rs  ──►  DataBlock result
                           │
                   KqlContext (table registry)
                           │
                    ┌───────┴────────┐
                    │  Scan table    │
                    │  Apply WHERE   │
                    │  Hash JOIN     │
                    │  GROUP BY+AGG  │
                    │  ORDER BY      │
                    │  LIMIT         │
                    └────────────────┘
```

The executor walks the AST, resolves table references from `KqlContext`,
evaluates expressions, and produces `DataBlock` results. For performance-
critical paths, execution can be routed through:

- **kore-parallel** — Rayon-powered parallel filter, sort, aggregation, group-by
- **kore-vectorized** — 1024-row SIMD batch processing (1000× fewer interpreter calls)
- **kore-codegen** — compiled predicates with column-at-a-time evaluation (4–20× faster)
- **kore-jit** — specialized GROUP BY using direct arrays instead of HashMaps
- **kore-gpu** — GPU dispatch for GROUP BY, sort, filter via wgpu/CUDA

### 2.4 Execute (Distributed)

```
SQL  ──►  Coordinator  ──►  Plan + Partition
                │
        ┌───────┼───────┐
        ▼       ▼       ▼
    Worker₀  Worker₁  Worker₂    (map phase: each runs SQL on its partition)
        │       │       │
        └───────┼───────┘
                ▼
          Shuffle / Merge            (hash-repartition by GROUP BY keys)
                │
        ┌───────┼───────┐
        ▼       ▼       ▼
    Reducer₀ Reducer₁ Reducer₂   (reduce phase: final aggregation)
                │
                ▼
          Coordinator merges         (concat partial results)
                │
                ▼
           Final DataBlock
```

The coordinator (`kore-coord`) implements a two-phase distributed execution
model similar to Spark's Driver:

1. **Map phase** — partition the data across registered workers; each worker
   runs the SQL on its local slice
2. **Shuffle** — hash-partition intermediate results by GROUP BY keys and push
   them to reducer workers (`kore-shuffle`, `kore-net`)
3. **Reduce phase** — each reducer concatenates its partitions and runs the
   final aggregation SQL
4. **Merge** — coordinator collects and concatenates all reducer outputs

The coordinator also supports **broadcast joins**: when the Catalyst planner
detects a small dimension table, it broadcasts it to all workers and runs the
join locally on each worker's partition.

---

## 3. Data Model

KORE uses a **columnar** in-memory representation, defined in `kore-core`:

### DataBlock

The fundamental unit of data — a collection of named columns with equal row
counts.

```rust
pub struct DataBlock {
    pub columns:  Vec<Column>,
    pub num_rows: usize,
}
```

Key operations: `new()`, `empty()`, `concat()`, `select_rows()`, `sort_by()`,
`column()`, `join_key()`.

### Column

A named column containing typed data:

```rust
pub struct Column {
    pub name: String,
    pub data: ColumnData,
}
```

### ColumnData

The storage backend, supporting five variants:

| Variant | Rust Type | Description |
|---|---|---|
| `Int64` | `Vec<Option<i64>>` | 64-bit integers with null support |
| `Float64` | `Vec<Option<f64>>` | 64-bit floats with null support |
| `Bool` | `Vec<Option<bool>>` | Booleans with null support |
| `Str` | `Vec<Option<String>>` | Variable-length strings |
| `StrDict` | `{ codes: Vec<u8>, dict: Vec<String> }` | Dictionary-encoded strings — 24× less memory than `Str` for low-cardinality columns (max 254 distinct values) |

### Value

Runtime value enum used for expression evaluation and cross-column operations:

```rust
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Null,
}
```

### Arrow Compact Format (kore-arrow)

For large-scale processing, `kore-arrow` provides a more memory-efficient
representation that replaces `Vec<Option<T>>` (16 bytes/value) with raw value
arrays plus 1-bit validity bitmaps (8 bytes + 1 bit per value) — **50% less
RAM** for numeric columns. This enables TPC-H SF10 (60M rows) to fit in memory.

---

## 4. Distributed Architecture

### Coordinator (`kore-coord`)

The cluster's master node. Responsibilities:

- **Worker registry** — accept `RegisterWorker` messages, track heartbeats,
  evict stale workers after 30s timeout
- **Query planning** — use `kore-catalyst` physical planner to choose join
  strategies (BroadcastHash vs SortMerge) based on catalog statistics
- **Task dispatch** — partition data, assign tasks to workers, collect results
- **Two-phase aggregation** — map → shuffle → reduce for GROUP BY queries
- **Security** — token authentication and RBAC enforcement when
  `KORE_AUTH_REQUIRED=1`
- **Metrics** — Prometheus counters, histograms, and job history via
  `kore-metrics`

### Worker (`kore-worker`)

A task execution node. Responsibilities:

- **Registration** — connect to coordinator, announce cores/memory, start
  heartbeat (every 5s)
- **Task execution** — receive SQL + data partition, execute via `KqlContext`,
  return results
- **Local table store** — hold registered tables (for broadcast joins and
  data locality)
- **Shuffle participation** — serve as mapper (hash-partition + push to
  reducers) and/or reducer (collect + aggregate)
- **Data locality** — load shards directly from S3/Parquet/CSV without going
  through the coordinator

### Network Protocol (`kore-net`)

All cluster communication uses `KoreMsg`, a tagged enum serialized over TCP:

```
Wire format:  [ 4-byte BE length ][ payload ]
Payload:      JSON (backward-compat) or bincode+LZ4 (default, ~3× faster)
Auto-detect:  readers detect format from the first byte (BINARY_MAGIC prefix)
```

Key message types:

| Message | Direction | Purpose |
|---|---|---|
| `RegisterWorker` | Worker → Coord | Announce presence |
| `Heartbeat` | Worker → Coord | Liveness + load reporting |
| `AssignTask` | Coord → Worker | Execute SQL on this data partition |
| `AssignTaskLocal` | Coord → Worker | Execute SQL on pre-registered table |
| `RegisterTable` | Coord → Worker | Broadcast a table for local join |
| `ShuffleMapTask` | Coord → Worker | Map phase of network shuffle |
| `ShufflePush` | Worker → Worker | Push partition to reducer |
| `ShuffleReduceTask` | Coord → Worker | Reduce phase after all maps complete |
| `SubmitQuery` | Client → Coord | Submit a distributed query |
| `LoadShard` | Coord → Worker | Load data from S3/disk (data locality) |

### Fault Tolerance (`kore-fault`)

Four mechanisms protect against failures:

1. **Lineage DAG** — tracks how each `DataBlock` was produced; lost data can
   be recomputed from source tables (mirrors Spark's RDD lineage)
2. **RetryScheduler** — exponential-backoff retry with configurable max
   attempts, jitter, and delay caps
3. **Checkpoint** — periodic disk snapshots to truncate long lineage chains
4. **Speculative execution** — if a task exceeds `median × threshold`
   duration, a backup task is launched on a different worker; first to finish
   wins

Partition-level lineage tracking (`TaskLineage`) records every dispatched task
so the coordinator can identify and re-dispatch partitions when a worker dies
mid-query.

---

## 5. Storage Layer

### In-Memory (`kore-core`)

The primary storage is the in-memory `DataBlock`. All query execution operates
directly on columnar `Vec<Option<T>>` arrays.

### Native Binary Format (`kore-store`)

A compact on-disk format for DataBlocks:

```
[magic:4 "KORE"]  [version:2]  [num_cols:4]  [num_rows:8]
Schema section (per column):
  [name_len:2]  [name:N]  [dtype:1]
Data section (per column):
  [compression:1]  [has_nulls:1]  [null_bitmap:ceil(n/8)]  [data_len:8]  [data:N]
```

Supports compression modes: Raw, RLE, Delta (i64), Dict, NaN-aware, LZ4.

### Parquet I/O (`kore-parquet`)

Reads and writes Apache Parquet files using the Arrow columnar API for fast,
zero-copy loading. Supports Int32/Int64, Float32/Float64, Utf8/LargeUtf8, and
Boolean column types.

### Delta Lake (`kore-delta`)

ACID transactional table format inspired by Delta Lake:

- **Append-only transaction log** — `_delta_log/v{N}.json` per commit
- **Snapshot isolation** — reads always see a consistent version
- **Time travel** — `read_at_version(v)` returns historical snapshots
- **ACID operations** — Insert, Delete (mark removed), UpdateSchema
- **Vacuum** — remove data files no longer referenced by recent versions

### Apache Iceberg (`kore-iceberg`)

Support for the Apache Iceberg table format, enabling interoperability with
existing data lake ecosystems.

### Cloud Storage (`kore-object-store`)

A unified `ObjectStore` trait abstracting:

| Provider | URI Scheme | Backend |
|---|---|---|
| Local filesystem | `/path/to/file` | `LocalStore` |
| Amazon S3 / MinIO | `s3://bucket/key` | `S3Store` (aws-sdk-s3) |
| Google Cloud Storage | `gs://bucket/key` | `GcsStore` |
| Azure Blob | `az://container/blob` | `AzureStore` |

### HDFS (`kore-hdfs`)

HDFS connector via WebHDFS REST API, enabling reads from Hadoop clusters.

---

## 6. Python Integration

KORE provides two Python integration paths:

### ctypes C ABI Bridge (`kore-python`)

A zero-dependency approach using Rust `#[no_mangle] extern "C"` functions:

```python
import ctypes, json
lib = ctypes.CDLL("./libkore_python.so")

sess = lib.kore_session_new()
lib.kore_load_csv(sess, b"orders", b"/data/orders.csv")
result = json.loads(lib.kore_query(sess, b"SELECT * FROM orders LIMIT 5"))
lib.kore_session_free(sess)
```

Also exposes a **DataFrame API** through C ABI — chainable `.filter()`,
`.select()`, `.group_by()`, `.agg()`, `.join()`, `.order_by()`, `.limit()`
methods that build SQL internally and execute on `.collect()`.

### PyO3 Native Bindings (`kore-pyo3`)

Native Python module via PyO3 (`pip install kore-engine`):

```python
from kore_engine import KoreSession

sess = KoreSession()
sess.load_csv("/data/orders.csv", "orders")
rows = sess.sql("SELECT region, SUM(sales) AS total FROM orders GROUP BY region")

df = sess.table("orders").filter("sales > 100").groupBy(["region"]).agg({"sales": "SUM"})
df.show()
pandas_df = df.to_pandas()
```

The PyO3 binding provides `KoreSession` and `KoreDataFrame` as native Python
classes with methods for SQL queries, DataFrame operations, Parquet I/O, and
direct conversion to pandas DataFrames.

---

## 7. API Surface

| Interface | Crate | Protocol |
|---|---|---|
| REST + WebSocket | `kore-api` | Axum HTTP (`/api/v1/query`, `/api/v1/tables`, `/api/v1/ml/*`) |
| PostgreSQL wire | `kore-server` | PG v3 protocol — connect with `psql`, JDBC, ODBC |
| Arrow Flight | `kore-flight` | Arrow IPC serialization + Flight service interface |
| MCP (AI tools) | `kore-mcp` | stdio/HTTP MCP server — exposes `kore_query`, `kore_load_csv`, `kore_schema`, etc. |
| C FFI | `kore-ffi` | C ABI for binding from C, Go, Java, Ruby, Swift, etc. |

---

## 8. Crate Dependency Diagram

```
                              kore-core
                    ┌────────────┼────────────────────────────┐
                    │            │                            │
                kore-sql    kore-store                   kore-join
                    │            │                       ┌───┘
           ┌───────┴──────┐     │                kore-sortmerge
           │              │     │                kore-bloom
     kore-optimize   kore-sql-v2│
           │              │     │
     kore-catalyst        │   kore-parquet
           │              │     │
     kore-catalog         │   kore-io
           │              │     │
           └──────┬───────┘   kore-compress
                  │             │
           kore-codegen    kore-delta
           kore-jit        kore-iceberg
                  │
           ┌──────┴──────┐
           │             │
     kore-parallel   kore-simd
     kore-vectorized kore-gpu
           │
     ┌─────┴──────────────────────────────┐
     │                                    │
  kore-net ◄─────────────────────── kore-arrow
     │                                    │
     ├── kore-coord ◄── kore-catalyst     │
     │        │     ◄── kore-catalog    kore-flight
     │        │     ◄── kore-fault
     │        │     ◄── kore-metrics
     │        │     ◄── kore-security
     │        │
     ├── kore-worker ◄── kore-shuffle
     │        │       ◄── kore-object-store
     │        │
     ├── kore-dist-net
     ├── kore-node
     ├── kore-rm
     │
     └── kore-aqe (Adaptive Query Execution)
           │
     ┌─────┴──────┐
     │            │
  kore-api    kore-server (PG wire)
  kore-mcp    kore-python
              kore-pyo3
              kore-ffi
```

### Key Dependency Relationships

- **Everything depends on `kore-core`** — it defines `DataBlock`, `Column`,
  `Value`, and `KoreError`
- **`kore-sql`** is the second most depended-on crate — the executor's
  `KqlContext` is used by workers, coordinators, and all API surfaces
- **`kore-net`** defines the wire protocol (`KoreMsg`, `KoreFrame`) used by
  `kore-coord`, `kore-worker`, and `kore-dist-net`
- **`kore-catalyst`** depends on `kore-catalog` for statistics and
  `kore-sql` for AST types; it feeds physical plans to `kore-coord`
- **`kore-fault`** is used by `kore-coord` for lineage tracking and retry
  scheduling
- **`kore-object-store`** is used by `kore-worker` for data-locality shard
  loading and by `kore-coord` for cloud table registration

---

## 9. Configuration

Key environment variables that control cluster behavior:

| Variable | Default | Purpose |
|---|---|---|
| `KORE_COORD_BIND` | `127.0.0.1:7878` | Coordinator listen address |
| `KORE_WORKER_BIND` | `0.0.0.0:0` | Worker task listener bind |
| `KORE_WORKER_ADVERTISE` | `127.0.0.1` | Address workers advertise to coordinator |
| `KORE_CLUSTER_LOCAL` | `1` (ON) | Use worker-local tables (register once, SQL-only tasks) |
| `KORE_NET_SHUFFLE` | `0` (OFF) | Enable true worker↔worker network shuffle |
| `KORE_WIRE` | `binary` | Wire format: `binary` (bincode+LZ4), `binary-raw`, or `json` |
| `KORE_AUTH_REQUIRED` | `0` (OFF) | Enforce token authentication for all cluster operations |
| `KORE_CLIENT_TOKEN` | — | Bearer token for client → coordinator auth |
| `KORE_WORKER_TOKEN` | — | Bearer token for worker → coordinator auth |
