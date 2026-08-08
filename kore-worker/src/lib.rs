//! KORE Layer 38 — Distributed Worker Node

mod shuffle_store;
mod table_store;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use kore_core::{DataBlock, KoreError};
use kore_net::{KoreFrame, KoreMsg, TaskStats, now_ms};
use kore_shuffle::HashPartitioner;
use kore_sql::executor::KqlContext;
use shuffle_store::ShuffleStore;
use table_store::TableStore;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{interval, Duration};

pub struct Worker {
    pub id:     String,
    pub cores:  usize,
    tables:     TableStore,
    shuffles:   ShuffleStore,
    active:     Arc<AtomicUsize>,
}

impl Worker {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            cores: num_cpus(),
            tables: TableStore::new(),
            shuffles: ShuffleStore::new(),
            active: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub async fn run(&self, coord_addr: &str) -> Result<(), std::io::Error> {
        self.run_with_bind(coord_addr, &kore_net::worker_bind_addr())
            .await
    }

    pub async fn run_with_bind(&self, coord_addr: &str, bind: &str) -> Result<(), std::io::Error> {
        let listener = TcpListener::bind(bind).await?;
        let port = listener.local_addr()?.port();
        let task_addr = kore_net::worker_advertise_addr(port);

        eprintln!(
            "[worker {}] task_addr={task_addr} tables={}",
            self.id,
            self.tables.table_count()
        );

        let mut reg_stream = TcpStream::connect(coord_addr).await?;
        KoreFrame::write(
            &mut reg_stream,
            &KoreMsg::RegisterWorker {
                id: self.id.clone(),
                task_addr: task_addr.clone(),
                cores: self.cores,
                memory_mb: available_mem_mb(),
            },
        )
        .await?;
        let _ack = KoreFrame::read(&mut reg_stream).await?;

        let id = Arc::new(self.id.clone());
        let active = Arc::clone(&self.active);
        let tables = self.tables.clone();
        let shuffles = self.shuffles.clone();

        {
            let id2 = id.clone();
            let ca = coord_addr.to_string();
            let active_hb = Arc::clone(&active);
            tokio::spawn(async move {
                let mut ticker = interval(Duration::from_secs(5));
                loop {
                    ticker.tick().await;
                    if let Ok(mut s) = TcpStream::connect(&ca).await {
                        let _ = KoreFrame::write(
                            &mut s,
                            &KoreMsg::Heartbeat {
                                worker_id: id2.as_ref().clone(),
                                timestamp_ms: now_ms(),
                                active_tasks: active_hb.load(Ordering::Relaxed),
                                free_mem_mb: available_mem_mb(),
                            },
                        )
                        .await;
                    }
                }
            });
        }

        loop {
            let (stream, _peer) = listener.accept().await?;
            let wid = id.clone();
            let tables = tables.clone();
            let shuffles = shuffles.clone();
            let active = Arc::clone(&active);
            tokio::spawn(async move {
                active.fetch_add(1, Ordering::Relaxed);
                if let Err(e) = handle_task_conn(stream, &wid, &tables, &shuffles).await {
                    eprintln!("[worker {wid}] task error: {e}");
                }
                active.fetch_sub(1, Ordering::Relaxed);
            });
        }
    }
}

