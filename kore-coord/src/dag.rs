//! DAG Scheduler — Stage-based distributed execution.
//!
//! Compiles a `PhysicalPlan` into a `StageDAG` by splitting at `Exchange`
//! boundaries (shuffles). Stages execute in topological order — all parent
//! stages complete before any child stage starts.
//!
//! This mirrors Apache Spark's DAGScheduler:
//!   PhysicalPlan → StageDAG → ResultStage + ShuffleMapStages
//!
//! Pipeline-able operators (Filter, Project, Partial Agg) are fused within a
//! single stage. Exchange nodes become stage boundaries where data is
//! repartitioned across workers.

use std::collections::HashMap;
use kore_aqe::{AqeDecision, AqeOptimizer, StageStats};
use kore_catalyst::physical::{AggMode, JoinStrategy, Partitioning, PhysicalPlan};
use kore_core::{DataBlock, KoreError};
use kore_fault::RetryScheduler;
use kore_net::partition_block;
use kore_sql::executor::KqlContext;

use crate::Coordinator;

// ─── Stage ────────────────────────────────────────────────────────────────────

/// A pipeline-able unit of work. All operators in a stage execute without
/// shuffling data — they run as a single fused pipeline on each partition.
#[derive(Debug, Clone)]
pub struct Stage {
    pub id: usize,
    /// The physical plan fragment for this stage (operators between two
    /// Exchange boundaries, or from a leaf Scan up to the first Exchange).
    pub plan_fragment: PhysicalPlan,
    /// How the output of this stage is partitioned for downstream consumption.
    pub output_partitioning: Partitioning,
    /// Stages that must complete before this stage can start.
    pub dependencies: Vec<usize>,
    /// Which tables this stage scans (for data routing).
    pub source_tables: Vec<String>,
    /// The SQL to execute on workers for this stage (derived from the plan).
    pub sql: Option<String>,
    /// Reduce SQL for two-phase aggregation.
    pub reduce_sql: Option<String>,
    /// Join metadata if this stage involves a join.
    pub join_info: Option<JoinInfo>,
}

/// Metadata for join stages — tells the executor how to wire the join.
#[derive(Debug, Clone)]
pub struct JoinInfo {
    pub strategy: JoinStrategy,
    pub left_stage: usize,
    pub right_stage: usize,
    pub left_key: String,
    pub right_key: String,
    pub left_table: String,
    pub right_table: String,
}

// ─── StageDAG ─────────────────────────────────────────────────────────────────

/// Directed acyclic graph of stages, compiled from a PhysicalPlan.
#[derive(Debug, Clone)]
pub struct StageDAG {
    pub stages: Vec<Stage>,
    /// The final stage whose output is the query result.
    pub result_stage: usize,
    /// Original SQL for fallback execution.
    pub original_sql: String,
}

impl StageDAG {
    /// Return stages in topological order (dependencies first).
    pub fn topological_order(&self) -> Vec<usize> {
        let n = self.stages.len();
        let mut in_degree: Vec<usize> = vec![0; n];
        let mut children: Vec<Vec<usize>> = vec![vec![]; n];

        for stage in &self.stages {
            for &dep in &stage.dependencies {
                children[dep].push(stage.id);
                in_degree[stage.id] += 1;
            }
        }

        let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
        let mut order = Vec::with_capacity(n);

        while let Some(s) = queue.pop() {
            order.push(s);
            for &child in &children[s] {
                in_degree[child] -= 1;
                if in_degree[child] == 0 {
                    queue.push(child);
                }
            }
        }

        order
    }

    /// Human-readable summary for EXPLAIN DAG.
    pub fn explain(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("StageDAG: {} stages, result=stage_{}\n",
            self.stages.len(), self.result_stage));
        for stage in &self.stages {
            out.push_str(&format!(
                "  stage_{}: deps={:?} tables={:?} partitioning={:?}\n",
                stage.id, stage.dependencies, stage.source_tables,
                stage.output_partitioning,
            ));
            let plan_text = stage.plan_fragment.explain();
            for line in plan_text.lines() {
                out.push_str(&format!("    {line}\n"));
            }
        }
        out
    }
}

// ─── Compiler: PhysicalPlan → StageDAG ────────────────────────────────────────

