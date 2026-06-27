//! Phase 3.2 Integration Tests - Advanced Statistics & Metadata
//!
//! Tests the column statistics, histograms, and block-level metadata
//! for advanced query optimization.

#[cfg(test)]
mod phase_3_2_integration_tests {
    use kore_fileformat::statistics::*;

    // ============================================================================
    // SECTION 1: Column Statistics Tests (8 tests)
    // ============================================================================

    #[test]
    fn test_column_statistics_creation() {
        let stats = ColumnStatistics::new(
            "user_id".to_string(),
            "Int64".to_string(),
            50000,
            0,
            50000,
        );

        assert_eq!(stats.name, "user_id");
        assert_eq!(stats.data_type, "Int64");
        assert_eq!(stats.row_count, 50000);
        assert_eq!(stats.null_count, 0);
        assert_eq!(stats.distinct_count, 50000);
    }

    #[test]
    fn test_column_statistics_cardinality_high() {
        let stats = ColumnStatistics::new(
            "id".to_string(),
            "Int64".to_string(),
            10000,
            0,
            10000,
        );
        assert_eq!(stats.cardinality(), 1.0); // 100% distinct
    }

    #[test]
    fn test_column_statistics_cardinality_low() {
        let stats = ColumnStatistics::new(
            "status".to_string(),
            "String".to_string(),
            10000,
            0,
            5,
        );
        assert_eq!(stats.cardinality(), 0.0005); // 0.05% distinct
    }

    #[test]
    fn test_column_statistics_null_percentage() {
        let stats = ColumnStatistics::new(
            "optional_field".to_string(),
            "String".to_string(),
            10000,
            2500,
            7500,
        );
        assert_eq!(stats.null_percent(), 25.0);
    }

    #[test]
    fn test_column_statistics_with_range() {
        let stats = ColumnStatistics::new("age".to_string(), "Int64".to_string(), 1000, 0, 100)
            .with_range(Some("18".to_string()), Some("99".to_string()));

        assert_eq!(stats.min_value, Some("18".to_string()));
        assert_eq!(stats.max_value, Some("99".to_string()));
    }

    #[test]
    fn test_column_statistics_with_avg_length() {
        let stats = ColumnStatistics::new("name".to_string(), "String".to_string(), 1000, 0, 950)
            .with_avg_length(25.5);

        assert_eq!(stats.avg_length, 25.5);
    }

    #[test]
    fn test_column_statistics_estimated_size_int64() {
        let stats = ColumnStatistics::new("id".to_string(), "Int64".to_string(), 100000, 0, 100000);
        assert_eq!(stats.estimated_size_bytes(), 800000); // 100000 * 8
    }

    #[test]
    fn test_column_statistics_estimated_size_string() {
        let stats = ColumnStatistics::new("description".to_string(), "String".to_string(), 1000, 0, 900)
            .with_avg_length(100.0);

        let size = stats.estimated_size_bytes();
        assert!(size > 90000 && size < 110000); // Approximately 100000
    }

    // ============================================================================
    // SECTION 2: Histogram Tests (10 tests)
    // ============================================================================

    #[test]
    fn test_histogram_creation() {
        let hist = Histogram::new("age".to_string());
        assert_eq!(hist.column_name, "age");
        assert!(hist.buckets.is_empty());
        assert_eq!(hist.total_count, 0);
    }

    #[test]
    fn test_histogram_add_single_bucket() {
        let mut hist = Histogram::new("score".to_string());
        let bucket = HistogramBucket::new("0".to_string(), "50".to_string(), 100);
        hist.add_bucket(bucket);

        assert_eq!(hist.buckets.len(), 1);
        assert_eq!(hist.total_count, 100);
    }

    #[test]
    fn test_histogram_add_multiple_buckets() {
        let mut hist = Histogram::new("grade".to_string());
        hist.add_bucket(HistogramBucket::new("A".to_string(), "B".to_string(), 500));
        hist.add_bucket(HistogramBucket::new("C".to_string(), "D".to_string(), 300));
        hist.add_bucket(HistogramBucket::new("E".to_string(), "F".to_string(), 200));

        assert_eq!(hist.buckets.len(), 3);
        assert_eq!(hist.total_count, 1000);
    }

