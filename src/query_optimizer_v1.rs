/// Query Optimizer for KORE v1.6.0
/// 
/// Cost-based query optimizer transforming logical plans to optimal physical plans.
/// Uses statistics from query_statistics_v1 to estimate costs and choose strategies.

use crate::query_statistics_v1::{CostModel, TableStats, Selectivity};

/// Logical plan node - represents logical operations
#[derive(Debug, Clone, PartialEq)]
pub enum LogicalExpr {
    /// Table scan: read all rows from table
    Scan { table: String },
    /// Filter: apply predicate to rows
    Filter {
        input: Box<LogicalExpr>,
        predicate: String, // Simplified as string for now
    },
    /// Project: select specific columns
    Project {
        input: Box<LogicalExpr>,
        columns: Vec<String>,
    },
    /// Join: combine two relations
    Join {
        left: Box<LogicalExpr>,
        right: Box<LogicalExpr>,
        join_keys: Vec<String>,
    },
    /// GroupBy: aggregate rows
    GroupBy {
        input: Box<LogicalExpr>,
        group_keys: Vec<String>,
        aggregates: Vec<String>,
    },
    /// Sort: order rows
    Sort {
        input: Box<LogicalExpr>,
        order_keys: Vec<String>,
    },
}

/// Physical plan node - executable operations
#[derive(Debug, Clone, PartialEq)]
pub enum PhysicalExpr {
    /// Sequential table scan
    SeqScan { table: String, cost: f64 },
    /// Index scan (if indexes available)
    IndexScan { table: String, index: String, cost: f64 },
    /// Filter with predicate
    Filter {
        input: Box<PhysicalExpr>,
        predicate: String,
        selectivity: f64,
    },
    /// Project columns
    Project {
        input: Box<PhysicalExpr>,
        columns: Vec<String>,
    },
    /// Hash join
    HashJoin {
        left: Box<PhysicalExpr>,
        right: Box<PhysicalExpr>,
        join_keys: Vec<String>,
        cost: f64,
    },
    /// Nested loop join
    NestedLoopJoin {
        left: Box<PhysicalExpr>,
        right: Box<PhysicalExpr>,
        join_keys: Vec<String>,
        cost: f64,
    },
    /// Sort-merge join
    SortMergeJoin {
        left: Box<PhysicalExpr>,
        right: Box<PhysicalExpr>,
        join_keys: Vec<String>,
        cost: f64,
    },
    /// GroupBy with aggregation
    GroupBy {
        input: Box<PhysicalExpr>,
        group_keys: Vec<String>,
        aggregates: Vec<String>,
        cost: f64,
    },
    /// Sort operation
    Sort {
        input: Box<PhysicalExpr>,
        order_keys: Vec<String>,
        cost: f64,
    },
}

impl PhysicalExpr {
    /// Estimate total cost of this physical plan
    pub fn total_cost(&self) -> f64 {
        match self {
            PhysicalExpr::SeqScan { cost, .. } => *cost,
            PhysicalExpr::IndexScan { cost, .. } => *cost,
            PhysicalExpr::Filter { input, selectivity, .. } => {
                let child_cost = input.total_cost();
                // Filter doesn't add I/O, just CPU (selectivity applied to child cost)
                child_cost
            }
            PhysicalExpr::Project { input, .. } => input.total_cost(),
            PhysicalExpr::HashJoin { left, right, cost, .. }
            | PhysicalExpr::NestedLoopJoin { left, right, cost, .. }
            | PhysicalExpr::SortMergeJoin { left, right, cost, .. } => {
                left.total_cost() + right.total_cost() + cost
            }
            PhysicalExpr::GroupBy { input, cost, .. } => input.total_cost() + cost,
            PhysicalExpr::Sort { input, cost, .. } => input.total_cost() + cost,
        }
    }

    /// Estimate output rows from this operation
    pub fn estimated_rows(&self, tables_stats: &std::collections::HashMap<String, TableStats>) -> u64 {
        match self {
            PhysicalExpr::SeqScan { table, .. } => {
                tables_stats.get(table).map(|s| s.row_count).unwrap_or(0)
            }
            PhysicalExpr::IndexScan { table, .. } => {
                tables_stats.get(table).map(|s| s.row_count).unwrap_or(0)
            }
            PhysicalExpr::Filter { input, selectivity, .. } => {
                let input_rows = input.estimated_rows(tables_stats);
                ((input_rows as f64) * selectivity) as u64
            }
            PhysicalExpr::Project { input, .. } => input.estimated_rows(tables_stats),
            PhysicalExpr::HashJoin { left, right, .. }
            | PhysicalExpr::NestedLoopJoin { left, right, .. }
            | PhysicalExpr::SortMergeJoin { left, right, .. } => {
                // Very simplified: assume join result is smaller than cross product
                let left_rows = left.estimated_rows(tables_stats);
                let right_rows = right.estimated_rows(tables_stats);
                ((left_rows as f64) * (right_rows as f64) * 0.1).ceil() as u64 // Assume 10% selectivity
            }
            PhysicalExpr::GroupBy { input, group_keys, .. } => {
                // Simplified: distinct values in group keys
                // In practice, would use column statistics
                group_keys.len() as u64 * 100 // Conservative estimate
            }
            PhysicalExpr::Sort { input, .. } => input.estimated_rows(tables_stats),
        }
    }
}

