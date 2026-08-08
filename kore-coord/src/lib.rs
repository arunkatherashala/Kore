//! KORE Layer 39 — Cluster Coordinator
//!
//! The coordinator is the master node that:
//!  1. Accepts worker registrations on its registration port.
//!  2. Accepts distributed query requests from clients.
//!  3. Partitions data and dispatches `AssignTask` to registered workers.
//!  4. Collects `TaskResult` responses and merges them.
//!  5. Implements two-phase distributed aggregation (map → shuffle → reduce).
//!
//! This mirrors Apache Spark's Driver + ClusterManager roles.

mod analyze;
mod exec;
mod plan;

pub use plan::Dispatch;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::net::{TcpListener, TcpStream};
use kore_catalog::Catalog;
use kore_core::{Column, ColumnData, DataBlock, KoreError};
use kore_fault::TaskLineage;
use kore_metrics::MetricsRegistry;
use kore_net::{KoreFrame, KoreMsg, TaskStats, partition_block, now_ms};
use kore_sql::executor::{KqlContext, execute};

// ─── Worker registry ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct WorkerInfo {
    id:           String,
    task_addr:    String,
    cores:        usize,
    memory_mb:    usize,
    last_seen:    u64,
    active_tasks: usize,
}

// ─── Coordinator ──────────────────────────────────────────────────────────────

/// A table registered on the coordinator: the block itself (for planning /
/// dispatch) and any catalog metadata already analyzed. The coordinator keeps
/// blocks in memory only until dispatch — after `execute_planned` returns,
/// the caller decides whether to drop them.
#[derive(Debug, Clone)]
pub struct RegisteredTable {
    pub name:  String,
    pub block: DataBlock,
}

#[derive(Clone)]
pub struct Coordinator {
    workers: Arc<Mutex<Vec<WorkerInfo>>>,
    /// Statistics catalog consulted by `kore_catalyst::plan_query` — populated
    /// by `analyze_table` and read (never mutated) during planning.
    catalog: Arc<Mutex<Catalog>>,
    /// Blocks the coordinator remembers for plan-driven dispatch.  Keyed by
    /// table name.  Populated by `register_table_for_planning` and consumed
    /// by `execute_planned`.
    registered: Arc<Mutex<HashMap<String, DataBlock>>>,
    /// Phase 18: partition-level lineage tracker — records every dispatched
    /// task's `(partition_idx, task_id, worker_id, sql, table)` so the coord
    /// can retry lost partitions when a worker dies mid-query.
    pub lineage: TaskLineage,
    /// Phase 19: metrics registry — counters (jobs.*), histograms
    /// (job.latency_ms), and job records feed EXPLAIN ANALYZE and the
    /// Prometheus exposition endpoint.
    pub metrics: Arc<MetricsRegistry>,
}

impl Coordinator {
    pub fn new() -> Self {
        Self {
            workers:    Arc::new(Mutex::new(Vec::new())),
            catalog:    Arc::new(Mutex::new(Catalog::new())),
            registered: Arc::new(Mutex::new(HashMap::new())),
            lineage:    TaskLineage::new(),
            metrics:    MetricsRegistry::new(),
        }
    }

    /// Number of currently registered workers.
    pub fn worker_count(&self) -> usize {
        self.workers.lock().unwrap().len()
    }

    /// Register a table for plan-driven dispatch: stores the block in the
    /// coordinator's staging map and analyzes it into the stats catalog so
    /// `plan_query` sees real row counts / NDV / histograms.
    ///
    /// This is the counterpart of Spark's `df.createOrReplaceTempView` +
    /// `ANALYZE TABLE ... COMPUTE STATISTICS` in a single call.
    pub fn register_table_for_planning(&self, name: &str, block: DataBlock) {
        self.catalog.lock().unwrap().analyze(name, &block);
        self.registered.lock().unwrap().insert(name.to_string(), block);
    }

