use kore_core::{Column, DataBlock};
use kore_store::{reader::KoreReader, KoreWriter};
use std::panic::catch_unwind;

fn sample(n: usize) -> DataBlock {
    DataBlock::new(vec![
        Column::int64("id", (0..n).map(|i| Some(i as i64)).collect()),
        Column::float64("v", (0..n).map(|i| if i % 7 == 0 { None } else { Some(i as f64 * 1.5) }).collect()),
        Column::str_col("s", (0..n).map(|i| Some(format!("k{}", i % 13))).collect()),
    ]).unwrap()
}

#[test]
fn roundtrip_large() {
    let b = sample(2_000_000);
    let bytes = KoreWriter::to_bytes(&b);
    let r = KoreReader::from_bytes(&bytes).unwrap();
    assert_eq!(r.num_rows, 2_000_000);
}

#[test]
fn truncated_files_never_panic() {
    let bytes = KoreWriter::to_bytes(&sample(500));
    let mut panics = vec![];
    for cut in (0..bytes.len()).step_by(1.max(bytes.len() / 400)) {
        let slice = bytes[..cut].to_vec();
        if catch_unwind(|| { let _ = KoreReader::from_bytes(&slice); }).is_err() { panics.push(cut); }
    }
    assert!(panics.is_empty(), "reader panicked on truncation at {:?}", &panics[..panics.len().min(10)]);
}

#[test]
fn bit_flips_never_panic() {
    let bytes = KoreWriter::to_bytes(&sample(500));
    let mut seed = 0x2545F4914F6CDD1Du64;
    let mut panics = vec![];
    for _ in 0..3000 {
        seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
        let mut m = bytes.clone();
        let idx = (seed as usize) % m.len();
        m[idx] ^= 1 << ((seed >> 40) % 8);
        if catch_unwind(|| { let _ = KoreReader::from_bytes(&m); }).is_err() { panics.push(idx); }
    }
    assert!(panics.is_empty(), "reader panicked on bit flips at offsets {:?}", &panics[..panics.len().min(10)]);
}

#[test]
fn concurrent_readers_and_writers() {
    let b = std::sync::Arc::new(sample(50_000));
    let hs: Vec<_> = (0..16).map(|_| { let b = b.clone(); std::thread::spawn(move || {
        for _ in 0..5 {
            let bytes = KoreWriter::to_bytes(&b);
            assert_eq!(KoreReader::from_bytes(&bytes).unwrap().num_rows, 50_000);
        }
    })}).collect();
    for h in hs { h.join().unwrap(); }
}

#[test]
fn payload_corruption_is_detected() {
    let mut bytes = KoreWriter::to_bytes(&sample(2000));
    // the first column's payload starts right after the schema; flip a byte deep inside it
    let mid = bytes.len() / 4;
    bytes[mid] ^= 0xFF;
    let r = KoreReader::from_bytes(&bytes);
    assert!(r.is_err(), "corrupted payload decoded without error");
}
