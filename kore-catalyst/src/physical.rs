//! Physical plan tree — the concrete execution DAG the coordinator dispatches.
//!
//! Modeled after Spark's `SparkPlan`. Each node describes *how* an operator
//! runs (which join strategy, whether to shuffle, etc.), not just *what* it
//! computes.
//!
//! # Node kinds
//!
//! | Node | Meaning |
//! |------|---------|
//! | `Scan`               | Table source. May carry pushed filter + column pruning. |
//! | `Filter`             | Row filter (WHERE clause). |
//! | `Project`            | Column projection / expression evaluation. |
//! | `HashAggregate`      | Group-by + aggregations. `Partial` on map side, `Final` after Exchange. |
//! | `Exchange`           | Repartition rows across workers by a partitioning scheme (shuffle). |
//! | `Sort`               | Order rows by key(s). |
//! | `Limit`              | Row-count cap. |
//! | `BroadcastHashJoin`  | Ship small side to all workers, local hash-join. |
//! | `ShuffleHashJoin`    | Repartition both sides by join key, then local hash-join. |
//! | `SortMergeJoin`      | Sort both sides then merge — for very large equi-joins. |
//!
//! # Construction
//!
//! `plan_query(&query, &catalog)` walks the logical `Query` from `kore-sql`
//! and produces a `PhysicalPlan`, choosing join strategies from catalog
//! statistics (`plan_join` heuristic below).

use kore_catalog::Catalog;
use kore_sql::ast::*;

// ─── Partitioning ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Partitioning {
    /// One partition — no split.
    Single,
    /// Hash-partition by these columns into `n` buckets.
    HashBy { cols: Vec<String>, n: usize },
    /// Range-partition (used for global ORDER BY).
    Range { col: String, n: usize, ascending: bool },
    /// Round-robin.
    RoundRobin(usize),
    /// Broadcast: every partition sees a full copy.
    Broadcast,
}

// ─── Aggregate mode (partial vs. final) ──────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggMode {
    /// Map-side / worker-local pre-aggregation.
    Partial,
    /// Reducer-side final aggregation.
    Final,
    /// Single-node: full aggregation, no exchange.
    Complete,
}

// ─── Join strategies ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinStrategy {
    BroadcastHash,
    ShuffleHash,
    SortMerge,
    NestedLoop,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JoinCond {
    pub left_col:  String,
    pub right_col: String,
}

// ─── Physical plan node ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum PhysicalPlan {
    Scan {
        table: String,
        projected_cols: Option<Vec<String>>,
        pushed_filter:  Option<Expr>,
        est_rows:       usize,
    },
    Filter {
        predicate: Expr,
        input:     Box<PhysicalPlan>,
    },
    Project {
        exprs: Vec<Projection>,
        input: Box<PhysicalPlan>,
    },
    HashAggregate {
        keys:  Vec<String>,
        aggs:  Vec<Projection>,
        mode:  AggMode,
        input: Box<PhysicalPlan>,
    },
    Exchange {
        partitioning: Partitioning,
        input:        Box<PhysicalPlan>,
    },
    Sort {
        keys:  Vec<OrderByItem>,
        input: Box<PhysicalPlan>,
    },
    Limit {
        n:     u64,
        input: Box<PhysicalPlan>,
    },
    Join {
        strategy:  JoinStrategy,
        join_type: JoinKind,
        left:      Box<PhysicalPlan>,
        right:     Box<PhysicalPlan>,
        on:        Vec<JoinCond>,
    },
    Union {
        inputs: Vec<PhysicalPlan>,
    },
}