    #[test]
    fn test_histogram_calculate_frequencies() {
        let mut hist = Histogram::new("status".to_string());
        hist.add_bucket(HistogramBucket::new("active".to_string(), "active".to_string(), 750));
        hist.add_bucket(HistogramBucket::new("inactive".to_string(), "inactive".to_string(), 250));

        hist.calculate_frequencies();

        assert!((hist.buckets[0].frequency - 75.0).abs() < 0.1);
        assert!((hist.buckets[1].frequency - 25.0).abs() < 0.1);
    }

    #[test]
    fn test_histogram_find_bucket_exact() {
        let mut hist = Histogram::new("age".to_string());
        hist.add_bucket(HistogramBucket::new("0".to_string(), "20".to_string(), 500));
        hist.add_bucket(HistogramBucket::new("21".to_string(), "40".to_string(), 600));
        hist.add_bucket(HistogramBucket::new("41".to_string(), "60".to_string(), 400));

        let bucket = hist.find_bucket("30");
        assert!(bucket.is_some());
        assert_eq!(bucket.unwrap().count, 600);
    }

    #[test]
    fn test_histogram_find_bucket_boundary() {
        let mut hist = Histogram::new("score".to_string());
        hist.add_bucket(HistogramBucket::new("0".to_string(), "100".to_string(), 1000));

        let bucket = hist.find_bucket("0");
        assert!(bucket.is_some());

        let bucket = hist.find_bucket("100");
        assert!(bucket.is_some());
    }

    #[test]
    fn test_histogram_estimate_selectivity_full_range() {
        let mut hist = Histogram::new("age".to_string());
        hist.add_bucket(HistogramBucket::new("000000".to_string(), "000020".to_string(), 200));
        hist.add_bucket(HistogramBucket::new("000021".to_string(), "000040".to_string(), 300));
        hist.add_bucket(HistogramBucket::new("000041".to_string(), "000060".to_string(), 400));
        hist.add_bucket(HistogramBucket::new("000061".to_string(), "000100".to_string(), 100));

        let selectivity = hist.estimate_selectivity("000000", "000100");
        assert!((selectivity - 1.0).abs() < 0.01); // Should select everything
    }

    #[test]
    fn test_histogram_estimate_selectivity_partial() {
        let mut hist = Histogram::new("age".to_string());
        hist.add_bucket(HistogramBucket::new("000000".to_string(), "000030".to_string(), 300));
        hist.add_bucket(HistogramBucket::new("000031".to_string(), "000060".to_string(), 400));
        hist.add_bucket(HistogramBucket::new("000061".to_string(), "000100".to_string(), 300));

        let selectivity = hist.estimate_selectivity("000030", "000060");
        assert!(selectivity > 0.2 && selectivity < 0.7);
    }

    #[test]
    fn test_histogram_with_bucket_frequencies() {
        let mut hist = Histogram::new("category".to_string());
        let bucket1 = HistogramBucket::new("A".to_string(), "A".to_string(), 400)
            .with_frequency(40.0);
        let bucket2 = HistogramBucket::new("B".to_string(), "B".to_string(), 300)
            .with_frequency(30.0);
        let bucket3 = HistogramBucket::new("C".to_string(), "C".to_string(), 300)
            .with_frequency(30.0);

        hist.add_bucket(bucket1);
        hist.add_bucket(bucket2);
        hist.add_bucket(bucket3);

        assert_eq!(hist.buckets[0].frequency, 40.0);
        assert_eq!(hist.buckets[1].frequency, 30.0);
        assert_eq!(hist.buckets[2].frequency, 30.0);
    }

    // ============================================================================
    // SECTION 3: Block Metadata Tests (8 tests)
    // ============================================================================

