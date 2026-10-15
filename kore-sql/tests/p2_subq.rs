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

// ---- LEFT JOIN + COUNT + GROUP BY (Q13 shape) ----

fn lj_tables(r: &mut Rng, nc: usize, no: usize, unique: bool, with_null: bool) -> (DataBlock, DataBlock) {
    let mut keys: Vec<i64> = (0..nc as i64).map(|k| k * 3 + 10).collect();
    for i in (1..keys.len()).rev() { let j = r.below(i + 1); keys.swap(i, j); }
    let cust = DataBlock::new(vec![
        Column::int64("ck", keys.iter().enumerate().map(|(i, &k)| {
            if with_null && i % 17 == 3 { None } else if !unique && i % 11 == 0 { Some(10) } else { Some(k) }
        }).collect()),
        Column::int64("cv", (0..nc).map(|_| Some(r.below(5) as i64)).collect()),
    ]).unwrap();
    let ord = DataBlock::new(vec![
        Column::int64("ok", (0..no).map(|_| if r.below(10) == 0 { None } else { Some((r.below(nc + 20) * 3 + 10) as i64 - r.below(2) as i64) }).collect()),
        Column::int64("oid", (0..no).map(|i| if r.below(6) == 0 { None } else { Some(i as i64) }).collect()),
        Column::float64("ox", (0..no).map(|_| if r.below(4) == 0 { None } else { Some(r.below(9) as f64) }).collect()),
        Column::str_col("os", (0..no).map(|_| if r.below(5) == 0 { None } else { Some(["special requests", "plain", "x"][r.below(3)].to_string()) }).collect()),
    ]).unwrap();
    (cust, ord)
}

#[test]
fn left_join_count_matches_joined_rows() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let qs = [
        "select ck, count(oid) as n from cust left outer join ord on ck = ok group by ck order by ck",
        "select ck, count(*) as n from cust left join ord on ck = ok group by ck order by ck",
        "select ck, count(ox) as a, count(os) as b, count(*) as c from cust left join ord on ok = ck and os not like '%special%' group by ck order by ck",
        "select ck as k, count(oid) as n from cust c left join ord o on c.ck = o.ok and o.ox > 3 and o.oid is not null group by ck order by k",
        "select n, count(*) as d from (select ck, count(oid) as n from cust left join ord on ck = ok and os <> 'plain' group by ck) t group by n order by d desc, n desc",
        "select ck, count(oid) as n from cust left join ord on ck = ok and ox > 100 group by ck order by ck",
        "select cv, count(oid) as n from cust left join ord on ck = ok group by cv order by cv",
    ];
    for (seed, (nc, no)) in [(0usize, 30usize), (40, 0), (50, 200), (1000, 90_000), (70_000, 150_000)].into_iter().enumerate() {
        for (unique, with_null) in [(true, false), (false, false), (true, true)] {
            let mut r = Rng(77 + seed as u64);
            let (cust, ord) = lj_tables(&mut r, nc, no, unique, with_null);
            let mut c = KqlContext::new();
            c.register("cust", cust);
            c.register("ord", ord);
            let before = testing::subq_leftcount_hits();
            for q in qs { check(&c, q); }
            if nc > 0 && no > 0 && unique && !with_null { assert!(testing::subq_leftcount_hits() > before, "fused path never taken for {nc}/{no}"); }
        }
    }
}

#[test]
fn correlated_scalar_sets_first_matches() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // a big inner table (>= 200k rows) and few outer keys: the key set restricts the inner rows before the filter runs
    let mut r = Rng(4242);
    let mut c = KqlContext::new();
    c.register("inn", inner(&mut r, 210_000, 100_000));
    c.register("outr", outer(&mut r, 300, 100_000));
    let before = testing::plan_sets_first_loads();
    for agg in ["0.5 * sum(iv)", "count(*)", "avg(iq)", "max(iv)"] {
        for filt in ["", " and iq < 40", " and iv is not null and iq between 5 and 30", " and iq > 1000"] {
            check(&c, &format!("select oid, (select {agg} from inn where ik = ok{filt}) as s from outr order by oid"));
            check(&c, &format!("select oid from outr where ov < (select {agg} from inn where ik = ok{filt}) order by oid"));
        }
    }
    assert!(testing::plan_sets_first_loads() > before, "key-set-first restriction never used");
}
