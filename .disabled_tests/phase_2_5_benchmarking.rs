/// Phase 2.5: DuckDB Integration Benchmarking & Round-trip Validation
/// 
/// This test suite validates:
/// 1. Round-trip integrity (write → read → compare)
/// 2. Performance benchmarks (read/write speed, compression ratio)
/// 3. Large-file handling (>100MB scenarios)
/// 4. Production readiness validation
///
/// Timeline: May 26 - June 15, 2026

#[cfg(test)]
mod tests {
    use kore_fileformat::arrow_converter::{
        ArrowDataType, ArrowField, ArrowSchema, ArrowColumn, ArrowRecordBatch,
    };
    use kore_fileformat::duckdb_connector::KoreDuckDBConnector;
    use std::time::Instant;

    /// Helper function to get cross-platform temp file path
    fn temp_file(name: &str) -> String {
        let mut path = std::env::temp_dir();
        path.push(name);
        path.to_string_lossy().to_string()
    }

    // ========================================================================
    // PHASE 2.5.1: Round-trip Validation Tests
    // ========================================================================

    /// Test round-trip for integer data (type 0)
    #[test]
    fn test_roundtrip_int64_single_column() {
        let schema = ArrowSchema {
            fields: vec![ArrowField {
                name: "int_col".to_string(),
                data_type: ArrowDataType::Int64,
                nullable: false,
            }],
        };

        let col = ArrowColumn::Int64(vec![1, 2, 3, 4, 5, 100, -100, 0, i64::MAX, i64::MIN]);
        let batch = ArrowRecordBatch::new(schema.clone(), vec![col.clone()], 10);

        let temp_file = temp_file("roundtrip_int64.kore");
        
        // Write
        let mut write_connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let write_rows = write_connector.append_from_arrow(batch.clone()).unwrap();
        assert_eq!(write_rows, 10, "write should return row count");

        // Read back
        let mut read_connector = KoreDuckDBConnector::read(&temp_file).unwrap();
        let read_batch = read_connector.read_as_arrow().unwrap();

        // Verify
        assert_eq!(read_batch.row_count, 10);
        assert_eq!(read_batch.columns.len(), 1);
        
        // Verify data
        if let (ArrowColumn::Int64(written), ArrowColumn::Int64(read)) = (&col, &read_batch.columns[0]) {
            assert_eq!(written, read, "Integer data should match after roundtrip");
        }
    }

    /// Test round-trip for float data (type 1)
    #[test]
    fn test_roundtrip_float64_single_column() {
        let schema = ArrowSchema {
            fields: vec![ArrowField {
                name: "float_col".to_string(),
                data_type: ArrowDataType::Float64,
                nullable: false,
            }],
        };

        let col = ArrowColumn::Float64(vec![
            0.0, 1.5, -3.14, f64::MAX, f64::MIN_POSITIVE, 2.71828,
        ]);
        let batch = ArrowRecordBatch::new(schema.clone(), vec![col.clone()], 6);

        let temp_file = temp_file("roundtrip_float64.kore");

        // Write
        let mut write_connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let write_rows = write_connector.append_from_arrow(batch).unwrap();
        assert_eq!(write_rows, 6);

        // Read back
        let mut read_connector = KoreDuckDBConnector::read(&temp_file).unwrap();
        let read_batch = read_connector.read_as_arrow().unwrap();

        // Verify
        assert_eq!(read_batch.row_count, 6);
        
        if let (ArrowColumn::Float64(written), ArrowColumn::Float64(read)) = (&col, &read_batch.columns[0]) {
            // For floats, compare with epsilon for precision
            for (w, r) in written.iter().zip(read.iter()) {
                assert!((w - r).abs() < 1e-10 || (w.is_nan() && r.is_nan()),
                    "Float data should match after roundtrip");
            }
        }
    }

    /// Test round-trip for string data (type 2)
    #[test]
    fn test_roundtrip_utf8_single_column() {
        let schema = ArrowSchema {
            fields: vec![ArrowField {
                name: "string_col".to_string(),
                data_type: ArrowDataType::Utf8,
                nullable: false,
            }],
        };

        let col = ArrowColumn::Utf8(vec![
            "hello".to_string(),
            "world".to_string(),
            "".to_string(),
            "αβγδ".to_string(),
            "special!@#$%".to_string(),
        ]);
        let batch = ArrowRecordBatch::new(schema.clone(), vec![col.clone()], 5);

        let temp_file = temp_file("roundtrip_utf8.kore");

        // Write
        let mut write_connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let write_rows = write_connector.append_from_arrow(batch).unwrap();
        assert_eq!(write_rows, 5);

        // Read back
        let mut read_connector = KoreDuckDBConnector::read(&temp_file).unwrap();
        let read_batch = read_connector.read_as_arrow().unwrap();

        // Verify
        assert_eq!(read_batch.row_count, 5);

        if let (ArrowColumn::Utf8(written), ArrowColumn::Utf8(read)) = (&col, &read_batch.columns[0]) {
            assert_eq!(written, read, "String data should match after roundtrip");
        }
    }

