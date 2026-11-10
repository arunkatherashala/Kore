//! Usage: kore-compat-writer <out_dir>
//! Writes files with the current kore_fileformat crate plus refusal cases, and <out_dir>/expected.txt
//! with the digest the current crate reads back. The reader (published crate) must agree.
use kore_fileformat::{crc32, from_bytes, to_bytes, DataBlock, DataType};
use std::path::Path;

fn digest(b: &DataBlock) -> String {
    let mut out = format!("rows {} cols {}", b.num_rows(), b.num_columns());
    for c in b.columns() {
        let mut h: u64 = 0xcbf29ce484222325;
        for v in &c.values {
            for byte in v.to_le_bytes() { h ^= byte as u64; h = h.wrapping_mul(0x100000001b3); }
        }
        out += &format!(" | {} {} {:016x}", c.name, c.dtype as u8, h);
    }
    out
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: kore-compat-writer <out_dir>");
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    let n = 1000u64;
    let mut b = DataBlock::new();
    b.add_column("price", DataType::F64, (0..n).map(|i| (i as f64 / 8.0 - 3.0).to_bits()).collect());
    b.add_column("qty", DataType::I64, (0..n).map(|i| (i as i64 * 7919 - 1_000_000) as u64).collect());
    b.add_column("neg_zero_nan", DataType::F64, (0..n).map(|i| if i % 2 == 0 { (-0.0f64).to_bits() } else { f64::NAN.to_bits() }).collect());
    let bytes = to_bytes(&b);
    let back = from_bytes(&bytes).expect("current crate cannot read its own file");
    assert_eq!(digest(&back), digest(&b));
    std::fs::write(dir.join("read_current.kore"), &bytes).unwrap();
    std::fs::write(dir.join("expected.txt"), format!("read_current.kore\t{}\n", digest(&b))).unwrap();

    // a truncated file must be refused
    std::fs::write(dir.join("refuse_truncated.kore"), &bytes[..bytes.len() / 2]).unwrap();
    // a file with a version number from the future (valid CRC): reported, see the reader
    let mut fut = bytes[..bytes.len() - 4].to_vec();
    fut[4..8].copy_from_slice(&99u32.to_le_bytes());
    let c = crc32(&fut);
    fut.extend_from_slice(&c.to_le_bytes());
    std::fs::write(dir.join("info_future_version.kore"), &fut).unwrap();
    // valid CRC but a column count larger than the body: reported, see the reader
    let mut bad = bytes[..bytes.len() - 4].to_vec();
    bad[8..12].copy_from_slice(&1000u32.to_le_bytes());
    let c = crc32(&bad);
    bad.extend_from_slice(&c.to_le_bytes());
    std::fs::write(dir.join("info_bad_column_count.kore"), &bad).unwrap();
    println!("wrote compatibility cases to {}", dir.display());
}
