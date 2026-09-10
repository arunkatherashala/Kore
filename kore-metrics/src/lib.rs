//! KORE Layer 57 — Prometheus Metrics + Job Tracking
//!
//! Production observability: every operation tracked, queryable, and
//! exportable in Prometheus exposition format.
//!
//! Metrics collected:
//! - **Job metrics**: active jobs, completed, failed, latency percentiles
//! - **Query metrics**: parse time, plan time, execute time, rows/bytes
//! - **Worker metrics**: CPU load, memory, active tasks per worker
//! - **Shuffle metrics**: bytes written/read, spill events
//! - **I/O metrics**: files read/written, bytes, errors

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use serde::{Deserialize, Serialize};

// ─── Metric types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricValue {
    Counter(u64),
    Gauge(f64),
    Histogram(HistogramData),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistogramData {
    pub count:  u64,
    pub sum:    f64,
    pub min:    f64,
    pub max:    f64,
    pub p50:    f64,
    pub p95:    f64,
    pub p99:    f64,
    pub buckets: Vec<(f64, u64)>,  // (upper_bound, count)
    samples:     Vec<f64>,
}

impl HistogramData {
    pub fn observe(&mut self, val: f64) {
        self.count += 1;
        self.sum   += val;
        if self.count == 1 || val < self.min { self.min = val; }
        if self.count == 1 || val > self.max { self.max = val; }
        self.samples.push(val);
        if self.samples.len() % 100 == 0 { self.recompute_percentiles(); }
    }

    fn recompute_percentiles(&mut self) {
        if self.samples.is_empty() { return; }
        let mut sorted = self.samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = sorted.len();
        self.p50 = sorted[(n as f64 * 0.50) as usize];
        self.p95 = sorted[(n as f64 * 0.95) as usize];
        self.p99 = sorted[(n as f64 * 0.99) as usize];
    }
}

// ─── Job tracking ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JobState { Running, Succeeded, Failed, Cancelled }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub job_id:      String,
    pub query:       String,
    pub state:       JobState,
    pub started_at:  u64,
    pub finished_at: Option<u64>,
    pub rows_input:  usize,
    pub rows_output: usize,
    pub bytes_read:  usize,
    pub num_stages:  usize,
    pub num_tasks:   usize,
    pub error:       Option<String>,
}

impl JobRecord {
    pub fn elapsed_ms(&self) -> u64 {
        let end = self.finished_at.unwrap_or_else(now_ms);
        end.saturating_sub(self.started_at)
    }
    pub fn is_running(&self) -> bool { self.state == JobState::Running }
}

// ─── Metrics registry ─────────────────────────────────────────────────────────

/// Record of a DAG stage execution for visualization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagStageRecord {
    pub query_id:     String,
    pub stage_id:     usize,
    pub stage_name:   String,
    pub status:       DagStageStatus,
    pub rows_output:  usize,
    pub elapsed_ms:   u64,
    pub worker_count: usize,
    pub dependencies: Vec<usize>,
    pub tables:       Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DagStageStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

#[derive(Default)]
pub struct MetricsRegistry {
    counters:   Mutex<HashMap<String, u64>>,
    gauges:     Mutex<HashMap<String, f64>>,
    histograms: Mutex<HashMap<String, HistogramData>>,
    jobs:       Mutex<Vec<JobRecord>>,
    dag_stages: Mutex<Vec<DagStageRecord>>,
}

impl MetricsRegistry {
    pub fn new() -> Arc<Self> { Arc::new(Self::default()) }

    // ── Counter operations ────────────────────────────────────────────────────

    pub fn inc(&self, name: &str)        { self.add(name, 1); }
    pub fn add(&self, name: &str, n: u64) {
        *self.counters.lock().unwrap().entry(name.to_string()).or_insert(0) += n;
    }
    pub fn counter(&self, name: &str) -> u64 {
        self.counters.lock().unwrap().get(name).copied().unwrap_or(0)
    }

    // ── Gauge operations ──────────────────────────────────────────────────────

    pub fn set(&self, name: &str, val: f64) {
        self.gauges.lock().unwrap().insert(name.to_string(), val);
    }
    pub fn gauge(&self, name: &str) -> f64 {
        self.gauges.lock().unwrap().get(name).copied().unwrap_or(0.0)
    }

    // ── Histogram operations ──────────────────────────────────────────────────

    pub fn observe(&self, name: &str, val: f64) {
        self.histograms.lock().unwrap().entry(name.to_string()).or_default().observe(val);
    }
    pub fn histogram(&self, name: &str) -> Option<HistogramData> {
        self.histograms.lock().unwrap().get(name).cloned()
    }

    // ── Job tracking ──────────────────────────────────────────────────────────

