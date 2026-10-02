//! Regression tests for silent wrong answers found by the TPC-H head-to-head
//! (benchmarks/tpch_honest). Expected values are worked out by hand.

use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::KqlContext;

fn ctx() -> KqlContext {
    let mut c = KqlContext::new();
    // customers 1..5; customer 5 has no orders
    c.register("cust", DataBlock::new(vec![
        Column::int64("c_id", (1..=5).map(Some).collect()),
        Column::str_col("c_seg", ["A", "B", "A", "B", "A"].iter().map(|s| Some(s.to_string())).collect()),
    ]).unwrap());
    // orders: (id, cust, price, comment)
    c.register("ord", DataBlock::new(vec![
        Column::int64("o_id", (10..16).map(Some).collect()),
        Column::int64("o_cust", [1, 1, 2, 3, 4, 4].iter().map(|&x| Some(x)).collect()),
        Column::float64("o_price", [10.0, 20.0, 30.0, 40.0, 50.0, 60.0].iter().map(|&x| Some(x)).collect()),
        Column::str_col("o_comment", ["ok", "special requests", "ok", "ok", "special requests", "ok"].iter().map(|s| Some(s.to_string())).collect()),
    ]).unwrap());
    // lines: (order, qty)
    c.register("line", DataBlock::new(vec![
        Column::int64("l_ord", [10, 10, 11, 12, 13, 14, 15].iter().map(|&x| Some(x)).collect()),
        Column::float64("l_qty", [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0].iter().map(|&x| Some(x)).collect()),
    ]).unwrap());
    c
}

fn f64s(b: &DataBlock, name: &str) -> Vec<f64> {
    let c = b.columns.iter().find(|c| c.name == name || c.name.ends_with(&format!(".{name}"))).unwrap_or_else(|| panic!("no column {name} in {:?}", b.columns.iter().map(|c| &c.name).collect::<Vec<_>>()));
    match &c.data {
        ColumnData::Float64(v) => v.iter().map(|x| x.unwrap()).collect(),
        ColumnData::Int64(v) => v.iter().map(|x| x.unwrap() as f64).collect(),
        _ => panic!("not numeric"),
    }
}

#[test]
fn trailing_tokens_are_an_error_not_silently_dropped() {
    let c = ctx();
    assert!(c.query("select c_id from cust garbage junk").is_err());
    assert!(c.query("select c_id from cust where c_id = 1 and").is_err());
    assert!(c.query("select c_id from cust limit 2 3").is_err());
    assert!(c.query("select c_id from cust;").is_ok(), "a trailing semicolon is fine");
}

#[test]
fn comma_join_uses_the_where_equality() {
    let c = ctx();
    let r = c.query("select o_id, c_seg from cust, ord where c_id = o_cust and c_seg = 'A' order by o_id").unwrap();
    // customers 1 and 3 are segment A: orders 10, 11, 13
    assert_eq!(f64s(&r, "o_id"), vec![10.0, 11.0, 13.0]);
}

#[test]
fn three_table_comma_join_and_single_table_filters() {
    let c = ctx();
    let r = c.query(
        "select sum(l_qty) as q from cust, ord, line \
         where c_id = o_cust and o_id = l_ord and c_seg = 'B' and o_price > 25").unwrap();
    // segment B = customers 2, 4; orders 12 (30), 14 (60), 15? (cust 4 price 50 is order 14 -> see data)
    // orders: 10:c1, 11:c1, 12:c2(30), 13:c3, 14:c4(50), 15:c4(60) -> lines for 12, 14, 15 = 4 + 6 + 7
    assert_eq!(f64s(&r, "q"), vec![17.0]);
}

#[test]
fn comma_join_without_any_equality_is_a_cross_product() {
    let c = ctx();
    let r = c.query("select count(*) as n from cust, ord").unwrap();
    assert_eq!(f64s(&r, "n"), vec![30.0]);
}

