//! NULL semantics that the Digital Life "World T" agent found wrong in this copy of the engine
//! (docs/world_t.md "Engine needs" items 1, 2 and 5): COUNT(col) counted NULLs, unknown functions returned NULL,
//! window functions returned NaN for missing values.
//!
//! Ported from `kore-sql/tests/null_semantics.rs` on `master-kore-engine` (236ae86c) and cut down to what this
//! older engine supports. Not ported, because this copy has no such feature: the `testing::set_*` execution-path
//! switches and the spill budget (this copy has one execution path), window frames (`ROWS` / `RANGE` clauses are
//! parsed but not applied; a window aggregate always covers its whole partition, so a frame cannot be empty), the
//! three-argument `LAG` / `LEAD` with a default (a parse error here), `LAG` / `LEAD` of a text column, aggregates
//! inside `HAVING` (this copy evaluates them as NULL; `HAVING <alias>` is used instead), `COUNT(DISTINCT <text>)`
//! under `GROUP BY`, `GROUP BY` / `ORDER BY` on an expression, `REGEXP_LIKE` (not a function in this copy), and the
//! SQLite oracle dump.

use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::KqlContext;
use std::collections::{BTreeMap, HashSet};

/// Cell text as in master's fuzz_oracle.rs (integer-valued doubles print as integers, NaN prints as "NaN").
fn cell(c: &Column, r: usize) -> String {
    let num = |x: f64| -> String { if x.fract() == 0.0 && x.abs() < 1e15 { format!("{}", x as i64) } else { format!("{}", x) } };
    match &c.data {
        ColumnData::Int64(v) => v[r].map(|x| x.to_string()).unwrap_or("NULL".into()),
        ColumnData::Float64(v) => v[r].map(num).unwrap_or("NULL".into()),
        ColumnData::Bool(v) => v[r].map(|x| (x as i32).to_string()).unwrap_or("NULL".into()),
        ColumnData::Str(v) => v[r].clone().unwrap_or("NULL".into()),
        ColumnData::StrDict { codes, dict } => if codes[r] == u8::MAX { "NULL".into() } else { dict[codes[r] as usize].clone() },
    }
}

fn rows(c: &KqlContext, sql: &str) -> Vec<Vec<String>> {
    let b = c.query(sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
    (0..b.num_rows).map(|r| b.columns.iter().map(|col| cell(col, r)).collect()).collect()
}

fn one(c: &KqlContext, sql: &str) -> Vec<String> {
    let r = rows(c, sql);
    assert_eq!(r.len(), 1, "{sql}: {r:?}");
    r.into_iter().next().unwrap()
}

// ───────────────────────── data ─────────────────────────

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 { self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); self.0 >> 33 }
    fn below(&mut self, n: u64) -> u64 { self.next() % n }
    fn chance(&mut self, pct: u64) -> bool { self.below(100) < pct }
}

const GROUPS: [&str; 7] = ["a", "b", "c", "d", "e", "f", "g"];
const STRS: [&str; 5] = ["", "p", "q", "rr", "s"];

/// The rows of table `t`, also used for the brute-force answers.
struct Data { id: Vec<i64>, g: Vec<Option<String>>, x: Vec<Option<f64>>, i: Vec<Option<i64>>, s: Vec<Option<String>> }

fn data(n: usize, seed: u64) -> Data {
    let mut r = Rng(seed);
    let mut d = Data { id: vec![], g: vec![], x: vec![], i: vec![], s: vec![] };
    for k in 0..n {
        d.id.push(k as i64 + 1);
        d.g.push(if r.chance(10) { None } else { Some(GROUPS[r.below(7) as usize].to_string()) });
        d.x.push(if r.chance(30) { None } else { Some(r.below(40) as f64 * 0.5 - 3.0) });
        d.i.push(if r.chance(30) { None } else { Some(r.below(50) as i64) });
        d.s.push(if r.chance(30) { None } else { Some(STRS[r.below(5) as usize].to_string()) });
    }
    d
}

