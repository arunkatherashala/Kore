//! Writes the `tests/fixtures_dev/store` set with the kore-store of branch dev (d9edbcfc, before the format
//! sync; docs/format_sync.md). Every file is read back with the same code and must give the written values.
//! Usage: cargo run -p kore-store --example gen_dev_fixtures --offline -- <out_dir>
use kore_store::{rowgroups, versioned, CompactOptions, DictSet, KoreReader, KoreWriter, SharedDict};

#[path = "../tests/common/dev_data.rs"]
mod dev_data;
use dev_data::*;

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("out dir"));
    std::fs::create_dir_all(&out).unwrap();
    assert!(std::env::var("KORE_READABLE_MODE").is_err(), "run with the default writer settings");
    let put = |name: &str, bytes: Vec<u8>| std::fs::write(out.join(name), bytes).unwrap();
    let check = |name: &str, want: &kore_core::DataBlock, dicts: &DictSet| {
        let got = KoreReader::from_bytes_with_dicts(&std::fs::read(out.join(name)).unwrap(), dicts).unwrap();
        same_values(want, &got).unwrap_or_else(|e| panic!("{name}: dev does not round-trip: {e}"));
    };
    let none = DictSet::new();

    // default writer (KoreWriter::to_bytes): what aru_store, kore_rows, checkpoints, machine_protocol use
    let plain: Vec<(&str, kore_core::DataBlock)> = vec![
        ("all_types", all_types()), ("json_cols", json_cols()), ("empty_rows", empty_rows()),
        ("no_columns", no_columns()), ("all_null", all_null()), ("large", large()),
    ];
    for (n, b) in &plain {
        let f = format!("{n}.kore");
        put(&f, KoreWriter::to_bytes(b));
        check(&f, b, &none);
    }
    // KoreWriter::write_file (streaming path)
    KoreWriter::write_file(&out.join("all_types_wf.kore"), &all_types()).unwrap();
    check("all_types_wf.kore", &all_types(), &none);
    KoreWriter::write_file(&out.join("json_cols_wf.kore"), &json_cols()).unwrap();
    check("json_cols_wf.kore", &json_cols(), &none);

    // row groups: large groups, and 128-row groups as the index runs use
    put("large_rg2048.kore", rowgroups::write(&large(), 2048).unwrap());
    check("large_rg2048.kore", &large(), &none);
    put("all_types_rg128.kore", rowgroups::write(&all_types(), 128).unwrap());
    check("all_types_rg128.kore", &all_types(), &none);

    // kore-store version log
    let v1 = versioned::append_version(None, &all_types(), 10).unwrap();
    let v2 = versioned::append_version(Some(&v1), &empty_rows(), 20).unwrap();
    let v3 = versioned::append_version(Some(&v2), &json_cols(), 30).unwrap();
    put("versionlog.kore", v3);
    check("versionlog.kore", &json_cols(), &none);

    // version-4 compact (log format 4 segments), default options
    for (n, b) in [("v4_all_types", all_types()), ("v4_json_cols", json_cols()), ("v4_empty_rows", empty_rows()),
                   ("v4_all_null", all_null()), ("v4_large", large())] {
        let f = format!("{n}.kore");
        put(&f, KoreWriter::to_bytes_compact(&b, &CompactOptions::new()).unwrap());
        check(&f, &b, &none);
    }
    // version-4 with a shared dictionary on `text`
    let d = SharedDict::from_content(&dict_content()).unwrap();
    put(&format!("{}.kdic", d.id_hex()), d.to_bytes().to_vec());
    put("v4_dict.kore", KoreWriter::to_bytes_compact(&dict_texts(), &CompactOptions::new().with_dict("text", d.clone())).unwrap());
    let mut ds = DictSet::new();
    ds.add(d);
    check("v4_dict.kore", &dict_texts(), &ds);
    assert!(KoreReader::from_bytes(&std::fs::read(out.join("v4_dict.kore")).unwrap()).is_err(), "v4_dict.kore must reference the dictionary");
    println!("ok");
}
