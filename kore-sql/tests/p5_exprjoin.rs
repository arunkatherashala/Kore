//! Expression-key hash joins (a.rn = b.rn + 1, substr(a.s,1,2) = b.s, ...) and the composite-key route for
//! joins with several equalities, against the old single-key + post-filter / nested-loop behaviour: same rows
//! (compared sorted), for INNER / LEFT / RIGHT / FULL, NULL keys, duplicates, empty sides, integer wrap-around,
//! float keys, text keys, keys above 2^53, and a CTE referenced 1-4 times with different outer filters.

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

fn table(r: &mut Rng, n: usize, dom: usize) -> DataBlock {
    const S: [&str; 6] = ["ab", "abc", "b", "", "xyz", "xy"];
    let edge = [i64::MAX, i64::MAX - 1, i64::MIN, i64::MIN + 1, 0, -1];
    DataBlock::new(vec![
        Column::int64("k", (0..n).map(|_| if r.below(12) == 0 { None } else { Some(r.below(dom) as i64) }).collect()),
        Column::int64("g", (0..n).map(|_| Some(r.below(3) as i64)).collect()),
        Column::int64("big", (0..n).map(|_| if r.below(12) == 0 { None } else { Some(BIG + r.below(dom) as i64) }).collect()),
        Column::int64("ed", (0..n).map(|_| if r.below(10) == 0 { None } else { Some(edge[r.below(edge.len())]) }).collect()),
        Column::float64("f", (0..n).map(|_| if r.below(10) == 0 { None } else { Some(r.below(dom) as f64 / 2.0) }).collect()),
        Column::str_col("s", (0..n).map(|_| if r.below(9) == 0 { None } else { Some(S[r.below(S.len())].to_string()) }).collect()),
        Column::int64("v", (0..n).map(|_| Some(r.below(100) as i64)).collect()),
    ]).unwrap()
}

fn render(b: &DataBlock) -> Vec<String> {
    let mut v: Vec<String> = (0..b.num_rows).map(|i| b.columns.iter().map(|c| format!("{:?}", c.data.get_value(i))).collect::<Vec<_>>().join("|")).collect();
    v.sort();
    v
}

fn run(c: &KqlContext, q: &str, off: bool) -> Result<Vec<String>, String> {
    testing::set_no_expr_join(off);
    let out = c.query(q).map(|b| render(&b)).map_err(|e| format!("{e:?}"));
    testing::set_no_expr_join(false);
    out
}

fn check(c: &KqlContext, q: &str, tag: &str) -> usize {
    let want = run(c, q, true);
    let got = run(c, q, false);
    assert_eq!(want, got, "{tag}: {q}");
    got.map(|v| v.len()).unwrap_or(0)
}

fn ctx(seed: u64, na: usize, nb: usize, dom: usize) -> KqlContext {
    let mut r = Rng(seed);
    let mut c = KqlContext::new();
    c.register("ta", table(&mut r, na, dom));
    c.register("tb", table(&mut r, nb, dom));
    c
}

const KINDS: [&str; 4] = ["JOIN", "LEFT JOIN", "RIGHT JOIN", "FULL JOIN"];

#[test]
fn expression_keys_all_join_kinds() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let before = testing::expr_join_hits();
    let conds = [
        "a.k = b.k + 1",
        "a.k + 1 = b.k",
        "a.k = b.k - 1 AND a.g = b.g",
        "a.k = b.k + 1 AND a.s = b.s",
        "a.k * 2 = b.k",
        "a.f = b.f + 0.5",
        "a.k = b.f + 1",
        "a.f * 2 = b.k",
        "substr(a.s, 1, 2) = b.s",
        "substr(a.s, 2) = substr(b.s, 1, 1)",
        "a.k = b.k + 1 AND a.v < b.v",
        "a.k = b.k + 1 AND a.g = b.g AND a.s = b.s",
        "b.k + 1 = a.k AND a.g = b.g",
    ];
    let mut total = 0;
    for (seed, (na, nb, dom)) in [(1u64, (40usize, 40usize, 6usize)), (2, (200, 150, 30)), (3, (0, 30, 5)), (4, (30, 0, 5)), (5, (0, 0, 3)), (6, (300, 300, 4))] {
        let c = ctx(seed, na, nb, dom);
        for cond in conds {
            for kind in KINDS {
                total += check(&c, &format!("SELECT a.k, a.g, a.s, a.v, b.k, b.g, b.s, b.v FROM ta a {kind} tb b ON {cond}"), &format!("seed {seed}"));
            }
        }
    }
    assert!(total > 0);
    assert!(testing::expr_join_hits() > before, "the expression-key path was never taken");
}

