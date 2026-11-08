//! NULL semantics that the Digital Life "World T" agent found wrong in an older copy of the engine
//! (docs/world_t.md "Engine needs" on branch world-t): COUNT(col) counted NULLs, unknown functions returned NULL,
//! window functions returned NaN for missing values. This tip already behaves correctly; these tests pin it on
//! every execution path (fused / vectorized / general / slow window / spilled) and at sizes around the executor
//! thresholds, against brute-force answers.
//!
//! `oracle_dump` (#[ignore]d) writes the same cases in the `fuzz_oracle` format so `tests/oracle/compare.py`
//! replays them in SQLite:
//!   NULLSEM_DIR=<dir> cargo test --release -p kore-sql --test null_semantics -- --ignored oracle_dump
//!   python kore-sql/tests/oracle/compare.py <dir>
//!
//! Lives in its own test binary because the switches are process-wide.

use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::{testing, KqlContext};
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;

static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> { LOCK.lock().unwrap_or_else(|e| e.into_inner()) }

/// Cell text as in fuzz_oracle.rs (integer-valued doubles print as integers, NaN prints as "NaN").
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

/// Every execution path a query can take: (name, switch on, switch off). `general` is quadratic on some shapes.
fn paths(n: usize) -> Vec<(&'static str, fn(bool))> {
    let mut v: Vec<(&'static str, fn(bool))> = vec![
        ("default", |_| {}),
        ("no_fused", testing::set_no_fused),
        ("no_vecexpr", testing::set_no_vecexpr),
        ("no_typed_groups", testing::set_no_typed_groups),
        ("no_wide_agg", testing::set_no_wide_agg),
        ("no_sorted_agg", testing::set_no_sorted_agg),
        ("no_fd_agg", testing::set_no_fd_agg),
        ("slow_window", testing::set_slow_window),
    ];
    if n <= 20_000 { v.push(("general", testing::set_force_general)); }
    v
}

fn nn<T>(v: &[Option<T>]) -> i64 { v.iter().filter(|x| x.is_some()).count() as i64 }

// ───────────────────────── 1. COUNT ─────────────────────────

#[test]
fn count_col_skips_nulls_count_star_counts_rows_world_t_table() {
    let _l = lock();
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
    let _l = lock();
    // 5 rows, 3 of them NULL in every column: COUNT(*) is 5 (rows), COUNT(col) is the non-NULL count of that column
    let mut c = KqlContext::new();
    c.register("n", DataBlock::new(vec![
        Column::int64("a", vec![Some(1), None, None, None, Some(2)]),
        Column::float64("b", vec![None, None, None, None, Some(2.5)]),
        Column::str_col("s", vec![Some("x".into()), None, None, None, Some("".into())]),
        Column::float64("z", vec![None; 5]),
    ]).unwrap());
    for (name, set) in paths(5) {
        set(true);
        assert_eq!(one(&c, "SELECT COUNT(*), COUNT(a), COUNT(b), COUNT(s), COUNT(z), COUNT(1) FROM n"), ["5", "2", "1", "2", "0", "5"], "{name}");
        assert_eq!(one(&c, "SELECT COUNT(*) FROM n WHERE a IS NULL AND b IS NULL AND s IS NULL AND z IS NULL"), ["3"], "{name}");
        // an all-NULL table: COUNT(*) counts its rows, COUNT(z) is 0, SUM / AVG are NULL
        assert_eq!(one(&c, "SELECT COUNT(*), COUNT(z), SUM(z), AVG(z) FROM n WHERE a IS NULL"), ["3", "0", "NULL", "NULL"], "{name}");
        // grouping on a NULL key: one group of the 3 all-NULL rows
        assert_eq!(rows(&c, "SELECT a, COUNT(*), COUNT(b), COUNT(a) FROM n GROUP BY a ORDER BY a NULLS FIRST"),
            vec![vec!["NULL", "3", "0", "0"], vec!["1", "1", "0", "1"], vec!["2", "1", "1", "1"]], "{name}");
        assert_eq!(one(&c, "SELECT COUNT(DISTINCT a), COUNT(DISTINCT z), COUNT(DISTINCT s) FROM n"), ["2", "0", "2"], "{name}");
        let w = rows(&c, "SELECT COUNT(*) OVER (), COUNT(b) OVER (), COUNT(z) OVER () FROM n");
        assert_eq!(w.len(), 5);
        assert!(w.iter().all(|r| r == &["5", "1", "0"]), "{name}: {w:?}");
        set(false);
    }
}

