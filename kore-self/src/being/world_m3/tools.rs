//! The being's tools in World M3. Each plan reads only the bytes it is handed and returns an answer, or no
//! answer when it fails visibly. Nothing here knows how a file was made (a test scans this file).
//!
//! World T's loaders, number reader and guarded SQL are reused unchanged (`world_t::tools`). Two additions
//! (owner-approved): the sniffing loader here also recognises `MM/DD/YYYY hh:mm:ss AM` timestamps and
//! rewrites them to ISO (the naive loader does not), and `native-filter` (one predicate, or a count or a sum
//! under one predicate, over the loaded table, no SQL).

use super::world::{Plan, Tmpl, ViewT};
use crate::being::discover::{self, Rule};
use crate::being::world_t::data::{delim_name, money_canon};
use crate::being::world_t::tools::{self as wt, clean_num, clean_text, guarded_sql, is_null, sniff_delim, split_quoted, Loaded};
pub use crate::being::world_t::tools::{SqlLimits, SqlRecord};
use kore_core::{DataBlock, Value};

pub use crate::being::world_t::tools::ToolOut;

fn fail(energy: i64, why: impl Into<String>) -> ToolOut {
    ToolOut { answer: None, energy, detail: why.into(), form: None, tried: 0, rule_id: None, sql: vec![] }
}
fn ok(energy: i64, a: String, detail: impl Into<String>) -> ToolOut {
    ToolOut { answer: Some(a), energy, detail: detail.into(), form: None, tried: 0, rule_id: None, sql: vec![] }
}

