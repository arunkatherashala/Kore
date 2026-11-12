//! ORDER BY oracle, KORE side: writes every case's input table, its SQL (as run by KORE and as run by DuckDB) and
//! KORE's result as TSV, for `kore-sql/tests/oracle_duckdb/compare_order_by.py` to rerun in DuckDB and compare row by
//! row (order matters).
//!
//!   cargo run -p kore-sql --example order_by_oracle_gen --offline -- OUT_DIR
//!
//! Files in OUT_DIR (created, must not exist):
//! - `tables.tsv`: `tid \t name:TYPE,name:TYPE,...` (BIGINT / DOUBLE / VARCHAR / BOOLEAN)
//! - `t_<tid>.tsv`: header line, then one line per row
//! - `queries.tsv`: `qid \t tid \t group \t kore_sql \t duckdb_sql`
//! - `r_<qid>.tsv`: KORE's result rows, no header
//!
//! Cells: `\N` is NULL; text is `=` + the value with `\\`, `\t`, `\n`, `\r` escaped (so "" and NULL differ); a
//! double is Rust's `{:?}` (`NaN`, `inf`, `-0.0`, `1e300`); a bool is `true` / `false`.
//!
//! The DuckDB SQL differs from KORE's only in the ORDER BY: `, id ASC` is appended (`id` is the input row number, so
//! this is "rows equal on every key keep their input order", the stability KORE promises; DuckDB's sort is not
//! stable), and in group `rand_orderby` a key without a NULLS clause is rewritten to model KORE's kept legacy NULL
//! placement (Int64 NULL as i64::MIN, Float64 NULL as f64::MAX, text NULL as ""), which no NULLS clause expresses.
//!
//! Case groups: `rand_orderby` (1,200 queries, seeds 1..60) and `rand_grouptop3` (200 tables, seeds 1..200) are the
//! randomized cases of `tests/order_by_multikey.rs` with the same random streams (the ORDER BY queries select every
//! column instead of `id` only); `ties` (multi-key ties, mixed directions, explicit NULLS FIRST / LAST, non-ASCII and
//! dictionary-encoded text, bools); `floats` (NaN, +-inf, -0.0); `window` (ROW_NUMBER over multi-key / text
//! ORDER BY, NULL-free columns); `m3_text` (the M3 group/count top-k shape with a text key).

use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::KqlContext;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 { self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); self.0 >> 33 }
    fn below(&mut self, n: u64) -> u64 { self.next() % n }
    fn chance(&mut self, pct: u64) -> bool { self.below(100) < pct }
}

#[derive(Clone)]
enum Data { I(Vec<Option<i64>>), F(Vec<Option<f64>>), S(Vec<Option<String>>, bool /* dictionary-encoded */), B(Vec<Option<bool>>) }

struct Table { cols: Vec<(String, Data)> }

impl Table {
    fn block(&self) -> DataBlock {
        DataBlock::new(self.cols.iter().map(|(name, d)| match d {
            Data::I(v) => Column::int64(name, v.clone()),
            Data::F(v) => Column::float64(name, v.clone()),
            Data::B(v) => Column::bool_col(name, v.clone()),
            Data::S(v, false) => Column::str_col(name, v.clone()),
            Data::S(v, true) => {
                let mut dict: Vec<String> = Vec::new();
                let codes = v.iter().map(|x| match x {
                    None => u8::MAX,
                    Some(t) => match dict.iter().position(|y| y == t) {
                        Some(p) => p as u8,
                        None => { dict.push(t.clone()); (dict.len() - 1) as u8 }
                    },
                }).collect();
                Column::str_dict(name, codes, dict)
            }
        }).collect()).unwrap()
    }
}

fn esc(s: &str) -> String {
    let mut o = String::from("=");
    for ch in s.chars() {
        match ch { '\\' => o.push_str("\\\\"), '\t' => o.push_str("\\t"), '\n' => o.push_str("\\n"), '\r' => o.push_str("\\r"), c => o.push(c) }
    }
    o
}

fn cell(d: &ColumnData, r: usize) -> String {
    match d {
        ColumnData::Int64(v) => v[r].map(|x| x.to_string()).unwrap_or("\\N".into()),
        ColumnData::Float64(v) => v[r].map(|x| format!("{x:?}")).unwrap_or("\\N".into()),
        ColumnData::Bool(v) => v[r].map(|x| x.to_string()).unwrap_or("\\N".into()),
        ColumnData::Str(v) => v[r].as_deref().map(esc).unwrap_or("\\N".into()),
        ColumnData::StrDict { codes, dict } => if codes[r] == u8::MAX { "\\N".into() } else { esc(&dict[codes[r] as usize]) },
    }
}