#[test]
fn aggregates_nested_in_expressions() {
    let c = ctx();
    let r = c.query("select 100.0 * sum(o_price) / count(*) as v, sum(o_price) / 7.0 as w from ord").unwrap();
    assert_eq!(r.num_rows, 1);
    assert!((f64s(&r, "v")[0] - 100.0 * 210.0 / 6.0).abs() < 1e-9);
    assert!((f64s(&r, "w")[0] - 30.0).abs() < 1e-9);
}

#[test]
fn nested_aggregates_with_group_by_having_and_order() {
    let c = ctx();
    let r = c.query(
        "select o_cust, sum(case when o_comment = 'ok' then o_price else 0 end) / sum(o_price) as share \
         from ord group by o_cust having sum(o_price) > 35 order by o_cust").unwrap();
    // cust 1: 30 total (excluded); cust 2: 30 (excluded); cust 3: 40 -> 1.0; cust 4: 110 -> 60/110
    assert_eq!(f64s(&r, "o_cust"), vec![3.0, 4.0]);
    let share = f64s(&r, "share");
    assert!((share[0] - 1.0).abs() < 1e-9 && (share[1] - 60.0 / 110.0).abs() < 1e-9);
}

#[test]
fn aggregate_over_comma_join_matches_the_explicit_join() {
    let c = ctx();
    let implicit = c.query("select 100.0 * sum(case when c_seg = 'A' then o_price else 0 end) / sum(o_price) as p from cust, ord where c_id = o_cust").unwrap();
    let explicit = c.query("select 100.0 * sum(case when c_seg = 'A' then o_price else 0 end) / sum(o_price) as p from cust join ord on c_id = o_cust").unwrap();
    assert_eq!(f64s(&implicit, "p"), f64s(&explicit, "p"));
    // A = customers 1, 3: 30 + 40 = 70 of 210
    assert!((f64s(&implicit, "p")[0] - 100.0 * 70.0 / 210.0).abs() < 1e-9);
}

#[test]
fn exists_and_not_exists_semi_joins() {
    let c = ctx();
    // customers without any order
    let r = c.query("select c_id from cust where not exists (select * from ord where o_cust = c_id)").unwrap();
    assert_eq!(f64s(&r, "c_id"), vec![5.0]);
    // inner-only filter next to the correlated equality, unqualified columns on both sides
    let r = c.query("select c_id from cust where exists (select * from ord where o_cust = c_id and o_price > 45) order by c_id").unwrap();
    assert_eq!(f64s(&r, "c_id"), vec![4.0]);
}

#[test]
fn exists_with_a_correlated_inequality() {
    let c = ctx();
    // orders whose customer has another order (customers 1 and 4)
    let r = c.query("select o_id from ord o1 where exists (select * from ord o2 where o2.o_cust = o1.o_cust and o2.o_id <> o1.o_id) order by o_id").unwrap();
    assert_eq!(f64s(&r, "o_id"), vec![10.0, 11.0, 14.0, 15.0]);
    let r = c.query("select o_id from ord o1 where not exists (select * from ord o2 where o2.o_cust = o1.o_cust and o2.o_id <> o1.o_id) order by o_id").unwrap();
    assert_eq!(f64s(&r, "o_id"), vec![12.0, 13.0]);
}

#[test]
fn having_with_aggregates_and_order_by_alias() {
    let c = ctx();
    let r = c.query("select o_cust, sum(o_price) as total from ord group by o_cust having sum(o_price) > 35 order by total desc").unwrap();
    assert_eq!(f64s(&r, "o_cust"), vec![4.0, 3.0]);
    assert_eq!(f64s(&r, "total"), vec![110.0, 40.0]);
    // threshold from an uncorrelated scalar subquery (TPC-H Q11 shape)
    let r = c.query(
        "select o_cust, sum(o_price) as value from ord group by o_cust \
         having sum(o_price) > (select sum(o_price) * 0.2 from ord) order by value desc").unwrap();
    assert_eq!(f64s(&r, "o_cust"), vec![4.0]);
}

