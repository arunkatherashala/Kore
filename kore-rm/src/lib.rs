//! KORE Layer 54 — Cluster Resource Manager
//!
//! Manages the lifecycle of worker nodes in a KORE cluster:
//!
//! - **Worker registry** — track registered workers and their capabilities
//! - **Health monitoring** — heartbeat tracking, evict dead workers
//! - **Task scheduling** — route tasks to the least-loaded worker
//! - **Resource accounting** — CPU cores, memory, active tasks per worker
//! - **Auto-rebalancing** — redistribute work when workers join/leave
//! - **Slot tracking** — fine-grained concurrency slots per worker
//! - **Locality-aware scheduling** — prefer workers with cached data
//! - **Fair scheduling** — round-robin across named queues
//!
//! This is KORE's equivalent of Apache Spark's cluster managers
//! (YARN, Kubernetes, Mesos, Standalone).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};

// ─── Worker info ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerCapacity {
    pub id:           String,
    pub addr:         String,
    pub cores:        usize,
    pub memory_mb:    usize,
    pub active_tasks: usize,
    pub free_mem_mb:  usize,
    pub registered:   u64,       // ms since epoch
    pub last_seen:    u64,
    pub active_slots: usize,
    pub max_slots:    usize,
    pub cached_tables: Vec<String>,
}

impl WorkerCapacity {
    pub fn load_factor(&self) -> f64 {
        let cpu_load  = self.active_tasks as f64 / self.cores.max(1) as f64;
        let mem_load  = 1.0 - (self.free_mem_mb as f64 / self.memory_mb.max(1) as f64);
        (cpu_load + mem_load) / 2.0
    }

    pub fn is_healthy(&self, timeout_ms: u64) -> bool {
        now_ms().saturating_sub(self.last_seen) < timeout_ms
    }

    /// Acquire a concurrency slot. Returns true if a slot was available.
    pub fn acquire_slot(&mut self) -> bool {
        if self.active_slots < self.max_slots {
            self.active_slots += 1;
            true
        } else {
            false
        }
    }

    /// Release a previously acquired slot.
    pub fn release_slot(&mut self) {
        self.active_slots = self.active_slots.saturating_sub(1);
    }

    /// Number of slots currently available.
    pub fn available_slots(&self) -> usize {
        self.max_slots.saturating_sub(self.active_slots)
    }
}

// ─── Scheduling policies ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulingPolicy {
    /// Always route to the worker with the lowest load factor.
    LeastLoaded,
    /// Round-robin across all healthy workers.
    RoundRobin,
    /// Route to the worker with the most free memory (for large tasks).
    MostMemory,
    /// Fill one worker completely before using the next.
    BinPacking,
}

// ─── Resource Manager ─────────────────────────────────────────────────────────

pub struct ResourceManager {
    workers:         Arc<Mutex<HashMap<String, WorkerCapacity>>>,
    policy:          SchedulingPolicy,
    heartbeat_timeout_ms: u64,
    rr_counter:      Arc<Mutex<usize>>,
}

impl ResourceManager {
    pub fn new(policy: SchedulingPolicy) -> Self {
        Self {
            workers: Arc::new(Mutex::new(HashMap::new())),
            policy,
            heartbeat_timeout_ms: 30_000,   // 30 seconds
            rr_counter: Arc::new(Mutex::new(0)),
        }
    }

    pub fn with_timeout(mut self, ms: u64) -> Self { self.heartbeat_timeout_ms = ms; self }

    // ── Worker lifecycle ──────────────────────────────────────────────────────

    /// Register a new worker (or update existing).
    pub fn register_worker(&self, info: WorkerCapacity) {
        self.workers.lock().unwrap().insert(info.id.clone(), info);
    }

    /// Remove a worker by ID.
    pub fn deregister_worker(&self, id: &str) -> bool {
        self.workers.lock().unwrap().remove(id).is_some()
    }