struct Out { dir: PathBuf, tables: String, queries: String, tid: usize, qid: usize, ctx: KqlContext }

impl Out {
    fn table(&mut self, t: &Table) {
        self.tid += 1;
        let schema = t.cols.iter().map(|(n, d)| format!("{n}:{}", match d {
            Data::I(_) => "BIGINT", Data::F(_) => "DOUBLE", Data::S(..) => "VARCHAR", Data::B(_) => "BOOLEAN",
        })).collect::<Vec<_>>().join(",");
        writeln!(self.tables, "{}\t{schema}", self.tid).unwrap();
        let b = t.block();
        let mut s = t.cols.iter().map(|c| c.0.clone()).collect::<Vec<_>>().join("\t");
        s.push('\n');
        for r in 0..b.num_rows {
            s.push_str(&b.columns.iter().map(|c| cell(&c.data, r)).collect::<Vec<_>>().join("\t"));
            s.push('\n');
        }
        std::fs::write(self.dir.join(format!("t_{}.tsv", self.tid)), s).unwrap();
        self.ctx = KqlContext::new();
        self.ctx.register("t", b);
    }

    fn query(&mut self, group: &str, kore_sql: &str, duck_sql: &str) {
        self.qid += 1;
        let b = self.ctx.query(kore_sql).unwrap_or_else(|e| panic!("{kore_sql}: {e}"));
        let mut s = String::new();
        for r in 0..b.num_rows {
            s.push_str(&b.columns.iter().map(|c| cell(&c.data, r)).collect::<Vec<_>>().join("\t"));
            s.push('\n');
        }
        std::fs::write(self.dir.join(format!("r_{}.tsv", self.qid)), s).unwrap();
        assert!(!kore_sql.contains('\t') && !duck_sql.contains('\t'));
        writeln!(self.queries, "{}\t{}\t{group}\t{kore_sql}\t{duck_sql}", self.qid, self.tid).unwrap();
    }
}

fn dir_s(desc: bool) -> &'static str { if desc { " DESC" } else { " ASC" } }
fn nulls_s(n: Option<bool>) -> &'static str { match n { Some(true) => " NULLS FIRST", Some(false) => " NULLS LAST", None => "" } }

/// `rand_orderby`: the same random stream as `randomized_multikey_order_by_matches_stable_reference`.
fn rand_orderby(o: &mut Out) {
    const COLS: [&str; 5] = ["i", "j", "f", "s", "d"];
    const STRS: [&str; 6] = ["", "a", "ab", "b", "ba", "zz"];
    for seed in 1..=60u64 {
        let mut r = Rng(seed * 7919);
        let n = 1 + r.below(300) as usize;
        let null_pct = [0, 10, 40][r.below(3) as usize];
        let card = [2, 3, 5, 50][r.below(4) as usize];
        let (mut i, mut j, mut f, mut s, mut d) = (vec![], vec![], vec![], vec![], vec![]);
        for _ in 0..n {
            i.push(if r.chance(null_pct) { None } else { Some(r.below(card) as i64 - 1) });
            j.push(if r.chance(null_pct) { None } else { Some(r.below(card) as i64) });
            f.push(if r.chance(null_pct) { None } else { Some(r.below(card) as f64 * 0.5 - 0.5) });
            s.push(if r.chance(null_pct) { None } else { Some(STRS[r.below(card.min(6)) as usize].to_string()) });
            d.push(if r.chance(null_pct) { None } else { Some(STRS[r.below(card.min(6)) as usize].to_string()) });
        }
        o.table(&Table { cols: vec![
            ("id".into(), Data::I((0..n as i64).map(Some).collect())),
            ("i".into(), Data::I(i)), ("j".into(), Data::I(j)), ("f".into(), Data::F(f)),
            ("s".into(), Data::S(s, false)), ("d".into(), Data::S(d, true)),
        ] });
        for _q in 0..20 {
            let nk = 1 + r.below(3) as usize;
            let mut keys: Vec<(usize, bool, Option<bool>)> = vec![];
            for _ in 0..nk {
                let k = r.below(5) as usize;
                let nulls = match r.below(4) { 0 => Some(true), 1 => Some(false), _ => None };
                keys.push((k, r.chance(50), nulls));
            }
            let limit = if r.chance(50) { Some(1 + r.below(10) as usize) } else { None };
            let lim = limit.map(|l| format!(" LIMIT {l}")).unwrap_or_default();
            let kore = keys.iter().map(|&(k, desc, nl)| format!("{}{}{}", COLS[k], dir_s(desc), nulls_s(nl))).collect::<Vec<_>>().join(", ");
            let duck = keys.iter().map(|&(k, desc, nl)| match nl {
                Some(_) => format!("{}{}{}", COLS[k], dir_s(desc), nulls_s(nl)),
                // KORE's legacy NULL placement without a NULLS clause
                None => format!("{}{} NULLS LAST", match COLS[k] {
                    "i" | "j" => format!("COALESCE({}, CAST(-9223372036854775808 AS HUGEINT))", COLS[k]),
                    "f" => "COALESCE(f, 1.7976931348623157e308)".to_string(),
                    c => format!("COALESCE({c}, '')"),
                }, dir_s(desc)),
            }).collect::<Vec<_>>().join(", ");
            let legacy = if keys.iter().any(|k| k.2.is_none()) { "rand_orderby_legacy" } else { "rand_orderby" };
            o.query(legacy, &format!("SELECT id, i, j, f, s, d FROM t ORDER BY {kore}{lim}"),
                &format!("SELECT id, i, j, f, s, d FROM t ORDER BY {duck}, id ASC{lim}"));
        }
    }
}