    pub fn start_job(&self, job_id: &str, query: &str) {
        self.jobs.lock().unwrap().push(JobRecord {
            job_id:      job_id.to_string(),
            query:       query.to_string(),
            state:       JobState::Running,
            started_at:  now_ms(),
            finished_at: None,
            rows_input:  0,
            rows_output: 0,
            bytes_read:  0,
            num_stages:  0,
            num_tasks:   0,
            error:       None,
        });
        self.inc("jobs.started");
    }

    pub fn finish_job(&self, job_id: &str, rows_in: usize, rows_out: usize, bytes: usize) {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(j) = jobs.iter_mut().find(|j| j.job_id == job_id && j.is_running()) {
            j.state       = JobState::Succeeded;
            j.finished_at = Some(now_ms());
            j.rows_input  = rows_in;
            j.rows_output = rows_out;
            j.bytes_read  = bytes;
            let elapsed   = j.elapsed_ms() as f64;
            drop(jobs);
            self.inc("jobs.succeeded");
            self.observe("job.latency_ms", elapsed);
            self.add("rows.processed", rows_in as u64);
            self.add("bytes.processed", bytes as u64);
        }
    }

    pub fn fail_job(&self, job_id: &str, error: &str) {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(j) = jobs.iter_mut().find(|j| j.job_id == job_id && j.is_running()) {
            j.state       = JobState::Failed;
            j.finished_at = Some(now_ms());
            j.error       = Some(error.to_string());
        }
        drop(jobs);
        self.inc("jobs.failed");
    }

    pub fn active_jobs(&self) -> Vec<JobRecord> {
        self.jobs.lock().unwrap().iter().filter(|j| j.is_running()).cloned().collect()
    }

    pub fn job_history(&self, limit: usize) -> Vec<JobRecord> {
        let jobs = self.jobs.lock().unwrap();
        jobs.iter().rev().take(limit).cloned().collect()
    }

    // ── Prometheus exposition format ──────────────────────────────────────────

    /// Export all metrics in Prometheus text format.
    /// Compatible with Prometheus scraping and Grafana dashboards.
    pub fn prometheus_text(&self) -> String {
        let mut out = String::new();

        // Counters
        for (name, val) in self.counters.lock().unwrap().iter() {
            let pname = name.replace('.', "_");
            out.push_str(&format!("# HELP kore_{pname} KORE counter metric\n"));
            out.push_str(&format!("# TYPE kore_{pname} counter\n"));
            out.push_str(&format!("kore_{pname} {val}\n\n"));
        }

        // Gauges
        for (name, val) in self.gauges.lock().unwrap().iter() {
            let pname = name.replace('.', "_");
            out.push_str(&format!("# HELP kore_{pname} KORE gauge metric\n"));
            out.push_str(&format!("# TYPE kore_{pname} gauge\n"));
            out.push_str(&format!("kore_{pname} {val}\n\n"));
        }

        // Histograms
        for (name, h) in self.histograms.lock().unwrap().iter() {
            let pname = name.replace('.', "_");
            out.push_str(&format!("# HELP kore_{pname} KORE histogram metric\n"));
            out.push_str(&format!("# TYPE kore_{pname} summary\n"));
            out.push_str(&format!("kore_{pname}{{quantile=\"0.5\"}} {}\n", h.p50));
            out.push_str(&format!("kore_{pname}{{quantile=\"0.95\"}} {}\n", h.p95));
            out.push_str(&format!("kore_{pname}{{quantile=\"0.99\"}} {}\n", h.p99));
            out.push_str(&format!("kore_{pname}_count {}\n", h.count));
            out.push_str(&format!("kore_{pname}_sum {}\n\n", h.sum));
        }

        // Job summary
        let jobs = self.jobs.lock().unwrap();
        let running   = jobs.iter().filter(|j| j.state == JobState::Running).count();
        let succeeded = jobs.iter().filter(|j| j.state == JobState::Succeeded).count();
        let failed    = jobs.iter().filter(|j| j.state == JobState::Failed).count();
        out.push_str(&format!("kore_jobs_running {running}\n"));
        out.push_str(&format!("kore_jobs_succeeded {succeeded}\n"));
        out.push_str(&format!("kore_jobs_failed {failed}\n"));
        out
    }

