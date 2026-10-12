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

// Expected values below were produced by running the same statements in Spark (non-ANSI, session time zone UTC) over the same data.
#[test]
fn aggregates_hash_zones_syntax_vs_spark() {
    check(&[
        ("select bit_and(v), bit_or(v), bit_xor(v) from t", "0,62,14"),
        ("select g, bit_or(id) from t group by g order by g", "NULL,6|x,3|y,7|z,5"),
        ("select bit_and(v) from t where id = 4", "NULL"),
        ("select regr_count(f, v), regr_avgx(f, v), regr_avgy(f, v), regr_sxx(f, v), regr_syy(f, v), regr_sxy(f, v) from t", "5,34,0.9,1720,32.7,7"),
        ("select regr_slope(f, v), regr_intercept(f, v), regr_r2(f, v) from t", "0.00407,0.761628,0.000871"),
        ("select regr_slope(f, v) from t where id = 1", "NULL"),
        ("select regr_r2(f, v) from t where id <= 2", "1"),
        ("select regr_count(f, v) from t where id = 4", "0"),
        ("select histogram_numeric(f, 2) from t", "[{-3.5, 1.0}, {2.0, 4.0}]"),
        ("select histogram_numeric(f, 10) from t", "[{-3.5, 1.0}, {0.0, 1.0}, {1.5, 1.0}, {2.5, 1.0}, {4.0, 1.0}]"),
        ("select hash(g), xxhash64(g), hash(f), xxhash64(f) from t order by id", "-80340190,-5636050478767222463,1290763749,7738255526519901366|-80340190,-5636050478767222463,-892273744,2435211733827758672|1475214315,-2901346384480684965,-1316292811,-4418739860910625355|1475214315,-2901346384480684965,42,42|1190301286,6959616394252558107,1290556682,-3054283333461073093|42,42,-1670924195,-5252525462095825812"),
        ("select hash(1, 'a', 2.5, true), xxhash64(1, 'a', 2.5, true), hash(array(1,2,3)), hash(named_struct('a',1,'b','x'))", "2009709968,8123807079836635594,-912918097,-1305545736"),
        ("select hash('abcdefghijklmnopqrstuvwxyz0123456789'), xxhash64('abcdefghijklmnopqrstuvwxyz0123456789'), hash(''), xxhash64(''), hash('héllo'), xxhash64('héllo')", "46508703,2724237963427677065,142593372,-7444071767201028348,1212979866,501425390238239234"),
        ("select hash(cast(5 as int)), hash(cast(5 as bigint)), hash(-1), hash(2147483648), hash(0.0), hash(-0.0)", "1023896466,1607884268,-1604776387,238268309,-1670924195,-1670924195"),
        ("select hash('Spark', array(123), 2), xxhash64('Spark', array(123), 2), hash(cast(null as int)), hash(1, cast(null as string))", "-1321691492,5602566077635097486,42,-559580957"),
        ("select hash(map('a', 1))", "ERR"),
        ("select soundex('Robert'), soundex('Rupert'), soundex('Tymczak'), soundex('Pfister'), soundex('Honeyman'), soundex('A'), soundex(null), soundex(''), soundex('123'), soundex('a-b'), soundex('Ashcraft')", "R163,R163,T522,P236,H555,A000,NULL,,123,A100,A261"),
        ("select from_utc_timestamp(timestamp '2024-01-15 12:00:00', 'America/Chicago'), from_utc_timestamp(timestamp '2024-07-15 12:00:00', 'America/Los_Angeles'), from_utc_timestamp(timestamp '2024-07-15 12:00:00', 'Europe/Berlin'), from_utc_timestamp(timestamp '2024-01-15 12:00:00', 'Australia/Sydney'), from_utc_timestamp(timestamp '2024-07-15 12:00:00', 'Australia/Sydney'), from_utc_timestamp(timestamp '2024-07-15 12:00:00', 'Asia/Tokyo')", "2024-01-15 06:00:00,2024-07-15 05:00:00,2024-07-15 14:00:00,2024-01-15 23:00:00,2024-07-15 22:00:00,2024-07-15 21:00:00"),
        ("select from_utc_timestamp(timestamp '2024-07-01 12:00:00', 'America/New_York'), from_utc_timestamp(timestamp '2024-03-10 06:59:59', 'America/New_York'), from_utc_timestamp(timestamp '2024-03-10 07:00:00', 'America/New_York'), from_utc_timestamp(timestamp '2024-10-27 00:59:59', 'Europe/London'), from_utc_timestamp(timestamp '2024-10-27 01:00:00', 'Europe/London'), from_utc_timestamp(timestamp '2024-01-01 12:00:00', '+05:30'), from_utc_timestamp(timestamp '2024-01-01 12:00:00', 'Asia/Kolkata')", "2024-07-01 08:00:00,2024-03-10 01:59:59,2024-03-10 03:00:00,2024-10-27 01:59:59,2024-10-27 01:00:00,2024-01-01 17:30:00,2024-01-01 17:30:00"),
        ("select to_utc_timestamp(timestamp '2024-07-15 12:00:00', 'America/New_York'), to_utc_timestamp(timestamp '2024-01-15 12:00:00', 'America/New_York'), to_utc_timestamp(timestamp '2024-11-03 01:30:00', 'America/New_York'), to_utc_timestamp(timestamp '2024-03-10 02:30:00', 'America/New_York'), to_utc_timestamp(timestamp '2024-03-31 01:30:00', 'Europe/Berlin'), to_utc_timestamp(timestamp '2024-10-27 02:30:00', 'Europe/Berlin')", "2024-07-15 16:00:00,2024-01-15 17:00:00,2024-11-03 05:30:00,2024-03-10 07:30:00,2024-03-31 00:30:00,2024-10-27 00:30:00"),
        ("select from_utc_timestamp(timestamp '2024-04-07 02:30:00', 'Australia/Sydney'), from_utc_timestamp(timestamp '2024-04-06 15:30:00', 'Australia/Sydney'), from_utc_timestamp(timestamp '2024-10-05 15:59:59', 'Australia/Sydney'), from_utc_timestamp(timestamp '2024-10-05 16:00:00', 'Australia/Sydney')", "2024-04-07 12:30:00,2024-04-07 02:30:00,2024-10-06 01:59:59,2024-10-06 03:00:00"),
        ("select from_utc_timestamp(timestamp '2024-07-15 12:00:00', 'UTC+2'), from_utc_timestamp(timestamp '2024-07-15 12:00:00', 'GMT-3')", "2024-07-15 14:00:00,2024-07-15 09:00:00"),
        ("select from_utc_timestamp(null, 'UTC'), from_utc_timestamp(timestamp '2024-07-15 12:00:00', null)", "NULL,NULL"),
        ("select from_utc_timestamp(timestamp '2024-07-15 12:00:00', 'Mars/Phobos')", "ERR"),
        ("select id from t sort by id desc", "6|5|4|3|2|1"),
        ("select id from t cluster by id", "1|2|3|4|5|6"),
        ("select id from t distribute by id sort by id", "1|2|3|4|5|6"),
        ("select g, count(*) from t group by g cluster by g", "NULL,1|x,2|y,2|z,1"),
        ("use default", ""),
        ("set spark.sql.shuffle.partitions=4", "spark.sql.shuffle.partitions,4"),
        ("select hash(2.5), hash(-1.25), xxhash64(2.5), hash(cast(2.5 as double))", "77911036,-550551638,9077239597295338739,-892273744"),
    ]);
}

