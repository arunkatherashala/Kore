//! Random multi-table query fuzzer, two oracles:
//!  * in-process differential: optimised path vs `set_no_fused` vs `set_force_general` vs `set_no_vecexpr`;
//!  * external SQLite oracle: the test dumps the tables (CSV, `\N` = NULL), the queries and kore's answers to
//!    `FUZZ_DIR`, and `kore-sql/tests/oracle/compare.py` replays them in SQLite.
//!
//! `#[ignore]`d (it is a fuzzer, sized by env vars):
//!   FUZZ_SCALE=small|mid|large  FUZZ_SEEDS=n  FUZZ_PER=queries-per-seed  FUZZ_DIR=out-dir
//! Run: cargo test --release -p kore-sql --test fuzz_oracle -- --ignored --nocapture

use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::{testing, KqlContext};
use std::fmt::Write as _;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize { (self.next() % n.max(1) as u64) as usize }
    fn chance(&mut self, pct: usize) -> bool { self.below(100) < pct }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T { &v[self.below(v.len())] }
}

struct Sizes { a: usize, b: usize, c: usize, d: usize, e: usize, ka: usize }

fn sizes(scale: &str, r: &mut Rng) -> Sizes {
    match scale {
        "large" => Sizes { a: 120_000 + r.below(60_000), b: 20_000 + r.below(20_000), c: 2000, d: 700, e: 6 + r.below(20), ka: 2500 },
        "mid" => Sizes { a: 9_000 + r.below(20_000), b: 3000 + r.below(3000), c: 300, d: 120, e: 5 + r.below(10), ka: 400 },
        _ => Sizes { a: 40 + r.below(40), b: 20 + r.below(30), c: 8 + r.below(10), d: 5 + r.below(8), e: r.below(6), ka: 10 + r.below(8) },
    }
}

const STRS: [&str; 12] = ["ab", "abc", "b", "", "Ab", "ba", "xyz", "abab", "zz", "a", "bb", "hello"];

fn onull<T>(r: &mut Rng, pct: usize, v: T) -> Option<T> { if r.chance(pct) { None } else { Some(v) } }

/// Columns of every table: k1, k2 (join keys), g (small group domain), v (int), f (float), s (text).
/// `uniq`: k1 is unique (a permutation prefix of 0..n, so key domain is `n` or `ka` whichever is larger).
fn make_table(r: &mut Rng, n: usize, ka: usize, uniq: bool, with_id: bool, null_pct: usize, dict: bool) -> DataBlock {
    let mut cols = Vec::new();
    if with_id { cols.push(Column::int64("id", (1..=n as i64).map(Some).collect())); }
    let k1: Vec<Option<i64>> = if uniq {
        let mut perm: Vec<i64> = (0..ka.max(n) as i64).collect();
        for i in (1..perm.len()).rev() { let j = r.below(i + 1); perm.swap(i, j); }
        perm.into_iter().take(n).map(|v| onull(r, null_pct, v)).collect()
    } else {
        (0..n).map(|_| { let v = r.below(ka) as i64; onull(r, null_pct, v) }).collect()
    };
    cols.push(Column::int64("k1", k1));
    cols.push(Column::int64("k2", (0..n).map(|_| { let v = r.below(5) as i64; onull(r, null_pct, v) }).collect()));
    cols.push(Column::int64("g", (0..n).map(|_| { let v = r.below(7) as i64; onull(r, null_pct, v) }).collect()));
    cols.push(Column::int64("v", (0..n).map(|_| { let v = r.below(101) as i64 - 50; onull(r, null_pct + 5, v) }).collect()));
    cols.push(Column::float64("f", (0..n).map(|_| { let v = r.below(2000) as f64 / 8.0 - 100.0; onull(r, null_pct + 5, v) }).collect()));
    let strs: Vec<Option<String>> = (0..n).map(|_| { let v = r.pick(&STRS).to_string(); onull(r, null_pct, v) }).collect();
    if dict {
        let mut d: Vec<String> = Vec::new();
        let codes: Vec<u8> = strs.iter().map(|x| match x {
            None => u8::MAX,
            Some(t) => match d.iter().position(|y| y == t) { Some(i) => i as u8, None => { d.push(t.clone()); (d.len() - 1) as u8 } },
        }).collect();
        cols.push(Column::str_dict("s", codes, d));
    } else {
        cols.push(Column::str_col("s", strs));
    }
    // high-cardinality group key (many groups: forces partitioned / hashed group-by)
    let hi = (n / 3).max(2);
    cols.push(Column::int64("h", (0..n).map(|_| { let v = r.below(hi) as i64; onull(r, null_pct / 2, v) }).collect()));
    DataBlock::new(cols).unwrap()
}

const TABS: [&str; 5] = ["ta", "tb", "tc", "td", "te"];
const ALIAS: [&str; 5] = ["a", "b", "c", "d", "e"];

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

