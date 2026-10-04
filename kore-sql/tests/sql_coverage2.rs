//! Second layer of Spark SQL coverage tests: higher-order functions, MAP values, generators, recursive CTEs,
//! constant LIMIT expressions, DML, ranking column types. Expected values were derived by hand from Spark semantics.
//! Format as in sql_coverage.rs: rows `|`, cells `,`, NULL prints `NULL`, `ERR` = must be rejected, `~` = unordered.

use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::KqlContext;

fn s(v: &[Option<&str>]) -> Vec<Option<String>> { v.iter().map(|x| x.map(|y| y.to_string())).collect() }

fn base() -> KqlContext {
    let mut c = KqlContext::new();
    c.register("t", DataBlock::new(vec![
        Column::int64("id", (1..=6).map(Some).collect()),
        Column::str_col("g", s(&[Some("x"), Some("x"), Some("y"), Some("y"), Some("z"), None])),
        Column::int64("v", vec![Some(10), Some(20), Some(30), None, Some(50), Some(60)]),
        Column::float64("f", vec![Some(1.5), Some(2.5), Some(-3.5), None, Some(4.0), Some(0.0)]),
    ]).unwrap());
    c
}

fn render(b: &DataBlock) -> String {
    let n = b.columns.first().map(|c| c.data.len()).unwrap_or(0);
    let mut rows = vec![];
    for r in 0..n {
        let cells: Vec<String> = b.columns.iter().map(|c| match &c.data {
            ColumnData::Int64(v) => v[r].map(|x| x.to_string()).unwrap_or("NULL".into()),
            ColumnData::Float64(v) => v[r].map(|x| {
                let t = format!("{:.6}", x);
                let t = t.trim_end_matches('0').trim_end_matches('.').to_string();
                if t.is_empty() || t == "-0" { "0".into() } else { t }
            }).unwrap_or("NULL".into()),
            ColumnData::Bool(v) => v[r].map(|x| x.to_string()).unwrap_or("NULL".into()),
            ColumnData::Str(v) => v[r].clone().unwrap_or("NULL".into()),
            ColumnData::StrDict { codes, dict } => if codes[r] == u8::MAX { "NULL".into() } else { dict[codes[r] as usize].clone() },
        }).collect();
        rows.push(cells.join(","));
    }
    rows.join("|")
}

fn check_in(c: &KqlContext, cases: &[(&str, &str)]) {
    let mut failures = Vec::new();
    for (sql, exp) in cases {
        let act = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.query(sql))) {
            Ok(Ok(b)) => render(&b),
            Ok(Err(e)) => format!("ERR({e})"),
            Err(_) => "PANIC".to_string(),
        };
        let (sorted, exp) = match exp.strip_prefix('~') { Some(e) => (true, e), None => (false, *exp) };
        let norm = |x: &str| if sorted { let mut v: Vec<&str> = x.split('|').collect(); v.sort(); v.join("|") } else { x.to_string() };
        let ok = if exp == "ERR" { act.starts_with("ERR") } else { norm(&act) == norm(exp) };
        if !ok { failures.push(format!("{sql}\n    expected {exp}\n    actual   {act}")); }
    }
    assert!(failures.is_empty(), "{} of {} cases failed:\n{}", failures.len(), cases.len(), failures.join("\n"));
}

fn check(cases: &[(&str, &str)]) { check_in(&base(), cases) }

#[test]
fn higher_order_functions() {
    check(&[
        ("select transform(array(1, 2, 3), x -> x + 1)", "[2, 3, 4]"),
        ("select transform(array(1, 2, 3), (x, i) -> x * i)", "[0, 2, 6]"),
        ("select filter(array(1, 2, 3, 4), x -> x % 2 = 0)", "[2, 4]"),
        ("select exists(array(1, 2, 3), x -> x > 2), forall(array(1, 2, 3), x -> x > 0), exists(array(1, 2), x -> x > 5)", "true,true,false"),
        ("select exists(array(1, null), x -> x > 5), forall(array(1, null), x -> x > 0)", "NULL,NULL"),
        ("select aggregate(array(1, 2, 3), 0, (acc, x) -> acc + x), reduce(array(1, 2, 3), 1, (acc, x) -> acc * x)", "6,6"),
        ("select aggregate(array(1, 2, 3), 0, (acc, x) -> acc + x, acc -> acc * 10)", "60"),
        ("select zip_with(array(1, 2), array(3, 4), (a, b) -> a + b)", "[4, 6]"),
        ("select zip_with(array(1, 2, 3), array(10), (a, b) -> coalesce(b, 0) + a)", "[11, 2, 3]"),
        ("select array_sort(array(3, 1, 2), (a, b) -> case when a < b then 1 when a > b then -1 else 0 end)", "[3, 2, 1]"),
        ("select transform(array('a', 'b'), x -> upper(x))", "[A, B]"),
        ("select id, transform(array(1, 2), x -> x + id) from t where id < 3", "1,[2, 3]|2,[3, 4]"),
        ("select transform(array(array(1, 2), array(3)), a -> size(a))", "[2, 1]"),
        ("select array_join(transform(sequence(1, 3), x -> cast(x as string)), ',')", "1,2,3"),
        ("select count(*) from t where exists(array(1, 2), x -> x = id)", "2"),
        ("select transform(array(1), 5)", "ERR"),
        ("select array_sort(array(3, 1, 2))", "[1, 2, 3]"),
        // EXISTS (subquery) keeps working next to the function of the same name
        ("select count(*) from t where exists (select 1 from t t2 where t2.id = t.id + 5)", "1"),
    ]);
}