    /// Update heartbeat timestamp and resource metrics for a worker.
    pub fn heartbeat(&self, id: &str, active_tasks: usize, free_mem_mb: usize) {
        let mut ws = self.workers.lock().unwrap();
        if let Some(w) = ws.get_mut(id) {
            w.last_seen    = now_ms();
            w.active_tasks = active_tasks;
            w.free_mem_mb  = free_mem_mb;
        }
    }

    /// Evict workers that haven't sent a heartbeat within timeout.
    pub fn evict_stale(&self) -> Vec<String> {
        let timeout = self.heartbeat_timeout_ms;
        let mut ws  = self.workers.lock().unwrap();
        let dead: Vec<String> = ws.values()
            .filter(|w| !w.is_healthy(timeout))
            .map(|w| w.id.clone())
            .collect();
        for id in &dead { ws.remove(id); }
        dead
    }

    // ── Scheduling ────────────────────────────────────────────────────────────

    /// Select a worker for a new task according to the scheduling policy.
    /// Returns `None` if no healthy workers are available.
    pub fn schedule(&self) -> Option<WorkerCapacity> {
        let ws: Vec<WorkerCapacity> = self.healthy_workers();
        if ws.is_empty() { return None; }

        match self.policy {
            SchedulingPolicy::LeastLoaded => {
                ws.into_iter().min_by(|a, b|
                    a.load_factor().partial_cmp(&b.load_factor()).unwrap_or(std::cmp::Ordering::Equal)
                )
            }
            SchedulingPolicy::RoundRobin => {
                let mut ctr = self.rr_counter.lock().unwrap();
                let idx = *ctr % ws.len();
                *ctr = (*ctr + 1) % ws.len();
                ws.into_iter().nth(idx)
            }
            SchedulingPolicy::MostMemory => {
                ws.into_iter().max_by_key(|w| w.free_mem_mb)
            }
            SchedulingPolicy::BinPacking => {
                // Prefer first non-full worker
                ws.into_iter().find(|w| w.active_tasks < w.cores)
                    .or_else(|| self.healthy_workers().into_iter().next())
            }
        }
    }

    /// Schedule N tasks across workers (returns one worker per task).
    pub fn schedule_n(&self, n: usize) -> Vec<WorkerCapacity> {
        (0..n).filter_map(|_| self.schedule()).collect()
    }

    /// Distribute N partitions across all healthy workers — returns
    /// `(worker_id, partition_ids)` assignments.
    pub fn assign_partitions(&self, n_partitions: usize) -> Vec<(String, Vec<usize>)> {
        let ws = self.healthy_workers();
        if ws.is_empty() { return vec![]; }
        let mut assignments: HashMap<String, Vec<usize>> = ws.iter()
            .map(|w| (w.id.clone(), vec![])).collect();
        for p in 0..n_partitions {
            let w = &ws[p % ws.len()];
            assignments.get_mut(&w.id).unwrap().push(p);
        }
        assignments.into_iter().collect()
    }

    // ── Introspection ─────────────────────────────────────────────────────────

    pub fn healthy_workers(&self) -> Vec<WorkerCapacity> {
        let timeout = self.heartbeat_timeout_ms;
        self.workers.lock().unwrap().values()
            .filter(|w| w.is_healthy(timeout))
            .cloned()
            .collect()
    }

    pub fn worker_count(&self)  -> usize { self.workers.lock().unwrap().len() }
    pub fn healthy_count(&self) -> usize { self.healthy_workers().len() }

    pub fn total_cores(&self)  -> usize { self.healthy_workers().iter().map(|w| w.cores).sum() }
    pub fn total_memory(&self) -> usize { self.healthy_workers().iter().map(|w| w.memory_mb).sum() }

    // ── Locality-aware scheduling ────────────────────────────────────────────

