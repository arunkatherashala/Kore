//! KORE Layer 74 — True multi-node cluster (gRPC-style over TCP).
//!
//! Uses length-prefixed JSON messages over plain TCP (tokio).
//! Each node is a `ClusterNode` which can act as Coordinator or Worker.
//!
//! # Protocol
//! Every message is framed as: `[u32 length (BE)] [JSON bytes]`

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::io;

use kore_core::{Column, ColumnData, DataBlock, Value};
use kore_sql::KqlContext;
use serde_json::{json, Value as JValue};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// ─── Config ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub node_id: String,
    pub host:    String,
    pub port:    u16,
    pub role:    NodeRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeRole {
    Coordinator,
    Worker,
}

impl NodeConfig {
    pub fn worker(id: &str, port: u16) -> Self {
        Self { node_id: id.into(), host: "127.0.0.1".into(), port, role: NodeRole::Worker }
    }
    pub fn coordinator(port: u16) -> Self {
        Self { node_id: "coordinator".into(), host: "127.0.0.1".into(), port, role: NodeRole::Coordinator }
    }
    pub fn addr(&self) -> String { format!("{}:{}", self.host, self.port) }
}

// ─── Result types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct QueryPlan {
    pub query_id: String,
    pub sql:      String,
    pub workers:  Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PartialResult {
    pub worker_id: String,
    pub query_id:  String,
    pub data:      DataBlock,
}

// ─── Cluster node ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ClusterNode {
    pub config: NodeConfig,
    pub peers:  Vec<NodeConfig>,
    data:       Arc<RwLock<HashMap<String, DataBlock>>>,
}

impl ClusterNode {
    pub fn new(config: NodeConfig) -> Self {
        Self { config, peers: Vec::new(), data: Arc::new(RwLock::new(HashMap::new())) }
    }

    pub fn add_peer(&mut self, peer: NodeConfig) {
        self.peers.push(peer);
    }

    // ── Local table store ─────────────────────────────────────────────────────

    pub fn store_table(&self, name: &str, block: DataBlock) {
        self.data.write().unwrap().insert(name.to_string(), block);
    }

    pub fn get_table(&self, name: &str) -> Option<DataBlock> {
        self.data.read().unwrap().get(name).cloned()
    }

    // ── TCP framing ───────────────────────────────────────────────────────────

    async fn send_msg(stream: &mut TcpStream, msg: &JValue) -> io::Result<()> {
        let bytes = serde_json::to_vec(msg).unwrap_or_default();
        let len   = bytes.len() as u32;
        stream.write_all(&len.to_be_bytes()).await?;
        stream.write_all(&bytes).await?;
        Ok(())
    }

    async fn recv_msg(stream: &mut TcpStream) -> io::Result<JValue> {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf).await?;
        serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    // ── Worker server ──────────────────────────────────────────────────────────