/// Optimization context with table statistics
pub struct OptimizerContext {
    pub table_stats: std::collections::HashMap<String, TableStats>,
    pub cost_model: CostModel,
}

impl OptimizerContext {
    pub fn new(table_stats: std::collections::HashMap<String, TableStats>) -> Self {
        OptimizerContext {
            table_stats,
            cost_model: CostModel::default(),
        }
    }
}

/// Main optimizer: convert logical plan to optimized physical plan
pub fn optimize(
    logical: LogicalExpr,
    context: &OptimizerContext,
) -> Result<PhysicalExpr, String> {
    optimize_internal(&logical, context)
}

fn optimize_internal(
    expr: &LogicalExpr,
    context: &OptimizerContext,
) -> Result<PhysicalExpr, String> {
    match expr {
        LogicalExpr::Scan { table } => {
            let table_stats = context
                .table_stats
                .get(table)
                .ok_or_else(|| format!("Table {} not found", table))?;
            let cost = context.cost_model.cost_table_scan(table_stats, table_stats.row_count);
            Ok(PhysicalExpr::SeqScan {
                table: table.clone(),
                cost,
            })
        }

        LogicalExpr::Filter { input, predicate } => {
            let physical_input = optimize_internal(input, context)?;
            Ok(PhysicalExpr::Filter {
                input: Box::new(physical_input),
                predicate: predicate.clone(),
                selectivity: 0.5, // Default 50% selectivity
            })
        }

        LogicalExpr::Project { input, columns } => {
            let physical_input = optimize_internal(input, context)?;
            Ok(PhysicalExpr::Project {
                input: Box::new(physical_input),
                columns: columns.clone(),
            })
        }

        LogicalExpr::Join {
            left,
            right,
            join_keys,
        } => {
            let physical_left = optimize_internal(left, context)?;
            let physical_right = optimize_internal(right, context)?;

            let left_rows = physical_left.estimated_rows(&context.table_stats);
            let right_rows = physical_right.estimated_rows(&context.table_stats);

            // Choose join strategy based on sizes
            let strategy = choose_join_strategy(left_rows, right_rows);

            let join_cost = match strategy {
                JoinStrategy::Hash => {
                    context
                        .cost_model
                        .cost_hash_join(left_rows, right_rows, join_keys.len())
                }
                JoinStrategy::NestedLoop => {
                    context.cost_model.cost_nested_loop_join(left_rows, right_rows)
                }
                JoinStrategy::SortMerge => {
                    context.cost_model.cost_merge_join(left_rows, right_rows)
                }
                _ => context.cost_model.cost_merge_join(left_rows, right_rows),
            };

            match strategy {
                JoinStrategy::Hash => Ok(PhysicalExpr::HashJoin {
                    left: Box::new(physical_left),
                    right: Box::new(physical_right),
                    join_keys: join_keys.clone(),
                    cost: join_cost,
                }),
                JoinStrategy::NestedLoop => Ok(PhysicalExpr::NestedLoopJoin {
                    left: Box::new(physical_left),
                    right: Box::new(physical_right),
                    join_keys: join_keys.clone(),
                    cost: join_cost,
                }),
                _ => Ok(PhysicalExpr::SortMergeJoin {
                    left: Box::new(physical_left),
                    right: Box::new(physical_right),
                    join_keys: join_keys.clone(),
                    cost: join_cost,
                }),
            }
        }

        LogicalExpr::GroupBy {
            input,
            group_keys,
            aggregates,
        } => {
            let physical_input = optimize_internal(input, context)?;
            let input_rows = physical_input.estimated_rows(&context.table_stats);
            let agg_cost = context.cost_model.cost_aggregate(input_rows, group_keys.len() as u64);

            Ok(PhysicalExpr::GroupBy {
                input: Box::new(physical_input),
                group_keys: group_keys.clone(),
                aggregates: aggregates.clone(),
                cost: agg_cost,
            })
        }

        LogicalExpr::Sort { input, order_keys } => {
            let physical_input = optimize_internal(input, context)?;
            let input_rows = physical_input.estimated_rows(&context.table_stats);
            let sort_cost = context.cost_model.cost_sort(input_rows);

            Ok(PhysicalExpr::Sort {
                input: Box::new(physical_input),
                order_keys: order_keys.clone(),
                cost: sort_cost,
            })
        }
    }
}

/// Join strategy selection heuristic
#[derive(Debug, Clone, Copy)]
enum JoinStrategy {
    Hash,
    NestedLoop,
    SortMerge,
}