async fn handle_task_conn(
    mut stream: TcpStream,
    worker_id: &str,
    tables: &TableStore,
    shuffles: &ShuffleStore,
) -> Result<(), std::io::Error> {
    let msg = KoreFrame::read(&mut stream).await?;

    match msg {
        KoreMsg::RegisterTable { table_name, data } => {
            let rows = data.num_rows;
            tables.register(&table_name, data);
            eprintln!("[worker {worker_id}] registered table {table_name} ({rows} rows)");
            KoreFrame::write(&mut stream, &KoreMsg::Pong).await?;
        }
        KoreMsg::AssignTaskLocal {
            task_id,
            partition_id,
            sql,
            table_name,
            ..
        } => {
            let t0 = now_ms();
            // Grab the primary table (must exist).
            let primary = tables.get(&table_name).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("table {table_name} not registered on worker"),
                )
            })?;
            let rows_in = primary.num_rows;

            // Build a full context including all locally registered tables
            // (broadcast tables, other joined tables, etc.). This is what
            // makes broadcast join work: `dim` is broadcast to this worker
            // via a prior RegisterTable and must be visible to the executor.
            let all_tables = tables.snapshot_all();
            respond_sql_multi(
                &mut stream,
                &task_id,
                partition_id,
                &sql,
                &table_name,
                all_tables,
                rows_in,
                t0,
                worker_id,
            )
            .await?;
            // `primary` retained implicitly via snapshot_all(); avoid warning.
            drop(primary);
        }
        KoreMsg::AssignTask {
            task_id,
            partition_id,
            sql,
            table_name,
            data,
            ..
        } => {
            let t0 = now_ms();
            let rows_in = data.num_rows;
            respond_sql(
                &mut stream,
                &task_id,
                partition_id,
                &sql,
                &table_name,
                data,
                rows_in,
                t0,
                worker_id,
            )
            .await?;
        }

        // ── True network shuffle (Phase 9) ────────────────────────────────
        KoreMsg::ShufflePush {
            shuffle_id,
            src_worker,
            partition,
            data,
        } => {
            let rows = data.num_rows;
            shuffles.push(&shuffle_id, partition, data);
            eprintln!("[worker {worker_id}] shuffle push  sh={shuffle_id} part={partition} rows={rows} from={src_worker}");
            KoreFrame::write(&mut stream, &KoreMsg::ShufflePushAck {
                shuffle_id, partition,
            }).await?;
        }
        KoreMsg::ShuffleMapTask {
            task_id,
            shuffle_id,
            stage_id: _,
            map_sql,
            table_name,
            partition_keys,
            n_reducers,
            reducer_addrs,
        } => {
            handle_shuffle_map(
                &mut stream, worker_id, tables,
                &task_id, &shuffle_id, &map_sql, &table_name,
                &partition_keys, n_reducers, &reducer_addrs,
            ).await?;
        }
        KoreMsg::ShuffleReduceTask {
            task_id,
            shuffle_id,
            reduce_partition,
            expected_maps,
            reduce_sql,
            table_name,
        } => {
            handle_shuffle_reduce(
                &mut stream, worker_id, shuffles,
                &task_id, &shuffle_id, reduce_partition, expected_maps,
                &reduce_sql, &table_name,
            ).await?;
        }

        KoreMsg::Ping => {
            KoreFrame::write(&mut stream, &KoreMsg::Pong).await?;
        }
        KoreMsg::Shutdown => {}

        // ── Data locality: worker loads its own shard from S3/disk ────────────
        KoreMsg::LoadShard { table_name, path, filter_sql } => {
            let t0 = now_ms();
            let result = load_shard_from_path(&path, filter_sql.as_deref());
            match result {
                Ok(mut block) => {
                    let rows = block.num_rows;
                    tables.register(&table_name, block);
                    let load_ms = now_ms() - t0;
                    eprintln!("[worker {worker_id}] loaded shard '{table_name}' from {path} ({rows} rows, {load_ms}ms)");
                    KoreFrame::write(&mut stream, &KoreMsg::LoadShardAck { table_name, rows, load_ms }).await?;
                }
                Err(e) => {
                    eprintln!("[worker {worker_id}] LoadShard failed: {path} — {e}");
                    KoreFrame::write(&mut stream, &KoreMsg::LoadShardErr {
                        table_name,
                        message: e.to_string(),
                    }).await?;
                }
            }
        }

        other => {
            eprintln!("[worker {worker_id}] unexpected: {:?}", other);
        }
    }
    Ok(())
}

// ─── Shuffle map ─────────────────────────────────────────────────────────────

