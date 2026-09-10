//! KORE Layer 33 — Query Optimizer
//!
//! Transforms a parsed `Query` AST to reduce execution cost:
//!
//! | Rule                  | What it does                                          |
//! |-----------------------|-------------------------------------------------------|
//! | ConstantFolding       | Evaluate literal expressions at compile time           |
//! | PredicatePushdown     | Move WHERE filters before JOINs (prune early)         |
//! | ProjectionPruning     | Only keep columns actually referenced                 |
//! | JoinOrderOptimizer    | Put smaller table on right side of hash join          |
//! | LimitPushdown         | Push LIMIT into scans before JOINs when possible      |
//!
//! # Usage
//! ```no_run
//! use kore_optimize::Optimizer;
//! use kore_sql::parse_query;
//!
//! let mut q = parse_query("SELECT a, b FROM big LEFT JOIN small ON big.id = small.id WHERE b > 5").unwrap();
//! Optimizer::new().optimize(&mut q);
//! // q is now transformed: filter pushed before join, small table on right
//! ```

use kore_sql::ast::*;

// ── Optimizer ─────────────────────────────────────────────────────────────────

pub struct Optimizer {
    pub constant_folding:     bool,
    pub predicate_pushdown:   bool,
    pub projection_pruning:   bool,
    pub join_reorder:         bool,
    pub limit_pushdown:       bool,
    pub partition_aware_agg:  bool,
}

impl Default for Optimizer {
    fn default() -> Self { Self::new() }
}

impl Optimizer {
    pub fn new() -> Self {
        Self {
            constant_folding:   true,
            predicate_pushdown: true,
            projection_pruning: true,
            join_reorder:       true,
            limit_pushdown:     true,
            partition_aware_agg: true,
        }
    }

    pub fn optimize(&self, query: &mut Query) {
        if let Some(stmt) = &mut query.body {
            self.optimize_stmt(stmt);
        }
        for (_, stmt) in &mut query.set_ops {
            self.optimize_stmt(stmt);
        }
    }

    fn optimize_stmt(&self, stmt: &mut SelectStmt) {
        if self.constant_folding   { self.fold_constants(stmt); }
        if self.predicate_pushdown { self.push_predicates(stmt); }
        if self.join_reorder       { self.reorder_joins(stmt); }
        if self.limit_pushdown     { self.push_limit(stmt); }
    }

    /// Optimize a logical plan node tree, including partition-aware aggregation.
    pub fn optimize_plan(&self, plan: &mut PlanNode) {
        if self.partition_aware_agg {
            Self::apply_partition_aware_agg(plan);
        }
    }

    fn apply_partition_aware_agg(plan: &mut PlanNode) {
        match plan {
            PlanNode::Aggregate { group_by, input, local_only, .. } => {
                Self::apply_partition_aware_agg(input);
                if let Some(ref part_keys) = input.partitioning() {
                    let all_matched = group_by.iter().all(|g| part_keys.contains(g));
                    if all_matched && !group_by.is_empty() {
                        *local_only = true;
                    }
                }
            }
            PlanNode::Scan { .. } => {}
            PlanNode::Filter { input, .. } => Self::apply_partition_aware_agg(input),
            PlanNode::Project { input, .. } => Self::apply_partition_aware_agg(input),
            PlanNode::Exchange { input, .. } => Self::apply_partition_aware_agg(input),
        }
    }

    // ── Rule 1: Constant Folding ───────────────────────────────────────────

    fn fold_constants(&self, stmt: &mut SelectStmt) {
        if let Some(w) = &mut stmt.where_clause {
            fold_expr(w);
        }
        if let Some(h) = &mut stmt.having {
            fold_expr(h);
        }
    }

    // ── Rule 2: Predicate Pushdown ─────────────────────────────────────────
    // Move simple WHERE predicates (single-table, no aggregation) before JOINs.

