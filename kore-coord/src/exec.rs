//! Distributed execution — local tables, shuffle merge, fault-tolerant dispatch.
//!
//! Two shuffle modes are supported:
//!  * **Coord-side** (default, `execute_local_tables`): workers run map SQL, coord
//!    collects partials, re-partitions on the coord thread, runs reduce SQL.
//!  * **Network shuffle** (`execute_network_shuffle`): workers push shuffle
//!    partitions directly to peer workers (Spark-style). Coordinator is a barrier.

use kore_core::DataBlock;
use kore_fault::{RetryConfig, RetryScheduler};
use kore_net::{KoreFrame, KoreMsg, partition_block};
use kore_shuffle::HashPartitioner;
use kore_sql::executor::KqlContext;
use tokio::net::TcpStream;

use crate::{Coordinator, WorkerInfo};

impl Coordinator {
    /// Execute SQL across workers — local table registration by default.
    ///
    /// Route:
    ///  * If `KORE_NET_SHUFFLE=1` **and** the query has a reduce phase with
    ///    detectable GROUP BY keys, use worker↔worker network shuffle.
    ///  * Else if `KORE_CLUSTER_LOCAL=1` (default), use coord-side local
    ///    tables.
    ///  * Else fall back to the original ephemeral path.
    pub async fn execute_distributed_v2(
        &self,
        sql: &str,
        table_name: &str,
        data: DataBlock,
        reduce_sql: Option<&str>,
    ) -> Result<DataBlock, kore_core::KoreError> {
        if kore_net::network_shuffle_enabled() {
            let keys = extract_group_by_keys(sql);
            if let (Some(rsql), false) = (reduce_sql, keys.is_empty()) {
                return self
                    .execute_network_shuffle(sql, rsql, table_name, data, &keys)
                    .await;
            }
        }
        if kore_net::cluster_local_tables() {
            self.execute_local_tables(sql, table_name, data, reduce_sql)
                .await
        } else {
            self.execute_distributed(sql, table_name, data, reduce_sql)
                .await
        }
    }

    /// Broadcast join (Phase 10).
    ///
    /// Ships `small` (dimension) table to every worker once, partitions
    /// `large` (fact) across workers, then each worker runs `join_sql`
    /// locally against its slice of `large` joined against the full `small`.
    /// Coordinator concats the results.
    ///
    /// This avoids shuffling `large` by the join key — the classic Spark
    /// broadcast-hash-join optimization for star schemas.
    pub async fn execute_broadcast_join(
        &self,
        join_sql:         &str,
        large_table:      &str,
        large_data:       DataBlock,
        small_table:      &str,
        small_data:       DataBlock,
    ) -> Result<DataBlock, kore_core::KoreError> {
        let workers = self.workers.lock().unwrap().clone();
        if workers.is_empty() {
            return Err(kore_core::KoreError::InvalidArgument(
                "no workers registered".into(),
            ));
        }
        let n = workers.len();
        let scheduler = RetryScheduler::new(RetryConfig::default());

        // 1. Broadcast: register the small side on EVERY worker.
        for worker in &workers {
            let addr = worker.task_addr.clone();
            let tname = small_table.to_string();
            let sd = small_data.clone();
            scheduler
                .run_with_retry(|_| {
                    let addr = addr.clone();
                    let tname = tname.clone();
                    let sd = sd.clone();
                    async move { register_table(&addr, &tname, sd).await }
                })
                .await
                .map_err(|e| kore_core::KoreError::InvalidArgument(e.to_string()))?;
        }

        // 2. Partition the large side and register each slice on one worker.
        let partitions = partition_block(large_data, n);
        let part_count = partitions.len();
        for (i, partition) in partitions.into_iter().enumerate() {
            let worker = &workers[i % n];
            let addr = worker.task_addr.clone();
            let tname = large_table.to_string();
            let part = partition.clone();
            scheduler
                .run_with_retry(|_| {
                    let addr = addr.clone();
                    let tname = tname.clone();
                    let part = part.clone();
                    async move { register_table(&addr, &tname, part).await }
                })
                .await
                .map_err(|e| kore_core::KoreError::InvalidArgument(e.to_string()))?;
        }

        // 3. Fan out the join SQL as local tasks. The join runs entirely
        //    on each worker's slice — no shuffle needed.
        let mut handles = Vec::with_capacity(part_count);
        for i in 0..part_count {
            let worker = workers[i % n].clone();
            let sql = join_sql.to_string();
            let tname = large_table.to_string();
            let sched = scheduler.clone();
            handles.push(tokio::spawn(async move {
                sched
                    .run_with_retry(|attempt| {
                        let addr = worker.task_addr.clone();
                        let sql = sql.clone();
                        let tname = tname.clone();
                        let task_id = format!("bcast-part{i}");
                        async move {
                            send_task_local(
                                &addr, &task_id, 0, i, &sql, &tname, attempt,
                            ).await
                        }
                    })
                    .await
            }));
        }

        let mut partials = Vec::with_capacity(part_count);
        for h in handles {
            let block = h
                .await
                .map_err(|e| kore_core::KoreError::InvalidArgument(format!("bcast panic: {e}")))?
                .map_err(|e| kore_core::KoreError::InvalidArgument(format!("bcast retry failed: {e:?}")))?;
            if block.num_rows > 0 { partials.push(block); }
        }
        if partials.is_empty() { return Ok(DataBlock::empty()); }
        DataBlock::concat(partials)
    }

