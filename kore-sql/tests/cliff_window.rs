//! Differential test for the window-function rewrite: every query runs through the original (quadratic) reference
//! implementation (`testing::set_slow_window(true)`) and the new one, on random tables with NULLs and ties; the
//! answers (values, column types, errors) must be identical.
//!
//! Own test binary because the switch is process-wide. The timing test is ignored by default:
//! `cargo test --release -p kore-sql --offline --test cliff_window -- --ignored --nocapture bench`

use kore_core::{Column, DataBlock};
use kore_sql::{testing, KqlContext};
use std::time::Instant;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
    fn chance(&mut self, pct: usize) -> bool { self.below(100) < pct }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T { &v[self.below(v.len())] }
}

fn opt<T>(r: &mut Rng, pct_null: usize, v: T) -> Option<T> { if r.chance(pct_null) { None } else { Some(v) } }

/// `groups` distinct partition keys, `tspan` distinct order keys (small = many peers).
fn table(r: &mut Rng, n: usize, groups: usize, tspan: usize) -> KqlContext {
    let strs = ["ab", "abc", "b", "", "Ab", "ba"];
    let mut c = KqlContext::new();
    c.register("t", DataBlock::new(vec![
        Column::int64("id", (1..=n as i64).map(Some).collect()),
        Column::int64("g", (0..n).map(|_| { let v = r.below(groups) as i64; opt(r, 8, v) }).collect()),
        Column::str_col("h", (0..n).map(|_| { let v = r.pick(&strs).to_string(); opt(r, 10, v) }).collect()),
        Column::int64("t", (0..n).map(|_| { let v = r.below(tspan) as i64 - 2; opt(r, 8, v) }).collect()),
        Column::float64("tf", (0..n).map(|_| { let v = r.below(tspan) as f64 * 0.5; opt(r, 8, v) }).collect()),
        Column::str_col("d", (0..n).map(|_| { let v = format!("2024-{:02}-{:02}", 1 + r.below(12), 1 + r.below(28)); opt(r, 8, v) }).collect()),
        Column::int64("v", (0..n).map(|_| { let v = r.below(50) as i64 - 10; opt(r, 20, v) }).collect()),
        Column::float64("w", (0..n).map(|_| { let v = r.below(40) as f64 * 0.25 - 3.0; opt(r, 20, v) }).collect()),
        Column::str_col("s", (0..n).map(|_| { let v = r.pick(&strs).to_string(); opt(r, 20, v) }).collect()),
    ]).unwrap());
    c
}

fn render(res: Result<DataBlock, kore_core::KoreError>) -> String {
    match res {
        Ok(b) => b.columns.iter().map(|c| format!("{}:{:?}", c.name, c.data)).collect::<Vec<_>>().join("\n"),
        Err(e) => format!("ERR {e}"),
    }
}

static OK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static ALL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn ab(ctx: &KqlContext, sql: &str) {
    testing::set_slow_window(true);
    let old = render(ctx.query(sql));
    testing::set_slow_window(false);
    let new = render(ctx.query(sql));
    OK.fetch_add(!new.starts_with("ERR") as usize, std::sync::atomic::Ordering::Relaxed);
    ALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if old != new {
        let cut = |s: &str| s.chars().take(1500).collect::<String>();
        panic!("window result differs for: {sql}\n--- old\n{}\n--- new\n{}", cut(&old), cut(&new));
    }
}

fn frame(r: &mut Rng, ord: &str) -> String {
    // frames: ROWS / RANGE / GROUPS
    let k = r.below(4);
    let off = |r: &mut Rng| r.below(4);
    match r.below(12) {
        0 => String::new(),
        1 => " rows between unbounded preceding and current row".into(),
        2 => " rows between unbounded preceding and unbounded following".into(),
        3 => " rows between current row and unbounded following".into(),
        4 => format!(" rows between {} preceding and {} following", off(r), off(r)),
        5 => format!(" rows between {} preceding and {} preceding", k + 1, k),
        6 => format!(" rows between {} following and {} following", k, k + 2),
        7 => " range between unbounded preceding and current row".into(),
        8 => " range between current row and unbounded following".into(),
        9 => if ord.contains("d ") || ord.starts_with("d") {
            format!(" range between interval {} days preceding and interval {} days following", 1 + r.below(30), r.below(30))
        } else {
            format!(" range between {} preceding and {} following", off(r), off(r))
        },
        10 => format!(" groups between {} preceding and {} following", off(r), off(r)),
        _ => format!(" groups between {} following and unbounded following", off(r) % 2),
    }
}

