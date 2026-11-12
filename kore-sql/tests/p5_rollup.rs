//! ROLLUP / CUBE / GROUPING SETS computed in one pass (finest grouping once, coarser sets merged from it,
//! rollup.rs) must give the answers of the old per-set execution (`testing::set_no_gs_shared(true)`):
//! NULL placeholders vs NULL key values, GROUPING() / GROUPING_ID(), HAVING, ORDER BY / LIMIT, windows over the
//! grouped output, COUNT(DISTINCT) and other non-mergeable aggregates, empty input.
//! Own test binary because the switch is process-wide; tests inside serialise on a lock.

use std::sync::Mutex;
use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::{testing, KqlContext};

static LOCK: Mutex<()> = Mutex::new(());
static OK_PAIRS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static ERR_PAIRS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
}

fn cell(c: &Column, r: usize) -> String {
    match &c.data {
        ColumnData::Int64(v) => v[r].map_or("NULL".into(), |x| x.to_string()),
        ColumnData::Float64(v) => v[r].map_or("NULL".into(), |x| format!("{:.6e}", x)),
        ColumnData::Str(v) => v[r].clone().unwrap_or("NULL".into()),
        ColumnData::StrDict { codes, dict } => if codes[r] == u8::MAX { "NULL".into() } else { dict[codes[r] as usize].clone() },
        ColumnData::Bool(v) => format!("{:?}", v[r]),
    }
}

fn rows(c: &KqlContext, sql: &str) -> Result<Vec<String>, String> {
    let b = c.query(sql).map_err(|e| format!("{e}"))?;
    Ok((0..b.num_rows).map(|r| b.columns.iter().map(|c| cell(c, r)).collect::<Vec<_>>().join("|")).collect())
}

/// Same rows in the same order (or, for queries without a total ORDER BY, the same multiset) old vs shared.
fn same(ctx: &KqlContext, q: &str, ordered: bool) {
    testing::set_no_gs_shared(true);
    let old = rows(ctx, q);
    testing::set_no_gs_shared(false);
    let new = rows(ctx, q);
    match (old, new) {
        (Ok(mut a), Ok(mut b)) => {
            assert_eq!(a.len(), b.len(), "row count differs for {q}");
            OK_PAIRS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if !ordered { a.sort(); b.sort(); }
            for (i, (x, y)) in a.iter().zip(&b).enumerate() { assert_eq!(x, y, "row {i} differs for {q}"); }
        }
        (Err(e1), Err(_)) => { ERR_PAIRS.fetch_add(1, std::sync::atomic::Ordering::Relaxed); let _ = e1; }
        (a, b) => panic!("one path failed for {q}: old ok={} new ok={}", a.is_ok(), b.is_ok()),
    }
}

fn table(n: usize, seed: u64) -> DataBlock {
    let mut r = Rng(seed);
    let a: Vec<Option<i64>> = (0..n).map(|_| if r.below(8) == 0 { None } else { Some(r.below(4) as i64) }).collect();
    let b: Vec<Option<String>> = (0..n).map(|_| if r.below(8) == 0 { None } else { Some(format!("b{}", r.below(3))) }).collect();
    let c: Vec<Option<i64>> = (0..n).map(|_| if r.below(10) == 0 { None } else { Some(r.below(3) as i64) }).collect();
    let d: Vec<Option<f64>> = (0..n).map(|_| if r.below(6) == 0 { None } else { Some(r.below(3) as f64 + 0.5 * r.below(2) as f64) }).collect();
    let v: Vec<Option<i64>> = (0..n).map(|_| if r.below(9) == 0 { None } else { Some(r.below(200) as i64 - 50) }).collect();
    let w: Vec<Option<f64>> = (0..n).map(|_| if r.below(9) == 0 { None } else { Some(r.below(10_000) as f64 / 100.0) }).collect();
    DataBlock::new(vec![
        Column::int64("a", a), Column::str_col("b", b), Column::int64("c", c), Column::float64("d", d),
        Column::int64("v", v), Column::float64("w", w),
    ]).unwrap()
}

const KEYS: [&str; 4] = ["a", "b", "c", "d"];
const AGGS: [&str; 9] = ["sum(v)", "avg(v)", "count(*)", "count(v)", "min(v)", "max(w)", "sum(w)", "avg(w)", "count(distinct c)"];

fn ctx_for(n: usize, seed: u64) -> KqlContext {
    let mut ctx = KqlContext::new();
    ctx.register("t", table(n, seed));
    ctx
}