fn choose_join_strategy(left_rows: u64, right_rows: u64) -> JoinStrategy {
    let smaller = left_rows.min(right_rows);
    let larger = left_rows.max(right_rows);

    // Very small tables: nested loop
    if larger < 1000 {
        return JoinStrategy::NestedLoop;
    }

    // Small inner table: hash join (fits in memory)
    if smaller < 1_000_000 {
        return JoinStrategy::Hash;
    }

    // Large tables: sort-merge
    JoinStrategy::SortMerge
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logical_expr_creation() {
        let scan = LogicalExpr::Scan {
            table: "users".to_string(),
        };
        assert_eq!(
            scan,
            LogicalExpr::Scan {
                table: "users".to_string()
            }
        );
    }

    #[test]
    fn test_physical_expr_cost() {
        let expr = PhysicalExpr::SeqScan {
            table: "users".to_string(),
            cost: 100.0,
        };
        assert_eq!(expr.total_cost(), 100.0);
    }

    #[test]
    fn test_optimizer_scan() {
        let logical = LogicalExpr::Scan {
            table: "users".to_string(),
        };
        let mut stats = std::collections::HashMap::new();
        stats.insert("users".to_string(), TableStats::new("users".to_string(), 1000));
        let context = OptimizerContext::new(stats);

        let result = optimize(logical, &context);
        assert!(result.is_ok());
        let physical = result.unwrap();
        assert!(matches!(physical, PhysicalExpr::SeqScan { .. }));
    }

    #[test]
    fn test_optimizer_filter() {
        let logical = LogicalExpr::Filter {
            input: Box::new(LogicalExpr::Scan {
                table: "users".to_string(),
            }),
            predicate: "age > 18".to_string(),
        };
        let mut stats = std::collections::HashMap::new();
        stats.insert("users".to_string(), TableStats::new("users".to_string(), 1000));
        let context = OptimizerContext::new(stats);

        let result = optimize(logical, &context);
        assert!(result.is_ok());
        let physical = result.unwrap();
        assert!(matches!(physical, PhysicalExpr::Filter { .. }));
    }

    #[test]
    fn test_optimizer_join_small_tables() {
        let logical = LogicalExpr::Join {
            left: Box::new(LogicalExpr::Scan {
                table: "t1".to_string(),
            }),
            right: Box::new(LogicalExpr::Scan {
                table: "t2".to_string(),
            }),
            join_keys: vec!["id".to_string()],
        };
        let mut stats = std::collections::HashMap::new();
        stats.insert("t1".to_string(), TableStats::new("t1".to_string(), 100));
        stats.insert("t2".to_string(), TableStats::new("t2".to_string(), 100));
        let context = OptimizerContext::new(stats);

        let result = optimize(logical, &context);
        assert!(result.is_ok());
        let physical = result.unwrap();
        assert!(matches!(
            physical,
            PhysicalExpr::NestedLoopJoin { .. }
        ));
    }

    #[test]
    fn test_optimizer_join_large_tables() {
        let logical = LogicalExpr::Join {
            left: Box::new(LogicalExpr::Scan {
                table: "t1".to_string(),
            }),
            right: Box::new(LogicalExpr::Scan {
                table: "t2".to_string(),
            }),
            join_keys: vec!["id".to_string()],
        };
        let mut stats = std::collections::HashMap::new();
        stats.insert("t1".to_string(), TableStats::new("t1".to_string(), 10_000_000));
        stats.insert("t2".to_string(), TableStats::new("t2".to_string(), 10_000_000));
        let context = OptimizerContext::new(stats);

        let result = optimize(logical, &context);
        assert!(result.is_ok());
        let physical = result.unwrap();
        // Should choose SortMerge for large tables
        assert!(matches!(
            physical,
            PhysicalExpr::SortMergeJoin { .. }
        ) || matches!(
            physical,
            PhysicalExpr::HashJoin { .. }
        ));
    }

    #[test]
    fn test_estimated_rows_scan() {
        let mut stats = std::collections::HashMap::new();
        stats.insert("users".to_string(), TableStats::new("users".to_string(), 1000));

        let scan = PhysicalExpr::SeqScan {
            table: "users".to_string(),
            cost: 1.0,
        };

        assert_eq!(scan.estimated_rows(&stats), 1000);
    }

    #[test]
    fn test_estimated_rows_filter() {
        let mut stats = std::collections::HashMap::new();
        stats.insert("users".to_string(), TableStats::new("users".to_string(), 1000));

        let scan = PhysicalExpr::SeqScan {
            table: "users".to_string(),
            cost: 1.0,
        };
        let filtered = PhysicalExpr::Filter {
            input: Box::new(scan),
            predicate: "age > 18".to_string(),
            selectivity: 0.75,
        };

        assert_eq!(filtered.estimated_rows(&stats), 750);
    }

    #[test]
    fn test_optimizer_sort() {
        let logical = LogicalExpr::Sort {
            input: Box::new(LogicalExpr::Scan {
                table: "users".to_string(),
            }),
            order_keys: vec!["name".to_string()],
        };
        let mut stats = std::collections::HashMap::new();
        stats.insert("users".to_string(), TableStats::new("users".to_string(), 1000));
        let context = OptimizerContext::new(stats);

        let result = optimize(logical, &context);
        assert!(result.is_ok());
        let physical = result.unwrap();
        assert!(matches!(physical, PhysicalExpr::Sort { .. }));
    }
}
