//! Row-group container read time (builds against a980727d and later). Usage: bench_rowgroups <rows> <group_rows> <reps>
//! Prints median [min-max] ms of a full read and of a 2-column read (no range pruning).
use kore_store::{rowgroups, KoreReader};
use std::time::Instant;

#[path = "support/events_gen.rs"]
mod events_gen;

fn main() {
    let a: Vec<usize> = std::env::args().skip(1).map(|x| x.parse().unwrap()).collect();
    let (rows, group, reps) = (a[0], a[1], a[2]);
    let bytes = rowgroups::write(&events_gen::events(rows, 3), group).unwrap();
    let t = |f: &dyn Fn() -> usize| {
        let mut v: Vec<f64> = (0..reps).map(|_| { let s = Instant::now(); assert_eq!(f(), rows); s.elapsed().as_secs_f64() * 1e3 }).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        format!("{:.2} [{:.2}-{:.2}]", v[v.len() / 2], v[0], v[v.len() - 1])
    };
    println!("rows={rows} group_rows={group} bytes={} full_ms={} two_cols_ms={}", bytes.len(),
        t(&|| KoreReader::from_bytes(&bytes).unwrap().num_rows),
        t(&|| rowgroups::read(&bytes, Some(&["kind", "text"]), None).unwrap().num_rows));
}
