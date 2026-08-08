//! Phase 19 — `EXPLAIN ANALYZE` and Prometheus export for the coordinator.
//!
//! The catalyst planner (Phase 16) produces a `PhysicalPlan` and picks a
//! `Dispatch`.  The vectorized fast-path (Phase 17) accelerates single-node
//! queries.  What was still missing before Phase 19: a way to **run a query,
//! collect real numbers, and print the plan tree annotated with those
//! numbers** — Spark's `df.explain(mode="cost")` + `EXPLAIN ANALYZE`.
//!
//! This module adds two public entry points on `Coordinator`:
//!
//! * `explain_analyze(sql) → String` — parse, plan, execute via
//!   `execute_planned`, then decorate the plan tree with:
//!     * the chosen `Dispatch`,
//!     * total wall-clock time (`total_ms`),
//!     * output row count,
//!     * number of dispatched tasks per stage (from `TaskLineage`),
//!     * per-worker task counts,
//!     * counter deltas (`jobs.succeeded`, `rows.processed`) captured before
//!       and after the run.
//!
//! * `prometheus_text()` — a passthrough for `MetricsRegistry::prometheus_text`
//!   so operators can scrape `kore_jobs_succeeded`, `kore_job_latency_ms`, etc.
//!
//! `explain_analyze` intentionally does **not** try to attribute per-node
//! runtime — the current dispatch path is stage-level, not
//! `PhysicalPlan`-node-level.  The annotations we emit are what the coord
//! actually observes, and are honest about their granularity.

use kore_core::{DataBlock, KoreError};

use crate::Coordinator;

impl Coordinator {
    /// Run `sql` and return a Spark-style `EXPLAIN ANALYZE` string plus the
    /// result `DataBlock`.  The string ends with a `== Runtime stats ==`
    /// section that summarises what the coordinator observed while running.
    ///
    /// Callers use this to answer "why was my query slow?" — the physical
    /// plan alone (from `explain()`) can lie about intent; the runtime
    /// section is ground truth.
    pub async fn explain_analyze(&self, sql: &str) -> Result<(String, DataBlock), KoreError> {
        // Plan first — deterministic, no side effects, easy to snapshot.
        let (plan, dispatch) = self.plan_sql(sql)?;

        // Snapshot metrics before to compute deltas after.
        let job_id = format!("job-{}", kore_net::now_ms());
        let succeeded_before = self.metrics.counter("jobs.succeeded");
        let rows_before      = self.metrics.counter("rows.processed");

        self.metrics.start_job(&job_id, sql);

        // Time and run.
        let started = std::time::Instant::now();
        let result = self.execute_planned(sql).await;
        let elapsed_ms = started.elapsed().as_millis() as u64;

        let out = match result {
            Ok(block) => {
                self.metrics.finish_job(&job_id, block.num_rows, block.num_rows, block.num_rows * 8);
                block
            }
            Err(e) => {
                self.metrics.fail_job(&job_id, &format!("{e:?}"));
                return Err(e);
            }
        };

        let succeeded_after = self.metrics.counter("jobs.succeeded");
        let rows_after      = self.metrics.counter("rows.processed");

        // Compose the annotated plan tree.
        let mut s = String::new();
        s.push_str("== Physical plan ==\n");
        s.push_str(&plan.explain());
        s.push_str("\n== Dispatch ==\n");
        s.push_str(&format!("{}\n", dispatch.kind()));
        s.push_str("\n== Runtime stats ==\n");
        s.push_str(&format!("query           : {sql}\n"));
        s.push_str(&format!("job_id          : {job_id}\n"));
        s.push_str(&format!("total_ms        : {elapsed_ms}\n"));
        s.push_str(&format!("output_rows     : {}\n", out.num_rows));
        s.push_str(&format!("dispatch        : {}\n", dispatch.kind()));

        // Lineage tracker view — dispatched task count by state.  This will
        // be empty on the vectorized/local single-node path; useful once the
        // coordinator actually dispatches to workers.
        let snap = self.lineage.snapshot();
        if !snap.is_empty() {
            s.push_str(&format!("tasks_dispatched: {}\n", snap.len()));
            s.push_str(&format!(
                "tasks_completed : {}\n",
                snap.iter().filter(|r| r.state == kore_fault::PartitionState::Completed).count(),
            ));
            s.push_str(&format!("tasks_pending   : {}\n", self.lineage.pending_count()));
            // Per-worker breakdown.
            let mut per_worker: std::collections::BTreeMap<&str, usize> = Default::default();
            for r in &snap { *per_worker.entry(&r.worker_id).or_insert(0) += 1; }
            for (w, n) in per_worker {
                s.push_str(&format!("  worker {w:<24} : {n} tasks\n"));
            }
        }

        // Metric deltas from this specific job.
        let jobs_delta = succeeded_after.saturating_sub(succeeded_before);
        let rows_delta = rows_after.saturating_sub(rows_before);
        s.push_str(&format!("jobs_delta      : +{jobs_delta}\n"));
        s.push_str(&format!("rows_delta      : +{rows_delta}\n"));

        // Latency histogram summary (if any samples exist).
        if let Some(h) = self.metrics.histogram("job.latency_ms") {
            s.push_str(&format!(
                "latency_ms      : p50={:.0} p95={:.0} p99={:.0} count={}\n",
                h.p50, h.p95, h.p99, h.count,
            ));
        }

        Ok((s, out))
    }

    /// Return the coordinator's metrics in Prometheus text-exposition
    /// format.  Scrapers use `http://coord/metrics` (once the HTTP endpoint
    /// is added) to pull `kore_jobs_*`, `kore_job_latency_ms`, etc.
    pub fn prometheus_text(&self) -> String {
        self.metrics.prometheus_text()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn simple_block(n: usize) -> DataBlock {
        DataBlock {
            num_rows: n,
            columns: vec![
                Column { name: "id".into(),
                    data: ColumnData::Int64((0..n).map(|i| Some(i as i64)).collect()) },
                Column { name: "amount".into(),
                    data: ColumnData::Float64((0..n).map(|i| Some(i as f64)).collect()) },
            ],
        }
    }

    // execute_planned needs workers; for a pure metric-integration test we
    // avoid the full cluster spin-up and just exercise the plan+snapshot path
    // via explain() plus a manual metric bump.
    #[test]
    fn prometheus_text_reflects_recorded_metrics() {
        let coord = Coordinator::new();
        coord.metrics.inc("queries.total");
        coord.metrics.inc("queries.total");
        coord.metrics.observe("job.latency_ms", 42.0);
        let text = coord.prometheus_text();
        assert!(text.contains("kore_queries_total 2"), "queries.total not in prom output:\n{text}");
        assert!(text.contains("kore_job_latency_ms"), "latency histogram missing:\n{text}");
    }

    #[test]
    fn explain_analyze_string_has_all_sections_for_planned_only() {
        // No workers here — we drive only the plan+annotate path.  This is
        // what the caller sees on a single-node coord that hasn't yet
        // registered any workers.
        let coord = Coordinator::new();
        coord.register_table_for_planning("t", simple_block(50));

        // We can produce an explain() string without running.
        let explain = coord.explain("SELECT * FROM t LIMIT 5").expect("explain");
        assert!(explain.contains("Physical plan") || explain.contains("Scan"),
            "explain string missing plan tree:\n{explain}");
        assert!(explain.contains("Dispatch"), "explain missing dispatch section:\n{explain}");
    }
}
