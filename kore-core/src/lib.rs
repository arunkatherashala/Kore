pub mod error;
pub mod types;
pub mod traits;

pub use error::KoreError;
pub use types::*;
pub use traits::{Estimator, Transformer};

#[cfg(test)]
mod tests {
    use super::*;

    // ─── DataBlock::new() ───────────────────────────────────────────────────────

    #[test]
    fn datablock_new_equal_length_columns() {
        let cols = vec![
            Column::int64("id", vec![Some(1), Some(2), Some(3)]),
            Column::float64("score", vec![Some(1.0), Some(2.5), Some(3.7)]),
        ];
        let block = DataBlock::new(cols).unwrap();
        assert_eq!(block.num_rows, 3);
        assert_eq!(block.columns.len(), 2);
    }

    #[test]
    fn datablock_new_mismatched_lengths_fails() {
        let cols = vec![
            Column::int64("id", vec![Some(1), Some(2)]),
            Column::float64("score", vec![Some(1.0)]),
        ];
        let result = DataBlock::new(cols);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, KoreError::SchemaMismatch(_)));
    }

    #[test]
    fn datablock_new_no_columns() {
        let block = DataBlock::new(vec![]).unwrap();
        assert_eq!(block.num_rows, 0);
        assert_eq!(block.columns.len(), 0);
    }

    // ─── DataBlock::empty() ─────────────────────────────────────────────────────

    #[test]
    fn datablock_empty_has_zero_rows_and_columns() {
        let block = DataBlock::empty();
        assert_eq!(block.num_rows, 0);
        assert_eq!(block.columns.len(), 0);
    }

    // ─── DataBlock::num_rows ────────────────────────────────────────────────────

    #[test]
    fn datablock_num_rows_single_row() {
        let block = DataBlock::new(vec![
            Column::int64("x", vec![Some(42)]),
        ]).unwrap();
        assert_eq!(block.num_rows, 1);
    }

    #[test]
    fn datablock_num_rows_many_rows() {
        let data: Vec<Option<i64>> = (0..100).map(|i| Some(i)).collect();
        let block = DataBlock::new(vec![Column::int64("x", data)]).unwrap();
        assert_eq!(block.num_rows, 100);
    }

    // ─── DataBlock::concat() ────────────────────────────────────────────────────

    #[test]
    fn datablock_concat_two_blocks() {
        let b1 = DataBlock::new(vec![
            Column::int64("id", vec![Some(1), Some(2)]),
        ]).unwrap();
        let b2 = DataBlock::new(vec![
            Column::int64("id", vec![Some(3), Some(4), Some(5)]),
        ]).unwrap();
        let combined = DataBlock::concat(vec![b1, b2]).unwrap();
        assert_eq!(combined.num_rows, 5);
        let col = combined.column("id").unwrap();
        assert_eq!(col.data.get_value(0), Value::Int(1));
        assert_eq!(col.data.get_value(4), Value::Int(5));
    }

    #[test]
    fn datablock_concat_empty_list() {
        let combined = DataBlock::concat(vec![]).unwrap();
        assert_eq!(combined.num_rows, 0);
        assert_eq!(combined.columns.len(), 0);
    }

    #[test]
    fn datablock_concat_schema_mismatch_fails() {
        let b1 = DataBlock::new(vec![
            Column::int64("id", vec![Some(1)]),
        ]).unwrap();
        let b2 = DataBlock::new(vec![
            Column::int64("id", vec![Some(2)]),
            Column::float64("extra", vec![Some(1.0)]),
        ]).unwrap();
        let result = DataBlock::concat(vec![b1, b2]);
        assert!(result.is_err());
    }

    // ─── DataBlock::select_rows() ───────────────────────────────────────────────

    #[test]
    fn datablock_select_rows_subset() {
        let block = DataBlock::new(vec![
            Column::int64("id", vec![Some(10), Some(20), Some(30), Some(40)]),
            Column::str_col("name", vec![
                Some("a".into()), Some("b".into()), Some("c".into()), Some("d".into()),
            ]),
        ]).unwrap();
        let selected = block.select_rows(&[1, 3]);
        assert_eq!(selected.num_rows, 2);
        assert_eq!(selected.column("id").unwrap().data.get_value(0), Value::Int(20));
        assert_eq!(selected.column("id").unwrap().data.get_value(1), Value::Int(40));
        assert_eq!(selected.column("name").unwrap().data.get_value(0), Value::Str("b".into()));
    }

    #[test]
    fn datablock_select_rows_empty_indices() {
        let block = DataBlock::new(vec![
            Column::int64("x", vec![Some(1), Some(2)]),
        ]).unwrap();
        let selected = block.select_rows(&[]);
        assert_eq!(selected.num_rows, 0);
    }

    // ─── Column constructors ────────────────────────────────────────────────────

    #[test]
    fn column_int64_constructor() {
        let col = Column::int64("age", vec![Some(25), None, Some(30)]);
        assert_eq!(col.name, "age");
        assert_eq!(col.data.len(), 3);
        assert_eq!(col.data.dtype(), DataType::Int64);
    }

    #[test]
    fn column_float64_constructor() {
        let col = Column::float64("temp", vec![Some(36.6), Some(37.0)]);
        assert_eq!(col.name, "temp");
        assert_eq!(col.data.len(), 2);
        assert_eq!(col.data.dtype(), DataType::Float64);
    }

    #[test]
    fn column_str_constructor() {
        let col = Column::str_col("city", vec![Some("NYC".into()), None]);
        assert_eq!(col.name, "city");
        assert_eq!(col.data.len(), 2);
        assert_eq!(col.data.dtype(), DataType::Str);
    }

    #[test]
    fn column_bool_constructor() {
        let col = Column::bool_col("flag", vec![Some(true), Some(false), None]);
        assert_eq!(col.name, "flag");
        assert_eq!(col.data.len(), 3);
        assert_eq!(col.data.dtype(), DataType::Bool);
    }

    // ─── ColumnData::get_value() ────────────────────────────────────────────────

    #[test]
    fn column_data_get_value_int64() {
        let data = ColumnData::Int64(vec![Some(42), None, Some(-1)]);
        assert_eq!(data.get_value(0), Value::Int(42));
        assert_eq!(data.get_value(1), Value::Null);
        assert_eq!(data.get_value(2), Value::Int(-1));
    }

    #[test]
    fn column_data_get_value_float64() {
        let data = ColumnData::Float64(vec![Some(3.14), None]);
        assert_eq!(data.get_value(0), Value::Float(3.14));
        assert_eq!(data.get_value(1), Value::Null);
    }

    #[test]
    fn column_data_get_value_bool() {
        let data = ColumnData::Bool(vec![Some(true), Some(false), None]);
        assert_eq!(data.get_value(0), Value::Bool(true));
        assert_eq!(data.get_value(1), Value::Bool(false));
        assert_eq!(data.get_value(2), Value::Null);
    }

    #[test]
    fn column_data_get_value_str() {
        let data = ColumnData::Str(vec![Some("hello".into()), None]);
        assert_eq!(data.get_value(0), Value::Str("hello".into()));
        assert_eq!(data.get_value(1), Value::Null);
    }

    #[test]
    fn column_data_get_value_out_of_bounds_returns_null() {
        let data = ColumnData::Int64(vec![Some(1)]);
        assert_eq!(data.get_value(99), Value::Null);
    }

    // ─── Value tests ────────────────────────────────────────────────────────────

    #[test]
    fn value_type_name() {
        assert_eq!(Value::Int(1).type_name(), "Int64");
        assert_eq!(Value::Float(1.0).type_name(), "Float64");
        assert_eq!(Value::Bool(true).type_name(), "Bool");
        assert_eq!(Value::Str("x".into()).type_name(), "Str");
        assert_eq!(Value::Null.type_name(), "Null");
    }

    #[test]
    fn value_as_f64() {
        assert_eq!(Value::Int(5).as_f64(), Some(5.0));
        assert_eq!(Value::Float(2.5).as_f64(), Some(2.5));
        assert_eq!(Value::Bool(true).as_f64(), None);
        assert_eq!(Value::Str("x".into()).as_f64(), None);
        assert_eq!(Value::Null.as_f64(), None);
    }

    #[test]
    fn value_equality() {
        assert_eq!(Value::Int(10), Value::Int(10));
        assert_ne!(Value::Int(10), Value::Int(20));
        assert_eq!(Value::Null, Value::Null);
        assert_ne!(Value::Int(0), Value::Null);
    }

    #[test]
    fn value_debug_format() {
        let dbg = format!("{:?}", Value::Int(42));
        assert!(dbg.contains("42"));
        let dbg_null = format!("{:?}", Value::Null);
        assert!(dbg_null.contains("Null"));
    }

    // ─── KoreError tests ────────────────────────────────────────────────────────

    #[test]
    fn error_display_column_not_found() {
        let e = KoreError::ColumnNotFound("foo".into());
        assert_eq!(e.to_string(), "column not found: foo");
    }

    #[test]
    fn error_display_schema_mismatch() {
        let e = KoreError::SchemaMismatch("bad schema".into());
        assert_eq!(e.to_string(), "schema mismatch: bad schema");
    }

    #[test]
    fn error_display_type_mismatch() {
        let e = KoreError::TypeMismatch {
            expected: "Int64".into(),
            got: "Str".into(),
        };
        assert_eq!(e.to_string(), "type mismatch: expected Int64, got Str");
    }

    #[test]
    fn error_display_index_out_of_bounds() {
        let e = KoreError::IndexOutOfBounds(42);
        assert_eq!(e.to_string(), "index out of bounds: 42");
    }

    #[test]
    fn error_display_not_fitted() {
        let e = KoreError::NotFitted;
        assert_eq!(e.to_string(), "model not fitted");
    }

    #[test]
    fn error_display_empty_dataset() {
        let e = KoreError::EmptyDataset;
        assert_eq!(e.to_string(), "empty dataset");
    }

    #[test]
    fn error_display_parse_error() {
        let e = KoreError::ParseError("bad input".into());
        assert_eq!(e.to_string(), "parse error: bad input");
    }

    #[test]
    fn error_display_network_error() {
        let e = KoreError::NetworkError("connection refused".into());
        assert_eq!(e.to_string(), "network error: connection refused");
    }

    #[test]
    fn error_display_timeout() {
        let e = KoreError::Timeout("5s elapsed".into());
        assert_eq!(e.to_string(), "operation timed out: 5s elapsed");
    }

    #[test]
    fn error_display_worker_unavailable() {
        let e = KoreError::WorkerUnavailable("node-3".into());
        assert_eq!(e.to_string(), "worker unavailable: node-3");
    }

    #[test]
    fn error_display_permission_denied() {
        let e = KoreError::PermissionDenied("read access".into());
        assert_eq!(e.to_string(), "permission denied: read access");
    }

    #[test]
    fn error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let kore_err: KoreError = io_err.into();
        assert!(matches!(kore_err, KoreError::Io(_)));
        assert!(kore_err.to_string().contains("file missing"));
    }

    #[test]
    fn error_from_serde_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json{{{").unwrap_err();
        let kore_err: KoreError = json_err.into();
        assert!(matches!(kore_err, KoreError::Json(_)));
    }

    // ─── Edge cases ─────────────────────────────────────────────────────────────

    #[test]
    fn datablock_all_null_column() {
        let col = Column::int64("nulls", vec![None, None, None, None]);
        let block = DataBlock::new(vec![col]).unwrap();
        assert_eq!(block.num_rows, 4);
        for i in 0..4 {
            assert_eq!(block.columns[0].data.get_value(i), Value::Null);
        }
    }

    #[test]
    fn datablock_all_same_values() {
        let col = Column::float64("constant", vec![Some(7.0); 50]);
        let block = DataBlock::new(vec![col]).unwrap();
        assert_eq!(block.num_rows, 50);
        for i in 0..50 {
            assert_eq!(block.columns[0].data.get_value(i), Value::Float(7.0));
        }
    }

    #[test]
    fn datablock_large_block_does_not_crash() {
        let n = 1_000_000;
        let data: Vec<Option<i64>> = (0..n).map(|i| Some(i as i64)).collect();
        let block = DataBlock::new(vec![Column::int64("big", data)]).unwrap();
        assert_eq!(block.num_rows, n);
        assert_eq!(block.columns[0].data.get_value(999_999), Value::Int(999_999));
    }

    #[test]
    fn datablock_mixed_column_types() {
        let block = DataBlock::new(vec![
            Column::int64("i", vec![Some(1), Some(2)]),
            Column::float64("f", vec![Some(1.1), Some(2.2)]),
            Column::str_col("s", vec![Some("a".into()), Some("b".into())]),
            Column::bool_col("b", vec![Some(true), Some(false)]),
        ]).unwrap();
        assert_eq!(block.num_rows, 2);
        assert_eq!(block.columns.len(), 4);
    }

    // ─── ColumnData::append_value() ─────────────────────────────────────────────

    #[test]
    fn column_data_append_value_correct_type() {
        let mut data = ColumnData::Int64(vec![]);
        data.append_value(&Value::Int(10)).unwrap();
        data.append_value(&Value::Null).unwrap();
        assert_eq!(data.len(), 2);
        assert_eq!(data.get_value(0), Value::Int(10));
        assert_eq!(data.get_value(1), Value::Null);
    }

    #[test]
    fn column_data_append_value_type_mismatch() {
        let mut data = ColumnData::Int64(vec![]);
        let result = data.append_value(&Value::Str("oops".into()));
        assert!(result.is_err());
    }

    // ─── DataBlock::sort_by() ───────────────────────────────────────────────────

    #[test]
    fn datablock_sort_by_int_ascending() {
        let block = DataBlock::new(vec![
            Column::int64("x", vec![Some(3), Some(1), Some(2)]),
        ]).unwrap();
        let sorted = block.sort_by("x", true).unwrap();
        assert_eq!(sorted.columns[0].data.get_value(0), Value::Int(1));
        assert_eq!(sorted.columns[0].data.get_value(1), Value::Int(2));
        assert_eq!(sorted.columns[0].data.get_value(2), Value::Int(3));
    }

    #[test]
    fn datablock_sort_by_nonexistent_column_fails() {
        let block = DataBlock::new(vec![
            Column::int64("x", vec![Some(1)]),
        ]).unwrap();
        let result = block.sort_by("missing", true);
        assert!(matches!(result.unwrap_err(), KoreError::ColumnNotFound(_)));
    }

    // ─── StrDict column ─────────────────────────────────────────────────────────

    #[test]
    fn str_dict_column_get_value() {
        let col = Column::str_dict(
            "category",
            vec![0, 1, u8::MAX, 0],
            vec!["cat".into(), "dog".into()],
        );
        assert_eq!(col.data.get_value(0), Value::Str("cat".into()));
        assert_eq!(col.data.get_value(1), Value::Str("dog".into()));
        assert_eq!(col.data.get_value(2), Value::Null);
        assert_eq!(col.data.get_value(3), Value::Str("cat".into()));
    }

    #[test]
    fn str_dict_column_get_str() {
        let col = Column::str_dict(
            "cat",
            vec![0, 1, u8::MAX],
            vec!["alpha".into(), "beta".into()],
        );
        assert_eq!(col.data.get_str(0), Some("alpha"));
        assert_eq!(col.data.get_str(1), Some("beta"));
        assert_eq!(col.data.get_str(2), None);
    }

    // ─── JoinKey ────────────────────────────────────────────────────────────────

    #[test]
    fn join_key_from_value() {
        assert_eq!(JoinKey::from(&Value::Int(5)), JoinKey::Int(5));
        assert_eq!(JoinKey::from(&Value::Bool(true)), JoinKey::Bool(true));
        assert_eq!(JoinKey::from(&Value::Str("x".into())), JoinKey::Str("x".into()));
        assert_eq!(JoinKey::from(&Value::Null), JoinKey::Null);
        assert_eq!(JoinKey::from(&Value::Float(1.0)), JoinKey::Null);
    }

    #[test]
    fn datablock_join_key_lookup() {
        let block = DataBlock::new(vec![
            Column::int64("id", vec![Some(10), Some(20)]),
        ]).unwrap();
        assert_eq!(block.join_key(0, "id").unwrap(), JoinKey::Int(10));
        assert_eq!(block.join_key(1, "id").unwrap(), JoinKey::Int(20));
    }

    #[test]
    fn datablock_join_key_missing_column() {
        let block = DataBlock::new(vec![
            Column::int64("id", vec![Some(1)]),
        ]).unwrap();
        let result = block.join_key(0, "nope");
        assert!(matches!(result.unwrap_err(), KoreError::ColumnNotFound(_)));
    }
}