#[test]
fn groups_frames() {
    let from = "from (values (1),(1),(2),(3),(3),(3),(5)) as q(x)";
    let cases: Vec<(String, &str)> = vec![
        (format!("select x, sum(x) over (order by x groups between 1 preceding and current row) {from}"), "1,2|1,2|2,4|3,11|3,11|3,11|5,14"),
        (format!("select x, sum(x) over (order by x groups 1 preceding) {from}"), "1,2|1,2|2,4|3,11|3,11|3,11|5,14"),
        (format!("select x, sum(x) over (order by x groups between current row and 1 following) {from}"), "1,4|1,4|2,11|3,14|3,14|3,14|5,5"),
        (format!("select x, sum(x) over (order by x groups between unbounded preceding and 1 preceding) {from}"), "1,NULL|1,NULL|2,2|3,4|3,4|3,4|5,13"),
        (format!("select x, sum(x) over (order by x groups between 1 following and 2 following) {from}"), "1,11|1,11|2,14|3,5|3,5|3,5|5,NULL"),
        (format!("select x, count(*) over (order by x groups between unbounded preceding and current row) {from}"), "1,2|1,2|2,3|3,6|3,6|3,6|5,7"),
    ];
    let refs: Vec<(&str, &str)> = cases.iter().map(|(a, b)| (a.as_str(), *b)).collect();
    check(&refs);
}

#[test]
fn tablesample_and_session_statements() {
    check(&[
        ("select count(*) from t tablesample (3 rows)", "3"),
        ("select s.id from t tablesample (2 rows) s order by s.id", "1|2"),
        ("select count(*) from t tablesample (100 percent)", "6"),
        ("select count(*) from t tablesample (0 percent)", "0"),
        ("select count(*) from t tablesample (bucket 1 out of 1)", "6"),
        ("select sum(c) from (select count(*) c from t tablesample (bucket 1 out of 2) repeatable (3) union all select count(*) c from t tablesample (bucket 2 out of 2) repeatable (3)) q", "6"),
        ("select count(*) from t tablesample (150 percent)", "ERR"),
        ("select count(*) from t tablesample (bucket 3 out of 2)", "ERR"),
    ]);
    // REPEATABLE gives the same rows every time
    let c = base();
    let a = c.query("select id from t tablesample (50 percent) repeatable (11)").unwrap();
    let b = c.query("select id from t tablesample (50 percent) repeatable (11)").unwrap();
    assert_eq!(render(&a), render(&b));
}

