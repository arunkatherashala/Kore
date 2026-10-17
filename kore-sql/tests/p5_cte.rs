//! Phase 5 CTE / semi-join work against the paths it replaces (on/off switch, exact result comparison on random tables with
//! NULL keys, NULL x values, duplicates, empty sides and single-row groups).

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

/// ws(o, w, v, id) : orders with warehouses; wr(ro, rv); cust(id2, o2)
fn make_ctx(r: &mut Rng, nws: usize, nwr: usize, odom: usize, wdom: usize) -> KqlContext {
    let mut ctx = KqlContext::new();
    ctx.register("ws", DataBlock::new(vec![
        Column::int64("o", (0..nws).map(|_| opt_i(r, odom, 10)).collect()),
        Column::int64("w", (0..nws).map(|_| opt_i(r, wdom, 6)).collect()),
        Column::float64("v", (0..nws).map(|_| Some(r.below(100) as f64 / 4.0)).collect()),
        Column::int64("id", (0..nws as i64).map(Some).collect()),
    ]).unwrap());
    ctx.register("wr", DataBlock::new(vec![
        Column::int64("ro", (0..nwr).map(|_| opt_i(r, odom + 2, 8)).collect()),
        Column::float64("rv", (0..nwr).map(|_| Some(r.below(40) as f64)).collect()),
    ]).unwrap());
    ctx
}

fn render(b: &DataBlock) -> Vec<String> {
    let mut v: Vec<String> = (0..b.num_rows).map(|i| b.columns.iter().map(|c| format!("{:?}", c.data.get_value(i))).collect::<Vec<_>>().join("|")).collect();
    v.sort();
    v
}

fn run(c: &KqlContext, q: &str, off: bool) -> Result<Vec<String>, String> {
    testing::set_no_semi_cte(off);
    let out = c.query(q).map(|b| render(&b)).map_err(|e| format!("{e:?}"));
    testing::set_no_semi_cte(false);
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

// (rows of ws, rows of wr, order domain, warehouse domain)
const SIZES: [(usize, usize, usize, usize); 7] = [(0, 0, 5, 3), (40, 0, 8, 3), (0, 30, 8, 3), (1, 3, 1, 1), (60, 40, 30, 1), (200, 120, 40, 4), (4000, 1500, 900, 5)];

fn for_all(queries: &[&str]) -> usize {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let hits0 = testing::semi_cte_hits();
    for (seed, (nws, nwr, odom, wdom)) in SIZES.into_iter().enumerate() {
        let mut r = Rng(0x9e3779b97f4a7c15 ^ (seed as u64 * 7919 + 1));
        let ctx = make_ctx(&mut r, nws, nwr, odom, wdom);
        for q in queries { check(&ctx, q); }
    }
    testing::semi_cte_hits() - hits0
}

const CTE: &str = "with ws_wh as (select ws1.o, ws1.w wh1, ws2.w wh2 from ws ws1, ws ws2 where ws1.o = ws2.o and ws1.w <> ws2.w) ";

#[test]
fn q95_shape_both_in_contexts() {
    let hits = for_all(&[
        &format!("{CTE} select count(distinct id) c, sum(v) s from ws where o in (select o from ws_wh) and o in (select ro from wr, ws_wh where ro = ws_wh.o)"),
        &format!("{CTE} select id from ws where o in (select o from ws_wh) order by id"),
        &format!("{CTE} select id from ws where o in (select ro from wr, ws_wh where ro = ws_wh.o) order by id"),
        &format!("{CTE} select id, v from ws where o not in (select o from ws_wh) order by id"),
        &format!("{CTE} select id from ws where o in (select o from ws_wh where o > 3) and v > 5 order by id"),
        &format!("{CTE} select id from ws where o in (select x.o from ws_wh x, ws_wh y where x.o = y.o) order by id"),
        &format!("{CTE} select ro from wr where ro in (select o from ws_wh) and ro in (select ro from wr, ws_wh where ro = ws_wh.o) order by ro"),
        &format!("{CTE} select id from ws where o in (select o from ws_wh) or v > 20 order by id"),
        // other aliases and a swapped comparison
        "with p as (select b.o as ord, a.w as w1, b.w as w2 from ws a, ws b where b.w <> a.w and b.o = a.o) select id from ws where o in (select ord from p) order by id",
    ]);
    assert!(hits > 0, "the semi rewrite never ran");
}

#[test]
fn shapes_that_must_keep_the_pairs() {
    let hits = for_all(&[
        // reads another column of the CTE
        &format!("{CTE} select id from ws where o in (select o from ws_wh where wh1 > 1) order by id"),
        &format!("{CTE} select id from ws where o in (select o from ws_wh where ws_wh.wh2 is not null) order by id"),
        // multiplicity visible
        &format!("{CTE} select count(*) from ws_wh"),
        &format!("{CTE} select o, count(*) c from ws_wh group by o order by o"),
        &format!("{CTE} select id from ws, ws_wh where ws.o = ws_wh.o order by id"),
        &format!("{CTE} select id from ws where o in (select count(*) from ws_wh) order by id"),
        &format!("{CTE} select id from ws where o in (select o from ws_wh limit 2) order by id"),
        &format!("{CTE} select id from ws where exists (select 1 from ws_wh where ws_wh.o = ws.o) order by id"),
        // different pair predicates
        "with p as (select a.o, a.w, b.w w2 from ws a, ws b where a.o = b.o and a.w < b.w) select id from ws where o in (select o from p) order by id",
        "with p as (select a.o, a.w, b.w w2 from ws a, ws b where a.o = b.o and a.w <> b.w and a.v > 3) select id from ws where o in (select o from p) order by id",
    ]);
    assert_eq!(hits, 0, "a rewrite fired where the pair rows are observable");
}
