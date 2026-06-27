use kore_fileformat::parallel_execution::*;
use kore_fileformat::query_execution::QueryPlanner;
use kore_fileformat::predicates::QueryFilter;
use std::sync::Arc;

/// Test thread pool creation and configuration
#[test]
fn test_thread_pool_basic_creation() {
    let pool = ExecutionThreadPool::new(4);
    assert_eq!(pool.worker_count(), 4);
    assert_eq!(pool.queue_size(), 1000);
}

/// Test thread pool auto-detection
#[test]
fn test_thread_pool_auto_detect_cpus() {
    let pool = ExecutionThreadPool::new(0);
    assert!(pool.worker_count() > 0);
    assert!(pool.worker_count() <= 128); // Reasonable upper bound
}

/// Test thread pool configuration builder
#[test]
fn test_thread_pool_with_configuration() {
    let pool = ExecutionThreadPool::new(8)
        .with_queue_size(2000);
    assert_eq!(pool.worker_count(), 8);
    assert_eq!(pool.queue_size(), 2000);
}

/// Test work item creation
#[test]
fn test_work_item_basic_properties() {
    let item = WorkItem::new(0, 1000, 500);
    assert_eq!(item.partition_id, 0);
    assert_eq!(item.start_row, 1000);
    assert_eq!(item.row_count, 500);
    assert_eq!(item.end_row(), 1500);
}

/// Test multiple work items
#[test]
fn test_work_item_multiple_items() {
    let items: Vec<_> = (0..10)
        .map(|i| WorkItem::new(i, i as u64 * 100, 100))
        .collect();
    
    assert_eq!(items.len(), 10);
    for (i, item) in items.iter().enumerate() {
        assert_eq!(item.partition_id, i as u32);
        assert_eq!(item.start_row, (i as u64) * 100);
        assert_eq!(item.end_row(), ((i as u64) + 1) * 100);
    }
}

/// Test query partition creation
#[test]
fn test_query_partition_basic_properties() {
    let data = vec![1, 2, 3, 4, 5];
    let partition = QueryPartition::new(0, data.clone(), 100);
    
    assert_eq!(partition.id, 0);
    assert_eq!(partition.row_count, 100);
    assert_eq!(partition.size_bytes(), 5);
    assert_eq!(partition.data.len(), 5);
}

/// Test data partitioning by row count
#[test]
fn test_data_partitioner_by_rows() {
    let data = vec![0; 1000];
    let partitions = DataPartitioner::partition_by_rows(data, 100, 4);
    
    assert_eq!(partitions.len(), 4);
    
    let total_rows: u64 = partitions.iter().map(|p| p.row_count).sum();
    assert_eq!(total_rows, 100);
    
    let total_bytes: usize = partitions.iter().map(|p| p.size_bytes()).sum();
    assert_eq!(total_bytes, 1000);
}

/// Test data partitioning by byte size
#[test]
fn test_data_partitioner_by_size() {
    let data = vec![0; 4000];
    let partitions = DataPartitioner::partition_by_size(data, 1000, 1000);
    
    assert_eq!(partitions.len(), 4);
    
    for partition in &partitions {
        assert!(partition.size_bytes() <= 1000);
    }
}

/// Test data partitioning with uneven division
#[test]
fn test_data_partitioner_uneven_division() {
    let data = vec![0; 1000];
    let partitions = DataPartitioner::partition_by_rows(data, 100, 3);
    
    assert_eq!(partitions.len(), 3);
    
    // Last partition should have fewer rows
    let last_partition = &partitions[partitions.len() - 1];
    assert!(last_partition.row_count <= 34);
}

/// Test partition result creation
#[test]
fn test_partition_result_basic_creation() {
    let result = PartitionResult::new(0, 1000, 500, 10);
    
    assert_eq!(result.partition_id, 0);
    assert_eq!(result.rows_processed, 1000);
    assert_eq!(result.matches, 500);
    assert_eq!(result.execution_time_ms, 10);
}