    /// Worker↔worker network shuffle (Phase 9).
    ///
    /// Steps:
    ///   1. `RegisterTable` a slice of `data` on each worker (map inputs).
    ///   2. Send `ShuffleMapTask` to each worker; each map worker
    ///      hash-partitions its output and pushes each partition to the
    ///      reducer that owns it. Coordinator gathers `ShuffleMapAck`.
    ///   3. Send `ShuffleReduceTask` to each reducer; reducer waits until
    ///      it has received `n_maps` blocks for its partition, concats, and
    ///      runs `reduce_sql`.
    ///   4. Coordinator concats reduce results (row-parallel), returns.
    pub async fn execute_network_shuffle(
        &self,
        map_sql:    &str,
        reduce_sql: &str,
        table_name: &str,
        data:       DataBlock,
        keys:       &[String],
    ) -> Result<DataBlock, kore_core::KoreError> {
        let workers = self.workers.lock().unwrap().clone();
        if workers.is_empty() {
            return Err(kore_core::KoreError::InvalidArgument(
                "no workers registered".into(),
            ));
        }
        let n = workers.len();
        let partitions = partition_block(data, n);
        let n_maps = partitions.len();
        let n_reducers = n; // 1 reducer per worker for now — natural placement.
        let reducer_addrs: Vec<String> =
            workers.iter().map(|w| w.task_addr.clone()).collect();
        let shuffle_id = format!("sh-{}", kore_net::now_ms());

        // ─ Step 1: register map input partitions on each worker.
        let scheduler = RetryScheduler::new(RetryConfig::default());
        for (i, partition) in partitions.into_iter().enumerate() {
            let worker = &workers[i % n];
            let addr = worker.task_addr.clone();
            let tname = table_name.to_string();
            let part = partition.clone();
            scheduler
                .run_with_retry(|_| {
                    let addr = addr.clone();
                    let tname = tname.clone();
                    let part = part.clone();
                    async move { register_table(&addr, &tname, part).await }
                })
                .await
                .map_err(|e| kore_core::KoreError::InvalidArgument(e.to_string()))?;
        }

        // ─ Step 2: fan out map tasks. Each returns a ShuffleMapAck.
        let mut map_handles = Vec::with_capacity(n_maps);
        for i in 0..n_maps {
            let worker = workers[i % n].clone();
            let msg = KoreMsg::ShuffleMapTask {
                task_id:        format!("map-{shuffle_id}-{i}"),
                shuffle_id:     shuffle_id.clone(),
                stage_id:       0,
                map_sql:        map_sql.to_string(),
                table_name:     table_name.to_string(),
                partition_keys: keys.to_vec(),
                n_reducers,
                reducer_addrs:  reducer_addrs.clone(),
            };
            map_handles.push(tokio::spawn(async move {
                send_and_wait_ack(&worker.task_addr, msg).await
            }));
        }
        for h in map_handles {
            h.await
                .map_err(|e| kore_core::KoreError::InvalidArgument(format!("map panic: {e}")))?
                .map_err(|e| kore_core::KoreError::InvalidArgument(format!("map error: {e}")))?;
        }

        // ─ Step 3: fan out reduce tasks.
        let mut reduce_handles = Vec::with_capacity(n_reducers);
        for p in 0..n_reducers {
            let reducer = workers[p % n].clone();
            let sh = shuffle_id.clone();
            let rsql = reduce_sql.to_string();
            let tname = table_name.to_string();
            reduce_handles.push(tokio::spawn(async move {
                send_reduce_task(&reducer, &sh, p, n_maps, &rsql, &tname).await
            }));
        }
        let mut reduce_blocks = Vec::with_capacity(n_reducers);
        for h in reduce_handles {
            let block = h
                .await
                .map_err(|e| kore_core::KoreError::InvalidArgument(format!("reduce panic: {e}")))?
                .map_err(|e| kore_core::KoreError::InvalidArgument(format!("reduce error: {e}")))?;
            if block.num_rows > 0 { reduce_blocks.push(block); }
        }

        if reduce_blocks.is_empty() { return Ok(DataBlock::empty()); }
        DataBlock::concat(reduce_blocks)
    }

