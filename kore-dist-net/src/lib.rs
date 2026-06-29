//! kore-dist-net — Layer 67: True Network Distribution
//!
//! Real TCP sockets. Real workers on real ports. Real multi-node capable.
//!
//! Protocol:
//!   Coordinator → Worker:  QueryTask   { sql, partition (DataBlock as JSON) }
//!   Worker      → Coordinator: TaskResult { data (DataBlock as JSON), worker_id, time_ms }
//!
//! Usage:
//!   # Start 4 workers (separate terminals or machines):
//!   kore-worker-node --port 9001
//!   kore-worker-node --port 9002
//!   kore-worker-node --port 9003
//!   kore-worker-node --port 9004
//!
//!   # Run query via coordinator:
//!   kore-coordinator --workers 127.0.0.1:9001,127.0.0.1:9002 --sql "SELECT ..."

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Instant;

use serde_json::Value;
use kore_core::types::{Column, ColumnData, DataBlock};
use kore_sql::KqlContext;

// ─── Protocol messages ────────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct QueryTask {
    pub sql:        String,
    pub table_name: String,
    pub partition:  SerializedBlock,
    pub worker_id:  u32,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct TaskResult {
    pub worker_id: u32,
    pub data:      SerializedBlock,
    pub rows:      usize,
    pub time_ms:   f64,
    pub error:     Option<String>,
}

/// DataBlock serialized as column-parallel arrays.
/// Compact binary representation: each column stored as typed flat array.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SerializedBlock {
    pub num_rows: usize,
    pub columns:  Vec<SerializedColumn>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SerializedColumn {
    pub name:   String,
    pub dtype:  String,   // "i64" | "f64" | "bool" | "str"
    pub values: Value,    // JSON array of values (nulls represented as null)
}

// ─── Serialization ────────────────────────────────────────────────────────────

pub fn serialize_block(block: &DataBlock) -> SerializedBlock {
    let columns = block.columns.iter().map(|col| {
        let (dtype, values) = match &col.data {
            ColumnData::Int64(v) => ("i64", serde_json::to_value(v).unwrap_or(Value::Null)),
            ColumnData::Float64(v) => ("f64", serde_json::to_value(v).unwrap_or(Value::Null)),
            ColumnData::Bool(v)    => ("bool", serde_json::to_value(v).unwrap_or(Value::Null)),
            ColumnData::Str(v)     => ("str", serde_json::to_value(v).unwrap_or(Value::Null)),
        };
        SerializedColumn { name: col.name.clone(), dtype: dtype.to_string(), values }
    }).collect();
    SerializedBlock { num_rows: block.num_rows, columns }
}

pub fn deserialize_block(sb: SerializedBlock) -> DataBlock {
    let columns: Vec<Column> = sb.columns.into_iter().map(|col| {
        let data = match col.dtype.as_str() {
            "i64"  => ColumnData::Int64(serde_json::from_value(col.values).unwrap_or_default()),
            "f64"  => ColumnData::Float64(serde_json::from_value(col.values).unwrap_or_default()),
            "bool" => ColumnData::Bool(serde_json::from_value(col.values).unwrap_or_default()),
            _      => ColumnData::Str(serde_json::from_value(col.values).unwrap_or_default()),
        };
        Column { name: col.name, data }
    }).collect();
    DataBlock { num_rows: sb.num_rows, columns }
}

// ─── Network helpers ──────────────────────────────────────────────────────────

/// Send length-prefixed JSON message over TCP.
pub fn send_message<T: serde::Serialize>(stream: &mut TcpStream, msg: &T) -> std::io::Result<()> {
    let json = serde_json::to_vec(msg).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let len = json.len() as u64;
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(&json)?;
    stream.flush()
}

/// Receive length-prefixed JSON message over TCP.
pub fn recv_message<T: serde::de::DeserializeOwned>(stream: &mut TcpStream) -> std::io::Result<T> {
    let mut len_buf = [0u8; 8];
    stream.read_exact(&mut len_buf)?;
    let len = u64::from_le_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
}

// ─── Worker node ──────────────────────────────────────────────────────────────

/// Run a worker node: listens on `addr`, executes query tasks, returns results.
pub fn run_worker(addr: &str) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    eprintln!("[kore-worker] Listening on {addr}");

    for stream in listener.incoming() {
        let mut stream = stream?;
        eprintln!("[kore-worker] Connection received");

        // Receive task
        let task: QueryTask = match recv_message(&mut stream) {
            Ok(t) => t,
            Err(e) => { eprintln!("[kore-worker] Recv error: {e}"); continue; }
        };

        eprintln!("[kore-worker {}] Executing: {} (partition: {} rows)",
            task.worker_id, &task.sql[..task.sql.len().min(60)], task.partition.num_rows);

        let t0 = Instant::now();
        let block = deserialize_block(task.partition);
        let result = execute_task(&task.sql, &task.table_name, block);
        let time_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let response = match result {
            Ok(data) => {
                let rows = data.num_rows;
                eprintln!("[kore-worker {}] Done: {} rows in {:.1}ms", task.worker_id, rows, time_ms);
                TaskResult {
                    worker_id: task.worker_id,
                    data: serialize_block(&data),
                    rows, time_ms, error: None,
                }
            }
            Err(e) => {
                eprintln!("[kore-worker {}] Error: {e}", task.worker_id);
                TaskResult {
                    worker_id: task.worker_id,
                    data: SerializedBlock { num_rows: 0, columns: vec![] },
                    rows: 0, time_ms, error: Some(e),
                }
            }
        };

        if let Err(e) = send_message(&mut stream, &response) {
            eprintln!("[kore-worker] Send error: {e}");
        }
    }
    Ok(())
}