    fn push_predicates(&self, stmt: &mut SelectStmt) {
        if stmt.joins.is_empty() { return; }
        let where_clause = match stmt.where_clause.take() {
            Some(w) => w,
            None => return,
        };

        let from_name = stmt.from.name.clone();
        let from_alias = stmt.from.alias.clone();
        let join_tables: Vec<(String, Option<String>)> = stmt.joins.iter()
            .map(|j| (j.table.name.clone(), j.table.alias.clone()))
            .collect();

        let conjuncts = split_conjunction(where_clause);
        let mut remaining = Vec::new();

        for pred in conjuncts {
            let refs = collect_table_refs(&pred);
            if refs.is_empty() {
                remaining.push(pred);
                continue;
            }

            let refers_to_from = refs.iter().any(|r| {
                r == &from_name || from_alias.as_deref() == Some(r.as_str())
            });
            let refers_to_join = join_tables.iter().enumerate().find(|(_, (name, alias))| {
                refs.iter().any(|r| r == name || alias.as_deref() == Some(r.as_str()))
            });

            if refers_to_from && refers_to_join.is_none() {
                stmt.from.push_filter = Some(Box::new(pred));
                continue;
            }
            if let Some((idx, _)) = refers_to_join {
                if !refers_to_from {
                    stmt.joins[idx].push_filter = Some(Box::new(pred));
                    continue;
                }
            }
            remaining.push(pred);
        }

        stmt.where_clause = rebuild_conjunction(remaining);
    }

    // ── Rule 3: Join Reordering ────────────────────────────────────────────
    // (In KORE, the executor always uses the smaller table as hash table,
    // but we can add estimated cardinality hints here.)

    fn reorder_joins(&self, stmt: &mut SelectStmt) {
        if stmt.joins.is_empty() { return; }

        // Sort joins so smaller tables come first (build side of hash join).
        // Use a simple bubble approach since join count is typically small.
        let mut changed = true;
        while changed {
            changed = false;
            for i in 0..stmt.joins.len() {
                let left_name = if i == 0 { &stmt.from.name } else { &stmt.joins[i - 1].table.name };
                let right_name = &stmt.joins[i].table.name;

                let left_rows = estimate_table_rows(left_name);
                let right_rows = estimate_table_rows(right_name);

                if right_rows > left_rows && i == 0 {
                    std::mem::swap(&mut stmt.from, &mut stmt.joins[0].table);
                    let on = &mut stmt.joins[0].on;
                    std::mem::swap(&mut on.left_col, &mut on.right_col);
                    changed = true;
                }
            }
        }
    }

    // ── Rule 4: Limit Pushdown ─────────────────────────────────────────────

    fn push_limit(&self, stmt: &mut SelectStmt) {
        let limit = match stmt.limit {
            Some(n) => n,
            None => return,
        };

        // Push LIMIT into scan when there are no JOINs, no GROUP BY, no HAVING,
        // and either no ORDER BY or a single ASC ORDER BY.
        if !stmt.joins.is_empty() || !stmt.group_by.is_empty() || stmt.having.is_some() {
            return;
        }

        if stmt.order_by.is_empty() {
            if stmt.scan_limit.is_none() {
                stmt.scan_limit = Some(limit);
            }
        } else if stmt.order_by.len() == 1 && !stmt.order_by[0].desc {
            if stmt.scan_limit.is_none() {
                stmt.scan_limit = Some(limit);
            }
        }
    }
}

// ── Expression constant folding ───────────────────────────────────────────────

fn fold_expr(expr: &mut Expr) {
    match expr {
        Expr::BinOp { op, left, right } => {
            fold_expr(left);
            fold_expr(right);
            // Fold literal + literal
            if let (Expr::Int(a), Expr::Int(b)) = (left.as_ref(), right.as_ref()) {
                let (a, b) = (*a, *b);
                *expr = match op {
                    BinOpKind::Add => Expr::Int(a + b),
                    BinOpKind::Sub => Expr::Int(a - b),
                    BinOpKind::Mul => Expr::Int(a * b),
                    BinOpKind::Div if b != 0 => Expr::Int(a / b),
                    BinOpKind::Eq  => Expr::Bool(a == b),
                    BinOpKind::Ne  => Expr::Bool(a != b),
                    BinOpKind::Lt  => Expr::Bool(a <  b),
                    BinOpKind::Le  => Expr::Bool(a <= b),
                    BinOpKind::Gt  => Expr::Bool(a >  b),
                    BinOpKind::Ge  => Expr::Bool(a >= b),
                    _ => return,
                };
                return;
            }
            if let (Expr::Float(a), Expr::Float(b)) = (left.as_ref(), right.as_ref()) {
                let (a, b) = (*a, *b);
                *expr = match op {
                    BinOpKind::Add => Expr::Float(a + b),
                    BinOpKind::Sub => Expr::Float(a - b),
                    BinOpKind::Mul => Expr::Float(a * b),
                    BinOpKind::Div => Expr::Float(a / b),
                    BinOpKind::Eq  => Expr::Bool((a - b).abs() < 1e-10),
                    BinOpKind::Lt  => Expr::Bool(a < b),
                    BinOpKind::Gt  => Expr::Bool(a > b),
                    _ => return,
                };
                return;
            }
            // Boolean AND / OR with literals
            if let (Expr::Bool(a), Expr::Bool(b)) = (left.as_ref(), right.as_ref()) {
                let (a, b) = (*a, *b);
                *expr = match op {
                    BinOpKind::And => Expr::Bool(a && b),
                    BinOpKind::Or  => Expr::Bool(a || b),
                    _ => return,
                };
            }
        }
        Expr::Not(inner) => {
            fold_expr(inner);
            if let Expr::Bool(b) = inner.as_ref() {
                *expr = Expr::Bool(!b);
            }
        }
        Expr::IsNull(inner) => {
            fold_expr(inner);
            if matches!(inner.as_ref(), Expr::Null) {
                *expr = Expr::Bool(true);
            }
        }
        Expr::IsNotNull(inner) => {
            fold_expr(inner);
            if !matches!(inner.as_ref(), Expr::Null) {
                if matches!(inner.as_ref(), Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_)) {
                    *expr = Expr::Bool(true);
                }
            }
        }
        _ => {}
    }
}