/// `MM/DD/YYYY hh:mm:ss AM` -> `YYYY-MM-DD hh:mm:ss` (None if it is not that shape).
pub fn us_to_iso(s: &str) -> Option<String> {
    let b = s.as_bytes();
    if b.len() != 22 || b[2] != b'/' || b[5] != b'/' || b[10] != b' ' || b[13] != b':' || b[16] != b':' || b[19] != b' ' {
        return None;
    }
    let num = |a: usize, z: usize| s.get(a..z).filter(|x| x.bytes().all(|c| c.is_ascii_digit())).and_then(|x| x.parse::<u32>().ok());
    let (m, d, y, h, mi, se) = (num(0, 2)?, num(3, 5)?, num(6, 10)?, num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let pm = match &s[20..] {
        "AM" => false,
        "PM" => true,
        _ => return None,
    };
    if !(1..=12).contains(&h) {
        return None;
    }
    let h24 = (h % 12) + if pm { 12 } else { 0 };
    Some(format!("{y:04}-{m:02}-{d:02} {h24:02}:{mi:02}:{se:02}"))
}

pub fn naive_load(b: &[u8]) -> Loaded {
    wt::naive_load(b)
}

/// World T's sniffing loader, plus the US timestamp rewrite.
pub fn sniff_load(b: &[u8]) -> Loaded {
    let mut l = wt::sniff_load(b);
    for row in l.rows.iter_mut() {
        for f in row.iter_mut() {
            if let Some(iso) = f.as_deref().and_then(us_to_iso) {
                *f = Some(iso);
            }
        }
    }
    l
}

fn num(l: &Loaded, s: &str) -> Option<f64> {
    if l.sniffed { clean_num(s, l.delim) } else { s.parse::<f64>().ok() }
}
fn cents(x: f64) -> i64 {
    (x * 100.0).round() as i64
}
fn col(l: &Loaded, name: &str) -> Result<usize, String> {
    l.header.iter().position(|h| h == name).ok_or_else(|| format!("no column named {name} in the header"))
}
fn round_half_up(num: i64, den: i64) -> i64 {
    let (n, d) = (num as i128, den as i128);
    ((2 * n + d).div_euclid(2 * d)) as i64
}
fn is_int(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty() && t.trim_start_matches('-').chars().all(|c| c.is_ascii_digit()) && t != "-"
}

/// Cents of a cell (Ok(None) for NULL, Err for a value that is not a number).
fn money_at(l: &Loaded, row: &[Option<String>], j: usize) -> Result<Option<i64>, String> {
    match row.get(j).cloned().flatten() {
        None => Ok(None),
        Some(s) => num(l, &s).map(|x| Some(cents(x))).ok_or_else(|| format!("not a number: {s:?}")),
    }
}

fn values(l: &Loaded, j: usize) -> Result<Vec<i64>, String> {
    let mut xs = vec![];
    for row in &l.rows {
        if let Some(c) = money_at(l, row, j)? {
            xs.push(c);
        }
    }
    if xs.is_empty() {
        return Err("no values".into());
    }
    Ok(xs)
}

// ---- the native executor: single-column statistics, quantiles by sorting, a key lookup ----

pub fn native(t: Tmpl, v: &ViewT, l: &Loaded) -> Result<String, String> {
    match t {
        Tmpl::ColCount => Ok(l.rows.len().to_string()),
        Tmpl::ColSum | Tmpl::ColMax | Tmpl::ColMean | Tmpl::QMedian | Tmpl::QP90 => {
            let mut xs = values(l, col(l, &v.cols[0])?)?;
            Ok(money_canon(match t {
                Tmpl::ColSum => xs.iter().sum(),
                Tmpl::ColMax => *xs.iter().max().unwrap(),
                Tmpl::ColMean => round_half_up(xs.iter().sum(), xs.len() as i64),
                Tmpl::QMedian => {
                    xs.sort();
                    let n = xs.len();
                    if n % 2 == 1 { xs[n / 2] } else { round_half_up(xs[n / 2 - 1] + xs[n / 2], 2) }
                }
                _ => {
                    xs.sort();
                    let k = (xs.len() * 9 + 9) / 10;
                    xs[k.max(1) - 1]
                }
            }))
        }
        Tmpl::ColDistinct => {
            let j = col(l, &v.cols[1])?;
            let s: std::collections::BTreeSet<String> = l.rows.iter().filter_map(|r| r.get(j).cloned().flatten()).collect();
            Ok(s.len().to_string())
        }
        Tmpl::Lookup => {
            let (k, j) = (col(l, &v.cols[2])?, col(l, &v.cols[0])?);
            let row = l.rows.iter().find(|r| r.get(k).cloned().flatten().as_deref() == Some(v.param.as_str())).ok_or("key not found")?;
            let s = row.get(j).cloned().flatten().ok_or("the value is null")?;
            if is_int(&s) {
                Ok(s.trim().parse::<i64>().map_err(|_| "bad integer")?.to_string())
            } else {
                num(l, &s).map(|x| money_canon(cents(x))).ok_or_else(|| format!("not a number: {s:?}"))
            }
        }
        _ => Err("the native executor has no filters or groups".into()),
    }
}

// ---- native-filter: one predicate (or a count / sum under one predicate), sniffing loader ----

const COMPONENT_NAMES: [&str; 8] = ["fare_amount", "extra", "mta_tax", "tip_amount", "tolls_amount", "improvement_surcharge", "congestion_surcharge", "Airport_fee"];

pub fn native_filter(v: &ViewT, l: &Loaded) -> Result<String, String> {
    let cell = |row: &Vec<Option<String>>, j: usize| row.get(j).cloned().flatten();
    let count = |p: &dyn Fn(&Vec<Option<String>>) -> Result<bool, String>| -> Result<String, String> {
        let mut n = 0usize;
        for r in &l.rows {
            n += p(r)? as usize;
        }
        Ok(n.to_string())
    };
    match v.tmpl {
        Tmpl::FltCount => {
            let j = col(l, &v.cols[0])?;
            if v.cols[3] == ">" {
                let k = crate::being::world_t::world::parse_cents(&v.param).ok_or("bad threshold")?;
                count(&|r| Ok(money_at(l, r, j)?.map_or(false, |x| x > k)))
            } else {
                let k: i64 = v.param.parse().map_err(|_| "bad value")?;
                count(&|r| Ok(cell(r, j).map_or(false, |s| s.trim().parse::<i64>().ok() == Some(k))))
            }
        }
        Tmpl::FltSum => {
            let (m, c) = (col(l, &v.cols[0])?, col(l, &v.cols[1])?);
            let mut s = 0i64;
            for r in &l.rows {
                if cell(r, c).as_deref().map(str::trim) == Some(v.param.as_str()) {
                    s += money_at(l, r, m)?.unwrap_or(0);
                }
            }
            Ok(money_canon(s))
        }
        Tmpl::AnFareNeg | Tmpl::AnDistZero => {
            let j = col(l, &v.cols[0])?;
            let neg = v.tmpl == Tmpl::AnFareNeg;
            count(&|r| Ok(money_at(l, r, j)?.map_or(false, |x| if neg { x < 0 } else { x == 0 })))
        }
        Tmpl::AnTotalMismatch => {
            let tj = col(l, "total_amount")?;
            let cj: Vec<usize> = COMPONENT_NAMES.iter().map(|n| col(l, n)).collect::<Result<_, _>>()?;
            count(&|r| {
                let Some(t) = money_at(l, r, tj)? else { return Ok(false) };
                let mut s = 0;
                for j in &cj {
                    match money_at(l, r, *j)? {
                        Some(x) => s += x,
                        None => return Ok(false),
                    }
                }
                Ok(s != t)
            })
        }
        Tmpl::AnOutDay | Tmpl::AnOutMonth => {
            let j = col(l, &v.cols[0])?;
            let (key, len) = if v.tmpl == Tmpl::AnOutDay { (v.param.clone(), 10) } else { ("2024-01".to_string(), 7) };
            count(&|r| Ok(cell(r, j).map_or(false, |s| s.get(..len) != Some(key.as_str()))))
        }
        Tmpl::AnFirstBack => {
            let (p, d, k) = (col(l, &v.cols[0])?, col(l, &v.cols[1])?, col(l, &v.cols[2])?);
            let mut best: Option<i64> = None;
            for r in &l.rows {
                if let (Some(a), Some(b)) = (cell(r, p), cell(r, d)) {
                    if b < a {
                        let id: i64 = cell(r, k).and_then(|s| s.trim().parse().ok()).ok_or("bad trip_id")?;
                        best = Some(best.map_or(id, |x: i64| x.min(id)));
                    }
                }
            }
            best.map(|x| x.to_string()).ok_or_else(|| "no row matches".into())
        }
        _ => Err("native-filter has no plan for this template".into()),
    }
}

// ---- SQL (KORE SQL through World T's guarded_sql: read-only, fresh context, timeout and row limit) ----

fn lit(s: &str) -> Result<String, String> {
    wt::sql_lit(s)
}

fn next_day(d: &str) -> Result<String, String> {
    let day: i64 = d.get(8..10).and_then(|x| x.parse().ok()).ok_or("bad day")?;
    Ok(if day >= 31 { "2024-02-01".into() } else { format!("2024-01-{:02}", day + 1) })
}

/// The queries of a template (two for the p90: a count, then the value at that rank).
pub fn sql_texts(v: &ViewT) -> Result<Vec<String>, String> {
    let c = &v.cols;
    let q = |s: String| Ok(vec![s]);
    match v.tmpl {
        Tmpl::ColSum => q(format!("SELECT SUM({}) AS v FROM t", c[0])),
        Tmpl::ColMax => q(format!("SELECT MAX({}) AS v FROM t", c[0])),
        Tmpl::ColMean => q(format!("SELECT SUM({0}) AS s, COUNT({0}) AS n FROM t", c[0])),
        Tmpl::ColCount => q("SELECT COUNT(*) AS v FROM t".into()),
        Tmpl::ColDistinct => q(format!("SELECT COUNT(DISTINCT {}) AS v FROM t", c[1])),
        Tmpl::FltCount => {
            let k = if c[3] == ">" { crate::being::world_t::world::parse_cents(&v.param).map(money_canon).ok_or("bad threshold")? } else { v.param.parse::<i64>().map_err(|_| "bad value")?.to_string() };
            q(format!("SELECT COUNT(*) AS v FROM t WHERE {} {} {k}", c[0], if c[3] == ">" { ">" } else { "=" }))
        }
        Tmpl::FltSum => q(format!("SELECT SUM({}) AS v FROM t WHERE {} = {}", c[0], c[1], v.param.parse::<i64>().map_err(|_| "bad value")?)),
        Tmpl::GrpTop => q(format!("SELECT {1}, SUM({0}) AS s FROM t WHERE {0} IS NOT NULL AND {1} IS NOT NULL GROUP BY {1} ORDER BY s DESC LIMIT 2", c[0], c[1])),
        Tmpl::GrpTop3 => q(format!("SELECT {0}, COUNT(*) AS n FROM t WHERE {0} IS NOT NULL GROUP BY {0} ORDER BY n DESC, {0} ASC LIMIT 3", c[1])),
        Tmpl::QMedian => q(format!("SELECT MEDIAN({}) AS v FROM t", c[0])),
        Tmpl::QP90 => Ok(vec![format!("SELECT COUNT({}) AS n FROM t", c[0]), format!("SELECT {0} AS v FROM t WHERE {0} IS NOT NULL ORDER BY {0} ASC LIMIT 1 OFFSET {{k}}", c[0])]),
        Tmpl::Lookup => q(format!("SELECT {} AS v FROM t WHERE {} = {}", c[0], c[2], v.param.parse::<i64>().map_err(|_| "bad key")?)),
        Tmpl::AnFareNeg => q(format!("SELECT COUNT(*) AS v FROM t WHERE {} < 0", c[0])),
        Tmpl::AnDistZero => q(format!("SELECT COUNT(*) AS v FROM t WHERE {} = 0", c[0])),
        Tmpl::AnTotalMismatch => {
            let nn: Vec<String> = COMPONENT_NAMES.iter().chain(["total_amount"].iter()).map(|n| format!("{n} IS NOT NULL")).collect();
            q(format!("SELECT COUNT(*) AS v FROM t WHERE {} AND ABS(total_amount - ({})) > 0.005", nn.join(" AND "), COMPONENT_NAMES.join(" + ")))
        }
        Tmpl::AnOutDay => q(format!("SELECT COUNT(*) AS v FROM t WHERE {0} < {1} OR {0} >= {2}", c[0], lit(&format!("{} 00:00:00", v.param))?, lit(&format!("{} 00:00:00", next_day(&v.param)?))?)),
        Tmpl::AnOutMonth => q(format!("SELECT COUNT(*) AS v FROM t WHERE {0} < '2024-01-01 00:00:00' OR {0} >= '2024-02-01 00:00:00'", c[0])),
        Tmpl::AnFirstBack => q(format!("SELECT MIN({}) AS v FROM t WHERE {} < {}", c[2], c[1], c[0])),
        _ => Err("no SQL for this template".into()),
    }
}

fn cell(b: &DataBlock, c: usize, r: usize) -> Value {
    if r >= b.num_rows {
        return Value::Null;
    }
    b.columns.get(c).map_or(Value::Null, |col| col.data.get_value(r))
}
fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

pub fn sql_answer(t: Tmpl, b: &DataBlock) -> Result<String, String> {
    let money = |v: Value| as_f64(&v).map(|x| money_canon(cents(x))).ok_or_else(|| format!("not a number: {v:?}"));
    let count = |v: Value| match v {
        Value::Int(i) => Ok(i.to_string()),
        Value::Float(f) if f.fract() == 0.0 => Ok((f as i64).to_string()),
        v => Err(format!("not a count: {v:?}")),
    };
    match t {
        Tmpl::ColSum | Tmpl::ColMax | Tmpl::FltSum | Tmpl::QMedian | Tmpl::QP90 => money(cell(b, 0, 0)),
        Tmpl::ColMean => {
            let s = as_f64(&cell(b, 0, 0)).ok_or("no sum")?;
            let n = as_f64(&cell(b, 1, 0)).ok_or("no count")? as i64;
            if n == 0 {
                return Err("no values".into());
            }
            Ok(money_canon(round_half_up(cents(s), n)))
        }
        Tmpl::Lookup => match cell(b, 0, 0) {
            Value::Int(i) => Ok(i.to_string()),
            v => money(v),
        },
        Tmpl::GrpTop => {
            if b.num_rows > 1 && as_f64(&cell(b, 1, 0)) == as_f64(&cell(b, 1, 1)) {
                return Err("a tie for the top".into());
            }
            count(cell(b, 0, 0))
        }
        Tmpl::GrpTop3 => {
            if b.num_rows < 3 {
                return Err("fewer than three groups".into());
            }
            (0..3).map(|r| count(cell(b, 0, r))).collect::<Result<Vec<_>, _>>().map(|v| v.join(","))
        }
        _ => count(cell(b, 0, 0)),
    }
}

fn sql_plan(v: &ViewT, l: &Loaded, lim: SqlLimits, e: i64, what: &str) -> ToolOut {
    let qs = match sql_texts(v) {
        Ok(q) => q,
        Err(x) => return fail(e, format!("{what}; {x}")),
    };
    let block = match wt::to_block(l) {
        Ok(b) => b,
        Err(x) => return fail(e, format!("{what}; {x}")),
    };
    let mut recs: Vec<SqlRecord> = vec![];
    let run = |sql: &str, recs: &mut Vec<SqlRecord>| -> Result<DataBlock, String> {
        let (res, rec) = guarded_sql(block.clone(), sql, lim);
        let why = rec.why.clone();
        recs.push(rec);
        res.ok_or(why)
    };
    let r = if v.tmpl == Tmpl::QP90 {
        run(&qs[0], &mut recs).and_then(|b| {
            let n = as_f64(&cell(&b, 0, 0)).ok_or("no count")? as usize;
            if n == 0 {
                return Err("no values".into());
            }
            let k = (n * 9 + 9) / 10 - 1;
            run(&qs[1].replace("{k}", &k.to_string()), &mut recs)
        })
    } else {
        run(&qs[0], &mut recs)
    }
    .and_then(|b| sql_answer(v.tmpl, &b));
    let shown: Vec<&String> = recs.iter().map(|q| &q.sql).collect();
    let mut o = match r {
        Ok(a) => ok(e, a, format!("{what}; KORE SQL (read-only, in-memory copy): {shown:?}")),
        Err(x) => fail(e, format!("{what}; KORE SQL {shown:?}: {x}")),
    };
    o.sql = recs;
    o
}

fn table_plan(plan: Plan, v: &ViewT, bytes: &[u8], lim: SqlLimits) -> ToolOut {
    let e = plan.energy(v.rows);
    let l = if matches!(plan, Plan::NaiveNative | Plan::NaiveSql) { naive_load(bytes) } else { sniff_load(bytes) };
    let what = format!("{} loader: {} columns, {} rows{}, delimiter {}", if l.sniffed { "sniffing" } else { "naive" }, l.header.len(), l.rows.len(), if l.dropped > 0 { format!(" ({} lines dropped)", l.dropped) } else { String::new() }, delim_name(l.delim));
    if plan.uses_sql() {
        return sql_plan(v, &l, lim, e, &what);
    }
    let r = if plan == Plan::NativeFilter { native_filter(v, &l) } else { native(v.tmpl, v, &l) };
    match r {
        Ok(a) => ok(e, a, format!("{what}; {}", if plan == Plan::NativeFilter { "native filter" } else { "native statistic" })),
        Err(x) => fail(e, format!("{what}; {x}")),
    }
}

// ---- series ----

pub fn series_values(bytes: &[u8]) -> Vec<i64> {
    clean_text(bytes).lines().skip(1).filter_map(|l| l.split(',').nth(1).and_then(|x| x.trim().parse().ok())).collect()
}

pub fn ts_sig(v: &ViewT) -> u8 {
    100 + v.ctx.idx() as u8
}

pub fn ts_terms(x: &[i64]) -> Vec<i64> {
    x[x.len().saturating_sub(10)..].to_vec()
}

fn mean_i(x: &[i64]) -> f64 {
    x.iter().sum::<i64>() as f64 / x.len().max(1) as f64
}

fn ts_plan(plan: Plan, v: &ViewT, bytes: &[u8], rules: &[Rule]) -> ToolOut {
    let e = plan.energy(v.rows);
    let x = series_values(bytes);
    let n = x.len();
    if n < 3 {
        return fail(e, "fewer than 3 values");
    }
    let season = |k: usize| -> Option<i64> {
        if n < 2 * k {
            return None;
        }
        Some(x[n - k] + (mean_i(&x[n - k..]) - mean_i(&x[n - 2 * k..n - k])).round() as i64)
    };
    let (a, how): (Option<i64>, &str) = match plan {
        Plan::TsLast => (Some(x[n - 1]), "the last value"),
        Plan::TsDrift => (Some(2 * x[n - 1] - x[n - 2]), "last value plus the last change"),
        Plan::TsSeason7 => (season(7), "the value 7 back plus the change of the 7-value mean"),
        Plan::TsSeason24 => (season(24), "the value 24 back plus the change of the 24-value mean"),
        Plan::TsMean => (Some(mean_i(&x[n.saturating_sub(7)..]).round() as i64), "the mean of the last 7 values"),
        Plan::TsRule => {
            let t = ts_terms(&x);
            return match discover::apply(rules, ts_sig(v), &t) {
                Some((val, r)) => {
                    let mut o = ok(e, val.to_string(), format!("own rule {} ({}) applied to the last {} values", r.id, r.form.dsl(), t.len()));
                    o.rule_id = Some(r.id.clone());
                    o
                }
                None => fail(e, "no rule of the being applies to this prefix"),
            };
        }
        _ => (None, ""),
    };
    match a {
        Some(a) => ok(e, a.to_string(), format!("{n} values read; {how}")),
        None => fail(e, format!("{n} values are too few for {}", plan.name())),
    }
}

// ---- malformed lines ----

fn is_date(s: &str) -> bool {
    let p: Vec<&str> = s.split('-').collect();
    if p.len() != 3 || p[0].len() != 4 || p[1].len() != 2 || p[2].len() != 2 {
        return false;
    }
    let (Ok(y), Ok(m), Ok(d)) = (p[0].parse::<i64>(), p[1].parse::<i64>(), p[2].parse::<i64>()) else { return false };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let dim = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    (1..=12).contains(&m) && d >= 1 && d <= dim[(m - 1) as usize]
}

/// An ISO or US timestamp with a real calendar date and time.
fn is_ts(s: &str) -> bool {
    let iso = if s.len() == 19 && s.as_bytes()[4] == b'-' { Some(s.to_string()) } else { us_to_iso(s) };
    let Some(t) = iso else { return false };
    let tm: Vec<Option<u32>> = t[11..].split(':').map(|x| x.parse().ok()).collect();
    t.as_bytes().get(10) == Some(&b' ') && is_date(&t[..10]) && tm.len() == 3 && matches!((tm[0], tm[1], tm[2]), (Some(h), Some(m), Some(x)) if h < 24 && m < 60 && x < 60)
}

fn looks_ts(s: &str) -> bool {
    (s.len() == 19 && s.as_bytes()[4] == b'-' && s.as_bytes()[10] == b' ') || (s.len() == 22 && s.as_bytes()[2] == b'/')
}

#[derive(Clone, Copy, PartialEq)]
enum CTy {
    Int,
    Num,
    Ts,
    Text,
}

fn fix_plan(plan: Plan, v: &ViewT, bytes: &[u8]) -> ToolOut {
    let e = plan.energy(v.rows);
    let text = clean_text(bytes);
    let d = sniff_delim(&text);
    let lines: Vec<Vec<String>> = text.lines().map(|l| split_quoted(l, d)).collect();
    let Some(h) = lines.first().map(|x| x.len()) else { return fail(e, "empty file") };
    for (i, f) in lines.iter().enumerate().skip(1) {
        if f.len() != h {
            return ok(e, (i + 1).to_string(), format!("line {} has {} fields, the header {h}", i + 1, f.len()));
        }
    }
    if plan == Plan::FixFields {
        return fail(e, "every line has the header's field count");
    }
    let mut types = vec![CTy::Text; h];
    for (j, t) in types.iter_mut().enumerate() {
        let vals: Vec<&str> = lines.iter().skip(1).map(|f| f[j].trim()).filter(|s| !is_null(s)).collect();
        let share = |p: &dyn Fn(&str) -> bool| !vals.is_empty() && vals.iter().filter(|s| p(s)).count() * 10 >= vals.len() * 9;
        *t = if share(&is_int) { CTy::Int } else if share(&|s| clean_num(s, d).is_some()) { CTy::Num } else if share(&looks_ts) { CTy::Ts } else { CTy::Text };
    }
    for (i, f) in lines.iter().enumerate().skip(1) {
        for (j, x) in f.iter().enumerate() {
            let x = x.trim();
            if is_null(x) {
                continue;
            }
            let good = match types[j] {
                CTy::Int => is_int(x),
                CTy::Num => clean_num(x, d).is_some(),
                CTy::Ts => is_ts(x),
                CTy::Text => true,
            };
            if !good {
                return ok(e, (i + 1).to_string(), format!("line {}: {x:?} does not fit column {}", i + 1, lines[0].get(j).map_or("?", |s| s.as_str())));
            }
        }
    }
    fail(e, "no line breaks the field count or the column types")
}

// ---- format ----

fn fmt_plan(plan: Plan, v: &ViewT, bytes: &[u8]) -> ToolOut {
    let e = plan.energy(v.rows);
    if plan == Plan::FmtExt {
        let ext = v.file.rsplit('.').next().unwrap_or("");
        let d = match ext {
            "tsv" => b'\t',
            "txt" => b'|',
            _ => b',',
        };
        return ok(e, format!("{}/header", delim_name(d)), format!("the extension .{ext} suggests {}; a header is assumed", delim_name(d)));
    }
    let text = clean_text(bytes);
    let d = sniff_delim(&text);
    let mut ls = text.lines();
    let (l1, l2) = (split_quoted(ls.next().unwrap_or(""), d), split_quoted(ls.next().unwrap_or(""), d));
    let numeric = |f: &Vec<String>| f.iter().filter(|x| clean_num(x, d).is_some() || looks_ts(x.trim())).count();
    let header = numeric(&l1) == 0 && numeric(&l2) > 0;
    ok(e, format!("{}/{}", delim_name(d), if header { "header" } else { "noheader" }), format!("sniffed {}; first line has {} numeric fields, second {}", delim_name(d), numeric(&l1), numeric(&l2)))
}

pub struct ToolCtx<'a> {
    pub rules: &'a [Rule],
    pub sql: SqlLimits,
}

pub fn run_plan(plan: Plan, v: &ViewT, bytes: &[u8], cx: &ToolCtx) -> ToolOut {
    match plan {
        Plan::NaiveNative | Plan::SniffNative | Plan::NaiveSql | Plan::SniffSql | Plan::NativeFilter => table_plan(plan, v, bytes, cx.sql),
        Plan::TsLast | Plan::TsDrift | Plan::TsSeason7 | Plan::TsSeason24 | Plan::TsMean | Plan::TsRule => ts_plan(plan, v, bytes, cx.rules),
        Plan::FixFields | Plan::FixTypes => fix_plan(plan, v, bytes),
        Plan::FmtExt | Plan::FmtSniff => fmt_plan(plan, v, bytes),
    }
}