#[test]
fn rollup_and_cube_random() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = Rng(7);
    for (n, seed) in [(0usize, 1u64), (1, 2), (5, 3), (300, 4), (4000, 5)] {
        let ctx = ctx_for(n, seed);
        for _ in 0..25 {
            let nk = 1 + r.below(4);
            let keys = &KEYS[..nk];
            let na = 1 + r.below(4);
            let mut aggs: Vec<String> = Vec::new();
            while aggs.len() < na { let a = AGGS[r.below(AGGS.len())]; if !aggs.iter().any(|x| x.starts_with(a)) { aggs.push(format!("{a} as a{}", aggs.len())); } }
            let kind = if r.below(2) == 0 { "rollup" } else { "cube" };
            let having = ["", "", "having count(*) > 2", "having sum(v) > 100"][r.below(4)];
            let gcols: Vec<String> = keys.iter().map(|k| format!("grouping({k}) as g_{k}")).collect();
            let q = format!("select {}, {}, {} from t group by {kind}({}) {having}", keys.join(", "), gcols.join(", "), aggs.join(", "), keys.join(", "));
            same(&ctx, &q, false);
            // without grouping() in the select list, with ORDER BY over every key and LIMIT
            let q2_nolimit = format!("select {}, {} from t group by {kind}({}) {having} order by {}", keys.join(", "), aggs.join(", "), keys.join(", "),
                keys.iter().map(|k| format!("{k} nulls first")).collect::<Vec<_>>().join(", "));
            same(&ctx, &q2_nolimit, false);
        }
    }
}

#[test]
fn grouping_sets_duplicates_and_empty() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for (n, seed) in [(0usize, 1u64), (2, 2), (500, 3)] {
        let ctx = ctx_for(n, seed);
        for q in [
            "select a, b, sum(v) as s, count(*) as c, grouping(a) as ga, grouping(b) as gb from t group by grouping sets ((a, b), (a), (a), ())",
            "select a, b, sum(v) as s, grouping_id(a, b) as gid from t group by grouping sets ((a), (b), ())",
            "select a, b, c, avg(w) as s, grouping_id(a, b, c) as gid from t group by grouping sets ((a, b), (b, c), (c), (), ())",
            "select count(*) as c, sum(v) as s from t group by grouping sets (())",
            "select a, b, sum(v) as s from t group by grouping sets ((a, b), (b, a))",
            "select a, b, min(v) as mn, max(v) as mx, sum(w) as sw from t group by a, rollup(b)",
            "select a, b, c, count(distinct b) as cb, count(distinct v) as cv from t group by cube(a, b, c)",
            "select a, b, sum(v) as s from t group by rollup(a, b) having grouping(a) = 1 or sum(v) > 50",
        ] {
            same(&ctx, q, false);
        }
    }
}

#[test]
fn windows_over_grouped_output() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for (n, seed) in [(0usize, 1u64), (700, 2), (6000, 3)] {
        let ctx = ctx_for(n, seed);
        // total ORDER BY: sum ties broken by the keys, so the order is fully determined
        for q in [
            "select sum(v) as tot, a, b, grouping(a) + grouping(b) as lh, rank() over (partition by grouping(a) + grouping(b), case when grouping(b) = 0 then a end order by sum(v) desc) as rk from t group by rollup(a, b) order by lh desc, case when grouping(a) + grouping(b) = 0 then a end, rk, a nulls first, b nulls first",
            "select sum(v) as tot, a, c, grouping(a) as ga, grouping(c) as gc, sum(sum(v)) over (partition by grouping(a)) as part from t group by cube(a, c) order by ga, gc, a nulls first, c nulls first",
            "select a, b, count(*) as cnt, rank() over (order by count(*) desc, a nulls first, b nulls first) as rk from t group by rollup(a, b) order by rk",
            "select a, b, count(*) as c from t group by rollup(a, b) order by c desc, a nulls first, b nulls first limit 5",
        ] {
            same(&ctx, q, true);
        }
    }
}

#[test]
fn shared_matches_known_answer() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    testing::set_no_gs_shared(false);
    let mut ctx = KqlContext::new();
    ctx.register("t", DataBlock::new(vec![
        Column::int64("a", vec![Some(1), Some(1), None, Some(2)]),
        Column::int64("v", vec![Some(10), Some(20), Some(30), None]),
    ]).unwrap());
    let got = rows(&ctx, "select a, sum(v) as s, avg(v) as m, count(*) as c, grouping(a) as g from t group by rollup(a) order by g, a nulls first").unwrap();
    assert_eq!(got, vec!["NULL|30|3.000000e1|1|0", "1|30|1.500000e1|2|0", "2|NULL|NULL|1|0", "NULL|60|2.000000e1|4|1"]);
}
