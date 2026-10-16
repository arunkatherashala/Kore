//! Phase 3 gap closers against the paths they replace (on/off switch, exact result comparison on random tables with
//! NULL keys, duplicates, empty inputs, CTEs used twice, scalar subqueries without a match, NOT EXISTS with NULL keys).

use kore_core::{Column, DataBlock};
use kore_sql::{testing, KqlContext};
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize { (self.next() % n.max(1) as u64) as usize }
}

fn opt_i(r: &mut Rng, dom: usize, null_every: usize) -> Option<i64> { if null_every > 0 && r.below(null_every) == 0 { None } else { Some(r.below(dom) as i64) } }

/// a(id, k, v, s), b(bk, bk2, bq, bv), c(ck), d(dk, dk2, x), t(tk, tv)
fn make_ctx(r: &mut Rng, na: usize, nb: usize, nc: usize, nd: usize, dom: usize) -> KqlContext {
    let mut ctx = KqlContext::new();
    ctx.register("a", DataBlock::new(vec![
        Column::int64("id", (0..na as i64).map(Some).collect()),
        Column::int64("k", (0..na).map(|_| opt_i(r, dom + 3, 9)).collect()),
        Column::float64("v", (0..na).map(|_| Some(r.below(100) as f64 / 4.0)).collect()),
        Column::str_col("s", (0..na).map(|_| if r.below(8) == 0 { None } else { Some(format!("n{}", r.below(5))) }).collect()),
    ]).unwrap());
    ctx.register("b", DataBlock::new(vec![
        Column::int64("bk", (0..nb).map(|_| opt_i(r, dom, 10)).collect()),
        Column::int64("bk2", (0..nb).map(|_| opt_i(r, 3, 15)).collect()),
        Column::int64("bq", (0..nb).map(|_| opt_i(r, dom, 12)).collect()),
        Column::float64("bv", (0..nb).map(|_| if r.below(9) == 0 { None } else { Some(r.below(100) as f64 / 4.0) }).collect()),
    ]).unwrap());
    ctx.register("c", DataBlock::new(vec![
        Column::int64("ck", (0..nc).map(|_| opt_i(r, dom, 8)).collect()),
    ]).unwrap());
    ctx.register("d", DataBlock::new(vec![
        Column::int64("dk", (0..nd).map(|_| opt_i(r, dom, 10)).collect()),
        Column::int64("dk2", (0..nd).map(|_| opt_i(r, 3, 15)).collect()),
        Column::float64("x", (0..nd).map(|_| if r.below(6) == 0 { None } else { Some(r.below(50) as f64) }).collect()),
    ]).unwrap());
    // nat(nk unique, nm text with duplicates / NULLs; above 250 distinct values when the key domain is large)
    let nn = dom + 3;
    let modv = if dom > 300 { 100_000 } else { 7 };
    ctx.register("nat", DataBlock::new(vec![
        Column::int64("nk", (0..nn as i64).map(Some).collect()),
        Column::str_col("nm", (0..nn).map(|i| if i % 11 == 5 { None } else { Some(format!("nm{}", i % modv)) }).collect()),
        Column::int64("nr", (0..nn as i64).map(|i| Some(i % 3)).collect()),
    ]).unwrap());
    ctx
}

fn render(b: &DataBlock) -> Vec<String> {
    (0..b.num_rows).map(|i| b.columns.iter().map(|c| format!("{:?}", c.data.get_value(i))).collect::<Vec<_>>().join("|")).collect()
}

fn run(c: &KqlContext, q: &str, off: bool) -> Result<Vec<String>, String> {
    testing::set_no_gap(off);
    let out = c.query(q).map(|b| render(&b)).map_err(|e| format!("{e:?}"));
    testing::set_no_gap(false);
    out
}

fn check(c: &KqlContext, q: &str) {
    let want = run(c, q, true);
    let got = run(c, q, false);
    match (want, got) {
        (Ok(w), Ok(g)) => {
            assert_eq!(w.len(), g.len(), "row count differs: {q}");
            if let Some(i) = (0..w.len()).find(|&i| w[i] != g[i]) { panic!("row {i} differs: {q}\noff={}\non={}", w[i], g[i]); }
        }
        (Err(_), Err(_)) => {}
        (w, g) => panic!("error mismatch {q}: off ok={} on ok={}", w.is_ok(), g.is_ok()),
    }
}

const SIZES: [(usize, usize, usize, usize, usize); 6] = [(0, 0, 0, 0, 5), (30, 0, 5, 0, 5), (0, 40, 5, 40, 5), (60, 200, 20, 300, 12), (60, 200, 3, 300, 3), (3000, 20000, 400, 30000, 500)];

