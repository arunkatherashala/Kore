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

mod codec;
pub use codec::{WireCodec, WireFormat, BINARY_MAGIC};

/// Internal: encode any `KoreMsg` with the fast binary format (bincode+LZ4).
/// Used by kore-worker's shuffle spill so on-disk and on-wire representations
/// match. Not part of the stable public API.
#[doc(hidden)]
pub fn __codec_encode_shuffle_push(msg: &KoreMsg) -> std::io::Result<Vec<u8>> {
    codec::encode(msg, WireFormat::BINARY_FAST)
}

#[doc(hidden)]
pub fn __codec_decode_shuffle_push(bytes: &[u8]) -> std::io::Result<KoreMsg> {
    codec::decode(bytes)
}

#[cfg(feature = "tls")]
pub mod tls;

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
    /// Coordinator → Worker: store a table partition locally (Phase 3 — no inline ship per task).
    RegisterTable {
        table_name: String,
        data:       DataBlock,
    },
    /// Coordinator → Worker: SQL only — data already registered via RegisterTable.
    AssignTaskLocal {
        task_id:      String,
        stage_id:     usize,
        partition_id: usize,
        sql:          String,
        table_name:   String,
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

    // ── Shuffle (legacy — kept for backward-compat with older peers) ─────────
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

    // ── True network shuffle (Phase 9) ───────────────────────────────────────
    // Model:
    //   1. Coordinator → each map worker:  ShuffleMapTask
    //   2. Worker runs map SQL, hash-partitions on `partition_keys`,
    //      then sends each partition to the reducer worker via ShufflePush.
    //   3. Reducer worker stores into its shuffle_store keyed by (shuffle_id, part).
    //   4. Worker → Coordinator: ShuffleMapAck when its map+push is done.
    //   5. Coordinator → each reduce worker: ShuffleReduceTask (waits for all N maps).
    //   6. Reducer concats all partitions with matching (shuffle_id, part) and runs
    //      the reduce SQL, returning ShuffleReduceResult.
    //
    /// Coordinator → map worker: run map SQL then hash-partition and push.
    ShuffleMapTask {
        task_id:        String,
        shuffle_id:     String,
        stage_id:       usize,
        map_sql:        String,
        table_name:     String,
        partition_keys: Vec<String>,
        n_reducers:     usize,
        /// One address per reduce partition (`reducer_addrs[p]` = who owns
        /// reduce partition `p`). Length must equal `n_reducers`.
        reducer_addrs:  Vec<String>,
    },
    /// Worker → peer worker: here is your shuffle partition. Reducer stores it.
    ShufflePush {
        shuffle_id:  String,
        src_worker:  String,
        partition:   usize,
        data:        DataBlock,
    },
    /// Peer worker → sending worker: partition accepted.
    ShufflePushAck {
        shuffle_id: String,
        partition:  usize,
    },
    /// Worker → Coordinator: map+push phase complete for this task.
    ShuffleMapAck {
        task_id:       String,
        shuffle_id:    String,
        partitions_pushed: Vec<usize>,
        stats:         TaskStats,
    },
    /// Coordinator → reduce worker: gather all pushes for `reduce_partition`
    /// from `expected_maps` map tasks, then run `reduce_sql` over them.
    ShuffleReduceTask {
        task_id:          String,
        shuffle_id:       String,
        reduce_partition: usize,
        expected_maps:    usize,
        reduce_sql:       String,
        table_name:       String,
    },
    /// Worker → Coordinator: reduce complete.
    ShuffleReduceResult {
        task_id:   String,
        shuffle_id: String,
        reduce_partition: usize,
        result:    DataBlock,
        stats:     TaskStats,
    },

    // ── Client queries ───────────────────────────────────────────────────────
    /// Client → Coordinator: run distributed SQL on registered workers.
    SubmitQuery {
        query_id:     String,
        sql:          String,
        table_name:   String,
        data:         DataBlock,
        reduce_sql:   Option<String>,
        /// When true, coordinator registers partitions on workers then sends SQL-only tasks.
        #[serde(default)]
        local_tables: bool,
    },
    /// Coordinator → Client: query succeeded.
    QueryResult {
        query_id: String,
        result:   DataBlock,
    },
    /// Coordinator → Client: query failed.
    QueryError {
        query_id: String,
        message:  String,
    },

    // ── Control ──────────────────────────────────────────────────────────────
    Shutdown,
    Ping,
    Pong,

    // ── Data locality (100K-node scale) ──────────────────────────────────────
    /// Coordinator → Worker: load a data shard from a path (S3/local/parquet/kore).
    /// Worker reads its own partition — coordinator never ships the data.
    /// This is the key message that removes the coordinator bottleneck at scale.
    ///
    /// Format detection by extension:
    ///   .parquet  → kore-parquet reader
    ///   .kore     → kore-store reader
    ///   .csv      → kore-io CSV reader
    ///   s3://...  → kore-object-store (AWS S3 / MinIO)
    LoadShard {
        table_name: String,
        /// Path: local file, s3://bucket/key, gs://bucket/key, az://container/blob
        path:       String,
        /// Optional row filter applied at load time (predicate pushdown).
        filter_sql: Option<String>,
    },
    /// Worker → Coordinator: shard loaded successfully.
    LoadShardAck {
        table_name: String,
        rows:       usize,
        load_ms:    u64,
    },
    /// Worker → Coordinator: shard load failed.
    LoadShardErr {
        table_name: String,
        message:    String,
    },
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
///
/// Since Phase 8, the payload can be either JSON (backward-compat) or a
/// binary format (bincode + optional LZ4). Sender picks via env var
/// `KORE_WIRE=binary|binary-raw|json` (default: `binary` = bincode + LZ4).
/// Readers auto-detect from the first byte of the frame body.
pub struct KoreFrame;

/// Hard safety cap on inbound frame size (protects against malformed peers).
/// 1 GiB — larger than any legitimate DataBlock we send in one shot; use
/// chunked shuffle for anything approaching this.
const MAX_FRAME_BYTES: usize = 1024 * 1024 * 1024;

impl KoreFrame {
    /// Write one message using the default wire format (from `KORE_WIRE` env,
    /// which defaults to `binary` = bincode + LZ4).
    pub async fn write<W>(w: &mut W, msg: &KoreMsg) -> std::io::Result<()>
    where W: tokio::io::AsyncWrite + Unpin
    {
        Self::write_with(w, msg, WireFormat::from_env()).await
    }

    /// Force a specific wire format for this message.
    pub async fn write_with<W>(w: &mut W, msg: &KoreMsg, fmt: WireFormat) -> std::io::Result<()>
    where W: tokio::io::AsyncWrite + Unpin
    {
        let payload = codec::encode(msg, fmt)?;
        let len = (payload.len() as u32).to_be_bytes();
        w.write_all(&len).await?;
        w.write_all(&payload).await?;
        w.flush().await
    }

    /// Read one message. Auto-detects JSON vs binary in the payload body.
    pub async fn read<R>(r: &mut R) -> std::io::Result<KoreMsg>
    where R: tokio::io::AsyncRead + Unpin
    {
        let mut len_buf = [0u8; 4];
        r.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > MAX_FRAME_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("frame too large: {len} bytes (cap {MAX_FRAME_BYTES})"),
            ));
        }
        let mut payload = vec![0u8; len];
        r.read_exact(&mut payload).await?;
        codec::decode(&payload)
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

