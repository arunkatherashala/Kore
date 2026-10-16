//! Regression tests for bugs found by the TPC-DS run (benchmarks/tpcds_honest). Expected values are hand-derived
//! (or noted as taken from DuckDB). Format as in sql_coverage2.rs: rows `|`, cells `,`, NULL prints `NULL`,
//! `ERR` = must be rejected, `~` = unordered.

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
    c.register("u", DataBlock::new(vec![
        Column::int64("uid", vec![Some(1), Some(2), Some(7)]),
        Column::str_col("un", s(&[Some("a"), Some("b"), Some("c")])),
    ]).unwrap());
    c.register("w", DataBlock::new(vec![
        Column::int64("wid", vec![Some(1), Some(2)]),
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
            ColumnData::Str(v) => v[r].clone().map(|x| format!("'{x}'")).unwrap_or("NULL".into()),
            ColumnData::StrDict { codes, dict } => if codes[r] == u8::MAX { "NULL".into() } else { format!("'{}'", dict[codes[r] as usize]) },
        }).collect();
        rows.push(cells.join(","));
    }
    rows.join("|")
}

fn check(cases: &[(&str, &str)]) {
    let c = base();
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

/// TPC-DS Q62/Q99: `SELECT expr AS a, * FROM t` silently dropped everything but the star columns.
#[test]
fn star_mixed_with_expressions() {
    check(&[
        ("select id * 10 as x, * from t where id < 3 order by id", "10,1,'x',10,1.5|20,2,'x',20,2.5"),
        ("select *, uid + 1 as x from u order by uid", "1,'a',2|2,'b',3|7,'c',8"),
        ("select a, uid from (select upper(un) a, * from u) q order by uid", "'A',1|'B',2|'C',7"),
    ]);
}

/// TPC-DS Q27/Q36: a constant select item next to GROUP BY came out as TEXT ('0' instead of 0), and a float
/// constant was rounded to 4 digits.
#[test]
fn constant_select_item_with_group_by_keeps_type() {
    check(&[
        ("select 0 as gflag, g, sum(v) s from t group by g order by g nulls first", "0,NULL,60|0,'x',30|0,'y',30|0,'z',50"),
        ("select 1.23456789 as c, g from t group by g order by g nulls first limit 1", "1.234568,NULL"),
        ("select 1 as k, count(*) n from t union all select 2 as k, count(*) n from t", "1,6|2,6"),
        ("select * from (select 0 as k, g, avg(v) a from t group by g union all select 1 as k, null as g, avg(v) a from t) q order by k desc, g nulls first limit 2", "1,NULL,34|0,NULL,60"),
    ]);
}

/// TPC-DS Q40/Q49/Q80/Q93: comma joins after an explicit JOIN ... ON.
/// u = (1,a),(2,b),(7,c); w = (1),(2); t has ids 1..6.
#[test]
fn comma_join_after_explicit_join() {
    check(&[
        ("select uid, un, wid, id from u left join w on uid = wid, t where t.id = u.uid order by uid",
         "1,'a',1,1|2,'b',2,2"),
        ("select uid, wid, id from u left join w on uid = wid, t where t.id = u.uid + 3 order by uid",
         "1,1,4|2,2,5"),
        // the LEFT JOIN keeps the unmatched u row 7 (no t row with id 7+0 though: filtered by the WHERE equality)
        ("select count(*) from u left join w on uid = wid, t where t.id = 6 and w.wid is null", "1"),
        ("select uid, wid from u left join w on uid = wid, t where t.id = 1 order by uid", "1,1|2,2|7,NULL"),
    ]);
}

/// TPC-DS Q6: ORDER BY a qualified GROUP BY column that the select list renames, with HAVING count(*).
#[test]
fn order_by_qualified_group_column_with_having() {
    check(&[
        ("select a.g gg, count(*) cnt from t a where a.g is not null group by a.g having count(*) >= 1 order by a.g desc",
         "'z',1|'y',2|'x',2"),
        ("select a.g gg, count(*) cnt from t a where a.g is not null group by a.g having count(*) >= 2 order by cnt, a.g",
         "'x',2|'y',2"),
    ]);
}

/// GROUP BY a select alias of a computed expression keeps the expression's type (was rendered as text).
#[test]
fn group_by_expression_alias_keeps_type() {
    check(&[
        ("select id % 2 as par, count(*) n from t group by par order by par", "0,3|1,3"),
        ("select f * 1.0001 as ff, count(*) n from t where f = 1.5 group by ff", "1.50015,1"),
    ]);
}
