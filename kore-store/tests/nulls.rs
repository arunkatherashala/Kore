use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{compress, reader::KoreReader, KoreWriter};

fn floats(b: &DataBlock) -> Vec<Option<f64>> {
    match &b.columns[0].data { ColumnData::Float64(v) => v.clone(), _ => panic!("not f64") }
}

fn same(a: &[Option<f64>], b: &[Option<f64>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| match (x, y) {
        (None, None) => true,
        (Some(p), Some(q)) => (p.is_nan() && q.is_nan()) || p == q,
        _ => false,
    })
}

#[test]
fn null_and_nan_are_distinct_through_the_file_format() {
    // high-cardinality -> NanRaw path, low-cardinality -> Dict path
    let high: Vec<Option<f64>> = (0..2000).map(|i| match i % 7 { 0 => None, 1 => Some(f64::NAN), _ => Some(i as f64 * 0.37) }).collect();
    let low: Vec<Option<f64>> = (0..2000).map(|i| match i % 4 { 0 => None, 1 => Some(f64::NAN), 2 => Some(1.5), _ => Some(-0.0) }).collect();
    for vals in [high, low] {
        let b = DataBlock::new(vec![Column::float64("f", vals.clone())]).unwrap();
        let back = KoreReader::from_bytes(&KoreWriter::to_bytes(&b)).unwrap();
        assert!(same(&floats(&back), &vals), "null/NaN not preserved");
    }
}

#[test]
fn canonical_nan_from_older_files_still_means_null() {
    let enc = compress::nan_encode_f64(&[Some(1.0), None]);
    // a pre-change writer stored NULL as f64::NAN, the same bits
    let mut legacy = Vec::new();
    legacy.extend_from_slice(&1.0f64.to_le_bytes());
    legacy.extend_from_slice(&f64::NAN.to_le_bytes());
    assert_eq!(enc, legacy);
    assert_eq!(compress::nan_decode_f64(&legacy, 2), vec![Some(1.0), None]);
}
