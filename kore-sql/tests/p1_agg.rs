//! Wider fused GROUP BY (more than three key columns, float / text keys, sorted-key runs), SUBSTR dictionary
//! columns and the projection that moves columns instead of cloning them must give exactly the answers of the
//! older paths. Every optimisation sits behind a `testing::set_no_*` switch; each query is run with all of them
//! off and all of them on and the rows (including group order) are compared.
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
    testing::set_no_wide_agg(!on);
    testing::set_no_sorted_agg(!on);
    testing::set_no_substr_dict(!on);
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

/// Same rows in the same order with the new paths off and on (errors must agree too).
fn same(ctx: &KqlContext, q: &str) {
    new_paths(false);
    let old = rows(ctx, q);
    new_paths(true);
    let new = rows(ctx, q);
    new_paths(true);
    match (old, new) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a.len(), b.len(), "row count differs for {q}");
            for (i, (x, y)) in a.iter().zip(&b).enumerate() { assert_eq!(x, y, "row {i} differs for {q}"); }
        }
        (Err(_), Err(_)) => {}
        (a, b) => panic!("one path failed for {q}: old ok={} new ok={}", a.is_ok(), b.is_ok()),
    }
}

/// `ok` sorted (runs of 1..5 equal keys, so runs cross every chunk boundary somewhere), the rest random.
fn fact(n: usize, seed: u64, sorted_nulls: bool) -> DataBlock {
    let mut r = Rng(seed);
    let mut cur = 0i64;
    let mut left = 0usize;
    let ok: Vec<Option<i64>> = (0..n).map(|i| {
        if left == 0 { cur += 1 + r.below(3) as i64; left = 1 + r.below(5); }
        left -= 1;
        if sorted_nulls && i == n / 3 { None } else { Some(cur) }
    }).collect();
    let k: Vec<Option<i64>> = (0..n).map(|_| if r.below(40) == 0 { None } else { Some(r.below((n / 10).max(1)) as i64 - 5) }).collect();
    let k2: Vec<Option<i64>> = (0..n).map(|_| Some(r.below(3) as i64)).collect();
    let flv = [0.0, -0.0, 1.5, 2.25, -7.125, 1e10];
    let fl: Vec<Option<f64>> = (0..n).map(|_| if r.below(15) == 0 { None } else { Some(flv[r.below(flv.len())]) }).collect();
    let s: Vec<Option<String>> = (0..n).map(|_| if r.below(25) == 0 { None } else { Some(format!("v{}", r.below(50))) }).collect();
    let s2: Vec<Option<String>> = (0..n).map(|_| Some(format!("x{}-long-text-{}", r.below((n / 6).max(1)), r.below(2)))).collect();
    let dg: Vec<u8> = (0..n).map(|_| { let x = r.below(4); if x == 3 { u8::MAX } else { x as u8 } }).collect();
    let i2: Vec<Option<i64>> = (0..n).map(|_| Some(r.below(8) as i64)).collect();
    let v: Vec<Option<i64>> = (0..n).map(|_| if r.below(20) == 0 { None } else { Some(r.below(1000) as i64 - 300) }).collect();
    let w: Vec<Option<f64>> = (0..n).map(|_| if r.below(20) == 0 { None } else { Some(r.below(100_000) as f64 / 100.0) }).collect();
    let qty: Vec<Option<f64>> = (0..n).map(|_| Some(r.below(50) as f64)).collect();
    DataBlock::new(vec![
        Column::int64("ok", ok), Column::int64("k", k), Column::int64("k2", k2), Column::float64("fl", fl),
        Column::str_col("s", s), Column::str_col("s2", s2),
        Column::str_dict("dg", dg, vec!["a".into(), "b".into(), "c".into()]),
        Column::int64("i2", i2), Column::int64("v", v), Column::float64("w", w), Column::float64("qty", qty),
    ]).unwrap()
}