    async fn execute_local_tables(
        &self,
        sql: &str,
        table_name: &str,
        data: DataBlock,
        reduce_sql: Option<&str>,
    ) -> Result<DataBlock, kore_core::KoreError> {
        let workers = self.workers.lock().unwrap().clone();
        if workers.is_empty() {
            return Err(kore_core::KoreError::InvalidArgument(
                "no workers registered".into(),
            ));
        }

        let n = workers.len();
        let partitions = partition_block(data, n);
        let part_count = partitions.len();
        let scheduler = RetryScheduler::new(RetryConfig::default());

        for (i, partition) in partitions.into_iter().enumerate() {
            let worker = &workers[i % n];
            let addr = worker.task_addr.clone();
            let tname = table_name.to_string();
            let part = partition.clone();
            scheduler
                .run_with_retry(|_| {
                    let addr = addr.clone();
                    let tname = tname.clone();
                    let part = part.clone();
                    async move { register_table(&addr, &tname, part).await }
                })
                .await
                .map_err(|e| kore_core::KoreError::InvalidArgument(e.to_string()))?;
        }

        let mut handles = Vec::new();
        for i in 0..part_count {
            let worker = workers[i % n].clone();
            let sql_copy = sql.to_string();
            let tname = table_name.to_string();
            let task_id = format!("local-part{i}");
            let sched = scheduler.clone();
            handles.push(tokio::spawn(async move {
                sched
                    .run_with_retry(|attempt| {
                        let addr = worker.task_addr.clone();
                        let sql_copy = sql_copy.clone();
                        let tname = tname.clone();
                        let task_id = task_id.clone();
                        async move {
                            send_task_local(
                                &addr,
                                &task_id,
                                0,
                                i,
                                &sql_copy,
                                &tname,
                                attempt,
                            )
                            .await
                        }
                    })
                    .await
            }));
        }

        let mut partials: Vec<DataBlock> = Vec::new();
        for h in handles {
            let result = h
                .await
                .map_err(|e| kore_core::KoreError::InvalidArgument(format!("task panic: {e}")))?
                .map_err(|e| kore_core::KoreError::InvalidArgument(format!("retry failed: {e:?}")))?;
            partials.push(result);
        }

        let merged = if reduce_sql.is_some() {
            shuffle_merge_partials(partials, sql)?
        } else {
            DataBlock::concat(partials)?
        };

        if let Some(rsql) = reduce_sql {
            let mut ctx = KqlContext::new();
            ctx.register("merged", merged);
            return ctx.query(rsql);
        }
        Ok(merged)
    }
}

/// Phase 4: hash-repartition partial aggregates before coordinator reduce.
fn shuffle_merge_partials(
    partials: Vec<DataBlock>,
    sql: &str,
) -> Result<DataBlock, kore_core::KoreError> {
    let merged = DataBlock::concat(partials)?;
    let lower = sql.to_lowercase();
    if !lower.contains("group by") {
        return Ok(merged);
    }
    let key_col = extract_group_by_key(&lower);
    let keys = if key_col.is_empty() {
        vec![]
    } else {
        vec![key_col]
    };
    if keys.is_empty() {
        return Ok(merged);
    }
    let np = (merged.num_rows.max(1)).min(32);
    let part = HashPartitioner::new(np, keys);
    let parts = part.partition(&merged);
    HashPartitioner::merge(parts)
}