/// `t` (plain strings) and `td` (the same rows, text dictionary-encoded); `z` is NULL on every row.
fn ctx_of(d: &Data) -> KqlContext {
    let n = d.id.len();
    let dict = |v: &[Option<String>], name: &str| {
        let mut dict: Vec<String> = Vec::new();
        let codes = v.iter().map(|x| match x {
            None => u8::MAX,
            Some(t) => match dict.iter().position(|y| y == t) { Some(p) => p as u8, None => { dict.push(t.clone()); (dict.len() - 1) as u8 } },
        }).collect();
        Column::str_dict(name, codes, dict)
    };
    let mut c = KqlContext::new();
    let cols = |dicted: bool| {
        vec![
            Column::int64("id", d.id.iter().map(|&v| Some(v)).collect()),
            if dicted { dict(&d.g, "g") } else { Column::str_col("g", d.g.clone()) },
            Column::float64("x", d.x.clone()),
            Column::int64("i", d.i.clone()),
            if dicted { dict(&d.s, "s") } else { Column::str_col("s", d.s.clone()) },
            Column::float64("z", vec![None; n]),
        ]
    };
    c.register("t", DataBlock::new(cols(false)).unwrap());
    c.register("td", DataBlock::new(cols(true)).unwrap());
    c
}

/// Rows in the order of the brute-force answers: by the first cell, NULL first.
fn sorted(mut r: Vec<Vec<String>>) -> Vec<Vec<String>> {
    r.sort_by(|a, b| (a[0] != "NULL", &a[0]).cmp(&(b[0] != "NULL", &b[0])));
    r
}

fn nn<T>(v: &[Option<T>]) -> i64 { v.iter().filter(|x| x.is_some()).count() as i64 }

// ───────────────────────── 1. COUNT ─────────────────────────

#[test]
fn count_col_skips_nulls_count_star_counts_rows_world_t_table() {
    // the World T table (docs/world_t.md): COUNT(x) over 1, NULL, 3, 4 is 3; COUNT(*) is 4; AVG ignores the NULL
    let mut c = KqlContext::new();
    c.register("t", DataBlock::new(vec![
        Column::float64("x", vec![Some(1.0), None, Some(3.0), Some(4.0)]),
        Column::str_col("g", vec![Some("a".into()), Some("b".into()), Some("a".into()), None]),
        Column::int64("d", vec![Some(1), Some(2), Some(3), Some(4)]),
    ]).unwrap());
    assert_eq!(one(&c, "SELECT COUNT(x) AS n, COUNT(*) AS c, AVG(x) AS a FROM t"), ["3", "4", "2.6666666666666665"]);
    assert_eq!(one(&c, "SELECT COUNT(g) AS n FROM t"), ["3"]);
    assert_eq!(one(&c, "SELECT COUNT(*) AS n FROM t WHERE x IS NOT NULL"), ["3"]);
}

