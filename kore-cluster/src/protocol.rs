//! Wire protocol: length-prefixed JSON over TCP.
//!
//! Frame layout: [4 bytes big-endian u32 = body length][N bytes JSON]

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use kore_core::{DataBlock, KoreError};

const MAX_FRAME: usize = 256 * 1024 * 1024;   // 256 MiB safety cap

// ─── Message types ────────────────────────────────────────────────────────────

/// Operations a coordinator can ask a worker to perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    /// Return the shard unchanged (identity / pass-through)
    PassThrough,
    /// Count rows in the shard and return a single-column {"count": N} block
    Count,
    /// Sum a named column
    Sum { column: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPayload {
    pub task_id:   u64,
    pub operation: Operation,
    pub data:      DataBlock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultPayload {
    pub task_id: u64,
    pub data:    DataBlock,
    pub error:   Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KoreMessage {
    Task(TaskPayload),
    Result(ResultPayload),
    Heartbeat { worker_id: String },
    Shutdown,
}

// ─── Framing helpers ──────────────────────────────────────────────────────────

/// Encode a `KoreMessage` as a length-prefixed JSON frame.
pub async fn send_message<W: AsyncWriteExt + Unpin>(
    stream: &mut W,
    msg: &KoreMessage,
) -> Result<(), KoreError> {
    let body = serde_json::to_vec(msg)?;
    if body.len() > MAX_FRAME {
        return Err(KoreError::Cluster("message exceeds MAX_FRAME".into()));
    }
    let len = body.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&body).await?;
    Ok(())
}

/// Read one `KoreMessage` from a length-prefixed JSON frame.
pub async fn recv_message<R: AsyncReadExt + Unpin>(
    stream: &mut R,
) -> Result<KoreMessage, KoreError> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(KoreError::Cluster(format!("frame too large: {} bytes", len)));
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;
    let msg: KoreMessage = serde_json::from_slice(&body)?;
    Ok(msg)
}