    /// Select a worker that already has `table` cached; fall back to least-loaded.
    pub fn schedule_with_locality(&self, table: &str) -> Option<String> {
        let ws = self.healthy_workers();
        if ws.is_empty() { return None; }

        // Prefer workers that have the table cached
        let with_cache: Vec<&WorkerCapacity> = ws.iter()
            .filter(|w| w.cached_tables.contains(&table.to_string()))
            .collect();

        if let Some(best) = with_cache.iter()
            .min_by(|a, b| a.load_factor().partial_cmp(&b.load_factor()).unwrap_or(std::cmp::Ordering::Equal))
        {
            return Some(best.id.clone());
        }

        // Fall back to least-loaded across all healthy workers
        ws.iter()
            .min_by(|a, b| a.load_factor().partial_cmp(&b.load_factor()).unwrap_or(std::cmp::Ordering::Equal))
            .map(|w| w.id.clone())
    }

    /// Cluster-wide load factor (0.0 = idle, 1.0 = fully loaded).
    pub fn cluster_load(&self) -> f64 {
        let ws = self.healthy_workers();
        if ws.is_empty() { return 0.0; }
        ws.iter().map(|w| w.load_factor()).sum::<f64>() / ws.len() as f64
    }

    pub fn cluster_status(&self) -> ClusterStatus {
        let ws = self.healthy_workers();
        ClusterStatus {
            total_workers:  self.worker_count(),
            healthy_workers: ws.len(),
            total_cores:    ws.iter().map(|w| w.cores).sum(),
            total_memory_mb: ws.iter().map(|w| w.memory_mb).sum(),
            cluster_load:   self.cluster_load(),
            active_tasks:   ws.iter().map(|w| w.active_tasks).sum(),
        }
    }
}

impl Default for ResourceManager {
    fn default() -> Self { Self::new(SchedulingPolicy::LeastLoaded) }
}

#[derive(Debug, Clone, Default)]
pub struct ClusterStatus {
    pub total_workers:   usize,
    pub healthy_workers: usize,
    pub total_cores:     usize,
    pub total_memory_mb: usize,
    pub cluster_load:    f64,
    pub active_tasks:    usize,
}

// ─── Fair Scheduler ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRequest {
    pub task_id:    String,
    pub sql:        String,
    pub table_name: String,
    pub priority:   u8,
}

/// Multi-queue fair scheduler with round-robin dispatch across named queues.
pub struct FairScheduler {
    queues:      HashMap<String, VecDeque<TaskRequest>>,
    queue_order: Vec<String>,
    current_idx: usize,
}

impl FairScheduler {
    pub fn new() -> Self {
        Self {
            queues: HashMap::new(),
            queue_order: Vec::new(),
            current_idx: 0,
        }
    }

    /// Submit a task into a named queue (creates the queue if it doesn't exist).
    pub fn submit(&mut self, queue: &str, task: TaskRequest) {
        if !self.queues.contains_key(queue) {
            self.queues.insert(queue.to_string(), VecDeque::new());
            self.queue_order.push(queue.to_string());
        }
        self.queues.get_mut(queue).unwrap().push_back(task);
    }

    /// Pick the next task using round-robin across queues.
    /// Skips empty queues. Returns None if all queues are empty.
    pub fn next_task(&mut self) -> Option<TaskRequest> {
        if self.queue_order.is_empty() {
            return None;
        }

        let n = self.queue_order.len();
        for _ in 0..n {
            let idx = self.current_idx % n;
            self.current_idx = (self.current_idx + 1) % n;
            let queue_name = &self.queue_order[idx];
            if let Some(q) = self.queues.get_mut(queue_name) {
                if let Some(task) = q.pop_front() {
                    return Some(task);
                }
            }
        }
        None
    }

    /// Number of queues currently registered.
    pub fn queue_count(&self) -> usize {
        self.queue_order.len()
    }

    /// Total pending tasks across all queues.
    pub fn pending_tasks(&self) -> usize {
        self.queues.values().map(|q| q.len()).sum()
    }
}

impl Default for FairScheduler {
    fn default() -> Self { Self::new() }
}

// ─── Real capacity detection ─────────────────────────────────────────────────

const DEFAULT_MEMORY_MB: usize = 4096;

