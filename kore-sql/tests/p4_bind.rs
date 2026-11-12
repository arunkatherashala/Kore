//! Column binding: a reference to a column that does not exist is an error (it used to evaluate to NULL silently),
//! every valid shape keeps working with identical results, and the benchmark corpora (TPC-H, TPC-DS) bind cleanly
//! against their real schemas.

use kore_core::{Column, DataBlock};
use kore_sql::{testing, KqlContext};
use std::sync::{Mutex, MutexGuard};

/// The bind switch is process-wide: tests that flip it must not overlap with the others.
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    testing::set_no_bind_check(false);
    g
}

fn ctx() -> KqlContext {
    let mut c = KqlContext::new();
    c.register("t", DataBlock::new(vec![
        Column::int64("k", vec![Some(1), Some(2), Some(2), Some(3), None]),
        Column::int64("v", vec![Some(10), Some(20), Some(30), Some(40), Some(50)]),
        Column::str_col("s", vec![Some("a".into()), Some("b".into()), Some("b".into()), None, Some("c".into())]),
        Column::float64("f", vec![Some(1.5), Some(2.5), None, Some(4.5), Some(5.5)]),
    ]).unwrap());
    c.register("u", DataBlock::new(vec![
        Column::int64("k", vec![Some(1), Some(2), Some(4)]),
        Column::int64("w", vec![Some(100), Some(200), Some(400)]),
    ]).unwrap());
    c.register("Mixed", DataBlock::new(vec![
        Column::int64("CamelCol", vec![Some(1), Some(2)]),
    ]).unwrap());
    c
}

fn rows(b: &DataBlock) -> Vec<String> {
    (0..b.num_rows)
        .map(|i| b.columns.iter().map(|c| format!("{:?}", c.data.get_value(i))).collect::<Vec<_>>().join("|"))
        .collect()
}

/// The query must fail with `column not found` naming `name`.
fn bad(c: &KqlContext, sql: &str, name: &str) {
    let e = match c.query(sql) { Ok(_) => panic!("expected an error for: {sql}"), Err(e) => e.to_string() };
    assert!(e.contains("column not found") && e.contains(name), "wrong error for `{sql}`: {e}");
}

/// The query must run, and give the same answer with the check on and off.
fn good(c: &KqlContext, sql: &str) {
    let on = c.query(sql).unwrap_or_else(|e| panic!("valid query rejected: {sql}\n  {e}"));
    testing::set_no_bind_check(true);
    let off = c.query(sql);
    testing::set_no_bind_check(false);
    let off = off.unwrap_or_else(|e| panic!("check-off run failed: {sql}\n  {e}"));
    assert_eq!(rows(&on), rows(&off), "result changed by the bind check: {sql}");
    assert_eq!(on.columns.iter().map(|x| x.name.clone()).collect::<Vec<_>>(), off.columns.iter().map(|x| x.name.clone()).collect::<Vec<_>>());
}

