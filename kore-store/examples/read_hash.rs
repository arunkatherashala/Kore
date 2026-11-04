//! Cross-store read check (builds against the old store, a980727d and later): reads every *.kore file in
//! <dir> with `KoreReader::read_file` and prints one content hash per file and a total, or the error.
//! Values are hashed through `ColumnData::get_value`, so plain and dictionary strings hash alike.
use kore_store::KoreReader;
use std::hash::{Hash, Hasher};

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("dir"));
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map_or(false, |x| x == "kore")).collect();
    files.sort();
    let mut total = std::collections::hash_map::DefaultHasher::new();
    let (mut ok, mut err) = (0, 0);
    for p in &files {
        match KoreReader::read_file(p) {
            Ok(b) => {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                b.num_rows.hash(&mut h);
                for c in &b.columns {
                    c.name.hash(&mut h);
                    for r in 0..b.num_rows { format!("{:?}", c.data.get_value(r)).hash(&mut h); }
                }
                let v = h.finish();
                v.hash(&mut total);
                ok += 1;
                println!("{} rows={} hash={v:016x}", p.file_name().unwrap().to_string_lossy(), b.num_rows);
            }
            Err(e) => { err += 1; println!("{} ERROR {e}", p.file_name().unwrap().to_string_lossy()); }
        }
    }
    println!("files={} ok={ok} err={err} total={:016x}", files.len(), total.finish());
}
