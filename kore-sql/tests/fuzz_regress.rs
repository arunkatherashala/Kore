//! Regression tests for wrong answers found by the random-query fuzzer (tests/fuzz_oracle.rs).

use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::KqlContext;

fn ctx() -> KqlContext {
    let mut c = KqlContext::new();
    c.register("l", DataBlock::new(vec![
        Column::int64("k", vec![Some(1), Some(2), Some(3), None, Some(2)]),
        Column::int64("g", vec![Some(10), Some(20), Some(30), Some(40), Some(50)]),
        Column::int64("v", vec![Some(1), Some(2), Some(3), Some(4), Some(5)]),
    ]).unwrap());
    c.register("r", DataBlock::new(vec![
        Column::int64("k", vec![Some(2), Some(2), Some(3), None, Some(9)]),
        Column::int64("g", vec![Some(7), Some(8), Some(9), Some(10), Some(11)]),
        Column::int64("v", vec![Some(100), Some(200), Some(300), Some(400), Some(500)]),
    ]).unwrap());
    c
}

fn rows(c: &KqlContext, sql: &str) -> Vec<Vec<String>> {
    let b = c.query(sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
    (0..b.num_rows).map(|r| b.columns.iter().map(|col| match &col.data {
        ColumnData::Int64(v) => v[r].map_or("NULL".into(), |x| x.to_string()),
        ColumnData::Float64(v) => v[r].map_or("NULL".into(), |x| x.to_string()),
        ColumnData::Str(v) => v[r].clone().unwrap_or("NULL".into()),
        _ => "?".into(),
    }).collect()).collect()
}

fn flat(c: &KqlContext, sql: &str) -> Vec<String> { rows(c, sql).into_iter().map(|r| r.join(",")).collect() }

#[test]
fn group_key_of_second_table_after_join_with_same_named_column() {
    let c = ctx();
    // l.g and r.g share a bare name: the key must come from r
    assert_eq!(flat(&c, "select r.g, count(distinct r.v) from l join r on l.k = r.k group by r.g order by r.g"), ["7,1", "8,1", "9,1"]);
    assert_eq!(flat(&c, "select r.g, count(*) from l, r where l.k = r.k group by r.g order by r.g"), ["7,2", "8,2", "9,1"]);
}

#[test]
fn empty_cross_join_keeps_its_columns() {
    let c = ctx();
    assert_eq!(flat(&c, "select r.v from l, r where l.v > 100"), Vec::<String>::new());
    assert_eq!(flat(&c, "select r.v, l.g from l, r where l.v > 100 and r.v > 0"), Vec::<String>::new());
}

#[test]
fn not_in_list_over_comma_join_still_sees_its_column() {
    let c = ctx();
    assert_eq!(flat(&c, "select count(*) from l, r where l.k = r.k and l.g not in (10, 20)"), ["3"]);
    assert_eq!(flat(&c, "select count(*) from l, r where l.k = r.k and r.g not in (7)"), ["3"]);
}

#[test]
fn having_with_same_named_columns_of_two_tables() {
    let c = ctx();
    assert_eq!(flat(&c, "select l.g, r.g, count(*) from l, r where l.k = r.k group by l.g, r.g having count(*) >= 1 order by 1, 2"),
        ["20,7,1", "20,8,1", "30,9,1", "50,7,1", "50,8,1"]);
}

#[test]
fn exists_inside_or_with_filter_and_null_key() {
    let c = ctx();
    // l.k = 1 has no partner, l.k NULL has none either: NOT EXISTS is TRUE for both
    assert_eq!(flat(&c, "select l.v from l where (not exists (select 1 from r q where q.k = l.k and q.g >= 8) or l.v > 100) order by 1"), ["1", "4"]);
    assert_eq!(flat(&c, "select l.v from l where (exists (select 1 from r q where q.k = l.k and q.g >= 8) or l.v > 100) order by 1"), ["2", "3", "5"]);
    assert_eq!(flat(&c, "select l.v from l where not (exists (select 1 from r q where q.k = l.k and q.g >= 8)) order by 1"), ["1", "4"]);
}

#[test]
fn not_in_empty_subquery_is_true_for_null() {
    let c = ctx();
    assert_eq!(flat(&c, "select l.v from l where l.k not in (select q.k from r q where q.g < 0) order by 1"), ["1", "2", "3", "4", "5"]);
    assert_eq!(flat(&c, "select l.v from l where l.k in (select q.k from r q where q.g < 0) order by 1"), Vec::<String>::new());
}

#[test]
fn order_by_qualified_column_in_general_path() {
    let c = ctx();
    // the ordinal makes this a general-path statement; r.g must not resolve to l.g
    assert_eq!(flat(&c, "select l.g, r.g from l, r where l.k = r.k order by r.g, 1"), ["20,7", "50,7", "20,8", "50,8", "30,9"]);
}

#[test]
fn limit_is_not_pushed_below_an_inner_join_or_an_aggregate() {
    let c = ctx();
    assert_eq!(flat(&c, "select count(*) from l join r on l.k = r.k limit 2"), ["5"]);
    assert_eq!(rows(&c, "select l.v from l join r on l.k = r.k limit 3").len(), 3);
    assert_eq!(flat(&c, "select count(*) from (select l.v from l join r on l.k = r.k limit 4) q"), ["4"]);
    assert_eq!(rows(&c, "select l.v from l join r on l.k = r.k join r r2 on r2.k = l.k limit 5").len(), 5);
}

#[test]
fn bigint_keys_above_2_pow_53_are_not_merged_by_semi_join_or_correlated_aggregate() {
    let mut c = KqlContext::new();
    c.register("bl", DataBlock::new(vec![Column::int64("k", vec![Some(9007199254740993), Some(9007199254740995), Some(5)])]).unwrap());
    c.register("br", DataBlock::new(vec![Column::int64("k", vec![Some(9007199254740992), Some(9007199254740994), Some(5)]), Column::int64("v", vec![Some(1), Some(2), Some(3)])]).unwrap());
    assert_eq!(flat(&c, "select k from bl where exists (select 1 from br where br.k = bl.k)"), ["5"]);
    assert_eq!(flat(&c, "select k from bl where not exists (select 1 from br where br.k = bl.k) order by k"), ["9007199254740993", "9007199254740995"]);
    assert_eq!(flat(&c, "select k, (select count(*) from br where br.k = bl.k) as n from bl order by k"), ["5,1", "9007199254740993,0", "9007199254740995,0"]);
    assert_eq!(flat(&c, "select bl.k from bl, br where bl.k = br.k"), ["5"]);
}

#[test]
fn distinct_and_group_by_compare_doubles_exactly() {
    let mut c = KqlContext::new();
    c.register("fl", DataBlock::new(vec![
        Column::float64("x", vec![Some(1e-12), Some(2e-12), Some(0.3), Some(0.1 + 0.2), Some(0.0), Some(-0.0), None]),
        Column::int64("i", vec![Some(1); 7]),
    ]).unwrap());
    // 1e-12, 2e-12, 0.3, 0.30000000000000004, 0.0 (== -0.0), NULL
    assert_eq!(rows(&c, "select distinct x from fl").len(), 6);
    assert_eq!(rows(&c, "select x, count(*) from fl group by x").len(), 6);
    assert_eq!(flat(&c, "select count(distinct x) from fl"), ["5"]);
}