impl PhysicalPlan {
    /// Estimated row count at this node's output. Coarse — used for join
    /// strategy selection and to feed cost estimation.
    pub fn est_rows(&self) -> usize {
        match self {
            Self::Scan { est_rows, .. } => *est_rows,
            Self::Filter { input, .. } => (input.est_rows() as f64 * 0.1) as usize,
            Self::Project { input, .. } => input.est_rows(),
            Self::HashAggregate { input, .. } => (input.est_rows() / 10).max(1),
            Self::Exchange { input, .. } => input.est_rows(),
            Self::Sort { input, .. } => input.est_rows(),
            Self::Limit { n, input } => (*n as usize).min(input.est_rows()),
            Self::Join { left, right, strategy, .. } => match strategy {
                JoinStrategy::BroadcastHash => left.est_rows(),
                _ => left.est_rows().saturating_add(right.est_rows()),
            },
            Self::Union { inputs } => inputs.iter().map(|i| i.est_rows()).sum(),
        }
    }

    /// Pretty-print as a tree (Spark `explain()`-style).
    pub fn explain(&self) -> String {
        let mut out = String::new();
        self.explain_indent(0, &mut out);
        out
    }

    fn explain_indent(&self, depth: usize, out: &mut String) {
        let pad = "  ".repeat(depth);
        match self {
            Self::Scan { table, projected_cols, pushed_filter, est_rows } => {
                out.push_str(&format!("{pad}Scan[{table}] est={est_rows}"));
                if let Some(cols) = projected_cols {
                    out.push_str(&format!(" cols=[{}]", cols.join(",")));
                }
                if pushed_filter.is_some() {
                    out.push_str(" (filter pushed)");
                }
                out.push('\n');
            }
            Self::Filter { predicate, input } => {
                out.push_str(&format!("{pad}Filter[{}]\n", debug_expr(predicate)));
                input.explain_indent(depth + 1, out);
            }
            Self::Project { exprs, input } => {
                out.push_str(&format!("{pad}Project[{} exprs]\n", exprs.len()));
                input.explain_indent(depth + 1, out);
            }
            Self::HashAggregate { keys, aggs, mode, input } => {
                out.push_str(&format!(
                    "{pad}HashAggregate[{mode:?}] keys=[{}] aggs={}\n",
                    keys.join(","), aggs.len()
                ));
                input.explain_indent(depth + 1, out);
            }
            Self::Exchange { partitioning, input } => {
                out.push_str(&format!("{pad}Exchange[{partitioning:?}]\n"));
                input.explain_indent(depth + 1, out);
            }
            Self::Sort { keys, input } => {
                out.push_str(&format!("{pad}Sort[{} keys]\n", keys.len()));
                input.explain_indent(depth + 1, out);
            }
            Self::Limit { n, input } => {
                out.push_str(&format!("{pad}Limit[{n}]\n"));
                input.explain_indent(depth + 1, out);
            }
            Self::Join { strategy, join_type, left, right, on } => {
                out.push_str(&format!("{pad}Join[{strategy:?} {join_type:?}] on={} conds\n", on.len()));
                left.explain_indent(depth + 1, out);
                right.explain_indent(depth + 1, out);
            }
            Self::Union { inputs } => {
                out.push_str(&format!("{pad}Union[{}]\n", inputs.len()));
                for i in inputs { i.explain_indent(depth + 1, out); }
            }
        }
    }
}

fn debug_expr(e: &Expr) -> String {
    // Keep it terse — this is just for explain().
    format!("{e:?}")
        .chars()
        .take(60)
        .collect::<String>()
}

// ─── Planner ──────────────────────────────────────────────────────────────────

