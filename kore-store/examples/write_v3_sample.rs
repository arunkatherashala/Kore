//! Writes one v2 and one v3 file of the same data into <dir>, used to check that a reader built from the
//! unmodified code reads the v2 file and refuses the v3 file (see docs/format_codecs.md).
use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{KoreWriter, WriteOptions};

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("dir"));
    std::fs::create_dir_all(&dir).unwrap();
    let n = 200usize;
    let block = DataBlock { num_rows: n, columns: vec![
        Column { name: "id".into(), data: ColumnData::Str((0..n).map(|k| Some(format!("{:064x}", (k as u128) * 0x1234_5678_9abc_def1))).collect()) },
        Column { name: "ts".into(), data: ColumnData::Int64((0..n as i64).map(|k| Some(1_700_000_000_000 + k * 17)).collect()) },
        Column { name: "text".into(), data: ColumnData::Str((0..n).map(|k| Some(format!("event {k} happened; event {} happened", k / 2))).collect()) },
    ] };
    std::fs::write(dir.join("sample_v2.kore"), KoreWriter::to_bytes(&block)).unwrap();
    std::fs::write(dir.join("sample_v3.kore"), KoreWriter::to_bytes_with(&block, &WriteOptions::auto()).unwrap()).unwrap();
}