/// Test result aggregator creation
#[test]
fn test_result_aggregator_initialization() {
    let agg = ResultAggregator::new();
    
    assert_eq!(agg.results().len(), 0);
    assert_eq!(agg.total_rows_processed(), 0);
    assert_eq!(agg.total_matches(), 0);
    assert_eq!(agg.total_time_ms(), 0);
}

/// Test result aggregation with multiple partitions
#[test]
fn test_result_aggregator_multiple_results() {
    let mut agg = ResultAggregator::new();
    
    agg.add_result(PartitionResult::new(0, 1000, 500, 10));
    agg.add_result(PartitionResult::new(1, 1000, 400, 12));
    agg.add_result(PartitionResult::new(2, 1000, 600, 8));
    
    assert_eq!(agg.results().len(), 3);
    assert_eq!(agg.total_rows_processed(), 3000);
    assert_eq!(agg.total_matches(), 1500);
    assert_eq!(agg.total_time_ms(), 30);
}

/// Test result aggregator average time calculation
#[test]
fn test_result_aggregator_average_time() {
    let mut agg = ResultAggregator::new();
    
    agg.add_result(PartitionResult::new(0, 1000, 500, 10));
    agg.add_result(PartitionResult::new(1, 1000, 500, 20));
    agg.add_result(PartitionResult::new(2, 1000, 500, 30));
    
    let avg = agg.avg_time_ms();
    assert!((avg - 20.0).abs() < 0.1);
}

/// Test result aggregator speedup calculation
#[test]
fn test_result_aggregator_speedup_factor() {
    let mut agg = ResultAggregator::new();
    
    // Simulate 4 partitions running in parallel, each taking 25ms
    // Sequential would be 100ms, parallel is 25ms → 4x speedup
    agg.add_result(PartitionResult::new(0, 1000, 500, 25));
    agg.add_result(PartitionResult::new(1, 1000, 500, 25));
    agg.add_result(PartitionResult::new(2, 1000, 500, 25));
    agg.add_result(PartitionResult::new(3, 1000, 500, 25));
    
    let speedup = agg.speedup_factor(100);
    assert!((speedup - 4.0).abs() < 0.1);
}

/// Test parallel executor creation
#[test]
fn test_parallel_executor_creation() {
    let planner = Arc::new(QueryPlanner::new());
    let executor = ParallelQueryExecutor::new(planner.clone());
    
    assert!(executor.thread_pool().worker_count() > 0);
}

/// Test parallel executor with custom thread pool
#[test]
fn test_parallel_executor_custom_thread_pool() {
    let planner = Arc::new(QueryPlanner::new());
    let pool = ExecutionThreadPool::new(2);
    let executor = ParallelQueryExecutor::new(planner)
        .with_thread_pool(pool);
    
    assert_eq!(executor.thread_pool().worker_count(), 2);
}

/// Test partition distribution across workers
#[test]
fn test_partition_distribution() {
    let data = vec![0; 10000];
    let partitions = DataPartitioner::partition_by_rows(data, 10000, 8);
    
    assert_eq!(partitions.len(), 8);
    
    // Each partition should have roughly equal rows
    let avg_rows = 10000 / 8;
    for partition in &partitions {
        assert!(partition.row_count > 0);
        assert!(partition.row_count <= avg_rows + 10);
    }
}

/// Test execution speedup scaling
#[test]
fn test_execution_speedup_scaling() {
    // Simulate increasing partition counts
    for worker_count in vec![1, 2, 4, 8] {
        let mut agg = ResultAggregator::new();
        let time_per_partition = 100;
        
        for i in 0..worker_count {
            agg.add_result(PartitionResult::new(
                i as u32,
                1000,
                500,
                time_per_partition,
            ));
        }
        
        let sequential_time = time_per_partition * worker_count as u64;
        let speedup = agg.speedup_factor(sequential_time);
        
        // With perfect parallelism, speedup ~= 1.0 (parallel time = sequential / workers)
        assert!(speedup > 0.5);
    }
}

