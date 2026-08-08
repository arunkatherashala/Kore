//! KORE Layer 40 — Fault Tolerance
//!
//! Provides:
//! - **Lineage DAG** — tracks how each DataBlock was produced so lost data
//!   can be recomputed from its sources (mirrors Spark's RDD lineage).
//! - **RetryScheduler** — exponential-backoff task retry for failed workers.
//! - **Checkpoint** — periodic disk snapshots to truncate lineage chains.
//! - **SpeculativeExec** — launch backup tasks for stragglers.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use kore_core::{DataBlock, KoreError};

// ─── Operation kinds ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OpKind {
    /// A loaded base table (source data — cannot be recomputed, must be cached).
    Source { table_name: String },
    /// A SQL filter/project over one parent.
    Filter { sql: String, table_name: String },
    /// An aggregation over one parent.
    Aggregate { sql: String, table_name: String },
    /// A join of two parents.
    Join { left_id: String, right_id: String, sql: String },
    /// A shuffle (re-partition).
    Shuffle { key: String },
    /// A union of N parents.
    Union,
}

// ─── Lineage node ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LineageNode {
    pub id:      String,
    pub op:      OpKind,
    /// IDs of parent nodes (inputs to this operation).
    pub parents: Vec<String>,
    /// Materialised result (None if evicted or not yet computed).
    pub result:  Option<DataBlock>,
}

// ─── Lineage DAG ──────────────────────────────────────────────────────────────

/// Directed Acyclic Graph of computation stages.
pub struct LineageDAG {
    nodes: HashMap<String, LineageNode>,
    next_id: usize,
}

impl LineageDAG {
    pub fn new() -> Self { Self { nodes: HashMap::new(), next_id: 0 } }

    /// Register a new computation stage.  Returns the assigned node id.
    pub fn add_node(&mut self, op: OpKind, parents: Vec<String>) -> String {
        let id = format!("stage-{}", self.next_id);
        self.next_id += 1;
        self.nodes.insert(id.clone(), LineageNode { id: id.clone(), op, parents, result: None });
        id
    }

    /// Store a materialised result for node `id`.
    pub fn set_result(&mut self, id: &str, data: DataBlock) {
        if let Some(n) = self.nodes.get_mut(id) { n.result = Some(data); }
    }

    /// Evict the cached result for node `id` to free memory.
    pub fn evict(&mut self, id: &str) {
        if let Some(n) = self.nodes.get_mut(id) { n.result = None; }
    }

    /// Get the cached result for node `id`, if present.
    pub fn get(&self, id: &str) -> Option<&DataBlock> {
        self.nodes.get(id)?.result.as_ref()
    }

    /// Recompute lost data for `id` by replaying its lineage.
    ///
    /// Uses DFS: if a parent's result is also missing, recursively recompute it.
    /// Source nodes cannot be recomputed — they must be cached.
    pub fn recompute<F>(&mut self, id: &str, exec: &mut F) -> Result<DataBlock, KoreError>
    where
        F: FnMut(&OpKind, Vec<DataBlock>) -> Result<DataBlock, KoreError>,
    {
        // Already cached?
        if let Some(n) = self.nodes.get(id) {
            if let Some(r) = &n.result { return Ok(r.clone()); }
        }

        let node = self.nodes.get(id)
            .ok_or_else(|| KoreError::InvalidArgument(format!("unknown lineage node: {id}")))?
            .clone();

        // Sources must be cached
        if matches!(&node.op, OpKind::Source { .. }) {
            return Err(KoreError::InvalidArgument(
                format!("source node '{}' was evicted and cannot be recomputed", id)
            ));
        }

        // Recursively ensure all parents are available
        let mut parent_data: Vec<DataBlock> = Vec::new();
        for pid in &node.parents {
            let pdata = self.recompute(pid, exec)?;
            parent_data.push(pdata);
        }

        // Execute the operation
        let result = exec(&node.op, parent_data)?;
        self.nodes.get_mut(id).unwrap().result = Some(result.clone());
        Ok(result)
    }

    /// Return the topological order (leaves → roots).
    pub fn topo_order(&self) -> Vec<String> {
        let mut visited = std::collections::HashSet::new();
        let mut order   = Vec::new();
        for id in self.nodes.keys() {
            self.dfs(id, &mut visited, &mut order);
        }
        order
    }

