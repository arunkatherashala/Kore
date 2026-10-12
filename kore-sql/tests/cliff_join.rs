//! Explicit joins with complex ON conditions: the hash path (composite keys + residual on candidate pairs) must
//! give exactly the rows of the old nested loop, for INNER / LEFT / RIGHT / FULL, and must scale.

use kore_core::{Column, DataBlock};
use kore_sql::{testing, KqlContext};
use std::time::Instant;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize { (self.next() % n.max(1) as u64) as usize }
}

fn table(r: &mut Rng, n: usize, kdom: usize) -> DataBlock {
    const S: [&str; 5] = ["a", "b", "", "cc", "d"];
    let nul = |r: &mut Rng| r.below(8) == 0;
    DataBlock::new(vec![
        Column::int64("k1", (0..n).map(|_| if nul(r) { None } else { Some(r.below(kdom) as i64) }).collect()),
        Column::int64("k2", (0..n).map(|_| if nul(r) { None } else { Some(r.below(3) as i64) }).collect()),
        Column::str_col("s", (0..n).map(|_| if nul(r) { None } else { Some(S[r.below(5)].to_string()) }).collect()),
        Column::int64("v", (0..n).map(|_| if nul(r) { None } else { Some(r.below(10) as i64) }).collect()),
        Column::float64("f", (0..n).map(|_| if nul(r) { None } else { Some(r.below(4) as f64) }).collect()),
    ]).unwrap()
}

fn rows(b: &DataBlock) -> Vec<String> {
    let mut v: Vec<String> = (0..b.num_rows)
        .map(|i| b.columns.iter().map(|c| format!("{:?}", c.data.get_value(i))).collect::<Vec<_>>().join("|"))
        .collect();
    v.sort();
    v
}

const ONS: &[&str] = &[
    "a.k1 = b.k1 and a.s = b.s",
    "a.k1 = b.k1 and a.k2 = b.k2 and a.s = b.s",
    "a.k1 = b.k1 and a.v < b.v",
    "a.k1 = b.k1 and (a.v + b.v) % 3 = 0 and b.k2 = 1",
    "a.k1 = b.k1 and a.k2 > 0",
    "a.k1 = b.k1 and a.f = b.f and a.v <> b.v",
    "a.k1 = b.v and a.s = b.s",
    "a.k1 = b.k1 and a.k1 = a.k2",
    "a.v < b.v and a.k2 = b.k2",
    "a.v < b.v",
    "a.v + b.v = 7 or a.k1 = b.k1",
];

#[test]
fn hash_path_equals_nested_loop() {
    let mut r = Rng(12345);
    let mut checked = 0;
    for (na, nb) in [(0usize, 0usize), (0, 7), (9, 0), (1, 1), (13, 11), (60, 45), (200, 150)] {
        for round in 0..3 {
            let mut c = KqlContext::new();
            c.register("ta", table(&mut r, na, 4 + 6 * round));
            c.register("tb", table(&mut r, nb, 4 + 6 * round));
            for kind in ["join", "left join", "right join", "full outer join"] {
                for on in ONS {
                    let q = format!("select a.k1 as ak, a.v as av, a.s as asx, b.k1 as bk, b.v as bv, b.s as bs, b.f as bf from ta a {kind} tb b on {on}");
                    testing::set_no_hash_residual(true);
                    let want = c.query(&q);
                    testing::set_no_hash_residual(false);
                    let got = c.query(&q);
                    match (want, got) {
                        (Ok(w), Ok(g)) => assert_eq!(rows(&g), rows(&w), "mismatch ({na},{nb}) round {round}: {q}"),
                        (Err(_), Err(_)) => {}
                        (w, g) => panic!("error mismatch for {q}: nested={:?} hash={:?}", w.is_ok(), g.is_ok()),
                    }
                    checked += 1;
                }
            }
        }
    }
    testing::set_no_hash_residual(false);
    println!("{checked} join queries compared");
}

#[test]
fn matching_on_key_but_failing_residual_is_unmatched() {
    let mut c = KqlContext::new();
    c.register("ta", DataBlock::new(vec![Column::int64("k", vec![Some(1), Some(2), None]), Column::int64("v", vec![Some(5), Some(5), Some(5)])]).unwrap());
    c.register("tb", DataBlock::new(vec![Column::int64("k", vec![Some(1), Some(1), None]), Column::int64("v", vec![Some(1), Some(9), Some(9)])]).unwrap());
    let q = |kind: &str| rows(&c.query(&format!("select a.k, a.v, b.k, b.v from ta a {kind} tb b on a.k = b.k and a.v < b.v")).unwrap());
    // a1 matches only (1,9); a2 and the NULL row are unmatched
    assert_eq!(q("left join").len(), 3);
    assert_eq!(q("join").len(), 1);
    // right: b(1,1) fails the residual, b(NULL) never matches -> 3 rows (one pair + two unmatched right rows)
    assert_eq!(q("right join").len(), 3);
    // full: pair + 2 unmatched left + 2 unmatched right
    assert_eq!(q("full join").len(), 5);
}

#[test]
fn composite_left_join_scales() {
    let n = 250_000usize;
    let mut c = KqlContext::new();
    let k1: Vec<Option<i64>> = (0..n as i64).map(|i| Some(i % 5000)).collect();
    c.register("ta", DataBlock::new(vec![
        Column::int64("k1", k1.clone()),
        Column::str_col("s", (0..n).map(|i| Some(format!("s{}", i % 7))).collect()),
    ]).unwrap());
    c.register("tc", DataBlock::new(vec![
        Column::int64("k1", (0..5000i64).map(Some).collect()),
        Column::str_col("s", (0..5000).map(|i| Some(format!("s{}", i % 7))).collect()),
    ]).unwrap());
    let t = Instant::now();
    let r = c.query("select count(*) as n from tc c left join ta a on a.k1 = c.k1 and a.s = c.s").unwrap();
    let el = t.elapsed();
    println!("composite LEFT JOIN 5k x 250k: {el:?}, {:?}", r.columns[0].data.get_value(0));
    assert!(el.as_secs_f64() < 10.0, "took {el:?}");
}

#[test]
fn theta_join_is_blocked() {
    let n = 3000usize;
    let mut c = KqlContext::new();
    c.register("ta", DataBlock::new(vec![Column::int64("v", (0..n as i64).map(Some).collect())]).unwrap());
    c.register("tb", DataBlock::new(vec![Column::int64("v", (0..n as i64).map(Some).collect())]).unwrap());
    let t = Instant::now();
    let r = c.query("select count(*) as n from ta a join tb b on a.v < b.v").unwrap();
    println!("theta 3k x 3k: {:?} {:?}", t.elapsed(), r.columns[0].data.get_value(0));
    assert_eq!(format!("{:?}", r.columns[0].data.get_value(0)), format!("{:?}", kore_core::Value::Int((n * (n - 1) / 2) as i64)));
}
