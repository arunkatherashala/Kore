//! Multi-key ORDER BY: all keys compared lexicographically in one stable sort.
//!
//! Before, ORDER BY sorted once per key (last key first) with an unstable sort, so a later key was not honoured on
//! ties of an earlier key. Found by Digital Life World M3: `SELECT g, COUNT(*) AS c FROM t GROUP BY g ORDER BY c DESC,
//! g ASC LIMIT 3` returned the wrong top 3 on 25.4% of real ties (docs/measured.md, S5).
//!
//! The reference is a deterministic Rust model (rusqlite is not available offline in this workspace): rows are sorted
//! with `Vec::sort_by` (stable) and a lexicographic comparator with this engine's NULL placement:
//! - no NULLS clause: NULL is the smallest value of every type (first ascending, last descending, as Spark);
//! - NULLS FIRST / NULLS LAST: NULLs go first / last whatever the direction.
//!
//! Engine sync (master-kore-engine 236ae86c): the old copy placed NULL per type without a NULLS clause (Int64 as
//! i64::MIN, Float64 as f64::MAX, text as ""); master's engine has the SQL / Spark default above, so the reference
//! follows it. The engine side of this test is master's ORDER BY, unchanged.

use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::KqlContext;
use std::cmp::Ordering;

fn cell(c: &Column, r: usize) -> String {
    match &c.data {
        ColumnData::Int64(v) => v[r].map(|x| x.to_string()).unwrap_or("NULL".into()),
        ColumnData::Float64(v) => v[r].map(|x| format!("{x}")).unwrap_or("NULL".into()),
        ColumnData::Bool(v) => v[r].map(|x| (x as i32).to_string()).unwrap_or("NULL".into()),
        ColumnData::Str(v) => v[r].clone().unwrap_or("NULL".into()),
        ColumnData::StrDict { codes, dict } => if codes[r] == u8::MAX { "NULL".into() } else { dict[codes[r] as usize].clone() },
    }
}

fn rows(c: &KqlContext, sql: &str) -> Vec<Vec<String>> {
    let b = c.query(sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
    (0..b.num_rows).map(|r| b.columns.iter().map(|col| cell(col, r)).collect()).collect()
}

fn col0(c: &KqlContext, sql: &str) -> Vec<String> { rows(c, sql).into_iter().map(|r| r[0].clone()).collect() }

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 { self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); self.0 >> 33 }
    fn below(&mut self, n: u64) -> u64 { self.next() % n }
    fn chance(&mut self, pct: u64) -> bool { self.below(100) < pct }
}

// ───────────────────────── hand-written cases ─────────────────────────

fn m3_ctx() -> KqlContext {
    // ties on the first key everywhere: the second key must decide
    let mut c = KqlContext::new();
    let ids: Vec<i64> = (1..=12).collect();
    let a = [3, 1, 3, 2, 3, 1, 2, 3, 1, 2, 3, 1];
    let s = ["m", "b", "a", "z", "c", "a", "b", "m", "z", "a", "b", "c"];
    c.register("t", DataBlock::new(vec![
        Column::int64("id", ids.iter().rev().map(|&v| Some(v)).collect()),
        Column::int64("a", a.iter().map(|&v| Some(v)).collect()),
        Column::float64("f", a.iter().map(|&v| Some(v as f64 / 2.0)).collect()),
        Column::str_col("s", s.iter().map(|v| Some(v.to_string())).collect()),
    ]).unwrap());
    c
}

#[test]
fn second_key_decides_on_ties_int_desc_then_id_asc() {
    let c = m3_ctx();
    // id is 12..1 in table order; a = 3 on ids 12,10,8,5,2 ...
    assert_eq!(col0(&c, "SELECT id FROM t ORDER BY a DESC, id ASC"), ["2", "5", "8", "10", "12", "3", "6", "9", "1", "4", "7", "11"]);
    assert_eq!(col0(&c, "SELECT id FROM t ORDER BY a DESC, id ASC LIMIT 3"), ["2", "5", "8"]);
    assert_eq!(col0(&c, "SELECT id FROM t ORDER BY a ASC, id DESC LIMIT 4"), ["11", "7", "4", "1"]);
    assert_eq!(col0(&c, "SELECT id FROM t ORDER BY f DESC, id ASC LIMIT 3"), ["2", "5", "8"]);
}