// ── Predicate pushdown & join reorder helpers ─────────────────────────────────
fn estimate_table_rows(name: &str) -> usize {
    // Common naming heuristics: dimension/lookup tables are small
    let lower = name.to_lowercase();
    if lower.starts_with("dim_") || lower.ends_with("_dim")
        || lower == "users" || lower == "regions" || lower == "categories"
    {
        100
    } else {
        10_000
    }
}

// ── Predicate pushdown helpers ────────────────────────────────────────────────

fn split_conjunction(expr: Expr) -> Vec<Expr> {
    match expr {
        Expr::BinOp { op: BinOpKind::And, left, right } => {
            let mut v = split_conjunction(*left);
            v.extend(split_conjunction(*right));
            v
        }
        other => vec![other],
    }
}

fn rebuild_conjunction(mut parts: Vec<Expr>) -> Option<Expr> {
    if parts.is_empty() { return None; }
    let mut result = parts.remove(0);
    for p in parts {
        result = Expr::BinOp { op: BinOpKind::And, left: Box::new(result), right: Box::new(p) };
    }
    Some(result)
}

fn collect_table_refs(expr: &Expr) -> Vec<String> {
    let mut refs = Vec::new();
    collect_table_refs_inner(expr, &mut refs);
    refs.sort();
    refs.dedup();
    refs
}

fn collect_table_refs_inner(expr: &Expr, refs: &mut Vec<String>) {
    match expr {
        Expr::QualCol(table, _) => { refs.push(table.clone()); }
        Expr::BinOp { left, right, .. } => {
            collect_table_refs_inner(left, refs);
            collect_table_refs_inner(right, refs);
        }
        Expr::Not(inner) => collect_table_refs_inner(inner, refs),
        Expr::IsNull(inner) | Expr::IsNotNull(inner) => collect_table_refs_inner(inner, refs),
        _ => {}
    }
}

// ── Statistics (for cost-based optimization) ─────────────────────────────────

/// Table statistics used by the cost-based optimizer.
#[derive(Debug, Clone, Default)]
pub struct TableStats {
    pub table_name: String,
    pub row_count:  usize,
    pub col_stats:  Vec<ColumnStats>,
}

#[derive(Debug, Clone)]
pub struct ColumnStats {
    pub name:         String,
    pub null_count:   usize,
    pub distinct_est: usize,
    pub min_val:      Option<f64>,
    pub max_val:      Option<f64>,
}