    #[test]
    fn test_block_metadata_creation() {
        let block = BlockMetadata::new(0, "user_data".to_string(), 10000, 80000);
        assert_eq!(block.block_id, 0);
        assert_eq!(block.column_name, "user_data");
        assert_eq!(block.row_count, 10000);
        assert_eq!(block.size_bytes, 80000);
    }

    #[test]
    fn test_block_metadata_with_range() {
        let block = BlockMetadata::new(1, "age".to_string(), 5000, 40000)
            .with_range(Some("20".to_string()), Some("50".to_string()));

        assert_eq!(block.min_value, Some("20".to_string()));
        assert_eq!(block.max_value, Some("50".to_string()));
    }

    #[test]
    fn test_block_metadata_with_null_count() {
        let block = BlockMetadata::new(0, "optional".to_string(), 10000, 8000)
            .with_null_count(1000);

        assert_eq!(block.null_count, 1000);
    }

    #[test]
    fn test_block_metadata_with_compressed_size() {
        let block = BlockMetadata::new(0, "data".to_string(), 10000, 80000)
            .with_compressed_size(20000);

        assert_eq!(block.compressed_size_bytes, 20000);
    }

    #[test]
    fn test_block_metadata_can_skip_range_complete_overlap() {
        let block = BlockMetadata::new(0, "age".to_string(), 5000, 40000)
            .with_range(Some("20".to_string()), Some("50".to_string()));

        // No overlap - block is entirely outside query range
        assert!(block.can_skip_for_range("0", "10"));
        assert!(block.can_skip_for_range("60", "100"));
    }

    #[test]
    fn test_block_metadata_can_skip_range_partial_overlap() {
        let block = BlockMetadata::new(0, "value".to_string(), 5000, 40000)
            .with_range(Some("000030".to_string()), Some("000070".to_string()));

        // Partial overlap - can't skip
        assert!(!block.can_skip_for_range("000000", "000050"));
        assert!(!block.can_skip_for_range("000050", "000100"));
    }

    #[test]
    fn test_block_metadata_compression_ratio() {
        let block = BlockMetadata::new(0, "compressed".to_string(), 10000, 100000)
            .with_compressed_size(25000);

        assert_eq!(block.compression_ratio(), 0.25);
    }

    #[test]
    fn test_block_metadata_no_null_count() {
        let block = BlockMetadata::new(0, "id".to_string(), 10000, 80000);
        assert_eq!(block.null_count, 0);
    }

    // ============================================================================
    // SECTION 4: Table Statistics Tests (10 tests)
    // ============================================================================

    #[test]
    fn test_table_statistics_creation() {
        let table = TableStatistics::new("customers".to_string(), 100000);
        assert_eq!(table.table_name, "customers");
        assert_eq!(table.row_count, 100000);
        assert!(table.column_stats.is_empty());
        assert!(table.histograms.is_empty());
        assert!(table.blocks.is_empty());
    }

    #[test]
    fn test_table_statistics_add_single_column() {
        let mut table = TableStatistics::new("orders".to_string(), 50000);
        let col_stats = ColumnStatistics::new("order_id".to_string(), "Int64".to_string(), 50000, 0, 50000);
        table.add_column_stats(col_stats);

        assert_eq!(table.column_stats.len(), 1);
        assert!(table.column_stats.contains_key("order_id"));
    }

    #[test]
    fn test_table_statistics_add_multiple_columns() {
        let mut table = TableStatistics::new("products".to_string(), 10000);
        
        table.add_column_stats(ColumnStatistics::new("id".to_string(), "Int64".to_string(), 10000, 0, 10000));
        table.add_column_stats(ColumnStatistics::new("name".to_string(), "String".to_string(), 10000, 100, 9500));
        table.add_column_stats(ColumnStatistics::new("price".to_string(), "Float64".to_string(), 10000, 0, 8000));

        assert_eq!(table.column_stats.len(), 3);
    }