/// COUNT(col), COUNT(DISTINCT col), COUNT in GROUP BY / HAVING / windows against brute force, at sizes around the
/// fused (8192), probe-chunk (32768) and 65536 thresholds, plain and dictionary text, on every path.
#[test]
fn count_col_matches_brute_force_on_every_path_and_size() {
    let _l = lock();
    for &n in &[7usize, 300, 8193, 20_000, 70_000] {
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
        for (name, set) in paths(n) {
            set(true);
            for tab in ["t", "td"] {
                let ctx = format!("{name} n={n} {tab}");
                assert_eq!(one(&c, &format!("SELECT COUNT(*), COUNT(x), COUNT(i), COUNT(s), COUNT(g), COUNT(z) FROM {tab}")),
                    [star, nn(&d.x), nn(&d.i), nn(&d.s), nn(&d.g), 0].map(|v| v.to_string()), "{ctx}");
                assert_eq!(one(&c, &format!("SELECT COUNT(DISTINCT i), COUNT(DISTINCT s), COUNT(DISTINCT z) FROM {tab}")),
                    [distinct_i, distinct_s, 0].map(|v| v.to_string()), "{ctx}");
                assert_eq!(rows(&c, &format!("SELECT g, COUNT(*), COUNT(x), COUNT(i), COUNT(s), COUNT(DISTINCT i) FROM {tab} GROUP BY g ORDER BY g NULLS FIRST")), grouped, "{ctx}");
                assert_eq!(rows(&c, &format!("SELECT g, COUNT(x) FROM {tab} GROUP BY g HAVING COUNT(x) >= {thr} ORDER BY g NULLS FIRST")), having, "{ctx}");
                // the same filter on COUNT(*) keeps every group with that many rows (rows, not values)
                let having_star = per.iter().filter(|(_, e)| e.0 >= thr).count();
                assert_eq!(rows(&c, &format!("SELECT g FROM {tab} GROUP BY g HAVING COUNT(*) >= {thr}")).len(), having_star, "{ctx}");
                if n <= 20_000 || name == "default" || name == "slow_window" {
                    // window COUNT: whole partition, running, and a sliding ROWS frame
                    let w = rows(&c, &format!("SELECT id, COUNT(x) OVER (PARTITION BY g), COUNT(*) OVER (PARTITION BY g), COUNT(i) OVER (ORDER BY id), \
                        COUNT(s) OVER (ORDER BY id ROWS BETWEEN 2 PRECEDING AND CURRENT ROW) FROM {tab} ORDER BY id"));
                    assert_eq!(w.len(), n, "{ctx}");
                    let mut run = 0i64;
                    for k in 0..n {
                        run += d.i[k].is_some() as i64;
                        let e = &per[&d.g[k]];
                        let slide = (k.saturating_sub(2)..=k).filter(|&j| d.s[j].is_some()).count();
                        assert_eq!(w[k], [d.id[k].to_string(), e.1.to_string(), e.0.to_string(), run.to_string(), slide.to_string()], "{ctx} row {k}");
                    }
                }
            }
            set(false);
        }
        // under a tiny memory budget (external sort / grace-hash GROUP BY)
        let mut cs = ctx_of(&d);
        cs.set_memory_limit(64 * 1024);
        assert_eq!(one(&cs, "SELECT COUNT(*), COUNT(x), COUNT(i), COUNT(s) FROM t"), [star, nn(&d.x), nn(&d.i), nn(&d.s)].map(|v| v.to_string()), "spill n={n}");
        assert_eq!(rows(&cs, "SELECT g, COUNT(*), COUNT(x), COUNT(i), COUNT(s), COUNT(DISTINCT i) FROM t GROUP BY g ORDER BY g NULLS FIRST"), grouped, "spill n={n}");
    }
}

// ───────────────────────── 2. unknown functions ─────────────────────────