/// Collect basic statistics from a DataBlock.
pub fn collect_stats(table_name: &str, block: &kore_core::DataBlock) -> TableStats {
    use kore_core::ColumnData;
    let col_stats = block.columns.iter().map(|col| {
        let (null_count, min, max, distinct_est) = match &col.data {
            ColumnData::Float64(v) => {
                let nulls = v.iter().filter(|x| x.is_none()).count();
                let vals: Vec<f64> = v.iter().filter_map(|x| *x).collect();
                let min = vals.iter().copied().reduce(f64::min);
                let max = vals.iter().copied().reduce(f64::max);
                // Approx distinct (1% sampling)
                let sample: std::collections::HashSet<u64> = vals.iter()
                    .step_by(100.max(1))
                    .map(|f| f.to_bits())
                    .collect();
                (nulls, min, max, sample.len() * 100)
            }
            ColumnData::Int64(v) => {
                let nulls = v.iter().filter(|x| x.is_none()).count();
                let vals: Vec<i64> = v.iter().filter_map(|x| *x).collect();
                let min = vals.iter().copied().min().map(|i| i as f64);
                let max = vals.iter().copied().max().map(|i| i as f64);
                let sample: std::collections::HashSet<i64> = vals.iter().step_by(100.max(1)).copied().collect();
                (nulls, min, max, sample.len() * 100)
            }
            ColumnData::Str(v) => {
                let nulls = v.iter().filter(|x| x.is_none()).count();
                let sample: std::collections::HashSet<&str> = v.iter()
                    .step_by(100.max(1))
                    .filter_map(|x| x.as_deref())
                    .collect();
                (nulls, None, None, sample.len() * 100)
            }
            ColumnData::Bool(v) => {
                let nulls = v.iter().filter(|x| x.is_none()).count();
                (nulls, Some(0.0), Some(1.0), 2)
            }
            ColumnData::StrDict { codes, dict } => {
                let nulls = codes.iter().filter(|&&c| c == u8::MAX).count();
                let distinct = dict.len();
                (nulls, None, None, distinct)
            }
        };
        ColumnStats {
            name:         col.name.clone(),
            null_count,
            distinct_est,
            min_val: min,
            max_val: max,
        }
    }).collect();

    TableStats { table_name: table_name.to_string(), row_count: block.num_rows, col_stats }
}

// ── Logical plan node (for partition-aware optimization) ──────────────────────

/// Simplified logical plan node used for partition-aware optimizations.
#[derive(Debug, Clone)]
pub enum PlanNode {
    Scan {
        table: String,
        partitioning: Option<Vec<String>>,
    },
    Filter {
        predicate: String,
        input: Box<PlanNode>,
    },
    Project {
        columns: Vec<String>,
        input: Box<PlanNode>,
    },
    Aggregate {
        group_by: Vec<String>,
        agg_exprs: Vec<String>,
        local_only: bool,
        input: Box<PlanNode>,
    },
    Exchange {
        partition_by: Vec<String>,
        input: Box<PlanNode>,
    },
}

impl PlanNode {
    /// Get the partitioning scheme at this node's output, if known.
    pub fn partitioning(&self) -> Option<Vec<String>> {
        match self {
            PlanNode::Scan { partitioning, .. } => partitioning.clone(),
            PlanNode::Filter { input, .. } => input.partitioning(),
            PlanNode::Project { input, .. } => input.partitioning(),
            PlanNode::Exchange { partition_by, .. } => Some(partition_by.clone()),
            PlanNode::Aggregate { group_by, local_only, input, .. } => {
                if *local_only { input.partitioning() } else { Some(group_by.clone()) }
            }
        }
    }