const GROUP_POOL: [&str; 8] = ["ok", "k", "k2", "fl", "s", "s2", "dg", "i2"];
const AGG_POOL: [&str; 13] = [
    "sum(v)", "sum(w)", "avg(w)", "avg(v)", "count(*)", "count(v)", "count(w)", "min(v)", "max(w)", "min(w)", "max(v)",
    "count(distinct v)", "sum(w * (1 - qty / 100))",
];
const WHERE_POOL: [&str; 6] = ["", "where i2 < 6", "where w > 10", "where s <> 'v3'", "where k is not null and k2 > 0", "where qty < 0"];
const HAVING_POOL: [&str; 4] = ["", "", "having count(*) > 1", "having sum(w) > 100"];

fn random_query(r: &mut Rng) -> String {
    let ng = 1 + r.below(8);
    let mut gs: Vec<&str> = Vec::new();
    while gs.len() < ng { let c = GROUP_POOL[r.below(GROUP_POOL.len())]; if !gs.contains(&c) { gs.push(c); } }
    let na = 1 + r.below(4);
    let mut aggs: Vec<String> = Vec::new();
    while aggs.len() < na { let a = AGG_POOL[r.below(AGG_POOL.len())]; if !aggs.iter().any(|x| x.starts_with(a)) { aggs.push(format!("{a} as a{}", aggs.len())); } }
    format!("select {}, {} from f {} group by {} {}", gs.join(", "), aggs.join(", "), WHERE_POOL[r.below(WHERE_POOL.len())], gs.join(", "), HAVING_POOL[r.below(HAVING_POOL.len())])
}

#[test]
fn random_group_by_shapes() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = Rng(99);
    for (n, seed, queries) in [(60_000usize, 1u64, 90usize), (5_000, 2, 50), (200, 3, 30), (130_000, 4, 40)] {
        let mut ctx = KqlContext::new();
        ctx.register("f", fact(n, seed, false));
        for _ in 0..queries { same(&ctx, &random_query(&mut r)); }
    }
}

#[test]
fn group_counts_one_to_eight() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ctx = KqlContext::new();
    ctx.register("f", fact(70_000, 5, false));
    for ng in 1..=8usize {
        let g = GROUP_POOL[..ng].join(", ");
        same(&ctx, &format!("select {g}, sum(w) as sw, count(*) as c, min(v) as mn, avg(v) as av from f group by {g}"));
        same(&ctx, &format!("select {g}, count(*) as c from f where i2 < 5 group by {g} having count(*) > 1"));
        // keys listed in another order than they are selected
        let rev: Vec<&str> = GROUP_POOL[..ng].iter().rev().cloned().collect();
        same(&ctx, &format!("select {g}, sum(v) as sv from f group by {}", rev.join(", ")));
    }
}

#[test]
fn sorted_key_runs() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // sorted: runs cross chunk boundaries; NULL inside a sorted column and a late inversion must not be treated as sorted
    for (name, tbl) in [("sorted", fact(300_000, 6, false)), ("sorted_with_null", fact(300_000, 7, true))] {
        let mut ctx = KqlContext::new();
        ctx.register("f", tbl);
        for q in [
            "select ok, sum(w) as sw, count(*) as c, min(v) as mn, max(w) as mx, avg(v) as a from f group by ok",
            "select ok, sum(w) as sw from f where i2 < 3 group by ok",
            "select ok, sum(qty) as sq from f group by ok having sum(qty) > 150",
            "select ok, count(v) as c, count(*) as n from f where k2 = 1 group by ok",
        ] {
            same(&ctx, q);
        }
        let _ = name;
    }
    // inversion at the very end
    let mut t = fact(300_000, 8, false);
    if let ColumnData::Int64(v) = &mut t.columns[0].data { let n = v.len(); v[n - 1] = Some(0); }
    let mut ctx = KqlContext::new();
    ctx.register("f", t);
    same(&ctx, "select ok, sum(w) as sw, count(*) as c from f group by ok");
    // all equal and strictly increasing keys
    let n = 200_000;
    let mut ctx = KqlContext::new();
    ctx.register("a", DataBlock::new(vec![Column::int64("ok", vec![Some(7); n]), Column::float64("w", (0..n).map(|i| Some(i as f64)).collect())]).unwrap());
    ctx.register("b", DataBlock::new(vec![Column::int64("ok", (0..n as i64).map(Some).collect()), Column::float64("w", (0..n).map(|i| Some(i as f64)).collect())]).unwrap());
    same(&ctx, "select ok, sum(w) as sw, count(*) as c from a group by ok");
    same(&ctx, "select ok, sum(w) as sw, count(*) as c from b group by ok");
}

