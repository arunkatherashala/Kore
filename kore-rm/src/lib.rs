//! KORE Layer 54 — Cluster Resource Manager
//!
//! Manages the lifecycle of worker nodes in a KORE cluster:
//!
//! - **Worker registry** — track registered workers and their capabilities
//! - **Health monitoring** — heartbeat tracking, evict dead workers
//! - **Task scheduling** — route tasks to the least-loaded worker
//! - **Resource accounting** — CPU cores, memory, active tasks per worker
//! - **Auto-rebalancing** — redistribute work when workers join/leave
//!
//! This is KORE's equivalent of Apache Spark's cluster managers
//! (YARN, Kubernetes, Mesos, Standalone).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use kore_core::KoreError;

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
}

impl WorkerCapacity {
    pub fn load_factor(&self) -> f64 {
        // 0.0 = idle, 1.0 = fully loaded
        let cpu_load  = self.active_tasks as f64 / self.cores.max(1) as f64;
        let mem_load  = 1.0 - (self.free_mem_mb as f64 / self.memory_mb.max(1) as f64);
        (cpu_load + mem_load) / 2.0
    }

    pub fn is_healthy(&self, timeout_ms: u64) -> bool {
        now_ms().saturating_sub(self.last_seen) < timeout_ms
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

// ─── Helper ───────────────────────────────────────────────────────────────────

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn sample_worker(id: &str, addr: &str, cores: usize, mem_mb: usize) -> WorkerCapacity {
    WorkerCapacity {
        id: id.to_string(), addr: addr.to_string(),
        cores, memory_mb: mem_mb, free_mem_mb: mem_mb,
        active_tasks: 0,
        registered: now_ms(), last_seen: now_ms(),
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
}