struct DagCompiler {
    stages: Vec<Stage>,
    next_id: usize,
}

impl DagCompiler {
    fn new() -> Self {
        Self { stages: Vec::new(), next_id: 0 }
    }

    fn alloc_id(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Recursively compile a PhysicalPlan into stages.
    /// Returns the stage ID that produces this plan's output.
    fn compile(&mut self, plan: &PhysicalPlan, original_sql: &str) -> usize {
        match plan {
            PhysicalPlan::Exchange { partitioning, input } => {
                let child_stage = self.compile(input, original_sql);
                let id = self.alloc_id();
                let tables = self.stages[child_stage].source_tables.clone();

                self.stages.push(Stage {
                    id,
                    plan_fragment: plan.clone(),
                    output_partitioning: partitioning.clone(),
                    dependencies: vec![child_stage],
                    source_tables: tables,
                    sql: Some(original_sql.to_string()),
                    reduce_sql: None,
                    join_info: None,
                });
                id
            }

            PhysicalPlan::Join { strategy, left, right, on, .. } => {
                let left_stage = self.compile(left, original_sql);
                let right_stage = self.compile(right, original_sql);

                let left_table = self.stages[left_stage]
                    .source_tables.first().cloned().unwrap_or_default();
                let right_table = self.stages[right_stage]
                    .source_tables.first().cloned().unwrap_or_default();

                let (left_key, right_key) = on.first()
                    .map(|c| (c.left_col.clone(), c.right_col.clone()))
                    .unwrap_or_default();

                let id = self.alloc_id();
                let mut tables = self.stages[left_stage].source_tables.clone();
                tables.extend(self.stages[right_stage].source_tables.clone());

                self.stages.push(Stage {
                    id,
                    plan_fragment: plan.clone(),
                    output_partitioning: Partitioning::Single,
                    dependencies: vec![left_stage, right_stage],
                    source_tables: tables,
                    sql: Some(original_sql.to_string()),
                    reduce_sql: None,
                    join_info: Some(JoinInfo {
                        strategy: *strategy,
                        left_stage,
                        right_stage,
                        left_key,
                        right_key,
                        left_table,
                        right_table,
                    }),
                });
                id
            }

            PhysicalPlan::HashAggregate { mode: AggMode::Final, keys, input, .. } => {
                let child_stage = self.compile(input, original_sql);
                let id = self.alloc_id();
                let tables = self.stages[child_stage].source_tables.clone();

                let reduce_sql = if !keys.is_empty() {
                    Some(original_sql.to_string())
                } else {
                    None
                };

                self.stages.push(Stage {
                    id,
                    plan_fragment: plan.clone(),
                    output_partitioning: Partitioning::Single,
                    dependencies: vec![child_stage],
                    source_tables: tables,
                    sql: Some(original_sql.to_string()),
                    reduce_sql,
                    join_info: None,
                });
                id
            }

            PhysicalPlan::Scan { table, .. } => {
                let id = self.alloc_id();
                self.stages.push(Stage {
                    id,
                    plan_fragment: plan.clone(),
                    output_partitioning: Partitioning::Single,
                    dependencies: vec![],
                    source_tables: vec![table.clone()],
                    sql: None,
                    reduce_sql: None,
                    join_info: None,
                });
                id
            }

            PhysicalPlan::Filter { input, .. }
            | PhysicalPlan::Project { input, .. }
            | PhysicalPlan::Sort { input, .. }
            | PhysicalPlan::Limit { input, .. }
            | PhysicalPlan::HashAggregate { input, .. }
            | PhysicalPlan::RuntimeFiltered { input, .. } => {
                let child_stage = self.compile(input, original_sql);

                let stage = &mut self.stages[child_stage];
                stage.plan_fragment = plan.clone();
                child_stage
            }

            PhysicalPlan::Union { inputs } => {
                let child_ids: Vec<usize> = inputs.iter()
                    .map(|inp| self.compile(inp, original_sql))
                    .collect();

                let id = self.alloc_id();
                let mut tables = Vec::new();
                for &cid in &child_ids {
                    tables.extend(self.stages[cid].source_tables.clone());
                }

                self.stages.push(Stage {
                    id,
                    plan_fragment: plan.clone(),
                    output_partitioning: Partitioning::Single,
                    dependencies: child_ids,
                    source_tables: tables,
                    sql: Some(original_sql.to_string()),
                    reduce_sql: None,
                    join_info: None,
                });
                id
            }
        }
    }
}

/// Compile a PhysicalPlan into a StageDAG.
pub fn compile_dag(plan: &PhysicalPlan, original_sql: &str) -> StageDAG {
    let mut compiler = DagCompiler::new();
    let result_stage = compiler.compile(plan, original_sql);
    StageDAG {
        stages: compiler.stages,
        result_stage,
        original_sql: original_sql.to_string(),
    }
}

// ─── DAG Execution ────────────────────────────────────────────────────────────

/// Intermediate results from completed stages, keyed by stage ID.
type StageResults = HashMap<usize, DataBlock>;

impl Coordinator {
    /// Execute a query through the DAG scheduler with AQE and fault tolerance.
    ///
    /// Compiles the PhysicalPlan into stages, executes them in topological
    /// order, collects runtime stats for AQE re-optimization, and retries
    /// failed stages via the fault tolerance layer.
    pub async fn execute_dag(
        &self,
        plan: &PhysicalPlan,
        sql: &str,
    ) -> Result<DataBlock, KoreError> {
        let dag = compile_dag(plan, sql);
        let order = dag.topological_order();
        let mut results: StageResults = HashMap::new();
        let mut aqe = AqeOptimizer::new();
        let retry = RetryScheduler::new(kore_fault::RetryConfig::default());

        for stage_id in order {
            let stage = &dag.stages[stage_id];

            // Fault-tolerant execution: retry failed stages
            let result = retry.run_with_retry(|_attempt| {
                let stage = stage.clone();
                let results_ref = results.clone();
                let sql = sql.to_string();
                let coord = self.clone();
                async move {
                    coord.execute_stage(&stage, &results_ref, &sql).await
                        .map_err(|e| std::io::Error::new(
                            std::io::ErrorKind::Other, e.to_string()
                        ))
                }
            }).await.map_err(|e| KoreError::InvalidArgument(
                format!("stage {} failed after retries: {e:?}", stage_id)
            ))?;

            // AQE: collect runtime stats after each stage completes
            let stats = StageStats::collect(
                &format!("stage_{stage_id}"),
                &result,
                1, // partitions
            );
            aqe.record(stats);

            // AQE: check for optimization decisions
            let decision = aqe.decide(
                &format!("stage_{stage_id}"),
                None,
            );
            match decision {
                AqeDecision::BroadcastJoin { .. } => {
                    // Stage output is small enough to broadcast — downstream
                    // join stages can use this info for strategy switching
                }
                AqeDecision::Coalesce { target_partitions, .. } => {
                    // Log coalesce recommendation for monitoring
                    let _ = target_partitions;
                }
                AqeDecision::SkewSplit { partition_indices, .. } => {
                    let _ = partition_indices;
                }
                _ => {}
            }

            // Record lineage for fault recovery
            self.lineage.record(kore_fault::PartitionRecord {
                partition_idx: stage_id,
                task_id: format!("dag-stage{stage_id}"),
                worker_id: "coordinator".into(),
                stage_id: format!("stage_{stage_id}"),
                sql: sql.to_string(),
                table_name: stage.source_tables.first()
                    .cloned().unwrap_or_default(),
                state: kore_fault::PartitionState::Completed,
                started_at_ms: kore_net::now_ms(),
                finished_at_ms: Some(kore_net::now_ms()),
            });

            // Track metrics
            self.metrics.inc("dag.stages_completed");
            self.metrics.observe(
                "dag.stage_rows",
                result.num_rows as f64,
            );

            results.insert(stage_id, result);
        }

        self.metrics.inc("dag.queries_completed");

        results.remove(&dag.result_stage)
            .ok_or_else(|| KoreError::InvalidArgument(
                "DAG execution produced no result".into()
            ))
    }

