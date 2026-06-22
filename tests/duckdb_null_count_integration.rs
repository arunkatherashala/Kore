use std::fs::File;

use kore_fileformat::arrow_converter::{
    ArrowColumn, ArrowDataType, ArrowField, ArrowRecordBatch, ArrowSchema,
};
use kore_fileformat::duckdb_connector::KoreDuckDBConnector;
use kore_fileformat::read_row_group_metadata_from_reader;

#[test]
fn integration_null_count_via_append_from_arrow() {
    let schema = ArrowSchema {
        fields: vec![
            ArrowField {
                name: "a".to_string(),
                data_type: ArrowDataType::Int32,
                nullable: false,
            },
            ArrowField {
                name: "b".to_string(),
                data_type: ArrowDataType::Null,
                nullable: true,
            },
        ],
    };

    let col_a = ArrowColumn::Int32(vec![1, 2, 3, 4]);
    // false = null row in Null column presence bitmap
    let col_b = ArrowColumn::Null(vec![true, false, true, false]);

    let batch = ArrowRecordBatch::new(schema, vec![col_a, col_b], 4);

    let tmp_path = std::env::temp_dir().join("kore_integration_null_count.kore");
    if tmp_path.exists() {
        let _ = std::fs::remove_file(&tmp_path);
    }

    let mut conn = KoreDuckDBConnector::new(tmp_path.to_str().unwrap()).unwrap();
    conn.append_from_arrow(batch).unwrap();

    let mut f = File::open(&tmp_path).expect("open file");
    let meta = read_row_group_metadata_from_reader(&mut f).expect("read metadata");

    assert_eq!(meta.column_stats.len(), 2);
    assert_eq!(meta.column_stats[1].null_count, 2);
}
