//! Fixed-overhead work: the parsed-statement cache must never change a result (DML/DDL/register in between),
//! and the serial small-input paths must agree with the parallel ones.

use kore_core::{Column, DataBlock};
use kore_sql::{testing, KqlContext};
use std::sync::Mutex;

static SWITCHES: Mutex<()> = Mutex::new(());

fn base(n: usize) -> DataBlock {
    DataBlock::new(vec![
        Column::int64("k", (0..n).map(|i| if i % 11 == 0 { None } else { Some((i % 7) as i64) }).collect()),
        Column::int64("v", (0..n).map(|i| Some((i * 3 % 101) as i64)).collect()),
        Column::float64("f", (0..n).map(|i| if i % 13 == 0 { None } else { Some(i as f64 * 0.25) }).collect()),
        Column::str_col("s", (0..n).map(|i| if i % 5 == 0 { None } else { Some(format!("s{}", i % 4)) }).collect()),
    ]).unwrap()
}

fn render(r: Result<DataBlock, kore_core::KoreError>) -> String {
    match r {
        Err(e) => format!("ERR {e}"),
        Ok(b) => {
            let mut rows: Vec<String> = (0..b.num_rows)
                .map(|i| b.columns.iter().map(|c| format!("{:?}", c.data.get_value(i))).collect::<Vec<_>>().join("|"))
                .collect();
            rows.sort();
            format!("{:?}\n{}", b.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>(), rows.join("\n"))
        }
    }
}

/// Statement script: `Q:` queries, `D:` DML/DDL, `R:` re-register table `t` with a different size.
const SCRIPT: &[&str] = &[
    "Q:select count(*) as c from t",
    "Q:select k, sum(v) as sv, avg(f) as af from t group by k",
    "Q:select count(*) as c from t",
    "D:create table m (a int, b varchar)",
    "D:insert into m values (1, 'x'), (2, 'y')",
    "Q:select * from m",
    "D:insert into m values (3, 'z')",
    "Q:select * from m",
    "Q:select * from m",
    "D:update m set b = 'q' where a = 1",
    "Q:select * from m",
    "D:delete from m where a = 2",
    "Q:select * from m",
    "D:create view vw as select k, sum(v) as sv from t group by k",
    "Q:select * from vw",
    "R:50",
    "Q:select count(*) as c from t",
    "Q:select * from vw",
    "Q:select k, sum(v) as sv, avg(f) as af from t group by k",
    "D:drop table m",
    "Q:select * from m",
    "D:create table m (a int, b varchar)",
    "Q:select * from m",
    "D:insert into m select k, s from t",
    "Q:select count(*) as c from m",
    "D:create table ctas as select k, v from t where v > 10",
    "Q:select count(*) as c, sum(v) as s from ctas",
    "D:drop view vw",
    "Q:select * from vw",
    "R:5000",
    "Q:select s, count(*) as c from t group by s order by s",
    "Q:with x as (select k, v from t where v > 50) select k, count(*) as c from x group by k",
    "Q:select count(*) as c from t",
    "Q:select 1 as one",
    "Q:selec broken",
    "Q:select count(*) as c from t",
];

fn run_script() -> Vec<String> {
    let mut ctx = KqlContext::new();
    ctx.register("t", base(2000));
    let mut out = Vec::new();
    for st in SCRIPT {
        let (kind, body) = st.split_at(2);
        match kind {
            "Q:" => out.push(render(ctx.query(body))),
            "D:" => out.push(format!("{:?}", ctx.execute_dml(body).map_err(|e| e.to_string()))),
            _ => { ctx.register("t", base(body.parse().unwrap())); out.push("reg".into()); }
        }
    }
    out
}

#[test]
fn stmt_cache_matches_uncached_across_ddl_dml_register() {
    let _g = SWITCHES.lock().unwrap_or_else(|e| e.into_inner());
    testing::set_no_stmt_cache(true);
    let uncached = run_script();
    testing::set_no_stmt_cache(false);
    let cold = run_script();
    let warm = run_script(); // every statement is now a cache hit
    assert_eq!(uncached, cold);
    assert_eq!(uncached, warm);
    // INSERT then SELECT sees the new row
    let mut ctx = KqlContext::new();
    ctx.execute_dml("create table z (a int)").unwrap();
    assert_eq!(render(ctx.query("select count(*) as c from z")), render(ctx.query("select count(*) as c from z")));
    let before = render(ctx.query("select count(*) as c from z"));
    ctx.execute_dml("insert into z values (1)").unwrap();
    let after = render(ctx.query("select count(*) as c from z"));
    assert_ne!(before, after);
}

#[test]
fn serial_small_matches_parallel() {
    let _g = SWITCHES.lock().unwrap_or_else(|e| e.into_inner());
    let queries = [
        "select count(*) as c, sum(v) as sv, min(f) as mf from t where v > 20",
        "select k, s, count(*) as c, sum(v) as sv, avg(f) as af from t group by k, s",
        "select * from t where k = 3 and s = 's1'",
        "select k, count(distinct s) as d from t group by k order by k",
        "select a.k, count(*) as c from t a join t b on a.k = b.k where a.v < 30 group by a.k",
    ];
    for n in [0usize, 1, 7, 100, 1000, 7000, 9000] {
        let mut ctx = KqlContext::new();
        ctx.register("t", base(n));
        let mut res = Vec::new();
        for off in [true, false] {
            testing::set_no_serial_small(off);
            res.push(queries.iter().map(|q| render(ctx.query(q))).collect::<Vec<_>>());
        }
        testing::set_no_serial_small(false);
        assert_eq!(res[0], res[1], "n={n}");
    }
}
