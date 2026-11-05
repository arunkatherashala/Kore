use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{reader::KoreReader, KoreWriter};

fn block() -> DataBlock {
    let mut s = 7u64;
    let mut rnd = move || { s ^= s << 13; s ^= s >> 7; s ^= s << 17; s };
    DataBlock::new(vec![
        Column::int64("a", (0..1000).map(Some).collect()),
        Column::float64("b", (0..1000).map(|_| Some((rnd() % 1_000_000) as f64 / 7.0)).collect()),
        Column::str_col("c", (0..1000).map(|i| Some(format!("row-{i}"))).collect()),
        Column::int64("big", (0..100_000).map(|_| Some(rnd() as i64)).take(1000).map(Some).map(|x| x.flatten()).collect()),
    ]).unwrap()
}

fn names(b: &DataBlock) -> Vec<&str> { b.columns.iter().map(|c| c.name.as_str()).collect() }

#[test]
fn projection_returns_requested_columns_in_requested_order() {
    std::env::set_var("KORE_READABLE_MODE", "none");
    let bytes = KoreWriter::to_bytes(&block());
    let r = KoreReader::from_bytes_columns(&bytes, &["c", "a"]).unwrap();
    assert_eq!(names(&r), ["c", "a"]);
    assert_eq!(r.num_rows, 1000);
    match &r.columns[1].data {
        ColumnData::Int64(v) => assert_eq!(v[999], Some(999)),
        _ => panic!("wrong type"),
    }
    // duplicates collapse, missing columns are an error, empty selection is allowed
    assert_eq!(names(&KoreReader::from_bytes_columns(&bytes, &["a", "a"]).unwrap()), ["a"]);
    assert!(KoreReader::from_bytes_columns(&bytes, &["nope"]).is_err());
    assert_eq!(KoreReader::from_bytes_columns(&bytes, &[]).unwrap().columns.len(), 0);
}

#[test]
fn damage_in_an_unrequested_column_is_not_touched() {
    std::env::set_var("KORE_READABLE_MODE", "none");
    // 'big' dominates the file, so a flip in the middle lands in its payload
    let mut bytes = KoreWriter::to_bytes(&block());
    let mid = bytes.len() * 9 / 10;
    bytes[mid] ^= 0xFF;
    assert!(KoreReader::from_bytes(&bytes).is_err(), "full read must detect the damage");
    let ok = KoreReader::from_bytes_columns(&bytes, &["a"]);
    assert!(ok.is_ok(), "projection of an undamaged column should still work: {:?}", ok.err());
}
