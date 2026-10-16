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