    fn dfs(&self, id: &str, visited: &mut std::collections::HashSet<String>, order: &mut Vec<String>) {
        if visited.contains(id) { return; }
        visited.insert(id.to_string());
        if let Some(node) = self.nodes.get(id) {
            for p in &node.parents { self.dfs(p, visited, order); }
        }
        order.push(id.to_string());
    }
}

impl Default for LineageDAG { fn default() -> Self { Self::new() } }

// ─── Retry scheduler ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts:   usize,
    pub base_delay_ms:  u64,
    pub max_delay_ms:   u64,
    pub jitter:         bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self { max_attempts: 3, base_delay_ms: 100, max_delay_ms: 30_000, jitter: true }
    }
}

pub struct RetryScheduler {
    pub config: RetryConfig,
}

impl Clone for RetryScheduler {
    fn clone(&self) -> Self {
        Self { config: self.config.clone() }
    }
}

impl RetryScheduler {
    pub fn new(config: RetryConfig) -> Self { Self { config } }

    /// Delay (ms) before the n-th retry (1-based).
    pub fn delay_ms(&self, attempt: usize) -> u64 {
        let exp = 2u64.pow((attempt - 1) as u32);
        let delay = (self.config.base_delay_ms * exp).min(self.config.max_delay_ms);
        if self.config.jitter {
            // Simple jitter: ±25%
            let spread = delay / 4;
            if spread == 0 { delay } else { delay - spread + (kore_hash(attempt as u64) % (spread * 2)) }
        } else {
            delay
        }
    }

    /// Execute a fallible async closure with retries.
    ///
    /// Returns `Ok(result)` on first success, or the last error.
    pub async fn run_with_retry<F, Fut, T, E>(&self, mut f: F) -> Result<T, E>
    where
        F: FnMut(usize) -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        E: std::fmt::Debug,
    {
        let mut last_err = None;
        for attempt in 1..=self.config.max_attempts {
            match f(attempt).await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    eprintln!("[retry] attempt {attempt}/{} failed: {:?}", self.config.max_attempts, e);
                    last_err = Some(e);
                    if attempt < self.config.max_attempts {
                        let delay = self.delay_ms(attempt);
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                    }
                }
            }
        }
        Err(last_err.unwrap())
    }
}

// ─── Checkpoint ──────────────────────────────────────────────────────────────

/// Writes a DataBlock snapshot to disk so lineage can be truncated there.
pub struct Checkpoint {
    dir: std::path::PathBuf,
}

impl Checkpoint {
    pub fn new(dir: impl Into<std::path::PathBuf>) -> Self {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).ok();
        Self { dir }
    }

    pub fn save(&self, stage_id: &str, block: &DataBlock) -> Result<(), KoreError> {
        let path = self.dir.join(format!("{stage_id}.json"));
        let json = serde_json::to_string(block)
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;
        std::fs::write(&path, json)
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))
    }

    pub fn load(&self, stage_id: &str) -> Result<DataBlock, KoreError> {
        let path = self.dir.join(format!("{stage_id}.json"));
        let json = std::fs::read_to_string(&path)
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))?;
        serde_json::from_str(&json)
            .map_err(|e| KoreError::InvalidArgument(e.to_string()))
    }
}

// ─── Partition-level lineage (Phase 18) ──────────────────────────────────────

use std::sync::{Arc, Mutex};