fn csv_cell(c: &Column, r: usize) -> String {
    let s = cell(c, r);
    if s == "NULL" && !matches!(&c.data, ColumnData::Str(_) | ColumnData::StrDict { .. }) { return "\\N".into(); }
    match &c.data {
        ColumnData::Str(v) if v[r].is_none() => "\\N".into(),
        ColumnData::StrDict { codes, .. } if codes[r] == u8::MAX => "\\N".into(),
        _ => if s.is_empty() { "\\E".into() } else { s },
    }
}

fn answer(c: &KqlContext, sql: &str) -> Result<Vec<String>, String> {
    let b = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.query(sql))) {
        Ok(Ok(b)) => b,
        Ok(Err(e)) => return Err(format!("error: {e}")),
        Err(_) => return Err("panic".into()),
    };
    Ok((0..b.num_rows).map(|r| b.columns.iter().map(|c| cell(c, r)).collect::<Vec<_>>().join("\t")).collect())
}

fn norm_num(s: &str) -> String {
    match s.parse::<f64>() {
        Ok(x) if !s.is_empty() => if x.fract() == 0.0 && x.abs() < 1e15 { format!("{}", x as i64) } else { format!("{:.6e}", x) },
        _ => s.to_string(),
    }
}
fn canon(rows: &[String], ordered: bool) -> Vec<String> {
    let mut v: Vec<String> = rows.iter().map(|r| r.split('\t').map(norm_num).collect::<Vec<_>>().join("\t")).collect();
    if !ordered { v.sort(); }
    v
}

// ───────────────────────── query generator ─────────────────────────

struct G<'a> { r: &'a mut Rng, n: usize, ks: usize }

impl<'a> G<'a> {
    fn col(&mut self, t: usize, kind: &str) -> String {
        let a = ALIAS[t];
        match kind {
            "int" => format!("{a}.{}", self.r.pick(&["v", "g", "k2", "k1", "h"])),
            "key" => format!("{a}.{}", self.r.pick(&["k1", "k2", "g"])),
            "grp" => format!("{a}.{}", self.r.pick(&["g", "k2", "s", "k1", "h"])),
            "float" => format!("{a}.f"),
            _ => format!("{a}.s"),
        }
    }
    fn tab_pred(&mut self, t: usize) -> String {
        let a = ALIAS[t];
        let cmp = *self.r.pick(&["=", "<>", "<", "<=", ">", ">="]);
        match self.r.below(14) {
            0 => format!("{a}.v {cmp} {}", self.r.below(60) as i64 - 30),
            1 => format!("{a}.g {cmp} {}", self.r.below(7)),
            2 => format!("{a}.f {cmp} {}", self.r.below(200) as f64 / 4.0 - 25.0),
            3 => format!("{a}.s {} '{}'", self.r.pick(&["=", "<>", "<", ">"]), self.r.pick(&STRS)),
            4 => format!("{a}.s like '{}'", self.r.pick(&["a%", "%b", "_b%", "ab", "%", "%b%", "x%z", ""])),
            5 => format!("{a}.s not like '{}'", self.r.pick(&["a%", "%b", "%l%"])),
            6 => format!("{a}.g in ({})", (0..1 + self.r.below(3)).map(|_| self.r.below(7).to_string()).collect::<Vec<_>>().join(", ")),
            7 => format!("{a}.k2 not in ({}, {})", self.r.below(5), self.r.below(5)),
            8 => format!("{a}.v between {} and {}", self.r.below(40) as i64 - 30, self.r.below(40) as i64 - 5),
            9 => format!("{a}.{} is {}null", self.r.pick(&["k1", "v", "s", "f", "g"]), if self.r.chance(50) { "not " } else { "" }),
            10 => format!("substr({a}.s, 1, 2) {} '{}'", self.r.pick(&["=", "<>", ">"]), self.r.pick(&["ab", "b", "hel", "a"])),
            11 => format!("{a}.s in ('ab', 'b', 'hello')"),
            12 => format!("{a}.k1 {cmp} {}", self.r.below(self.ks.max(1)) ),
            _ => format!("({a}.v + {a}.g) {cmp} {}", self.r.below(30)),
        }
    }
    fn cross_or(&mut self, ts: &[usize]) -> String {
        let t1 = *self.r.pick(ts);
        let t2 = *self.r.pick(ts);
        let (p1, p2) = (self.tab_pred(t1), self.tab_pred(t2));
        match self.r.below(4) {
            0 => format!("({p1} or {p2})"),
            1 => { let p3 = self.tab_pred(t1); format!("(({p1} and {p2}) or {p3})") }
            2 => format!("({p1} or {p2} or {}.v > {}.v)", ALIAS[t1], ALIAS[t2]),
            _ => format!("(not ({p1}) or {p2})"),
        }
    }