#[test]
fn count_star_unchanged_counts_rows_whose_columns_are_all_null() {
    // 5 rows, 3 of them NULL in every column: COUNT(*) is 5 (rows), COUNT(col) is the non-NULL count of that column
    let mut c = KqlContext::new();
    c.register("n", DataBlock::new(vec![
        Column::int64("a", vec![Some(1), None, None, None, Some(2)]),
        Column::float64("b", vec![None, None, None, None, Some(2.5)]),
        Column::str_col("s", vec![Some("x".into()), None, None, None, Some("".into())]),
        Column::float64("z", vec![None; 5]),
    ]).unwrap());
    assert_eq!(one(&c, "SELECT COUNT(*) AS c0, COUNT(a) AS c1, COUNT(b) AS c2, COUNT(s) AS c3, COUNT(z) AS c4, COUNT(1) AS c5 FROM n"), ["5", "2", "1", "2", "0", "5"]);
    assert_eq!(one(&c, "SELECT COUNT(*) AS c FROM n WHERE a IS NULL AND b IS NULL AND s IS NULL AND z IS NULL"), ["3"]);
    // an all-NULL table: COUNT(*) counts its rows, COUNT(z) is 0, SUM / AVG are NULL
    assert_eq!(one(&c, "SELECT COUNT(*) AS c0, COUNT(z) AS c1, SUM(z) AS s, AVG(z) AS v FROM n WHERE a IS NULL"), ["3", "0", "NULL", "NULL"]);
    // COUNT of an expression counts the rows where it is not NULL
    assert_eq!(one(&c, "SELECT COUNT(a + 1) AS c0, COUNT(UPPER(s)) AS c1, COUNT(NULL) AS c2 FROM n"), ["2", "2", "0"]);
    // grouping on a NULL key: one group of the 3 all-NULL rows
    assert_eq!(rows(&c, "SELECT a, COUNT(*) AS c, COUNT(b) AS cb, COUNT(a) AS ca FROM n GROUP BY a ORDER BY a NULLS FIRST"),
        vec![vec!["NULL", "3", "0", "0"], vec!["1", "1", "0", "1"], vec!["2", "1", "1", "1"]]);
    assert_eq!(one(&c, "SELECT COUNT(DISTINCT a) AS c0, COUNT(DISTINCT z) AS c1, COUNT(DISTINCT s) AS c2 FROM n"), ["2", "0", "2"]);
    let w = rows(&c, "SELECT COUNT(*) OVER () AS c0, COUNT(b) OVER () AS c1, COUNT(z) OVER () AS c2, COUNT(a) OVER () AS c3, COUNT(s) OVER () AS c4, COUNT(1) OVER () AS c5 FROM n");
    assert_eq!(w.len(), 5);
    assert!(w.iter().all(|r| r == &["5", "1", "0", "2", "2", "5"]), "{w:?}");
}

