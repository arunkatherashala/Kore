//! Phase 2 subquery operators against the paths they replace (on/off switch, exact result comparison):
//! correlated scalar aggregates evaluated straight from the catalog table, with NULL keys, duplicate keys,
//! empty inner/outer sides, missing matches (NULL / COUNT = 0), COUNT(*) vs COUNT(col), two correlation keys,
//! float keys, keys above 2^53 and sizes on both sides of the parallel threshold.

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

const BIG: i64 = (1i64 << 53) + 1_000;

fn inner(r: &mut Rng, n: usize, kdom: usize) -> DataBlock {
    DataBlock::new(vec![
        Column::int64("ik", (0..n).map(|_| if r.below(10) == 0 { None } else { Some(r.below(kdom) as i64) }).collect()),
        Column::int64("ik2", (0..n).map(|_| Some(r.below(3) as i64)).collect()),
        Column::int64("ibig", (0..n).map(|_| if r.below(10) == 0 { None } else { Some(BIG + r.below(kdom) as i64) }).collect()),
        Column::float64("ifk", (0..n).map(|_| Some(r.below(kdom) as f64 / 2.0)).collect()),
        Column::float64("iv", (0..n).map(|_| if r.below(5) == 0 { None } else { Some(r.below(100) as f64 / 4.0) }).collect()),
        Column::int64("iq", (0..n).map(|_| Some(r.below(50) as i64)).collect()),
    ]).unwrap()
}

fn outer(r: &mut Rng, n: usize, kdom: usize) -> DataBlock {
    DataBlock::new(vec![
        Column::int64("ok", (0..n).map(|_| if r.below(12) == 0 { None } else { Some(r.below(kdom + 5) as i64) }).collect()),
        Column::int64("ok2", (0..n).map(|_| Some(r.below(3) as i64)).collect()),
        Column::int64("obig", (0..n).map(|_| Some(BIG + r.below(kdom + 5) as i64)).collect()),
        Column::float64("ofk", (0..n).map(|_| Some(r.below(kdom + 5) as f64 / 2.0)).collect()),
        Column::float64("ov", (0..n).map(|_| Some(r.below(100) as f64 / 8.0)).collect()),
        Column::int64("oid", (0..n as i64).map(Some).collect()),
    ]).unwrap()
}

fn render(b: &DataBlock) -> Vec<String> {
    (0..b.num_rows).map(|i| b.columns.iter().map(|c| format!("{:?}", c.data.get_value(i))).collect::<Vec<_>>().join("|")).collect()
}

fn run(c: &KqlContext, q: &str, off: bool) -> Result<Vec<String>, String> {
    testing::set_no_subq(off);
    let out = c.query(q).map(|b| render(&b)).map_err(|e| format!("{e:?}"));
    testing::set_no_subq(false);
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

fn queries(big: bool) -> Vec<String> {
    let mut v = Vec::new();
    let aggs: &[&str] = if big { &["0.2 * avg(iq)", "count(iv)", "min(iv)"] } else { &["avg(iv)", "sum(iv)", "min(iv)", "max(iv)", "count(*)", "count(iv)", "0.2 * avg(iq)", "sum(iv * iq) / 3.0", "count(iv) + 1"] };
    for agg in aggs.iter().copied() {
        for filt in ["", " and iq < 40", " and iq > 1000", " and iv is not null and iq between 5 and 30"] {
            v.push(format!("select oid, ov, (select {agg} from inn where ik = ok{filt}) as s from outr order by oid"));
            v.push(format!("select oid from outr where ov < (select {agg} from inn where ik = ok{filt}) order by oid"));
            v.push(format!("select oid from outr where ov >= (select {agg} from inn i where i.ik = outr.ok and i.ik2 = ok2{filt}) order by oid"));
            v.push(format!("select oid, (select {agg} from inn where ibig = obig{filt}) as s from outr order by oid"));
            v.push(format!("select oid, (select {agg} from inn where ifk = ofk{filt}) as s from outr order by oid"));
        }
    }
    v
}

#[test]
fn correlated_scalar_direct_matches_materialised() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // (inner rows, outer rows, key domain)
    for (seed, (ni, no, kd)) in [(0usize, 0usize, 5usize), (50, 0, 5), (0, 40, 5), (300, 120, 20), (300, 120, 3), (70_000, 500, 400), (70_000, 800, 60_000)]
        .into_iter().enumerate()
    {
        let mut r = Rng(0x9e3779b97f4a7c15 ^ seed as u64);
        let mut c = KqlContext::new();
        c.register("inn", inner(&mut r, ni, kd));
        c.register("outr", outer(&mut r, no, kd));
        let before = testing::subq_direct_hits();
        for q in queries(ni > 10_000) { check(&c, &q); }
        if ni > 0 && no > 0 { assert!(testing::subq_direct_hits() > before, "direct path never taken for sizes {ni}/{no}"); }
    }
}