/// Broadcast threshold (rows). Matches `kore-distributed::planner`.
fn broadcast_row_threshold() -> usize {
    std::env::var("KORE_BROADCAST_ROWS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000)
}

/// Number of shuffle partitions (matches Spark's `spark.sql.shuffle.partitions`
/// default of 200; scale down for small clusters).
fn shuffle_partitions() -> usize {
    std::env::var("KORE_SHUFFLE_PARTITIONS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200)
}

/// Pick a join strategy given left/right cardinalities.
pub fn choose_join_strategy(left_rows: usize, right_rows: usize) -> JoinStrategy {
    let smaller = left_rows.min(right_rows);
    let larger  = left_rows.max(right_rows);
    if smaller <= broadcast_row_threshold() {
        JoinStrategy::BroadcastHash
    } else if larger > 10_000_000 {
        JoinStrategy::SortMerge
    } else {
        JoinStrategy::ShuffleHash
    }
}

/// Translate a logical `Query` into a physical plan.
///
/// Applies these rewrites while building:
///  * predicate pushdown into `Scan.pushed_filter`
///  * column pruning via `Scan.projected_cols`
///  * partial-then-final aggregation (with `Exchange` between)
///  * join strategy selection by cardinality
///  * `Limit` propagation
pub fn plan_query(query: &Query, catalog: &Catalog) -> Option<PhysicalPlan> {
    let stmt = query.body.as_ref()?;
    Some(plan_select(stmt, catalog))
}

fn plan_select(stmt: &SelectStmt, catalog: &Catalog) -> PhysicalPlan {
    // 1. Base scan for the FROM table.
    let base_rows = catalog.get(&stmt.from.name).map(|m| m.row_count).unwrap_or(1_000);
    let referenced = referenced_cols(stmt);
    let projected = if referenced.is_empty() { None } else { Some(referenced.clone()) };
    let mut plan = PhysicalPlan::Scan {
        table:          stmt.from.name.clone(),
        projected_cols: projected,
        pushed_filter:  stmt.where_clause.clone(),   // pushed → suppresses top-level Filter
        est_rows:       base_rows,
    };

    // 2. Joins — cardinality-based strategy selection.
    let mut left_rows = base_rows;
    for j in &stmt.joins {
        let right_rows = catalog.get(&j.table.name).map(|m| m.row_count).unwrap_or(1_000);
        let strategy = choose_join_strategy(left_rows, right_rows);

        let right_scan = PhysicalPlan::Scan {
            table:          j.table.name.clone(),
            projected_cols: None,
            pushed_filter:  None,
            est_rows:       right_rows,
        };

        let (l_input, r_input) = match strategy {
            JoinStrategy::BroadcastHash => {
                // Broadcast the smaller side.
                if right_rows <= left_rows {
                    (plan, PhysicalPlan::Exchange {
                        partitioning: Partitioning::Broadcast,
                        input:        Box::new(right_scan),
                    })
                } else {
                    (PhysicalPlan::Exchange {
                        partitioning: Partitioning::Broadcast,
                        input:        Box::new(plan),
                    }, right_scan)
                }
            }
            JoinStrategy::ShuffleHash | JoinStrategy::SortMerge => {
                let n = shuffle_partitions();
                let l = PhysicalPlan::Exchange {
                    partitioning: Partitioning::HashBy {
                        cols: vec![j.on.left_col.clone()], n,
                    },
                    input: Box::new(plan),
                };
                let r = PhysicalPlan::Exchange {
                    partitioning: Partitioning::HashBy {
                        cols: vec![j.on.right_col.clone()], n,
                    },
                    input: Box::new(right_scan),
                };
                (l, r)
            }
            JoinStrategy::NestedLoop => (plan, right_scan),
        };

        plan = PhysicalPlan::Join {
            strategy,
            join_type: j.join_type.clone(),
            left:      Box::new(l_input),
            right:     Box::new(r_input),
            on:        vec![JoinCond {
                left_col:  j.on.left_col.clone(),
                right_col: j.on.right_col.clone(),
            }],
        };
        // For subsequent joins we treat the join output as the new "left".
        left_rows = match strategy {
            JoinStrategy::BroadcastHash => left_rows,
            _ => left_rows.saturating_add(right_rows),
        };
    }

    // 3. Filter — only wrap when we didn't push it into the scan (i.e. after joins).
    if !stmt.joins.is_empty() {
        if let Some(w) = &stmt.where_clause {
            plan = PhysicalPlan::Filter {
                predicate: w.clone(),
                input:     Box::new(plan),
            };
        }
    }

    // 4. Aggregation — split into partial + exchange + final.
    let has_agg = stmt.projections.iter().any(|p| projection_has_agg(p));
    if !stmt.group_by.is_empty() || has_agg {
        let keys = stmt.group_by.clone();
        let aggs = stmt.projections.clone();
        plan = PhysicalPlan::HashAggregate {
            keys:  keys.clone(),
            aggs:  aggs.clone(),
            mode:  AggMode::Partial,
            input: Box::new(plan),
        };
        // Exchange by grouping keys, then final aggregate.
        let n = shuffle_partitions();
        let partitioning = if keys.is_empty() {
            Partitioning::Single
        } else {
            Partitioning::HashBy { cols: keys.clone(), n }
        };
        plan = PhysicalPlan::Exchange { partitioning, input: Box::new(plan) };
        plan = PhysicalPlan::HashAggregate {
            keys, aggs, mode: AggMode::Final, input: Box::new(plan),
        };
    } else {
        // Simple projection over post-filter plan.
        plan = PhysicalPlan::Project {
            exprs: stmt.projections.clone(),
            input: Box::new(plan),
        };
    }

    // 5. Sort + Limit.
    if !stmt.order_by.is_empty() {
        plan = PhysicalPlan::Sort { keys: stmt.order_by.clone(), input: Box::new(plan) };
    }
    if let Some(n) = stmt.limit {
        plan = PhysicalPlan::Limit { n, input: Box::new(plan) };
    }
    plan
}

fn referenced_cols(stmt: &SelectStmt) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    for p in &stmt.projections {
        if let Projection::Expr { expr, .. } = p {
            collect_cols(expr, &mut out);
        }
        // Projection::Star means all cols → return empty to signal "no pruning".
        if matches!(p, Projection::Star) { return vec![]; }
    }
    if let Some(w) = &stmt.where_clause { collect_cols(w, &mut out); }
    for k in &stmt.group_by { out.insert(k.clone()); }
    if let Some(h) = &stmt.having { collect_cols(h, &mut out); }
    out.into_iter().collect()
}

fn collect_cols(e: &Expr, out: &mut std::collections::BTreeSet<String>) {
    match e {
        Expr::Col(c) => { out.insert(c.clone()); }
        Expr::QualCol(_, c) => { out.insert(c.clone()); }
        Expr::BinOp { left, right, .. } => { collect_cols(left, out); collect_cols(right, out); }
        Expr::Not(inner) | Expr::IsNull(inner) | Expr::IsNotNull(inner) => collect_cols(inner, out),
        Expr::Agg { expr, .. } => collect_cols(expr, out),
        Expr::Case { operand, branches, else_val } => {
            if let Some(o) = operand { collect_cols(o, out); }
            for (c, r) in branches { collect_cols(c, out); collect_cols(r, out); }
            if let Some(e) = else_val { collect_cols(e, out); }
        }
        Expr::In { expr, values, .. } => {
            collect_cols(expr, out);
            for v in values { collect_cols(v, out); }
        }
        Expr::Between { expr, low, high, .. } => {
            collect_cols(expr, out); collect_cols(low, out); collect_cols(high, out);
        }
        Expr::Like { expr, pattern, .. } => { collect_cols(expr, out); collect_cols(pattern, out); }
        Expr::FuncCall { args, .. } => { for a in args { collect_cols(a, out); } }
        _ => {}
    }
}

fn projection_has_agg(p: &Projection) -> bool {
    match p {
        Projection::Star => false,
        Projection::Expr { expr, .. } => expr_has_agg(expr),
    }
}

fn expr_has_agg(e: &Expr) -> bool {
    match e {
        Expr::Agg { .. } => true,
        Expr::BinOp { left, right, .. } => expr_has_agg(left) || expr_has_agg(right),
        Expr::Not(inner) | Expr::IsNull(inner) | Expr::IsNotNull(inner) => expr_has_agg(inner),
        Expr::FuncCall { args, .. } => args.iter().any(expr_has_agg),
        _ => false,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};
    use kore_sql::parser::parse_query;

    fn make_catalog(orders: usize, users: usize) -> Catalog {
        let mut cat = Catalog::new();
        cat.analyze("orders", &DataBlock {
            num_rows: orders,
            columns: vec![Column { name: "id".into(), data: ColumnData::Int64(vec![]) }],
        });
        cat.analyze("users",  &DataBlock {
            num_rows: users,
            columns: vec![Column { name: "id".into(), data: ColumnData::Int64(vec![]) }],
        });
        cat
    }

    #[test]
    fn plan_simple_filter_projection() {
        let q = parse_query("SELECT id FROM orders WHERE id > 5").unwrap();
        let cat = make_catalog(1000, 1);
        let plan = plan_query(&q, &cat).expect("plan");
        // Expect: Project ← Scan(filter pushed)
        let text = plan.explain();
        assert!(text.contains("Scan[orders]"), "explain missing Scan:\n{text}");
        assert!(text.contains("(filter pushed)"), "expected pushdown:\n{text}");
        assert!(text.contains("Project"),        "expected Project:\n{text}");
    }

    #[test]
    fn plan_group_by_inserts_exchange() {
        let q = parse_query(
            "SELECT region, SUM(sales) AS total FROM orders GROUP BY region"
        ).unwrap();
        let cat = make_catalog(1_000_000, 1);
        let plan = plan_query(&q, &cat).expect("plan");
        let text = plan.explain();
        // Should have Partial → Exchange → Final aggregate.
        assert!(text.contains("HashAggregate[Final]"), "missing Final agg:\n{text}");
        assert!(text.contains("HashAggregate[Partial]"), "missing Partial agg:\n{text}");
        assert!(text.contains("Exchange[HashBy"), "missing HashBy exchange:\n{text}");
    }

    #[test]
    fn plan_join_picks_broadcast_for_small_dim() {
        let q = parse_query(
            "SELECT * FROM orders INNER JOIN users ON orders.uid = users.id"
        ).unwrap();
        let cat = make_catalog(10_000_000, 100);
        let plan = plan_query(&q, &cat).expect("plan");
        let text = plan.explain();
        assert!(text.contains("Join[BroadcastHash"),
            "expected BroadcastHash for small dim:\n{text}");
        assert!(text.contains("Broadcast"), "expected Broadcast exchange:\n{text}");
    }

    #[test]
    fn plan_join_picks_sortmerge_for_large_x_large() {
        let q = parse_query(
            "SELECT * FROM orders INNER JOIN users ON orders.uid = users.id"
        ).unwrap();
        // Both sides > 10M → sort-merge join.
        let cat = make_catalog(50_000_000, 20_000_000);
        let plan = plan_query(&q, &cat).expect("plan");
        let text = plan.explain();
        assert!(text.contains("Join[SortMerge"),
            "expected SortMerge for large*large:\n{text}");
    }

    #[test]
    fn choose_join_strategy_ranges() {
        std::env::remove_var("KORE_BROADCAST_ROWS");
        assert_eq!(choose_join_strategy(10_000_000, 100), JoinStrategy::BroadcastHash);
        assert_eq!(choose_join_strategy(1_000_000, 2_000_000), JoinStrategy::ShuffleHash);
        assert_eq!(choose_join_strategy(50_000_000, 30_000_000), JoinStrategy::SortMerge);
    }

    #[test]
    fn est_rows_after_filter_shrinks() {
        let scan = PhysicalPlan::Scan {
            table: "t".into(), projected_cols: None, pushed_filter: None, est_rows: 1_000,
        };
        let filt = PhysicalPlan::Filter {
            predicate: Expr::Bool(true), input: Box::new(scan),
        };
        assert!(filt.est_rows() < 1_000);
    }

    #[test]
    fn plan_limit_wraps_top() {
        let q = parse_query("SELECT * FROM orders LIMIT 42").unwrap();
        let cat = make_catalog(1_000, 1);
        let plan = plan_query(&q, &cat).expect("plan");
        // Top of tree should be Limit
        assert!(matches!(plan, PhysicalPlan::Limit { n: 42, .. }),
            "expected Limit at root:\n{}", plan.explain());
    }
}