/// COUNT(col), COUNT(DISTINCT col), COUNT in GROUP BY / HAVING / windows against brute force, at sizes around the
/// parallel GROUP BY threshold (50 000 rows), plain and dictionary text.
#[test]
fn count_col_matches_brute_force_at_every_size() {
    for &n in &[7usize, 300, 8193, 70_000] {
        let d = data(n, n as u64 * 7 + 1);
        let c = ctx_of(&d);
        let star = n as i64;
        let distinct_i = d.i.iter().flatten().collect::<HashSet<_>>().len() as i64;
        let distinct_s = d.s.iter().flatten().collect::<HashSet<_>>().len() as i64;
        // per group (NULL group key included): (count(*), count(x), count(i), count(s), count(distinct i))
        let mut per: BTreeMap<Option<String>, (i64, i64, i64, i64, HashSet<i64>)> = BTreeMap::new();
        for k in 0..n {
            let e = per.entry(d.g[k].clone()).or_insert((0, 0, 0, 0, HashSet::new()));
            e.0 += 1; e.1 += d.x[k].is_some() as i64; e.2 += d.i[k].is_some() as i64; e.3 += d.s[k].is_some() as i64;
            if let Some(v) = d.i[k] { e.4.insert(v); }
        }
        let grouped: Vec<Vec<String>> = per.iter().map(|(g, e)| vec![g.clone().unwrap_or("NULL".into()), e.0.to_string(), e.1.to_string(), e.2.to_string(), e.3.to_string(), e.4.len().to_string()]).collect();
        let thr = (n as i64 / 20).max(1);
        let having: Vec<Vec<String>> = per.iter().filter(|(_, e)| e.1 >= thr).map(|(g, e)| vec![g.clone().unwrap_or("NULL".into()), e.1.to_string()]).collect();
        for tab in ["t", "td"] {
            let ctx = format!("n={n} {tab}");
            assert_eq!(one(&c, &format!("SELECT COUNT(*) AS c0, COUNT(x) AS c1, COUNT(i) AS c2, COUNT(s) AS c3, COUNT(g) AS c4, COUNT(z) AS c5 FROM {tab}")),
                [star, nn(&d.x), nn(&d.i), nn(&d.s), nn(&d.g), 0].map(|v| v.to_string()), "{ctx}");
            assert_eq!(one(&c, &format!("SELECT COUNT(DISTINCT i) AS c0, COUNT(DISTINCT z) AS c1 FROM {tab}")),
                [distinct_i, 0].map(|v| v.to_string()), "{ctx}");
            if tab == "t" {
                assert_eq!(one(&c, &format!("SELECT COUNT(DISTINCT s) AS c FROM {tab}")), [distinct_s.to_string()], "{ctx}");
            }
            // (sorted here: this copy's ORDER BY of a text GROUP BY key is not what is under test)
            assert_eq!(sorted(rows(&c, &format!("SELECT g, COUNT(*) AS c0, COUNT(x) AS c1, COUNT(i) AS c2, COUNT(s) AS c3, COUNT(DISTINCT i) AS c4 FROM {tab} GROUP BY g"))), grouped, "{ctx}");
            assert_eq!(sorted(rows(&c, &format!("SELECT g, COUNT(x) AS cx FROM {tab} GROUP BY g HAVING cx >= {thr}"))), having, "{ctx}");
            // the same filter on COUNT(*) keeps every group with that many rows (rows, not values)
            let having_star = per.iter().filter(|(_, e)| e.0 >= thr).count();
            assert_eq!(rows(&c, &format!("SELECT g, COUNT(*) AS cs FROM {tab} GROUP BY g HAVING cs >= {thr}")).len(), having_star, "{ctx}");
            // window COUNT over the whole partition
            let w = rows(&c, &format!("SELECT id, COUNT(x) OVER (PARTITION BY g) AS w1, COUNT(*) OVER (PARTITION BY g) AS w2, \
                COUNT(i) OVER () AS w3, COUNT(s) OVER (PARTITION BY g) AS w4 FROM {tab} ORDER BY id"));
            assert_eq!(w.len(), n, "{ctx}");
            for k in 0..n {
                let e = &per[&d.g[k]];
                assert_eq!(w[k], [d.id[k].to_string(), e.1.to_string(), e.0.to_string(), nn(&d.i).to_string(), e.3.to_string()], "{ctx} row {k}");
            }
        }
    }
}

/// A correlated scalar subquery is decorrelated into a per-key aggregate: NULLs of the aggregated column must not
/// count (COUNT) nor be read as 0 (SUM / AVG / MIN / MAX), and a key with no inner rows has COUNT 0.
#[test]
fn correlated_aggregate_subquery_skips_nulls() {
    let mut c = KqlContext::new();
    c.register("o", DataBlock::new(vec![
        Column::int64("k", vec![Some(1), Some(2), Some(3), Some(4)]),
        Column::int64("lim", vec![Some(2), Some(1), Some(0), Some(0)]),
        Column::float64("v", vec![Some(4.5), Some(-1.5), Some(0.0), Some(0.0)]),
    ]).unwrap());
    // key 1: values 5, NULL, 4 -> COUNT 2, SUM 9, AVG 4.5, MIN 4, MAX 5
    // key 2: values -2, NULL -> COUNT 1, SUM -2, AVG -2, MIN -2, MAX -2
    // key 3: values NULL, NULL -> COUNT 0, SUM / AVG / MIN / MAX NULL
    // key 4: no rows -> COUNT 0
    c.register("i", DataBlock::new(vec![
        Column::int64("k", vec![Some(1), Some(1), Some(1), Some(2), Some(2), Some(3), Some(3)]),
        Column::float64("x", vec![Some(5.0), None, Some(4.0), Some(-2.0), None, None, None]),
    ]).unwrap());
    let ks = |sql: &str| rows(&c, sql).into_iter().map(|r| r[0].clone()).collect::<Vec<_>>();
    assert_eq!(ks("SELECT o.k FROM o WHERE o.lim = (SELECT COUNT(i.x) FROM i WHERE i.k = o.k) ORDER BY o.k"), ["1", "2", "3", "4"]);
    assert_eq!(ks("SELECT o.k FROM o WHERE o.v = (SELECT AVG(i.x) FROM i WHERE i.k = o.k) ORDER BY o.k"), ["1"]);
    // key 3 had MIN 0 (its NULLs read as 0), so 0 >= MIN held
    assert_eq!(ks("SELECT o.k FROM o WHERE o.v >= (SELECT MIN(i.x) FROM i WHERE i.k = o.k) ORDER BY o.k"), ["1", "2"]);
    assert_eq!(ks("SELECT o.k FROM o WHERE o.v > (SELECT MAX(i.x) FROM i WHERE i.k = o.k) ORDER BY o.k"), ["2"]);
    assert_eq!(ks("SELECT o.k FROM o WHERE o.v < (SELECT SUM(i.x) FROM i WHERE i.k = o.k) ORDER BY o.k"), ["1"]);
    // an all-NULL key has a NULL SUM: the comparison is not true for key 3
    assert_eq!(ks("SELECT o.k FROM o WHERE o.v <= (SELECT SUM(i.x) FROM i WHERE i.k = o.k) ORDER BY o.k"), ["1"]);
}