#[test]
fn alter_table_use_set() {
    let mut c = base();
    c.execute_dml("ALTER TABLE t ADD COLUMN extra STRING").unwrap();
    c.execute_dml("ALTER TABLE t ADD COLUMNS (n INT, d DOUBLE)").unwrap();
    check_in(&c, &[
        ("select id, extra, n, d from t where id = 1", "1,NULL,NULL,NULL"),
        ("select count(*) from t", "6"),
    ]);
    c.execute_dml("UPDATE t SET n = 5 WHERE id = 2").unwrap();
    c.execute_dml("ALTER TABLE t RENAME COLUMN n TO num").unwrap();
    check_in(&c, &[("select id, num from t where id <= 2 order by id", "1,NULL|2,5"), ("select n from t", "ERR")]);
    c.execute_dml("ALTER TABLE t DROP COLUMN extra").unwrap();
    check_in(&c, &[("select extra from t", "ERR"), ("select count(*) from t", "6")]);
    c.execute_dml("ALTER TABLE t RENAME TO t2").unwrap();
    check_in(&c, &[("select count(*) from t2", "6"), ("select count(*) from t", "ERR")]);
    assert!(c.execute_dml("ALTER TABLE t2 ADD COLUMN id INT").is_err());
    assert!(c.execute_dml("ALTER TABLE missing ADD COLUMN x INT").is_err());
    assert!(c.execute_dml("ALTER TABLE t2 SET TBLPROPERTIES ('a'='b')").is_err());
    c.execute_dml("USE default").unwrap();
    c.execute_dml("SET spark.sql.shuffle.partitions=8").unwrap();
}


#[test]
fn explain_shape() {
    let c = base();
    let plan = |sql: &str| -> String {
        let b = c.query(sql).unwrap();
        match &b.columns[0].data { ColumnData::Str(v) => v[0].clone().unwrap(), _ => panic!("plan column") }
    };
    let p = plan("explain select g, sum(v) from t where id > 1 group by g order by g limit 3");
    assert_eq!(p, "== Physical Plan ==\nTakeOrderedAndProject(limit=3, orderBy=[g ASC NULLS FIRST], output=[g, sum(v)])\n+- Project [g, sum(v)]\n   +- HashAggregate(keys=[g], functions=[sum(v)])\n      +- Filter (id > 1)\n         +- Scan t");
    let j = plan("explain select a.id from t a join t b on a.id = b.id");
    assert!(j.contains("SortMergeJoin Inner") && j.contains(":- Scan t") && j.contains("+- Scan t"), "{j}");
    let e = plan("explain extended select id from t");
    assert!(e.contains("== Parsed Logical Plan ==") && e.contains("== Optimized Logical Plan ==") && e.contains("== Physical Plan =="), "{e}");
    assert!(c.query("explain select from").is_err());
}

#[test]
fn window_function_time_windows() {
    check(&[
        ("select window(ts, '10 minutes') from (select timestamp '2024-01-01 00:03:00' ts) q", "{2024-01-01 00:00:00, 2024-01-01 00:10:00}"),
        // `window(..).start` is KORE-only syntax (Spark rejects field access on a function call); expected value by hand
        ("select window(ts, '10 minutes').start, window(ts, '1 hour').end from (select timestamp '2024-01-01 05:43:21' ts) q", "2024-01-01 05:40:00,2024-01-01 06:00:00"),
        ("select window(ts, '1 day', '1 day', '6 hours').start from (select timestamp '2024-01-01 05:00:00' ts) q", "2023-12-31 06:00:00"),
        ("select window.start, window.end, count(*) from (values (timestamp '2024-01-01 00:03:00'), (timestamp '2024-01-01 00:09:59'), (timestamp '2024-01-01 00:10:00')) as e(ts) group by window(ts, '10 minutes') order by 1", "2024-01-01 00:00:00,2024-01-01 00:10:00,2|2024-01-01 00:10:00,2024-01-01 00:20:00,1"),
        ("select window.start, count(*) c from (values (timestamp '2024-01-01 00:03:00'), (timestamp '2024-01-01 00:09:59'), (timestamp '2024-01-01 00:10:00')) as e(ts) group by window(ts, '10 minutes') having count(*) > 1", "2024-01-01 00:00:00,2"),
        ("select window(ts, '10 minutes', '5 minutes') from (select timestamp '2024-01-01 00:03:00' ts) q", "ERR"),
        ("select window(ts, '1 month') from (select timestamp '2024-01-01 00:03:00' ts) q", "ERR"),
    ]);
}
