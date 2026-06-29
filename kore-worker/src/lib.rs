//! KORE Layer 38 — Distributed Worker Node
//!
//! A worker:
//!  1. Binds a TCP listener for task connections.
//!  2. Connects to the coordinator and sends RegisterWorker.
//!  3. Accepts task connections from the coordinator.
//!  4. Executes each AssignTask using kore-sql and returns TaskResult.
//!  5. Sends periodic Heartbeats to the coordinator.

use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, interval};
use kore_core::{DataBlock, KoreError};
use kore_net::{KoreFrame, KoreMsg, TaskStats, now_ms};
use kore_sql::executor::{KqlContext, execute};

// ─── Worker ───────────────────────────────────────────────────────────────────

pub struct Worker {
    pub id:    String,
    pub cores: usize,
}

impl Worker {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), cores: num_cpus() }
    }

    /// Start the worker: register with coordinator and serve tasks.
    ///
    /// * `coord_addr` — e.g. `"127.0.0.1:7878"`
    pub async fn run(&self, coord_addr: &str) -> Result<(), std::io::Error> {
        // 1. Bind task listener (OS assigns free port)
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let task_addr = listener.local_addr()?.to_string();

        // 2. Register with coordinator
        let mut reg_stream = TcpStream::connect(coord_addr).await?;
        KoreFrame::write(&mut reg_stream, &KoreMsg::RegisterWorker {
            id:        self.id.clone(),
            task_addr: task_addr.clone(),
            cores:     self.cores,
            memory_mb: available_mem_mb(),
        }).await?;

        // Wait for ack
        let _ack = KoreFrame::read(&mut reg_stream).await?;

        let id = Arc::new(self.id.clone());

        // 3. Heartbeat task (every 5 s)
        {
            let id2 = id.clone();
            let ca  = coord_addr.to_string();
            tokio::spawn(async move {
                let mut ticker = interval(Duration::from_secs(5));
                loop {
                    ticker.tick().await;
                    if let Ok(mut s) = TcpStream::connect(&ca).await {
                        let _ = KoreFrame::write(&mut s, &KoreMsg::Heartbeat {
                            worker_id:    id2.as_ref().clone(),
                            timestamp_ms: now_ms(),
                            active_tasks: 0,
                            free_mem_mb:  available_mem_mb(),
                        }).await;
                    }
                }
            });
        }

        // 4. Accept task connections
        loop {
            let (stream, _peer) = listener.accept().await?;
            let wid = id.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_task_conn(stream, &wid).await {
                    eprintln!("[worker {}] task error: {e}", wid);
                }
            });
        }
    }
}

// ─── Task handler ─────────────────────────────────────────────────────────────

async fn handle_task_conn(
    mut stream: TcpStream,
    worker_id: &str,
) -> Result<(), std::io::Error> {
    let msg = KoreFrame::read(&mut stream).await?;

    match msg {
        KoreMsg::AssignTask { task_id, partition_id, sql, table_name, data, .. } => {
            let t0 = now_ms();
            let rows_in = data.num_rows;

            let result = run_sql(&sql, &table_name, data);

            let (result_block, err_msg) = match result {
                Ok(b)  => (Some(b), None),
                Err(e) => (None, Some(e.to_string())),
            };

            if let Some(err) = err_msg {
                KoreFrame::write(&mut stream, &KoreMsg::TaskError {
                    task_id, message: err, attempt: 1,
                }).await?;
                return Ok(());
            }

            let result = result_block.unwrap();
            let rows_out = result.num_rows;
            let elapsed  = now_ms() - t0;

            eprintln!("[worker {worker_id}] task {task_id} part={partition_id} \
                       rows {rows_in}→{rows_out} in {elapsed}ms");

            KoreFrame::write(&mut stream, &KoreMsg::TaskResult {
                task_id,
                partition_id,
                result,
                stats: TaskStats {
                    elapsed_ms:    elapsed,
                    rows_in,
                    rows_out,
                    bytes_read:    0,
                    bytes_written: 0,
                    attempt:       1,
                },
            }).await?;
        }

        KoreMsg::Ping => { KoreFrame::write(&mut stream, &KoreMsg::Pong).await?; }
        KoreMsg::Shutdown => {}
        other => {
            eprintln!("[worker {worker_id}] unexpected message: {:?}", other);
        }
    }
    Ok(())
}

// ─── SQL execution helper ─────────────────────────────────────────────────────

fn run_sql(sql: &str, table_name: &str, data: DataBlock) -> Result<DataBlock, KoreError> {
    let mut ctx = KqlContext::new();
    ctx.register(table_name, data);
    ctx.query(sql)
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

fn available_mem_mb() -> usize {
    // Rough estimate: not available on all platforms without external crate
    512
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::{TcpListener, TcpStream};
    use kore_core::{Column, ColumnData, DataBlock};
    use kore_net::{KoreFrame, KoreMsg};

    async fn fake_coordinator(listener: TcpListener) -> (String, String) {
        // Accept worker registration, send ack, return worker's task_addr
        let (mut stream, _) = listener.accept().await.unwrap();
        let msg = KoreFrame::read(&mut stream).await.unwrap();
        if let KoreMsg::RegisterWorker { id, task_addr, .. } = msg {
            KoreFrame::write(&mut stream, &KoreMsg::RegisterAck {
                worker_id: id.clone(),
            }).await.unwrap();
            return (id, task_addr);
        }
        panic!("expected RegisterWorker");
    }

    #[tokio::test]
    async fn test_worker_registers_and_executes() {
        // Fake coordinator
        let coord_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let coord_addr = coord_listener.local_addr().unwrap().to_string();

        // Start fake coordinator in background, capture worker's task_addr
        let coord_task = tokio::spawn(fake_coordinator(coord_listener));

        // Start real worker
        let worker = Worker::new("w1");
        let ca = coord_addr.clone();
        tokio::spawn(async move { let _ = worker.run(&ca).await; });

        // Wait for registration to complete
        let (_, task_addr) = coord_task.await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Now send a task directly to the worker's task_addr
        let data = DataBlock {
            num_rows: 5,
            columns: vec![
                Column { name: "id".into(),  data: ColumnData::Int64(vec![Some(1),Some(2),Some(3),Some(4),Some(5)]) },
                Column { name: "val".into(), data: ColumnData::Float64(vec![Some(10.0),Some(20.0),Some(30.0),Some(40.0),Some(50.0)]) },
            ],
        };

        let mut conn = TcpStream::connect(&task_addr).await.unwrap();
        KoreFrame::write(&mut conn, &KoreMsg::AssignTask {
            task_id:      "t1".into(),
            stage_id:     0,
            partition_id: 0,
            sql:          "SELECT * FROM tbl WHERE val > 25".into(),
            table_name:   "tbl".into(),
            data,
        }).await.unwrap();

        let reply = KoreFrame::read(&mut conn).await.unwrap();
        match reply {
            KoreMsg::TaskResult { result, .. } => {
                // val > 25 → rows with val=30,40,50 → 3 rows
                assert_eq!(result.num_rows, 3);
            }
            other => panic!("unexpected reply: {:?}", other),
        }
    }
}
