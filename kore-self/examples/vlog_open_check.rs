//! Opens and verifies an existing versioned log and prints the outcome (one line). Used to check by
//! hand that a binary built from older code refuses a newer log format instead of misreading it:
//!
//!   vlog_open_check ROOT OWNER
#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;

use std::path::PathBuf;
use versioned::*;

fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("ROOT"));
    let owner = std::env::args().nth(2).expect("OWNER");
    match VersionedLog::open(&root, &owner) {
        Err(e) => println!("open: ERR {e}"),
        Ok(l) => {
            let v = l.verify();
            let n = l.try_open_latest().map(|s| s.raw(Table::Events).len());
            println!("open: OK format={} latest={} verify={:?} events={:?}", l.format().number(), l.latest_version(), v.map_err(|e| e.to_string()), n.map_err(|e| e.to_string()));
        }
    }
}