#[test]
fn unresolved_columns_are_errors() {
    let _g = lock();
    let c = ctx();
    // select list
    bad(&c, "select nosuch + 1 from t", "nosuch");
    bad(&c, "select k, nosuch from t", "nosuch");
    bad(&c, "select upper(nosuch) from t", "nosuch");
    bad(&c, "select case when nosuch > 1 then 1 else 0 end from t", "nosuch");
    bad(&c, "select nosuch", "nosuch");
    // WHERE
    bad(&c, "select * from t where nosuch = 1", "nosuch");
    bad(&c, "select k from t where k = 1 and (v > 2 or nosuch is null)", "nosuch");
    bad(&c, "select k from t where nosuch in (1, 2)", "nosuch");
    bad(&c, "select k from t where nosuch between 1 and 2", "nosuch");
    bad(&c, "select k from t where nosuch like 'a%'", "nosuch");
    // aggregate argument
    bad(&c, "select count(nosuch) from t", "nosuch");
    bad(&c, "select sum(nosuch) from t group by k", "nosuch");
    bad(&c, "select k, count(distinct nosuch) from t group by k", "nosuch");
    bad(&c, "select sum(v) filter (where nosuch > 1) from t", "nosuch");
    // GROUP BY / HAVING / ORDER BY
    bad(&c, "select count(*) from t group by nosuch", "nosuch");
    bad(&c, "select k, count(*) from t group by k having nosuch > 1", "nosuch");
    bad(&c, "select k, count(*) from t group by k having sum(nosuch) > 1", "nosuch");
    bad(&c, "select k from t order by nosuch", "nosuch");
    bad(&c, "select k, v from t order by k, nosuch desc", "nosuch");
    bad(&c, "select k from t group by k order by nosuch", "nosuch");
    bad(&c, "select k from t group by rollup(k, nosuch)", "nosuch");
    bad(&c, "select k, sum(v) from t group by k, nosuch + 1", "nosuch");
    // JOIN
    bad(&c, "select * from t a join u b on a.nosuch = b.k", "nosuch");
    bad(&c, "select * from t a join u b on a.k = b.nosuch", "nosuch");
    bad(&c, "select * from t a left join u b on a.k = b.k and b.nosuch > 1", "nosuch");
    bad(&c, "select a.k from t a, u b where a.k = b.nosuch", "nosuch");
    bad(&c, "select * from t join u using (nosuch)", "nosuch");
    // qualifier typo
    bad(&c, "select tt.k from t", "tt.k");
    bad(&c, "select * from t where tt.k = 1", "tt.k");
    bad(&c, "select t.nosuch from t", "t.nosuch");
    bad(&c, "select a.k from t a, u b where aa.k = b.k", "aa.k");
    bad(&c, "select tt.* from t", "tt");
    // windows
    bad(&c, "select sum(v) over (partition by nosuch) from t", "nosuch");
    bad(&c, "select row_number() over (order by nosuch) from t", "nosuch");
    bad(&c, "select sum(nosuch) over (partition by k) from t", "nosuch");
    bad(&c, "select lag(nosuch) over (order by k) from t", "nosuch");
    bad(&c, "select sum(v) over w from t window w as (partition by nosuch)", "nosuch");
    // subqueries
    bad(&c, "select (select max(nosuch) from u) from t", "nosuch");
    bad(&c, "select * from t where k in (select nosuch from u)", "nosuch");
    bad(&c, "select * from t where nosuch in (select k from u)", "nosuch");
    bad(&c, "select * from t where exists (select 1 from u where u.k = t.nosuch)", "nosuch");
    bad(&c, "select * from t where exists (select 1 from u where u.nosuch = t.k)", "nosuch");
    bad(&c, "select * from t where v > (select avg(w) from u where w > nosuch)", "nosuch");
    bad(&c, "select * from (select k from t) d where nosuch = 1", "nosuch");
    bad(&c, "select d.v from (select k from t) d", "d.v");
    bad(&c, "select * from t where k > all (select nosuch from u)", "nosuch");
    // CTE bodies and references
    bad(&c, "with x as (select nosuch from t) select * from x", "nosuch");
    bad(&c, "with x as (select k from t) select nosuch from x", "nosuch");
    bad(&c, "with x as (select k from t) select v from x", "v");
    bad(&c, "with x as (select k from t), y as (select k, nosuch from x) select * from y", "nosuch");
    // set operations
    bad(&c, "select k from t union all select nosuch from u", "nosuch");
    bad(&c, "select k from t union select k from u order by nosuch", "nosuch");
    // lambdas do not hide a missing column
    bad(&c, "select transform(array(1,2), x -> x + nosuch) from t", "nosuch");
    // view
    let mut c2 = ctx();
    c2.create_view("vw", "select k, v from t");
    bad(&c2, "select nosuch from vw", "nosuch");
    bad(&c2, "select w from vw", "w");
    // message names the candidates
    let e = c.query("select nosuch from t").unwrap_err().to_string();
    assert!(e.contains("column not found: nosuch"), "{e}");
    assert!(e.contains("t.k") && e.contains("t.v"), "candidates missing: {e}");
}

#[test]
fn dml_columns_are_checked() {
    let _g = lock();
    let mut c = KqlContext::new();
    c.register_mut("m", DataBlock::new(vec![
        Column::int64("k", vec![Some(1), Some(2)]),
        Column::int64("v", vec![Some(10), Some(20)]),
    ]).unwrap());
    let e = c.execute_dml("update m set v = nosuch + 1").unwrap_err().to_string();
    assert!(e.contains("column not found") && e.contains("nosuch"), "{e}");
    let e = c.execute_dml("update m set v = 1 where nosuch = 2").unwrap_err().to_string();
    assert!(e.contains("column not found") && e.contains("nosuch"), "{e}");
    let e = c.execute_dml("update m set nosuch = 1").unwrap_err().to_string();
    assert!(e.contains("nosuch"), "{e}");
    let e = c.execute_dml("delete from m where nosuch = 1").unwrap_err().to_string();
    assert!(e.contains("column not found") && e.contains("nosuch"), "{e}");
    let e = c.execute_dml("insert into m (k, nosuch) values (1, 2)").unwrap_err().to_string();
    assert!(e.contains("nosuch"), "{e}");
    let e = c.execute_dml("insert into m select nosuch, 1 from m").unwrap_err().to_string();
    assert!(e.contains("column not found") && e.contains("nosuch"), "{e}");
    // valid statements still work and nothing was modified by the failed ones
    assert_eq!(c.execute_dml("update m set v = v + 1 where k = 1").unwrap().1, 1);
    assert_eq!(c.execute_dml("delete from m where k = 2").unwrap().1, 1);
    assert_eq!(c.query("select v from m").unwrap().num_rows, 1);
}

