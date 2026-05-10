/// JOIN algorithm optimization and strategy selection
///
/// Provides multiple JOIN implementations (hash join, nested loop, sort merge)
/// with cost-based strategy selection based on table sizes and cardinality.

use std::collections::HashMap;

/// JOIN algorithm types
#[derive(Clone, Debug, PartialEq)]
pub enum JoinAlgorithm {
    NestedLoop,
    HashJoin,
    SortMerge,
    IndexNested,
}

impl JoinAlgorithm {
    pub fn name(&self) -> &str {
        match self {
            JoinAlgorithm::NestedLoop => "NestedLoop",
            JoinAlgorithm::HashJoin => "HashJoin",
            JoinAlgorithm::SortMerge => "SortMerge",
            JoinAlgorithm::IndexNested => "IndexNested",
        }
    }
}

/// Cost model for a specific JOIN algorithm
#[derive(Clone, Debug, PartialEq)]
pub struct JoinCostModel {
    pub algorithm: JoinAlgorithm,
    pub cpu_cost: f64,
    pub memory_cost: f64,
    pub io_cost: f64,
    pub total_cost: f64,
}

impl JoinCostModel {
    pub fn new(
        algorithm: JoinAlgorithm,
        cpu: f64,
        mem: f64,
        io: f64,
    ) -> Self {
        Self {
            algorithm,
            cpu_cost: cpu,
            memory_cost: mem,
            io_cost: io,
            total_cost: cpu + mem + io,
        }
    }
}

/// Statistics for a table
#[derive(Clone, Debug)]
pub struct TableStats {
    pub table_name: String,
    pub row_count: usize,
    pub column_count: usize,
    pub avg_row_size: usize,
    pub has_index: bool,
    pub is_sorted: bool,
}

impl TableStats {
    pub fn new(
        name: &str,
        rows: usize,
        cols: usize,
        row_size: usize,
    ) -> Self {
        Self {
            table_name: name.to_string(),
            row_count: rows,
            column_count: cols,
            avg_row_size: row_size,
            has_index: false,
            is_sorted: false,
        }
    }

    pub fn with_index(mut self, has_idx: bool) -> Self {
        self.has_index = has_idx;
        self
    }

    pub fn with_sorted(mut self, sorted: bool) -> Self {
        self.is_sorted = sorted;
        self
    }

    pub fn total_size_bytes(&self) -> usize {
        self.row_count * self.avg_row_size
    }
}

/// JOIN optimizer for strategy selection
pub struct JoinOptimizer {
    tables: HashMap<String, TableStats>,
}

impl JoinOptimizer {
    pub fn new() -> Self {
        Self {
            tables: HashMap::new(),
        }
    }

    pub fn register_table(&mut self, stats: TableStats) {
        self.tables.insert(stats.table_name.clone(), stats);
    }

    pub fn get_table(&self, name: &str) -> Option<&TableStats> {
        self.tables.get(name)
    }

    /// Select optimal JOIN algorithm based on table statistics
    pub fn select_algorithm(
        &self,
        left_table: &str,
        right_table: &str,
        selectivity: f64,
    ) -> JoinAlgorithm {
        let left = match self.get_table(left_table) {
            Some(t) => t,
            None => return JoinAlgorithm::NestedLoop,
        };

        let right = match self.get_table(right_table) {
            Some(t) => t,
            None => return JoinAlgorithm::NestedLoop,
        };

        // Very small tables → nested loop
        if left.row_count < 1000 && right.row_count < 1000 {
            return JoinAlgorithm::NestedLoop;
        }

        // If one table has index, use index nested loop
        if (left.has_index || right.has_index) && selectivity < 0.5 {
            return JoinAlgorithm::IndexNested;
        }

        // Both sorted → sort merge
        if left.is_sorted && right.is_sorted {
            return JoinAlgorithm::SortMerge;
        }

        // Large tables → hash join
        if left.row_count > 10000 || right.row_count > 10000 {
            return JoinAlgorithm::HashJoin;
        }

        JoinAlgorithm::NestedLoop
    }