    /// Start a worker TCP server.  Returns a JoinHandle that runs forever.
    pub fn start_worker(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let addr = self.config.addr();
            let listener = TcpListener::bind(&addr).await
                .expect(&format!("worker bind {addr}"));

            loop {
                let Ok((mut stream, _peer)) = listener.accept().await else { continue };
                let node = self.clone();
                tokio::spawn(async move {
                    if let Err(e) = node.handle_connection(&mut stream).await {
                        eprintln!("[worker {}] connection error: {e}", node.config.node_id);
                    }
                });
            }
        })
    }

    async fn handle_connection(&self, stream: &mut TcpStream) -> io::Result<()> {
        let msg = Self::recv_msg(stream).await?;
        let action = msg.get("action").and_then(|v| v.as_str()).unwrap_or("");

        match action {
            "load_shard" => {
                let table = msg["table"].as_str().unwrap_or("shard");
                let block = json_to_block(&msg["data"])?;
                self.store_table(table, block.clone());
                let reply = json!({"status": "ok", "rows": block.num_rows});
                Self::send_msg(stream, &reply).await?;
            }
            "run_sql" => {
                let sql      = msg["sql"].as_str().unwrap_or("");
                let query_id = msg["query_id"].as_str().unwrap_or("q0");
                let table    = msg["table"].as_str().unwrap_or("shard");

                let result = {
                    let mut ctx = KqlContext::new();
                    if let Some(block) = self.get_table(table) {
                        ctx.register(table, block);
                    }
                    ctx.query(sql)
                };

                match result {
                    Ok(block) => {
                        let reply = json!({
                            "query_id": query_id,
                            "result":   block_to_json_rows(&block),
                            "rows":     block.num_rows,
                        });
                        Self::send_msg(stream, &reply).await?;
                    }
                    Err(e) => {
                        let reply = json!({"error": e.to_string()});
                        Self::send_msg(stream, &reply).await?;
                    }
                }
            }
            "ping" => {
                Self::send_msg(stream, &json!({"status": "pong"})).await?;
            }
            _ => {
                Self::send_msg(stream, &json!({"error": "unknown action"})).await?;
            }
        }
        Ok(())
    }

    // ── Coordinator: distribute query ──────────────────────────────────────────

    /// Split `table` into N shards, send each to a worker, run `sql`, merge results.
    pub async fn distribute_query(
        &self,
        sql:   &str,
        table: &str,
    ) -> Result<DataBlock, String> {
        let workers = &self.peers;
        if workers.is_empty() {
            return Err("no workers configured".into());
        }

        let block = self.get_table(table)
            .ok_or_else(|| format!("table '{table}' not found on coordinator"))?;

        let n_workers = workers.len();
        let shard_size = (block.num_rows + n_workers - 1) / n_workers;

        // Send shards in parallel and collect partial results
        let mut handles: Vec<tokio::task::JoinHandle<Result<DataBlock, String>>> = Vec::new();

        for (wi, worker) in workers.iter().enumerate() {
            let start = wi * shard_size;
            let end   = ((wi + 1) * shard_size).min(block.num_rows);
            if start >= block.num_rows { break; }

            let shard = block.select_rows(
                &(start..end).collect::<Vec<_>>()
            );
            let worker_addr = worker.addr();
            let sql_clone   = sql.to_string();
            let query_id    = format!("q-{wi}");
            let table_name  = table.to_string();

            handles.push(tokio::spawn(async move {
                run_on_worker(&worker_addr, &table_name, shard, &sql_clone, &query_id).await
            }));
        }

        let mut parts: Vec<DataBlock> = Vec::new();
        for h in handles {
            match h.await {
                Ok(Ok(b))  => parts.push(b),
                Ok(Err(e)) => return Err(format!("worker error: {e}")),
                Err(e)     => return Err(format!("task join error: {e}")),
            }
        }

        DataBlock::concat(parts).map_err(|e| e.to_string())
    }

    // ── Start local cluster ────────────────────────────────────────────────────

    /// Start N worker nodes on localhost starting at `base_port`.
    /// Returns JoinHandles (callers should keep them alive).
    pub fn start_local_cluster(n_workers: usize, base_port: u16) -> (ClusterNode, Vec<tokio::task::JoinHandle<()>>) {
        let mut coordinator = ClusterNode::new(NodeConfig::coordinator(base_port));
        let mut handles = Vec::new();

        for i in 0..n_workers {
            let port   = base_port + 1 + i as u16;
            let config = NodeConfig::worker(&format!("w{i}"), port);
            coordinator.add_peer(config.clone());
            let worker = ClusterNode::new(config);
            handles.push(worker.start_worker());
        }

        (coordinator, handles)
    }

    // ── Health check ──────────────────────────────────────────────────────────

    pub async fn health_check(&self) -> HashMap<String, bool> {
        let mut results = HashMap::new();
        for peer in &self.peers {
            let addr = peer.addr();
            let alive = ping_worker(&addr).await;
            results.insert(peer.node_id.clone(), alive);
        }
        results
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

async fn run_on_worker(
    addr:      &str,
    table:     &str,
    shard:     DataBlock,
    sql:       &str,
    query_id:  &str,
) -> Result<DataBlock, String> {
    let mut stream = TcpStream::connect(addr).await
        .map_err(|e| format!("connect {addr}: {e}"))?;

    // 1. Load shard
    let load_msg = json!({
        "action": "load_shard",
        "table":  table,
        "data":   block_to_json_rows(&shard),
    });
    ClusterNode::send_msg(&mut stream, &load_msg).await
        .map_err(|e| e.to_string())?;
    let _ack = ClusterNode::recv_msg(&mut stream).await
        .map_err(|e| e.to_string())?;

    // 2. Drop and reconnect (single-request-per-connection model)
    drop(stream);
    let mut stream = TcpStream::connect(addr).await
        .map_err(|e| format!("connect {addr}: {e}"))?;

    // 3. Run SQL
    let run_msg = json!({
        "action":   "run_sql",
        "sql":      sql,
        "query_id": query_id,
        "table":    table,
    });
    ClusterNode::send_msg(&mut stream, &run_msg).await
        .map_err(|e| e.to_string())?;
    let reply = ClusterNode::recv_msg(&mut stream).await
        .map_err(|e| e.to_string())?;

    if let Some(err) = reply.get("error") {
        return Err(err.to_string());
    }

    json_to_block(&reply["result"]).map_err(|e| e.to_string())
}

async fn ping_worker(addr: &str) -> bool {
    let Ok(mut stream) = TcpStream::connect(addr).await else { return false };
    let msg = json!({"action": "ping"});
    if ClusterNode::send_msg(&mut stream, &msg).await.is_err() { return false; }
    matches!(ClusterNode::recv_msg(&mut stream).await, Ok(v) if v["status"] == "pong")
}

fn block_to_json_rows(block: &DataBlock) -> JValue {
    let rows: Vec<JValue> = (0..block.num_rows).map(|r| {
        let mut obj = serde_json::Map::new();
        for col in &block.columns {
            let v = match col.data.get_value(r) {
                Value::Int(i)   => JValue::Number(i.into()),
                Value::Float(f) => serde_json::Number::from_f64(f)
                    .map(JValue::Number).unwrap_or(JValue::Null),
                Value::Bool(b)  => JValue::Bool(b),
                Value::Str(s)   => JValue::String(s),
                Value::Null     => JValue::Null,
            };
            obj.insert(col.name.clone(), v);
        }
        JValue::Object(obj)
    }).collect();
    JValue::Array(rows)
}

fn json_to_block(rows: &JValue) -> io::Result<DataBlock> {
    let arr = rows.as_array()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "expected JSON array"))?;
    if arr.is_empty() {
        return Ok(DataBlock::empty());
    }

    // Infer schema from first row
    let first = arr[0].as_object()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "expected JSON object rows"))?;

    let mut col_names: Vec<String> = first.keys().cloned().collect();
    col_names.sort(); // deterministic order

    // Collect values per column
    let mut col_data: Vec<Vec<JValue>> = vec![Vec::with_capacity(arr.len()); col_names.len()];
    for row in arr {
        let obj = row.as_object()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "row not object"))?;
        for (ci, name) in col_names.iter().enumerate() {
            col_data[ci].push(obj.get(name).cloned().unwrap_or(JValue::Null));
        }
    }

    let mut columns: Vec<Column> = Vec::new();
    for (ci, name) in col_names.iter().enumerate() {
        let vals = &col_data[ci];
        // Infer type: i64 > f64 > str
        let all_int = vals.iter().all(|v| v.is_null() || v.is_i64());
        if all_int {
            let data: Vec<Option<i64>> = vals.iter()
                .map(|v| v.as_i64())
                .collect();
            columns.push(Column::int64(name, data));
        } else {
            let all_float = vals.iter().all(|v| v.is_null() || v.is_number());
            if all_float {
                let data: Vec<Option<f64>> = vals.iter()
                    .map(|v| v.as_f64())
                    .collect();
                columns.push(Column::float64(name, data));
            } else {
                let data: Vec<Option<String>> = vals.iter()
                    .map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                columns.push(Column::str_col(name, data));
            }
        }
    }

    DataBlock::new(columns)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_table(n: usize) -> DataBlock {
        let ids:    Vec<Option<i64>> = (0..n as i64).map(Some).collect();
        let cats:   Vec<Option<String>> = (0..n).map(|i| Some(format!("cat{}", i % 3))).collect();
        let vals:   Vec<Option<f64>> = (0..n).map(|i| Some(i as f64)).collect();
        DataBlock::new(vec![
            Column::int64("id",       ids),
            Column::str_col("cat",    cats),
            Column::float64("amount", vals),
        ]).unwrap()
    }

    #[tokio::test]
    async fn worker_ping() {
        let worker = ClusterNode::new(NodeConfig::worker("w0", 15001));
        let _h = worker.start_worker();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(ping_worker("127.0.0.1:15001").await);
    }

    #[tokio::test]
    async fn local_cluster_health() {
        let (coord, _handles) = ClusterNode::start_local_cluster(2, 15010);
        // Give workers time to bind
        tokio::time::sleep(Duration::from_millis(80)).await;
        let health = coord.health_check().await;
        assert_eq!(health.len(), 2);
        for (_id, alive) in &health {
            assert!(*alive, "worker not alive: {_id}");
        }
    }

    #[tokio::test]
    async fn distribute_select_star() {
        let (mut coord, _handles) = ClusterNode::start_local_cluster(2, 15020);
        tokio::time::sleep(Duration::from_millis(80)).await;

        let table = make_table(100);
        coord.store_table("t", table);

        // Rewrite SQL to use the worker's local table name "t"
        let result = coord.distribute_query("SELECT id, amount FROM t", "t").await
            .expect("distribute_query failed");

        assert_eq!(result.num_rows, 100);
    }

    #[tokio::test]
    async fn json_roundtrip() {
        let block = make_table(10);
        let json  = block_to_json_rows(&block);
        let back  = json_to_block(&json).unwrap();
        assert_eq!(back.num_rows, 10);
    }
}