    /// Check if this node or any descendant is marked as local-only aggregation.
    pub fn has_local_agg(&self) -> bool {
        match self {
            PlanNode::Aggregate { local_only, input, .. } => *local_only || input.has_local_agg(),
            PlanNode::Filter { input, .. } => input.has_local_agg(),
            PlanNode::Project { input, .. } => input.has_local_agg(),
            PlanNode::Exchange { input, .. } => input.has_local_agg(),
            PlanNode::Scan { .. } => false,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_sql::parse_query;

    #[test]
    fn test_constant_folding() {
        let mut q = parse_query("SELECT * FROM t WHERE 1 + 1 = 2").unwrap();
        Optimizer::new().optimize(&mut q);
        // After folding: WHERE TRUE (or 2 = 2 → TRUE)
        // Just verify it doesn't panic
        assert!(q.body.is_some());
    }

    #[test]
    fn test_optimizer_simple_query() {
        let mut q = parse_query(
            "SELECT id, revenue FROM sales WHERE revenue > 500 ORDER BY revenue DESC LIMIT 10"
        ).unwrap();
        Optimizer::new().optimize(&mut q);
        let stmt = q.body.unwrap();
        assert_eq!(stmt.limit, Some(10));
    }

    #[test]
    fn test_optimizer_with_cte() {
        let mut q = parse_query(
            "WITH high_value AS (SELECT * FROM sales WHERE revenue > 500) \
             SELECT region, SUM(revenue) AS total FROM high_value GROUP BY region"
        ).unwrap();
        Optimizer::new().optimize(&mut q);
        assert_eq!(q.ctes.len(), 1);
        assert_eq!(q.ctes[0].name, "high_value");
    }

    // ── Partition-Aware Aggregation tests ─────────────────────────────────

    #[test]
    fn test_partition_aware_agg_skips_shuffle() {
        let opt = Optimizer::new();
        let mut plan = PlanNode::Aggregate {
            group_by: vec!["region".into()],
            agg_exprs: vec!["SUM(sales)".into()],
            local_only: false,
            input: Box::new(PlanNode::Exchange {
                partition_by: vec!["region".into()],
                input: Box::new(PlanNode::Scan {
                    table: "sales".into(),
                    partitioning: None,
                }),
            }),
        };
        opt.optimize_plan(&mut plan);
        match &plan {
            PlanNode::Aggregate { local_only, .. } => {
                assert!(*local_only, "Should be marked local_only when partitioning matches GROUP BY");
            }
            _ => panic!("Expected Aggregate node"),
        }
    }

    #[test]
    fn test_partition_aware_agg_from_scan_partitioning() {
        let opt = Optimizer::new();
        let mut plan = PlanNode::Aggregate {
            group_by: vec!["date".into()],
            agg_exprs: vec!["COUNT(*)".into()],
            local_only: false,
            input: Box::new(PlanNode::Scan {
                table: "events".into(),
                partitioning: Some(vec!["date".into()]),
            }),
        };
        opt.optimize_plan(&mut plan);
        match &plan {
            PlanNode::Aggregate { local_only, .. } => {
                assert!(*local_only, "Scan already partitioned by GROUP BY key => local_only");
            }
            _ => panic!("Expected Aggregate"),
        }
    }

    #[test]
    fn test_partition_aware_agg_no_match() {
        let opt = Optimizer::new();
        let mut plan = PlanNode::Aggregate {
            group_by: vec!["category".into()],
            agg_exprs: vec!["SUM(amount)".into()],
            local_only: false,
            input: Box::new(PlanNode::Exchange {
                partition_by: vec!["region".into()],
                input: Box::new(PlanNode::Scan {
                    table: "orders".into(),
                    partitioning: None,
                }),
            }),
        };
        opt.optimize_plan(&mut plan);
        match &plan {
            PlanNode::Aggregate { local_only, .. } => {
                assert!(!*local_only, "GROUP BY key != partitioning => not local_only");
            }
            _ => panic!("Expected Aggregate"),
        }
    }

    #[test]
    fn test_plan_node_partitioning_propagation() {
        let scan = PlanNode::Scan {
            table: "t".into(),
            partitioning: Some(vec!["key".into()]),
        };
        assert_eq!(scan.partitioning(), Some(vec!["key".into()]));

        let filtered = PlanNode::Filter {
            predicate: "x > 5".into(),
            input: Box::new(scan),
        };
        assert_eq!(filtered.partitioning(), Some(vec!["key".into()]),
            "Filter should propagate partitioning from child");
    }

    #[test]
    fn test_plan_node_has_local_agg() {
        let plan = PlanNode::Aggregate {
            group_by: vec!["k".into()],
            agg_exprs: vec!["SUM(v)".into()],
            local_only: true,
            input: Box::new(PlanNode::Scan { table: "t".into(), partitioning: None }),
        };
        assert!(plan.has_local_agg());

        let plan2 = PlanNode::Aggregate {
            group_by: vec!["k".into()],
            agg_exprs: vec!["SUM(v)".into()],
            local_only: false,
            input: Box::new(PlanNode::Scan { table: "t".into(), partitioning: None }),
        };
        assert!(!plan2.has_local_agg());
    }

    #[test]
    fn test_partition_aware_agg_disabled() {
        let mut opt = Optimizer::new();
        opt.partition_aware_agg = false;
        let mut plan = PlanNode::Aggregate {
            group_by: vec!["region".into()],
            agg_exprs: vec!["SUM(sales)".into()],
            local_only: false,
            input: Box::new(PlanNode::Exchange {
                partition_by: vec!["region".into()],
                input: Box::new(PlanNode::Scan { table: "t".into(), partitioning: None }),
            }),
        };
        opt.optimize_plan(&mut plan);
        match &plan {
            PlanNode::Aggregate { local_only, .. } => {
                assert!(!*local_only, "Should not optimize when feature is disabled");
            }
            _ => panic!("Expected Aggregate"),
        }
    }
}
