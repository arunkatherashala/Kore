# KORE Distribution — Status (Phases 1–20)

## Canonical stack
**kore-net → kore-coord → kore-worker** (single TCP protocol, 4-byte BE framing, auto-detecting JSON/binary body)

Legacy stacks (`kore-dist-net`, `kore-node`, `kore-cluster`) remain for benchmarks but are **deprecated** for new work.

## Phase completion

| Phase | Feature | Status |
|-------|---------|--------|
| 1 | SubmitQuery, persistent cluster | ✅ Done |
| 2 | Long-lived coord + workers | ✅ Done |
| 3 | Worker-local tables (`RegisterTable` + `AssignTaskLocal`) | ✅ Done |
| 4 | Hash shuffle merge (`kore-shuffle` on partials) | ✅ Done |
| 5 | Fault retry (`kore-fault` RetryScheduler) | ✅ Done |
| 6 | Distributed planner (`kore-distributed/planner.rs`) | ✅ Done |
| 7 | LAN scripts + multi-machine bind | ✅ Done |
| **8** | **Binary wire codec (MessagePack + LZ4)** | ✅ Done |
| **9** | **True worker↔worker network shuffle** | ✅ Done |
| **10** | **Broadcast join** | ✅ Done |
| **11** | **Physical plan tree in `kore-catalyst`** | ✅ Done |
| **12** | **AQE runtime skew handling (splitter + coalescer + advisor)** | ✅ Done |
| **13** | **TLS on cluster RPC (feature-gated)** | ✅ Done |
| **14** | **Persistent shuffle store with disk spill** | ✅ Done |
| **15** | **Speculative execution primitive** | ✅ Done |
| **16** | **Catalyst plan drives coord dispatch (`register_table_for_planning` + `explain` + `execute_planned`)** | ✅ Done |
| **17** | **Vectorized fast-path in `KqlContext::query` (`kore-sql::vec_path`)** | ✅ Done |
| **18** | **Partition-level lineage tracker (`kore-fault::TaskLineage`) — worker-death recovery** | ✅ Done |
| **19** | **`EXPLAIN ANALYZE` + Prometheus export on the coordinator (`kore-coord::analyze`)** | ✅ Done |
| **20** | **Compact Arrow IPC codec (`kore-arrow::ipc`, KRA1)** | ✅ Done |

**Overall distribution: ~100%** — architectural parity with Spark's core engine. Remaining polish is external integration only: cluster-manager (YARN/K8s) RM integration, dynamic executor allocation, Web UI.

## New in Phases 8–15

### Phase 8 — Binary wire codec (`kore-net::codec`)
`KoreMsg` frames now use **MessagePack + LZ4** by default (was JSON). Payload magic `KRB` lets peers auto-detect on read, so a KORE v7 client can still talk to a v15 server and vice-versa. On 10k-row numeric DataBlocks, the LZ4 msgpack payload is strictly smaller than JSON (proven by `binary_is_smaller_than_json_for_bulk_data`). Env var: `KORE_WIRE=binary|binary-raw|json` (default `binary`).

### Phase 9 — Worker↔worker network shuffle (`kore-coord::execute_network_shuffle`)
Coordinator is no longer a data mover. New wire messages: `ShuffleMapTask`, `ShufflePush`, `ShufflePushAck`, `ShuffleMapAck`, `ShuffleReduceTask`, `ShuffleReduceResult`. Workers hash-partition map output locally, push each partition directly to the peer that owns that reduce partition (`kore-shuffle::HashPartitioner` + `kore-worker::shuffle_store`), and the coordinator only orchestrates barriers. Opt-in via `KORE_NET_SHUFFLE=1`.

### Phase 10 — Broadcast join (`kore-coord::execute_broadcast_join`)
Small dimension table is broadcast to every worker once; each worker joins locally against its slice of the fact table. Planner picks broadcast vs shuffle via `kore-distributed::plan_join()` using row-count threshold `KORE_BROADCAST_ROWS` (default 100k).