#[test]
fn map_values() {
    check(&[
        ("select map('a', 1, 'b', 2)", "{a -> 1, b -> 2}"),
        ("select map('a', 1)['a'], map('a', 1)['z']", "1,NULL"),
        ("select element_at(map('a', 1), 'a'), size(map('a', 1, 'b', 2))", "1,2"),
        ("select map_keys(map('a', 1)), map_values(map('a', 1))", "[a],[1]"),
        ("select map_from_arrays(array(1, 2), array('a', 'b'))", "{1 -> a, 2 -> b}"),
        ("select map_concat(map(1, 2), map(3, 4))", "{1 -> 2, 3 -> 4}"),
        ("select map_filter(map(1, 2, 3, 4), (k, v) -> v > 2)", "{3 -> 4}"),
        ("select transform_values(map(1, 2, 3, 4), (k, v) -> v * 10)", "{1 -> 20, 3 -> 40}"),
        ("select transform_keys(map(1, 2), (k, v) -> k + 1)", "{2 -> 2}"),
        ("select map_contains_key(map(1, 2), 1), map_contains_key(map(1, 2), 5)", "true,false"),
        ("select str_to_map('a:1,b:2'), str_to_map('a:1,b:2')['b']", "{a -> 1, b -> 2},2"),
        ("select map(1, array(1, 2)), array(map(1, 2))", "{1 -> [1, 2]},[{1 -> 2}]"),
        ("select explode(map('a', 1, 'b', 2))", "a,1|b,2"),
        ("select map('a', 1, 'a', 2)", "{a -> 2}"),
        ("select map('a')", "ERR"),
        ("select map(null, 1)", "ERR"),
        ("select to_json(array(1, 2)), to_json(map('a', 1))", "[1,2],{\"a\":1}"),
        ("select from_json('[1,2,3]', 'array<int>'), from_json('{\"a\":1}', 'map<string,int>')", "[1, 2, 3],{a -> 1}"),
        ("select from_json('{\"a\":1}', 'a int')", "ERR"),
    ]);
}

#[test]
fn generators() {
    check(&[
        ("select pos, e from t lateral view posexplode(array(10, 20)) q as pos, e where id = 1", "0,10|1,20"),
        ("select posexplode(array(10, 20, 30))", "0,10|1,20|2,30"),
        ("select id, e from t lateral view explode_outer(array()) q as e where id < 3", "1,NULL|2,NULL"),
        ("select id, e from t lateral view explode(array()) q as e where id < 3", ""),
        ("select kk, vv from t lateral view explode(map('a', 1, 'b', 2)) q as kk, vv where id = 1", "a,1|b,2"),
        ("select regexp_extract_all('a1b22c333', '([0-9]+)', 1)", "[1, 22, 333]"),
        ("select regexp_extract_all('a1b22c333', '([a-z])([0-9]+)', 2), regexp_extract_all('xyz', '[0-9]+')", "[1, 22, 333],[]"),
    ]);
}

#[test]
fn constant_limit_and_recursive_ctes() {
    check(&[
        ("select id from t order by id limit 1 + 1", "1|2"),
        ("select id from t order by id limit 2 offset 1 + 1", "3|4"),
        ("select id from t order by id limit id", "ERR"),
        ("select id from t order by id limit -1", "ERR"),
        ("with recursive r as (select 1 as n union all select n + 1 from r where n < 5) select * from r", "1|2|3|4|5"),
        ("with recursive r(n) as (select 1 union all select n + 1 from r where n < 5) select sum(n) from r", "15"),
        ("with recursive fib(a, b) as (select 0, 1 union all select b, a + b from fib where a < 50) select a from fib", "0|1|1|2|3|5|8|13|21|34|55"),
        ("with recursive t2 as (select 1 as n union select n + 1 from t2 where n < 3 union select 1) select * from t2", "~1|2|3"),
        ("with recursive tree(id, depth) as (select 1, 0 union all select id * 2, depth + 1 from tree where depth < 3) select id, depth from tree order by id", "1,0|2,1|4,2|8,3"),
        ("with recursive r as (select 1 as n union all select n + 1 from r) select * from r limit 3", "ERR"),
    ]);
}

