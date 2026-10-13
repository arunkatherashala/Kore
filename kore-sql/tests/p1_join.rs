//! Parallel join mechanics (partitioned hash build, parallel concatenation / gather / composite key / lazy
//! pre-steps) against the original serial code: same rows in the same order, for comma joins (lazy and composite
//! paths), explicit INNER / LEFT / RIGHT / FULL joins, NULL keys, duplicate keys, empty sides, keys above 2^53 and
//! sizes on both sides of the parallel thresholds (64k gather, 128k hash build, 32k probe chunk).

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

fn fact(r: &mut Rng, n: usize, kdom: usize) -> DataBlock {
    const S: [&str; 5] = ["a", "bb", "", "ccc", "dd"];
    DataBlock::new(vec![
        Column::int64("fk", (0..n).map(|_| if r.below(20) == 0 { None } else { Some(r.below(kdom) as i64) }).collect()),
        Column::int64("fk2", (0..n).map(|_| Some(r.below(3) as i64)).collect()),
        Column::int64("fbig", (0..n).map(|_| if r.below(20) == 0 { None } else { Some(BIG + r.below(kdom) as i64) }).collect()),
        Column::str_col("s", (0..n).map(|_| if r.below(9) == 0 { None } else { Some(S[r.below(5)].to_string()) }).collect()),
        Column::int64("v", (0..n).map(|_| Some(r.below(100) as i64)).collect()),
        Column::float64("x", (0..n).map(|_| if r.below(9) == 0 { None } else { Some(r.below(50) as f64 / 4.0) }).collect()),
    ]).unwrap()
}

/// unique-key dimension (keys shuffled)
fn dim_unique(r: &mut Rng, n: usize) -> DataBlock {
    let mut keys: Vec<i64> = (0..n as i64).collect();
    for i in (1..keys.len()).rev() { let j = r.below(i + 1); keys.swap(i, j); }
    DataBlock::new(vec![
        Column::int64("dk", keys.iter().map(|&k| Some(k)).collect()),
        Column::int64("dbig", keys.iter().map(|&k| Some(BIG + k)).collect()),
        Column::int64("d2", (0..n).map(|_| Some(r.below(3) as i64)).collect()),
        Column::str_col("ds", (0..n).map(|_| if r.below(7) == 0 { None } else { Some(format!("d{}", r.below(1000))) }).collect()),
        Column::int64("dv", (0..n).map(|_| Some(r.below(10) as i64)).collect()),
    ]).unwrap()
}

/// non-unique dimension with duplicate and NULL keys
fn dim_dup(r: &mut Rng, n: usize, kdom: usize) -> DataBlock {
    DataBlock::new(vec![
        Column::int64("ek", (0..n).map(|_| if r.below(15) == 0 { None } else { Some(r.below(kdom) as i64) }).collect()),
        Column::int64("e2", (0..n).map(|_| Some(r.below(3) as i64)).collect()),
        Column::str_col("es", (0..n).map(|_| Some(format!("e{}", r.below(50)))).collect()),
        Column::float64("ef", (0..n).map(|_| Some(r.below(8) as f64)).collect()),
    ]).unwrap()
}

fn render(b: &DataBlock) -> Vec<String> {
    (0..b.num_rows).map(|i| b.columns.iter().map(|c| format!("{:?}", c.data.get_value(i))).collect::<Vec<_>>().join("|")).collect()
}

fn run(c: &KqlContext, q: &str, serial: bool) -> Result<Vec<String>, String> {
    testing::set_no_par_join(serial);
    let out = c.query(q).map(|b| render(&b)).map_err(|e| format!("{e:?}"));
    testing::set_no_par_join(false);
    out
}

fn check(c: &KqlContext, q: &str, tag: &str) -> usize {
    let want = run(c, q, true);
    let got = run(c, q, false);
    match (want, got) {
        (Ok(w), Ok(g)) => {
            assert_eq!(w.len(), g.len(), "row count differs [{tag}]: {q}");
            // same rows in the same order; on a failure report the first differing row
            if let Some(i) = (0..w.len()).find(|&i| w[i] != g[i]) { panic!("row {i} differs [{tag}]: {q}\nserial={}\nparallel={}", w[i], g[i]); }
            w.len()
        }
        (Err(_), Err(_)) => 0,
        (w, g) => panic!("error mismatch [{tag}] {q}: serial ok={} parallel ok={}", w.is_ok(), g.is_ok()),
    }
}

const COMMA: &[&str] = &[
    "select f.fk, f.s, f.x, d.ds, d.dv from f, d where f.fk = d.dk and d.dv < 5",
    "select f.fk, f.s, d.ds, e.es, e.ef from f, d, e where f.fk = d.dk and d.dk = e.ek and d.dv < 7",
    "select f.fk, f.fk2, e.es, f.s from f, e where f.fk = e.ek and f.fk2 = e.e2",
    "select f.fbig, d.dk, d.ds from f, d where f.fbig = d.dbig",
    "select d.ds, count(*) as n, sum(f.v) as sv from f, d where f.fk = d.dk group by d.ds order by d.ds",
    "select f.fk, f.s, d.ds, d.dv, e.es from f, d, e where f.fk = d.dk and f.fk = e.ek and f.fk2 = e.e2 and f.fk2 = d.d2",
];