#[test]
fn lenient_shapes_still_work() {
    let _g = lock();
    let mut c = ctx();
    c.create_view("vw", "select k as kk, v * 2 as vv from t");
    let sqls = [
        // select aliases in ORDER BY / GROUP BY / HAVING, lateral alias
        "select k as kk, v as vv from t order by kk",
        "select k as kk, count(*) as n from t group by kk order by n desc, kk",
        "select k as kk, sum(v) as sv from t group by k having sv > 10 order by sv",
        "select v + 1 as a, a * 2 as b from t order by b",
        "select upper(s) as u, count(*) as n from t group by u order by u",
        // star forms
        "select t.* from t",
        "select t.*, u.w from t join u on t.k = u.k",
        "select a.*, b.w from t a join u b on a.k = b.k",
        "select * from t",
        "select *, v + 1 as x from t",
        "select v * 10 as x, * from t",
        // ordinals
        "select k, v from t order by 2 desc",
        "select k, sum(v) from t group by 1 order by 1",
        // correlated subqueries
        "select 1 as x from t where exists (select 1 from u where u.k = t.k)",
        "select k from t where not exists (select 1 from u where u.k = t.k)",
        "select k, (select max(w) from u where u.k = t.k) as m from t",
        "select k from t where v > (select avg(w) from u where u.k = t.k)",
        "select k from t where k in (select k from u where u.w > t.v)",
        "select k from t a where exists (select 1 from u b where b.k = a.k and b.w > a.v)",
        // case-insensitive names
        "select K, V from T where S = 'b' order by K",
        "select camelcol, CAMELCOL, MIXED.camelcol from mixed",
        "select t.K from t",
        "select A.k from t a",
        // count(*), literals, special words, no FROM
        "select count(*) from t",
        "select count(1), count(distinct k) from t",
        "select 1 + 1",
        "select current_date is not null as d, current_timestamp is not null as ts",
        "select true as a, false as b, null as c",
        "select cast(v as double) as d, cast(k as string) as s from t",
        "select date_add('2024-01-01', 3), dateadd(day, 3, '2024-01-01')",
        "select '2024-01-01' + interval 1 day",
        "select extract(year from '2024-03-04') as y",
        // derived tables, VALUES, CTEs, recursive
        "select d.k from (select k, v from t) d where d.v > 10",
        "select x from (select k as x from t) d",
        "select * from (select k, v from t) d(a, b) where a = 1",
        "select a, b from (values (1, 'x'), (2, 'y')) as q(a, b)",
        "select col1, col2 from (values (1, 'x'), (2, 'y')) as q",
        "select id from range(5)",
        "with x as (select k, v from t), y as (select k, v + 1 as v1 from x) select y.k, x.v, y.v1 from x join y on x.k = y.k",
        "with recursive r(n) as (select 1 union all select n + 1 from r where n < 5) select n from r",
        "with c(a, b) as (select k, v from t) select a, b from c",
        "select * from (select k, count(*) from t group by k) d",
        "select \"count(1)\" from (select count(1) from t) d",
        // joins
        "select a.k, b.w from t a join u b on a.k = b.k",
        "select * from t join u using (k)",
        "select k, w from t join u using (k)",
        "select * from t natural join u",
        "select a.k from t a left join u b on a.k = b.k where b.w is null",
        "select a.k from t a cross join u b",
        "select t.k, u.w from t, u where t.k = u.k",
        "select k from t left semi join u on t.k = u.k",
        "select k from t left anti join u on t.k = u.k",
        // windows
        "select k, sum(v) over (partition by k order by v) as r from t",
        "select k, row_number() over (order by v desc) as rn from t",
        "select k, sum(v) over w as r from t window w as (partition by k order by v)",
        "select k, v, lag(v) over (order by v) as p from t qualify row_number() over (order by v) < 3",
        // set operations
        "select k from t union all select k from u order by k",
        "select k as x from t union select k from u order by x",
        "select k, v from t union all select k, w from u order by 1, 2",
        // higher-order functions
        "select transform(array(1, 2, 3), x -> x + 1) as a",
        "select transform(array(1, 2, 3), x -> x + k) from t",
        "select filter(array(1, 2, 3), (x) -> x > 1) as a",
        "select aggregate(array(1, 2, 3), 0, (acc, x) -> acc + x) as a",
        "select zip_with(array(1, 2), array(3, 4), (x, y) -> x + y) as a",
        // struct field access, generators
        "select s.k, s.v from (select struct(k, v) as s from t) d",
        "select d.s.k from (select struct(k, v) as s from t) d",
        "select p.col from (select explode(array(1, 2)) as col) p",
        "select k, e from t lateral view explode(array(1, 2)) x as e",
        "select k, x.e from t lateral view explode(array(1, 2)) x as e",
        "select k, p, e from t lateral view posexplode(array(1, 2)) x as p, e",
        // pivot-ish and misc
        "select distinct k from t order by k",
        "select k from t where s is not null and v between 10 and 40 order by v limit 2",
        "select grouping(k) as g, k, sum(v) from t group by rollup(k)",
        "select k, sum(v) from t group by cube(k) order by 1",
        "select k, case when v > 20 then 'hi' else 'lo' end as c from t",
        "select coalesce(s, 'z') as c from t order by c",
        "select count(*) filter (where v > 10) as c from t",
        "select k, collect_list(v) as l from t group by k",
        "select k from t where k = (select min(k) from u)",
        "select * from t where k > any (select k from u)",
        "select t.k from t tablesample (50 percent) where 1 = 0",
        "select v from t where (select count(*) from u) > 1 order by v",
        "select sum(v) as total from t having sum(v) > 0",
        "select k, sum(v) as sv from t group by k order by sum(v) desc",
        "select s, count(*) from t group by s order by count(*) desc, s",
        "select k from vw_missing_is_a_different_error_not_a_column_error where 1 = 0",
        "select kk, vv from vw where vv > 20",
        "select vw.kk from vw",
        "select * from vw",
    ];
    let mut unsupported: Vec<String> = Vec::new();
    for sql in sqls {
        if sql.contains("vw_missing") {
            // an unknown table is still the old error, never a column error
            let e = c.query(sql).unwrap_err().to_string();
            assert!(!e.contains("column not found"), "{e}");
            continue;
        }
        match c.query(sql) {
            Ok(_) => good(&c, sql),
            Err(e) => {
                unsupported.push(format!("{sql} => {e}"));
                assert!(!e.to_string().contains("column not found"), "valid query rejected by the bind check: {sql}\n  {e}");
            }
        }
    }
    for u in &unsupported { eprintln!("engine-unsupported (not a bind error): {u}"); }
    assert!(unsupported.len() <= 6, "too many queries failed for other reasons: {unsupported:?}");
}