#[test]
fn multi_key_offset_self_join() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Q47 shape: several plain keys plus offset equalities in both directions on a self join
    let mut r = Rng(11);
    let n = 400;
    let rows = DataBlock::new(vec![
        Column::str_col("cat", (0..n).map(|_| Some(["x", "y", "z"][r.below(3)].to_string())).collect()),
        Column::str_col("brand", (0..n).map(|_| if r.below(15) == 0 { None } else { Some(["p", "q"][r.below(2)].to_string()) }).collect()),
        Column::int64("rn", (0..n).map(|_| if r.below(20) == 0 { None } else { Some(r.below(12) as i64) }).collect()),
        Column::float64("amt", (0..n).map(|_| Some(r.below(1000) as f64 / 8.0)).collect()),
    ]).unwrap();
    let mut c = KqlContext::new();
    c.register("v1", rows);
    let before = testing::expr_join_hits();
    let q = "SELECT v1.cat, v1.brand, v1.rn, v1.amt, l.amt AS pa, u.amt AS na FROM v1 \
             JOIN v1 l ON v1.cat = l.cat AND v1.brand = l.brand AND v1.rn = l.rn + 1 \
             JOIN v1 u ON v1.cat = u.cat AND v1.brand = u.brand AND v1.rn = u.rn - 1";
    let n1 = check(&c, q, "self join both directions");
    assert!(n1 > 0);
    assert!(testing::expr_join_hits() > before);
    check(&c, &q.replace("JOIN v1 l", "LEFT JOIN v1 l").replace("JOIN v1 u", "LEFT JOIN v1 u"), "left self join");
    check(&c, "SELECT v1.rn, l.rn FROM v1 JOIN v1 l ON v1.rn = l.rn + 1 AND v1.cat = l.cat WHERE v1.amt > 50", "filter");
}

#[test]
fn cte_used_one_to_four_times() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let c = ctx(21, 120, 10, 8);
    let cte = "WITH t AS (SELECT k, g, s, SUM(v) AS sv, COUNT(*) AS n FROM ta WHERE k IS NOT NULL GROUP BY k, g, s) ";
    let qs = [
        "SELECT x.k, x.sv FROM t x WHERE x.g = 1",
        "SELECT x.k, x.sv, y.sv FROM t x JOIN t y ON x.g = y.g AND x.k = y.k + 1 AND x.s = y.s WHERE x.n > 1",
        "SELECT x.k, y.sv, z.sv FROM t x, t y, t z WHERE x.k = y.k AND x.s = y.s AND x.g = y.g AND y.k = z.k AND y.s = z.s AND z.g = y.g AND x.g = 0 AND z.n > 1",
        "SELECT a.k, b.sv, c.sv, d.sv FROM t a JOIN t b ON a.k = b.k AND a.s = b.s AND a.g = b.g JOIN t c ON a.k = c.k + 1 AND a.g = c.g AND a.s = c.s JOIN t d ON a.k = d.k - 1 AND a.g = d.g AND a.s = d.s",
        "SELECT x.k, y.k FROM t x LEFT JOIN t y ON x.k = y.k + 1 AND x.g = y.g AND y.n > 1 WHERE x.g < 2",
    ];
    for q in qs { check(&c, &format!("{cte}{q}"), "cte"); }
}

#[test]
fn comma_joins_with_offsets() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let c = ctx(31, 150, 150, 10);
    for q in [
        "SELECT a.k, b.k FROM ta a, tb b WHERE a.k = b.k + 1 AND a.g = b.g",
        "SELECT a.k, b.k FROM ta a, tb b WHERE a.k + 2 = b.k AND a.s = b.s AND a.v > 10",
        "SELECT a.k, b.s FROM ta a, tb b WHERE substr(a.s, 1, 1) = substr(b.s, 1, 1) AND a.g = b.g",
    ] { check(&c, q, "comma"); }
}