/// Detect local system capacity: (cpu_cores, memory_mb).
/// Uses `available_parallelism` for cores; memory defaults to DEFAULT_MEMORY_MB
/// since cross-platform memory detection without external crates is complex.
pub fn detect_capacity() -> (usize, usize) {
    let cores = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1);
    (cores, DEFAULT_MEMORY_MB)
}

// ─── Helper ───────────────────────────────────────────────────────────────────

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[allow(dead_code)]
fn sample_worker(id: &str, addr: &str, cores: usize, mem_mb: usize) -> WorkerCapacity {
    WorkerCapacity {
        id: id.to_string(), addr: addr.to_string(),
        cores, memory_mb: mem_mb, free_mem_mb: mem_mb,
        active_tasks: 0,
        registered: now_ms(), last_seen: now_ms(),
        active_slots: 0,
        max_slots: cores * 2,
        cached_tables: Vec::new(),
    }
}

#[allow(dead_code)]
fn sample_worker_with_cache(id: &str, addr: &str, cores: usize, mem_mb: usize, tables: Vec<&str>) -> WorkerCapacity {
    WorkerCapacity {
        id: id.to_string(), addr: addr.to_string(),
        cores, memory_mb: mem_mb, free_mem_mb: mem_mb,
        active_tasks: 0,
        registered: now_ms(), last_seen: now_ms(),
        active_slots: 0,
        max_slots: cores * 2,
        cached_tables: tables.into_iter().map(String::from).collect(),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn rm() -> ResourceManager { ResourceManager::new(SchedulingPolicy::LeastLoaded).with_timeout(60_000) }

    fn add_workers(rm: &ResourceManager, n: usize) {
        for i in 0..n {
            rm.register_worker(sample_worker(
                &format!("w{i}"),
                &format!("127.0.0.1:{}", 9000 + i),
                4, 4096,
            ));
        }
    }

    #[test]
    fn test_register_and_count() {
        let rm = rm();
        add_workers(&rm, 3);
        assert_eq!(rm.worker_count(), 3);
        assert_eq!(rm.healthy_count(), 3);
    }

    #[test]
    fn test_schedule_least_loaded() {
        let rm = rm();
        add_workers(&rm, 3);
        // Make w0 busy
        rm.heartbeat("w0", 4, 512);  // 4 active tasks (core count = 4, load=1.0)
        let selected = rm.schedule().unwrap();
        assert_ne!(selected.id, "w0");  // should pick a less-loaded worker
    }

    #[test]
    fn test_round_robin() {
        let rm = ResourceManager::new(SchedulingPolicy::RoundRobin).with_timeout(60_000);
        add_workers(&rm, 3);
        let s1 = rm.schedule().unwrap().id;
        let s2 = rm.schedule().unwrap().id;
        let s3 = rm.schedule().unwrap().id;
        // All 3 should be different
        assert_ne!(s1, s2);
        assert_ne!(s2, s3);
    }

    #[test]
    fn test_assign_partitions() {
        let rm = rm();
        add_workers(&rm, 4);
        let assignments = rm.assign_partitions(12);
        // 4 workers × 3 partitions each = 12 total
        let total: usize = assignments.iter().map(|(_, p)| p.len()).sum();
        assert_eq!(total, 12);
    }

    #[test]
    fn test_evict_stale() {
        let rm = ResourceManager::new(SchedulingPolicy::LeastLoaded).with_timeout(0);  // timeout=0 = immediate
        rm.register_worker(sample_worker("dead", "127.0.0.1:9999", 2, 1024));
        std::thread::sleep(std::time::Duration::from_millis(10));
        let evicted = rm.evict_stale();
        assert_eq!(evicted, vec!["dead"]);
        assert_eq!(rm.healthy_count(), 0);
    }

    #[test]
    fn test_cluster_status() {
        let rm = rm();
        add_workers(&rm, 3);
        let status = rm.cluster_status();
        assert_eq!(status.total_workers, 3);
        assert_eq!(status.total_cores, 12);    // 3 workers × 4 cores
        assert_eq!(status.total_memory_mb, 12288); // 3 × 4096
        assert_eq!(status.cluster_load, 0.0);  // no active tasks
    }

    #[test]
    fn test_deregister() {
        let rm = rm();
        add_workers(&rm, 2);
        assert!(rm.deregister_worker("w0"));
        assert_eq!(rm.worker_count(), 1);
        assert!(!rm.deregister_worker("w99")); // doesn't exist
    }

    // ── Slot tracking tests ────────────────────────────────────────────────

    #[test]
    fn test_acquire_and_release_slots() {
        let mut w = sample_worker("w0", "127.0.0.1:9000", 4, 4096);
        assert_eq!(w.max_slots, 8); // 4 cores * 2
        assert_eq!(w.available_slots(), 8);

        assert!(w.acquire_slot());
        assert_eq!(w.active_slots, 1);
        assert_eq!(w.available_slots(), 7);

        assert!(w.acquire_slot());
        assert_eq!(w.active_slots, 2);

        w.release_slot();
        assert_eq!(w.active_slots, 1);
        assert_eq!(w.available_slots(), 7);
    }

    #[test]
    fn test_acquire_slot_at_max() {
        let mut w = sample_worker("w0", "127.0.0.1:9000", 1, 1024);
        assert_eq!(w.max_slots, 2); // 1 core * 2
        assert!(w.acquire_slot());
        assert!(w.acquire_slot());
        assert!(!w.acquire_slot()); // at max
        assert_eq!(w.available_slots(), 0);
    }

    #[test]
    fn test_release_slot_underflow() {
        let mut w = sample_worker("w0", "127.0.0.1:9000", 4, 4096);
        assert_eq!(w.active_slots, 0);
        w.release_slot(); // should not underflow
        assert_eq!(w.active_slots, 0);
    }

    // ── Locality-aware scheduling tests ────────────────────────────────────

    #[test]
    fn test_locality_prefers_cached_worker() {
        let rm = rm();
        rm.register_worker(sample_worker_with_cache(
            "w0", "127.0.0.1:9000", 4, 4096, vec!["orders", "users"],
        ));
        rm.register_worker(sample_worker_with_cache(
            "w1", "127.0.0.1:9001", 4, 4096, vec!["products"],
        ));
        rm.register_worker(sample_worker_with_cache(
            "w2", "127.0.0.1:9002", 4, 4096, vec!["orders"],
        ));

        let chosen = rm.schedule_with_locality("orders").unwrap();
        // Should pick w0 or w2 (both have "orders" cached)
        assert!(chosen == "w0" || chosen == "w2");
    }

    #[test]
    fn test_locality_falls_back_to_least_loaded() {
        let rm = rm();
        rm.register_worker(sample_worker_with_cache(
            "w0", "127.0.0.1:9000", 4, 4096, vec!["users"],
        ));
        rm.register_worker(sample_worker_with_cache(
            "w1", "127.0.0.1:9001", 4, 4096, vec!["products"],
        ));

        // No worker has "orders" cached, should fall back to least-loaded
        let chosen = rm.schedule_with_locality("orders").unwrap();
        assert!(chosen == "w0" || chosen == "w1");
    }

    #[test]
    fn test_locality_empty_cluster() {
        let rm = rm();
        assert!(rm.schedule_with_locality("orders").is_none());
    }

    #[test]
    fn test_locality_prefers_least_loaded_among_cached() {
        let rm = rm();
        let mut w0 = sample_worker_with_cache("w0", "127.0.0.1:9000", 4, 4096, vec!["orders"]);
        w0.active_tasks = 3; // heavily loaded
        w0.free_mem_mb = 1024;
        rm.register_worker(w0);

        rm.register_worker(sample_worker_with_cache(
            "w1", "127.0.0.1:9001", 4, 4096, vec!["orders"],
        )); // idle

        let chosen = rm.schedule_with_locality("orders").unwrap();
        assert_eq!(chosen, "w1"); // w1 is less loaded
    }

    // ── Fair scheduler tests ───────────────────────────────────────────────

    #[test]
    fn test_fair_scheduler_submit_and_next() {
        let mut fs = FairScheduler::new();
        fs.submit("queue_a", TaskRequest {
            task_id: "t1".into(), sql: "SELECT 1".into(),
            table_name: "orders".into(), priority: 1,
        });
        fs.submit("queue_b", TaskRequest {
            task_id: "t2".into(), sql: "SELECT 2".into(),
            table_name: "users".into(), priority: 1,
        });

        assert_eq!(fs.queue_count(), 2);
        assert_eq!(fs.pending_tasks(), 2);

        let task = fs.next_task().unwrap();
        assert_eq!(task.task_id, "t1");
        assert_eq!(fs.pending_tasks(), 1);
    }

    #[test]
    fn test_fair_scheduler_round_robin() {
        let mut fs = FairScheduler::new();

        // Submit 2 tasks to each of 3 queues
        for i in 0..2 {
            fs.submit("alpha", TaskRequest {
                task_id: format!("a{i}"), sql: "SELECT 1".into(),
                table_name: "t".into(), priority: 1,
            });
            fs.submit("beta", TaskRequest {
                task_id: format!("b{i}"), sql: "SELECT 2".into(),
                table_name: "t".into(), priority: 1,
            });
            fs.submit("gamma", TaskRequest {
                task_id: format!("g{i}"), sql: "SELECT 3".into(),
                table_name: "t".into(), priority: 1,
            });
        }

        // Round-robin should alternate between queues
        let t1 = fs.next_task().unwrap();
        let t2 = fs.next_task().unwrap();
        let t3 = fs.next_task().unwrap();

        assert_eq!(t1.task_id, "a0");
        assert_eq!(t2.task_id, "b0");
        assert_eq!(t3.task_id, "g0");

        let t4 = fs.next_task().unwrap();
        let t5 = fs.next_task().unwrap();
        let t6 = fs.next_task().unwrap();
        assert_eq!(t4.task_id, "a1");
        assert_eq!(t5.task_id, "b1");
        assert_eq!(t6.task_id, "g1");
    }

    #[test]
    fn test_fair_scheduler_skips_empty_queues() {
        let mut fs = FairScheduler::new();
        fs.submit("alpha", TaskRequest {
            task_id: "a0".into(), sql: "SELECT 1".into(),
            table_name: "t".into(), priority: 1,
        });
        fs.submit("beta", TaskRequest {
            task_id: "b0".into(), sql: "SELECT 2".into(),
            table_name: "t".into(), priority: 1,
        });

        // Drain alpha
        let _ = fs.next_task(); // a0
        let _ = fs.next_task(); // b0

        // Submit only to beta
        fs.submit("beta", TaskRequest {
            task_id: "b1".into(), sql: "SELECT 3".into(),
            table_name: "t".into(), priority: 1,
        });

        let task = fs.next_task().unwrap();
        assert_eq!(task.task_id, "b1"); // skips empty alpha
    }

    #[test]
    fn test_fair_scheduler_empty() {
        let mut fs = FairScheduler::new();
        assert!(fs.next_task().is_none());

        fs.submit("q", TaskRequest {
            task_id: "t1".into(), sql: "S".into(),
            table_name: "t".into(), priority: 0,
        });
        let _ = fs.next_task();
        assert!(fs.next_task().is_none()); // all drained
    }

    // ── Real capacity detection tests ──────────────────────────────────────

    #[test]
    fn test_detect_capacity() {
        let (cores, mem) = detect_capacity();
        assert!(cores >= 1, "should detect at least 1 core");
        assert_eq!(mem, DEFAULT_MEMORY_MB);
    }

    #[test]
    fn test_detect_capacity_matches_parallelism() {
        let (cores, _) = detect_capacity();
        let expected = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(1);
        assert_eq!(cores, expected);
    }
}
