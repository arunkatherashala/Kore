//! Usage: kore-compat-reader <dir>
//! Reads every file in <dir> with the kore_fileformat crate this binary was built against:
//!   read_*.kore    must read and match <dir>/expected.txt
//!   refuse_*.kore  must be refused with an error (Err, not a panic)
//!   info_*.kore    behaviour is printed only (known limitations of published versions)
//! Exit status 1 if any read/refuse expectation fails.
use kore_fileformat::{from_bytes, DataBlock};
use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

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
    let dir = std::env::args().nth(1).expect("usage: kore-compat-reader <dir>");
    let dir = std::path::Path::new(&dir);
    let expected: BTreeMap<String, String> = std::fs::read_to_string(dir.join("expected.txt")).unwrap()
        .lines().filter_map(|l| l.split_once('\t')).map(|(a, b)| (a.to_string(), b.to_string())).collect();
    std::panic::set_hook(Box::new(|_| {})); // panics are reported below, not printed twice
    let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".kore")).collect();
    names.sort();
    let (mut failed, mut checked) = (0, 0);
    for name in &names {
        let data = std::fs::read(dir.join(name)).unwrap();
        let r = catch_unwind(AssertUnwindSafe(|| from_bytes(&data)));
        let what = match &r {
            Ok(Ok(b)) => format!("read: {}", digest(b)),
            Ok(Err(e)) => format!("refused: {e}"),
            Err(_) => "PANIC".to_string(),
        };
        let ok = if name.starts_with("read_") {
            checked += 1;
            matches!(&r, Ok(Ok(b)) if expected.get(name.as_str()) == Some(&digest(b)))
        } else if name.starts_with("refuse_") {
            checked += 1;
            matches!(&r, Ok(Err(e)) if !e.is_empty())
        } else {
            println!("info {name}: {what}");
            continue;
        };
        println!("{} {name}: {what}", if ok { "ok  " } else { "FAIL" });
        if !ok { failed += 1; }
    }
    assert!(checked >= 2, "no read_/refuse_ cases found in {}", dir.display());
    if failed > 0 { eprintln!("{failed} compatibility check(s) failed"); std::process::exit(1); }
    println!("PASS ({checked} checks)");
}