#[test]
fn three_keys_with_strings_and_mixed_directions() {
    let c = m3_ctx();
    // a DESC, s ASC, id DESC: a=3 rows are (id,s) = (12,m),(10,a),(8,c),(5,m),(2,b)
    assert_eq!(col0(&c, "SELECT id FROM t ORDER BY a DESC, s ASC, id DESC LIMIT 5"), ["10", "2", "8", "12", "5"]);
    assert_eq!(col0(&c, "SELECT id FROM t ORDER BY s DESC, a ASC, id ASC LIMIT 4"), ["4", "9", "5", "12"]);
}

#[test]
fn m3_shape_group_count_top3_smallest_key_on_ties() {
    // GROUP BY g, COUNT(*) DESC, g ASC LIMIT 3: counts e=3, b=3, d=3, a=3, c=1 -> a, b, d
    let mut c = KqlContext::new();
    let g = ["e", "b", "d", "a", "e", "c", "b", "d", "a", "e", "a", "b", "d"];
    c.register("t", DataBlock::new(vec![
        Column::int64("id", (1..=g.len() as i64).map(Some).collect()),
        Column::str_col("g", g.iter().map(|v| Some(v.to_string())).collect()),
    ]).unwrap());
    assert_eq!(rows(&c, "SELECT g, COUNT(*) AS n FROM t GROUP BY g ORDER BY n DESC, g ASC LIMIT 3"),
        [["a", "3"], ["b", "3"], ["d", "3"]]);
    assert_eq!(rows(&c, "SELECT g, COUNT(*) AS n FROM t GROUP BY g ORDER BY n DESC, g DESC LIMIT 2"),
        [["e", "3"], ["d", "3"]]);
    // integer group key, as in the M3 task (smallest id on ties)
    let mut c = KqlContext::new();
    let k = [7, 3, 9, 3, 7, 9, 1, 5, 5, 1];
    c.register("t", DataBlock::new(vec![Column::int64("k", k.iter().map(|&v| Some(v)).collect())]).unwrap());
    assert_eq!(rows(&c, "SELECT k, COUNT(*) AS n FROM t GROUP BY k ORDER BY n DESC, k ASC LIMIT 3"),
        [["1", "2"], ["3", "2"], ["5", "2"]]);
}

#[test]
fn second_key_decides_on_ties_large_table() {
    // 600 rows, 3 values of a: the unstable per-key sort scrambled the id order inside each a
    let mut c = KqlContext::new();
    let n = 600i64;
    c.register("t", DataBlock::new(vec![
        Column::int64("id", (0..n).map(|i| Some((i * 37) % n)).collect()),
        Column::int64("a", (0..n).map(|i| Some(i % 3)).collect()),
    ]).unwrap());
    let got: Vec<i64> = col0(&c, "SELECT id FROM t ORDER BY a DESC, id ASC").iter().map(|x| x.parse().unwrap()).collect();
    let mut want: Vec<(i64, i64)> = (0..n).map(|i| (i % 3, (i * 37) % n)).collect();
    want.sort_by(|x, y| y.0.cmp(&x.0).then(x.1.cmp(&y.1)));
    assert_eq!(got, want.iter().map(|w| w.1).collect::<Vec<_>>());
    let got = col0(&c, "SELECT id FROM t ORDER BY a DESC, id ASC LIMIT 3");
    assert_eq!(got, ["2", "5", "8"]);
}

#[test]
fn full_ties_keep_input_order() {
    let c = m3_ctx();
    // a = 1 on table rows 2,6,9,12 -> ids 11,7,4,1 in table order
    assert_eq!(col0(&c, "SELECT id FROM t WHERE a = 1 ORDER BY a"), ["11", "7", "4", "1"]);
    assert_eq!(col0(&c, "SELECT id FROM t WHERE a = 1 ORDER BY a DESC, f ASC"), ["11", "7", "4", "1"]);
}