#[test]
fn ranking_types_window_order_and_misc_semantics() {
    check(&[
        ("select typeof(row_number() over (order by id)), typeof(rank() over (order by id)), typeof(dense_rank() over (order by id)), typeof(ntile(2) over (order by id)) from t limit 1", "bigint,bigint,bigint,bigint"),
        ("select id, row_number() over (order by id), ntile(2) over (order by id) from t", "1,1,1|2,2,1|3,3,1|4,4,2|5,5,2|6,6,2"),
        // without an outer ORDER BY rows come out in window order (partition keys, then the window's ORDER BY)
        ("select g, id, row_number() over (partition by g order by id desc) from t", "NULL,6,1|x,2,1|x,1,2|y,4,1|y,3,2|z,5,1"),
        ("select id, sum(case when id > 3 then 1.5 else 1 end) over (order by id) from t", "1,1|2,2|3,3|4,4.5|5,6|6,7.5"),
        ("select grouping_id() from t group by rollup(g)", "~0|0|0|0|1"),
        ("select g, grouping_id(), sum(v) from t group by rollup(g) order by g", "NULL,0,60|NULL,1,170|x,0,30|y,0,30|z,0,50"),
        ("select g, id, sum(v) from t where id < 4 group by g, rollup(id) order by g, id", "x,NULL,30|x,1,10|x,2,20|y,NULL,30|y,3,30"),
        ("select date '2024-03-15' + 1, date '2024-03-15' - 1, 1 + date '2024-03-15'", "2024-03-16,2024-03-14,2024-03-16"),
        ("select date '2024-03-15' - date '2024-03-01'", "ERR"),
        ("select format_string('%e', 12345.678), format_string('%o', 8)", "1.234568e+04,10"),
        ("select format_number(0.5, 0), format_number(2.5, 0), format_number(1234.5, 0), format_number(1234567.891, 2)", "0,2,1,234,1,234,567.89"),
        ("select make_timestamp(2024, 3, 15, 10, 20, 30), unix_millis(timestamp '1970-01-01 00:00:01'), json_array_length('[1,2,3]')", "2024-03-15 10:20:30,1000,3"),
    ]);
}

fn dml(c: &mut KqlContext, sql: &str) -> (String, usize) { c.execute_dml(sql).unwrap_or_else(|e| panic!("{sql}: {e}")) }

#[test]
fn dml_and_ddl() {
    let mut c = base();
    assert_eq!(dml(&mut c, "create table w as select id, v, g from t").1, 6);
    // UPDATE changes exactly the matching rows (it used to leave the data untouched)
    assert_eq!(dml(&mut c, "update w set v = 99 where id = 2").1, 1);
    assert_eq!(dml(&mut c, "update w set v = v + 1, g = 'q' where id > 4").1, 2);
    check_in(&c, &[("select id, v, g from w order by id", "1,10,x|2,99,x|3,30,y|4,NULL,y|5,51,q|6,61,q")]);
    // a NULL predicate result keeps the row on DELETE (row 4 has v NULL)
    assert_eq!(dml(&mut c, "delete from w where v > 50").1, 3);
    check_in(&c, &[("select id from w order by id", "1|3|4")]);
    // doubled quotes inside string literals, multi-row VALUES, column lists
    dml(&mut c, "insert into w values (7, 70, 'it''s'), (8, null, null)");
    dml(&mut c, "insert into w (id, g) values (9, 'n')");
    check_in(&c, &[("select id, v, g from w where id > 6 order by id", "7,70,it's|8,NULL,NULL|9,NULL,n")]);
    assert!(c.execute_dml("insert into w values (10, 1.5, 'x')").is_err());
    assert!(c.execute_dml("insert into w values (10, 'abc', 'x')").is_err());
    assert!(c.execute_dml("insert into w values (10, 1)").is_err());
    // IF [NOT] EXISTS / OR REPLACE / TEMPORARY forms
    dml(&mut c, "create table if not exists w as select 1 as z");
    check_in(&c, &[("select count(*) from w", "6")]);
    dml(&mut c, "create or replace temporary view vw as select id from w where id > 7");
    check_in(&c, &[("select * from vw order by id", "8|9")]);
    dml(&mut c, "create table e (a int, b string)");
    dml(&mut c, "insert into e values (1, 'x')");
    check_in(&c, &[("select a, b from e", "1,x")]);
    dml(&mut c, "delete from e");
    check_in(&c, &[("select count(*) from e", "0")]);
    dml(&mut c, "insert into e values (2, 'y')");
    check_in(&c, &[("select a, b from e", "2,y")]);
    dml(&mut c, "drop view if exists vw");
    dml(&mut c, "drop table if exists w");
    dml(&mut c, "drop table if exists nosuch");
    assert!(c.query("select * from w").is_err());
    assert!(c.execute_dml("drop table nosuch").is_err());
}
