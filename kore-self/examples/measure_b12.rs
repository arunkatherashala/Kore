//! Track B12 measurements, learning to learn in World O (synthetic world, author-defined families; see
//! docs/measured.md, Track B12).
//!   cargo run -p kore-self --example measure_b12 --offline --release -- diag [--seeds 101..120] [--budget 1200000]
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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("diag") => report::cmd_diag(&args[1..]),
        Some("report") => full::cmd_report(&args[1..]),
        Some("v2") => report::cmd_v2(&args[1..]),
        Some("seqdebug") => report::cmd_seqdebug(&args[1..]),
        _ => eprintln!("usage: measure_b12 diag|report|v2 [--seeds a..b] [--budget N] [--window N] [--conds a,b]"),
    }
}