#[test]
fn substr_dictionary_columns() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let n = 40_000;
    let mut r = Rng(11);
    let dates: Vec<Option<String>> = (0..n).map(|_| if r.below(30) == 0 { None } else { Some(format!("{}-{:02}-{:02}", 1990 + r.below(9), 1 + r.below(12), 1 + r.below(28))) }).collect();
    // more than 254 distinct prefixes: falls back to String columns
    let many: Vec<Option<String>> = (0..n).map(|_| Some(format!("{:05}-é", r.below(2000)))).collect();
    let uni: Vec<Option<String>> = (0..n).map(|_| Some(["éàü-1", "ab", "", "日本語テキスト", "x"][r.below(5)].to_string())).collect();
    let mut ctx = KqlContext::new();
    ctx.register("t", DataBlock::new(vec![
        Column::str_col("d", dates), Column::str_col("m", many), Column::str_col("u", uni),
        Column::float64("w", (0..n).map(|i| Some((i % 97) as f64)).collect()),
    ]).unwrap());
    for q in [
        "select substr(d, 1, 4) as y, sum(w) as s from t group by substr(d, 1, 4)",
        "select y, sum(w) as s, count(*) as c from (select substr(d, 1, 4) as y, w from t) as x group by y",
        "select y, sum(w) as s from (select substr(d, 6, 2) as y, w from t) as x group by y order by y desc",
        "select y, count(*) as c from (select substr(m, 1, 3) as y from t) as x group by y",
        "select y, count(*) as c from (select substr(m, 2) as y from t) as x group by y",
        "select y, count(*) as c from (select substr(u, 1, 2) as y from t) as x group by y",
        "select y, count(*) as c from (select substr(u, 2, 0) as y from t) as x group by y",
        "select y, count(*) as c from (select substr(u, 50, 3) as y from t) as x group by y",
        "select y, z, sum(w) as s from (select substr(d, 1, 4) as y, substr(d, 6, 2) as z, w from t) as x group by y, z",
        "select substr(d, 1, 4) as y, w from t where w < 3",
        "select y, s from (select substr(d, 1, 7) as y, sum(w) as s from t group by substr(d, 1, 7)) as q where s > 100 order by y",
    ] {
        same(&ctx, q);
    }
}

