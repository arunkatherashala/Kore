use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{compress, reader::KoreReader, KoreWriter};

fn block(n: usize) -> DataBlock {
    let mut s = 42u64;
    let mut rnd = move || { s ^= s << 13; s ^= s >> 7; s ^= s << 17; s };
    DataBlock::new(vec![
        Column::int64("id", (0..n as i64).map(Some).collect()),
        Column::float64("price", (0..n).map(|_| Some((rnd() % 1_000_000_000) as f64 / 1e6)).collect()),
        Column::int64("qty", (0..n).map(|_| Some((rnd() % 10_000) as i64 + 1)).collect()),
        Column::float64("sparse", (0..n).map(|i| if i % 5 == 0 { None } else { Some(i as f64 * 0.25) }).collect()),
    ]).unwrap()
}

#[test]
fn shuffle_roundtrip_and_size() {
    let b = block(100_000);
    std::env::set_var("KORE_READABLE_MODE", "none");

    std::env::remove_var("KORE_SHUFFLE");
    let plain = KoreWriter::to_bytes(&b);

    std::env::set_var("KORE_SHUFFLE", "1");
    let shuffled = KoreWriter::to_bytes(&b);
    std::env::remove_var("KORE_SHUFFLE");

    assert!(shuffled.len() < plain.len(), "shuffle should shrink this data: {} vs {}", shuffled.len(), plain.len());
    let r = KoreReader::from_bytes(&shuffled).unwrap();
    assert_eq!(r.num_rows, 100_000);
    for (a, c) in b.columns.iter().zip(&r.columns) {
        match (&a.data, &c.data) {
            (ColumnData::Int64(x), ColumnData::Int64(y)) => assert_eq!(x, y),
            (ColumnData::Float64(x), ColumnData::Float64(y)) => assert_eq!(x, y),
            _ => panic!("dtype mismatch"),
        }
    }
    println!("plain {} bytes, shuffled {} bytes", plain.len(), shuffled.len());

    // corrupting a shuffled file must still give an error, not a panic
    for i in (0..shuffled.len()).step_by(shuffled.len() / 200) {
        let mut m = shuffled.clone();
        m[i] ^= 0x55;
        let _ = KoreReader::from_bytes(&m);
    }
}

#[test]
fn shuffle_inverse() {
    let data: Vec<u8> = (0..13 * 100).map(|i| (i * 7) as u8).collect();
    for w in [8usize, 9, 13] {
        let d = &data[..data.len() / w * w];
        assert_eq!(compress::byte_unshuffle(&compress::byte_shuffle(d, w), w), d);
    }
}