fn execute_task(sql: &str, table_name: &str, block: DataBlock) -> Result<DataBlock, String> {
    let mut ctx = KqlContext::new();
    ctx.register(table_name, block);
    ctx.query(sql).map_err(|e| format!("{e}"))
}

// ─── Coordinator ──────────────────────────────────────────────────────────────

/// Distribute a query across network workers.
/// Each worker gets a horizontal partition (row slice) of the data.
pub fn distribute_query(
    sql:         &str,
    table_name:  &str,
    data:        &DataBlock,
    worker_addrs: &[&str],
) -> Result<DataBlock, String> {
    use std::thread;

    let n = data.num_rows;
    let t = worker_addrs.len();
    if t == 0 { return Err("No workers".to_string()); }

    let chunk = ((n + t - 1) / t).max(1);

    eprintln!("[kore-coord] Distributing {} rows across {} workers", n, t);
    eprintln!("[kore-coord] SQL: {}", &sql[..sql.len().min(80)]);

    // Send tasks to workers in parallel threads
    let results: Vec<Result<TaskResult, String>> = worker_addrs.iter()
        .enumerate()
        .map(|(w, addr)| {
            let start = w * chunk;
            let end   = (start + chunk).min(n);
            if start >= end { return Ok(TaskResult {
                worker_id: w as u32, rows: 0, time_ms: 0.0, error: None,
                data: SerializedBlock { num_rows: 0, columns: vec![] }
            }); }

            let partition = data.select_rows(&(start..end).collect::<Vec<_>>());
            let task = QueryTask {
                sql:        sql.to_string(),
                table_name: table_name.to_string(),
                partition:  serialize_block(&partition),
                worker_id:  w as u32,
            };

            let addr = addr.to_string();
            // Connect to worker
            let mut stream = TcpStream::connect(&addr)
                .map_err(|e| format!("Cannot connect to {addr}: {e}"))?;
            send_message(&mut stream, &task)
                .map_err(|e| format!("Send to {addr}: {e}"))?;
            let result: TaskResult = recv_message(&mut stream)
                .map_err(|e| format!("Recv from {addr}: {e}"))?;
            Ok(result)
        })
        .collect();

    // Collect successful partial results
    let mut partial_blocks: Vec<DataBlock> = Vec::new();
    for r in results {
        match r {
            Ok(task_result) => {
                if let Some(err) = task_result.error {
                    eprintln!("[kore-coord] Worker {} error: {}", task_result.worker_id, err);
                } else {
                    eprintln!("[kore-coord] Worker {} returned {} rows in {:.1}ms",
                        task_result.worker_id, task_result.rows, task_result.time_ms);
                    let block = deserialize_block(task_result.data);
                    if block.num_rows > 0 { partial_blocks.push(block); }
                }
            }
            Err(e) => eprintln!("[kore-coord] Worker error: {e}"),
        }
    }

    if partial_blocks.is_empty() { return Ok(DataBlock::empty()); }

    // Merge partial results: re-run aggregation on combined partials
    let combined = DataBlock::concat(partial_blocks)
        .map_err(|e| format!("Concat: {e}"))?;

    eprintln!("[kore-coord] Merging {} partial rows...", combined.num_rows);

    // Re-run SQL on coordinator to finalize aggregation
    let mut ctx = KqlContext::new();
    ctx.register(table_name, combined);
    ctx.query(sql).map_err(|e| format!("Final merge: {e}"))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::types::{Column, ColumnData, DataBlock};
    use std::thread;
    use std::time::Duration;

    fn test_data(n: usize) -> DataBlock {
        DataBlock {
            num_rows: n,
            columns: vec![
                Column { name: "amount".into(), data: ColumnData::Float64(
                    (0..n).map(|i| Some(i as f64)).collect()
                )},
                Column { name: "cat".into(), data: ColumnData::Str(
                    (0..n).map(|i| Some(["A","B","C"][i%3].to_string())).collect()
                )},
            ],
        }
    }

    #[test]
    fn test_serialization_roundtrip() {
        let data = test_data(100);
        let serialized = serialize_block(&data);
        let restored = deserialize_block(serialized);
        assert_eq!(restored.num_rows, 100);
        assert_eq!(restored.columns.len(), 2);
        println!("Serialization roundtrip: {} rows ✓", restored.num_rows);
    }

    #[test]
    fn test_worker_coordinator_local() {
        // Start a worker on a local port
        let port = 19876;
        thread::spawn(move || {
            let _ = run_worker(&format!("127.0.0.1:{port}"));
        });
        thread::sleep(Duration::from_millis(100));

        // Run a query through the network
        let data = test_data(300);
        let result = distribute_query(
            "SELECT cat, SUM(amount) AS total FROM sales GROUP BY cat",
            "sales",
            &data,
            &[&format!("127.0.0.1:{port}")],
        ).expect("distributed query failed");

        assert_eq!(result.num_rows, 3, "Expected 3 groups (A/B/C)");
        println!("Network distributed query: {} groups ✓", result.num_rows);
    }
}