async fn handle_shuffle_map(
    stream: &mut TcpStream,
    worker_id: &str,
    tables: &TableStore,
    task_id: &str,
    shuffle_id: &str,
    map_sql: &str,
    table_name: &str,
    partition_keys: &[String],
    n_reducers: usize,
    reducer_addrs: &[String],
) -> Result<(), std::io::Error> {
    let t0 = now_ms();
    if reducer_addrs.len() != n_reducers {
        return send_error(stream, task_id, format!(
            "reducer_addrs len {} != n_reducers {}",
            reducer_addrs.len(), n_reducers
        )).await;
    }

    // Load local slice of the source table.
    let data = match tables.get(table_name) {
        Some(d) => d,
        None => return send_error(stream, task_id, format!(
            "table {table_name} not registered on worker"
        )).await,
    };
    let rows_in = data.num_rows;

    // Run map SQL locally.
    let mapped = match run_sql(map_sql, table_name, data) {
        Ok(b) => b,
        Err(e) => return send_error(stream, task_id, format!("map sql: {e}")).await,
    };

    // Hash-partition the map output by shuffle keys.
    let part = HashPartitioner::new(n_reducers, partition_keys.to_vec());
    let parts = part.partition(&mapped);

    // Push each partition to the reducer that owns it. Empty partitions are
    // skipped — the reducer's `expected_maps` counts map tasks that produced
    // ANY output, so we need to always push (even empty blocks) to keep the
    // barrier count deterministic.
    let mut pushed: Vec<usize> = Vec::with_capacity(n_reducers);
    for (p, block) in parts.into_iter().enumerate() {
        let addr = &reducer_addrs[p];
        let push = KoreMsg::ShufflePush {
            shuffle_id:  shuffle_id.to_string(),
            src_worker:  worker_id.to_string(),
            partition:   p,
            data:        block,
        };
        match push_to_peer(addr, &push).await {
            Ok(()) => pushed.push(p),
            Err(e) => return send_error(stream, task_id, format!(
                "push to reducer {p} at {addr}: {e}"
            )).await,
        }
    }

    let rows_out: usize = pushed.len();
    let elapsed = now_ms() - t0;
    eprintln!("[worker {worker_id}] shuffle map  {task_id} sh={shuffle_id} {rows_in} rows -> {n_reducers} reducers {elapsed}ms");

    KoreFrame::write(stream, &KoreMsg::ShuffleMapAck {
        task_id:    task_id.to_string(),
        shuffle_id: shuffle_id.to_string(),
        partitions_pushed: pushed,
        stats: TaskStats {
            elapsed_ms:    elapsed,
            rows_in,
            rows_out,
            bytes_read:    0,
            bytes_written: 0,
            attempt:       1,
        },
    }).await
}

async fn push_to_peer(addr: &str, msg: &KoreMsg) -> Result<(), std::io::Error> {
    let mut conn = TcpStream::connect(addr).await?;
    KoreFrame::write(&mut conn, msg).await?;
    match KoreFrame::read(&mut conn).await? {
        KoreMsg::ShufflePushAck { .. } => Ok(()),
        other => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("expected ShufflePushAck, got {other:?}"),
        )),
    }
}

// ─── Shuffle reduce ──────────────────────────────────────────────────────────

async fn handle_shuffle_reduce(
    stream: &mut TcpStream,
    worker_id: &str,
    shuffles: &ShuffleStore,
    task_id: &str,
    shuffle_id: &str,
    reduce_partition: usize,
    expected_maps: usize,
    reduce_sql: &str,
    table_name: &str,
) -> Result<(), std::io::Error> {
    let t0 = now_ms();
    // 60s timeout — covers slow LAN links but fails fast if a mapper is dead.
    let timeout = Duration::from_secs(60);
    let blocks = match shuffles
        .wait_and_drain(shuffle_id, reduce_partition, expected_maps, timeout)
        .await
    {
        Some(b) => b,
        None => return send_error(stream, task_id, format!(
            "shuffle reduce timeout: sh={shuffle_id} part={reduce_partition} \
             expected {expected_maps}, have {}",
            shuffles.count(shuffle_id, reduce_partition)
        )).await,
    };

    let merged = match DataBlock::concat(blocks) {
        Ok(b) => b,
        Err(e) => return send_error(stream, task_id, format!("concat: {e}")).await,
    };
    let rows_in = merged.num_rows;

    let result = match run_sql(reduce_sql, table_name, merged) {
        Ok(b) => b,
        Err(e) => return send_error(stream, task_id, format!("reduce sql: {e}")).await,
    };
    let rows_out = result.num_rows;
    let elapsed = now_ms() - t0;
    eprintln!("[worker {worker_id}] shuffle reduce  {task_id} sh={shuffle_id} part={reduce_partition} {rows_in}->{rows_out} {elapsed}ms");

    KoreFrame::write(stream, &KoreMsg::ShuffleReduceResult {
        task_id:          task_id.to_string(),
        shuffle_id:       shuffle_id.to_string(),
        reduce_partition,
        result,
        stats: TaskStats {
            elapsed_ms:    elapsed,
            rows_in,
            rows_out,
            bytes_read:    0,
            bytes_written: 0,
            attempt:       1,
        },
    }).await
}