/// One in-flight partition tracked by the coordinator.
///
/// The existing coarse-grained `LineageDAG` records whole-stage lineage — it
/// answers "how would I rebuild stage S from its parents?".  This finer-grained
/// tracker answers a different question that comes up on real clusters:
/// **when worker W dies mid-query, which partitions were in flight on W, and
/// how do I re-dispatch them to a survivor?**
///
/// A `PartitionRecord` carries just enough state to replay one task on a
/// different worker:
///
///  * `partition_idx` — the coordinator's partition ordering (0..n).
///  * `task_id` — unique task ID (used by the retry scheduler + metrics).
///  * `worker_id` — the worker this partition was originally assigned to.
///  * `stage_id` — logical stage this partition belongs to (map / reduce /
///    broadcast-join / etc.).  A single query may have several.
///  * `sql` — the SQL fragment the worker executed for this partition.
///  * `table_name` — the coordinator-side table name the partition was
///    registered under (workers key their local tables by this).
///  * `state` — `Pending` before dispatch, `Completed` after ack.
///
/// The tracker itself is a `Send + Sync` map — the coordinator's shared state
/// wraps it in `Arc<Mutex<TaskLineage>>` so tokio tasks can update it.
#[derive(Debug, Clone)]
pub struct PartitionRecord {
    pub partition_idx: usize,
    pub task_id:       String,
    pub worker_id:     String,
    pub stage_id:      String,
    pub sql:           String,
    pub table_name:    String,
    pub state:         PartitionState,
    pub started_at_ms: u64,
    pub finished_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionState {
    Pending,
    Completed,
    /// Set by `mark_worker_lost` when the assigned worker died before ack.
    /// The coordinator's recovery loop picks these up and re-dispatches them
    /// to a surviving worker.
    LostReadyToRetry,
}

/// Coordinator-side per-partition lineage tracker.
///
/// Cheap to clone (`Arc` internal); safe to share across spawned tokio tasks.
#[derive(Debug, Clone, Default)]
pub struct TaskLineage {
    inner: Arc<Mutex<TaskLineageInner>>,
}

#[derive(Debug, Default)]
struct TaskLineageInner {
    /// Keyed by `task_id` for O(1) lookup on ack.
    records: HashMap<String, PartitionRecord>,
}

impl TaskLineage {
    pub fn new() -> Self { Self::default() }

    /// Record a new partition dispatch. Panics if `task_id` already exists —
    /// task IDs must be unique across a query.
    pub fn record(&self, rec: PartitionRecord) {
        let mut inner = self.inner.lock().unwrap();
        assert!(
            !inner.records.contains_key(&rec.task_id),
            "duplicate task_id in lineage: {}", rec.task_id,
        );
        inner.records.insert(rec.task_id.clone(), rec);
    }

    /// Mark a task as completed (worker ack'd).  Silently no-op if unknown.
    pub fn mark_completed(&self, task_id: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(r) = inner.records.get_mut(task_id) {
            r.state = PartitionState::Completed;
            r.finished_at_ms = Some(now_ms_local());
        }
    }

    /// Mark every pending partition on `worker_id` as ready for retry.
    /// Returns the records that need re-dispatch — the coordinator's recovery
    /// path re-runs them on a surviving worker.
    pub fn mark_worker_lost(&self, worker_id: &str) -> Vec<PartitionRecord> {
        let mut inner = self.inner.lock().unwrap();
        let mut lost = Vec::new();
        for rec in inner.records.values_mut() {
            if rec.worker_id == worker_id && rec.state == PartitionState::Pending {
                rec.state = PartitionState::LostReadyToRetry;
                lost.push(rec.clone());
            }
        }
        lost
    }

    /// Reassign a lost partition to a new worker; the coordinator has already
    /// dispatched to the new worker and just needs to update the tracker.
    pub fn reassign(&self, task_id: &str, new_worker_id: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(r) = inner.records.get_mut(task_id) {
            r.worker_id     = new_worker_id.to_string();
            r.state         = PartitionState::Pending;
            r.started_at_ms = now_ms_local();
            r.finished_at_ms = None;
        }
    }

    /// Snapshot every record — for tests, EXPLAIN ANALYZE, and debugging.
    pub fn snapshot(&self) -> Vec<PartitionRecord> {
        let inner = self.inner.lock().unwrap();
        inner.records.values().cloned().collect()
    }

    /// Snapshot only the pending partitions.
    pub fn pending(&self) -> Vec<PartitionRecord> {
        self.snapshot().into_iter()
            .filter(|r| r.state == PartitionState::Pending)
            .collect()
    }

    /// Snapshot only the completed partitions.
    pub fn completed(&self) -> Vec<PartitionRecord> {
        self.snapshot().into_iter()
            .filter(|r| r.state == PartitionState::Completed)
            .collect()
    }

    /// How many partitions are currently pending?  Useful for the coord's
    /// "wait for stage barrier" logic.
    pub fn pending_count(&self) -> usize {
        self.inner.lock().unwrap().records.values()
            .filter(|r| r.state == PartitionState::Pending).count()
    }
}

/// Best-effort millisecond clock — shared between coord dispatch and lineage
/// tracking so timings correlate.
fn now_ms_local() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ─── Speculative execution ────────────────────────────────────────────────────

/// Tracks task durations and identifies stragglers.
pub struct SpeculativeTracker {
    durations: Vec<u64>,   // elapsed ms per completed task
    threshold: f64,        // launch backup if task exceeds median × threshold
}

impl SpeculativeTracker {
    pub fn new(threshold: f64) -> Self { Self { durations: Vec::new(), threshold } }