fn window_query(r: &mut Rng, with_part: bool) -> String {
    let parts = ["g", "h", "g, h", "coalesce(g, 0) % 3"];
    let ords = ["t", "t desc", "tf", "tf desc nulls first", "t nulls last", "d", "d desc", "s", "h, t", "t, v desc", "v", "w desc"];
    let part = if with_part { format!("partition by {}", r.pick(&parts)) } else { String::new() };
    let ord = r.pick(&ords).to_string();
    let arg = r.pick(&["v", "w", "t", "s", "d", "h", "v + 1", "coalesce(v, 0)"]).to_string();
    let n = 1 + r.below(7);
    let (f, framed) = match r.below(12) {
        0 => ("row_number()".to_string(), false),
        1 => ("rank()".into(), false),
        2 => ("dense_rank()".into(), false),
        3 => ("percent_rank()".into(), false),
        4 => ("cume_dist()".into(), false),
        5 => (format!("ntile({n})"), false),
        6 => (format!("lag({arg}, {}, {})", r.below(4), if r.chance(50) { "-1" } else { "null" }), false),
        7 => (format!("lead({arg}, {}, {})", r.below(4), if r.chance(50) { "-1" } else { "null" }), false),
        8 => (format!("first_value({arg})"), true),
        9 => (format!("last_value({arg})"), true),
        10 => (format!("{}_value({arg}) ignore nulls", if r.chance(50) { "first" } else { "last" }), true),
        _ => (format!("nth_value({arg}, {})", 1 + r.below(3)), true),
    };
    let mut fr = if framed { frame(r, &ord) } else { String::new() };
    if fr.contains("interval") && !(ord == "d" || ord == "d desc") { fr = " rows between 1 preceding and 1 following".into(); }
    // numeric RANGE offsets need one numeric key
    if fr.contains("range") && fr.contains("preceding and") && fr.contains("following") && !fr.contains("interval")
        && !matches!(ord.as_str(), "t" | "t desc" | "tf" | "tf desc nulls first" | "t nulls last" | "v" | "w desc") { fr = " rows between 1 preceding and current row".into(); }
    format!("select id, {f} over ({part} order by {ord}{fr}) x from t")
}

fn agg_window_query(r: &mut Rng, with_part: bool) -> String {
    let aggs_cheap = ["sum(v)", "sum(w)", "avg(v)", "avg(w)", "count(*)", "count(v)", "count(s)", "min(v)", "max(v)", "min(w)", "max(w)", "min(s)", "max(s)", "min(d)", "max(d)", "count(distinct v)", "count(distinct s)", "min(coalesce(v, 100))"];
    let aggs_more = ["stddev(v)", "var_pop(w)", "median(v)", "string_agg(s, ',')", "bool_or(v > 3)", "any_value(v)", "first(v)", "last(v)", "max_by(v, w)", "sum(v) filter (where v > 5)"];
    let a = if r.chance(80) { *r.pick(&aggs_cheap) } else { *r.pick(&aggs_more) };
    let parts = ["g", "h", "g, h", "coalesce(g, 0) % 3"];
    let part = if with_part { format!("partition by {}", r.pick(&parts)) } else { String::new() };
    let ordered = r.chance(75);
    let numeric = r.chance(40);
    let ord = if numeric { r.pick(&["t", "t desc", "tf", "tf desc", "v", "w desc", "t nulls last"]).to_string() }
        else { r.pick(&["t", "d", "d desc", "s", "h, t", "t, v desc"]).to_string() };
    let mut fr = if ordered { frame(r, &ord) } else { String::new() };
    if fr.contains("interval") && !(ord == "d" || ord == "d desc") { fr = " rows between 1 preceding and 1 following".into(); }
    if fr.contains("range between") && fr.contains("following") && fr.contains("preceding and") && !fr.contains("interval") && !numeric { fr = " rows between 2 preceding and current row".into(); }
    // GROUPS with only-current style ok. Also allow naming a window
    let over = format!("over ({} {}{})", part, if ordered { format!("order by {ord}") } else { String::new() }, fr);
    format!("select id, {a} {over} x from t")
}

fn run_batch(seed: u64, n: usize, groups: usize, tspan: usize, queries: usize, with_part: bool) {
    let mut r = Rng(seed);
    let ctx = table(&mut r, n, groups, tspan);
    for _ in 0..queries {
        let q = if r.chance(50) { agg_window_query(&mut r, with_part) } else { window_query(&mut r, with_part) };
        ab(&ctx, &q);
    }
}

#[test]
fn tiny_tables() {
    for (i, n) in [0usize, 1, 2, 5].iter().enumerate() {
        for s in 0..6 {
            run_batch(100 + (i * 10 + s) as u64, *n, 3, 3, 120, true);
            run_batch(200 + (i * 10 + s) as u64, *n, 3, 3, 60, false);
        }
    }
}

#[test]
fn zz_queries_mostly_succeed() {
    run_batch(900, 1000, 7, 6, 300, true);
    let (ok, all) = (OK.load(std::sync::atomic::Ordering::Relaxed), ALL.load(std::sync::atomic::Ordering::Relaxed));
    println!("{ok}/{all} queries ok");
    assert!(ok * 10 >= all * 8, "too many errors: {ok}/{all}");
}

