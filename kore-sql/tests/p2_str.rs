//! String-heavy GROUP BY / HAVING / top-N shortcuts must give exactly the answers of the older paths:
//!  * payload keys of a joined table (c_name, c_address, ...) gathered after the aggregation instead of before it;
//!  * groups failing `HAVING agg <op> c` dropped inside the aggregation of a lifted aggregate subquery;
//!  * bounded top-N for keys with NULLs, NULLS FIRST/LAST clauses and OFFSET.
//! Each query runs with every switch off and then on and the rows (including order) are compared.
//! Own test binary because the switches are process-wide; tests inside serialise on a lock.

use std::sync::Mutex;
use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::{testing, KqlContext};

static LOCK: Mutex<()> = Mutex::new(());

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
}

fn new_paths(on: bool) {
    testing::set_no_defer_gb(!on);
    testing::set_no_having_push(!on);
    testing::set_no_topn_ext(!on);
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

/// Same rows with the new paths off and on; in the same order when `exact`, else as multisets (errors must agree).
fn same_mode(ctx: &KqlContext, q: &str, exact: bool) -> usize {
    new_paths(false);
    let old = rows(ctx, q);
    new_paths(true);
    let new = rows(ctx, q);
    match (old, new) {
        (Ok(mut a), Ok(mut b)) => {
            if !exact { a.sort(); b.sort(); }
            assert_eq!(a.len(), b.len(), "row count differs for {q}");
            for (i, (x, y)) in a.iter().zip(&b).enumerate() { assert_eq!(x, y, "row {i} differs for {q}"); }
            a.len()
        }
        (Err(_), Err(_)) => 0,
        (a, b) => panic!("one path failed for {q}: old ok={} new ok={}", a.is_ok(), b.is_ok()),
    }
}
fn same(ctx: &KqlContext, q: &str) -> usize { same_mode(ctx, q, true) }
fn same_set(ctx: &KqlContext, q: &str) -> usize { same_mode(ctx, q, false) }

// ---------------------------------------------------------------------------------------------------------------
// deferred payload keys
// ---------------------------------------------------------------------------------------------------------------

/// customer / orders / lineitem shaped tables. `dup_keys`: customer keys repeat with different texts (the payload
/// columns then do NOT follow from the join key).
fn tpch_like(seed: u64, ncust: usize, nord: usize, nli: usize, dup_keys: bool) -> KqlContext {
    let mut r = Rng(seed);
    let names = ["Customer#1", "Customer#2", "caf\u{e9}-\u{4e2d}\u{6587}", "", "Customer#5", "\u{1f600} x"];
    let ck: Vec<Option<i64>> = (0..ncust).map(|i| Some(if dup_keys { (i / 2) as i64 } else { i as i64 })).collect();
    let cn: Vec<Option<String>> = (0..ncust).map(|i| if r.below(30) == 0 { None } else { Some(format!("{}-{}", names[r.below(names.len())], i)) }).collect();
    let ca: Vec<Option<String>> = (0..ncust).map(|i| Some(format!("addr {} {}", i % 17, if r.below(2) == 0 { "\u{e9}" } else { "e" }))).collect();
    let cb: Vec<Option<f64>> = (0..ncust).map(|_| if r.below(25) == 0 { None } else { Some(r.below(20_000) as f64 / 100.0 - 50.0) }).collect();
    let cnat: Vec<Option<i64>> = (0..ncust).map(|_| Some(r.below(5) as i64)).collect();
    let mut ctx = KqlContext::new();
    ctx.register("cust", DataBlock::new(vec![
        Column::int64("c_custkey", ck), Column::str_col("c_name", cn), Column::str_col("c_address", ca),
        Column::float64("c_acctbal", cb), Column::int64("c_nationkey", cnat),
    ]).unwrap());
    let ncust_keys = if dup_keys { ncust / 2 } else { ncust };
    let ok: Vec<Option<i64>> = (0..nord).map(|i| Some(i as i64)).collect();
    let oc: Vec<Option<i64>> = (0..nord).map(|_| if r.below(50) == 0 { None } else { Some(r.below(ncust_keys + 20) as i64) }).collect();
    let od: Vec<Option<String>> = (0..nord).map(|_| Some(format!("199{}-0{}-1{}", 2 + r.below(7), 1 + r.below(9), r.below(10)))).collect();
    ctx.register("ord", DataBlock::new(vec![
        Column::int64("o_orderkey", ok), Column::int64("o_custkey", oc), Column::str_col("o_orderdate", od),
    ]).unwrap());
    let lo: Vec<Option<i64>> = (0..nli).map(|_| Some(r.below(nord) as i64)).collect();
    let lp: Vec<Option<f64>> = (0..nli).map(|_| if r.below(40) == 0 { None } else { Some(r.below(100_000) as f64 / 100.0) }).collect();
    let ld: Vec<Option<f64>> = (0..nli).map(|_| Some(r.below(11) as f64 / 100.0)).collect();
    let lq: Vec<Option<f64>> = (0..nli).map(|_| Some(1.0 + r.below(50) as f64)).collect();
    let lf: Vec<Option<String>> = (0..nli).map(|_| Some(["R", "A", "N"][r.below(3)].to_string())).collect();
    ctx.register("li", DataBlock::new(vec![
        Column::int64("l_orderkey", lo), Column::float64("l_extendedprice", lp), Column::float64("l_discount", ld),
        Column::float64("l_quantity", lq), Column::str_col("l_returnflag", lf),
    ]).unwrap());
    let nn = ["ALGERIA", "BRAZIL", "CANADA", "EGYPT", "FRANCE"];
    ctx.register("nat", DataBlock::new(vec![
        Column::int64("n_nationkey", (0..5).map(Some).collect()),
        Column::str_col("n_name", nn.iter().map(|s| Some(s.to_string())).collect()),
    ]).unwrap());
    ctx
}

const Q10_LIKE: &str = "select c_custkey, c_name, sum(l_extendedprice * (1 - l_discount)) as revenue, c_acctbal, n_name, c_address \
    from cust, ord, li, nat where c_custkey = o_custkey and l_orderkey = o_orderkey and c_nationkey = n_nationkey \
    and l_returnflag = 'R' and o_orderdate >= '1993-03-01' group by c_custkey, c_name, c_acctbal, n_name, c_address";

#[test]
fn deferred_payload_keys_match() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let hits0 = testing::defer_gb_hits();
    for (seed, nc, no, nl) in [(1u64, 800usize, 6_000usize, 40_000usize), (2, 50, 400, 3_000), (3, 3, 10, 40), (4, 1_500, 3_000, 30_000), (5, 5, 0, 0)] {
        let ctx = tpch_like(seed, nc, no, nl, false);
        // unordered: same groups; ordered by every output column: same rows in the same order
        same_set(&ctx, Q10_LIKE);
        let n = same(&ctx, &format!("{Q10_LIKE} order by revenue desc, c_custkey"));
        let _ = n;
        same(&ctx, &format!("{Q10_LIKE} order by revenue desc, c_custkey limit 20"));
        same(&ctx, &format!("{Q10_LIKE} order by c_name, c_custkey limit 7 offset 3"));
        same(&ctx, &format!("{Q10_LIKE} having sum(l_extendedprice) > 1000 order by c_custkey"));
        // projection order / aliases differ from the group-by order
        same_set(&ctx, "select n_name as nation, c_name as who, count(*) as n, c_custkey from cust, ord, li, nat \
            where c_custkey = o_custkey and l_orderkey = o_orderkey and c_nationkey = n_nationkey \
            group by c_custkey, c_name, n_name");
        // only some of the payload columns are grouped (the others are not needed at all)
        same_set(&ctx, "select c_custkey, c_name, min(l_extendedprice) as m from cust, ord, li \
            where c_custkey = o_custkey and l_orderkey = o_orderkey group by c_custkey, c_name");
        // payload column also read by an aggregate: must not be deferred, still correct
        same_set(&ctx, "select c_custkey, c_acctbal, sum(c_acctbal) as s, count(*) as n from cust, ord, li \
            where c_custkey = o_custkey and l_orderkey = o_orderkey group by c_custkey, c_acctbal");
        same_set(&ctx, "select c_custkey, c_name, max(c_name) as mx from cust, ord, li \
            where c_custkey = o_custkey and l_orderkey = o_orderkey group by c_custkey, c_name");
        // text key without any numeric key: nothing to group by once the payload is deferred
        same_set(&ctx, "select c_name, count(*) as n from cust, ord where c_custkey = o_custkey group by c_name");
        // WHERE conditions over payload columns and ORs across tables
        same_set(&ctx, "select c_custkey, c_name, sum(l_quantity) as q from cust, ord, li \
            where c_custkey = o_custkey and l_orderkey = o_orderkey and c_name <> 'zz' and (c_acctbal > 10 or l_quantity < 5) \
            group by c_custkey, c_name");
    }
    assert!(testing::defer_gb_hits() > hits0, "the deferred group-by was never used");
}