#[test]
fn nulls_with_explicit_clause_and_second_key() {
    let mut c = KqlContext::new();
    c.register("t", DataBlock::new(vec![
        Column::int64("id", (1..=7).map(Some).collect()),
        Column::int64("a", vec![Some(2), None, Some(1), None, Some(2), Some(1), None]),
        Column::str_col("s", vec![Some("bb".into()), None, Some("ba".into()), Some("x".into()), Some("bc".into()), None, None]),
    ]).unwrap());
    assert_eq!(col0(&c, "SELECT id FROM t ORDER BY a ASC NULLS FIRST, id DESC"), ["7", "4", "2", "6", "3", "5", "1"]);
    assert_eq!(col0(&c, "SELECT id FROM t ORDER BY a DESC NULLS LAST, id ASC"), ["1", "5", "3", "6", "2", "4", "7"]);
    // text with an explicit NULLS clause compares whole strings (before: the first byte only)
    assert_eq!(col0(&c, "SELECT id FROM t ORDER BY s ASC NULLS LAST, id ASC"), ["3", "1", "5", "4", "2", "6", "7"]);
    assert_eq!(col0(&c, "SELECT id FROM t ORDER BY s DESC NULLS FIRST, id DESC LIMIT 5"), ["7", "6", "2", "4", "5"]);
}

#[test]
fn window_order_by_multikey_and_text() {
    let c = m3_ctx();
    // ROW_NUMBER over (a DESC, id ASC): the second key decides on ties
    let r = rows(&c, "SELECT id, ROW_NUMBER() OVER (ORDER BY a DESC, id ASC) AS rn FROM t ORDER BY rn LIMIT 4");
    assert_eq!(r, [["2", "1"], ["5", "2"], ["8", "3"], ["10", "4"]]);
    // a single text key in a window ORDER BY is sorted (before: left in table order)
    let r = rows(&c, "SELECT s, ROW_NUMBER() OVER (ORDER BY s) AS rn FROM t ORDER BY rn LIMIT 3");
    assert_eq!(r, [["a", "1"], ["a", "2"], ["a", "3"]]);
    let r = rows(&c, "SELECT id, ROW_NUMBER() OVER (ORDER BY s DESC, id ASC) AS rn FROM t ORDER BY rn LIMIT 3");
    assert_eq!(r, [["4", "1"], ["9", "2"], ["5", "3"]]);
}

// ───────────────────────── randomized, against the reference ─────────────────────────

#[derive(Clone)]
enum V { I(Option<i64>), F(Option<f64>), S(Option<String>) }

/// The reference comparison of one key, with this engine's NULL placement (see the module doc).
fn ref_cmp(a: &V, b: &V, desc: bool, nulls: Option<bool>) -> Ordering {
    let dir = |o: Ordering| if desc { o.reverse() } else { o };
    // no NULLS clause: NULL is the smallest value (NULLS FIRST ascending, NULLS LAST descending)
    if let Some(nf) = nulls.or(Some(!desc)) {
        let (an, bn) = match (a, b) {
            (V::I(x), V::I(y)) => (x.is_none(), y.is_none()),
            (V::F(x), V::F(y)) => (x.is_none(), y.is_none()),
            (V::S(x), V::S(y)) => (x.is_none(), y.is_none()),
            _ => unreachable!(),
        };
        match (an, bn) {
            (true, true) => return Ordering::Equal,
            (true, false) => return if nf { Ordering::Less } else { Ordering::Greater },
            (false, true) => return if nf { Ordering::Greater } else { Ordering::Less },
            _ => {}
        }
    }
    dir(match (a, b) {
        (V::I(x), V::I(y)) => x.unwrap_or(i64::MIN).cmp(&y.unwrap_or(i64::MIN)),
        (V::F(x), V::F(y)) => x.unwrap_or(f64::MAX).partial_cmp(&y.unwrap_or(f64::MAX)).unwrap(),
        (V::S(x), V::S(y)) => x.as_deref().unwrap_or("").cmp(y.as_deref().unwrap_or("")),
        _ => unreachable!(),
    })
}