// ───────────────────────── 2. unknown functions ─────────────────────────

#[test]
fn unknown_function_is_an_error_naming_the_function() {
    let c = ctx_of(&data(50, 3));
    let cases = [
        ("SELECT NO_SUCH_FN(x) AS z FROM t", "NO_SUCH_FN"),
        ("SELECT id FROM t WHERE NO_SUCH_FN(x) = 1", "NO_SUCH_FN"),
        ("SELECT g FROM t GROUP BY g HAVING Bogus_Agg(x) > 1", "Bogus_Agg"),
        ("SELECT SUM(nosuch(x)) OVER (ORDER BY id) AS w FROM t", "nosuch"),
        ("SELECT id FROM t WHERE id IN (SELECT REGEXP_LIKEE(s, 'a') FROM td)", "REGEXP_LIKEE"),
        ("SELECT UPPER(no_such_inner(s)) AS u FROM t", "no_such_inner"),
        ("select No_Such_Fn(1) from t", "No_Such_Fn"),
        ("WITH q AS (SELECT id FROM t) SELECT MyFn(id) AS m FROM q", "MyFn"),
        ("SELECT CASE WHEN id > 1 THEN frobnicate(g) ELSE 'x' END AS c FROM t", "frobnicate"),
        // a column of the same spelling is not taken for the function: only an identifier followed by `(`
        ("SELECT id AS nosuch2, NoSuch2(id) AS v FROM t", "NoSuch2"),
        // REGEXP_LIKE is not a function in this copy of the engine (it is on master): an error, not NULL
        ("SELECT REGEXP_LIKE(s, '^r') AS r FROM t", "REGEXP_LIKE"),
    ];
    for (sql, f) in cases {
        let e = match c.query(sql) { Ok(b) => panic!("{sql}: expected an error, got {} rows", b.num_rows), Err(e) => e.to_string() };
        // the message names the function exactly as it is written in the SQL
        assert!(e.ends_with(&format!("unknown function: {f}")), "{sql}: {e}");
    }
    // known functions, in any case, and a CAST type with a precision still work
    assert_eq!(rows(&c, "SELECT upper(s) AS u, Coalesce(x, 0) AS v, CAST(x AS DECIMAL(10, 2)) AS w FROM t").len(), 50);
    assert_eq!(rows(&c, "SELECT LEFT(s, 1) AS l, EXTRACT(YEAR FROM '2024-01-02') AS y FROM t").len(), 50);
}

// ───────────────────────── 3. windows: NULL, never NaN ─────────────────────────