    pub fn record(&mut self, elapsed_ms: u64) { self.durations.push(elapsed_ms); }

    pub fn median_ms(&self) -> Option<u64> {
        if self.durations.is_empty() { return None; }
        let mut sorted = self.durations.clone();
        sorted.sort_unstable();
        Some(sorted[sorted.len() / 2])
    }

    /// Returns `true` if a task running for `elapsed_ms` should trigger
    /// speculative execution.
    pub fn is_straggler(&self, elapsed_ms: u64) -> bool {
        self.median_ms()
            .map(|m| elapsed_ms as f64 > m as f64 * self.threshold)
            .unwrap_or(false)
    }
}

/// Race a primary task against a delayed backup: return the first successful
/// result; abort the loser. This is the Spark speculative-execution primitive.
///
/// Semantics:
///  * Launches the primary task immediately.
///  * After `backup_after_ms`, if the primary is still running, launches the
///    backup task (must be idempotent — same output either way).
///  * Returns whichever completes first with `Ok(_)`; the other is aborted.
///  * If one fails and the other succeeds, returns the successful one.
///  * If both fail, returns the primary's error.
///
/// The primary and backup should target different workers so a stalled worker
/// doesn't block both attempts.
pub async fn run_with_speculation<T, E, F1, F2, Fut1, Fut2>(
    primary: F1,
    backup:  F2,
    backup_after_ms: u64,
) -> Result<T, E>
where
    T: Send + 'static,
    E: Send + 'static + std::fmt::Debug,
    F1: FnOnce() -> Fut1 + Send + 'static,
    F2: FnOnce() -> Fut2 + Send + 'static,
    Fut1: std::future::Future<Output = Result<T, E>> + Send + 'static,
    Fut2: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    let mut primary_h = tokio::spawn(async move { primary().await });
    let sleep_dur = std::time::Duration::from_millis(backup_after_ms);

    // Wait either for primary to finish, or the speculation timer.
    tokio::select! {
        biased;
        r = &mut primary_h => {
            return match r {
                Ok(v) => v,
                Err(join_err) => panic!("primary panic: {join_err}"),
            };
        }
        _ = tokio::time::sleep(sleep_dur) => {
            // Primary is a straggler → launch backup and race.
        }
    }