fn for_all(queries: &[&str]) {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for (seed, (na, nb, nc, nd, dom)) in SIZES.into_iter().enumerate() {
        let mut r = Rng(0x9e3779b97f4a7c15 ^ (seed as u64 * 7919 + 1));
        let ctx = make_ctx(&mut r, na, nb, nc, nd, dom);
        for q in queries { check(&ctx, q); }
    }
}

#[test]
fn nested_in_subquery_is_precomputed_once() {
    for_all(&[
        // TPC-H Q20 shape
        "select id from a where k in (select bk from b where bq in (select ck from c) and bv > (select 0.5 * sum(x) from d where dk = bk and dk2 = bk2)) order by id",
        "select id from a where k not in (select bk from b where bq in (select ck from c) and bv > (select 0.5 * sum(x) from d where dk = bk)) order by id",
        "select id from a where k in (select bk from b where bv > (select 0.5 * sum(x) from d where dk = bk and x > 10)) and v > 5 order by id",
        "select id, s from a where k in (select bk from b where exists (select * from d where dk = bk)) order by id",
        "select id from a where k in (select bk from b where bq in (select ck from c where ck > 1)) order by id",
        // a subquery that really is correlated to the outer query must stay correlated
        "select id from a where k in (select bk from b where bq in (select ck from c where ck = a.k)) order by id",
        "select id from a where v in (select bv from b where bk in (select dk from d where dk = k)) order by id",
        "select id from a where k in (select bk from b where bv > (select 0.5 * sum(x) from d where dk = bk and x < v)) order by id",
    ]);
}

#[test]
fn small_table_text_through_joins_and_projection() {
    for_all(&[
        "select nm, count(*) as c, sum(v) as sv from a, nat where k = nk group by nm order by nm",
        "select nm as nation, substr(s, 1, 2) as y, v * 2 as w, id from a, nat where k = nk order by id",
        "select nation, y, sum(w) as t from (select nm as nation, substr(s, 1, 2) as y, v * 2 as w from a, nat where k = nk) as p group by nation, y order by nation, y desc",
        "select nm, s, count(*) from a, nat, b where k = nk and bk = nk group by nm, s order by nm, s",
        "select nm from a, nat where k = nk and nm like 'nm1%' order by nm, id",
        "select nm, nr, id from a, nat where k = nk and nr = 1 order by id",
        "select id, nm, nm as again from a left join nat on k = nk order by id",
        "select id, k, nm from a, nat where k = nk and nm is null order by id",
        "select nm, count(*) from nat group by nm order by nm",
        "select id, nm, k, nk from a, nat where k = nk order by id desc limit 7",
    ]);
}

#[test]
fn projection_moves_plain_columns() {
    for_all(&[
        "select id, k, v + 1 as w, s from a order by id",
        "select s as t, v * 2 as v2, s from a order by id",
        "select id as v, v as id2, id + 1 as v from a order by id2",
        "select k, k + 1 as k1, k from a order by id",
        "select s, upper(s) as u, id from a where k > 1 order by id",
        "select id as a1, id, id * 2 as id from a order by a1",
    ]);
}

fn date_s(r: &mut Rng, bad: bool) -> Option<String> {
    if r.below(20) == 0 { return None; }
    if bad && r.below(5000) == 0 { return Some("not-a-date".into()); }
    Some(format!("199{}-{:02}-{:02}", 2 + r.below(5), 1 + r.below(12), 1 + r.below(28)))
}

/// li(lk order key, ls supplier, cd, rd dates), sup(sk, sn), ord(ok, st)
fn lineitem_ctx(r: &mut Rng, n: usize, bad: bool) -> KqlContext {
    let mut ctx = KqlContext::new();
    let nord = (n / 4).max(1);
    ctx.register("li", DataBlock::new(vec![
        Column::int64("lk", (0..n).map(|_| if r.below(50) == 0 { None } else { Some(r.below(nord) as i64) }).collect()),
        Column::int64("ls", (0..n).map(|_| if r.below(50) == 0 { None } else { Some(r.below(200) as i64) }).collect()),
        Column::str_col("cd", (0..n).map(|_| date_s(r, bad)).collect()),
        Column::str_col("rd", (0..n).map(|_| date_s(r, bad)).collect()),
    ]).unwrap());
    ctx.register("sup", DataBlock::new(vec![
        Column::int64("sk", (0..200i64).map(Some).collect()),
        Column::int64("sn", (0..200i64).map(|i| Some(i % 25)).collect()),
    ]).unwrap());
    ctx.register("ord", DataBlock::new(vec![
        Column::int64("ok", (0..nord as i64).map(Some).collect()),
        Column::str_col("st", (0..nord).map(|i| Some(if i % 3 == 0 { "F".to_string() } else { "O".to_string() })).collect()),
    ]).unwrap());
    ctx
}