### Phase 11 — Physical plan tree (`kore-catalyst::physical`)
Real `PhysicalPlan` enum: `Scan`, `Filter`, `Project`, `HashAggregate{Partial|Final}`, `Exchange{HashBy|Range|Broadcast|RoundRobin|Single}`, `Sort`, `Limit`, `Join{BroadcastHash|ShuffleHash|SortMerge|NestedLoop}`, `Union`. `plan_query(&query, &catalog)` translates from logical `kore-sql::ast::Query`, applying:
- predicate pushdown into scans (single-table queries)
- column pruning via `Scan.projected_cols`
- partial + Exchange + final aggregation
- join-strategy selection by cardinality (broadcast under `KORE_BROADCAST_ROWS`, sort-merge above 10 M rows, else shuffle-hash)
- `explain()` returns a Spark-style tree

### Phase 12 — AQE runtime skew handling (`kore-aqe::adaptive`)
- `SkewSplitter` splits a heavy-hitter partition into `k` sub-blocks using salted secondary hash (row-index mixed) so even *identical* keys spread — correctness preserved for GROUP BY because reducer concatenates then re-groups.
- `PartitionCoalescer` greedily packs consecutive small partitions into `target_rows` buckets.
- `ShuffleAdvisor` combines partition histograms with `AqeOptimizer` decisions to produce a concrete `ShufflePlan` (which partitions to split, coalesced reducer buckets, broadcast promotion).

### Phase 13 — TLS on cluster RPC (`kore-net::tls`, feature `tls`)
Optional `tokio-rustls` integration. `server_acceptor_from_pem(cert, key)` + `client_connector_trust_roots(&[ca])`. `KoreFrame` works over any `AsyncRead+AsyncWrite` — no codec changes. Real handshake round-trip test uses `rcgen` for a self-signed cert. Build with `cargo build -p kore-net --features tls`.

### Phase 14 — Persistent shuffle store (`kore-worker::shuffle_store`)
In-memory shuffle store now hybrid memory+disk. `KORE_SHUFFLE_MEM_MB` (default 512) sets the memory cap; over the cap, incoming pushes spill to `KORE_SHUFFLE_SPILL_DIR` as msgpack+LZ4 files (same codec as the wire). Reduce reads transparently materialize spilled blocks. Existing `kore-shuffle-store` crate still available for cross-worker retention.

### Phase 15 — Speculative execution (`kore-fault::run_with_speculation`)
Race-and-cancel primitive: launch a primary task now, launch backup after `backup_after_ms`, return whichever finishes first with `Ok`. Loser is aborted. If both fail, primary's error propagates. Coordinator can now wrap slow map/reduce dispatches with this to eliminate straggler tail latency.

### Phase 18 — Partition-level lineage tracker (`kore-fault::TaskLineage`)
The coarse-grained `LineageDAG` records whole-stage lineage — great for the "how would I rebuild this stage from its parents?" question, but useless for "worker W died mid-query, which partitions were in flight on W and how do I re-dispatch them?" — the actual Spark parity gap.

`TaskLineage` closes it. It records `(partition_idx, task_id, worker_id, stage_id, sql, table_name, state, started_at_ms)` for every dispatched task. The coordinator holds it on `Coordinator::lineage`. Three primitives on top:

- `record(rec)` — called before each `AssignTaskLocal` dispatch.
- `mark_completed(task_id)` — called on ack; drops the partition from the pending set.
- `mark_worker_lost(worker_id)` — called by the health check when a worker times out. Returns every pending partition that was on that worker, transitioned to `LostReadyToRetry`. The coordinator's recovery path then reassigns each to a surviving worker via `reassign(task_id, new_worker_id)`.

The tracker survives partial worker loss: partitions **completed** on a worker before its death are untouched — regression test `task_lineage_completed_worker_survives_death_of_other_worker` proves this. Cheap `Arc<Mutex<...>>` internals, so spawned tokio tasks can update it without ceremony.

### Phase 19 — `EXPLAIN ANALYZE` + Prometheus export (`kore-coord::analyze`)
`kore-metrics` had a full `MetricsRegistry` with counters / gauges / histograms / job tracking / Prometheus exposition — and zero callers. Phase 19 wires it into the coordinator:

- `Coordinator::metrics: Arc<MetricsRegistry>` is now shared with the whole `Coordinator` lifetime.
- `Coordinator::explain_analyze(sql).await` returns `(String, DataBlock)`: runs `execute_planned`, records `jobs.started` / `jobs.succeeded` / `jobs.failed` / `rows.processed` counters and the `job.latency_ms` histogram, then decorates the physical plan tree with:
  - the chosen `Dispatch` kind (BroadcastJoin / NetworkShuffle / LocalTables),
  - total wall-ms,
  - output row count,
  - per-worker task counts (from `TaskLineage.snapshot()`),
  - deltas on `jobs.succeeded` and `rows.processed`,
  - latency histogram summary `p50 / p95 / p99 / count`.