#[test]
fn unknown_function_is_an_error_naming_the_function() {
    let _l = lock();
    let c = ctx_of(&data(50, 3));
    let cases = [
        ("SELECT NO_SUCH_FN(x) AS z FROM t", "NO_SUCH_FN"),
        ("SELECT id FROM t WHERE NO_SUCH_FN(x) = 1", "NO_SUCH_FN"),
        ("SELECT frobnicate(g), COUNT(*) FROM t GROUP BY frobnicate(g)", "frobnicate"),
        ("SELECT g FROM t GROUP BY g HAVING Bogus_Agg(x) > 1", "Bogus_Agg"),
        ("SELECT id FROM t ORDER BY nosuch(id)", "nosuch"),
        ("SELECT SUM(nosuch(x)) OVER (ORDER BY id) FROM t", "nosuch"),
        ("SELECT id FROM t WHERE id IN (SELECT REGEXP_LIKEE(s, 'a') FROM td)", "REGEXP_LIKEE"),
        ("SELECT UPPER(no_such_inner(s)) FROM t", "no_such_inner"),
        ("select No_Such_Fn(1) from t", "No_Such_Fn"),
        ("WITH q AS (SELECT id FROM t) SELECT MyFn(id) FROM q", "MyFn"),
        // a column of the same spelling is not taken for the function: only an identifier followed by `(`
        ("SELECT id AS nosuch2, NoSuch2(id) FROM t", "NoSuch2"),
    ];
    for (name, set) in paths(50) {
        set(true);
        for (sql, f) in cases {
            let e = match c.query(sql) { Ok(b) => panic!("{name}: {sql}: expected an error, got {} rows", b.num_rows), Err(e) => e.to_string() };
            // the message names the function exactly as it is written in the SQL
            assert!(e.ends_with(&format!("unknown function: {f}")), "{name}: {sql}: {e}");
        }
        // REGEXP_LIKE is a real function on this tip (World T saw it return NULL on an older engine)
        assert_eq!(rows(&c, "SELECT REGEXP_LIKE(s, '^r') FROM t WHERE s IS NOT NULL ORDER BY id").len(), nn(&data(50, 3).s) as usize, "{name}");
        set(false);
    }
}

// ───────────────────────── 3. windows: NULL, never NaN ─────────────────────────

#[test]
fn window_missing_values_are_null_world_t_table() {
    let _l = lock();
    let mut c = KqlContext::new();
    c.register("t", DataBlock::new(vec![
        Column::float64("x", vec![Some(1.0), None, Some(3.0), Some(4.0)]),
        Column::int64("d", vec![Some(1), Some(2), Some(3), Some(4)]),
    ]).unwrap());
    for (name, set) in paths(4) {
        set(true);
        let col = |sql: &str| rows(&c, sql).into_iter().map(|r| r[1].clone()).collect::<Vec<_>>();
        assert_eq!(col("SELECT d, LAG(x) OVER (ORDER BY d) FROM t ORDER BY d"), ["NULL", "1", "NULL", "3"], "{name}");
        assert_eq!(col("SELECT d, LEAD(x) OVER (ORDER BY d) FROM t ORDER BY d"), ["NULL", "3", "4", "NULL"], "{name}");
        assert_eq!(col("SELECT d, LAG(d, 2) OVER (ORDER BY d) FROM t ORDER BY d"), ["NULL", "NULL", "1", "2"], "{name}");
        assert_eq!(col("SELECT d, LEAD(d, 5) OVER (ORDER BY d) FROM t ORDER BY d"), ["NULL", "NULL", "NULL", "NULL"], "{name}");
        assert_eq!(col("SELECT d, LAG(d, 1, 0) OVER (ORDER BY d) FROM t ORDER BY d"), ["0", "1", "2", "3"], "{name}");
        assert_eq!(col("SELECT d, LEAD(x, 1, -1) OVER (ORDER BY d) FROM t ORDER BY d"), ["NULL", "3", "4", "-1"], "{name}");
        // empty frames: the row before the first row, rows after the last
        let prev = "OVER (ORDER BY d ROWS BETWEEN 1 PRECEDING AND 1 PRECEDING)";
        assert_eq!(col(&format!("SELECT d, SUM(x) {prev} FROM t ORDER BY d")), ["NULL", "1", "NULL", "3"], "{name}");
        assert_eq!(col(&format!("SELECT d, AVG(x) {prev} FROM t ORDER BY d")), ["NULL", "1", "NULL", "3"], "{name}");
        assert_eq!(col(&format!("SELECT d, MIN(x) {prev} FROM t ORDER BY d")), ["NULL", "1", "NULL", "3"], "{name}");
        assert_eq!(col(&format!("SELECT d, MAX(d) {prev} FROM t ORDER BY d")), ["NULL", "1", "2", "3"], "{name}");
        assert_eq!(col(&format!("SELECT d, COUNT(x) {prev} FROM t ORDER BY d")), ["0", "1", "0", "1"], "{name}");
        assert_eq!(col(&format!("SELECT d, FIRST_VALUE(d) {prev} FROM t ORDER BY d")), ["NULL", "1", "2", "3"], "{name}");
        assert_eq!(col("SELECT d, SUM(d) OVER (ORDER BY d ROWS BETWEEN 2 FOLLOWING AND 3 FOLLOWING) FROM t ORDER BY d"), ["7", "4", "NULL", "NULL"], "{name}");
        assert_eq!(col("SELECT d, AVG(x) OVER (ORDER BY d RANGE BETWEEN 10 FOLLOWING AND 20 FOLLOWING) FROM t ORDER BY d"), ["NULL", "NULL", "NULL", "NULL"], "{name}");
        assert_eq!(col("SELECT d, NTH_VALUE(x, 3) OVER (ORDER BY d) FROM t ORDER BY d"), ["NULL", "NULL", "3", "3"], "{name}");
        set(false);
    }
}

