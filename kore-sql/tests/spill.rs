//! Spill-to-disk tests: a tiny memory budget must give the same answers as unlimited memory.

use kore_core::{Column, ColumnData, DataBlock, Value};
use kore_sql::KqlContext;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 { self.next() % n }
}

fn rows_of(b: &DataBlock) -> Vec<Vec<String>> {
    (0..b.num_rows).map(|r| b.columns.iter().map(|c| format!("{:?}", c.data.get_value(r))).collect()).collect()
}
fn sorted_rows(b: &DataBlock) -> Vec<Vec<String>> { let mut v = rows_of(b); v.sort(); v }
fn names(b: &DataBlock) -> Vec<String> { b.columns.iter().map(|c| c.name.clone()).collect() }

/// Table: id (unique), k (Int key with NULLs, duplicates), s (Str with NULLs), v (Float with NULLs).
fn table(n: usize, key_card: u64, seed: u64, null_every: u64) -> DataBlock {
    let mut g = Rng(seed);
    let mut id = vec![]; let mut k = vec![]; let mut s = vec![]; let mut v = vec![];
    for i in 0..n {
        id.push(Some(i as i64));
        k.push(if g.below(null_every) == 0 { None } else { Some(g.below(key_card) as i64 - 3) });
        s.push(if g.below(null_every) == 0 { None } else { Some(format!("s{}", g.below(key_card))) });
        v.push(if g.below(null_every) == 0 { None } else { Some((g.below(2000) as f64) / 8.0 - 100.0) });
    }
    DataBlock { num_rows: n, columns: vec![
        Column { name: "id".into(), data: ColumnData::Int64(id) },
        Column { name: "k".into(),  data: ColumnData::Int64(k) },
        Column { name: "s".into(),  data: ColumnData::Str(s) },
        Column { name: "v".into(),  data: ColumnData::Float64(v) },
    ]}
}

fn pair(l: DataBlock, r: DataBlock) -> (KqlContext, KqlContext) {
    let mut a = KqlContext::new(); a.register("l", l); a.register("r", r);
    let mut b = a.clone(); b.set_memory_limit(1); // 1 byte: everything must spill
    (a, b)
}

#[test]
fn default_is_unlimited_and_never_spills() {
    let mut c = KqlContext::new();
    assert_eq!(c.memory_limit(), None);
    c.register("l", table(500, 10, 1, 7));
    c.query("SELECT * FROM l ORDER BY v DESC").unwrap();
    c.query("SELECT k, SUM(v) AS t FROM l GROUP BY k").unwrap();
    assert_eq!(c.spill_stats().files_created, 0);
}

#[test]
fn join_all_types_match_unlimited() {
    // Left has NULL keys and heavy duplicates; few distinct keys => many empty partitions.
    let cases = [
        (table(1500, 40, 11, 9), table(900, 40, 12, 11)),
        (table(800, 3, 13, 5),   table(60, 3, 14, 4)),      // very few keys: nearly all partitions empty
        (table(300, 20, 15, 6),  table(0, 20, 16, 6)),      // empty right side
        (table(0, 20, 17, 6),    table(300, 20, 18, 6)),    // empty left side
    ];
    for (ci, (l, r)) in cases.into_iter().enumerate() {
        let (mem, spill) = pair(l, r);
        for jt in ["INNER JOIN", "LEFT JOIN", "FULL JOIN"] {
            for key in ["k", "s"] {
                let sql = format!("SELECT * FROM l AS a {jt} r AS b ON a.{key} = b.{key}");
                let want = mem.query(&sql).unwrap();
                let got = spill.query(&sql).unwrap();
                assert_eq!(names(&want), names(&got), "case {ci} {sql}");
                assert_eq!(want.num_rows, got.num_rows, "case {ci} {sql}");
                assert_eq!(sorted_rows(&want), sorted_rows(&got), "case {ci} {sql}");
            }
        }
        if ci == 0 { assert!(spill.spill_stats().join_spills > 0); }
    }
}

#[test]
fn join_with_null_keys_only() {
    let n = |len: usize| DataBlock { num_rows: len, columns: vec![
        Column { name: "k".into(), data: ColumnData::Int64(vec![None; len]) },
        Column { name: "x".into(), data: ColumnData::Int64((0..len as i64).map(Some).collect()) },
    ]};
    let (mem, spill) = pair(n(20), n(30));
    for jt in ["INNER JOIN", "LEFT JOIN", "FULL JOIN"] {
        let sql = format!("SELECT * FROM l AS a {jt} r AS b ON a.k = b.k");
        assert_eq!(sorted_rows(&mem.query(&sql).unwrap()), sorted_rows(&spill.query(&sql).unwrap()), "{sql}");
    }
}

#[test]
fn group_by_multi_key_with_nulls_matches_exactly() {
    for (n, card, nulls) in [(3000usize, 6u64, 5u64), (2000, 40, 3), (500, 1, 4), (1, 2, 2)] {
        let mut mem = KqlContext::new();
        mem.register("t", table(n, card, 99 + n as u64, nulls));
        let mut spill = mem.clone();
        spill.set_memory_limit(1);
        for sql in [
            "SELECT k, s, SUM(v) AS sv, COUNT(v) AS c, MIN(v) AS mn, MAX(v) AS mx, AVG(v) AS av FROM t GROUP BY k, s",
            "SELECT s, COUNT(id) AS c, SUM(id) AS si FROM t GROUP BY s",
            "SELECT k, SUM(v) AS sv FROM t GROUP BY k HAVING sv > 0",
            "SELECT k, s FROM t GROUP BY k, s",
        ] {
            let want = mem.query(sql).unwrap();
            let got = spill.query(sql).unwrap();
            assert_eq!(names(&want), names(&got), "{sql}");
            // identical, including group order (first appearance) and float bits
            assert_eq!(rows_of(&want), rows_of(&got), "n={n} {sql}");
        }
        assert!(spill.spill_stats().group_spills > 0);
    }
}

