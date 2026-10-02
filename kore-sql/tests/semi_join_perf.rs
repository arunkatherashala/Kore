//! Scale check for EXISTS / NOT EXISTS: these must run as hash semi/anti joins, not per-row subqueries.

use kore_core::{Column, DataBlock};
use kore_sql::KqlContext;
use std::time::Instant;

fn big_ctx(orders: usize, lines_per_order: usize) -> KqlContext {
    let mut c = KqlContext::new();
    c.register("orders", DataBlock::new(vec![
        Column::int64("o_orderkey", (1..=orders as i64).map(Some).collect()),
        Column::int64("o_custkey", (1..=orders as i64).map(|i| Some(i % 1000 + 1)).collect()),
    ]).unwrap());
    let n = orders * lines_per_order;
    c.register("lineitem", DataBlock::new(vec![
        Column::int64("l_orderkey", (0..n as i64).map(|i| Some(i / lines_per_order as i64 + 1)).collect()),
        Column::int64("l_commit", (0..n as i64).map(|i| Some(i % 7)).collect()),
        Column::int64("l_receipt", (0..n as i64).map(|i| Some(i % 5)).collect()),
    ]).unwrap());
    c.register("customer", DataBlock::new(vec![
        Column::int64("c_custkey", (1..=1500).map(Some).collect()),
    ]).unwrap());
    c
}

#[test]
fn exists_scales_linearly() {
    let c = big_ctx(150_000, 4);
    let t = Instant::now();
    let r = c.query("select count(*) as n from orders where exists (select * from lineitem where l_orderkey = o_orderkey and l_commit < l_receipt)").unwrap();
    let el = t.elapsed();
    println!("EXISTS over 150k x 600k took {el:?}, result {:?}", r.columns[0].data.get_value(0));
    assert!(el.as_secs_f64() < 10.0, "EXISTS took {el:?}: not running as a semi-join");

    let t = Instant::now();
    let r = c.query("select count(*) as n from customer where not exists (select * from orders where o_custkey = c_custkey)").unwrap();
    let el = t.elapsed();
    println!("NOT EXISTS took {el:?}, result {:?}", r.columns[0].data.get_value(0));
    assert!(el.as_secs_f64() < 5.0, "NOT EXISTS took {el:?}");
}