    /// Row counts of every table analyzed by `register_table_for_planning`,
    /// smallest first.  Callers use this to inspect what the catalyst planner
    /// will see (useful in tests and EXPLAIN output).
    pub fn catalog_sizes(&self) -> Vec<(String, usize)> {
        self.catalog
            .lock()
            .unwrap()
            .tables_by_size()
            .into_iter()
            .map(|(n, r)| (n.to_string(), r))
            .collect()
    }

    /// Access the registered block for a table (returns `None` if not staged).
    pub(crate) fn take_registered(&self, name: &str) -> Option<DataBlock> {
        self.registered.lock().unwrap().remove(name)
    }

    pub(crate) fn peek_registered(&self, name: &str) -> Option<DataBlock> {
        self.registered.lock().unwrap().get(name).cloned()
    }

    /// Run the coordinator: listen for worker registrations, heartbeats, and client queries.
    pub async fn run(&self, listener: TcpListener) {
        let workers = self.workers.clone();
        let coord = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                ticker.tick().await;
                coord.evict_stale_workers(30_000);
            }
        });
        loop {
            if let Ok((mut stream, peer)) = listener.accept().await {
                let w = workers.clone();
                let c = self.clone();
                tokio::spawn(async move {
                    match KoreFrame::read(&mut stream).await {
                        Ok(KoreMsg::RegisterWorker { id, task_addr, cores, memory_mb }) => {
                            eprintln!("[coord] worker registered: {id}  addr={task_addr} peer={peer}");
                            let info = WorkerInfo {
                                id: id.clone(),
                                task_addr,
                                cores,
                                memory_mb,
                                last_seen: now_ms(),
                                active_tasks: 0,
                            };
                            w.lock().unwrap().push(info);
                            let _ = KoreFrame::write(&mut stream, &KoreMsg::RegisterAck { worker_id: id }).await;
                        }
                        Ok(KoreMsg::Heartbeat {
                            worker_id,
                            timestamp_ms,
                            active_tasks,
                            ..
                        }) => {
                            let mut ws = w.lock().unwrap();
                            if let Some(info) = ws.iter_mut().find(|wi| wi.id == worker_id) {
                                info.last_seen = timestamp_ms;
                                info.active_tasks = active_tasks;
                            }
                        }
                        Ok(KoreMsg::SubmitQuery {
                            query_id,
                            sql,
                            table_name,
                            data,
                            reduce_sql,
                            local_tables,
                        }) => {
                            eprintln!("[coord] query {query_id} from {peer} local={local_tables}");
                            let reduce = reduce_sql.as_deref();
                            let use_local =
                                local_tables || kore_net::cluster_local_tables();
                            let resp = if use_local {
                                match c
                                    .execute_distributed_v2(&sql, &table_name, data, reduce)
                                    .await
                                {
                                    Ok(result) => KoreMsg::QueryResult { query_id, result },
                                    Err(e) => KoreMsg::QueryError {
                                        query_id,
                                        message: e.to_string(),
                                    },
                                }
                            } else {
                                match c
                                    .execute_distributed(&sql, &table_name, data, reduce)
                                    .await
                                {
                                    Ok(result) => KoreMsg::QueryResult { query_id, result },
                                    Err(e) => KoreMsg::QueryError {
                                        query_id,
                                        message: e.to_string(),
                                    },
                                }
                            };
                            let _ = KoreFrame::write(&mut stream, &resp).await;
                        }
                        Ok(KoreMsg::Ping) => { let _ = KoreFrame::write(&mut stream, &KoreMsg::Pong).await; }
                        Ok(other) => {
                            eprintln!("[coord] unexpected message from {peer}: {:?}", other);
                        }
                        Err(e) => eprintln!("[coord] read error from {peer}: {e}"),
                    }
                });
            }
        }
    }

    /// Submit a query to a **running** coordinator over TCP (persistent cluster mode).
    pub async fn submit_query(
        coord_addr: &str,
        sql: &str,
        table_name: &str,
        data: DataBlock,
        reduce_sql: Option<&str>,
    ) -> Result<DataBlock, KoreError> {
        let query_id = format!("q-{}", now_ms());
        let mut stream = TcpStream::connect(coord_addr).await
            .map_err(|e| KoreError::InvalidArgument(format!("connect coord: {e}")))?;
        KoreFrame::write(&mut stream, &KoreMsg::SubmitQuery {
            query_id: query_id.clone(),
            sql: sql.to_string(),
            table_name: table_name.to_string(),
            data,
            reduce_sql: reduce_sql.map(|s| s.to_string()),
            local_tables: kore_net::cluster_local_tables(),
        }).await
            .map_err(|e| KoreError::InvalidArgument(format!("write query: {e}")))?;
        match KoreFrame::read(&mut stream).await
            .map_err(|e| KoreError::InvalidArgument(format!("read result: {e}")))? {
            KoreMsg::QueryResult { result, .. } => Ok(result),
            KoreMsg::QueryError { message, .. } => Err(KoreError::InvalidArgument(message)),
            other => Err(KoreError::InvalidArgument(format!("unexpected: {:?}", other))),
        }
    }

    // ─── Distributed query execution ─────────────────────────────────────────

    /// Execute `sql` over `data` spread across all registered workers.
    ///
    /// # Two-phase distributed execution
    /// 1. **Map phase** — each worker runs `sql` on its partition.
    /// 2. **Reduce phase** — coordinator merges all partial results with a
    ///    final `SELECT … GROUP BY …` (if the original query aggregates).
    pub async fn execute_distributed(
        &self,
        sql: &str,
        table_name: &str,
        data: DataBlock,
        reduce_sql: Option<&str>,  // e.g. "SELECT region, SUM(total) AS total FROM merged GROUP BY region"
    ) -> Result<DataBlock, KoreError> {
        let workers = self.workers.lock().unwrap().clone();
        if workers.is_empty() {
            return Err(KoreError::InvalidArgument("no workers registered".into()));
        }

        let n = workers.len();
        let partitions = partition_block(data, n);

        // Dispatch tasks in parallel
        let mut handles = Vec::new();
        for (i, partition) in partitions.into_iter().enumerate() {
            let worker    = workers[i % n].clone();
            let sql_copy  = sql.to_string();
            let tname     = table_name.to_string();
            let task_id   = format!("task-stage0-part{i}");

            handles.push(tokio::spawn(async move {
                send_task(&worker.task_addr, &task_id, 0, i, &sql_copy, &tname, partition).await
            }));
        }

        // Collect partial results
        let mut partials: Vec<DataBlock> = Vec::new();
        for h in handles {
            let result = h.await
                .map_err(|e| KoreError::InvalidArgument(format!("task panic: {e}")))?
                .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;
            partials.push(result);
        }

        // Merge all partials
        let merged = DataBlock::concat(partials)?;

        // Optional reduce phase (for distributed aggregation)
        if let Some(rsql) = reduce_sql {
            let mut ctx = KqlContext::new();
            ctx.register("merged", merged);
            return ctx.query(rsql);
        }

        Ok(merged)
    }

    /// Remove workers that haven't sent a heartbeat in `timeout_ms`.
    pub fn evict_stale_workers(&self, timeout_ms: u64) {
        let now = now_ms();
        let mut ws = self.workers.lock().unwrap();
        ws.retain(|w| now.saturating_sub(w.last_seen) < timeout_ms);
    }
}