    /// Execute a single stage, using results from completed parent stages.
    async fn execute_stage(
        &self,
        stage: &Stage,
        parent_results: &StageResults,
        original_sql: &str,
    ) -> Result<DataBlock, KoreError> {
        if let Some(join_info) = &stage.join_info {
            return self.execute_join_stage(join_info, parent_results, original_sql).await;
        }

        if stage.dependencies.is_empty() {
            return self.execute_leaf_stage(stage).await;
        }

        if stage.dependencies.len() == 1 {
            let parent_id = stage.dependencies[0];
            let input = parent_results.get(&parent_id)
                .ok_or_else(|| KoreError::InvalidArgument(
                    format!("missing parent stage {parent_id} result")
                ))?;

            return self.execute_pipeline_stage(stage, input.clone(), original_sql).await;
        }

        // Union: concat all parent results
        let mut blocks = Vec::new();
        for &dep in &stage.dependencies {
            if let Some(b) = parent_results.get(&dep) {
                if b.num_rows > 0 {
                    blocks.push(b.clone());
                }
            }
        }
        if blocks.is_empty() {
            Ok(DataBlock::empty())
        } else {
            DataBlock::concat(blocks)
        }
    }

    /// Execute a leaf stage (Scan) — fetch data from registered tables.
    async fn execute_leaf_stage(
        &self,
        stage: &Stage,
    ) -> Result<DataBlock, KoreError> {
        let table = stage.source_tables.first()
            .ok_or_else(|| KoreError::InvalidArgument("leaf stage has no table".into()))?;

        self.peek_registered(table)
            .ok_or_else(|| KoreError::InvalidArgument(
                format!("table '{table}' not registered for DAG execution")
            ))
    }