    /// Comma-join FROM/WHERE over `nt` tables; returns (table indexes in FROM order, from clause, where conjuncts).
    fn comma_join(&mut self, nt: usize) -> (Vec<usize>, String, Vec<String>) {
        let mut ts: Vec<usize> = (0..5).collect();
        for i in (1..5).rev() { let j = self.r.below(i + 1); ts.swap(i, j); }
        ts.truncate(nt);
        // the big table `a` is the most interesting; include it often
        if !ts.contains(&0) && self.r.chance(70) { ts[0] = 0; }
        let mut conds = Vec::new();
        let mut order = ts.clone();
        for i in 1..ts.len() {
            let t = ts[i];
            let par = ts[self.r.below(i)];
            match self.r.below(10) {
                0 if self.n < 200 || t == 4 || par == 4 => {} // cross join (only safe when small, or against the tiny table)
                1 => conds.push(format!("{}.k1 = {}.k1 and {}.k2 = {}.k2", ALIAS[t], ALIAS[par], ALIAS[t], ALIAS[par])),
                2 => conds.push(format!("{}.s = {}.s and {}.k1 = {}.k1", ALIAS[t], ALIAS[par], ALIAS[t], ALIAS[par])),
                3 => conds.push(format!("{}.k1 = {}.k2", ALIAS[t], ALIAS[par])),
                _ => conds.push(format!("{}.k1 = {}.k1", ALIAS[t], ALIAS[par])),
            }
            // an equality chain with the join predicate written in the other direction
            if self.r.chance(30) { if let Some(last) = conds.last_mut() { if last.matches(" = ").count() == 1 && last.matches(" and ").count() == 0 { let parts: Vec<&str> = last.split(" = ").collect(); *last = format!("{} = {}", parts[1], parts[0]); } } }
        }
        // cross joins of two big tables are not allowed: if a table ended up unconnected to a big one, fine for small only
        if self.n >= 200 {
            // make sure no big table lacks an equality (avoid |a|*|b| blow-up): add k1 equality when missing
            for i in 1..ts.len() {
                let t = ts[i];
                let a = ALIAS[t];
                if !conds.iter().any(|c| c.contains(&format!("{a}."))) && t <= 1 { let par = ts[0]; conds.push(format!("{a}.k1 = {}.k1", ALIAS[par])); }
            }
        }
        for i in (1..order.len()).rev() { if self.r.chance(40) { let j = self.r.below(i + 1); order.swap(i, j); } }
        let from = order.iter().map(|&t| format!("{} {}", TABS[t], ALIAS[t])).collect::<Vec<_>>().join(", ");
        // filters
        for &t in &ts {
            if self.r.chance(45) { let p = self.tab_pred(t); conds.push(p); }
        }
        if ts.len() > 1 && self.r.chance(25) { let o = self.cross_or(&ts); conds.push(o); }
        (ts, from, conds)
    }

    fn agg(&mut self, ts: &[usize]) -> String {
        let t = *self.r.pick(ts);
        let t2 = *self.r.pick(ts);
        match self.r.below(18) {
            0 => "count(*)".into(),
            1 => format!("count({})", self.col(t, "int")),
            2 => format!("sum({})", self.col(t, "int")),
            3 => format!("sum({})", self.col(t, "float")),
            4 => format!("min({})", self.col(t, "int")),
            5 => format!("max({})", self.col(t, "int")),
            6 => format!("avg({})", self.col(t, "int")),
            7 => format!("avg({})", self.col(t, "float")),
            8 => format!("min({})", self.col(t, "str")),
            9 => format!("max({})", self.col(t, "str")),
            10 => format!("count(distinct {})", self.col(t, "int")),
            11 => format!("sum({} + {})", self.col(t, "int"), self.col(t2, "int")),
            12 => format!("sum({} * {})", self.col(t, "float"), self.col(t2, "int")),
            13 => format!("sum(case when {} then 1 else 0 end)", self.tab_pred(t)),
            14 => format!("sum(case when {} then {} else 0 end)", self.tab_pred(t), self.col(t2, "float")),
            15 => format!("max({}) - min({})", self.col(t, "float"), self.col(t, "float")),
            16 => format!("sum(distinct {})", self.col(t, "int")),
            _ => format!("count({})", self.col(t, "str")),
        }
    }

    fn where_clause(conds: &[String]) -> String {
        if conds.is_empty() { String::new() } else { format!(" where {}", conds.join(" and ")) }
    }