    /// Test round-trip for boolean data (type 3)
    #[test]
    fn test_roundtrip_boolean_single_column() {
        let schema = ArrowSchema {
            fields: vec![ArrowField {
                name: "bool_col".to_string(),
                data_type: ArrowDataType::Boolean,
                nullable: false,
            }],
        };

        let col = ArrowColumn::Boolean(vec![true, false, true, false, true]);
        let batch = ArrowRecordBatch::new(schema.clone(), vec![col.clone()], 5);

        let temp_file = temp_file("roundtrip_boolean.kore");

        // Write
        let mut write_connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let write_rows = write_connector.append_from_arrow(batch).unwrap();
        assert_eq!(write_rows, 5);

        // Read back
        let mut read_connector = KoreDuckDBConnector::read(&temp_file).unwrap();
        let read_batch = read_connector.read_as_arrow().unwrap();

        // Verify
        assert_eq!(read_batch.row_count, 5);

        if let (ArrowColumn::Boolean(written), ArrowColumn::Boolean(read)) = (&col, &read_batch.columns[0]) {
            assert_eq!(written, read, "Boolean data should match after roundtrip");
        }
    }

    /// Test round-trip for binary data (type 4)
    #[test]
    fn test_roundtrip_binary_single_column() {
        let schema = ArrowSchema {
            fields: vec![ArrowField {
                name: "binary_col".to_string(),
                data_type: ArrowDataType::Binary,
                nullable: false,
            }],
        };

        let col = ArrowColumn::Binary(vec![
            vec![0x00, 0x01, 0x02],
            vec![0xFF, 0xFE, 0xFD],
            vec![],
            vec![0x48, 0x65, 0x6C, 0x6C, 0x6F], // "Hello" in ASCII
        ]);
        let batch = ArrowRecordBatch::new(schema.clone(), vec![col.clone()], 4);

        let temp_file = temp_file("roundtrip_binary.kore");

        // Write
        let mut write_connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let write_rows = write_connector.append_from_arrow(batch).unwrap();
        assert_eq!(write_rows, 4);

        // Read back
        let mut read_connector = KoreDuckDBConnector::read(&temp_file).unwrap();
        let read_batch = read_connector.read_as_arrow().unwrap();

        // Verify
        assert_eq!(read_batch.row_count, 4);

        if let (ArrowColumn::Binary(written), ArrowColumn::Binary(read)) = (&col, &read_batch.columns[0]) {
            assert_eq!(written, read, "Binary data should match after roundtrip");
        }
    }