#[test]
fn randomized_multikey_order_by_matches_stable_reference() {
    const COLS: [&str; 5] = ["i", "j", "f", "s", "d"]; // d: dictionary-encoded text
    const STRS: [&str; 6] = ["", "a", "ab", "b", "ba", "zz"];
    let mut cases = 0;
    for seed in 1..=60u64 {
        let mut r = Rng(seed * 7919);
        let n = 1 + r.below(300) as usize;
        let null_pct = [0, 10, 40][r.below(3) as usize];
        let card = [2, 3, 5, 50][r.below(4) as usize];
        let mut data: Vec<Vec<V>> = Vec::new();
        for _ in 0..n {
            let mut row = vec![];
            row.push(V::I(if r.chance(null_pct) { None } else { Some(r.below(card) as i64 - 1) }));
            row.push(V::I(if r.chance(null_pct) { None } else { Some(r.below(card) as i64) }));
            row.push(V::F(if r.chance(null_pct) { None } else { Some(r.below(card) as f64 * 0.5 - 0.5) }));
            row.push(V::S(if r.chance(null_pct) { None } else { Some(STRS[r.below(card.min(6)) as usize].to_string()) }));
            row.push(V::S(if r.chance(null_pct) { None } else { Some(STRS[r.below(card.min(6)) as usize].to_string()) }));
            data.push(row);
        }
        let ints = |k: usize| data.iter().map(|row| match &row[k] { V::I(x) => *x, _ => unreachable!() }).collect::<Vec<_>>();
        let strs = |k: usize| data.iter().map(|row| match &row[k] { V::S(x) => x.clone(), _ => unreachable!() }).collect::<Vec<_>>();
        let floats = data.iter().map(|row| match &row[2] { V::F(x) => *x, _ => unreachable!() }).collect::<Vec<_>>();
        let mut dict: Vec<String> = Vec::new();
        let codes = strs(4).iter().map(|x| match x {
            None => u8::MAX,
            Some(t) => match dict.iter().position(|y| y == t) { Some(p) => p as u8, None => { dict.push(t.clone()); (dict.len() - 1) as u8 } },
        }).collect();
        let mut c = KqlContext::new();
        c.register("t", DataBlock::new(vec![
            Column::int64("id", (0..n as i64).map(Some).collect()),
            Column::int64("i", ints(0)),
            Column::int64("j", ints(1)),
            Column::float64("f", floats),
            Column::str_col("s", strs(3)),
            Column::str_dict("d", codes, dict),
        ]).unwrap());

        for _q in 0..20 {
            let nk = 1 + r.below(3) as usize;
            let mut keys: Vec<(usize, bool, Option<bool>)> = vec![];
            for _ in 0..nk {
                let k = r.below(5) as usize;
                let nulls = match r.below(4) { 0 => Some(true), 1 => Some(false), _ => None };
                keys.push((k, r.chance(50), nulls));
            }
            let limit = if r.chance(50) { Some(1 + r.below(10) as usize) } else { None };
            let order = keys.iter().map(|&(k, desc, nulls)| format!("{}{}{}", COLS[k], if desc { " DESC" } else { " ASC" },
                match nulls { Some(true) => " NULLS FIRST", Some(false) => " NULLS LAST", None => "" })).collect::<Vec<_>>().join(", ");
            let sql = format!("SELECT id FROM t ORDER BY {order}{}", limit.map(|l| format!(" LIMIT {l}")).unwrap_or_default());

            let mut want: Vec<usize> = (0..n).collect();
            want.sort_by(|&a, &b| {
                for &(k, desc, nulls) in &keys {
                    let o = ref_cmp(&data[a][k], &data[b][k], desc, nulls);
                    if o != Ordering::Equal { return o; }
                }
                Ordering::Equal
            });
            if let Some(l) = limit { want.truncate(l); }
            let want: Vec<String> = want.iter().map(|i| i.to_string()).collect();
            assert_eq!(col0(&c, &sql), want, "seed {seed}: {sql}");
            cases += 1;
        }
    }
    assert_eq!(cases, 1200);
}

#[test]
fn randomized_group_count_top_k_matches_reference() {
    // the M3 shape on many random tables: GROUP BY k, COUNT(*) DESC, k ASC LIMIT 3
    for seed in 1..=200u64 {
        let mut r = Rng(seed * 104729);
        // large enough that the sort is not an insertion sort (stable by accident below about 20 rows)
        let n = 1 + r.below(3000) as usize;
        let card = 2 + r.below(60);
        let k: Vec<i64> = (0..n).map(|_| r.below(card) as i64).collect();
        let mut c = KqlContext::new();
        c.register("t", DataBlock::new(vec![Column::int64("k", k.iter().map(|&v| Some(v)).collect())]).unwrap());
        let mut counts: std::collections::BTreeMap<i64, i64> = Default::default();
        for &v in &k { *counts.entry(v).or_default() += 1; }
        let mut want: Vec<(i64, i64)> = counts.into_iter().collect();
        want.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        want.truncate(3);
        let want: Vec<Vec<String>> = want.iter().map(|(k, n)| vec![k.to_string(), n.to_string()]).collect();
        assert_eq!(rows(&c, "SELECT k, COUNT(*) AS n FROM t GROUP BY k ORDER BY n DESC, k ASC LIMIT 3"), want, "seed {seed}");
    }
}