#[test]
fn group_by_empty_table() {
    let mut mem = KqlContext::new();
    mem.register("t", table(0, 3, 1, 3));
    let mut spill = mem.clone();
    spill.set_memory_limit(1);
    let sql = "SELECT k, SUM(v) AS sv FROM t GROUP BY k";
    assert_eq!(rows_of(&mem.query(sql).unwrap()), rows_of(&spill.query(sql).unwrap()));
}

#[test]
fn order_by_single_key_ties_nulls_desc() {
    let mut mem = KqlContext::new();
    mem.register("t", table(2500, 12, 5, 6));
    let mut spill = mem.clone();
    spill.set_memory_limit(1);
    for col in ["k", "v", "s", "id"] {
        for dir in ["ASC", "DESC"] {
            let sql = format!("SELECT * FROM t ORDER BY {col} {dir}");
            let want = mem.query(&sql).unwrap();
            let got = spill.query(&sql).unwrap();
            let ci = want.columns.iter().position(|c| c.name.ends_with(col)).unwrap();
            // the sort-key column must be identical position by position
            let wk: Vec<_> = rows_of(&want).into_iter().map(|r| r[ci].clone()).collect();
            let gk: Vec<_> = rows_of(&got).into_iter().map(|r| r[ci].clone()).collect();
            assert_eq!(wk, gk, "{sql}");
            // and no row may be lost or duplicated
            assert_eq!(sorted_rows(&want), sorted_rows(&got), "{sql}");
        }
    }
    assert!(spill.spill_stats().sort_spills > 0);
}

#[test]
fn order_by_multi_key_exact() {
    let mut mem = KqlContext::new();
    mem.register("t", table(2500, 5, 21, 6));
    let mut spill = mem.clone();
    spill.set_memory_limit(1);
    for sql in [
        "SELECT * FROM t ORDER BY k DESC, s, id",
        "SELECT * FROM t ORDER BY s, v DESC, id DESC",
        "SELECT * FROM t ORDER BY k, v", // ties remain: both paths are stable on input order
        "SELECT id, k FROM t ORDER BY k DESC, v DESC LIMIT 50",
    ] {
        let want = mem.query(sql).unwrap();
        let got = spill.query(sql).unwrap();
        assert_eq!(rows_of(&want), rows_of(&got), "{sql}");
    }
}

#[test]
fn multi_key_order_by_is_lexicographic() {
    // Second key must hold within ties of the first key.
    let mut c = KqlContext::new();
    c.register("t", table(3000, 4, 77, 1_000_000));
    let r = c.query("SELECT k, id FROM t ORDER BY k, id DESC").unwrap();
    let g = |col: usize, i: usize| match r.columns[col].data.get_value(i) { Value::Int(x) => x, _ => i64::MIN };
    for i in 1..r.num_rows {
        let (a, b) = ((g(0, i - 1), g(1, i - 1)), (g(0, i), g(1, i)));
        assert!(a.0 < b.0 || (a.0 == b.0 && a.1 > b.1), "{:?} then {:?}", a, b);
    }
}

#[test]
fn combined_query_and_dir_cleanup() {
    let dir = std::env::temp_dir().join(format!("kore-sql-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut mem = KqlContext::new();
    mem.register("l", table(1200, 15, 3, 7));
    mem.register("r", table(400, 15, 4, 7));
    let mut spill = mem.clone();
    spill.set_memory_limit(2048);
    spill.set_spill_dir(&dir);
    let sql = "SELECT a.k, SUM(b.v) AS t, COUNT(a.id) AS n FROM l AS a INNER JOIN r AS b ON a.k = b.k \
               GROUP BY a.k ORDER BY t DESC, k";
    let want = mem.query(sql);
    let got = spill.query(sql);
    match (want, got) {
        (Ok(w), Ok(g)) => assert_eq!(rows_of(&w), rows_of(&g)),
        (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
        (a, b) => panic!("mismatch: ok={} vs ok={}", a.is_ok(), b.is_ok()),
    }
    let st = spill.spill_stats();
    assert!(st.files_created > 0 && st.bytes_written > 0);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "spill files must be removed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn right_and_implicit_joins_match_unlimited() {
    let (a, b) = pair(table(700, 30, 21, 9), table(500, 30, 22, 7));
    for sql in [
        "SELECT x.id, y.id FROM l AS x, r AS y WHERE x.k = y.k",
        "SELECT x.id, y.id FROM l AS x, r AS y WHERE x.k = y.k AND x.s = y.s",
    ] {
        let want = a.query(sql).unwrap_or_else(|e| panic!("{sql}: {e:?}"));
        let got = b.query(sql).unwrap();
        assert_eq!(names(&want), names(&got), "{sql}");
        assert_eq!(sorted_rows(&want), sorted_rows(&got), "{sql}");
    }
    assert!(b.spill_stats().join_spills >= 2);
}
