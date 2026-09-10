# Getting Started with KORE Engine

**Version:** 1.8.0  
**Status:** Alpha — functional for evaluation, not yet production-hardened

---

## 1. What is KORE Engine?

KORE is a high-performance analytical SQL engine written in Rust. It processes
columnar data using vectorized SIMD execution, parallel query plans (via Rayon),
and optional distributed processing across a coordinator + worker cluster. KORE
supports a broad SQL dialect (SELECT, JOIN, GROUP BY, CTEs, window functions,
DML), reads and writes CSV, NDJSON, Parquet, and its own `.kore` format, and
exposes a Python API for interactive analytics. Think of it as a single-binary
alternative to Spark for exploratory and medium-scale analytical workloads.

**Key features:**

- Columnar storage with dictionary encoding, RLE, and bit-packing compression
- SIMD-accelerated aggregation and filtering (kore-simd)
- Parallel query execution via Rayon (kore-parallel)
- Distributed processing: coordinator + workers over TCP (kore-coord, kore-worker)
- Full SQL support: JOINs, CTEs, window functions, subqueries, DML
- Apache Parquet and Iceberg I/O
- Python API — both ctypes (py-kore) and native PyO3 (kore-pyo3)
- Cost-based and adaptive query optimization (kore-optimize, kore-aqe, kore-catalyst)
- JIT-compiled predicates (kore-codegen, kore-jit)
- TPC-H benchmark suite with Spark comparisons (kore-tpch)

---

## 2. Prerequisites

### Required