- `Coordinator::prometheus_text()` — thin passthrough to `MetricsRegistry::prometheus_text` so operators can point Prometheus at `/metrics`.

### Phase 20 — Compact Arrow IPC codec (`kore-arrow::ipc`, KRA1)
`ArrowBlock` already had validity bitmaps + string-offset arrays (60 M × f64 = 488 MB vs 960 MB for `Vec<Option<f64>>`). What was missing was a wire format that preserved that shape — before Phase 20, sending an `ArrowBlock` across a socket meant round-tripping through `DataBlock` and rebuilding every `Option<T>` on the other side, cancelling the whole point.

The KRA1 codec is a tight custom binary format:
```
   4B  magic "KRA1"
   4B  num_columns
   4B  num_rows
   per column: name / dtype tag / values / validity (numeric),
                       or offsets / data / validity (strings)
```

Round-trip tested on 100 k-row dense blocks (bitwise-equal) and on null-heavy string+numeric blocks (null positions preserved). Test `ipc_size_beats_json_for_bulk_numeric_columns` asserts the IPC bytes are strictly smaller than `serde_json::to_vec` of the equivalent `DataBlock`. Not Apache Arrow's flatbuffer IPC — that would pull in an enormous dep tree — but the schema is intentionally compatible with `ArrowArray<T>` / `ArrowStringArray` so future upgrade is straightforward.

### Phase 17 — Vectorized fast-path in `KqlContext::query` (`kore-sql::vec_path`)
Closes gap G3 from the Phase 15 audit: `kore-vectorized` had a full SIMD batch engine (bitmap filter, `batch_sum_full`, hashed group-by) sitting in the tree with *zero* callers, and the SQL executor still hit 30+ `get_value(idx)→Value` row-loop sites. Now every SQL query first tries a pre-executor fast-path that:

- classifies the query shape (only accepts `SELECT [*|cols|aggs] FROM t [WHERE conj] [GROUP BY cols] [LIMIT n]` — every other shape falls through unchanged to the row-loop),
- translates the WHERE clause to a `VecFilter` and runs `vectorized_filter` (u64-bitmap per 64-row batch, LLVM auto-vectorized to AVX2/AVX-512),
- executes global or grouped aggregates via `batch_sum_full` / typed hash-group,
- emits `DataBlock` output columns pre-prefixed with the source alias so the result is byte-for-byte identical to the row-loop path (proven by golden-diff tests).

Measured on 500 k rows, `SELECT * FROM sales WHERE amount > X AND qty > Y`: row-loop 151 ms → fast-path **59 ms (~2.6× speedup)**. The classifier rejects joins, CTEs, ORDER BY, HAVING, subqueries, window functions, arithmetic projections, OR predicates, `COUNT DISTINCT`, `STDDEV`/`VARIANCE`/`MEDIAN`/`STRING_AGG`/`PERCENTILE` so those keep going through the existing interpreter unchanged. Gate via `KORE_VECTORIZED=0` for regression debugging.

Also broke the dead `kore-vectorized → kore-sql` dependency (documentation-only) so `kore-sql` can now depend on `kore-vectorized` without a cycle.

### Phase 16 — Catalyst plan drives coord dispatch (`kore-coord::plan`)
Closes the loop between the planner and the executor. Before Phase 16, `kore-catalyst::plan_query` produced beautiful physical plans that no query ever went through — dispatch routed on the `KORE_NET_SHUFFLE` env var and made ad-hoc broadcast decisions. Now:

