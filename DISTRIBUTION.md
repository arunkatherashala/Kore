# KORE Engine — Distribution & Packaging

## Current Distribution Method

KORE is currently distributed as **source code**. Build from source with:

```bash
cargo build --release
```

This produces these binaries in `target/release/`:

| Binary | Description |
|--------|-------------|
| `kore-coord` | Cluster coordinator — manages workers, routes queries |
| `kore-worker` | Worker node — receives and executes tasks |
| `kore-submit` | CLI job submission tool (spark-submit equivalent) |
| `kore-api` | REST API server for HTTP query submission |
| `kore-tpch` | TPC-H benchmark runner |
| `libkore_python.so` / `kore_python.dll` | Shared library for Python ctypes API |

## Cluster Deployment

### Single-Node (Development)

```bash
# Start coordinator on port 9090
./kore-coord --port 9090

# Start 2 local workers
./kore-worker --id w1 --master 127.0.0.1:9090
./kore-worker --id w2 --master 127.0.0.1:9090

# Submit a query
./kore-submit --master 127.0.0.1:9090 --sql "SELECT 1 + 1"
```

### Docker

```bash
docker build -t kore:latest .

# Coordinator
docker run -p 9090:9090 kore:latest kore-coord --port 9090

# Worker
docker run kore:latest kore-worker --id w1 --master <coord-ip>:9090
```

### Kubernetes (Helm)

```bash
helm install kore deploy/helm/kore/ \
  --set coordinator.replicas=1 \
  --set worker.replicas=4 \
  --set worker.memory=8Gi
```

## Security

Set `KORE_AUTH_REQUIRED=1` to enable token-based authentication.
Workers use `KORE_WORKER_TOKEN`, clients use `KORE_CLIENT_TOKEN`.

Issue tokens via the coordinator API:
```rust
let token = coord.issue_token("my-app", vec![Role::JobSubmitter]);
```

## Python API

The Python API uses ctypes over a C ABI shared library.
No PyPI package is available yet — build from source:

```bash
cargo build --release -p kore-python
# Copy libkore_python.so (or .dll) to py-kore/
```

```python
from kore import KoreSession

spark = KoreSession()
spark.read_parquet("data.parquet", "events")

df = (spark.table("events")
      .filter("amount > 100")
      .groupBy("region")
      .agg(amount="SUM", cnt="COUNT")
      .orderBy("sum_amount", desc=True)
      .limit(10))
df.show()
```

## Roadmap to Full Distribution

- [ ] Publish to crates.io
- [ ] PyPI wheel via maturin (PyO3 bindings)
- [ ] Docker Hub images
- [ ] Helm chart in artifact registry
- [ ] JDBC/ODBC drivers
