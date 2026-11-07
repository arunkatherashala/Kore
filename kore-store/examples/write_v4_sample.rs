//! Writes the same data as a default (v2) file, a version-4 file without a dictionary and a version-4
//! file that references a shared dictionary (plus the `.kdic` object) into <dir>. Used to check that a
//! reader built from code before version 4 reads the v2 file and refuses both v4 files at the version
//! field (see docs/kore_size.md).
use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{CompactOptions, KoreWriter, SharedDict};

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("dir"));
    std::fs::create_dir_all(&dir).unwrap();
    let n = 200usize;
    let block = DataBlock { num_rows: n, columns: vec![
        Column { name: "id".into(), data: ColumnData::Str((0..n).map(|k| Some(format!("{:064x}", (k as u128) * 0x1234_5678_9abc_def1))).collect()) },
        Column { name: "at".into(), data: ColumnData::Str((0..n).map(|k| Some(format!("2026-01-01T00:{:02}:{:02}Z", k / 60, k % 60))).collect()) },
        Column { name: "text".into(), data: ColumnData::Str((0..n).map(|k| Some(format!("event {k} happened; event {} happened", k / 2))).collect()) },
    ] };
    let d = SharedDict::from_content(b"event happened; event happened; the quick brown fox").unwrap();
    std::fs::write(dir.join("sample_v2.kore"), KoreWriter::to_bytes(&block)).unwrap();
    std::fs::write(dir.join("sample_v4.kore"), KoreWriter::to_bytes_compact(&block, &CompactOptions::new()).unwrap()).unwrap();
    let o = CompactOptions::new().with_column("text", kore_store::ColumnCodec::new(kore_store::Transform::Plain, kore_store::Stage::Zstd)).with_dict("text", d.clone());
    std::fs::write(dir.join("sample_v4_dict.kore"), KoreWriter::to_bytes_compact(&block, &o).unwrap()).unwrap();
    std::fs::write(dir.join(d.file_name()), d.to_bytes()).unwrap();
}