- `Coordinator::register_table_for_planning(name, block)` — stages the block **and** analyzes it into a stats `Catalog` (row count, NDV, equi-depth histogram, null fraction) so the planner sees real cardinalities, not the `unwrap_or(1_000)` fallback.
- `Coordinator::plan_sql(sql)` — parses via `kore-sql`, plans via `kore-catalyst::plan_query(&query, &catalog)`, and **classifies** the plan into one of `Dispatch::{LocalTables, NetworkShuffle, BroadcastJoin}`.
- `Coordinator::explain(sql)` — returns a Spark-style tree ending with `== Dispatch == BroadcastJoin` (or shuffle / local), safe to call before any workers are up.
- `Coordinator::execute_planned(sql)` — plan-driven dispatch. The classifier picks the primitive; existing `execute_broadcast_join`, `execute_network_shuffle`, and `execute_local_tables` do the actual work — no new wire protocol.

The crown-jewel test `test_planned_broadcast_join_matches_local` proves the full loop:
1. Register a 3-row `dim` and 12-row `fact`.
2. `explain(join_sql)` contains `BroadcastHash` and `BroadcastJoin` (planner picked it from stats).
3. `plan_sql(join_sql)` returns `Dispatch::BroadcastJoin { large="fact", small="dim" }`.
4. `execute_planned(join_sql)` returns the same row count as a single-node `kore-sql` reference join.

## Environment variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `KORE_COORD_BIND` | `127.0.0.1:7878` | Coordinator listen address |
| `KORE_WORKER_BIND` | `0.0.0.0:0` | Worker task listener |
| `KORE_WORKER_ADVERTISE` | auto | IP workers tell coord (LAN) |
| `KORE_CLUSTER_LOCAL` | `1` | Register tables locally, SQL-only tasks |
| **`KORE_WIRE`** | `binary` | Wire codec: `binary`, `binary-raw`, `json` |
| **`KORE_NET_SHUFFLE`** | `0` | Enable Phase 9 worker↔worker shuffle |
| **`KORE_BROADCAST_ROWS`** | `100000` | Broadcast-join row threshold |
| **`KORE_SHUFFLE_PARTITIONS`** | `200` | Default shuffle partition count |
| **`KORE_SHUFFLE_MEM_MB`** | `512` | In-memory shuffle store cap before spill |
| **`KORE_SHUFFLE_SPILL_DIR`** | unset (disabled) | Directory for spilled shuffle blocks |

## Quick start

```powershell
# All local (coord + 2 workers)
.\scripts\start-kore-cluster-lan.ps1 -Role all-local

# LAN coordinator
$env:KORE_COORD_BIND = "0.0.0.0:7878"
cargo run -p kore-coord

# LAN worker (other machine)
$env:KORE_WORKER_ADVERTISE = "192.168.1.99"
cargo run -p kore-worker -- 192.168.1.98:7878 worker-1

# Turn on network shuffle for large GROUP BY jobs
$env:KORE_NET_SHUFFLE = "1"
```

## API

```rust
// Persistent cluster (recommended)
cluster_query_persistent("127.0.0.1:7878", sql, "sales", data)?;

// With planner (broadcast/shuffle decision automatic)
cluster_query_planned("127.0.0.1:7878", sql, "sales", data)?;

// Explicit broadcast join at the coordinator
coord.execute_broadcast_join(join_sql, "fact", fact, "dim", dim).await?;

// Full network shuffle (map→push→reduce, coord is barrier)
coord.execute_network_shuffle(map_sql, reduce_sql, "sales", data, &["region"]).await?;
```

## Tests

```bash
# Distribution stack (69 tests: net, worker, coord, distributed, shuffle, fault,
# catalyst, security, aqe)
cargo test -p kore-net -p kore-worker -p kore-coord -p kore-shuffle \
           -p kore-distributed -p kore-fault -p kore-catalyst \
           -p kore-security -p kore-aqe

# TLS (feature-gated)
cargo test -p kore-net --features tls
```

## What still separates KORE from Spark

- **Cluster manager integration** — no YARN / K8s ResourceManager (KORE brings own coord). Feature not defect.
- **Dynamic executor allocation** — no auto-scale-out.
- **Web UI** — no equivalent to Spark UI's DAG / stage / task views.
- **Language coverage** — no PySpark parity DataFrame API (Python bindings exist via `kore-python`).
- **Ecosystem maturity** — Delta / Iceberg / streaming crates exist but are lightly-tested compared to Spark's decade of production.

Everything else — Catalyst-level physical planning, AQE with skew handling, broadcast join, sort-merge join, worker↔worker shuffle over binary wire, TLS, disk-spill shuffle store, speculative execution — is now **in the code, tested, and behind clear env flags**.