    /// Calculate estimated cost for nested loop JOIN
    pub fn cost_nested_loop(
        &self,
        left_table: &str,
        right_table: &str,
    ) -> JoinCostModel {
        let left = self.get_table(left_table).unwrap();
        let right = self.get_table(right_table).unwrap();

        let cpu_cost =
            (left.row_count * right.row_count) as f64 / 1000.0;
        let memory_cost = 10.0;
        let io_cost = 5.0;

        JoinCostModel::new(
            JoinAlgorithm::NestedLoop,
            cpu_cost,
            memory_cost,
            io_cost,
        )
    }

    /// Calculate estimated cost for hash JOIN
    pub fn cost_hash_join(
        &self,
        left_table: &str,
        right_table: &str,
    ) -> JoinCostModel {
        let left = self.get_table(left_table).unwrap();
        let right = self.get_table(right_table).unwrap();

        let cpu_cost =
            (left.row_count + right.row_count) as f64 / 100.0;
        let memory_cost = ((left.row_count + right.row_count)
            * (left.avg_row_size + right.avg_row_size)) as f64
            / 1000000.0;
        let io_cost = 3.0;

        JoinCostModel::new(
            JoinAlgorithm::HashJoin,
            cpu_cost,
            memory_cost,
            io_cost,
        )
    }

    /// Calculate estimated cost for sort merge JOIN
    pub fn cost_sort_merge(
        &self,
        left_table: &str,
        right_table: &str,
    ) -> JoinCostModel {
        let left = self.get_table(left_table).unwrap();
        let right = self.get_table(right_table).unwrap();

        let cpu_cost = ((left.row_count as f64
            * (left.row_count as f64).log2())
            + (right.row_count as f64
                * (right.row_count as f64).log2()))
            / 1000.0;
        let memory_cost = 20.0;
        let io_cost = 4.0;

        JoinCostModel::new(
            JoinAlgorithm::SortMerge,
            cpu_cost,
            memory_cost,
            io_cost,
        )
    }

    /// Calculate estimated cost for index nested loop
    pub fn cost_index_nested(
        &self,
        left_table: &str,
        right_table: &str,
        selectivity: f64,
    ) -> JoinCostModel {
        let left = self.get_table(left_table).unwrap();
        let right = self.get_table(right_table).unwrap();

        let estimated_matches =
            (left.row_count as f64) * selectivity;
        let cpu_cost = estimated_matches / 10.0;
        let memory_cost = 15.0;
        let io_cost = 2.0;

        JoinCostModel::new(
            JoinAlgorithm::IndexNested,
            cpu_cost,
            memory_cost,
            io_cost,
        )
    }

    /// Compare all algorithms and return ranked list
    pub fn compare_algorithms(
        &self,
        left_table: &str,
        right_table: &str,
        selectivity: f64,
    ) -> Vec<JoinCostModel> {
        let mut costs = vec![
            self.cost_nested_loop(left_table, right_table),
            self.cost_hash_join(left_table, right_table),
            self.cost_sort_merge(left_table, right_table),
            self.cost_index_nested(
                left_table,
                right_table,
                selectivity,
            ),
        ];

        costs.sort_by(|a, b| {
            a.total_cost.partial_cmp(&b.total_cost).unwrap()
        });

        costs
    }
}

impl Default for JoinOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Hash JOIN implementation
pub struct HashJoinExecutor {
    build_table_rows: usize,
    probe_table_rows: usize,
    hash_table_size: usize,
}

impl HashJoinExecutor {
    pub fn new(build_rows: usize, probe_rows: usize) -> Self {
        Self {
            build_table_rows: build_rows,
            probe_table_rows: probe_rows,
            hash_table_size: Self::estimate_hash_table_size(
                build_rows,
            ),
        }
    }