/// `rand_grouptop3`: the same random stream as `randomized_group_count_top_k_matches_reference`.
fn rand_grouptop3(o: &mut Out) {
    for seed in 1..=200u64 {
        let mut r = Rng(seed * 104729);
        let n = 1 + r.below(3000) as usize;
        let card = 2 + r.below(60);
        let k: Vec<Option<i64>> = (0..n).map(|_| Some(r.below(card) as i64)).collect();
        o.table(&Table { cols: vec![("k".into(), Data::I(k))] });
        let sql = "SELECT k, COUNT(*) AS n FROM t GROUP BY k ORDER BY n DESC, k ASC LIMIT 3";
        o.query("rand_grouptop3", sql, sql);
    }
}

const TEXT: [&str; 10] = ["", "a", "Z", "e", "é", "ä", "ab", "日本", "zz", "Ω"];

fn ties(o: &mut Out) {
    let mut r = Rng(9001);
    let n = 500usize;
    let pick = |r: &mut Rng, pct: u64| !r.chance(pct);
    let (mut a, mut f, mut s, mut d, mut b) = (vec![], vec![], vec![], vec![], vec![]);
    for _ in 0..n {
        a.push(if pick(&mut r, 15) { Some(r.below(4) as i64 - 2) } else { None });
        f.push(if pick(&mut r, 15) { Some([-1.5, 0.0, 0.25, 2.0][r.below(4) as usize]) } else { None });
        s.push(if pick(&mut r, 15) { Some(TEXT[r.below(10) as usize].to_string()) } else { None });
        d.push(if pick(&mut r, 15) { Some(TEXT[r.below(10) as usize].to_string()) } else { None });
        b.push(if pick(&mut r, 15) { Some(r.chance(50)) } else { None });
    }
    o.table(&Table { cols: vec![
        ("id".into(), Data::I((0..n as i64).map(Some).collect())),
        ("a".into(), Data::I(a)), ("f".into(), Data::F(f)), ("s".into(), Data::S(s, false)),
        ("d".into(), Data::S(d, true)), ("b".into(), Data::B(b)),
    ] });
    const K: [&str; 5] = ["a", "f", "s", "d", "b"];
    let sel = "SELECT id, a, f, s, d, b FROM t ORDER BY";
    let run = |o: &mut Out, keys: &[(usize, bool, bool)], lim: &str| {
        let ob = keys.iter().map(|&(k, desc, nf)| format!("{}{}{}", K[k], dir_s(desc), nulls_s(Some(nf)))).collect::<Vec<_>>().join(", ");
        o.query("ties", &format!("{sel} {ob}{lim}"), &format!("{sel} {ob}, id ASC{lim}"));
    };
    for k1 in 0..5 { for d1 in [false, true] { for n1 in [false, true] { for lim in ["", " LIMIT 7"] {
        run(o, &[(k1, d1, n1)], lim);
    } } } }
    for k1 in 0..5 { for k2 in 0..5 { if k1 == k2 { continue; }
        for d1 in [false, true] { for d2 in [false, true] { for n1 in [false, true] { for n2 in [false, true] { for lim in ["", " LIMIT 7"] {
            run(o, &[(k1, d1, n1), (k2, d2, n2)], lim);
        } } } } }
    } }
    for _ in 0..100 {
        let keys: Vec<(usize, bool, bool)> = (0..3).map(|_| (r.below(5) as usize, r.chance(50), r.chance(50))).collect();
        let lim = if r.chance(50) { format!(" LIMIT {}", 1 + r.below(20)) } else { String::new() };
        run(o, &keys, &lim);
    }
}