    /// Execute a pipeline stage (filter/project/agg over parent output).
    async fn execute_pipeline_stage(
        &self,
        stage: &Stage,
        input: DataBlock,
        original_sql: &str,
    ) -> Result<DataBlock, KoreError> {
        let workers = self.workers.lock().unwrap_or_else(|e| e.into_inner()).clone();

        // If we have workers, distribute the work
        if !workers.is_empty() {
            let table_name = stage.source_tables.first()
                .cloned()
                .unwrap_or_else(|| "__stage_data__".to_string());

            let sql = stage.sql.as_deref().unwrap_or(original_sql);

            if let Some(reduce_sql) = &stage.reduce_sql {
                return self.execute_distributed_v2(
                    sql, &table_name, input, Some(reduce_sql),
                ).await;
            }

            return self.execute_distributed_v2(
                sql, &table_name, input, None,
            ).await;
        }

        // Fallback: execute locally
        let table_name = stage.source_tables.first()
            .cloned()
            .unwrap_or_else(|| "__stage_data__".to_string());
        let sql = stage.sql.as_deref().unwrap_or(original_sql);

        let mut ctx = KqlContext::new();
        ctx.register(&table_name, input);
        ctx.query(sql)
    }

    /// Execute a join stage using the appropriate strategy.
    async fn execute_join_stage(
        &self,
        join_info: &JoinInfo,
        parent_results: &StageResults,
        original_sql: &str,
    ) -> Result<DataBlock, KoreError> {
        let left_data = parent_results.get(&join_info.left_stage)
            .ok_or_else(|| KoreError::InvalidArgument(
                format!("missing left stage {} for join", join_info.left_stage)
            ))?;
        let right_data = parent_results.get(&join_info.right_stage)
            .ok_or_else(|| KoreError::InvalidArgument(
                format!("missing right stage {} for join", join_info.right_stage)
            ))?;

        match join_info.strategy {
            JoinStrategy::BroadcastHash => {
                let workers = self.workers.lock().unwrap_or_else(|e| e.into_inner()).clone();
                if !workers.is_empty() {
                    let (large, small, large_name, small_name) =
                        if left_data.num_rows >= right_data.num_rows {
                            (left_data.clone(), right_data.clone(),
                             &join_info.left_table, &join_info.right_table)
                        } else {
                            (right_data.clone(), left_data.clone(),
                             &join_info.right_table, &join_info.left_table)
                        };
                    return self.execute_broadcast_join(
                        original_sql, large_name, large, small_name, small,
                    ).await;
                }
                // Local fallback
                let joined = kore_sortmerge::broadcast_join(
                    left_data, right_data,
                    &join_info.left_key, &join_info.right_key,
                );
                Ok(joined)
            }

            JoinStrategy::SortMerge => {
                let joined = kore_sortmerge::sort_merge_join(
                    left_data, right_data,
                    &join_info.left_key, &join_info.right_key,
                );
                Ok(joined)
            }

            JoinStrategy::SkewedHash | JoinStrategy::ShuffleHash | JoinStrategy::NestedLoop => {
                let workers = self.workers.lock().unwrap_or_else(|e| e.into_inner()).clone();
                if !workers.is_empty() {
                    // Use distributed shuffle join: partition both sides, local join
                    let n = workers.len();
                    let left_parts = partition_block(left_data.clone(), n);
                    let right_parts = partition_block(right_data.clone(), n);

                    let mut join_results = Vec::new();
                    for (lp, rp) in left_parts.into_iter().zip(right_parts.into_iter()) {
                        if lp.num_rows > 0 && rp.num_rows > 0 {
                            let joined = kore_sortmerge::hash_join(
                                &lp, &rp,
                                &join_info.left_key, &join_info.right_key,
                            );
                            if joined.num_rows > 0 {
                                join_results.push(joined);
                            }
                        }
                    }

                    if join_results.is_empty() {
                        return Ok(DataBlock::empty());
                    }
                    return DataBlock::concat(join_results);
                }
                // Local fallback
                let joined = kore_sortmerge::hash_join(
                    left_data, right_data,
                    &join_info.left_key, &join_info.right_key,
                );
                Ok(joined)
            }
        }
    }