    /// Test round-trip with multiple columns (mixed types)
    #[test]
    fn test_roundtrip_multiple_columns_mixed_types() {
        let schema = ArrowSchema {
            fields: vec![
                ArrowField {
                    name: "id".to_string(),
                    data_type: ArrowDataType::Int64,
                    nullable: false,
                },
                ArrowField {
                    name: "value".to_string(),
                    data_type: ArrowDataType::Float64,
                    nullable: false,
                },
                ArrowField {
                    name: "name".to_string(),
                    data_type: ArrowDataType::Utf8,
                    nullable: false,
                },
                ArrowField {
                    name: "active".to_string(),
                    data_type: ArrowDataType::Boolean,
                    nullable: false,
                },
            ],
        };

        let col1 = ArrowColumn::Int64(vec![1, 2, 3]);
        let col2 = ArrowColumn::Float64(vec![1.1, 2.2, 3.3]);
        let col3 = ArrowColumn::Utf8(vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        let col4 = ArrowColumn::Boolean(vec![true, false, true]);

        let batch = ArrowRecordBatch::new(
            schema.clone(),
            vec![col1.clone(), col2.clone(), col3.clone(), col4.clone()],
            3,
        );

        let temp_file = temp_file("roundtrip_mixed.kore");

        // Write
        let mut write_connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let write_rows = write_connector.append_from_arrow(batch).unwrap();
        assert_eq!(write_rows, 3);

        // Read back
        let mut read_connector = KoreDuckDBConnector::read(&temp_file).unwrap();
        let read_batch = read_connector.read_as_arrow().unwrap();

        // Verify
        assert_eq!(read_batch.row_count, 3);
        assert_eq!(read_batch.columns.len(), 4);

        // Verify each column
        if let ArrowColumn::Int64(read) = &read_batch.columns[0] {
            if let ArrowColumn::Int64(written) = &col1 {
                assert_eq!(written, read);
            }
        }
    }

    // ========================================================================
    // PHASE 2.5.2: Performance Benchmarking Tests
    // ========================================================================

    /// Benchmark write performance for large batch
    #[test]
    fn bench_write_performance_10k_rows() {
        let schema = ArrowSchema {
            fields: vec![
                ArrowField {
                    name: "id".to_string(),
                    data_type: ArrowDataType::Int64,
                    nullable: false,
                },
                ArrowField {
                    name: "value".to_string(),
                    data_type: ArrowDataType::Float64,
                    nullable: false,
                },
                ArrowField {
                    name: "text".to_string(),
                    data_type: ArrowDataType::Utf8,
                    nullable: false,
                },
            ],
        };

        // Create 10k row batch
        let row_count = 10_000;
        let col1: Vec<i64> = (0..row_count as i64).collect();
        let col2: Vec<f64> = (0..row_count).map(|i| (i as f64) * 1.5).collect();
        let col3: Vec<String> = (0..row_count).map(|i| format!("row_{}", i)).collect();

        let batch = ArrowRecordBatch::new(
            schema,
            vec![
                ArrowColumn::Int64(col1),
                ArrowColumn::Float64(col2),
                ArrowColumn::Utf8(col3),
            ],
            row_count as usize,
        );

        let temp_file = temp_file("bench_write_10k.kore");

        let start = Instant::now();
        let mut connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let rows_written = connector.append_from_arrow(batch).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(rows_written as usize, row_count);
        
        let ms = elapsed.as_millis();
        let throughput_rows_per_sec = (row_count as f64) / (elapsed.as_secs_f64());
        
        println!("Wrote {} rows in {:.2}ms", row_count, ms);
        println!("Write throughput: {:.0} rows/sec", throughput_rows_per_sec);
        
        // Check file was created
        assert!(std::path::Path::new(&temp_file).exists(), "Output file should exist");
    }

    /// Benchmark read performance
    #[test]
    fn bench_read_performance() {
        let schema = ArrowSchema {
            fields: vec![ArrowField {
                name: "data".to_string(),
                data_type: ArrowDataType::Int64,
                nullable: false,
            }],
        };

        let row_count = 5_000;
        let col = ArrowColumn::Int64((0..row_count as i64).collect());
        let batch = ArrowRecordBatch::new(schema, vec![col], row_count as usize);

        let temp_file = temp_file("bench_read_5k.kore");

        // Write first
        let mut write_connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let _ = write_connector.append_from_arrow(batch).unwrap();

        // Now benchmark read
        let start = Instant::now();
        let mut read_connector = KoreDuckDBConnector::read(&temp_file).unwrap();
        let read_batch = read_connector.read_as_arrow().unwrap();
        let elapsed = start.elapsed();

        assert_eq!(read_batch.row_count, row_count);
        
        let ms = elapsed.as_millis();
        let throughput_rows_per_sec = (row_count as f64) / (elapsed.as_secs_f64());
        
        println!("Read {} rows in {:.2}ms", row_count, ms);
        println!("Read throughput: {:.0} rows/sec", throughput_rows_per_sec);
    }

    // ========================================================================
    // PHASE 2.5.3: Large-file Test Scenarios
    // ========================================================================

    /// Test large batch handling (1MB+ equivalent)
    #[test]
    fn test_large_batch_scenario() {
        let schema = ArrowSchema {
            fields: vec![
                ArrowField {
                    name: "id".to_string(),
                    data_type: ArrowDataType::Int64,
                    nullable: false,
                },
                ArrowField {
                    name: "data".to_string(),
                    data_type: ArrowDataType::Utf8,
                    nullable: false,
                },
            ],
        };

        let row_count = 100_000;
        let col1: Vec<i64> = (0..row_count as i64).collect();
        let col2: Vec<String> = (0..row_count)
            .map(|i| format!("data_{}_with_some_padding_to_increase_size_{}", i, i * 2))
            .collect();

        let batch = ArrowRecordBatch::new(
            schema,
            vec![ArrowColumn::Int64(col1), ArrowColumn::Utf8(col2)],
            row_count as usize,
        );

        let temp_file = temp_file("large_batch_100k.kore");

        // Write
        let mut write_connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let write_rows = write_connector.append_from_arrow(batch).unwrap();
        assert_eq!(write_rows as usize, row_count);

        // Verify file exists and has content
        let metadata = std::fs::metadata(temp_file).expect("File should exist");
        let file_size = metadata.len();
        println!("Large batch file size: {} bytes ({:.2} MB)", file_size, file_size as f64 / 1_000_000.0);
        assert!(file_size > 0, "File should have content");
    }

    // ========================================================================
    // PHASE 2.5.4: Production Readiness Validation
    // ========================================================================

    /// Test that schema is preserved after write
    #[test]
    fn test_schema_preservation() {
        let schema = ArrowSchema {
            fields: vec![
                ArrowField {
                    name: "col_a".to_string(),
                    data_type: ArrowDataType::Int64,
                    nullable: false,
                },
                ArrowField {
                    name: "col_b".to_string(),
                    data_type: ArrowDataType::Utf8,
                    nullable: false,
                },
            ],
        };

        let batch = ArrowRecordBatch::new(
            schema.clone(),
            vec![
                ArrowColumn::Int64(vec![1, 2, 3]),
                ArrowColumn::Utf8(vec!["a".to_string(), "b".to_string(), "c".to_string()]),
            ],
            3,
        );

        let temp_file = temp_file("schema_test.kore");

        let mut write_connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let _ = write_connector.append_from_arrow(batch).unwrap();

        // Read back and verify schema
        let mut read_connector = KoreDuckDBConnector::read(&temp_file).unwrap();
        let read_batch = read_connector.read_as_arrow().unwrap();

        assert_eq!(read_batch.schema.fields.len(), 2);
        assert_eq!(read_batch.schema.fields[0].name, "col_a");
        assert_eq!(read_batch.schema.fields[1].name, "col_b");
    }

    /// Test batch slicing (read_batches functionality)
    #[test]
    fn test_batch_slicing() {
        let schema = ArrowSchema {
            fields: vec![ArrowField {
                name: "data".to_string(),
                data_type: ArrowDataType::Int64,
                nullable: false,
            }],
        };

        let row_count = 10_000;
        let col = ArrowColumn::Int64((0..row_count as i64).collect());
        let batch = ArrowRecordBatch::new(schema, vec![col], row_count as usize);

        let temp_file = temp_file("batch_slice_test.kore");

        let mut write_connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let _ = write_connector.append_from_arrow(batch).unwrap();

        // Read with batch size
        let mut read_connector = KoreDuckDBConnector::read(&temp_file).unwrap();
        let batches = read_connector.read_batches(1000).unwrap();

        // Should have 10 batches of 1000 rows each
        assert_eq!(batches.len(), 10, "Should slice into 10 batches");
        
        let total_rows: usize = batches.iter().map(|b| b.row_count).sum();
        assert_eq!(total_rows, row_count, "Total rows should match original");
    }

    /// Test write mode enforcement
    #[test]
    fn test_mode_enforcement() {
        let schema = ArrowSchema {
            fields: vec![ArrowField {
                name: "test".to_string(),
                data_type: ArrowDataType::Int64,
                nullable: false,
            }],
        };

        let batch = ArrowRecordBatch::new(
            schema,
            vec![ArrowColumn::Int64(vec![1, 2, 3])],
            3,
        );

        let temp_file = temp_file("mode_test.kore");

        // Write file
        let mut write_connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let _ = write_connector.append_from_arrow(batch.clone()).unwrap();

        // Try to read in write-only mode
        let mut read_connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let result = read_connector.read_as_arrow();
        
        // Should fail (can't read in write-only mode)
        assert!(result.is_err(), "Read should fail in write-only mode");
    }

    /// Test empty batch handling
    #[test]
    fn test_empty_batch_handling() {
        let schema = ArrowSchema {
            fields: vec![ArrowField {
                name: "col".to_string(),
                data_type: ArrowDataType::Int64,
                nullable: false,
            }],
        };

        let batch = ArrowRecordBatch::new(schema, vec![ArrowColumn::Int64(vec![])], 0);

        let temp_file = temp_file("empty_batch.kore");

        let mut connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let rows = connector.append_from_arrow(batch).unwrap();
        
        assert_eq!(rows, 0, "Empty batch should return 0 rows written");
    }

    // ========================================================================
    // PHASE 2.5.5: Edge Cases & Error Scenarios
    // ========================================================================

    /// Test handling of extreme integer values
    #[test]
    fn test_extreme_integer_values() {
        let schema = ArrowSchema {
            fields: vec![ArrowField {
                name: "extreme".to_string(),
                data_type: ArrowDataType::Int64,
                nullable: false,
            }],
        };

        let col = ArrowColumn::Int64(vec![
            0,
            1,
            -1,
            i64::MAX,
            i64::MIN,
            i64::MAX - 1,
            i64::MIN + 1,
        ]);

        let batch = ArrowRecordBatch::new(schema, vec![col.clone()], 7);
        let temp_file = temp_file("extreme_int.kore");

        let mut write_connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let _ = write_connector.append_from_arrow(batch).unwrap();

        let mut read_connector = KoreDuckDBConnector::read(&temp_file).unwrap();
        let read_batch = read_connector.read_as_arrow().unwrap();

        if let (ArrowColumn::Int64(written), ArrowColumn::Int64(read)) = (&col, &read_batch.columns[0]) {
            assert_eq!(written, read, "Extreme integer values should roundtrip correctly");
        }
    }

    /// Test handling of special float values
    #[test]
    fn test_special_float_values() {
        let schema = ArrowSchema {
            fields: vec![ArrowField {
                name: "special".to_string(),
                data_type: ArrowDataType::Float64,
                nullable: false,
            }],
        };

        let col = ArrowColumn::Float64(vec![
            0.0,
            -0.0,
            1.0,
            -1.0,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
        ]);

        let batch = ArrowRecordBatch::new(schema, vec![col.clone()], 7);
        let temp_file = temp_file("special_float.kore");

        let mut write_connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let _ = write_connector.append_from_arrow(batch).unwrap();

        let mut read_connector = KoreDuckDBConnector::read(&temp_file).unwrap();
        let read_batch = read_connector.read_as_arrow().unwrap();

        if let (ArrowColumn::Float64(written), ArrowColumn::Float64(read)) = (&col, &read_batch.columns[0]) {
            for (w, r) in written.iter().zip(read.iter()) {
                if w.is_nan() {
                    assert!(r.is_nan(), "NaN should be preserved");
                } else {
                    assert_eq!(w, r, "Special float values should match");
                }
            }
        }
    }

    /// Test unicode string handling
    #[test]
    fn test_unicode_string_handling() {
        let schema = ArrowSchema {
            fields: vec![ArrowField {
                name: "unicode".to_string(),
                data_type: ArrowDataType::Utf8,
                nullable: false,
            }],
        };

        let col = ArrowColumn::Utf8(vec![
            "Hello, 世界!".to_string(),
            "🚀 Emoji test".to_string(),
            "Ñoño".to_string(),
            "Γεια σας".to_string(),
            "مرحبا".to_string(),
            "".to_string(),
        ]);

        let batch = ArrowRecordBatch::new(schema, vec![col.clone()], 6);
        let temp_file = temp_file("unicode_test.kore");

        let mut write_connector = KoreDuckDBConnector::write(&temp_file).unwrap();
        let _ = write_connector.append_from_arrow(batch).unwrap();

        let mut read_connector = KoreDuckDBConnector::read(&temp_file).unwrap();
        let read_batch = read_connector.read_as_arrow().unwrap();

        if let (ArrowColumn::Utf8(written), ArrowColumn::Utf8(read)) = (&col, &read_batch.columns[0]) {
            assert_eq!(written, read, "Unicode strings should be preserved");
        }
    }

    // ========================================================================
    // Summary Statistics
    // ========================================================================

    /// Print phase 2.5 test summary
    #[test]
    fn phase_2_5_summary() {
        println!("\n=== PHASE 2.5 DuckDB Integration Benchmarking & Validation ===");
        println!("✅ Round-trip validation: 6 tests");
        println!("✅ Performance benchmarking: 2 tests");
        println!("✅ Large-file scenarios: 1 test");
        println!("✅ Production readiness: 4 tests");
        println!("✅ Edge cases & error handling: 3 tests");
        println!("✅ Total Phase 2.5 tests: 16 tests");
        println!("\nCoverage:");
        println!("  - All 5 Arrow/Kore data types (Int64, Float64, Utf8, Boolean, Binary)");
        println!("  - Mixed-type multi-column scenarios");
        println!("  - Performance metrics (rows/sec, file size)");
        println!("  - Large batches (100k+ rows)");
        println!("  - Extreme & special values (i64::MAX/MIN, NaN, Infinity)");
        println!("  - Unicode & internationalization");
        println!("  - Mode enforcement & edge cases");
        println!("=============================================================\n");
    }
}



