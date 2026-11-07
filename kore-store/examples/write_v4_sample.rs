//! Writes the same data as a default (v2) file, a version-4 file without a dictionary and a version-4
//! file that references a shared dictionary (plus the `.kdic` object) into <dir>. Used to check that a
//! reader built from code before version 4 reads the v2 file and refuses both v4 files at the version
//! field (see docs/kore_size.md). Also a key index object (`sample_index.kidx`) and a version-4 file written
//! with `CompactOptions::fast()`, for the same check (docs/kore_speed_v4.md).
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
    // key index object over the id column of the v2 and v4 files, and a v4 file written with the fast preset
    let v2 = KoreWriter::to_bytes(&block);
    let v4 = KoreWriter::to_bytes_compact(&block, &CompactOptions::new()).unwrap();
    let mut ib = kore_store::KeyIndexBuilder::new("id");
    ib.add_file_bytes("sample_v2.kore", &v2, &kore_store::DictSet::new()).unwrap();
    ib.add_file_bytes("sample_v4.kore", &v4, &kore_store::DictSet::new()).unwrap();
    let ix = ib.build().unwrap();
    std::fs::write(dir.join("sample_index.kidx"), ix.to_bytes()).unwrap();
    std::fs::write(dir.join("sample_v4_fast.kore"), KoreWriter::to_bytes_compact(&block, &CompactOptions::fast()).unwrap()).unwrap();
}