/// Test data partitioner with large data
#[test]
fn test_data_partitioner_large_dataset() {
    let data = vec![0; 1_000_000];
    let partitions = DataPartitioner::partition_by_rows(data, 1_000_000, 16);
    
    assert!(partitions.len() > 0);
    
    let total_size: usize = partitions.iter().map(|p| p.size_bytes()).sum();
    assert_eq!(total_size, 1_000_000);
}

/// Test result aggregator with many partitions
#[test]
fn test_result_aggregator_many_partitions() {
    let mut agg = ResultAggregator::new();
    
    for i in 0..100 {
        agg.add_result(PartitionResult::new(i, 100, 50, 5));
    }
    
    assert_eq!(agg.results().len(), 100);
    assert_eq!(agg.total_rows_processed(), 10_000);
    assert_eq!(agg.total_matches(), 5_000);
}

/// Test parallel executor with empty partitions
#[test]
fn test_parallel_executor_empty_partitions() {
    let planner = Arc::new(QueryPlanner::new());
    let executor = ParallelQueryExecutor::new(planner);
    let filter = QueryFilter::default();
    
    let agg = executor.execute_parallel(Vec::new(), &filter, 0);
    
    assert_eq!(agg.total_rows_processed(), 0);
    assert_eq!(agg.results().len(), 0);
}

/// Test partition cloning
#[test]
fn test_partition_cloning() {
    let data = vec![1, 2, 3, 4, 5];
    let partition1 = QueryPartition::new(0, data.clone(), 100);
    let partition2 = partition1.clone();
    
    assert_eq!(partition1.id, partition2.id);
    assert_eq!(partition1.row_count, partition2.row_count);
    assert_eq!(partition1.data, partition2.data);
}

/// Test work item cloning
#[test]
fn test_work_item_cloning() {
    let item1 = WorkItem::new(0, 1000, 500);
    let item2 = item1.clone();
    
    assert_eq!(item1.partition_id, item2.partition_id);
    assert_eq!(item1.start_row, item2.start_row);
    assert_eq!(item1.row_count, item2.row_count);
}

/// Test result aggregator default
#[test]
fn test_result_aggregator_default() {
    let agg = ResultAggregator::default();
    assert_eq!(agg.total_rows_processed(), 0);
}

/// Test thread pool default
#[test]
fn test_thread_pool_default() {
    let pool = ExecutionThreadPool::default();
    assert!(pool.worker_count() > 0);
}

/// Test partition result cloning
#[test]
fn test_partition_result_cloning() {
    let result1 = PartitionResult::new(0, 1000, 500, 10);
    let result2 = result1.clone();
    
    assert_eq!(result1.partition_id, result2.partition_id);
    assert_eq!(result1.rows_processed, result2.rows_processed);
    assert_eq!(result1.matches, result2.matches);
}

/// Test selectivity from partition results
#[test]
fn test_partition_selectivity_calculation() {
    let result = PartitionResult::new(0, 2000, 1000, 10);
    let selectivity = (result.matches as f64) / (result.rows_processed as f64);
    
    assert!((selectivity - 0.5).abs() < 0.01);
}

/// Test combined partition statistics
#[test]
fn test_combined_partition_statistics() {
    let mut agg = ResultAggregator::new();
    
    agg.add_result(PartitionResult::new(0, 2000, 1000, 10));
    agg.add_result(PartitionResult::new(1, 2000, 800, 12));
    agg.add_result(PartitionResult::new(2, 2000, 1200, 8));
    
    let total_matches = agg.total_matches();
    let total_rows = agg.total_rows_processed();
    let combined_selectivity = (total_matches as f64) / (total_rows as f64);
    
    assert!((combined_selectivity - 0.5).abs() < 0.01);
}