    #[test]
    fn test_table_statistics_add_histogram() {
        let mut table = TableStatistics::new("sales".to_string(), 100000);
        let mut hist = Histogram::new("amount".to_string());
        hist.add_bucket(HistogramBucket::new("0".to_string(), "100".to_string(), 30000));
        hist.add_bucket(HistogramBucket::new("101".to_string(), "500".to_string(), 50000));
        hist.add_bucket(HistogramBucket::new("501".to_string(), "1000".to_string(), 20000));

        table.add_histogram(hist);

        assert_eq!(table.histograms.len(), 1);
        assert!(table.histograms.contains_key("amount"));
    }

    #[test]
    fn test_table_statistics_add_blocks() {
        let mut table = TableStatistics::new("data".to_string(), 100000);
        
        table.add_block(BlockMetadata::new(0, "col1".to_string(), 10000, 80000));
        table.add_block(BlockMetadata::new(1, "col1".to_string(), 10000, 80000));
        table.add_block(BlockMetadata::new(2, "col1".to_string(), 10000, 80000));

        assert_eq!(table.blocks.len(), 3);
    }

    #[test]
    fn test_table_statistics_get_skippable_blocks() {
        let mut table = TableStatistics::new("events".to_string(), 100000);
        
        let block1 = BlockMetadata::new(0, "timestamp".to_string(), 10000, 80000)
            .with_range(Some("2024-01-01".to_string()), Some("2024-01-31".to_string()));
        let block2 = BlockMetadata::new(1, "timestamp".to_string(), 10000, 80000)
            .with_range(Some("2024-02-01".to_string()), Some("2024-02-29".to_string()));
        let block3 = BlockMetadata::new(2, "timestamp".to_string(), 10000, 80000)
            .with_range(Some("2024-03-01".to_string()), Some("2024-03-31".to_string()));

        table.add_block(block1);
        table.add_block(block2);
        table.add_block(block3);

        // Query for April data - all blocks can be skipped
        let skippable = table.get_skippable_blocks("timestamp", "2024-04-01", "2024-04-30");
        assert_eq!(skippable.len(), 3);
    }

    #[test]
    fn test_table_statistics_estimate_selectivity() {
        let mut table = TableStatistics::new("metrics".to_string(), 100000);
        
        let mut hist = Histogram::new("value".to_string());
        hist.add_bucket(HistogramBucket::new("0".to_string(), "50".to_string(), 25000));
        hist.add_bucket(HistogramBucket::new("51".to_string(), "100".to_string(), 50000));
        hist.add_bucket(HistogramBucket::new("101".to_string(), "200".to_string(), 25000));

        table.add_histogram(hist);

        let selectivity = table.estimate_selectivity("value", "50", "100");
        assert!(selectivity.is_some());
        assert!(selectivity.unwrap() > 0.3);
    }

    #[test]
    fn test_table_statistics_total_block_size() {
        let mut table = TableStatistics::new("chunks".to_string(), 100000);
        
        table.add_block(BlockMetadata::new(0, "data".to_string(), 10000, 100000));
        table.add_block(BlockMetadata::new(1, "data".to_string(), 10000, 150000));
        table.add_block(BlockMetadata::new(2, "data".to_string(), 10000, 200000));

        assert_eq!(table.total_block_size(), 450000);
    }

    #[test]
    fn test_table_statistics_avg_compression_ratio() {
        let mut table = TableStatistics::new("compressed".to_string(), 100000);
        
        let block1 = BlockMetadata::new(0, "col".to_string(), 10000, 100000)
            .with_compressed_size(50000);
        let block2 = BlockMetadata::new(1, "col".to_string(), 10000, 100000)
            .with_compressed_size(25000);
        let block3 = BlockMetadata::new(2, "col".to_string(), 10000, 100000)
            .with_compressed_size(75000);

        table.add_block(block1);
        table.add_block(block2);
        table.add_block(block3);

        let avg = table.avg_compression_ratio();
        assert!((avg - 0.5).abs() < 0.01); // (0.5 + 0.25 + 0.75) / 3 = 0.5
    }