    /// JSON export for REST API / dashboards.
    pub fn json_snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "counters":  *self.counters.lock().unwrap(),
            "gauges":    *self.gauges.lock().unwrap(),
            "active_jobs": self.active_jobs().len(),
            "job_history": self.job_history(10),
            "dag_stages": *self.dag_stages.lock().unwrap(),
        })
    }

    // ── DAG stage visualization ───────────────────────────────────────────────

    /// Record a completed DAG stage for visualization.
    pub fn record_dag_stage(&self, stage: DagStageRecord) {
        self.dag_stages.lock().unwrap().push(stage);
    }

    /// Get all DAG stages for a specific query.
    pub fn dag_stages_for_query(&self, query_id: &str) -> Vec<DagStageRecord> {
        self.dag_stages.lock().unwrap().iter()
            .filter(|s| s.query_id == query_id)
            .cloned()
            .collect()
    }

    /// Export DAG stage graph as JSON for UI consumption.
    pub fn dag_graph_json(&self, query_id: &str) -> serde_json::Value {
        let stages = self.dag_stages_for_query(query_id);
        let nodes: Vec<serde_json::Value> = stages.iter().map(|s| {
            serde_json::json!({
                "id": s.stage_id,
                "name": s.stage_name,
                "status": format!("{:?}", s.status),
                "rows_output": s.rows_output,
                "elapsed_ms": s.elapsed_ms,
                "worker_count": s.worker_count,
                "dependencies": s.dependencies,
            })
        }).collect();
        let edges: Vec<serde_json::Value> = stages.iter().flat_map(|s| {
            s.dependencies.iter().map(move |dep| {
                serde_json::json!({
                    "from": dep,
                    "to": s.stage_id,
                })
            })
        }).collect();
        serde_json::json!({
            "query_id": query_id,
            "nodes": nodes,
            "edges": edges,
        })
    }
}

// ─── Timer helper ─────────────────────────────────────────────────────────────

/// RAII timer that records elapsed time to a histogram when dropped.
pub struct Timer<'a> {
    registry: &'a MetricsRegistry,
    metric:   &'a str,
    start:    Instant,
}

impl<'a> Timer<'a> {
    pub fn start(registry: &'a MetricsRegistry, metric: &'a str) -> Self {
        Self { registry, metric, start: Instant::now() }
    }
}

impl<'a> Drop for Timer<'a> {
    fn drop(&mut self) {
        self.registry.observe(self.metric, self.start.elapsed().as_millis() as f64);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_gauge() {
        let reg = MetricsRegistry::new();
        reg.inc("queries.total");
        reg.inc("queries.total");
        reg.add("bytes.read", 1024);
        assert_eq!(reg.counter("queries.total"), 2);
        assert_eq!(reg.counter("bytes.read"), 1024);
        reg.set("workers.active", 4.0);
        assert_eq!(reg.gauge("workers.active"), 4.0);
    }

    #[test]
    fn test_histogram() {
        let reg = MetricsRegistry::new();
        for i in 1..=100 {
            reg.observe("query.latency_ms", i as f64);
        }
        let h = reg.histogram("query.latency_ms").unwrap();
        assert_eq!(h.count, 100);
        assert!((h.sum - 5050.0).abs() < 1.0);
    }

    #[test]
    fn test_job_tracking() {
        let reg = MetricsRegistry::new();
        reg.start_job("job-1", "SELECT * FROM t");
        assert_eq!(reg.active_jobs().len(), 1);
        reg.finish_job("job-1", 1000, 500, 8192);
        assert_eq!(reg.active_jobs().len(), 0);
        assert_eq!(reg.counter("jobs.succeeded"), 1);
        assert_eq!(reg.counter("rows.processed"), 1000);
    }

    #[test]
    fn test_job_failure() {
        let reg = MetricsRegistry::new();
        reg.start_job("job-fail", "bad SQL");
        reg.fail_job("job-fail", "parse error");
        assert_eq!(reg.counter("jobs.failed"), 1);
        let hist = reg.job_history(10);
        assert_eq!(hist[0].state, JobState::Failed);
    }

    #[test]
    fn test_dag_stage_recording() {
        let reg = MetricsRegistry::new();
        reg.record_dag_stage(DagStageRecord {
            query_id: "q-1".into(),
            stage_id: 0,
            stage_name: "Scan orders".into(),
            status: DagStageStatus::Completed,
            rows_output: 1000,
            elapsed_ms: 50,
            worker_count: 4,
            dependencies: vec![],
            tables: vec!["orders".into()],
        });
        reg.record_dag_stage(DagStageRecord {
            query_id: "q-1".into(),
            stage_id: 1,
            stage_name: "Shuffle HashBy(region)".into(),
            status: DagStageStatus::Completed,
            rows_output: 1000,
            elapsed_ms: 120,
            worker_count: 4,
            dependencies: vec![0],
            tables: vec!["orders".into()],
        });

        let stages = reg.dag_stages_for_query("q-1");
        assert_eq!(stages.len(), 2);

        let graph = reg.dag_graph_json("q-1");
        let nodes = graph["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 2);
        let edges = graph["edges"].as_array().unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0]["from"], 0);
        assert_eq!(edges[0]["to"], 1);
    }

    #[test]
    fn test_prometheus_export() {
        let reg = MetricsRegistry::new();
        reg.inc("queries.total");
        reg.set("workers.active", 3.0);
        reg.observe("latency_ms", 42.0);
        reg.start_job("j1", "SELECT 1");
        let prom = reg.prometheus_text();
        assert!(prom.contains("kore_queries_total 1"));
        assert!(prom.contains("kore_workers_active 3"));
        assert!(prom.contains("kore_jobs_running 1"));
    }
}