#[test]
fn window_lag_lead_and_empty_frames_match_brute_force() {
    let _l = lock();
    for &n in &[9usize, 8193, 70_000] {
        let d = data(n, 11 + n as u64);
        let c = ctx_of(&d);
        let f = |v: Option<f64>| v.map(|x| if x.fract() == 0.0 { format!("{}", x as i64) } else { format!("{x}") }).unwrap_or("NULL".into());
        let iv = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or("NULL".into());
        for (name, set) in paths(n) {
            if n > 20_000 && !matches!(name, "default" | "slow_window" | "no_fused" | "no_vecexpr") { continue; }
            set(true);
            let w = rows(&c, "SELECT id, LAG(x) OVER (ORDER BY id), LEAD(i) OVER (ORDER BY id), LAG(s, 2) OVER (ORDER BY id), \
                SUM(x) OVER (ORDER BY id ROWS BETWEEN 1 PRECEDING AND 1 PRECEDING), AVG(i) OVER (ORDER BY id ROWS BETWEEN 1 FOLLOWING AND 1 FOLLOWING), \
                MIN(z) OVER (ORDER BY id), LAG(i, 1, -7) OVER (ORDER BY id) FROM t ORDER BY id");
            assert_eq!(w.len(), n);
            for k in 0..n {
                let exp = [
                    d.id[k].to_string(),
                    if k >= 1 { f(d.x[k - 1]) } else { "NULL".into() },
                    if k + 1 < n { iv(d.i[k + 1]) } else { "NULL".into() },
                    if k >= 2 { d.s[k - 2].clone().unwrap_or("NULL".into()) } else { "NULL".into() },
                    if k >= 1 { f(d.x[k - 1]) } else { "NULL".into() },
                    if k + 1 < n { d.i[k + 1].map(|v| v.to_string()).unwrap_or("NULL".into()) } else { "NULL".into() },
                    "NULL".into(),
                    if k >= 1 { iv(d.i[k - 1]) } else { "-7".into() },
                ];
                assert_eq!(w[k], exp, "{name} n={n} row {k}");
            }
            // per partition: the first row of every partition has no LAG, the last no LEAD
            let p = rows(&c, "SELECT COUNT(*) FROM (SELECT LAG(id) OVER (PARTITION BY g ORDER BY id) AS p FROM td) q WHERE p IS NULL");
            assert_eq!(p[0][0], d.g.iter().collect::<HashSet<_>>().len().to_string(), "{name} n={n}");
            set(false);
        }
        let all = rows(&c, "SELECT LAG(x) OVER (ORDER BY id), AVG(x) OVER (ORDER BY id ROWS BETWEEN 3 FOLLOWING AND 4 FOLLOWING), \
            STDDEV(x) OVER (ORDER BY id ROWS BETWEEN 1 PRECEDING AND 1 PRECEDING) FROM t");
        assert!(all.iter().flatten().all(|v| !v.eq_ignore_ascii_case("nan")), "n={n}: a window returned NaN");
    }
}

// ───────────────────────── SQLite oracle dump ─────────────────────────