    // ============================================================================
    // SECTION 5: Real-World Scenario Tests (6 tests)
    // ============================================================================

    #[test]
    fn test_scenario_user_analytics_query() {
        // Query: Find active users aged 25-40 with high engagement
        let mut table = TableStatistics::new("user_analytics".to_string(), 1000000);

        // Add column statistics
        let age_stats = ColumnStatistics::new("age".to_string(), "Int64".to_string(), 1000000, 5000, 80)
            .with_range(Some("18".to_string()), Some("99".to_string()));
        let status_stats = ColumnStatistics::new("status".to_string(), "String".to_string(), 1000000, 0, 3);
        let engagement_stats = ColumnStatistics::new("engagement_score".to_string(), "Float64".to_string(), 1000000, 0, 50000);

        table.add_column_stats(age_stats);
        table.add_column_stats(status_stats);
        table.add_column_stats(engagement_stats);

        // Verify statistics exist
        assert_eq!(table.column_stats.len(), 3);
        assert!(table.column_stats["status"].is_good_filter_column()); // Low cardinality
    }

    #[test]
    fn test_scenario_time_series_with_blocks() {
        let mut table = TableStatistics::new("events".to_string(), 500000);

        // Create blocks representing daily partitions
        for day in 1..=30 {
            let min = format!("2024-03-{:02}", day);
            let max = format!("2024-03-{:02}", day);
            let block = BlockMetadata::new(day - 1, "timestamp".to_string(), 16000, 128000)
                .with_range(Some(min), Some(max));
            table.add_block(block);
        }

        // Query for specific date range - some blocks will be skipped
        let skippable = table.get_skippable_blocks("timestamp", "2024-04-01", "2024-04-30");
        assert_eq!(skippable.len(), 30); // All March blocks can be skipped
    }

    #[test]
    fn test_scenario_sparse_optional_fields() {
        let mut table = TableStatistics::new("documents".to_string(), 1000000);

        let required_field = ColumnStatistics::new("id".to_string(), "Int64".to_string(), 1000000, 0, 1000000);
        let optional_field = ColumnStatistics::new("metadata".to_string(), "String".to_string(), 1000000, 900000, 100000)
            .with_avg_length(500.0);

        table.add_column_stats(required_field);
        table.add_column_stats(optional_field);

        assert!(!table.column_stats["id"].is_sparse());
        assert!(table.column_stats["metadata"].is_sparse());
    }

    #[test]
    fn test_scenario_histogram_based_optimization() {
        let mut table = TableStatistics::new("sales".to_string(), 1000000);

        let mut price_histogram = Histogram::new("price".to_string());
        price_histogram.add_bucket(HistogramBucket::new("0".to_string(), "100".to_string(), 200000));
        price_histogram.add_bucket(HistogramBucket::new("101".to_string(), "500".to_string(), 400000));
        price_histogram.add_bucket(HistogramBucket::new("501".to_string(), "1000".to_string(), 300000));
        price_histogram.add_bucket(HistogramBucket::new("1001".to_string(), "5000".to_string(), 100000));

        table.add_histogram(price_histogram);

        let selectivity = table.estimate_selectivity("price", "500", "1500");
        assert!(selectivity.is_some());
        let sel = selectivity.unwrap();
        assert!(sel > 0.25 && sel < 0.45);
    }

    #[test]
    fn test_scenario_multi_block_column_skip() {
        let mut table = TableStatistics::new("warehouse".to_string(), 1000000);

        // Create 20 blocks for a region column (zero-padded for numeric comparison)
        for region_id in 1..=20 {
            let region = format!("REGION_{:06}", region_id);
            let block = BlockMetadata::new(region_id as u32 - 1, "region".to_string(), 50000, 400000)
                .with_range(Some(region.clone()), Some(region));
            table.add_block(block);
        }

        // Query for specific regions (zero-padded)
        let skippable = table.get_skippable_blocks("region", "REGION_000021", "REGION_000030");
        assert_eq!(skippable.len(), 20); // All existing blocks are outside query range
    }
}