/// Coordinator bind address (default localhost; set `KORE_COORD_BIND=0.0.0.0:7878` for LAN).
pub fn coord_bind_addr() -> String {
    std::env::var("KORE_COORD_BIND").unwrap_or_else(|_| "127.0.0.1:7878".into())
}

/// Use worker-local tables (register once, SQL-only tasks). Default ON.
pub fn cluster_local_tables() -> bool {
    match std::env::var("KORE_CLUSTER_LOCAL") {
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => false,
        _ => true,
    }
}

/// Use true worker↔worker network shuffle instead of coord-side repartition.
/// Default OFF for now (opt in with `KORE_NET_SHUFFLE=1`) since Phase 9 is
/// newer than the coord-side path and we roll it out behind a flag.
pub fn network_shuffle_enabled() -> bool {
    match std::env::var("KORE_NET_SHUFFLE") {
        Ok(v) if v == "1" || v.eq_ignore_ascii_case("true") => true,
        _ => false,
    }
}

/// Worker task listener bind (default all interfaces).
pub fn worker_bind_addr() -> String {
    std::env::var("KORE_WORKER_BIND").unwrap_or_else(|_| "0.0.0.0:0".into())
}

/// Address workers advertise to coordinator (`host:port`). Falls back to listener local addr.
pub fn worker_advertise_addr(port: u16) -> String {
    if let Ok(host) = std::env::var("KORE_WORKER_ADVERTISE") {
        let host = host.trim().trim_end_matches(':');
        return format!("{host}:{port}");
    }
    format!("127.0.0.1:{port}")
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