/// The queries replayed in SQLite by `tests/oracle/compare.py` ("O" = ordered result, "U" = compare as a multiset).
fn oracle_queries() -> Vec<(&'static str, String)> {
    let mut q: Vec<(&'static str, String)> = Vec::new();
    for tab in ["t"] {
        q.push(("U", format!("select count(*), count(x), count(i), count(s), count(g), count(z), count(1) from {tab}")));
        q.push(("U", format!("select count(distinct i), count(distinct s), count(distinct x), count(distinct z) from {tab}")));
        q.push(("U", format!("select g, count(*), count(x), count(i), count(s), count(distinct i), count(z) from {tab} group by g")));
        q.push(("U", format!("select g, count(x) from {tab} group by g having count(x) > 3")));
        q.push(("U", format!("select g, count(*) from {tab} group by g having count(i) < count(*)")));
        q.push(("U", format!("select i, count(x), count(*) from {tab} where id < 400 group by i")));
        q.push(("U", format!("select count(x), count(*) from {tab} where x is null")));
        q.push(("O", format!("select id, count(x) over (partition by g), count(*) over (partition by g), count(i) over (order by id) from {tab} order by id")));
        q.push(("O", format!("select id, count(s) over (order by id rows between 2 preceding and current row), count(z) over () from {tab} order by id")));
        q.push(("O", format!("select id, lag(x) over (order by id), lead(x) over (order by id), lag(i, 3) over (order by id), lead(s, 2) over (order by id) from {tab} order by id")));
        q.push(("O", format!("select id, lag(i, 1, -1) over (order by id), lead(x, 1, 0.5) over (order by id) from {tab} order by id")));
        q.push(("O", format!("select id, lag(id) over (partition by g order by id), lead(id) over (partition by g order by id) from {tab} order by id")));
        q.push(("O", format!("select id, sum(x) over (order by id rows between 1 preceding and 1 preceding), avg(i) over (order by id rows between 1 following and 1 following) from {tab} order by id")));
        q.push(("O", format!("select id, min(x) over (order by id rows between 2 following and 3 following), max(i) over (order by id rows between 3 preceding and 2 preceding) from {tab} order by id")));
        q.push(("O", format!("select id, count(x) over (order by id rows between 1 preceding and 1 preceding), first_value(x) over (order by id rows between 1 preceding and 1 preceding) from {tab} order by id")));
        q.push(("O", format!("select id, sum(z) over (order by id), avg(z) over (partition by g), min(z) over () from {tab} order by id")));
        q.push(("O", format!("select id, sum(x) over (partition by g order by id rows between 1 preceding and 1 preceding) from {tab} order by id")));
        q.push(("O", format!("select id, nth_value(x, 3) over (order by id), last_value(i) over (order by id rows between 1 following and 1 following) from {tab} order by id")));
        // unknown functions: SQLite errors too (compare.py skips a query SQLite rejects); kore's error is checked above
        q.push(("U", format!("select no_such_fn(x) from {tab}")));
    }
    // all-NULL rows in a second table
    q.push(("U", "select count(*), count(a), count(b), count(s), count(z) from n".into()));
    q.push(("U", "select a, count(*), count(b), count(a) from n group by a".into()));
    q
}

#[test]
#[ignore]
fn oracle_dump() {
    let _l = lock();
    let dir = std::env::var("NULLSEM_DIR").expect("set NULLSEM_DIR to the output directory");
    for &n in &[60usize, 8193, 20_000] {
        let d = data(n, 101 + n as u64);
        let mut c = ctx_of(&d);
        c.register("n", DataBlock::new(vec![
            Column::int64("a", vec![Some(1), None, None, None, Some(2)]),
            Column::float64("b", vec![None, None, None, None, Some(2.5)]),
            Column::str_col("s", vec![Some("x".into()), None, None, None, Some("".into())]),
            Column::float64("z", vec![None; 5]),
        ]).unwrap());
        for (name, set) in paths(n) {
            set(true);
            let sd = format!("{dir}/n{n}_{name}");
            std::fs::create_dir_all(&sd).unwrap();
            for (tab, hdr) in [("t", "id:i,g:s,x:f,i:i,s:s,z:f"), ("n", "a:i,b:f,s:s,z:f")] {
                let b = c.get(tab).unwrap();
                let mut s = String::from(hdr);
                s.push('\n');
                for r in 0..b.num_rows {
                    let cells: Vec<String> = b.columns.iter().map(|col| {
                        let v = cell(col, r);
                        let null = match &col.data { ColumnData::Str(x) => x[r].is_none(), _ => v == "NULL" };
                        if null { "\\N".into() } else if v.is_empty() { "\\E".into() } else { v }
                    }).collect();
                    s.push_str(&cells.join(","));
                    s.push('\n');
                }
                std::fs::write(format!("{sd}/{tab}.csv"), s).unwrap();
            }
            let (mut out_q, mut out_a) = (String::new(), String::new());
            for (qi, (o, sql)) in oracle_queries().into_iter().enumerate() {
                let _ = writeln!(out_q, "{o}\t{sql}");
                match c.query(&sql) {
                    Ok(b) => {
                        let _ = writeln!(out_a, "Q {qi} {}", b.num_rows);
                        for r in 0..b.num_rows { let _ = writeln!(out_a, "{}", b.columns.iter().map(|col| cell(col, r)).collect::<Vec<_>>().join("\t")); }
                    }
                    Err(e) => { let _ = writeln!(out_a, "E {qi} {}", e.to_string().replace('\n', " ")); }
                }
            }
            std::fs::write(format!("{sd}/queries.txt"), out_q).unwrap();
            std::fs::write(format!("{sd}/kore.txt"), out_a).unwrap();
            set(false);
        }
    }
}