    fn estimate_hash_table_size(rows: usize) -> usize {
        // Hash table size ≈ 1.3x rows (load factor ~0.77)
        ((rows as f64) * 1.3) as usize
    }

    pub fn memory_required_mb(&self) -> f64 {
        (self.hash_table_size * 64) as f64 / 1000000.0
    }

    pub fn estimated_output_rows(
        &self,
        selectivity: f64,
    ) -> usize {
        ((self.probe_table_rows as f64) * selectivity).ceil() as usize
    }
}

/// Nested Loop JOIN implementation
pub struct NestedLoopExecutor {
    outer_rows: usize,
    inner_rows: usize,
}

impl NestedLoopExecutor {
    pub fn new(outer: usize, inner: usize) -> Self {
        Self {
            outer_rows: outer,
            inner_rows: inner,
        }
    }

    pub fn comparisons_required(&self) -> usize {
        self.outer_rows * self.inner_rows
    }

    pub fn estimated_output_rows(
        &self,
        selectivity: f64,
    ) -> usize {
        ((self.comparisons_required() as f64) * selectivity)
            .ceil() as usize
    }

    pub fn memory_required_mb(&self) -> f64 {
        // Minimal memory requirement
        1.0
    }
}

/// Sort Merge JOIN implementation
pub struct SortMergeExecutor {
    left_rows: usize,
    right_rows: usize,
}

impl SortMergeExecutor {
    pub fn new(left: usize, right: usize) -> Self {
        Self {
            left_rows: left,
            right_rows: right,
        }
    }

    pub fn sort_cost(&self) -> f64 {
        let left_sort = (self.left_rows as f64)
            * (self.left_rows as f64).log2();
        let right_sort = (self.right_rows as f64)
            * (self.right_rows as f64).log2();
        (left_sort + right_sort) / 1000.0
    }

    pub fn merge_cost(&self) -> f64 {
        ((self.left_rows + self.right_rows) as f64) / 100.0
    }

    pub fn total_cost(&self) -> f64 {
        self.sort_cost() + self.merge_cost()
    }

