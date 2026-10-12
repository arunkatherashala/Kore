//! The ISO-date accelerator (dateaccel.rs) must give exactly the answers of the string path, on random data with
//! NULLs and boundary dates, for malformed columns and literals, and after INSERT / UPDATE / DELETE.
//! One test function: the switch is process-wide and tests in a binary run in parallel threads.

use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::{testing, KqlContext};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
}

const BOUNDARY: [&str; 8] = ["1995-06-15", "1995-06-14", "1995-06-16", "1995-01-01", "1994-12-31", "1995-12-31", "0000-01-01", "9999-12-31"];

fn rand_date(r: &mut Rng) -> Option<String> {
    match r.below(20) {
        0 => None,
        1 | 2 => Some(BOUNDARY[r.below(BOUNDARY.len())].to_string()),
        // an impossible calendar date still orders like its string
        3 => Some(format!("199{}-02-31", r.below(10))),
        _ => Some(format!("{}-{:02}-{:02}", 1990 + r.below(10), 1 + r.below(12), 1 + r.below(28))),
    }
}

fn dates(n: usize, seed: u64) -> Vec<Option<String>> {
    let mut r = Rng(seed);
    (0..n).map(|_| rand_date(&mut r)).collect()
}

fn block(n: usize, seed: u64, d_override: Option<(usize, &str)>) -> DataBlock {
    let mut r = Rng(seed ^ 0x55);
    let mut d = dates(n, seed);
    if let Some((i, s)) = d_override { d[i] = Some(s.to_string()); }
    let e = dates(n, seed + 1);
    let id: Vec<Option<i64>> = (0..n as i64).map(Some).collect();
    let k: Vec<Option<i64>> = (0..n).map(|_| Some(r.below(50) as i64)).collect();
    let v: Vec<Option<i64>> = (0..n).map(|_| if r.below(10) == 0 { None } else { Some(r.below(1000) as i64) }).collect();
    DataBlock::new(vec![Column::int64("id", id), Column::int64("k", k), Column::int64("v", v), Column::str_col("d", d), Column::str_col("e", e)]).unwrap()
}

fn small_dim() -> DataBlock {
    DataBlock::new(vec![
        Column::int64("k", (0..50).map(Some).collect()),
        Column::int64("w", (0..50).map(|i| Some(i * 3)).collect()),
    ]).unwrap()
}

fn cell(c: &Column, r: usize) -> String {
    match &c.data {
        ColumnData::Int64(v) => v[r].map_or("NULL".into(), |x| x.to_string()),
        ColumnData::Float64(v) => v[r].map_or("NULL".into(), |x| format!("{:.6e}", x)),
        ColumnData::Str(v) => v[r].clone().unwrap_or("NULL".into()),
        ColumnData::StrDict { codes, dict } => if codes[r] == u8::MAX { "NULL".into() } else { dict[codes[r] as usize].clone() },
        ColumnData::Bool(v) => format!("{:?}", v[r]),
    }
}

