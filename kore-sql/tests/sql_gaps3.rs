//! Third layer of Spark SQL coverage tests (sql-gaps-3): DATE/TIMESTAMP comparison semantics, DECIMAL display,
//! STRUCT, and assorted syntax. Expected values were derived by hand from Spark 3.1 semantics.
//! Format as in sql_coverage2.rs: rows `|`, cells `,`, NULL prints `NULL`, `ERR` = must be rejected, `~` = unordered.

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
fn date_timestamp_comparison() {
    check(&[
        ("select date '2024-03-15' = timestamp '2024-03-15 00:00:00'", "true"),
        ("select timestamp '2024-03-15 00:00:00' = date '2024-03-15'", "true"),
        ("select date '2024-03-15' < timestamp '2024-03-15 00:00:01'", "true"),
        ("select date '2024-03-16' > timestamp '2024-03-15 23:59:59'", "true"),
        ("select date '2024-03-15' <> timestamp '2024-03-15 10:00:00'", "true"),
        ("select timestamp '2024-03-15 10:00:00' > '2024-03-15'", "true"),
        ("select '2024-03-15' = date '2024-03-15'", "true"),
        ("select date '2024-03-15' >= '2024-03-15 00:00:00'", "true"),
        ("select timestamp '2024-03-15 10:00:00.5' > timestamp '2024-03-15 10:00:00'", "true"),
        ("select timestamp '2024-03-15 10:00:00' between date '2024-03-15' and date '2024-03-16'", "true"),
        ("select date '2024-03-15' in (timestamp '2024-03-15 00:00:00', timestamp '2025-01-01 00:00:00')", "true"),
        ("select cast(date '2024-03-15' as timestamp)", "2024-03-15 00:00:00"),
        ("select cast(timestamp '2024-03-15 10:20:30' as date)", "2024-03-15"),
        ("select cast('2024-03-15' as timestamp)", "2024-03-15 00:00:00"),
        ("select to_date('2024-03-15 10:20:30')", "2024-03-15"),
        ("select date '2024-03-15' - date '2024-03-01'", "ERR"),
        ("select date '2024-02-30'", "ERR"),
        ("select timestamp '2024-13-01 00:00:00'", "ERR"),
        ("select cast('2024-02-30' as date)", "NULL"),
        ("select to_date('2024-02-30')", "NULL"),
        ("select timestamp '2024-01-31 10:00:00' + interval 1 month", "2024-02-29 10:00:00"),
        ("select timestamp '2024-03-15 10:00:00' - interval 2 hours", "2024-03-15 08:00:00"),
        ("select timestamp '2024-03-15 10:00:00' + interval '1' day", "2024-03-16 10:00:00"),
        ("select date '2024-01-31' + interval 1 month", "2024-02-29"),
        ("select date_trunc('month', timestamp '2024-03-15 10:20:30')", "2024-03-01 00:00:00"),
        ("select date_trunc('hour', timestamp '2024-03-15 10:20:30')", "2024-03-15 10:00:00"),
        ("select date_trunc('week', date '2024-03-15')", "2024-03-11 00:00:00"),
        ("select date_trunc('quarter', timestamp '2024-05-15 10:20:30')", "2024-04-01 00:00:00"),
        ("select date_trunc('year', date '2024-05-15')", "2024-01-01 00:00:00"),
        ("select trunc(date '2024-03-15', 'month')", "2024-03-01"),
        ("select trunc(date '2024-03-15', 'year')", "2024-01-01"),
    ]);
}

#[test]
fn date_timestamp_columns() {
    let mut c = KqlContext::new();
    c.register("d", DataBlock::new(vec![
        Column::int64("id", vec![Some(1), Some(2), Some(3)]),
        Column::str_col("dt", s(&[Some("2024-03-14"), Some("2024-03-15"), None])),
        Column::str_col("ts", s(&[Some("2024-03-14 12:00:00"), Some("2024-03-15 00:00:00"), Some("2024-03-15 00:00:01")])),
    ]).unwrap());
    check_in(&c, &[
        ("select id from d where dt = timestamp '2024-03-15 00:00:00'", "2"),
        ("select id from d where ts = date '2024-03-15'", "2"),
        ("select id from d where ts > date '2024-03-15' order by id", "3"),
        ("select id from d where ts >= '2024-03-15' order by id", "2|3"),
        ("select id from d where dt < timestamp '2024-03-15 00:00:01' order by id", "1|2"),
        ("select id from d where dt = ts order by id", "2"),
        ("select count(*) from d where ts <= '2024-03-14'", "0"),
        ("select count(*) from d where dt <> '2024-03-15 00:00:00'", "1"),
    ]);
}