#[test]
fn semi_scan_fuses_filter_and_sets_use_date_accel() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let qs = [
        // TPC-H Q21 shape
        "select sk, count(*) as c from sup, li l1, ord where sk = l1.ls and ok = l1.lk and st = 'F' and l1.rd > l1.cd and sn = 3 \
         and exists (select * from li l2 where l2.lk = l1.lk and l2.ls <> l1.ls) \
         and not exists (select * from li l3 where l3.lk = l1.lk and l3.ls <> l1.ls and l3.rd > l3.cd) group by sk order by c desc, sk",
        // TPC-H Q4 shape
        "select st, count(*) as c from ord where exists (select * from li where lk = ok and rd > cd) group by st order by st",
        "select st, count(*) as c from ord where not exists (select * from li where lk = ok and rd > cd and ls < 50) group by st order by st",
        "select count(*) from ord where exists (select * from li where lk = ok and cd >= '1994-01-01' and cd < '1995-01-01')",
        "select count(*) from sup, li l1 where sk = l1.ls and l1.rd > l1.cd and sn = 7",
        "select count(*) from sup, li l1 where sk = l1.ls and l1.rd > l1.cd and substr(l1.cd, 1, 4) = '1995' and sn = 7",
        // a predicate the vectorised evaluator may not cover: upper() on text
        "select count(*) from ord where exists (select * from li where lk = ok and upper(cd) > '1994')",
        "select st, count(*) as c from ord where exists (select * from li where lk = ok and rd > cd and ls <> 5) group by st order by st",
    ];
    for (seed, (n, bad)) in [(0usize, false), (300, false), (260_000, false), (260_000, true)].into_iter().enumerate() {
        let mut r = Rng(0xabcdef12345 + seed as u64);
        let ctx = lineitem_ctx(&mut r, n, bad);
        for q in qs { check(&ctx, q); }
    }
    assert!(testing::semi_fuse_hits() > 0, "the fused semi-join scan never ran");
}

#[test]
fn substr_of_payload_text_is_computed_at_the_source() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let qs = [
        "select nm as nation, substr(s, 1, 2) as sy, v * 2 as w from a, nat where k = nk order by nation, sy, w, nm",
        "select substr(s, 2) as t, id from a, nat where k = nk and nr = 1 order by id",
        "select substr(s, 1, 1) as t, s from a, nat where k = nk order by s, t, nm",
        "select id, substr(s, 1, 3) as t, substr(s, 2, 1) as u from a, nat where k = nk order by id",
        "select id, substr(s, 1, 3) as id from a, nat where k = nk order by id",
        "select id, substr(s, 1, 3) as t from a, nat where k = nk and s like 'n1%' order by id",
        "select id, substr(s, 1, 3) as t from a, nat where k = nk order by t, id",
        "select substr(s, 1, 4) as t, count(*) as c from a, nat where k = nk group by substr(s, 1, 4) order by t",
        "select id, substr(s, 1, 2) as t from a, nat, b where k = nk and bk = nk and bv > 20 order by id, t",
        "select nation, y, sum(w) as t from (select nm as nation, substr(s, 1, 2) as y, v * 2 as w from a, nat where k = nk) as p group by nation, y order by nation, y desc",
    ];
    for (seed, (na, nb, nc, nd, dom)) in SIZES.into_iter().enumerate() {
        let mut r = Rng(0x1357 ^ (seed as u64 * 31 + 5));
        let mut ctx = make_ctx(&mut r, na, nb, nc, nd, dom);
        // a larger, low-cardinality text payload so that the SUBSTR dictionary applies
        if na >= 3000 {
            ctx.register("a", DataBlock::new(vec![
                Column::int64("id", (0..na as i64).map(Some).collect()),
                Column::int64("k", (0..na).map(|_| opt_i(&mut r, dom + 3, 9)).collect()),
                Column::float64("v", (0..na).map(|_| Some(r.below(100) as f64 / 4.0)).collect()),
                Column::str_col("s", (0..na).map(|_| if r.below(8) == 0 { None } else { Some(format!("{}-{:02}-x", 1992 + r.below(7), 1 + r.below(12))) }).collect()),
            ]).unwrap());
        }
        for q in qs { check(&ctx, q); }
    }
    assert!(testing::lz_substr_hits() > 0, "the SUBSTR pushdown never ran");
}