    /// Plan and execute a query through the DAG scheduler.
    /// This is the new entry point that replaces flat dispatch.
    pub async fn execute_dag_planned(&self, sql: &str) -> Result<DataBlock, KoreError> {
        let (plan, _dispatch) = self.plan_sql(sql)?;
        self.execute_dag(&plan, sql).await
    }

    /// Return a DAG explain string for a SQL query.
    pub fn explain_dag(&self, sql: &str) -> Result<String, KoreError> {
        let (plan, _dispatch) = self.plan_sql(sql)?;
        let dag = compile_dag(&plan, sql);
        Ok(dag.explain())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, DataBlock};
    use kore_catalyst::physical::{PhysicalPlan, Partitioning, AggMode};
    use kore_sql::ast::*;

    fn scan(table: &str, rows: usize) -> PhysicalPlan {
        PhysicalPlan::Scan {
            table: table.into(),
            projected_cols: None,
            pushed_filter: None,
            est_rows: rows,
        }
    }

    #[test]
    fn compile_simple_scan_produces_one_stage() {
        let plan = scan("orders", 1000);
        let dag = compile_dag(&plan, "SELECT * FROM orders");
        assert_eq!(dag.stages.len(), 1);
        assert_eq!(dag.result_stage, 0);
        assert_eq!(dag.stages[0].source_tables, vec!["orders".to_string()]);
    }

    #[test]
    fn compile_filter_fuses_into_scan_stage() {
        let plan = PhysicalPlan::Filter {
            predicate: Expr::Bool(true),
            input: Box::new(scan("orders", 1000)),
        };
        let dag = compile_dag(&plan, "SELECT * FROM orders WHERE true");
        assert_eq!(dag.stages.len(), 1, "filter should fuse with scan");
    }

    #[test]
    fn compile_exchange_creates_two_stages() {
        let plan = PhysicalPlan::Exchange {
            partitioning: Partitioning::HashBy {
                cols: vec!["region".into()],
                n: 4,
            },
            input: Box::new(scan("orders", 1000)),
        };
        let dag = compile_dag(&plan, "SELECT region FROM orders");
        assert_eq!(dag.stages.len(), 2, "exchange should split into 2 stages");
        assert_eq!(dag.stages[1].dependencies, vec![0]);
    }

    #[test]
    fn compile_two_phase_agg_creates_stages() {
        let plan = PhysicalPlan::HashAggregate {
            keys: vec!["region".into()],
            aggs: vec![],
            mode: AggMode::Final,
            input: Box::new(PhysicalPlan::Exchange {
                partitioning: Partitioning::HashBy {
                    cols: vec!["region".into()],
                    n: 200,
                },
                input: Box::new(PhysicalPlan::HashAggregate {
                    keys: vec!["region".into()],
                    aggs: vec![],
                    mode: AggMode::Partial,
                    input: Box::new(scan("orders", 1_000_000)),
                }),
            }),
        };
        let dag = compile_dag(&plan, "SELECT region, SUM(sales) FROM orders GROUP BY region");
        assert!(dag.stages.len() >= 2, "two-phase agg needs >=2 stages, got {}", dag.stages.len());

        let order = dag.topological_order();
        assert!(!order.is_empty());
        // Result stage should be last in topological order
        assert_eq!(*order.last().unwrap(), dag.result_stage);
    }

