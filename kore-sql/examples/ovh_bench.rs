//! Fixed per-query overhead micro-benchmark. Run (after the timing window): 
//!   cargo run --release -p kore-sql --example ovh_bench -- [rows_lineitem]
//! Prints median/min microseconds per query (parse stage, full query) so before/after builds can be compared.
use kore_core::{Column, DataBlock};
use kore_sql::KqlContext;
use std::time::Instant;

fn stats(mut v: Vec<f64>) -> (f64, f64) { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); (v[v.len() / 2], v[0]) }

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(600_000);
    let mut ctx = KqlContext::new();
    ctx.register("nation", DataBlock::new(vec![
        Column::int64("n_nationkey", (0..25).map(Some).collect()),
        Column::str_col("n_name", (0..25).map(|i| Some(format!("NATION{i}"))).collect()),
        Column::int64("n_regionkey", (0..25).map(|i| Some(i % 5)).collect()),
    ]).unwrap());
    ctx.register("region", DataBlock::new(vec![
        Column::int64("r_regionkey", (0..5).map(Some).collect()),
        Column::str_col("r_name", (0..5).map(|i| Some(format!("REGION{i}"))).collect()),
    ]).unwrap());
    ctx.register("lineitem", DataBlock::new(vec![
        Column::int64("l_orderkey", (0..n as i64).map(|i| Some(i / 4)).collect()),
        Column::float64("l_extendedprice", (0..n).map(|i| Some((i % 1000) as f64 + 0.5)).collect()),
        Column::float64("l_discount", (0..n).map(|i| Some((i % 11) as f64 / 100.0)).collect()),
        Column::int64("l_quantity", (0..n).map(|i| Some((i % 50) as i64)).collect()),
        Column::int64("l_shipdate", (0..n).map(|i| Some(8000 + (i % 2500) as i64)).collect()),
    ]).unwrap());
    let qs = [
        ("select 1", "select 1 as one"),
        ("count nation", "select count(*) as c from nation"),
        ("select region", "select * from region"),
        ("group25", "select n_regionkey, count(*) as c, sum(n_nationkey) as s from nation group by n_regionkey"),
        ("q6like", "select sum(l_extendedprice * l_discount) as revenue from lineitem where l_discount between 0.05 and 0.07 and l_quantity < 24"),
        ("group-lineitem", "select l_quantity, sum(l_extendedprice) as s, count(*) as c from lineitem group by l_quantity order by l_quantity"),
    ];
    if std::env::var_os("OVH_NOCACHE").is_some() { kore_sql::testing::set_no_stmt_cache(true); }
    let iters = 300;
    println!("{:<16} {:>10} {:>10}  (microseconds, {} iters)", "query", "median", "min", iters);
    for (name, sql) in qs {
        for _ in 0..20 { let _ = ctx.query(sql); }
        let mut full = Vec::new();
        let mut parse = Vec::new();
        for _ in 0..iters {
            let t = Instant::now();
            let b = ctx.query(sql).unwrap();
            full.push(t.elapsed().as_secs_f64() * 1e6);
            let _ = b.num_rows;
            let t = Instant::now();
            let _ = kore_sql::parser::parse_query(sql).unwrap();
            parse.push(t.elapsed().as_secs_f64() * 1e6);
        }
        let (fm, fn_) = stats(full);
        let (pm, pn) = stats(parse);
        println!("{:<16} {:>10.1} {:>10.1}   parse-only {:>7.1} / {:>7.1}", name, fm, fn_, pm, pn);
    }
}