fn extract_group_by_key(lower_sql: &str) -> String {
    let Some(pos) = lower_sql.rfind("group by") else {
        return String::new();
    };
    let tail = &lower_sql[pos + 8..];
    tail.split(|c: char| c == ',' || c == ' ' || c == '\n')
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .trim()
        .to_string()
}

async fn register_table(
    addr: &str,
    table_name: &str,
    data: DataBlock,
) -> Result<(), std::io::Error> {
    let mut conn = TcpStream::connect(addr).await?;
    KoreFrame::write(
        &mut conn,
        &KoreMsg::RegisterTable {
            table_name: table_name.to_string(),
            data,
        },
    )
    .await?;
    match KoreFrame::read(&mut conn).await? {
        KoreMsg::Pong | KoreMsg::RegisterAck { .. } => Ok(()),
        KoreMsg::TaskError { message, .. } => Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            message,
        )),
        _ => Ok(()),
    }
}

async fn send_and_wait_ack(
    addr: &str,
    msg: KoreMsg,
) -> Result<(), std::io::Error> {
    let mut conn = TcpStream::connect(addr).await?;
    KoreFrame::write(&mut conn, &msg).await?;
    match KoreFrame::read(&mut conn).await? {
        KoreMsg::ShuffleMapAck { .. } => Ok(()),
        KoreMsg::TaskError { message, .. } => Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            message,
        )),
        other => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("expected ShuffleMapAck, got {other:?}"),
        )),
    }
}

async fn send_reduce_task(
    reducer: &WorkerInfo,
    shuffle_id: &str,
    reduce_partition: usize,
    expected_maps: usize,
    reduce_sql: &str,
    table_name: &str,
) -> Result<DataBlock, std::io::Error> {
    let mut conn = TcpStream::connect(&reducer.task_addr).await?;
    KoreFrame::write(&mut conn, &KoreMsg::ShuffleReduceTask {
        task_id:          format!("reduce-{shuffle_id}-{reduce_partition}"),
        shuffle_id:       shuffle_id.to_string(),
        reduce_partition,
        expected_maps,
        reduce_sql:       reduce_sql.to_string(),
        table_name:       table_name.to_string(),
    }).await?;
    match KoreFrame::read(&mut conn).await? {
        KoreMsg::ShuffleReduceResult { result, .. } => Ok(result),
        KoreMsg::TaskError { message, .. } => Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            message,
        )),
        other => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("expected ShuffleReduceResult, got {other:?}"),
        )),
    }
}

/// Extract `GROUP BY a, b, c` keys from a SQL string. Best-effort textual
/// parse — used to decide whether network shuffle can safely partition by
/// these columns. Returns empty when no GROUP BY is present.
pub(crate) fn extract_group_by_keys(sql: &str) -> Vec<String> {
    let lower = sql.to_lowercase();
    let Some(pos) = lower.rfind("group by") else { return vec![]; };
    let tail_start = pos + "group by".len();
    let tail_lower = &lower[tail_start..];
    // Cut at ORDER BY / LIMIT / HAVING to avoid picking up trailing tokens.
    let cut = ["order by", "limit", "having"]
        .iter()
        .filter_map(|kw| tail_lower.find(kw))
        .min()
        .unwrap_or(tail_lower.len());
    let key_slice = &sql[tail_start..tail_start + cut];
    key_slice
        .split(',')
        .map(|s| s.trim().split_whitespace().next().unwrap_or("").trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub async fn send_task_local(
    addr: &str,
    task_id: &str,
    stage_id: usize,
    partition_id: usize,
    sql: &str,
    table_name: &str,
    attempt: usize,
) -> Result<DataBlock, std::io::Error> {
    let mut conn = TcpStream::connect(addr).await?;
    KoreFrame::write(
        &mut conn,
        &KoreMsg::AssignTaskLocal {
            task_id: task_id.to_string(),
            stage_id,
            partition_id,
            sql: sql.to_string(),
            table_name: table_name.to_string(),
        },
    )
    .await?;

    match KoreFrame::read(&mut conn).await? {
        KoreMsg::TaskResult { result, .. } => Ok(result),
        KoreMsg::TaskError { message, .. } => Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("attempt {attempt}: {message}"),
        )),
        other => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unexpected: {:?}", other),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_group_by_key() {
        assert_eq!(
            extract_group_by_key("select region, sum(x) from t group by region"),
            "region"
        );
    }
}
