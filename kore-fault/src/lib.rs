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