async fn send_error(
    stream: &mut TcpStream,
    task_id: &str,
    message: String,
) -> Result<(), std::io::Error> {
    KoreFrame::write(stream, &KoreMsg::TaskError {
        task_id: task_id.to_string(),
        message,
        attempt: 1,
    }).await
}

async fn respond_sql(
    stream: &mut TcpStream,
    task_id: &str,
    partition_id: usize,
    sql: &str,
    table_name: &str,
    data: DataBlock,
    rows_in: usize,
    t0: u64,
    worker_id: &str,
) -> Result<(), std::io::Error> {
    match run_sql(sql, table_name, data) {
        Ok(result) => {
            let rows_out = result.num_rows;
            let elapsed = now_ms() - t0;
            eprintln!(
                "[worker {worker_id}] {task_id} part={partition_id} {rows_in}→{rows_out} {elapsed}ms"
            );
            KoreFrame::write(
                stream,
                &KoreMsg::TaskResult {
                    task_id: task_id.to_string(),
                    partition_id,
                    result,
                    stats: TaskStats {
                        elapsed_ms: elapsed,
                        rows_in,
                        rows_out,
                        bytes_read: 0,
                        bytes_written: 0,
                        attempt: 1,
                    },
                },
            )
            .await?;
        }
        Err(e) => {
            KoreFrame::write(
                stream,
                &KoreMsg::TaskError {
                    task_id: task_id.to_string(),
                    message: e.to_string(),
                    attempt: 1,
                },
            )
            .await?;
        }
    }
    Ok(())
}

fn run_sql(sql: &str, table_name: &str, data: DataBlock) -> Result<DataBlock, KoreError> {
    let mut ctx = KqlContext::new();
    ctx.register(table_name, data);
    ctx.query(sql)
}

/// Run SQL with multiple registered tables — used for broadcast joins and
/// any query that references dimensions in addition to the primary table.
fn run_sql_multi(
    sql: &str,
    tables: Vec<(String, DataBlock)>,
) -> Result<DataBlock, KoreError> {
    let mut ctx = KqlContext::new();
    for (name, data) in tables {
        ctx.register(name, data);
    }
    ctx.query(sql)
}