const EXPLICIT: &[&str] = &[
    "select f.fk, f.s, d.dk, d.ds from f {j} d on f.fk = d.dk",
    "select f.fk, f.s, e.ek, e.es from f {j} e on f.fk = e.ek",
    "select f.fbig, d.dbig, d.ds from f {j} d on f.fbig = d.dbig",
    "select f.fk, e.ek, e.ef from f {j} e on f.fk = e.ek and f.fk2 = e.e2",
];

fn go(nf: usize, nd: usize, ne: usize, kdom: usize, seed: u64) -> usize {
    let mut r = Rng(seed);
    let mut c = KqlContext::new();
    c.register("f", fact(&mut r, nf, kdom));
    c.register("d", dim_unique(&mut r, nd));
    c.register("e", dim_dup(&mut r, ne, kdom));
    let tag = format!("nf={nf} nd={nd} ne={ne} kdom={kdom}");
    let mut rows = 0;
    for q in COMMA { rows += check(&c, q, &tag); }
    for j in ["join", "left join", "right join", "full outer join"] {
        for q in EXPLICIT { rows += check(&c, &q.replace("{j}", j), &tag); }
    }
    rows
}

#[test]
fn small_and_empty_sides() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for (nf, nd, ne) in [(0, 0, 0), (0, 5, 5), (7, 0, 3), (7, 3, 0), (1, 1, 1), (50, 20, 30)] { go(nf, nd, ne, 12, 7 + nf as u64); }
}

#[test]
fn across_parallel_thresholds() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // build sides just below / above 128k (hash build) and 64k (gather); probe sides cross the 32k chunk
    let mut total = 0;
    for (nf, nd, ne, kdom) in [(40_000, 66_000, 66_000, 30_000), (140_000, 131_071, 131_073, 60_000), (150_000, 140_000, 70_000, 200_000)] {
        total += go(nf, nd, ne, kdom, 99 + nf as u64);
    }
    assert!(total > 0);
}

#[test]
fn heavy_duplicate_and_skewed_keys() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = Rng(5);
    let n = 140_000usize;
    let mut c = KqlContext::new();
    // build side: a handful of keys (long chains, skewed hash partitions) plus wide sparse keys
    c.register("f", DataBlock::new(vec![
        Column::int64("fk", (0..30_000).map(|i| Some(if i % 3 == 0 { i64::MAX - (i % 7) } else { (i % 40) as i64 * 1_000_003 })).collect()),
        Column::int64("v", (0..30_000).map(|i| Some(i as i64)).collect()),
    ]).unwrap());
    c.register("e", DataBlock::new(vec![
        Column::int64("ek", (0..n).map(|i| match r.below(4) { 0 => None, 1 => Some(i64::MAX - (i % 7) as i64), 2 => Some(((i % 40) as i64) * 1_000_003), _ => Some(r.next() as i64 * 977) }).collect()),
        Column::str_col("es", (0..n).map(|i| Some(format!("e{}", i % 11))).collect()),
    ]).unwrap());
    c.register("one", DataBlock::new(vec![
        Column::int64("ok", vec![Some(7); n]),
        Column::int64("w", (0..n as i64).map(Some).collect()),
    ]).unwrap());
    c.register("two", DataBlock::new(vec![
        Column::int64("tk", (0..2000).map(|i| Some(if i < 1500 { 7 } else { i })).collect()),
        Column::int64("z", (0..2000i64).map(Some).collect()),
    ]).unwrap());
    let mut rows = 0;
    for j in ["join", "left join", "right join", "full outer join"] {
        rows += check(&c, &format!("select f.fk, f.v, e.es from f {j} e on f.fk = e.ek"), "dup");
        rows += check(&c, &format!("select t.z, o.w from two t {j} one o on t.tk = o.ok"), "alldup");
        rows += check(&c, &format!("select t.z, o.w from one o {j} two t on t.tk = o.ok"), "alldup2");
    }
    assert!(rows > 0);
}

#[test]
fn composite_key_wide_and_float_mix() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let n = 100_000usize;
    let mut r = Rng(31);
    let mut c = KqlContext::new();
    c.register("a", DataBlock::new(vec![
        Column::int64("k1", (0..n).map(|_| if r.below(10) == 0 { None } else { Some(BIG + r.below(300) as i64) }).collect()),
        Column::float64("k2", (0..n).map(|_| Some(r.below(4) as f64)).collect()),
        Column::str_col("k3", (0..n).map(|_| Some(["x", "y"][r.below(2)].to_string())).collect()),
        Column::int64("va", (0..n as i64).map(Some).collect()),
    ]).unwrap());
    c.register("b", DataBlock::new(vec![
        Column::int64("k1", (0..1200).map(|i| Some(BIG + (i % 400) as i64)).collect()),
        Column::int64("k2", (0..1200).map(|i| Some((i % 4) as i64)).collect()),
        Column::str_col("k3", (0..1200).map(|i| Some(["x", "y"][i % 2].to_string())).collect()),
        Column::int64("vb", (0..1200i64).map(Some).collect()),
    ]).unwrap());
    let rows = check(&c, "select a.va, b.vb from a, b where a.k1 = b.k1 and a.k2 = b.k2 and a.k3 = b.k3", "composite3");
    assert!(rows > 0);
}
