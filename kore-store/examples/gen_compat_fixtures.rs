//! Writes the v2 compatibility fixtures in kore-store/tests/fixtures using ONLY the API that existed
//! before the codec work (KoreWriter::to_bytes, rowgroups::write, versioned::append_version).
//! The committed fixtures were produced by the unmodified writer (commit a980727d).
//! Usage: cargo run -p kore-store --example gen_compat_fixtures --offline -- <out_dir>
use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{rowgroups, versioned, KoreWriter};

#[path = "../tests/common/compat_data.rs"]
mod compat_data;

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("out dir"));
    std::fs::create_dir_all(&out).unwrap();
    // The default writer appends a readable text trailer (preview mode); also cover "none" via env.
    let mixed = compat_data::mixed();
    std::fs::write(out.join("mixed_default.kore"), KoreWriter::to_bytes(&mixed)).unwrap();
    std::env::set_var("KORE_READABLE_MODE", "none");
    std::fs::write(out.join("mixed_notrailer.kore"), KoreWriter::to_bytes(&mixed)).unwrap();
    std::fs::write(out.join("sorted_ints.kore"), KoreWriter::to_bytes(&compat_data::sorted_ints())).unwrap();
    std::fs::write(out.join("lowcard_strs.kore"), KoreWriter::to_bytes(&compat_data::lowcard_strs())).unwrap();
    std::fs::write(out.join("text.kore"), KoreWriter::to_bytes(&compat_data::text())).unwrap();
    std::fs::write(out.join("rowgroups.kore"), rowgroups::write(&compat_data::mixed(), 100).unwrap()).unwrap();
    let v1 = versioned::append_version(None, &compat_data::sorted_ints(), 10).unwrap();
    let v2 = versioned::append_version(Some(&v1), &compat_data::mixed(), 20).unwrap();
    std::fs::write(out.join("versionlog.kore"), v2).unwrap();
    let _ = (Column { name: String::new(), data: ColumnData::Bool(vec![]) }, DataBlock { columns: vec![], num_rows: 0 });
}