    /// Returns (sql, ordered). Unordered answers are compared as multisets.
    fn query(&mut self) -> (String, bool) {
        let big = self.n >= 200;
        let mut pick = self.r.below(30);
        // the row-based window evaluator is quadratic in the partition size: keep windows to small / mid tables
        if self.n >= 60_000 && matches!(pick, 18 | 19 | 20) { pick = 0; }
        match pick {
            0 | 1 | 2 | 3 => {
                // aggregate over a comma join, group by 0-3 keys
                let nt = 1 + self.r.below(if big { 4 } else { 5 });
                let (ts, from, conds) = self.comma_join(nt);
                let nk = self.r.below(4);
                let keys: Vec<String> = (0..nk).map(|_| { let t = *self.r.pick(&ts); self.col(t, "grp") }).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
                let na = 1 + self.r.below(4);
                let aggs: Vec<String> = (0..na).map(|i| format!("{} as m{i}", self.agg(&ts))).collect();
                let mut sel: Vec<String> = keys.clone();
                sel.extend(aggs);
                let gb = if keys.is_empty() { String::new() } else if self.r.chance(15) { format!(" group by {}", (1..=keys.len()).map(|i| i.to_string()).collect::<Vec<_>>().join(", ")) } else { format!(" group by {}", keys.join(", ")) };
                let having = if !keys.is_empty() && self.r.chance(30) { format!(" having count(*) {} {}", self.r.pick(&[">", ">=", "<"]), self.r.below(6)) } else if !keys.is_empty() && self.r.chance(15) { " having sum(1) > 1 or min(1) < 0".to_string() } else { String::new() };
                (format!("select {} from {}{}{}{}", sel.join(", "), from, Self::where_clause(&conds), gb, having), false)
            }
            4 | 5 => {
                // plain join projection with ORDER BY + LIMIT (total order: every projected column, nulls first)
                let nt = 1 + self.r.below(3);
                let (ts, from, conds) = self.comma_join(nt);
                let nc = 1 + self.r.below(3);
                let mut cols: Vec<String> = Vec::new();
                for _ in 0..nc { let t = *self.r.pick(&ts); let k = *self.r.pick(&["int", "float", "str", "key"]); let c = self.col(t, k); if !cols.contains(&c) { cols.push(c); } }
                let dirs: Vec<String> = cols.iter().map(|c| format!("{c} {} {}", self.r.pick(&["asc", "desc"]), self.r.pick(&["nulls first", "nulls last", "", ""]))).collect();
                let lim = if big { let m = if self.r.chance(30) { 70_000 } else { 120 }; 1 + self.r.below(m) } else { 1 + self.r.below(30) };
                let off = if self.r.chance(40) { format!(" offset {}", self.r.below(20)) } else { String::new() };
                // full tuple ordering is deterministic as a multiset of rows, even with duplicates
                (format!("select {} from {}{} order by {} limit {lim}{off}", cols.join(", "), from, Self::where_clause(&conds), dirs.join(", ")), true)
            }
            6 | 7 => {
                // a-unique-id top N: id tiebreak, sort key with ties
                let sk = *self.r.pick(&["a.g", "a.v", "a.s", "a.f", "a.k2"]);
                let sk2 = *self.r.pick(&["a.v", "a.h", "a.k1"]);
                let extra = if self.r.chance(60) { format!(" where {}", self.tab_pred(0)) } else { String::new() };
                let lim = if self.n >= 200 { *self.r.pick(&[1usize, 10, 100, 1000, 70_000]) } else { 1 + self.r.below(20) };
                (format!("select a.id, {sk}, {sk2} from ta a{extra} order by {sk} {} {}, {sk2} {} {}, a.id limit {lim}{}", self.r.pick(&["asc", "desc"]), self.r.pick(&["nulls first", "nulls last"]), self.r.pick(&["asc", "desc"]), self.r.pick(&["nulls first", "nulls last"]), if self.r.chance(40) { format!(" offset {}", self.r.below(50)) } else { String::new() }), true)
            }
            8 | 9 | 10 => {
                // subquery predicates over (possibly joined) outer
                let nt = 1 + self.r.below(2);
                let (ts, from, mut conds) = self.comma_join(nt);
                let o = ALIAS[ts[0]];
                let inner_t = *self.r.pick(&[1usize, 2, 3, 4]);
                let (it, ia) = (TABS[inner_t], "q");
                let inner_where = if self.r.chance(50) { format!(" and {}", self.tab_pred_alias(ia)) } else { String::new() };
                let sub = match self.r.below(12) {
                    0 => format!("exists (select 1 from {it} {ia} where {ia}.k1 = {o}.k1{inner_where})"),
                    1 => format!("not exists (select 1 from {it} {ia} where {ia}.k1 = {o}.k1{inner_where})"),
                    2 => format!("{o}.k1 in (select {ia}.k1 from {it} {ia} where {}.v is not null{inner_where})", ia),
                    3 => format!("{o}.k1 not in (select {ia}.k1 from {it} {ia} where 1 = 1{inner_where})"),
                    4 => format!("{o}.k1 not in (select {ia}.k1 from {it} {ia} where {ia}.k1 is not null{inner_where})"),
                    5 => format!("{o}.v > (select avg({ia}.v) from {it} {ia} where {ia}.k1 = {o}.k1{inner_where})"),
                    6 => format!("{o}.f <= (select max({ia}.f) from {it} {ia} where {ia}.k1 = {o}.k1 and {ia}.k2 = {o}.k2{inner_where})"),
                    7 => format!("(select count(*) from {it} {ia} where {ia}.k1 = {o}.k1{inner_where}) {} {}", self.r.pick(&[">", "=", "<", ">="]), self.r.below(4)),
                    8 => format!("{o}.v < (select sum({ia}.v) from {it} {ia} where {ia}.k2 = {o}.k2{inner_where})"),
                    9 => format!("{o}.k2 in (select {ia}.k2 from {it} {ia} where {ia}.g > {})", self.r.below(7)),
                    10 => format!("not exists (select 1 from {it} {ia} where {ia}.k1 = {o}.k1 and {ia}.s = {o}.s{inner_where})"),
                    _ => format!("{o}.g = (select min({ia}.g) from {it} {ia})"),
                };
                // sometimes combine with a second subquery or an OR
                if self.r.chance(15) { let p = self.tab_pred(ts[0]); conds.push(format!("({sub} or {p})")); } else { conds.push(sub); }
                let agg = self.r.chance(70);
                if agg {
                    (format!("select count(*), sum({o}.v), min({o}.k1), max({o}.h) from {from}{}", Self::where_clause(&conds)), false)
                } else {
                    (format!("select {o}.id from {from}{} order by 1 limit 200", Self::where_clause(&conds).replace(" where ", " where ")), true)
                }
            }
            11 | 12 => {
                // DISTINCT over join
                let nt = 1 + self.r.below(3);
                let (ts, from, conds) = self.comma_join(nt);
                let nc = 1 + self.r.below(3);
                let mut cols: Vec<String> = Vec::new();
                for _ in 0..nc { let t = *self.r.pick(&ts); let c = self.col(t, "grp"); if !cols.contains(&c) { cols.push(c); } }
                if self.r.chance(50) { (format!("select count(*) from (select distinct {} from {}{}) q", cols.join(", "), from, Self::where_clause(&conds)), false) }
                else { (format!("select distinct {} from {}{}", cols.join(", "), from, Self::where_clause(&conds)), false) }
            }
            13 | 14 | 15 => {
                // outer joins with residual ON conditions
                let kind = *self.r.pick(&["left join", "right join", "full join", "left join", "join"]);
                let res = *self.r.pick(&["", " and b.v > a.v", " and b.g < 4", " and a.g <> b.g", " and (b.v > 0 or a.v > 0)", " and a.s = b.s", " and b.k2 = a.k2"]);
                let key = *self.r.pick(&["a.k1 = b.k1", "a.k1 = b.k1 and a.k2 = b.k2", "a.k1 = b.k2"]);
                let wh = if self.r.chance(40) { format!(" where {}", self.r.pick(&["a.v > 0", "b.v is null", "b.k1 is not null", "a.g in (1, 2, 3)", "a.s like 'a%' or b.s like 'b%'"])) } else { String::new() };
                let (l, rr) = if big { ("tc", "td") } else { ("ta", "tb") };
                let key = if big { key.replace("b.k1 and", "b.k1 and").to_string() } else { key.to_string() };
                if self.r.chance(55) {
                    (format!("select a.g, count(*), count(b.k1), sum(b.v), sum(a.v), max(b.s) from {l} a {kind} {rr} b on {key}{res}{wh} group by a.g"), false)
                } else if big && self.r.chance(60) {
                    (format!("select count(*), count(a.v), count(b.v), sum(a.v), sum(b.f) from ta a {kind} tc b on a.k1 = b.k1{res}{wh}"), false)
                } else if !big {
                    (format!("select a.id, b.v, a.v from ta a {kind} tb b on {key}{res}{wh}"), false)
                } else {
                    (format!("select a.g, count(*), count(b.k1), sum(b.v) from tc a {kind} tb b on {key}{res}{wh} group by a.g"), false)
                }
            }
            16 | 17 => {
                let op = *self.r.pick(&["union", "union all", "intersect", "except"]);
                let (t1, t2) = (*self.r.pick(&[0usize, 1, 2, 3, 4]), *self.r.pick(&[0usize, 1, 2, 3, 4]));
                let (c1, c2) = (*self.r.pick(&["k1", "g", "k2", "v"]), *self.r.pick(&["k1", "g", "k2", "v"]));
                let w1 = if self.r.chance(50) { format!(" where {}", self.tab_pred_alias("x")) } else { String::new() };
                let w2 = if self.r.chance(50) { format!(" where {}", self.tab_pred_alias("y")) } else { String::new() };
                let outer = if self.r.chance(50) { format!("select count(*), sum(u) from (select x.{c1} as u from {} x{w1} {op} select y.{c2} from {} y{w2}) z", TABS[t1], TABS[t2]) } else { format!("select x.{c1} from {} x{w1} {op} select y.{c2} from {} y{w2}", TABS[t1], TABS[t2]) };
                (outer, false)
            }
            18 | 19 => {
                // window over a join
                let nt = 1 + self.r.below(2);
                let (ts, from, conds) = self.comma_join(nt);
                let t = ts[0];
                let part = *self.r.pick(&["g", "k2", "s"]);
                let f = *self.r.pick(&["sum(@.v)", "count(*)", "max(@.f)", "min(@.v)", "avg(@.v)", "count(@.v)"]);
                let f = f.replace('@', ALIAS[t]);
                (format!("select count(*), sum(w), max(w) from (select 1 as one, {f} over (partition by {a}.{part}) as w from {from}{}) z", Self::where_clause(&conds), a = ALIAS[t]), false)
            }
            20 => {
                let nt = 2 + self.r.below(3);
                let (ts, from, conds) = self.comma_join(nt);
                let t = ts[0];
                let a = ALIAS[t];
                (format!("select count(*) from (select {a}.v, row_number() over (partition by {a}.g order by {a}.v, {a}.k1) as rn from {from}{}) z where rn <= 2", Self::where_clause(&conds)), false)
            }
            21 | 22 => {
                // grouped subquery joined with base (derived table + comma join)
                let (g, ag) = (*self.r.pick(&["k1", "k2", "g"]), *self.r.pick(&["sum(v)", "count(*)", "max(f)", "min(s)"]));
                let other = *self.r.pick(&["tb", "tc", "td"]);
                let wh = if self.r.chance(50) { format!(" where {}", self.tab_pred_alias("a")) } else { String::new() };
                (format!("select count(*), count(q.m), sum(o.v) from (select a.{g} as k, {ag_} as m from ta a{wh} group by a.{g}) q, {other} o where q.k = o.k1", ag_ = ag.replace('(', "(a.").replace("(a.*)", "(*)")), false)
            }
            23 | 24 => {
                // SUM/COUNT of an unjoined big table with filters & many groups (fused agg)
                let k = *self.r.pick(&["a.g", "a.k1", "a.h", "a.s", "a.k2"]);
                let k2 = if self.r.chance(40) { format!(", {}", self.r.pick(&["a.g", "a.s", "a.k2"])) } else { String::new() };
                let k = if k2.contains(k) { "a.h" } else { k };
                let na = 1 + self.r.below(3);
                let aggs: Vec<String> = (0..na).map(|_| self.agg(&[0])).collect();
                let wh = Self::where_clause(&(0..self.r.below(3)).map(|_| self.tab_pred(0)).collect::<Vec<_>>());
                (format!("select {k}{k2}, {} from ta a{wh} group by {k}{k2}", aggs.join(", ")), false)
            }
            25 => {
                // correlated scalar subquery in the select list over the big table
                let it = *self.r.pick(&["tb", "tc"]);
                let f = *self.r.pick(&["count(*)", "sum(q.v)", "max(q.f)", "min(q.k2)"]);
                (format!("select count(*), sum(w), max(w) from (select a.id as id, (select {f} from {it} q where q.k1 = a.k1) as w from ta a where {}) z", self.tab_pred(0)), false)
            }
            26 | 27 => {
                // IN list / hash set and LIKE / SUBSTR kernels on joins
                let nt = 1 + self.r.below(3);
                let (ts, from, mut conds) = self.comma_join(nt);
                let t = *self.r.pick(&ts);
                let a = ALIAS[t];
                let extra = match self.r.below(5) {
                    0 => format!("{a}.k1 in ({})", (0..20 + self.r.below(30)).map(|_| self.r.below(self.ks.max(2)).to_string()).collect::<Vec<_>>().join(", ")),
                    1 => format!("{a}.k1 not in ({})", (0..20 + self.r.below(30)).map(|_| self.r.below(self.ks.max(2)).to_string()).collect::<Vec<_>>().join(", ")),
                    2 => format!("substr({a}.s, 2, 2) = '{}'", self.r.pick(&["b", "bc", "ab", "el", "z", ""])),
                    3 => format!("{a}.s like '%{}%'", self.r.pick(&["a", "b", "ab", "l", "zz"])),
                    _ => format!("({a}.s like 'a%' or {a}.k1 in ({}, {}))", self.r.below(self.ks.max(2)), self.r.below(self.ks.max(2))),
                };
                conds.push(extra);
                (format!("select count(*), sum({a}.v), count(distinct {a}.s) from {from}{}", Self::where_clause(&conds)), false)
            }
            28 => {
                // LIMIT without ORDER BY: which rows is arbitrary, how many is not
                let nt = 2 + self.r.below(2);
                let (ts, _from, conds) = self.comma_join(nt);
                let kind = *self.r.pick(&["join", "left join", "right join", "full join"]);
                let t0 = ts[0];
                let (l, rr) = (self.r.pick(&["tc", "td"]).to_string(), self.r.pick(&["tb", "tc", "td"]).to_string());
                let _ = (t0, conds);
                let lim = 1 + self.r.below(if self.n >= 200 { 3000 } else { 40 });
                (format!("select count(*) from (select x.v from {l} x {kind} {rr} y on x.k1 = y.k1 limit {lim}) z"), false)
            }
            _ => {
                // expression aggregates incl. HAVING over join with NULL-producing keys
                let nt = 2 + self.r.below(if big { 2 } else { 3 });
                let (ts, from, conds) = self.comma_join(nt);
                let t = *self.r.pick(&ts);
                let k = self.col(t, "grp");
                let a1 = self.agg(&ts);
                let a2 = self.agg(&ts);
                (format!("select {k}, {a1}, {a2} from {from}{} group by {k} having {a1} is not null", Self::where_clause(&conds)), false)
            }
        }
    }