    pub fn memory_required_mb(&self) -> f64 {
        // Needs space for sorting
        ((self.left_rows + self.right_rows) * 100) as f64
            / 1000000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_algorithm_names() {
        assert_eq!(JoinAlgorithm::NestedLoop.name(), "NestedLoop");
        assert_eq!(JoinAlgorithm::HashJoin.name(), "HashJoin");
        assert_eq!(JoinAlgorithm::SortMerge.name(), "SortMerge");
        assert_eq!(JoinAlgorithm::IndexNested.name(), "IndexNested");
    }

    #[test]
    fn test_join_cost_model() {
        let cost =
            JoinCostModel::new(JoinAlgorithm::HashJoin, 10.0, 5.0, 2.0);

        assert_eq!(cost.cpu_cost, 10.0);
        assert_eq!(cost.memory_cost, 5.0);
        assert_eq!(cost.io_cost, 2.0);
        assert_eq!(cost.total_cost, 17.0);
    }

    #[test]
    fn test_table_stats() {
        let stats = TableStats::new("users", 10000, 5, 100);

        assert_eq!(stats.row_count, 10000);
        assert_eq!(stats.column_count, 5);
        assert_eq!(stats.total_size_bytes(), 1000000);
    }

    #[test]
    fn test_table_stats_with_index() {
        let stats = TableStats::new("users", 10000, 5, 100)
            .with_index(true)
            .with_sorted(true);

        assert!(stats.has_index);
        assert!(stats.is_sorted);
    }

    #[test]
    fn test_join_optimizer_register_table() {
        let mut optimizer = JoinOptimizer::new();
        let stats = TableStats::new("users", 10000, 5, 100);

        optimizer.register_table(stats);
        assert!(optimizer.get_table("users").is_some());
    }

    #[test]
    fn test_join_optimizer_select_small_tables() {
        let mut optimizer = JoinOptimizer::new();
        optimizer.register_table(TableStats::new("t1", 100, 2, 50));
        optimizer.register_table(TableStats::new("t2", 100, 2, 50));

        let algo = optimizer.select_algorithm("t1", "t2", 0.5);
        assert_eq!(algo, JoinAlgorithm::NestedLoop);
    }

    #[test]
    fn test_join_optimizer_select_large_tables() {
        let mut optimizer = JoinOptimizer::new();
        optimizer.register_table(TableStats::new("t1", 50000, 5, 100));
        optimizer.register_table(TableStats::new("t2", 50000, 5, 100));

        let algo = optimizer.select_algorithm("t1", "t2", 0.5);
        assert_eq!(algo, JoinAlgorithm::HashJoin);
    }

    #[test]
    fn test_cost_nested_loop() {
        let mut optimizer = JoinOptimizer::new();
        optimizer.register_table(TableStats::new("t1", 1000, 2, 50));
        optimizer.register_table(TableStats::new("t2", 1000, 2, 50));

        let cost = optimizer.cost_nested_loop("t1", "t2");
        assert!(cost.total_cost > 0.0);
    }

    #[test]
    fn test_cost_hash_join() {
        let mut optimizer = JoinOptimizer::new();
        optimizer.register_table(TableStats::new("t1", 10000, 5, 100));
        optimizer.register_table(TableStats::new("t2", 10000, 5, 100));

        let cost = optimizer.cost_hash_join("t1", "t2");
        assert!(cost.total_cost > 0.0);
        assert!(cost.memory_cost > 0.0);
    }

    #[test]
    fn test_cost_comparison() {
        let mut optimizer = JoinOptimizer::new();
        optimizer.register_table(TableStats::new("t1", 10000, 5, 100));
        optimizer.register_table(TableStats::new("t2", 10000, 5, 100));

        let costs = optimizer.compare_algorithms("t1", "t2", 0.1);

        assert_eq!(costs.len(), 4);
        // First in list should have lowest total cost
        for i in 1..costs.len() {
            assert!(costs[0].total_cost <= costs[i].total_cost);
        }
    }

    #[test]
    fn test_hash_join_executor() {
        let executor = HashJoinExecutor::new(10000, 10000);

        assert!(executor.memory_required_mb() > 0.0);
        assert_eq!(executor.estimated_output_rows(0.5), 5000);
    }

    #[test]
    fn test_nested_loop_executor() {
        let executor = NestedLoopExecutor::new(1000, 1000);

        assert_eq!(executor.comparisons_required(), 1000000);
        assert_eq!(executor.estimated_output_rows(0.01), 10000);
    }

    #[test]
    fn test_sort_merge_executor() {
        let executor = SortMergeExecutor::new(5000, 5000);

        assert!(executor.sort_cost() > 0.0);
        assert!(executor.merge_cost() > 0.0);
        assert!(executor.total_cost() > 0.0);
        assert!(executor.memory_required_mb() > 0.0);
    }

    #[test]
    fn test_join_optimizer_with_index() {
        let mut optimizer = JoinOptimizer::new();
        optimizer.register_table(
            TableStats::new("t1", 100000, 5, 100)
                .with_index(true),
        );
        optimizer.register_table(TableStats::new("t2", 100000, 5, 100));

        let algo = optimizer.select_algorithm("t1", "t2", 0.2);
        assert_eq!(algo, JoinAlgorithm::IndexNested);
    }

    #[test]
    fn test_join_optimizer_sorted_tables() {
        let mut optimizer = JoinOptimizer::new();
        optimizer.register_table(
            TableStats::new("t1", 50000, 5, 100)
                .with_sorted(true),
        );
        optimizer.register_table(
            TableStats::new("t2", 50000, 5, 100)
                .with_sorted(true),
        );

        let algo = optimizer.select_algorithm("t1", "t2", 0.5);
        assert_eq!(algo, JoinAlgorithm::SortMerge);
    }
}