/// Keys above 2^53 / wrapping arithmetic: the old evaluators compare through f64 there, so the reference is computed
/// here with the row evaluator's semantics (exact integers, wrapping).
#[test]
fn keys_above_2_pow_53_are_exact() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = Rng(41);
    let (ta, tb) = (table(&mut r, 300, 40), table(&mut r, 300, 40));
    let col = |b: &DataBlock, n: &str| -> Vec<Option<i64>> {
        let c = b.columns.iter().find(|c| c.name == n).unwrap();
        (0..b.num_rows).map(|i| match c.data.get_value(i) { kore_core::Value::Int(x) => Some(x), _ => None }).collect()
    };
    let cols = ["big", "ed"].map(|n| (col(&ta, n), col(&tb, n)));
    let mut c = KqlContext::new();
    c.register("ta", ta);
    c.register("tb", tb);
    type F = fn(i64, i64) -> bool;
    let cases: [(&str, usize, F); 7] = [
        ("a.big = b.big + 1", 0, |x, y| x == y.wrapping_add(1)),
        ("a.big + 1 = b.big", 0, |x, y| x.wrapping_add(1) == y),
        ("a.big = b.big - 3", 0, |x, y| x == y.wrapping_sub(3)),
        ("a.ed = b.ed + 1", 1, |x, y| x == y.wrapping_add(1)),
        ("a.ed + 1 = b.ed", 1, |x, y| x.wrapping_add(1) == y),
        ("a.ed * 2 = b.ed", 1, |x, y| x.wrapping_mul(2) == y),
        ("a.ed = b.ed * 2 + 1", 1, |x, y| x == y.wrapping_mul(2).wrapping_add(1)),
    ];
    for (cond, ci, f) in cases {
        let (xs, ys) = &cols[ci];
        let mut want = 0i64;
        for x in xs.iter().flatten() { for y in ys.iter().flatten() { if f(*x, *y) { want += 1; } } }
        for kind in ["JOIN", "LEFT JOIN", "RIGHT JOIN", "FULL JOIN"] {
            // matched pairs only: count rows with both sides present
            let got = c.query(&format!("SELECT COUNT(*) AS n FROM ta a {kind} tb b ON {cond} WHERE a.v IS NOT NULL AND b.v IS NOT NULL")).unwrap();
            match got.columns[0].data.get_value(0) { kore_core::Value::Int(n) => assert_eq!(n, want, "{kind}: {cond}"), v => panic!("{v:?}") }
        }
    }
}

/// General aggregation (window over aggregates) with typed group codes vs the per-cell key strings.
#[test]
fn typed_group_codes_match_key_strings() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = Rng(51);
    let mut c = KqlContext::new();
    for (name, n, dom) in [("ta", 500usize, 7usize), ("tb", 1, 3), ("tc", 0, 3), ("td", 5000, 60)] {
        c.register(name, table(&mut r, n, dom));
    }
    let qs = [
        "SELECT k, g, s, SUM(v) AS sv, AVG(SUM(v)) OVER (PARTITION BY g) AS av, rank() OVER (PARTITION BY g ORDER BY SUM(v), k, s) AS rn FROM {t} GROUP BY k, g, s",
        "SELECT f, big, SUM(v) AS sv, COUNT(*) AS n, rank() OVER (ORDER BY SUM(v), f, big) AS rn FROM {t} GROUP BY f, big",
        "SELECT s, SUM(v) AS sv, AVG(SUM(v)) OVER () AS av FROM {t} GROUP BY s",
        "SELECT ed, k, MIN(v) AS mn, MAX(v) AS mx, row_number() OVER (ORDER BY ed, k) AS rn FROM {t} GROUP BY ed, k",
        "SELECT g, SUM(v) AS sv, SUM(SUM(v)) OVER () AS tot FROM {t} WHERE v > 1000 GROUP BY g",
        "SELECT k, g, s, f FROM {t} UNION SELECT k, g, s, f FROM ta",
        "SELECT k, s, big FROM {t} UNION SELECT k, s, big FROM {t} WHERE v < 50 UNION SELECT k, s, big FROM tb",
        "SELECT f, ed FROM {t} UNION SELECT f, ed FROM tc",
        "SELECT g FROM {t} UNION ALL SELECT g FROM tb UNION SELECT g FROM ta",
    ];
    for t in ["ta", "tb", "tc", "td"] {
        for q in qs {
            let q = q.replace("{t}", t);
            let run = |off: bool| -> Result<Vec<String>, String> {
                testing::set_no_typed_groups(off);
                let o = c.query(&q).map(|b| { let mut v: Vec<String> = (0..b.num_rows).map(|i| b.columns.iter().map(|c| format!("{:?}", c.data.get_value(i))).collect::<Vec<_>>().join("|")).collect(); v.sort(); v }).map_err(|e| format!("{e:?}"));
                testing::set_no_typed_groups(false);
                o
            };
            assert_eq!(run(true), run(false), "{q}");
        }
    }
}