#[test]
fn thousand_rows() {
    for s in 0..4 {
        run_batch(300 + s, 1000, 7, 6, 150, true);
        run_batch(310 + s, 1000, 40, 300, 150, true);
        run_batch(320 + s, 1000, 1, 5, 120, false);
    }
}

#[test]
fn large_rows_many_partitions() {
    // 100k rows, ~400 partitions of ~250 rows: the reference is only quadratic per partition
    run_batch(400, 100_000, 400, 50, 14, true);
    let mut r = Rng(401);
    let ctx = table(&mut r, 100_000, 400, 3000);
    for q in [
        "select id, row_number() over (order by t, id) x from t",
        "select id, rank() over (order by t) x from t",
        "select id, lag(v, 2) over (order by id) x from t",
        "select id, sum(v) over (partition by g order by t) x from t",
        "select id, min(v) over (partition by g) x from t",
        "select id, max(w) over (partition by g order by t rows between 5 preceding and 3 following) x from t",
        "select id, count(distinct s) over (partition by g order by t) x from t",
        "select id, last_value(v) ignore nulls over (partition by g order by t) x from t",
    ] { ab(&ctx, q); }
}

#[test]
fn named_windows_and_multiple_functions() {
    let mut r = Rng(500);
    let ctx = table(&mut r, 1000, 9, 12);
    for q in [
        "select id, sum(v) over w, rank() over w, min(v) over (w rows between 2 preceding and current row) from t window w as (partition by g order by t)",
        "select id, row_number() over w1, max(w) over w2 from t window w1 as (order by t, id), w2 as (partition by h)",
        "select id from t qualify row_number() over (partition by g order by t, id) = 1",
        "select g, sum(v), rank() over (order by sum(v) desc) from t group by g",
        "select id, sum(v) over (partition by g order by t groups between 1 preceding and current row) from t",
        "select id, cumsum(v) over (partition by g order by id) from t",
        "select id, sum(v) over (order by id rows between 1 following and 0 following) from t",
        "select id, min(v) over (partition by g order by t rows between 3 following and 1 following) from t",
        "select id, sum(v) over (partition by g order by t range between 1 preceding and 1 following), count(*) over (partition by g order by t range between current row and 2 following) from t",
    ] { ab(&ctx, q); }
    // errors must agree too
    for q in [
        "select ntile(0) over (order by t) from t",
        "select sum(v) over (order by t, id range between 1 preceding and current row) from t",
        "select sum(v) over (order by s range between 1 preceding and current row) from t",
        "select nth_value(v, 0) over (order by t) from t",
    ] { ab(&ctx, q); }
}

fn timed(ctx: &KqlContext, name: &str, sql: &str, slow: bool) -> f64 {
    testing::set_slow_window(slow);
    let t = Instant::now();
    let res = ctx.query(sql).expect(sql);
    let s = t.elapsed().as_secs_f64();
    testing::set_slow_window(false);
    println!("{:>5} {:<44} {:>9.3}s  ({} rows)", if slow { "old" } else { "new" }, name, s, res.num_rows);
    s
}

/// Before / after timings. `CLIFF_ROWS` (default 1_000_000) for the new implementation; the old one runs on `CLIFF_OLD_ROWS` (150_000).
#[test]
#[ignore]
fn bench_window() {
    let rows: usize = std::env::var("CLIFF_ROWS").ok().and_then(|v| v.parse().ok()).unwrap_or(1_000_000);
    let old_rows: usize = std::env::var("CLIFF_OLD_ROWS").ok().and_then(|v| v.parse().ok()).unwrap_or(150_000);
    let queries = [
        ("min(v) over (partition by g)", "select min(v) over (partition by g) x from t"),
        ("sum(v) over (partition by g order by t)", "select sum(v) over (partition by g order by t) x from t"),
        ("row_number() over (order by t)", "select row_number() over (order by t) x from t"),
        ("rank() over (partition by g order by v)", "select rank() over (partition by g order by v) x from t"),
        ("lag(v) over (partition by g order by t)", "select lag(v) over (partition by g order by t) x from t"),
    ];
    for (n, slow) in [(old_rows, true), (rows, false), (old_rows, false)] {
        let mut r = Rng(7);
        // few partitions (~100 groups): partitions are huge, the quadratic worst case
        let ctx = {
            let mut c = KqlContext::new();
            c.register("t", DataBlock::new(vec![
                Column::int64("g", (0..n).map(|_| Some(r.below(100) as i64)).collect()),
                Column::int64("t", (0..n).map(|_| Some(r.below(1_000_000) as i64)).collect()),
                Column::int64("v", (0..n).map(|_| Some(r.below(10_000) as i64)).collect()),
            ]).unwrap());
            c
        };
        println!("== {n} rows ==");
        for (name, sql) in &queries { timed(&ctx, name, sql, slow); }
    }
}