#[test]
fn switch_restores_old_behaviour() {
    let _g = lock();
    let c = ctx();
    assert!(c.query("select nosuch + 1 from t").is_err());
    testing::set_no_bind_check(true);
    let r = c.query("select nosuch + 1 as x from t").unwrap();
    testing::set_no_bind_check(false);
    assert_eq!(r.num_rows, 5);
    assert!(rows(&r).iter().all(|x| x == "Null"));
}

fn tpch_schema() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("region", vec!["r_regionkey", "r_name", "r_comment"]),
        ("nation", vec!["n_nationkey", "n_name", "n_regionkey", "n_comment"]),
        ("supplier", vec!["s_suppkey", "s_name", "s_address", "s_nationkey", "s_phone", "s_acctbal", "s_comment"]),
        ("customer", vec!["c_custkey", "c_name", "c_address", "c_nationkey", "c_phone", "c_acctbal", "c_mktsegment", "c_comment"]),
        ("part", vec!["p_partkey", "p_name", "p_mfgr", "p_brand", "p_type", "p_size", "p_container", "p_retailprice", "p_comment"]),
        ("partsupp", vec!["ps_partkey", "ps_suppkey", "ps_availqty", "ps_supplycost", "ps_comment"]),
        ("orders", vec!["o_orderkey", "o_custkey", "o_orderstatus", "o_totalprice", "o_orderdate", "o_orderpriority", "o_clerk", "o_shippriority", "o_comment"]),
        ("lineitem", vec!["l_orderkey", "l_partkey", "l_suppkey", "l_linenumber", "l_quantity", "l_extendedprice", "l_discount", "l_tax",
            "l_returnflag", "l_linestatus", "l_shipdate", "l_commitdate", "l_receiptdate", "l_shipinstruct", "l_shipmode", "l_comment"]),
    ]
}