    let mut backup_h = tokio::spawn(async move { backup().await });
    // Now race primary vs backup. First Ok wins; both Err → primary's Err.
    let mut primary_err: Option<E> = None;
    loop {
        tokio::select! {
            r = &mut primary_h, if !primary_h.is_finished() => {
                match r {
                    Ok(Ok(v)) => { backup_h.abort(); return Ok(v); }
                    Ok(Err(e)) => primary_err = Some(e),
                    Err(_) => primary_err = None,
                }
            }
            r = &mut backup_h, if !backup_h.is_finished() => {
                match r {
                    Ok(Ok(v)) => { primary_h.abort(); return Ok(v); }
                    Ok(Err(e)) => {
                        if let Some(pe) = primary_err {
                            return Err(pe);
                        } else {
                            return Err(e);
                        }
                    }
                    Err(_) => {
                        if let Some(pe) = primary_err {
                            return Err(pe);
                        }
                        // Both futures gone with no usable error — fabricate an
                        // error propagation path by continuing (extremely rare).
                    }
                }
            }
            else => {
                // Both finished with no Ok — should be caught above.
                if let Some(pe) = primary_err {
                    return Err(pe);
                }
                unreachable!("both tasks finished but no primary_err captured");
            }
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn kore_hash(v: u64) -> u64 {
    let mut h = v ^ 0xcbf2_9ce4_8422_2325;
    h = h.wrapping_mul(0x0000_0100_0000_01b3);
    h
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn make_block(n: usize) -> DataBlock {
        DataBlock {
            num_rows: n,
            columns: vec![
                Column { name: "v".into(),
                    data: ColumnData::Int64((0..n).map(|i| Some(i as i64)).collect()) },
            ],
        }
    }

    #[test]
    fn test_lineage_dag_build_and_evict() {
        let mut dag = LineageDAG::new();
        let src_id  = dag.add_node(OpKind::Source { table_name: "raw".into() }, vec![]);
        let flt_id  = dag.add_node(
            OpKind::Filter { sql: "SELECT * FROM raw WHERE v > 2".into(), table_name: "raw".into() },
            vec![src_id.clone()],
        );

        dag.set_result(&src_id, make_block(5));
        dag.set_result(&flt_id, make_block(2));

        assert!(dag.get(&src_id).is_some());
        dag.evict(&src_id);
        assert!(dag.get(&src_id).is_none());
        assert!(dag.get(&flt_id).is_some()); // filter still cached
    }

    #[test]
    fn test_lineage_recompute() {
        let mut dag = LineageDAG::new();
        let src_id  = dag.add_node(OpKind::Source { table_name: "data".into() }, vec![]);
        let flt_id  = dag.add_node(
            OpKind::Filter { sql: "SELECT * FROM data WHERE v > 1".into(), table_name: "data".into() },
            vec![src_id.clone()],
        );

        // Cache source, compute filter
        dag.set_result(&src_id, make_block(5));

        let mut exec = |op: &OpKind, parents: Vec<DataBlock>| -> Result<DataBlock, KoreError> {
            if let OpKind::Filter { sql, table_name } = op {
                let mut ctx = kore_sql::executor::KqlContext::new();
                ctx.register(table_name.clone(), parents[0].clone());
                ctx.query(sql)
            } else {
                Ok(parents.into_iter().next().unwrap_or_else(|| make_block(0)))
            }
        };

        let result = dag.recompute(&flt_id, &mut exec).unwrap();
        // v in [0..5], v > 1 → [2,3,4] → 3 rows
        assert_eq!(result.num_rows, 3);
    }

    #[test]
    fn test_retry_delays() {
        let sched = RetryScheduler::new(RetryConfig {
            max_attempts: 5, base_delay_ms: 100, max_delay_ms: 5000, jitter: false,
        });
        assert_eq!(sched.delay_ms(1), 100);
        assert_eq!(sched.delay_ms(2), 200);
        assert_eq!(sched.delay_ms(3), 400);
        assert_eq!(sched.delay_ms(4), 800);
        assert_eq!(sched.delay_ms(5), 1600);
    }

    #[tokio::test]
    async fn test_retry_succeeds_on_third_attempt() {
        use std::sync::{Arc, Mutex};
        let attempts = Arc::new(Mutex::new(0usize));
        let sched = RetryScheduler::new(RetryConfig {
            max_attempts: 3, base_delay_ms: 1, max_delay_ms: 10, jitter: false,
        });
        let a = attempts.clone();
        let result: Result<i32, String> = sched.run_with_retry(|n| {
            let a2 = a.clone();
            async move {
                *a2.lock().unwrap() = n;
                if n < 3 { Err(format!("fail attempt {n}")) } else { Ok(42) }
            }
        }).await;
        assert_eq!(result.unwrap(), 42);
        assert_eq!(*attempts.lock().unwrap(), 3);
    }

    #[test]
    fn test_speculative_execution() {
        let mut tracker = SpeculativeTracker::new(2.0);
        // Record fast tasks
        for _ in 0..8 { tracker.record(100); }
        tracker.record(101); // noise

        // A task taking 250ms > 100 * 2.0 = 200ms → straggler
        assert!(tracker.is_straggler(250));
        // A task taking 150ms is fine
        assert!(!tracker.is_straggler(150));
    }

    #[tokio::test]
    async fn speculation_primary_wins_when_fast() {
        // Primary finishes in 10ms, backup would take 500ms.
        let r: Result<i32, String> = run_with_speculation(
            || async {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                Ok::<i32, String>(1)
            },
            || async {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                Ok::<i32, String>(2)
            },
            100, // start backup after 100ms; primary should beat it
        ).await;
        assert_eq!(r.unwrap(), 1);
    }

    #[tokio::test]
    async fn speculation_backup_wins_when_primary_stalls() {
        // Primary would take 500ms, backup 20ms after 50ms delay.
        // Total race window: backup finishes at 50+20=70ms, primary at 500ms.
        let r: Result<i32, String> = run_with_speculation(
            || async {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                Ok::<i32, String>(1)
            },
            || async {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                Ok::<i32, String>(2)
            },
            50,
        ).await;
        assert_eq!(r.unwrap(), 2);
    }

    #[tokio::test]
    async fn speculation_returns_error_when_both_fail() {
        let r: Result<i32, String> = run_with_speculation(
            || async {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                Err::<i32, String>("primary bad".into())
            },
            || async {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                Err::<i32, String>("backup bad".into())
            },
            50,
        ).await;
        assert!(r.is_err());
    }

    // ── Phase 18: TaskLineage tests ───────────────────────────────────────

    fn make_partition_rec(idx: usize, worker: &str) -> PartitionRecord {
        PartitionRecord {
            partition_idx:  idx,
            task_id:        format!("task-{worker}-{idx}"),
            worker_id:      worker.to_string(),
            stage_id:       "stage-0".into(),
            sql:            "SELECT * FROM t".into(),
            table_name:     "t".into(),
            state:          PartitionState::Pending,
            started_at_ms:  0,
            finished_at_ms: None,
        }
    }

    #[test]
    fn task_lineage_records_and_completes_partitions() {
        let lineage = TaskLineage::new();
        for i in 0..3 {
            lineage.record(make_partition_rec(i, "w1"));
        }
        assert_eq!(lineage.pending_count(), 3);
        lineage.mark_completed("task-w1-1");
        assert_eq!(lineage.pending_count(), 2);
        assert_eq!(lineage.completed().len(), 1);
    }

    #[test]
    fn task_lineage_reports_lost_partitions_on_worker_death() {
        let lineage = TaskLineage::new();
        // Two workers, 4 partitions total: 0,1 on w1; 2,3 on w2.
        lineage.record(make_partition_rec(0, "w1"));
        lineage.record(make_partition_rec(1, "w1"));
        lineage.record(make_partition_rec(2, "w2"));
        lineage.record(make_partition_rec(3, "w2"));

        // Complete one on each worker.
        lineage.mark_completed("task-w1-0");
        lineage.mark_completed("task-w2-2");

        // w2 dies — the only pending partition on w2 is idx 3.
        let lost = lineage.mark_worker_lost("w2");
        assert_eq!(lost.len(), 1);
        assert_eq!(lost[0].partition_idx, 3);
        assert_eq!(lost[0].state, PartitionState::LostReadyToRetry);

        // w1's partition 0 is completed (untouched); partition 1 still pending.
        assert_eq!(lineage.pending_count(), 1);
    }

    #[test]
    fn task_lineage_reassigns_lost_partition_to_new_worker() {
        let lineage = TaskLineage::new();
        lineage.record(make_partition_rec(0, "w1"));
        lineage.record(make_partition_rec(1, "w1"));

        let lost = lineage.mark_worker_lost("w1");
        assert_eq!(lost.len(), 2);

        // Recovery reassigns partition 0 to w3.
        lineage.reassign("task-w1-0", "w3");
        let snap = lineage.snapshot();
        let rec0 = snap.iter().find(|r| r.partition_idx == 0).unwrap();
        assert_eq!(rec0.worker_id, "w3");
        assert_eq!(rec0.state, PartitionState::Pending);
    }

    #[test]
    fn task_lineage_completed_worker_survives_death_of_other_worker() {
        // Regression: if worker A completes its work then worker B dies,
        // we must not roll back A's completion.
        let lineage = TaskLineage::new();
        lineage.record(make_partition_rec(0, "wA"));
        lineage.record(make_partition_rec(1, "wB"));

        lineage.mark_completed("task-wA-0");
        let lost = lineage.mark_worker_lost("wB");
        assert_eq!(lost.len(), 1);
        assert_eq!(lineage.completed().len(), 1);
        assert_eq!(lineage.completed()[0].partition_idx, 0);
    }

    #[test]
    fn test_checkpoint_roundtrip() {
        let dir = std::env::temp_dir().join("kore_checkpoint_test");
        let cp  = Checkpoint::new(&dir);
        let b   = make_block(3);
        cp.save("stage-0", &b).unwrap();
        let b2 = cp.load("stage-0").unwrap();
        assert_eq!(b2.num_rows, 3);
        std::fs::remove_dir_all(&dir).ok();
    }
}