fn floats(o: &mut Out) {
    let mut r = Rng(4242);
    let vals = [Some(f64::NAN), Some(f64::INFINITY), Some(f64::NEG_INFINITY), Some(-0.0), Some(0.0), Some(1.5), Some(-2.0), Some(1e300), None];
    let n = 200usize;
    let f: Vec<Option<f64>> = (0..n).map(|_| vals[r.below(9) as usize]).collect();
    let g: Vec<Option<i64>> = (0..n).map(|_| Some(r.below(3) as i64)).collect();
    o.table(&Table { cols: vec![("id".into(), Data::I((0..n as i64).map(Some).collect())), ("f".into(), Data::F(f)), ("g".into(), Data::I(g))] });
    for desc in [false, true] { for nf in [false, true] { for lim in ["", " LIMIT 5"] {
        let fk = format!("f{}{}", dir_s(desc), nulls_s(Some(nf)));
        let gk = format!("g{} NULLS LAST", dir_s(!desc));
        for ob in [fk.clone(), format!("{fk}, {gk}"), format!("{gk}, {fk}")] {
            o.query("floats", &format!("SELECT id, f, g FROM t ORDER BY {ob}{lim}"), &format!("SELECT id, f, g FROM t ORDER BY {ob}, id ASC{lim}"));
        }
    } } }
}

fn window(o: &mut Out) {
    let mut r = Rng(777);
    let n = 300usize;
    let a: Vec<Option<i64>> = (0..n).map(|_| Some(r.below(4) as i64)).collect();
    let f: Vec<Option<f64>> = (0..n).map(|_| Some(r.below(3) as f64 * 0.5)).collect();
    let s: Vec<Option<String>> = (0..n).map(|_| Some(TEXT[r.below(10) as usize].to_string())).collect();
    let d: Vec<Option<String>> = (0..n).map(|_| Some(TEXT[r.below(10) as usize].to_string())).collect();
    o.table(&Table { cols: vec![
        ("id".into(), Data::I((0..n as i64).map(Some).collect())),
        ("a".into(), Data::I(a)), ("f".into(), Data::F(f)), ("s".into(), Data::S(s, false)), ("d".into(), Data::S(d, true)),
    ] });
    const K: [&str; 4] = ["a", "f", "s", "d"];
    let w = |o: &mut Out, part: &str, ob: &str| {
        o.query("window",
            &format!("SELECT id, ROW_NUMBER() OVER ({part}ORDER BY {ob}) AS rn FROM t ORDER BY id ASC NULLS LAST"),
            &format!("SELECT id, ROW_NUMBER() OVER ({part}ORDER BY {ob}, id ASC) AS rn FROM t ORDER BY id ASC NULLS LAST"));
    };
    for k in 0..4 { for desc in [false, true] { w(o, "", &format!("{}{}", K[k], dir_s(desc))); } }
    for k1 in 0..4 { for k2 in 0..4 { if k1 == k2 { continue; } for d1 in [false, true] { for d2 in [false, true] {
        w(o, "", &format!("{}{}, {}{}", K[k1], dir_s(d1), K[k2], dir_s(d2)));
    } } } }
    for k in 1..4 { for desc in [false, true] { w(o, "PARTITION BY a ", &format!("{}{}", K[k], dir_s(desc))); } }
}

fn m3_text(o: &mut Out) {
    for seed in 1..=50u64 {
        let mut r = Rng(seed * 31337);
        let n = 1 + r.below(400) as usize;
        let card = 2 + r.below(9);
        let g: Vec<Option<String>> = (0..n).map(|_| if r.chance(10) { None } else { Some(TEXT[r.below(card) as usize].to_string()) }).collect();
        o.table(&Table { cols: vec![("g".into(), Data::S(g, seed % 2 == 0))] });
        for sql in ["SELECT g, COUNT(*) AS n FROM t WHERE g IS NOT NULL GROUP BY g ORDER BY n DESC, g ASC LIMIT 3",
                    "SELECT g, COUNT(*) AS n FROM t WHERE g IS NOT NULL GROUP BY g ORDER BY n ASC, g DESC LIMIT 4"] {
            o.query("m3_text", sql, sql);
        }
    }
}

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).expect("usage: order_by_oracle_gen OUT_DIR"));
    assert!(!Path::new(&dir).exists(), "{} exists", dir.display());
    std::fs::create_dir_all(&dir).unwrap();
    let mut o = Out { dir: dir.clone(), tables: String::new(), queries: String::new(), tid: 0, qid: 0, ctx: KqlContext::new() };
    rand_orderby(&mut o);
    rand_grouptop3(&mut o);
    ties(&mut o);
    floats(&mut o);
    window(&mut o);
    m3_text(&mut o);
    std::fs::write(dir.join("tables.tsv"), &o.tables).unwrap();
    std::fs::write(dir.join("queries.tsv"), &o.queries).unwrap();
    println!("tables={} queries={}", o.tid, o.qid);
}