#[test]
fn decimal_display() {
    check(&[
        ("select cast(1.5 as decimal(5,2))", "1.50"),
        ("select cast(3 as decimal(5,2))", "3.00"),
        ("select cast(-0.001 as decimal(5,2))", "0.00"),
        ("select cast(123.456 as decimal(10,2))", "123.46"),
        ("select cast(1234567.891 as decimal(5,2))", "NULL"),
        ("select cast('2.5' as decimal(4,1))", "2.5"),
        ("select cast(2.7 as decimal)", "3"),
        ("select cast(1.50 as decimal(5,2)), cast(v as decimal(6,3)) from t where id = 1", "1.50,10.000"),
        ("select typeof(cast(1 as decimal(5,2)))", "decimal(5,2)"),
        // arithmetic and aggregates stay numeric (double) and are not formatted
        ("select cast(1.5 as decimal(5,2)) + 1", "2.5"),
        ("select sum(cast(v as decimal(10,2))) from t", "170"),
        // inside a derived table the value stays numeric
        ("select c + 1 from (select cast(1.5 as decimal(5,2)) c) q", "2.5"),
        ("select 0.1 + 0.2", "0.3"),
    ]);
}

#[test]
fn struct_values() {
    check(&[
        ("select struct(1, 'a')", "{1, a}"),
        ("select named_struct('a', 1, 'b', 'x')", "{1, x}"),
        ("select named_struct('a', 1, 'b', cast(null as string))", "{1, null}"),
        ("select struct(1, 2).col2", "2"),
        ("select named_struct('a', 1, 'b', 'x').b", "x"),
        ("select s.a, s.b from (select named_struct('a', 1, 'b', 'x') as s) q", "1,x"),
        ("select q.s.a from (select named_struct('a', 7) as s) q", "7"),
        ("select s.n.k from (select named_struct('n', named_struct('k', 5)) as s) q", "5"),
        ("select struct(id, g) from t where id = 1", "{1, x}"),
        ("select s.id, s.g from (select struct(id, g) as s from t where id = 2) q", "2,x"),
        ("select named_struct('a', 1) = named_struct('a', 1)", "true"),
        ("select array(named_struct('a', 1), named_struct('a', 2))", "[{1}, {2}]"),
        ("select named_struct('a', array(1, 2), 'm', map('k', 1))", "{[1, 2], {k -> 1}}"),
        ("select to_json(named_struct('a', 1, 'b', 'x'))", "{\"a\":1,\"b\":\"x\"}"),
        ("select to_json(named_struct('a', 1, 'b', cast(null as int)))", "{\"a\":1}"),
        ("select to_json(array(named_struct('a', 1)))", "[{\"a\":1}]"),
        ("select from_json('{\"a\":1,\"b\":\"x\"}', 'a INT, b STRING')", "{1, x}"),
        ("select from_json('{\"a\":1,\"b\":\"x\"}', 'struct<a:int,b:string>').b", "x"),
        ("select from_json('{\"a\":1}', 'a INT, b STRING')", "{1, null}"),
        ("select from_json('{\"a\":{\"c\":[1,2]}}', 'a struct<c:array<int>>').a.c", "[1, 2]"),
        ("select from_json('[1,2]', 'a INT, b STRING')", "NULL"),
        ("select from_json('not json', 'a INT, b STRING')", "NULL"),
        ("select from_json('{\"a\":1.5}', 'a DOUBLE').a", "1.5"),
        ("select s.zzz from (select named_struct('a', 1) as s) q", "ERR"),
        ("select named_struct('a')", "ERR"),
    ]);
}

#[test]
fn generators_inline_stack_json_tuple() {
    check(&[
        ("select inline(array(named_struct('a', 1, 'b', 'x'), named_struct('a', 2, 'b', 'y')))", "1,x|2,y"),
        ("select a, b from (select inline(array(named_struct('a', 1, 'b', 'x'), named_struct('a', 2, 'b', 'y')))) q order by a desc", "2,y|1,x"),
        ("select stack(2, 1, 'a', 2, 'b')", "1,a|2,b"),
        ("select stack(2, 1, 2, 3)", "1,2|3,NULL"),
        ("select col0 from (select stack(3, 'x', 'y', 'z')) q order by col0", "x|y|z"),
        ("select json_tuple('{\"a\":1,\"b\":\"x\",\"c\":[1,2]}', 'a', 'b', 'c', 'd')", "1,x,[1,2],NULL"),
        ("select json_tuple('not json', 'a')", "NULL"),
        ("select c0 from (select json_tuple('{\"k\":\"v\"}', 'k')) q", "v"),
        ("select x, y from t lateral view stack(2, id, g, id * 10, 'q') s as x, y where id = 1 order by x", "1,x|10,q"),
        ("select a from t lateral view inline(array(named_struct('a', id))) i where id <= 2 order by a", "1|2"),
        ("select p1, q1 from t lateral view json_tuple('{\"p\":1,\"q\":2}', 'p', 'q') j as p1, q1 where id = 1", "1,2"),
    ]);
}