#[test]
fn with_clause_is_honoured_by_the_public_query_entry_point() {
    let c = ctx();
    let r = kore_sql::query("with big as (select o_cust, sum(o_price) as s from ord group by o_cust) select max(s) as m from big", &c).unwrap();
    assert_eq!(f64s(&r, "m"), vec![110.0]);
    // the CTE must also be visible inside a subquery (TPC-H Q15 shape)
    let r = kore_sql::query(
        "with big as (select o_cust, sum(o_price) as s from ord group by o_cust) \
         select o_cust from big where s = (select max(s) from big)", &c).unwrap();
    assert_eq!(f64s(&r, "o_cust"), vec![4.0]);
}

#[test]
fn correlated_scalar_aggregate_subqueries() {
    let c = ctx();
    // the order with the highest price for its customer: 11 (c1: 20), 12 (c2), 13 (c3), 15 (c4: 60)
    let r = c.query("select o_id from ord where o_price = (select max(o2.o_price) from ord o2 where o2.o_cust = ord.o_cust) order by o_id").unwrap();
    assert_eq!(f64s(&r, "o_id"), vec![11.0, 12.0, 13.0, 15.0]);

    // several tables inside the subquery (TPC-H Q2 shape): customers whose orders have exactly 2 lines in total
    let r = c.query("select c_id from cust where 2 = (select count(*) from ord, line where o_id = l_ord and o_cust = c_id)").unwrap();
    assert_eq!(f64s(&r, "c_id"), vec![4.0]);

    // comparison with a per-group average (TPC-H Q17 shape): only order 10's second line (qty 2 > avg 1.5)
    let r = c.query("select count(*) as n from line where l_qty > (select avg(l2.l_qty) from line l2 where l2.l_ord = line.l_ord)").unwrap();
    assert_eq!(f64s(&r, "n"), vec![1.0]);

    // the subquery reads the same tables as the outer query: its own columns shadow the outer ones (Q2 shape).
    // cheapest order per customer: the outer row must match the minimum over *its* customer's orders
    let r = c.query(
        "select o_id from ord, cust where o_cust = c_id and o_price = (select min(o_price) from ord, cust where o_cust = c_id) order by o_id").unwrap();
    // inner `ord`/`cust` shadow the outer ones, so nothing correlates: the minimum over everything is 10 (order 10)
    assert_eq!(f64s(&r, "o_id"), vec![10.0]);

    // Q2 shape: the subquery re-reads `line`, which the outer query also has; only o_id is outer-only.
    // Order 10 has lines with qty 1 and 2, so only the qty-2 line is its maximum.
    let r = c.query(
        "select l_qty from ord, line where o_id = l_ord and o_id = 10 and l_qty = (select max(l_qty) from line where l_ord = o_id)").unwrap();
    assert_eq!(f64s(&r, "l_qty"), vec![2.0]);

    // a group with no inner rows compares as NULL, so the row is dropped
    let r = c.query("select c_id from cust where 99 > (select min(o_price) from ord where o_cust = c_id) order by c_id").unwrap();
    assert_eq!(f64s(&r, "c_id"), vec![1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn left_join_with_extra_on_condition_keeps_unmatched_rows() {
    let c = ctx();
    // TPC-H Q13 shape: the comment filter belongs to the ON clause, so customers whose orders are all
    // filtered out still appear with a zero count.
    let r = c.query(
        "select c_id, count(o_id) as n from cust left join ord on c_id = o_cust and o_comment not like '%special%' \
         group by c_id order by c_id").unwrap();
    // cust 1: orders 10 (ok), 11 (special) -> 1; 2: 1; 3: 1; 4: 14 special, 15 ok -> 1; 5: none -> 0
    assert_eq!(f64s(&r, "c_id"), vec![1.0, 2.0, 3.0, 4.0, 5.0]);
    assert_eq!(f64s(&r, "n"), vec![1.0, 1.0, 1.0, 1.0, 0.0]);
}