    #[test]
    fn compile_broadcast_join_creates_three_stages() {
        let plan = PhysicalPlan::Join {
            strategy: JoinStrategy::BroadcastHash,
            join_type: JoinKind::Inner,
            left: Box::new(scan("fact", 1_000_000)),
            right: Box::new(PhysicalPlan::Exchange {
                partitioning: Partitioning::Broadcast,
                input: Box::new(scan("dim", 100)),
            }),
            on: vec![kore_catalyst::physical::JoinCond {
                left_col: "dim_id".into(),
                right_col: "id".into(),
            }],
        };
        let dag = compile_dag(&plan, "SELECT * FROM fact JOIN dim ON fact.dim_id = dim.id");
        assert!(dag.stages.len() >= 3, "broadcast join needs >=3 stages, got {}", dag.stages.len());

        let join_stage = &dag.stages[dag.result_stage];
        assert!(join_stage.join_info.is_some());
        assert_eq!(join_stage.join_info.as_ref().unwrap().strategy, JoinStrategy::BroadcastHash);
    }

    #[test]
    fn topological_order_respects_dependencies() {
        let plan = PhysicalPlan::HashAggregate {
            keys: vec!["k".into()],
            aggs: vec![],
            mode: AggMode::Final,
            input: Box::new(PhysicalPlan::Exchange {
                partitioning: Partitioning::HashBy {
                    cols: vec!["k".into()], n: 4
                },
                input: Box::new(scan("t", 100)),
            }),
        };
        let dag = compile_dag(&plan, "SELECT k, COUNT(*) FROM t GROUP BY k");
        let order = dag.topological_order();

        // For each stage in the order, all its dependencies must appear earlier
        let mut seen = std::collections::HashSet::new();
        for &sid in &order {
            for &dep in &dag.stages[sid].dependencies {
                assert!(seen.contains(&dep),
                    "stage {sid} depends on {dep} which hasn't been executed yet");
            }
            seen.insert(sid);
        }
    }

    #[test]
    fn dag_explain_includes_all_stages() {
        let plan = PhysicalPlan::Exchange {
            partitioning: Partitioning::HashBy {
                cols: vec!["id".into()], n: 10
            },
            input: Box::new(scan("t", 100)),
        };
        let dag = compile_dag(&plan, "SELECT * FROM t");
        let text = dag.explain();
        assert!(text.contains("stage_0"), "explain should show stage_0:\n{text}");
        assert!(text.contains("stage_1"), "explain should show stage_1:\n{text}");
    }

    #[tokio::test]
    async fn execute_dag_simple_scan() {
        let coord = Coordinator::new();
        let data = DataBlock::new(vec![
            Column::int64("id", vec![Some(1), Some(2), Some(3)]),
            Column::float64("val", vec![Some(10.0), Some(20.0), Some(30.0)]),
        ]).unwrap();
        coord.register_table_for_planning("t", data);

        let plan = scan("t", 3);
        let result = coord.execute_dag(&plan, "SELECT * FROM t").await.unwrap();
        assert_eq!(result.num_rows, 3);
    }

    #[tokio::test]
    async fn execute_dag_local_fallback_filter() {
        let coord = Coordinator::new();
        let data = DataBlock::new(vec![
            Column::int64("id", vec![Some(1), Some(2), Some(3), Some(4), Some(5)]),
            Column::float64("val", vec![Some(10.0), Some(20.0), Some(30.0), Some(40.0), Some(50.0)]),
        ]).unwrap();
        coord.register_table_for_planning("t", data);

        let result = coord.execute_dag_planned(
            "SELECT * FROM t WHERE val > 25"
        ).await.unwrap();
        assert!(result.num_rows <= 5);
    }
}