#[allow(clippy::too_many_arguments)]
async fn respond_sql_multi(
    stream: &mut TcpStream,
    task_id: &str,
    partition_id: usize,
    sql: &str,
    _primary_table: &str,
    tables: Vec<(String, DataBlock)>,
    rows_in: usize,
    t0: u64,
    worker_id: &str,
) -> Result<(), std::io::Error> {
    let table_count = tables.len();
    match run_sql_multi(sql, tables) {
        Ok(result) => {
            let rows_out = result.num_rows;
            let elapsed = now_ms() - t0;
            eprintln!(
                "[worker {worker_id}] {task_id} part={partition_id} tables={table_count} {rows_in}->{rows_out} {elapsed}ms"
            );
            KoreFrame::write(
                stream,
                &KoreMsg::TaskResult {
                    task_id: task_id.to_string(),
                    partition_id,
                    result,
                    stats: TaskStats {
                        elapsed_ms: elapsed,
                        rows_in,
                        rows_out,
                        bytes_read: 0,
                        bytes_written: 0,
                        attempt: 1,
                    },
                },
            )
            .await?;
        }
        Err(e) => {
            KoreFrame::write(
                stream,
                &KoreMsg::TaskError {
                    task_id: task_id.to_string(),
                    message: e.to_string(),
                    attempt: 1,
                },
            )
            .await?;
        }
    }
    Ok(())
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

fn available_mem_mb() -> usize {
    512
}

/// Load a DataBlock from a local or remote path.
/// Supports: .parquet, .kore, .csv, .tsv, s3://, gs://, az://
fn load_shard_from_path(path: &str, filter_sql: Option<&str>) -> Result<DataBlock, KoreError> {
    let block = if path.starts_with("s3://") || path.starts_with("gs://") || path.starts_with("az://") {
        // Cloud: download to a temp local path via object-store, then parse
        let local_cache = format!(".kore_cache/{}", path.replace("://", "_").replace('/', "_"));
        let store: &dyn kore_object_store::ObjectStore = &kore_object_store::LocalStore::new(".");
        // For now: treat cloud paths as local paths with prefix stripped (LAN/MinIO setup)
        // Full S3 AWS Sig V4 support: set KORE_S3_ENDPOINT, KORE_S3_KEY, KORE_S3_SECRET
        let bare = path.splitn(4, '/').skip(3).collect::<Vec<_>>().join("/");
        kore_io::CsvReader::new(&bare).read()
            .map_err(|e| KoreError::InvalidArgument(format!("cloud shard '{path}': {e}")))?
    } else {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        match ext.as_str() {
            "parquet" => kore_parquet::ParquetReader::new(path).read()
                .map_err(|e| KoreError::InvalidArgument(format!("parquet: {e}")))?,
            "kore" => kore_store::reader::KoreReader::read_file(std::path::Path::new(path))?,
            "csv" | "tsv" => {
                let delim = if ext == "tsv" { b'\t' } else { b',' };
                kore_io::CsvReader::new(path).delimiter(delim).read()
                    .map_err(|e| KoreError::InvalidArgument(format!("csv: {e}")))?
            }
            _ => return Err(KoreError::InvalidArgument(
                format!("LoadShard: unsupported format '{}' for path '{}'", ext, path)
            )),
        }
    };

    // Apply optional predicate pushdown filter at load time
    if let Some(sql) = filter_sql {
        let filter_sql = format!("SELECT * FROM __shard__ WHERE {sql}");
        let mut ctx = kore_sql::executor::KqlContext::new();
        ctx.register("__shard__", block);
        ctx.query(&filter_sql).map_err(|e| KoreError::InvalidArgument(e.to_string()))
    } else {
        Ok(block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};
    use kore_net::{KoreFrame, KoreMsg};

    async fn fake_coordinator(listener: TcpListener) -> (String, String) {
        let (mut stream, _) = listener.accept().await.unwrap();
        let msg = KoreFrame::read(&mut stream).await.unwrap();
        if let KoreMsg::RegisterWorker { id, task_addr, .. } = msg {
            KoreFrame::write(
                &mut stream,
                &KoreMsg::RegisterAck {
                    worker_id: id.clone(),
                },
            )
            .await
            .unwrap();
            return (id, task_addr);
        }
        panic!("expected RegisterWorker");
    }

    #[tokio::test]
    async fn test_worker_local_table_flow() {
        let coord_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let coord_addr = coord_listener.local_addr().unwrap().to_string();
        let coord_task = tokio::spawn(fake_coordinator(coord_listener));

        let worker = Worker::new("w1");
        let ca = coord_addr.clone();
        tokio::spawn(async move {
            let _ = worker.run(&ca).await;
        });

        let (_, task_addr) = coord_task.await.unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;

        let data = DataBlock {
            num_rows: 5,
            columns: vec![
                Column {
                    name: "id".into(),
                    data: ColumnData::Int64(vec![
                        Some(1),
                        Some(2),
                        Some(3),
                        Some(4),
                        Some(5),
                    ]),
                },
                Column {
                    name: "val".into(),
                    data: ColumnData::Float64(vec![
                        Some(10.0),
                        Some(20.0),
                        Some(30.0),
                        Some(40.0),
                        Some(50.0),
                    ]),
                },
            ],
        };

        let mut reg_conn = TcpStream::connect(&task_addr).await.unwrap();
        KoreFrame::write(
            &mut reg_conn,
            &KoreMsg::RegisterTable {
                table_name: "tbl".into(),
                data,
            },
        )
        .await
        .unwrap();
        let _ = KoreFrame::read(&mut reg_conn).await.unwrap();
        drop(reg_conn);

        let mut task_conn = TcpStream::connect(&task_addr).await.unwrap();
        KoreFrame::write(
            &mut task_conn,
            &KoreMsg::AssignTaskLocal {
                task_id: "t1".into(),
                stage_id: 0,
                partition_id: 0,
                sql: "SELECT * FROM tbl WHERE val > 25".into(),
                table_name: "tbl".into(),
            },
        )
        .await
        .unwrap();

        match KoreFrame::read(&mut task_conn).await.unwrap() {
            KoreMsg::TaskResult { result, .. } => assert_eq!(result.num_rows, 3),
            other => panic!("unexpected: {:?}", other),
        }
    }
}