#[test]
fn deferred_payload_keys_not_dependent_on_join_key() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // customer keys repeat with different names / addresses: grouping by (key, name, ...) must keep them apart
    for (seed, nc, no, nl) in [(11u64, 800usize, 6_000usize, 40_000usize), (12, 60, 400, 3_000), (13, 4, 12, 50)] {
        let ctx = tpch_like(seed, nc, no, nl, true);
        same_set(&ctx, Q10_LIKE);
        same(&ctx, &format!("{Q10_LIKE} order by revenue desc, c_custkey, c_name, c_address, n_name limit 15"));
        same_set(&ctx, "select c_custkey, c_name, count(*) as n from cust, ord, li \
            where c_custkey = o_custkey and l_orderkey = o_orderkey group by c_custkey, c_name");
        same_set(&ctx, "select c_custkey, count(*) as n from cust, ord where c_custkey = o_custkey group by c_custkey");
    }
}

#[test]
fn deferred_payload_keys_random() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = Rng(5);
    let groups = ["c_custkey", "c_name", "c_address", "c_acctbal", "n_name", "c_nationkey"];
    let aggs = ["sum(l_extendedprice)", "count(*)", "min(l_quantity)", "max(l_discount)", "avg(l_extendedprice * (1 - l_discount))"];
    let wheres = ["", "and l_returnflag = 'R'", "and o_orderdate >= '1994-01-01'", "and c_acctbal > 0", "and l_quantity < 10"];
    for (seed, dup) in [(21u64, false), (22, true), (23, false)] {
        let ctx = tpch_like(seed, 400, 3_000, 25_000, dup);
        for _ in 0..40 {
            let ng = 1 + r.below(groups.len());
            let mut gs: Vec<&str> = Vec::new();
            while gs.len() < ng { let c = groups[r.below(groups.len())]; if !gs.contains(&c) { gs.push(c); } }
            let na = 1 + r.below(3);
            let mut ag: Vec<String> = Vec::new();
            while ag.len() < na { let a = aggs[r.below(aggs.len())]; if !ag.iter().any(|x| x.starts_with(a)) { ag.push(format!("{a} as a{}", ag.len())); } }
            let tables = if gs.contains(&"n_name") { "cust, ord, li, nat" } else { "cust, ord, li" };
            let nat = if gs.contains(&"n_name") { " and c_nationkey = n_nationkey" } else { "" };
            let q = format!("select {}, {} from {tables} where c_custkey = o_custkey and l_orderkey = o_orderkey{nat} {} group by {}",
                gs.join(", "), ag.join(", "), wheres[r.below(wheres.len())], gs.join(", "));
            same_set(&ctx, &q);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// HAVING threshold inside the aggregation
// ---------------------------------------------------------------------------------------------------------------

fn li_table(seed: u64, n: usize, norders: usize, with_nulls: bool) -> KqlContext {
    let mut r = Rng(seed);
    // sorted keys (runs) or random keys
    let mut cur = 0i64;
    let mut left = 0usize;
    let sorted = seed % 2 == 0;
    let ok: Vec<Option<i64>> = (0..n).map(|i| {
        if sorted {
            if left == 0 { cur = r.below(norders) as i64 + (i as i64 / 7) * 0; left = 1 + r.below(8); }
            left -= 1;
            Some(cur)
        } else if with_nulls && r.below(60) == 0 { None } else { Some(r.below(norders) as i64) }
    }).collect();
    let q: Vec<Option<f64>> = (0..n).map(|_| if with_nulls && r.below(12) == 0 { None } else { Some(r.below(60) as f64) }).collect();
    let qi: Vec<Option<i64>> = (0..n).map(|_| if with_nulls && r.below(12) == 0 { None } else { Some(r.below(60) as i64) }).collect();
    let tag: Vec<Option<String>> = (0..n).map(|_| Some(["a", "b", "c"][r.below(3)].to_string())).collect();
    let mut ctx = KqlContext::new();
    ctx.register("li", DataBlock::new(vec![
        Column::int64("l_orderkey", ok), Column::float64("l_quantity", q), Column::int64("l_qi", qi), Column::str_col("tag", tag),
    ]).unwrap());
    let ids: Vec<Option<i64>> = (0..norders).map(|i| Some(i as i64)).collect();
    let tp: Vec<Option<f64>> = (0..norders).map(|_| Some(r.below(1000) as f64)).collect();
    ctx.register("ord", DataBlock::new(vec![Column::int64("o_orderkey", ids), Column::float64("o_totalprice", tp)]).unwrap());
    ctx
}

#[test]
fn having_threshold_matches() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let hits0 = testing::having_push_hits();
    let ops = [">", ">=", "<", "<="];
    for (seed, n, no, nulls) in [(1u64, 50_000usize, 4_000usize, false), (2, 50_000, 4_000, false), (3, 30_000, 2_000, true), (4, 20_000, 9_000, true), (6, 5_000, 100, true), (7, 100, 10, false)] {
        let ctx = li_table(seed, n, no, nulls);
        for op in ops {
            for k in ["0", "25", "300", "300.5", "-1", "1000000"] {
                same(&ctx, &format!("select l_orderkey from li group by l_orderkey having sum(l_quantity) {op} {k} order by l_orderkey"));
                same(&ctx, &format!("select l_orderkey, count(*) as c from li group by l_orderkey having sum(l_qi) {op} {k} order by l_orderkey"));
                same(&ctx, &format!("select l_orderkey, sum(l_quantity) as sq from li group by l_orderkey having avg(l_quantity) {op} {k} order by l_orderkey"));
            }
            same(&ctx, &format!("select l_orderkey from li group by l_orderkey having count(*) {op} 8 order by l_orderkey"));
            same(&ctx, &format!("select tag, count(*) as c from li group by tag having min(l_quantity) {op} 1 order by tag"));
            same(&ctx, &format!("select l_orderkey from li where tag <> 'a' group by l_orderkey having max(l_quantity) {op} 40 order by l_orderkey"));
        }
        // the TPC-H Q18 shape: the lifted aggregate subquery feeds IN
        same(&ctx, "select o_orderkey, o_totalprice from ord where o_orderkey in \
            (select l_orderkey from li group by l_orderkey having sum(l_quantity) > 250) order by o_totalprice desc, o_orderkey");
        same(&ctx, "select o_orderkey, sum(l_quantity) as q from ord, li where o_orderkey in \
            (select l_orderkey from li group by l_orderkey having sum(l_quantity) > 250) and o_orderkey = l_orderkey \
            group by o_orderkey order by q desc, o_orderkey limit 5");
        // threshold on an aggregate nobody selects, constant on the left
        same(&ctx, "select l_orderkey from li group by l_orderkey having 100 < sum(l_quantity) order by l_orderkey");
        same(&ctx, "select l_orderkey, sum(l_quantity) as s from li group by l_orderkey having sum(l_quantity) > 100 and count(*) > 2 order by l_orderkey");
        // nothing survives
        same(&ctx, "select l_orderkey from li group by l_orderkey having sum(l_quantity) > 1e12");
    }
    assert!(testing::having_push_hits() > hits0, "no group was ever dropped inside the aggregation");
}

// ---------------------------------------------------------------------------------------------------------------
// top-N
// ---------------------------------------------------------------------------------------------------------------

fn topn_table(seed: u64, n: usize) -> KqlContext {
    let mut r = Rng(seed);
    let a: Vec<Option<i64>> = (0..n).map(|_| if r.below(9) == 0 { None } else { Some(r.below(30) as i64 - 10) }).collect();
    let f: Vec<Option<f64>> = (0..n).map(|_| if r.below(9) == 0 { None } else { Some([0.0, -0.0, 1.5, -2.5, 7.0][r.below(5)]) }).collect();
    let s: Vec<Option<String>> = (0..n).map(|_| if r.below(9) == 0 { None } else { Some(format!("s{}\u{e9}", r.below(12))) }).collect();
    let b: Vec<Option<bool>> = (0..n).map(|_| if r.below(7) == 0 { None } else { Some(r.below(2) == 0) }).collect();
    let dg: Vec<u8> = (0..n).map(|_| { let x = r.below(4); if x == 3 { u8::MAX } else { x as u8 } }).collect();
    let nn: Vec<Option<i64>> = (0..n).map(|_| Some(r.below(5) as i64)).collect();
    let id: Vec<Option<i64>> = (0..n as i64).map(Some).collect();
    let mut ctx = KqlContext::new();
    ctx.register("t", DataBlock::new(vec![
        Column::int64("id", id), Column::int64("a", a), Column::float64("f", f), Column::str_col("s", s),
        Column::bool_col("b", b), Column::str_dict("dg", dg, vec!["x".into(), "y".into(), "z".into()]), Column::int64("nn", nn),
    ]).unwrap());
    ctx
}

#[test]
fn top_n_with_nulls_offset_and_clauses() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let hits0 = testing::topn_ext_hits();
    let ctx = topn_table(3, 70_000);
    let keys = ["a", "f", "s", "dg", "nn", "b"];
    let dirs = ["", " asc", " desc", " nulls first", " nulls last", " desc nulls first", " desc nulls last", " asc nulls last"];
    let mut r = Rng(17);
    for _ in 0..80 {
        let nk = 1 + r.below(3);
        let mut ks: Vec<String> = Vec::new();
        while ks.len() < nk {
            let k = keys[r.below(keys.len())];
            if !ks.iter().any(|x| x.starts_with(k)) { ks.push(format!("{k}{}", dirs[r.below(dirs.len())])); }
        }
        let lim = [1usize, 5, 20, 100][r.below(4)];
        let off = [0usize, 0, 3, 40][r.below(4)];
        let q = if off == 0 { format!("select id, a, f, s, b, dg from t order by {} limit {lim}", ks.join(", ")) }
                else { format!("select id, a, f, s, b, dg from t order by {} limit {lim} offset {off}", ks.join(", ")) };
        same(&ctx, &q);
    }
    assert!(testing::topn_ext_hits() > hits0, "the NULL-aware top-N was never used");
    // fewer rows than LIMIT + OFFSET, offset past the end
    let small = topn_table(4, 60_000);
    same(&small, "select id, a from t order by a desc nulls last, id limit 100000 offset 59990");
    same(&small, "select id, a from t order by a, id limit 3 offset 100000");
    same(&small, "select id, a from t where nn = 9 order by a limit 5 offset 2");
}