#[test]
fn window_missing_values_are_null_world_t_table() {
    let mut c = KqlContext::new();
    c.register("t", DataBlock::new(vec![
        Column::float64("x", vec![Some(1.0), None, Some(3.0), Some(4.0)]),
        Column::int64("d", vec![Some(1), Some(2), Some(3), Some(4)]),
        Column::int64("i", vec![None, Some(7), None, Some(9)]),
        Column::str_col("g", vec![Some("a".into()), Some("b".into()), Some("a".into()), Some("b".into())]),
        Column::float64("z", vec![None; 4]),
    ]).unwrap());
    let col = |sql: &str| rows(&c, sql).into_iter().map(|r| r[1].clone()).collect::<Vec<_>>();
    assert_eq!(col("SELECT d, LAG(x) OVER (ORDER BY d) AS w FROM t ORDER BY d"), ["NULL", "1", "NULL", "3"]);
    assert_eq!(col("SELECT d, LEAD(x) OVER (ORDER BY d) AS w FROM t ORDER BY d"), ["NULL", "3", "4", "NULL"]);
    assert_eq!(col("SELECT d, LAG(d, 2) OVER (ORDER BY d) AS w FROM t ORDER BY d"), ["NULL", "NULL", "1", "2"]);
    assert_eq!(col("SELECT d, LEAD(d, 5) OVER (ORDER BY d) AS w FROM t ORDER BY d"), ["NULL", "NULL", "NULL", "NULL"]);
    // a NULL in an integer column stays NULL (it was read as 0)
    assert_eq!(col("SELECT d, LAG(i) OVER (ORDER BY d) AS w FROM t ORDER BY d"), ["NULL", "NULL", "7", "NULL"]);
    assert_eq!(col("SELECT d, LEAD(i) OVER (ORDER BY d) AS w FROM t ORDER BY d"), ["7", "NULL", "9", "NULL"]);
    // per partition: the first row of each partition has no LAG, the last no LEAD
    assert_eq!(col("SELECT d, LAG(x) OVER (PARTITION BY g ORDER BY d) AS w FROM t ORDER BY d"), ["NULL", "NULL", "1", "NULL"]);
    assert_eq!(col("SELECT d, LEAD(d) OVER (PARTITION BY g ORDER BY d) AS w FROM t ORDER BY d"), ["3", "4", "NULL", "NULL"]);
    // aggregates over a partition with no value: NULL, COUNT 0
    for (f, exp) in [("SUM", "NULL"), ("AVG", "NULL"), ("MIN", "NULL"), ("MAX", "NULL"), ("COUNT", "0")] {
        assert_eq!(col(&format!("SELECT d, {f}(z) OVER () AS w FROM t ORDER BY d")), [exp; 4], "{f}");
        assert_eq!(col(&format!("SELECT d, {f}(z) OVER (PARTITION BY g ORDER BY d) AS w FROM t ORDER BY d")), [exp; 4], "{f}");
    }
    // NULLs of an integer column are skipped by the aggregates (they were read as 0)
    assert_eq!(col("SELECT d, MIN(i) OVER () AS w FROM t ORDER BY d"), ["7"; 4]);
    assert_eq!(col("SELECT d, AVG(i) OVER () AS w FROM t ORDER BY d"), ["8"; 4]);
    assert_eq!(col("SELECT d, COUNT(i) OVER () AS w FROM t ORDER BY d"), ["2"; 4]);
    assert_eq!(col("SELECT d, SUM(i) OVER (PARTITION BY g) AS w FROM t ORDER BY d"), ["NULL", "16", "NULL", "16"]);
    // FIRST_VALUE / LAST_VALUE of a NULL value is NULL
    assert_eq!(col("SELECT d, FIRST_VALUE(i) OVER (ORDER BY d) AS w FROM t ORDER BY d"), ["NULL"; 4]);
    assert_eq!(col("SELECT d, LAST_VALUE(x) OVER (PARTITION BY g ORDER BY d) AS w FROM t ORDER BY d"), ["3", "4", "3", "4"]);
    // running sum before the first value; running average with no value yet
    assert_eq!(col("SELECT d, CUMSUM(z) OVER (ORDER BY d) AS w FROM t ORDER BY d"), ["NULL"; 4]);
    assert_eq!(col("SELECT d, CUMSUM(i) OVER (ORDER BY d) AS w FROM t ORDER BY d"), ["NULL", "7", "7", "16"]);
}

