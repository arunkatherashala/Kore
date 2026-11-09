//! Track L2 measurements, learning to learn across families (synthetic worlds, author-defined families; see
//! docs/measured.md, Track L2).
//!   cargo run -p kore-self --example measure_l2 --offline --release -- diag [--seeds 101..120] [--budget 1200000]
//!   cargo run -p kore-self --example measure_l2 --offline --release -- report [--seeds 1..20]
//! Run data is printed and never committed.

#![allow(dead_code)]
#[path = "../src/versioned/mod.rs"]
mod versioned;
#[path = "../src/being/mod.rs"]
mod being;
#[path = "measure_b11_parts/worldo.rs"]
mod worldo;
#[path = "measure_b12_parts/report.rs"]
mod report;
#[path = "measure_b12_parts/full.rs"]
mod full;
#[path = "measure_l2_parts/diag.rs"]
mod diag;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("diag") => diag::cmd_diag(&args[1..]),
        _ => eprintln!("usage: measure_l2 diag|report [--seeds a..b] [--budget N] [--conds a,b]"),
    }
}