    fn tab_pred_alias(&mut self, alias: &str) -> String {
        // predicates over the (single) alias `alias`
        let cmp = *self.r.pick(&["=", "<>", "<", ">="]);
        match self.r.below(5) {
            0 => format!("{alias}.v {cmp} {}", self.r.below(40) as i64 - 20),
            1 => format!("{alias}.g {cmp} {}", self.r.below(7)),
            2 => format!("{alias}.s like '{}'", self.r.pick(&["a%", "%b", "%"])),
            3 => format!("{alias}.f {cmp} {}", self.r.below(100) as f64 / 4.0 - 10.0),
            _ => format!("{alias}.k2 {cmp} {}", self.r.below(5)),
        }
    }
}

#[test]
#[ignore]
fn fuzz_joins_and_aggregates() {
    let scale = std::env::var("FUZZ_SCALE").unwrap_or_else(|_| "small".into());
    let seeds: u64 = std::env::var("FUZZ_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
    let per: usize = std::env::var("FUZZ_PER").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
    let first: u64 = std::env::var("FUZZ_FIRST").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let dir = std::env::var("FUZZ_DIR").ok();
    let mut mismatches: Vec<String> = Vec::new();
    let (mut total, mut ok) = (0usize, 0usize);
    for seed in first..first + seeds {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xABCDEF);
        let sz = sizes(&scale, &mut r);
        let np = *r.pick(&[0usize, 5, 20]);
        let dict = seed % 2 == 0;
        let ta = make_table(&mut r, sz.a, sz.ka, false, true, np, dict);
        let tb = make_table(&mut r, sz.b, sz.ka, false, false, np, false);
        let tc = make_table(&mut r, sz.c, sz.ka, true, false, if np == 0 { 0 } else { 8 }, false);
        let td = make_table(&mut r, sz.d, sz.ka, true, false, 0, dict);
        let te = make_table(&mut r, sz.e, sz.ka, false, false, 10, false);
        let mut ctx = KqlContext::new();
        let blocks = [("ta", ta), ("tb", tb), ("tc", tc), ("td", td), ("te", te)];
        for (n, b) in &blocks { ctx.register(*n, b.clone()); }
        let mut out_q = String::new();
        let mut out_a = String::new();
        let sdir = dir.as_ref().map(|d| format!("{d}/{scale}_{seed}"));
        if let Some(sd) = &sdir {
            std::fs::create_dir_all(sd).unwrap();
            for (n, b) in &blocks {
                let mut s = String::new();
                s.push_str(&b.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>().join(","));
                s.push('\n');
                for i in 0..b.num_rows { s.push_str(&b.columns.iter().map(|c| csv_cell(c, i)).collect::<Vec<_>>().join(",")); s.push('\n'); }
                std::fs::write(format!("{sd}/{n}.csv"), s).unwrap();
            }
        }
        let mut qr = Rng(seed * 31 + 7);
        for qi in 0..per {
            let (sql, ordered) = { let mut g = G { r: &mut qr, n: sz.a, ks: sz.ka }; g.query() };
            total += 1;
            testing::set_force_general(false); testing::set_no_vecexpr(false); testing::set_no_fused(false);
            if std::env::var("FUZZ_TRACE").is_ok() { eprintln!("TRACE seed {seed} q{qi}: {sql}"); }
            let t0 = std::time::Instant::now();
            let fast = answer(&ctx, &sql);
            if t0.elapsed().as_secs_f64() > 5.0 { println!("SLOW {:.1}s seed {seed} q{qi}: {sql}", t0.elapsed().as_secs_f64()); }
            testing::set_no_fused(true);
            let nofused = answer(&ctx, &sql);
            testing::set_no_fused(false);
            testing::set_no_vecexpr(true);
            let rowonly = answer(&ctx, &sql);
            testing::set_no_vecexpr(false);
            // the row-based general tail is quadratic on joins: only run it on small data
            let general = if sz.a < 200 { testing::set_force_general(true); let g = answer(&ctx, &sql); testing::set_force_general(false); Some(g) } else { None };
            let agree = |a: &Result<Vec<String>, String>, b: &Result<Vec<String>, String>| match (a, b) {
                (Ok(x), Ok(y)) => canon(x, ordered) == canon(y, ordered),
                (Err(_), Err(_)) => true,
                _ => false,
            };
            if fast.is_ok() { ok += 1; }
            let bad = !agree(&fast, &nofused) || !agree(&fast, &rowonly) || general.as_ref().map_or(false, |g| !agree(&fast, g));
            if bad {
                mismatches.push(format!("DIFF seed {seed} {scale}: {sql}\n   fast    {}\n   nofused {}\n   rowonly {}\n   general {}", show(&fast), show(&nofused), show(&rowonly), general.as_ref().map(show).unwrap_or_default()));
            }
            if sdir.is_some() {
                let _ = writeln!(out_q, "{}\t{}", if ordered { "O" } else { "U" }, sql);
                match &fast {
                    Ok(rows) => { let _ = writeln!(out_a, "Q {qi} {}", rows.len()); for rw in rows.iter().take(200_000) { let _ = writeln!(out_a, "{rw}"); } }
                    Err(e) => { let _ = writeln!(out_a, "E {qi} {}", e.replace('\n', " ")); }
                }
            }
        }
        if let Some(sd) = &sdir {
            std::fs::write(format!("{sd}/queries.txt"), out_q).unwrap();
            std::fs::write(format!("{sd}/kore.txt"), out_a).unwrap();
        }
    }
    println!("fuzzed {total} queries ({scale}), {ok} answered");
    for m in &mismatches { println!("{m}"); }
    assert!(mismatches.is_empty(), "{} of {} queries disagree across paths", mismatches.len(), total);
}

