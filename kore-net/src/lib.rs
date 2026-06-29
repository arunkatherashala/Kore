//! KORE Layer 37 — Network Transport Protocol
//!
//! Provides the wire protocol used between the coordinator and workers:
//! - `KoreMsg` enum — every message type in the cluster
//! - `KoreFrame` — async length-prefixed framing (4-byte BE + JSON body)
//! - `TaskStats` — per-task execution metadata
//!
//! Wire format:
//!   [ 4 bytes: payload length (big-endian u32) ][ N bytes: serde_json payload ]

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use kore_core::DataBlock;

// ─── Messages ─────────────────────────────────────────────────────────────────

/// Every message exchanged between coordinator ↔ workers over TCP.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum KoreMsg {
    // ── Registration ────────────────────────────────────────────────────────
    /// Worker → Coordinator: announce presence.
    RegisterWorker {
        id:         String,
        task_addr:  String,   // TCP address where coordinator sends tasks
        cores:      usize,
        memory_mb:  usize,
    },
    /// Coordinator → Worker: registration confirmed.
    RegisterAck { worker_id: String },

    // ── Task dispatch ────────────────────────────────────────────────────────
    /// Coordinator → Worker: execute this SQL on this data partition.
    AssignTask {
        task_id:      String,
        stage_id:     usize,
        partition_id: usize,
        sql:          String,
        table_name:   String,
        data:         DataBlock,
    },
    /// Worker → Coordinator: task completed successfully.
    TaskResult {
        task_id:      String,
        partition_id: usize,
        result:       DataBlock,
        stats:        TaskStats,
    },
    /// Worker → Coordinator: task failed.
    TaskError {
        task_id:  String,
        message:  String,
        attempt:  usize,
    },

    // ── Heartbeat ────────────────────────────────────────────────────────────
    Heartbeat {
        worker_id:    String,
        timestamp_ms: u64,
        active_tasks: usize,
        free_mem_mb:  usize,
    },

    // ── Shuffle ──────────────────────────────────────────────────────────────
    /// Worker → Coordinator (shuffle store): push a partition.
    ShuffleWrite {
        src_worker:    String,
        dest_part:     usize,
        data:          DataBlock,
    },
    /// Coordinator → Worker: pull these shuffle partitions.
    ShuffleRead { partition_ids: Vec<usize> },
    /// Coordinator → Worker: here are the requested shuffle partitions.
    ShuffleData { partitions: Vec<(usize, DataBlock)> },

    // ── Control ──────────────────────────────────────────────────────────────
    Shutdown,
    Ping,
    Pong,
}

/// Per-task performance metadata.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskStats {
    pub elapsed_ms:    u64,
    pub rows_in:       usize,
    pub rows_out:      usize,
    pub bytes_read:    usize,
    pub bytes_written: usize,
    pub attempt:       usize,
}

// ─── Framing ──────────────────────────────────────────────────────────────────

/// Zero-copy length-prefixed framing over `tokio::io` streams.
pub struct KoreFrame;

impl KoreFrame {
    /// Write one message: `[4-byte BE length][JSON payload]`.
    pub async fn write<W>(w: &mut W, msg: &KoreMsg) -> std::io::Result<()>
    where W: tokio::io::AsyncWrite + Unpin
    {
        let payload = serde_json::to_vec(msg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let len = (payload.len() as u32).to_be_bytes();
        w.write_all(&len).await?;
        w.write_all(&payload).await?;
        w.flush().await
    }

    /// Read one message.  Blocks until a full frame arrives.
    pub async fn read<R>(r: &mut R) -> std::io::Result<KoreMsg>
    where R: tokio::io::AsyncRead + Unpin
    {
        let mut len_buf = [0u8; 4];
        r.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > 256 * 1024 * 1024 {   // 256 MB safety cap
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("frame too large: {len} bytes"),
            ));
        }
        let mut payload = vec![0u8; len];
        r.read_exact(&mut payload).await?;
        serde_json::from_slice(&payload)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

// ─── Utility ──────────────────────────────────────────────────────────────────

/// Returns the current time as milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ─── Partition helper ─────────────────────────────────────────────────────────

/// Split a `DataBlock` into `n` roughly equal partitions by row range.
pub fn partition_block(block: DataBlock, n: usize) -> Vec<DataBlock> {
    if n == 0 { return vec![block]; }
    let rows = block.num_rows;
    let chunk = (rows + n - 1) / n;   // ceiling division
    (0..n)
        .map(|i| {
            let start = i * chunk;
            let end   = rows.min(start + chunk);
            if start >= rows {
                // Empty partition (fewer rows than partitions)
                DataBlock {
                    num_rows: 0,
                    columns: block.columns.iter().map(|c| {
                        kore_core::Column { name: c.name.clone(), data: c.data.empty_like() }
                    }).collect(),
                }
            } else {
                let indices: Vec<usize> = (start..end).collect();
                block.select_rows(&indices)
            }
        })
        .filter(|p| p.num_rows > 0)
        .collect()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::{TcpListener, TcpStream};
    use kore_core::{Column, ColumnData, DataBlock};

    fn make_block(n: usize) -> DataBlock {
        DataBlock {
            num_rows: n,
            columns: vec![
                Column { name: "id".into(),
                    data: ColumnData::Int64((0..n).map(|i| Some(i as i64)).collect()) },
                Column { name: "val".into(),
                    data: ColumnData::Float64((0..n).map(|i| Some(i as f64 * 1.5)).collect()) },
            ],
        }
    }

    #[test]
    fn test_partition_block() {
        let b = make_block(10);
        let parts = partition_block(b, 3);
        // 4 + 4 + 2 = 10
        assert_eq!(parts.len(), 3);
        let total: usize = parts.iter().map(|p| p.num_rows).sum();
        assert_eq!(total, 10);
    }

    #[tokio::test]
    async fn test_framing_roundtrip() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Server: read one message and echo it back
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let msg = KoreFrame::read(&mut stream).await.unwrap();
            KoreFrame::write(&mut stream, &msg).await.unwrap();
        });

        // Client: send a Ping, expect Pong-as-echo
        let mut client = TcpStream::connect(addr).await.unwrap();
        KoreFrame::write(&mut client, &KoreMsg::Ping).await.unwrap();
        let reply = KoreFrame::read(&mut client).await.unwrap();
        assert!(matches!(reply, KoreMsg::Ping));
    }

    #[tokio::test]
    async fn test_large_datablock_framing() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let block = make_block(1000);
        let block_clone = block.clone();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let msg = KoreFrame::read(&mut stream).await.unwrap();
            KoreFrame::write(&mut stream, &KoreMsg::Pong).await.unwrap();
            if let KoreMsg::AssignTask { data, .. } = msg {
                assert_eq!(data.num_rows, 1000);
            }
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        KoreFrame::write(&mut client, &KoreMsg::AssignTask {
            task_id: "t1".into(), stage_id: 0, partition_id: 0,
            sql: "SELECT * FROM data".into(), table_name: "data".into(),
            data: block_clone,
        }).await.unwrap();
        let _ = KoreFrame::read(&mut client).await.unwrap();
    }
}