fn empty_ctx(schema: &[(String, Vec<String>)]) -> KqlContext {
    let mut c = KqlContext::new();
    for (t, cols) in schema {
        let columns = cols.iter().map(|n| Column::int64(n, vec![])).collect();
        c.register(t.clone(), DataBlock::new(columns).unwrap());
    }
    c
}

fn fixture(name: &str) -> serde_json::Value {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(name);
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

/// Every query of a corpus must bind against the schema, and must stop binding when a column is misspelled.
fn corpus(queries: &serde_json::Value, schema: &[(String, Vec<String>)]) -> usize {
    let c = empty_ctx(schema);
    let mut n = 0;
    for (name, q) in queries.as_object().unwrap() {
        let sql = q.as_str().unwrap();
        if let Err(e) = testing::bind_check(sql, &c) {
            panic!("{name}: valid benchmark query rejected: {e}\n{sql}");
        }
        n += 1;
    }
    n
}

#[test]
fn tpch_corpus_binds() {
    let _g = lock();
    let schema: Vec<(String, Vec<String>)> = tpch_schema().into_iter().map(|(t, c)| (t.to_string(), c.into_iter().map(String::from).collect())).collect();
    let n = corpus(&fixture("fixtures_tpch_queries.json"), &schema);
    assert_eq!(n, 22);
    // sanity: dropping one column from the schema makes some query fail (the check really looks at the names)
    let mut broken = schema.clone();
    broken.iter_mut().find(|(t, _)| t == "lineitem").unwrap().1.retain(|c| c != "l_shipdate");
    let c = empty_ctx(&broken);
    let q = fixture("fixtures_tpch_queries.json");
    let failures = q.as_object().unwrap().values().filter(|s| testing::bind_check(s.as_str().unwrap(), &c).is_err()).count();
    assert!(failures >= 3, "only {failures} queries notice a missing l_shipdate");
}

#[test]
fn tpcds_corpus_binds() {
    let _g = lock();
    let sch = fixture("fixtures_tpcds_schema.json");
    let schema: Vec<(String, Vec<String>)> = sch.as_object().unwrap().iter()
        .map(|(t, cols)| (t.clone(), cols.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect())).collect();
    assert_eq!(schema.len(), 24);
    // the 99 queries live next to the benchmark runner
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("benchmarks").join("tpcds_honest").join("queries.json");
    let q: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    assert_eq!(corpus(&q, &schema), 99);
    // a dropped column is noticed
    let mut broken = schema.clone();
    broken.iter_mut().find(|(t, _)| t == "store_sales").unwrap().1.retain(|c| c != "ss_net_profit");
    let c = empty_ctx(&broken);
    let failures = q.as_object().unwrap().values().filter(|s| testing::bind_check(s.as_str().unwrap(), &c).is_err()).count();
    assert!(failures >= 3, "only {failures} queries notice a missing ss_net_profit");
}

#[test]
fn check_costs_microseconds() {
    let _g = lock();
    let sch = fixture("fixtures_tpcds_schema.json");
    let schema: Vec<(String, Vec<String>)> = sch.as_object().unwrap().iter()
        .map(|(t, cols)| (t.clone(), cols.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect())).collect();
    let c = empty_ctx(&schema);
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("benchmarks").join("tpcds_honest").join("queries.json");
    let q: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let parsed: Vec<String> = q.as_object().unwrap().values().map(|s| s.as_str().unwrap().to_string()).collect();
    let t0 = std::time::Instant::now();
    for _ in 0..5 { for s in &parsed { testing::bind_check(s, &c).unwrap(); } }
    // includes the parse of each query; the check alone is a small fraction of it
    let per = t0.elapsed().as_micros() as f64 / (5.0 * parsed.len() as f64);
    eprintln!("parse + bind check: {per:.0} us per TPC-DS query");
    let t1 = std::time::Instant::now();
    for _ in 0..5 { for s in &parsed { kore_sql::parse_query(s.trim()).unwrap(); } }
    let parse_only = t1.elapsed().as_micros() as f64 / (5.0 * parsed.len() as f64);
    eprintln!("parse only: {parse_only:.0} us; bind check alone: about {:.0} us per TPC-DS query", per - parse_only);
    assert!(per < 5_000.0, "binding is too slow: {per} us per query");
}