fn rows(c: &KqlContext, sql: &str) -> Vec<String> {
    let b = c.query(sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
    (0..b.num_rows).map(|r| b.columns.iter().map(|c| cell(c, r)).collect::<Vec<_>>().join("|")).collect()
}

fn predicates(col: &str) -> Vec<String> {
    let d = col;
    let mut p: Vec<String> = Vec::new();
    for lit in ["1995-06-15", "1995-01-01", "1990-01-01", "1999-12-31", "0000-01-01", "9999-12-31", "1995-02-31", "2000-01-01"] {
        for op in ["<", "<=", ">", ">=", "=", "<>", "!="] {
            p.push(format!("{d} {op} '{lit}'"));
            p.push(format!("'{lit}' {op} {d}"));
        }
    }
    p.extend([
        format!("{d} between '1993-01-01' and '1995-12-31'"),
        format!("{d} not between '1993-01-01' and '1995-12-31'"),
        format!("{d} between '1995-06-15' and '1995-06-15'"),
        format!("{d} between '1996-01-01' and '1994-01-01'"),
        format!("{d} in ('1994-01-01', '1995-06-15', '1999-12-31')"),
        format!("{d} not in ('1994-01-01', '1995-06-15', '1999-12-31')"),
        format!("substr({d}, 1, 4) = '1995'"),
        format!("substr({d}, 1, 4) <> '1995'"),
        format!("substr({d}, 1, 4) < '1995'"),
        format!("substr({d}, 1, 4) >= '1995'"),
        format!("substr({d}, 1, 4) between '1993' and '1996'"),
        format!("substr({d}, 1, 4) in ('1992', '1998')"),
        format!("substring({d}, 1, 7) = '1995-06'"),
        format!("substr({d}, 1, 7) > '1995-06'"),
        format!("substr({d}, 1, 4) = 1995"),
        format!("{d} < e"),
        format!("{d} >= e"),
        format!("{d} = e"),
        format!("e < {d}"),
        format!("substr({d}, 1, 4) < substr(e, 1, 4)"),
        format!("{d} is null"),
        format!("{d} is not null"),
        format!("not ({d} < '1995-06-15')"),
        format!("{d} < '1995-06-15' or v > 900"),
        format!("{d} >= '1994-01-01' and {d} < '1995-01-01' and v < 500"),
        format!("({d} < '1992-01-01' or {d} > '1998-01-01') and not (v < 10)"),
        // malformed / non-date literals: must fall back to string comparison
        format!("{d} < '1995-6-15'"),
        format!("{d} < '1995'"),
        format!("{d} = '1995-06-15 00:00:00'"),
        format!("{d} >= '1995-06-15T00:00:00'"),
        format!("{d} < 'abc'"),
        format!("{d} >= ''"),
        format!("{d} < '1995-06-1x'"),
        format!("{d} in ('1995-06-15', 'zzz')"),
        format!("{d} between '1995' and '1996'"),
        format!("substr({d}, 1, 4) = '19950'"),
        format!("substr({d}, 1, 4) = '95'"),
        format!("substr({d}, 1, 4) < '1995-01'"),
        format!("substr({d}, 2, 4) = '995-'"),
    ]);
    p
}

fn run_all(ctx: &KqlContext, table: &str, col: &str, expect_accel: Option<bool>, label: &str) {
    let dim = format!("{table}, dim");
    let before = kore_sql::testing::date_accel_hits();
    let mut results: Vec<(String, Vec<String>, Vec<String>, Vec<String>)> = Vec::new();
    for p in predicates(col) {
        let q1 = format!("select count(*) as n, sum(v) as sv, min(id) as mn, max(id) as mx from {table} where {p}");
        let q2 = format!("select id from {table} where {p} order by id");
        let q3 = format!("select count(*) as n, sum(w) as sw from {dim} where {table}.k = dim.k and {p}");
        results.push((p.clone(), rows(ctx, &q1), rows(ctx, &q2), rows(ctx, &q3)));
    }
    let used = kore_sql::testing::date_accel_hits() > before;
    match expect_accel {
        Some(want) => assert_eq!(used, want, "{label}: accelerator use (hits) mismatch"),
        None => {}
    }
    testing::set_no_date_accel(true);
    let h = kore_sql::testing::date_accel_hits();
    for (p, a1, a2, a3) in &results {
        let q1 = format!("select count(*) as n, sum(v) as sv, min(id) as mn, max(id) as mx from {table} where {p}");
        let q2 = format!("select id from {table} where {p} order by id");
        let q3 = format!("select count(*) as n, sum(w) as sw from {dim} where {table}.k = dim.k and {p}");
        assert_eq!(a1, &rows(ctx, &q1), "{label}: fused/agg differs for {p}");
        assert_eq!(a2, &rows(ctx, &q2), "{label}: filter differs for {p}");
        assert_eq!(a3, &rows(ctx, &q3), "{label}: join differs for {p}");
    }
    assert_eq!(h, kore_sql::testing::date_accel_hits(), "switch must disable the accelerator");
    testing::set_no_date_accel(false);
}

#[test]
fn date_accelerator_matches_string_path() {
    // multi-window table (> 65536 rows) and a small one (> the accelerator's minimum)
    for (n, name) in [(150_000usize, "t"), (3_000, "s")] {
        let mut ctx = KqlContext::new();
        ctx.register(name, block(n, 11, None));
        ctx.register("dim", small_dim());
        run_all(&ctx, name, "d", Some(true), name);
    }

    // below the minimum size the accelerator stays off but answers are the same
    {
        let mut ctx = KqlContext::new();
        ctx.register("tiny", block(100, 5, None));
        ctx.register("dim", small_dim());
        run_all(&ctx, "tiny", "d", Some(false), "tiny");
    }

    // one malformed value anywhere disables the column; answers still equal the string path
    for bad in ["1995-06-15 10:00:00", "1995-6-15", "x", "", "1995-06-15 ", "１995-06-15", "1995/06/15", "1995-06-1\u{e9}"] {
        for pos in [0usize, 2_999] {
            let mut ctx = KqlContext::new();
            ctx.register("b", block(3_000, 21, Some((pos, bad))));
            ctx.register("dim", small_dim());
            // the clean column `e` is still accelerated, so only the d-only queries can assert non-use
            let h = testing::date_accel_hits();
            assert_eq!(rows(&ctx, "select count(*) from b where d < '1995-06-15' or substr(d, 1, 4) = '1995'").len(), 1);
            assert_eq!(h, testing::date_accel_hits(), "bad {bad:?} at {pos}: malformed column must not be accelerated");
            run_all(&ctx, "b", "d", None, &format!("bad {bad:?} at {pos}"));
        }
    }

    // DML: every statement replaces the table, so the cache must not leak across versions
    let mut ctx = KqlContext::new();
    ctx.register_mut("m", block(3_000, 31, None));
    ctx.register("dim", small_dim());
    run_all(&ctx, "m", "d", Some(true), "m initial");

    ctx.execute_dml("INSERT INTO m VALUES (100000, 3, 7, '1995-06-15', '1995-06-16')").unwrap();
    run_all(&ctx, "m", "d", Some(true), "after well-formed insert");

    ctx.execute_dml("INSERT INTO m VALUES (100001, 3, 7, '1995-06-15 12:00:00', '1995-06-16')").unwrap();
    run_all(&ctx, "m", "d", None, "after malformed insert");

    ctx.execute_dml("DELETE FROM m WHERE id = 100001").unwrap();
    run_all(&ctx, "m", "d", Some(true), "after deleting the malformed row");

    ctx.execute_dml("UPDATE m SET d = '1995-06-15' WHERE id < 100").unwrap();
    run_all(&ctx, "m", "d", Some(true), "after update to a date");

    ctx.execute_dml("UPDATE m SET d = 'oops' WHERE id = 7").unwrap();
    run_all(&ctx, "m", "d", None, "after update to junk");

    ctx.execute_dml("DELETE FROM m WHERE d = 'oops'").unwrap();
    run_all(&ctx, "m", "d", Some(true), "after delete of the junk row");

    // re-registering the table under the same name starts from a clean cache
    ctx.register_mut("m", block(3_000, 77, None));
    run_all(&ctx, "m", "d", Some(true), "after re-register");

    // a filter must see the new data, not a stale encoding (explicit check on top of the differential)
    let before = rows(&ctx, "select count(*) from m where d = '2001-02-03'");
    assert_eq!(before, vec!["0"]);
    ctx.execute_dml("UPDATE m SET d = '2001-02-03' WHERE id = 5").unwrap();
    assert_eq!(rows(&ctx, "select count(*) from m where d = '2001-02-03'"), vec!["1"]);
}