impl Default for Coordinator { fn default() -> Self { Self::new() } }

// ─── Task dispatch helper ─────────────────────────────────────────────────────

async fn send_task(
    addr: &str,
    task_id: &str,
    stage_id: usize,
    partition_id: usize,
    sql: &str,
    table_name: &str,
    data: DataBlock,
) -> Result<DataBlock, std::io::Error> {
    let mut conn = TcpStream::connect(addr).await?;
    KoreFrame::write(&mut conn, &KoreMsg::AssignTask {
        task_id:      task_id.to_string(),
        stage_id,
        partition_id,
        sql:          sql.to_string(),
        table_name:   table_name.to_string(),
        data,
    }).await?;

    match KoreFrame::read(&mut conn).await? {
        KoreMsg::TaskResult { result, .. } => Ok(result),
        KoreMsg::TaskError  { message, .. } =>
            Err(std::io::Error::new(std::io::ErrorKind::Other, message)),
        other =>
            Err(std::io::Error::new(std::io::ErrorKind::InvalidData,
                format!("unexpected: {:?}", other))),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio::time::{sleep, Duration};
    use kore_core::{Column, ColumnData, DataBlock};

    async fn start_worker(coord_addr: String) {
        let w = kore_worker::Worker::new(format!("w-{}", rand_id()));
        let _ = w.run(&coord_addr).await;
    }

    fn rand_id() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos() as u64
    }

    fn sales_data() -> DataBlock {
        DataBlock {
            num_rows: 9,
            columns: vec![
                Column { name: "region".into(), data: ColumnData::Str(vec![
                    Some("EU".into()), Some("US".into()), Some("EU".into()),
                    Some("US".into()), Some("AP".into()), Some("EU".into()),
                    Some("US".into()), Some("AP".into()), Some("AP".into()),
                ]) },
                Column { name: "sales".into(), data: ColumnData::Float64(vec![
                    Some(100.0), Some(200.0), Some(150.0),
                    Some(300.0), Some(120.0), Some(80.0),
                    Some(250.0), Some(90.0),  Some(110.0),
                ]) },
            ],
        }
    }

    #[tokio::test]
    async fn test_distributed_filter() {
        // Start coordinator
        let coord_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let coord_addr = coord_listener.local_addr().unwrap().to_string();
        let coord = Arc::new(Coordinator::new());
        let c2 = coord.clone();
        tokio::spawn(async move { c2.run(coord_listener).await });

        // Start 2 workers
        for _ in 0..2 {
            let ca = coord_addr.clone();
            tokio::spawn(start_worker(ca));
        }

        // Wait for both workers to register
        for _ in 0..20 {
            if coord.worker_count() >= 2 { break; }
            sleep(Duration::from_millis(50)).await;
        }
        assert!(coord.worker_count() >= 1, "no workers registered");

        let data = sales_data();

        // Distributed filter: sales > 150
        let result = coord.execute_distributed(
            "SELECT * FROM sales WHERE sales > 150",
            "sales",
            data,
            None,
        ).await.unwrap();

        // EU:100 EU:150 EU:80 AP:120 AP:90 AP:110  ← filtered out
        // US:200 US:300 US:250  ← kept (3 rows)
        assert!(result.num_rows >= 3, "expected ≥3 rows, got {}", result.num_rows);
    }

    #[tokio::test]
    async fn test_distributed_aggregation() {
        let coord_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let coord_addr = coord_listener.local_addr().unwrap().to_string();
        let coord = Arc::new(Coordinator::new());
        let c2 = coord.clone();
        tokio::spawn(async move { c2.run(coord_listener).await });

        for _ in 0..3 {
            let ca = coord_addr.clone();
            tokio::spawn(start_worker(ca));
        }

        for _ in 0..30 {
            if coord.worker_count() >= 1 { break; }
            sleep(Duration::from_millis(50)).await;
        }

        let data = sales_data();

        let result = coord.execute_distributed(
            "SELECT region, SUM(sales) AS total FROM sales GROUP BY region",
            "sales",
            data,
            Some("SELECT region, SUM(total) AS total FROM merged GROUP BY region"),
        ).await.unwrap();

        assert_eq!(result.num_rows, 3);
    }

    #[tokio::test]
    async fn test_broadcast_join_end_to_end() {
        // Set up a small dim table + a bigger fact table.
        // Verify broadcast_join returns correct row count matching a local join.
        std::env::set_var("KORE_CLUSTER_LOCAL", "1");

        let coord_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let coord_addr = coord_listener.local_addr().unwrap().to_string();
        let coord = Arc::new(Coordinator::new());
        let c2 = coord.clone();
        tokio::spawn(async move { c2.run(coord_listener).await });

        for i in 0..2 {
            let ca = coord_addr.clone();
            tokio::spawn(async move {
                let w = kore_worker::Worker::new(format!("bj-w-{i}"));
                let _ = w.run(&ca).await;
            });
        }
        for _ in 0..40 {
            if coord.worker_count() >= 2 { break; }
            sleep(Duration::from_millis(50)).await;
        }
        assert!(coord.worker_count() >= 2, "workers didn't register");

        // Small dim: 3 rows (regions).
        let dim = DataBlock {
            num_rows: 3,
            columns: vec![
                Column { name: "region".into(),
                    data: ColumnData::Str(vec![
                        Some("EU".into()), Some("US".into()), Some("AP".into()),
                    ]) },
                Column { name: "region_mgr".into(),
                    data: ColumnData::Str(vec![
                        Some("Alice".into()), Some("Bob".into()), Some("Chi".into()),
                    ]) },
            ],
        };

        // Big fact: 12 rows (sales).
        let fact = DataBlock {
            num_rows: 12,
            columns: vec![
                Column { name: "region".into(),
                    data: ColumnData::Str((0..12).map(|i| {
                        Some(["EU","US","AP"][i % 3].to_string())
                    }).collect()) },
                Column { name: "amount".into(),
                    data: ColumnData::Float64((0..12).map(|i| Some((i as f64) * 10.0)).collect()) },
            ],
        };

        // Local reference join (single-worker semantics via kore-sql).
        let mut ctx = KqlContext::new();
        ctx.register("fact", fact.clone());
        ctx.register("dim",  dim.clone());
        let expected = ctx.query(
            "SELECT * FROM fact INNER JOIN dim ON fact.region = dim.region"
        ).expect("local join");
        let expected_rows = expected.num_rows;
        assert_eq!(expected_rows, 12,
            "reference local join: expected 12 rows, got {}", expected_rows);

        // Distributed broadcast join through the cluster.
        let result = coord
            .execute_broadcast_join(
                "SELECT * FROM fact INNER JOIN dim ON fact.region = dim.region",
                "fact", fact,
                "dim",  dim,
            )
            .await
            .expect("broadcast join");

        assert_eq!(result.num_rows, expected_rows,
            "broadcast join should match local join row count \
             ({} vs {})", result.num_rows, expected_rows);
    }

    #[tokio::test]
    async fn test_network_shuffle_group_by() {
        // Explicitly enable Phase 9 network shuffle for this test.
        std::env::set_var("KORE_NET_SHUFFLE", "1");
        std::env::set_var("KORE_CLUSTER_LOCAL", "1");

        let coord_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let coord_addr = coord_listener.local_addr().unwrap().to_string();
        let coord = Arc::new(Coordinator::new());
        let c2 = coord.clone();
        tokio::spawn(async move { c2.run(coord_listener).await });

        for i in 0..3 {
            let ca = coord_addr.clone();
            tokio::spawn(async move {
                let w = kore_worker::Worker::new(format!("shuf-w-{i}"));
                let _ = w.run(&ca).await;
            });
        }

        for _ in 0..40 {
            if coord.worker_count() >= 3 { break; }
            sleep(Duration::from_millis(50)).await;
        }
        assert!(coord.worker_count() >= 3, "workers didn't register");

        let data = sales_data();
        let result = coord
            .execute_distributed_v2(
                "SELECT region, SUM(sales) AS total FROM sales GROUP BY region",
                "sales",
                data,
                Some("SELECT region, SUM(total) AS total FROM sales GROUP BY region"),
            )
            .await
            .expect("network shuffle query");

        assert_eq!(result.num_rows, 3,
            "expected 3 regions after network shuffle, got {}", result.num_rows);

        std::env::remove_var("KORE_NET_SHUFFLE");
    }

    #[tokio::test]
    async fn test_planned_broadcast_join_matches_local() {
        // Phase 16 end-to-end: register two tables, ask the coordinator to
        // plan a JOIN via kore-catalyst, verify:
        //   (a) the plan picks BroadcastHash from catalog stats,
        //   (b) execute_planned dispatches to execute_broadcast_join,
        //   (c) the row count matches a single-node kore-sql reference.
        std::env::set_var("KORE_CLUSTER_LOCAL", "1");

        let coord_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let coord_addr = coord_listener.local_addr().unwrap().to_string();
        let coord = Arc::new(Coordinator::new());
        let c2 = coord.clone();
        tokio::spawn(async move { c2.run(coord_listener).await });

        for i in 0..2 {
            let ca = coord_addr.clone();
            tokio::spawn(async move {
                let w = kore_worker::Worker::new(format!("planned-w-{i}"));
                let _ = w.run(&ca).await;
            });
        }
        for _ in 0..40 {
            if coord.worker_count() >= 2 { break; }
            sleep(Duration::from_millis(50)).await;
        }
        assert!(coord.worker_count() >= 2, "workers didn't register");

        let dim = DataBlock {
            num_rows: 3,
            columns: vec![
                Column { name: "region".into(),
                    data: ColumnData::Str(vec![
                        Some("EU".into()), Some("US".into()), Some("AP".into()),
                    ]) },
                Column { name: "region_mgr".into(),
                    data: ColumnData::Str(vec![
                        Some("Alice".into()), Some("Bob".into()), Some("Chi".into()),
                    ]) },
            ],
        };
        let fact = DataBlock {
            num_rows: 12,
            columns: vec![
                Column { name: "region".into(),
                    data: ColumnData::Str((0..12).map(|i| {
                        Some(["EU","US","AP"][i % 3].to_string())
                    }).collect()) },
                Column { name: "amount".into(),
                    data: ColumnData::Float64((0..12).map(|i| Some((i as f64) * 10.0)).collect()) },
            ],
        };

        // Local reference — what execute_planned should return.
        let mut ctx = KqlContext::new();
        ctx.register("fact", fact.clone());
        ctx.register("dim",  dim.clone());
        let expected = ctx.query(
            "SELECT * FROM fact INNER JOIN dim ON fact.region = dim.region"
        ).expect("local reference");

        // Register tables → catalog gets analyzed → planner sees real stats.
        coord.register_table_for_planning("fact", fact);
        coord.register_table_for_planning("dim",  dim);

        // (a) Explain should show BroadcastHash chosen from stats.
        let sql = "SELECT * FROM fact INNER JOIN dim ON fact.region = dim.region";
        let explain = coord.explain(sql).expect("explain");
        assert!(explain.contains("BroadcastHash"),
            "expected BroadcastHash in plan:\n{explain}");
        assert!(explain.contains("BroadcastJoin"),
            "expected BroadcastJoin dispatch:\n{explain}");

        // (b) plan_sql should classify as BroadcastJoin with `dim` as small side.
        let (_plan, dispatch) = coord.plan_sql(sql).expect("plan");
        match &dispatch {
            crate::plan::Dispatch::BroadcastJoin { large_table, small_table, .. } => {
                assert_eq!(large_table, "fact");
                assert_eq!(small_table, "dim");
            }
            other => panic!("expected BroadcastJoin, got {other:?}"),
        }

        // (c) End-to-end: plan-driven execute matches local reference.
        let result = coord.execute_planned(sql).await.expect("execute_planned");
        assert_eq!(result.num_rows, expected.num_rows,
            "planned execution should match local: {} vs {}",
            result.num_rows, expected.num_rows);
    }

    #[tokio::test]
    async fn test_distributed_local_tables() {
        let coord_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let coord_addr = coord_listener.local_addr().unwrap().to_string();
        let coord = Arc::new(Coordinator::new());
        let c2 = coord.clone();
        tokio::spawn(async move { c2.run(coord_listener).await });

        for _ in 0..2 {
            let ca = coord_addr.clone();
            tokio::spawn(start_worker(ca));
        }

        for _ in 0..20 {
            if coord.worker_count() >= 2 {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
        assert!(coord.worker_count() >= 1, "no workers registered");

        let data = sales_data();
        let result = coord
            .execute_distributed_v2(
                "SELECT * FROM sales WHERE sales > 150",
                "sales",
                data,
                None,
            )
            .await
            .unwrap();

        assert!(result.num_rows >= 3, "expected ≥3 rows, got {}", result.num_rows);
    }
}