/// A NaN that is a value of the data is not a missing value: it is kept (it is not turned into NULL), and the
/// window aggregates treat it as IEEE arithmetic does, like the plain aggregates of this engine.
#[test]
fn window_keeps_a_nan_that_is_a_data_value() {
    let mut c = KqlContext::new();
    c.register("t", DataBlock::new(vec![
        Column::float64("x", vec![Some(1.0), Some(f64::NAN), Some(3.0)]),
        Column::int64("d", vec![Some(1), Some(2), Some(3)]),
    ]).unwrap());
    let col = |sql: &str| rows(&c, sql).into_iter().map(|r| r[1].clone()).collect::<Vec<_>>();
    assert_eq!(col("SELECT d, LAG(x) OVER (ORDER BY d) AS w FROM t ORDER BY d"), ["NULL", "1", "NaN"]);
    assert_eq!(col("SELECT d, COUNT(x) OVER () AS w FROM t ORDER BY d"), ["3"; 3]);
    assert_eq!(col("SELECT d, SUM(x) OVER () AS w FROM t ORDER BY d"), ["NaN"; 3]);
    assert_eq!(one(&c, "SELECT SUM(x) AS s, COUNT(x) AS n FROM t"), ["NaN", "3"]);
}

#[test]
fn window_lag_lead_match_brute_force() {
    for &n in &[9usize, 8193, 70_000] {
        let d = data(n, 11 + n as u64);
        let c = ctx_of(&d);
        let f = |v: Option<f64>| v.map(|x| if x.fract() == 0.0 { format!("{}", x as i64) } else { format!("{x}") }).unwrap_or("NULL".into());
        let iv = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or("NULL".into());
        let w = rows(&c, "SELECT id, LAG(x) OVER (ORDER BY id) AS w1, LEAD(i) OVER (ORDER BY id) AS w2, LAG(i, 2) OVER (ORDER BY id) AS w3, \
            MIN(z) OVER (ORDER BY id) AS w4, AVG(z) OVER (PARTITION BY g) AS w5 FROM t ORDER BY id");
        assert_eq!(w.len(), n);
        for k in 0..n {
            let exp = [
                d.id[k].to_string(),
                if k >= 1 { f(d.x[k - 1]) } else { "NULL".into() },
                if k + 1 < n { iv(d.i[k + 1]) } else { "NULL".into() },
                if k >= 2 { iv(d.i[k - 2]) } else { "NULL".into() },
                "NULL".into(),
                "NULL".into(),
            ];
            assert_eq!(w[k], exp, "n={n} row {k}");
        }
        // per partition: the first row of every partition has no LAG
        let p = rows(&c, "SELECT COUNT(*) AS c FROM (SELECT LAG(id) OVER (PARTITION BY g ORDER BY id) AS p FROM td) q WHERE p IS NULL");
        assert_eq!(p[0][0], d.g.iter().collect::<HashSet<_>>().len().to_string(), "n={n}");
        let all = rows(&c, "SELECT LAG(x) OVER (ORDER BY id) AS a, LEAD(i, 3) OVER (ORDER BY id) AS b, AVG(x) OVER (PARTITION BY g) AS c, \
            MIN(z) OVER () AS e, MAX(z) OVER () AS f, FIRST_VALUE(x) OVER (PARTITION BY g ORDER BY id) AS h FROM t");
        assert!(all.iter().flatten().all(|v| !v.eq_ignore_ascii_case("nan") && !v.contains("inf")), "n={n}: a window returned NaN or infinity");
    }
}
