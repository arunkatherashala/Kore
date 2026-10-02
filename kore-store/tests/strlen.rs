use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{compress, reader::KoreReader, KoreWriter};

fn strings(n: usize) -> Vec<Option<String>> {
    // ISO dates (many distinct values, so no dictionary) plus nulls, empty strings and unicode
    (0..n)
        .map(|i| match i % 11 {
            0 => None,
            1 => Some(String::new()),
            2 => Some(format!("caf\u{e9}-{}\u{1f600}", i)),
            _ => Some(format!("199{}-{:02}-{:02}", i % 9, (i * 7) % 12 + 1, (i * 13) % 28 + 1)),
        })
        .collect()
}

#[test]
fn length_layout_roundtrip_and_smaller() {
    let vals = strings(20_000);
    let b = DataBlock::new(vec![Column::str_col("d", vals.clone())]).unwrap();
    std::env::set_var("KORE_READABLE_MODE", "none");

    std::env::remove_var("KORE_STR_LENGTHS");
    let plain = KoreWriter::to_bytes(&b);
    std::env::set_var("KORE_STR_LENGTHS", "1");
    let lens = KoreWriter::to_bytes(&b);
    std::env::remove_var("KORE_STR_LENGTHS");

    assert!(lens.len() < plain.len(), "length layout should be smaller: {} vs {}", lens.len(), plain.len());
    for bytes in [&plain, &lens] {
        let r = KoreReader::from_bytes(bytes).unwrap();
        match &r.columns[0].data {
            ColumnData::Str(v) => assert_eq!(v, &vals),
            _ => panic!("not a string column"),
        }
    }
    println!("plain {} bytes, lengths {} bytes", plain.len(), lens.len());

    // damaged files must give an error or a value, never a panic
    for i in (0..lens.len()).step_by((lens.len() / 300).max(1)) {
        let mut m = lens.clone();
        m[i] ^= 0x5a;
        let _ = KoreReader::from_bytes(&m);
    }
}

#[test]
fn length_codec_rejects_malformed() {
    let good = compress::encode_strs_len(&[Some("ab".into()), None, Some("c".into())]);
    assert_eq!(compress::decode_strs_len(&good).len(), 3);
    assert!(compress::decode_strs_len(&good[..good.len() - 1]).is_empty());
    assert!(compress::decode_strs_len(&[1, 0, 0]).is_empty());
    let mut huge = good.clone();
    huge[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(compress::decode_strs_len(&huge).is_empty());
}