| Tool | Minimum version | Install |
|------|----------------|---------|
| Rust toolchain | 1.75+ | [rustup.rs](https://rustup.rs) |
| cargo | (bundled with rustup) | — |

### Optional (for the Python API)

| Tool | Minimum version | Install |
|------|----------------|---------|
| Python | 3.8+ | [python.org](https://www.python.org/downloads/) |
| maturin | 1.0+ | `pip install maturin` (only for PyO3 bindings) |

### System recommendations

- 8 GB RAM minimum (16 GB recommended for TPC-H benchmarks)
- Linux, macOS (Intel or Apple Silicon), or Windows

---

## 3. Building from Source

```bash
git clone <repo-url>
cd kore

# Build every crate in release mode
cargo build --release
```

This compiles the full workspace — engine core, SQL layer, I/O, distributed
components, Python bindings, benchmarks, and the demo binary. Build artifacts
land in `target/release/`.

To build only a specific crate:

```bash
cargo build --release -p kore-sql      # SQL engine only
cargo build --release -p kore-python   # Python ctypes shared library
```

---

## 4. Your First Query (Rust)

The central type is `KqlContext` from `kore-sql`. Register a `DataBlock`,
then run SQL against it.

```rust
use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::executor::KqlContext;

fn main() {
    let mut ctx = KqlContext::new();

    // Build a table from column vectors
    let sales = DataBlock::new(vec![
        Column::str_col("region", vec![
            Some("US".into()), Some("EU".into()), Some("US".into()),
            Some("AP".into()), Some("EU".into()),
        ]),
        Column::float64("amount", vec![
            Some(250.0), Some(130.0), Some(490.0),
            Some(310.0), Some(220.0),
        ]),
    ]).unwrap();

    ctx.register("sales", sales);

    let result = ctx.query(
        "SELECT region, SUM(amount) AS total, COUNT(*) AS cnt \
         FROM sales GROUP BY region ORDER BY total DESC"
    ).unwrap();

    // result is a DataBlock — iterate its columns
    println!("rows: {}", result.num_rows);
    for col in &result.columns {
        println!("  column: {}", col.name);
    }
}
```

Add these dependencies to your `Cargo.toml`:

```toml
[dependencies]
kore-core = { path = "../kore-core" }
kore-sql  = { path = "../kore-sql" }
```

---

## 5. Loading Data

### From CSV (kore-io)

```rust
use kore_io::CsvReader;

let block = CsvReader::new("sales.csv").read().unwrap();
// block is a DataBlock with auto-inferred column types
```

Tab-delimited files work too:

```rust
let block = CsvReader::new("data.tsv").delimiter(b'\t').read().unwrap();
```

### From Parquet (kore-parquet)

```rust
use kore_parquet::ParquetReader;

let block = ParquetReader::new("orders.parquet").read().unwrap();
```

Write results back to Parquet:

```rust
use kore_parquet::ParquetWriter;

ParquetWriter::write_file(&result, "output.parquet").unwrap();
```

### From NDJSON (kore-io)

```rust
use kore_io::NdJsonReader;

let block = NdJsonReader::new("events.ndjson").read().unwrap();
```

### Via SQL INSERT

```rust
use kore_sql::executor::KqlContext;

let mut ctx = KqlContext::new();
ctx.query(
    "CREATE TABLE orders (id INT, region TEXT, amount FLOAT)"
).unwrap();

ctx.query(
    "INSERT INTO orders VALUES \
     (1, 'US', 250.50), \
     (2, 'EU', 130.00), \
     (3, 'AP', 310.75)"
).unwrap();
```

---

## 6. Python API

KORE offers two Python integration paths.

### Option A: ctypes (py-kore/kore.py)

This approach loads the compiled Rust shared library via Python's `ctypes` and
provides a PySpark-like DataFrame API.

**Build the shared library first:**

```bash
cargo build --release -p kore-python
```

**Copy the library into `py-kore/`:**

```bash
# Linux
cp target/release/libkore_python.so py-kore/

# macOS
cp target/release/libkore_python.dylib py-kore/

# Windows
copy target\release\kore_python.dll py-kore\
```

**Use it:**

```python
from kore import KoreSession

spark = KoreSession()
spark.load_csv("data/orders.csv", "orders")

# PySpark-style DataFrame chain
df = (spark.table("orders")
      .filter("amount > 100")
      .select("id", "region", "amount")
      .groupBy("region")
      .agg(amount="SUM", cnt="COUNT")
      .orderBy("sum_amount", desc=True)
      .limit(10))
df.show()

# Or run raw SQL
rows = spark.sql("SELECT region, SUM(amount) FROM orders GROUP BY region")
print(rows)  # list of dicts
```

### Option B: PyO3 native bindings (kore-pyo3)

This approach builds a native Python extension module using maturin + PyO3.

**Install and build:**

```bash
pip install maturin
cd kore-pyo3
maturin develop --release
```

**Use it:**

```python
from kore_engine import KoreSession

session = KoreSession()
session.load_csv("data/orders.csv", "orders")

# SQL query — returns list of dicts
rows = session.sql("SELECT region, SUM(amount) AS total FROM orders GROUP BY region")
for row in rows:
    print(row)

# DataFrame API
df = session.table("orders").filter("amount > 200").orderBy("amount", True)
df.show()

# Export to pandas
pandas_df = df.to_pandas()
print(pandas_df)

# Write query results to Parquet
session.write_parquet(
    "SELECT * FROM orders WHERE amount > 500",
    "high_value.parquet"
)
```

---

## 7. Running the Demo

The `kore-demo` binary runs an end-to-end showcase: local SQL queries, Parquet
I/O, and a distributed cluster (coordinator + 3 workers, all in-process).

```bash
cargo run --release -p kore-demo
```

The demo generates 100,000 sample orders, runs analytical queries (GROUP BY,
CTEs, high-value filters), writes/reads Parquet, then boots a coordinator with
three workers and executes distributed GROUP BY and broadcast JOIN queries.

Expected output includes formatted result tables and timing information for
each phase.

---

## 8. Running TPC-H Benchmarks

The `kore-tpch` crate runs real TPC-H queries against generated data and
compares KORE's wall-clock times to published Apache Spark numbers.

```bash
# Scale factor 1 (~6M rows, ~800MB generated data)
cargo run --release -p kore-tpch -- --scale 1
```

Benchmarked queries include:

| Query | Description |
|-------|-------------|
| Q1 | Global aggregation (scan + GROUP BY) |
| Q3 | Hash join + GROUP BY + ORDER BY |
| Q5 | Multi-join (3+ tables) + aggregation |
| Q6 | Filter + SUM (no join, high selectivity) |
| Q10 | 4-table join + GROUP BY + ORDER BY |
| W1 | Window functions (ROW_NUMBER, running SUM) |
| S1 | Sort 6M rows by multiple columns |
| D1 | Distributed GROUP BY simulation |

Results print a comparison table showing KORE time vs. Spark baselines
(Spark 3.5 on m5.4xlarge, from published Databricks benchmarks).

---

## 9. Distributed Mode (Alpha)

KORE supports distributing queries across a coordinator and multiple worker
nodes connected over TCP. This is analogous to Spark's Driver + Executor model.

**Start the coordinator:**

```bash
cargo run --release -p kore-coord -- --port 9090
```

**Start one or more workers (in separate terminals):**

```bash
cargo run --release -p kore-worker -- --id worker-1 --master 127.0.0.1:9090
cargo run --release -p kore-worker -- --id worker-2 --master 127.0.0.1:9090
```

**Submit a query:**

```bash
cargo run --release -p kore-submit -- \
  --master 127.0.0.1:9090 \
  --data orders.parquet --table orders \
  --sql "SELECT region, SUM(amount) FROM orders GROUP BY region"
```

The coordinator partitions data across workers, each worker runs the map-side
query, and the coordinator merges partial results. The cluster supports:

- Two-phase distributed aggregation (map + reduce)
- Broadcast joins (small dimension table sent to every worker)
- DAG-based stage scheduling
- Fault tolerance with task lineage and retry (kore-fault)
- Token authentication and RBAC (kore-security)

> **Alpha warning:** Distributed mode is functional for evaluation but has not
> been hardened for production. Expect API changes. Single-node mode is the
> recommended starting point.

---

## 10. What's Next

- **DEPLOYMENT.md** — Full deployment guide, installation options, platform notes, and performance tuning: [DEPLOYMENT.md](../DEPLOYMENT.md)
- **Architecture** — The workspace has 60+ crates organized in numbered layers (see `Cargo.toml`). Key layers:
  - `kore-core` (types) → `kore-sql` (SQL engine) → `kore-optimize` / `kore-catalyst` (optimizer)
  - `kore-io` / `kore-parquet` / `kore-iceberg` (I/O)
  - `kore-coord` + `kore-worker` (distributed)
  - `kore-simd` / `kore-jit` / `kore-gpu` (acceleration)
- **Python integration** — `py-kore/kore.py` (ctypes) and `kore-pyo3/` (native PyO3)
- **REST API** — `kore-api` exposes an Axum-based HTTP/WebSocket server
- **PostgreSQL wire protocol** — `kore-server` lets standard SQL clients connect
- **MCP server** — `kore-mcp` provides AI assistant integration
- **Kafka streaming** — `kore-kafka` for streaming source/sink
- **Cloud storage** — `kore-object-store` for S3/GCS/Azure Blob

---

*KORE Engine v1.8.0 — Apache License 2.0*
