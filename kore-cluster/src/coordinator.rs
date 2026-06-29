//! Coordinator — distributes DataBlock shards to Workers and merges results.

use std::sync::atomic::{AtomicU64, Ordering};
use tokio::net::TcpStream;
use kore_core::{DataBlock, KoreError};
use crate::protocol::{
    recv_message, send_message, KoreMessage, Operation, TaskPayload,
};

static TASK_COUNTER: AtomicU64 = AtomicU64::new(1);

pub struct Coordinator {
    worker_addrs: Vec<String>,
}

impl Coordinator {
    pub fn new(worker_addrs: Vec<&str>) -> Self {
        Self { worker_addrs: worker_addrs.into_iter().map(|s| s.into()).collect() }
    }

    /// Distribute `block` across workers in equal shards, apply `operation` on
    /// each shard, then concatenate the partial results.
    pub async fn distribute(
        &self,
        block:     &DataBlock,
        operation: Operation,
    ) -> Result<DataBlock, KoreError> {
        if self.worker_addrs.is_empty() {
            return Err(KoreError::Cluster("no workers registered".into()));
        }
        let n_workers  = self.worker_addrs.len();
        let chunk_size = (block.num_rows / n_workers).max(1);

        let mut handles = Vec::new();
        let mut start = 0usize;

        for (worker_idx, addr) in self.worker_addrs.iter().enumerate() {
            if start >= block.num_rows { break; }
            let end     = if worker_idx == n_workers - 1 { block.num_rows } else { (start + chunk_size).min(block.num_rows) };
            let indices: Vec<usize> = (start..end).collect();
            let shard   = block.select_rows(&indices);
            let addr    = addr.clone();
            let op      = operation.clone();

            handles.push(tokio::spawn(async move {
                send_shard_to_worker(&addr, shard, op).await
            }));
            start = end;
        }

        let mut results: Vec<DataBlock> = Vec::new();
        for h in handles {
            match h.await {
                Ok(Ok(d))  => results.push(d),
                Ok(Err(e)) => return Err(e),
                Err(e)     => return Err(KoreError::Cluster(format!("task panic: {}", e))),
            }
        }

        DataBlock::concat(results)
    }

    /// Broadcast `block` to every worker (each processes the full block),
    /// collect all results, concatenate.  Useful for fan-out operations.
    pub async fn broadcast(
        &self,
        block:     &DataBlock,
        operation: Operation,
    ) -> Result<Vec<DataBlock>, KoreError> {
        let mut handles = Vec::new();
        for addr in &self.worker_addrs {
            let shard = block.clone();
            let addr  = addr.clone();
            let op    = operation.clone();
            handles.push(tokio::spawn(async move {
                send_shard_to_worker(&addr, shard, op).await
            }));
        }
        let mut results = Vec::new();
        for h in handles {
            results.push(h.await.map_err(|e| KoreError::Cluster(e.to_string()))??);
        }
        Ok(results)
    }
}

async fn send_shard_to_worker(
    addr:      &str,
    shard:     DataBlock,
    operation: Operation,
) -> Result<DataBlock, KoreError> {
    let mut stream = TcpStream::connect(addr).await?;
    let task_id    = TASK_COUNTER.fetch_add(1, Ordering::Relaxed);
    let task       = KoreMessage::Task(TaskPayload { task_id, operation, data: shard });
    send_message(&mut stream, &task).await?;

    match recv_message(&mut stream).await? {
        KoreMessage::Result(r) => {
            if let Some(err) = r.error {
                Err(KoreError::Cluster(err))
            } else {
                Ok(r.data)
            }
        }
        other => Err(KoreError::Cluster(format!("unexpected response: {:?}", other))),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, DataBlock};
    use crate::Worker;

    async fn start_local_worker(port: u16) {
        let w = Worker::new(&format!("w{}", port), &format!("127.0.0.1:{}", port));
        w.start();
        // Brief pause to allow the listener to bind
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn distribute_count() {
        start_local_worker(19800).await;
        start_local_worker(19801).await;

        let block = DataBlock::new(vec![
            Column::int64("id", (0..100i64).map(Some).collect()),
        ]).unwrap();

        let coord = Coordinator::new(vec!["127.0.0.1:19800", "127.0.0.1:19801"]);
        let result = coord.distribute(&block, Operation::Count).await.unwrap();

        // Two partial Count results merged — total count columns = 2
        assert_eq!(result.num_rows, 2);
        let counts: i64 = (0..result.num_rows)
            .filter_map(|i| result.column("count")?.data.get_value(i).as_f64().map(|v| v as i64))
            .sum();
        assert_eq!(counts, 100);
    }

    #[tokio::test]
    async fn distribute_passthrough() {
        start_local_worker(19810).await;
        start_local_worker(19811).await;

        let block = DataBlock::new(vec![
            Column::float64("v", (0..40).map(|i| Some(i as f64)).collect()),
        ]).unwrap();

        let coord  = Coordinator::new(vec!["127.0.0.1:19810", "127.0.0.1:19811"]);
        let result = coord.distribute(&block, Operation::PassThrough).await.unwrap();
        assert_eq!(result.num_rows, 40);
    }
}