fn show(a: &Result<Vec<String>, String>) -> String {
    match a { Ok(v) => format!("{} rows [{}]", v.len(), v.iter().take(6).cloned().collect::<Vec<_>>().join(" | ")), Err(e) => e.clone() }
}

/// Debug helper: load a dumped dataset (FUZZ_DATA=dir) and run FUZZ_SQL, printing the rows. `FUZZ_DICT=1` dictionary-encodes `s`.
#[test]
#[ignore]
fn repro() {
    let (Ok(dir), Ok(sql)) = (std::env::var("FUZZ_DATA"), std::env::var("FUZZ_SQL")) else { return };
    let mut ctx = KqlContext::new();
    for t in TABS {
        let text = std::fs::read_to_string(format!("{dir}/{t}.csv")).unwrap();
        let mut lines = text.lines();
        let hdr: Vec<&str> = lines.next().unwrap().split(',').collect();
        let rows: Vec<Vec<&str>> = lines.map(|l| l.split(',').collect()).collect();
        let cols: Vec<Column> = hdr.iter().enumerate().map(|(i, h)| match *h {
            "f" => Column::float64(h, rows.iter().map(|r| if r[i] == NULLT { None } else { Some(r[i].parse().unwrap()) }).collect()),
            "s" => {
                let v: Vec<Option<String>> = rows.iter().map(|r| match r[i] { NULLT => None, EMPTYT => Some(String::new()), x => Some(x.to_string()) }).collect();
                if std::env::var("FUZZ_DICT").is_ok() {
                    let mut d: Vec<String> = Vec::new();
                    let codes: Vec<u8> = v.iter().map(|x| match x { None => u8::MAX, Some(t) => match d.iter().position(|y| y == t) { Some(i) => i as u8, None => { d.push(t.clone()); (d.len() - 1) as u8 } } }).collect();
                    Column::str_dict("s", codes, d)
                } else { Column::str_col("s", v) }
            }
            _ => Column::int64(h, rows.iter().map(|r| if r[i] == NULLT { None } else { Some(r[i].parse().unwrap()) }).collect()),
        }).collect();
        ctx.register(t, DataBlock::new(cols).unwrap());
    }
    if std::env::var("FUZZ_NOFUSED").is_ok() { testing::set_no_fused(true); }
    if std::env::var("FUZZ_GENERAL").is_ok() { testing::set_force_general(true); }
    if std::env::var("FUZZ_NOVEC").is_ok() { testing::set_no_vecexpr(true); }
    let mut rows = answer(&ctx, &sql).unwrap_or_else(|e| vec![e]);
    rows.sort();
    println!("{} rows", rows.len());
    for r in rows.iter().take(40) { println!("{r}"); }
}
const NULLT: &str = r"\N";
const EMPTYT: &str = r"";