/// Join shapes: the grouped table joined to a dimension with unique keys and to one with duplicate keys
/// (duplicates multiply rows, so nothing may be folded below such a join).
#[test]
fn join_shapes() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = Rng(21);
    let nf = 80_000usize;
    let nd = 3_000usize;
    let mut ctx = KqlContext::new();
    let mut f = fact(nf, 12, false);
    // join column drawn from the dimension keys plus a few misses and NULLs
    let jk: Vec<Option<i64>> = (0..nf).map(|_| if r.below(30) == 0 { None } else { Some(r.below(nd + 200) as i64) }).collect();
    f.columns.push(Column::int64("jk", jk));
    ctx.register("f", f);
    let dk: Vec<Option<i64>> = (0..nd as i64).map(Some).collect();
    let dname: Vec<Option<String>> = (0..nd).map(|i| Some(format!("name{}", i % 40))).collect();
    let dval: Vec<Option<f64>> = (0..nd).map(|i| if i % 17 == 0 { None } else { Some((i % 13) as f64) }).collect();
    ctx.register("du", DataBlock::new(vec![Column::int64("dk", dk.clone()), Column::str_col("dname", dname.clone()), Column::float64("dval", dval.clone())]).unwrap());
    // duplicate keys: every key twice, and some keys 3 times, plus NULL keys
    let dup_k: Vec<Option<i64>> = (0..nd as i64).flat_map(|i| { let c = if i % 5 == 0 { 3 } else { 2 }; std::iter::repeat(Some(i)).take(c) }).chain([None, None]).collect();
    let m = dup_k.len();
    ctx.register("dd", DataBlock::new(vec![
        Column::int64("dk", dup_k),
        Column::str_col("dname", (0..m).map(|i| Some(format!("name{}", i % 40))).collect()),
        Column::float64("dval", (0..m).map(|i| Some((i % 7) as f64)).collect()),
    ]).unwrap());
    ctx.register("de", DataBlock::new(vec![Column::int64("dk", vec![]), Column::str_col("dname", vec![]), Column::float64("dval", vec![])]).unwrap());
    for dim in ["du", "dd", "de"] {
        for q in [
            format!("select jk, sum(w) as sw, count(*) as c from f, {dim} where jk = dk group by jk"),
            format!("select ok, sum(w) as sw, count(*) as c, min(v) as mn from f, {dim} where jk = dk group by ok"),
            format!("select ok, dname, sum(w * (1 - qty / 100)) as sw, avg(v) as av from f, {dim} where jk = dk and i2 < 6 group by ok, dname"),
            format!("select ok, dname, dval, s, k2, sum(w) as sw, count(v) as cv from f, {dim} where jk = dk group by ok, dname, dval, s, k2"),
            format!("select dname, sum(w) as sw, count(*) as c from f, {dim} where jk = dk group by dname"),
            format!("select dk, dname, sum(w) as sw, count(*) as c from f, {dim} where jk = dk group by dk, dname having count(*) > 2"),
            format!("select ok, jk, i2, s, dname, sum(v) as sv, max(w) as mw from f, {dim} where jk = dk and w > 20 group by ok, jk, i2, s, dname"),
            format!("select ok, count(distinct v) as cd, sum(w) as sw from f, {dim} where jk = dk group by ok"),
        ] {
            same(&ctx, &q);
        }
    }
}

#[test]
fn empty_and_tiny_inputs() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for n in [0usize, 1, 2, 7] {
        let mut ctx = KqlContext::new();
        ctx.register("f", fact(n, 30 + n as u64, false));
        for q in [
            "select ok, k, k2, fl, s, s2, dg, i2, sum(w) as sw, count(*) as c from f group by ok, k, k2, fl, s, s2, dg, i2",
            "select ok, sum(w) as sw from f group by ok",
            "select s, fl, sum(w) as sw from f where i2 > 100 group by s, fl",
        ] {
            same(&ctx, q);
        }
    }
}

/// The tests above are only meaningful if the new paths really ran; run last (names sort after the others is not
/// guaranteed, so it runs a few of the shapes itself).
#[test]
fn new_paths_are_exercised() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    new_paths(true);
    let (w0, s0, d0) = testing::agg_hits();
    let mut ctx = KqlContext::new();
    ctx.register("f", fact(120_000, 40, false));
    rows(&ctx, "select ok, k, k2, fl, s, sum(w) as sw from f group by ok, k, k2, fl, s").unwrap();
    rows(&ctx, "select ok, sum(w) as sw from f group by ok").unwrap();
    rows(&ctx, "select y, count(*) as c from (select substr(s2, 1, 2) as y from f) as x group by y").unwrap();
    let (w1, s1, d1) = testing::agg_hits();
    assert!(w1 > w0 && s1 > s0 && d1 > d0, "wide {w0}->{w1}, sorted {s0}->{s1}, substr dict {d0}->{d1}");
}
