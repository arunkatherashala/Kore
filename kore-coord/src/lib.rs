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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::net::{TcpListener, TcpStream};
use kore_core::{Column, ColumnData, DataBlock, KoreError};
use kore_net::{KoreFrame, KoreMsg, TaskStats, partition_block, now_ms};
use kore_sql::executor::{KqlContext, execute};

// ─── Worker registry ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct WorkerInfo {
    id:        String,
    task_addr: String,
    cores:     usize,
    memory_mb: usize,
    last_seen: u64,
}

// ─── Coordinator ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Coordinator {
    workers: Arc<Mutex<Vec<WorkerInfo>>>,
}

impl Coordinator {
    pub fn new() -> Self {
        Self { workers: Arc::new(Mutex::new(Vec::new())) }
    }

    /// Number of currently registered workers.
    pub fn worker_count(&self) -> usize {
        self.workers.lock().unwrap().len()
    }

    /// Run the coordinator: listen for worker registrations + heartbeats.
    pub async fn run(&self, listener: TcpListener) {
        let workers = self.workers.clone();
        loop {
            if let Ok((mut stream, peer)) = listener.accept().await {
                let w = workers.clone();
                tokio::spawn(async move {
                    match KoreFrame::read(&mut stream).await {
                        Ok(KoreMsg::RegisterWorker { id, task_addr, cores, memory_mb }) => {
                            eprintln!("[coord] worker registered: {id}  addr={task_addr}");
                            let info = WorkerInfo { id: id.clone(), task_addr, cores, memory_mb, last_seen: now_ms() };
                            w.lock().unwrap().push(info);
                            let _ = KoreFrame::write(&mut stream, &KoreMsg::RegisterAck { worker_id: id }).await;
                        }
                        Ok(KoreMsg::Heartbeat { worker_id, timestamp_ms, .. }) => {
                            // Update last_seen
                            let mut ws = w.lock().unwrap();
                            if let Some(info) = ws.iter_mut().find(|wi| wi.id == worker_id) {
                                info.last_seen = timestamp_ms;
                            }
                        }
                        Ok(KoreMsg::Ping) => { let _ = KoreFrame::write(&mut stream, &KoreMsg::Pong).await; }
                        _ => {}
                    }
                });
            }
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

        // Phase-1: each worker computes partial SUM per region
        // Phase-2: coordinator re-aggregates the merged partials
        let result = coord.execute_distributed(
            "SELECT region, SUM(sales) AS total FROM sales GROUP BY region",
            "sales",
            data,
            Some("SELECT region, SUM(total) AS total FROM merged GROUP BY region"),
        ).await.unwrap();

        // 3 distinct regions
        assert_eq!(result.num_rows, 3);
    }
}
