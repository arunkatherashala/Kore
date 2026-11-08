//! The being's tools in World T. Each plan reads only the bytes it is handed (the sandbox's file, or
//! the visible prefix of a series) and returns an answer, or no answer when it fails visibly. Nothing
//! here knows how a file was generated: a test scans this file for the generator's hidden names.
//!
//! * loaders: `naive` (split on commas and newlines, nothing else) and `sniff` (byte-order mark,
//!   CRLF, delimiter detection, quoted fields, padding, null tokens, thousands separators, decimal
//!   commas);
//! * executors: `native` (single-column statistics and a key lookup, no filters or groups) and `sql`
//!   (the KORE SQL engine, `KqlContext`, read-only on an in-memory copy of the loaded table: the
//!   query passes `check_readonly` first, the table is registered in a fresh context, and the query has
//!   a timeout and a row limit set by the controller: `guarded_sql`);
//! * series: last value, drift, seasonal with a level shift (7 or 24), mean of the last 7, and the
//!   being's own discovered rules (the B2 search, `discover::apply`);
//! * derived columns: the product of two columns, a fitted ratio, and a verified search over a small
//!   grammar of column rules (B2-style: fitted per file, held-out check, never wrong);
//! * malformed lines: a field-count check and a type check;
//! * format: the delimiter the extension suggests, or a sniffer;
//! * logs: a substring count, a structured parse with native counting, the parse plus SQL.

use super::world::{parse_cents, Plan, Tmpl, ViewT};
use crate::being::discover::{self, Rule};
use kore_core::{Column, DataBlock, Value};
use kore_sql::KqlContext;

pub const NULL_TOKENS: [&str; 6] = ["", "NA", "NULL", "null", "-", "N/A"];
pub const DELIMS: [u8; 4] = [b',', b';', b'\t', b'|'];

#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub header: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub delim: u8,
    pub sniffed: bool,
    pub dropped: usize,
}

/// What a plan did (logged with the action) and its answer.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOut {
    pub answer: Option<String>,
    pub energy: i64,
    pub detail: String,
    /// Column-rule search: the accepted form's code, and the forms tried.
    pub form: Option<u8>,
    pub tried: u32,
    /// Series rule used.
    pub rule_id: Option<String>,
    /// Every query the plan generated.
    pub sql: Vec<SqlRecord>,
}

impl ToolOut {
    fn fail(energy: i64, why: impl Into<String>) -> ToolOut {
        ToolOut { answer: None, energy, detail: why.into(), form: None, tried: 0, rule_id: None, sql: vec![] }
    }
    fn ok(energy: i64, a: String, detail: impl Into<String>) -> ToolOut {
        ToolOut { answer: Some(a), energy, detail: detail.into(), form: None, tried: 0, rule_id: None, sql: vec![] }
    }
}

// ---- loaders ----

pub fn naive_load(b: &[u8]) -> Loaded {
    let text = String::from_utf8_lossy(b);
    let mut lines = text.split('\n').filter(|l| !l.is_empty());
    let header: Vec<String> = lines.next().unwrap_or("").split(',').map(String::from).collect();
    let rows = lines.map(|l| l.split(',').map(|f| if f.is_empty() { None } else { Some(f.to_string()) }).collect()).collect();
    Loaded { header, rows, delim: b',', sniffed: false, dropped: 0 }
}

/// Split one line on `d`, honouring double quotes ("" is a quote inside a quoted field).
pub fn split_quoted(line: &str, d: u8) -> Vec<String> {
    let d = d as char;
    let mut out = vec![];
    let mut cur = String::new();
    let mut inq = false;
    let cs: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if inq {
            if c == '"' {
                if cs.get(i + 1) == Some(&'"') {
                    cur.push('"');
                    i += 1;
                } else {
                    inq = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' && cur.trim().is_empty() {
            cur.clear();
            inq = true;
        } else if c == d {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
        i += 1;
    }
    out.push(cur);
    out
}

pub fn clean_text(b: &[u8]) -> String {
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b);
    String::from_utf8_lossy(b).replace("\r\n", "\n")
}

/// The delimiter whose field count is the same (and above 1) on the first lines; the largest such.
pub fn sniff_delim(text: &str) -> u8 {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(12).collect();
    let mut best: Option<(bool, usize, u8)> = None;
    for d in DELIMS {
        let counts: Vec<usize> = lines.iter().map(|l| split_quoted(l, d).len()).collect();
        let first = counts.first().copied().unwrap_or(1);
        let same = counts.iter().filter(|c| **c == first).count();
        let consistent = first > 1 && same * 10 >= counts.len() * 9;
        let key = (consistent, if first > 1 { first } else { 0 }, d);
        if best.map_or(true, |b| (key.0, key.1) > (b.0, b.1)) {
            best = Some(key);
        }
    }
    best.map_or(b',', |b| b.2)
}

pub fn is_null(s: &str) -> bool {
    NULL_TOKENS.contains(&s.trim())
}

pub fn sniff_load(b: &[u8]) -> Loaded {
    let text = clean_text(b);
    let delim = sniff_delim(&text);
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<String> = split_quoted(lines.next().unwrap_or(""), delim).into_iter().map(|h| h.trim().to_string()).collect();
    let mut rows = vec![];
    let mut dropped = 0;
    for l in lines {
        let f = split_quoted(l, delim);
        if f.len() != header.len() {
            dropped += 1;
            continue;
        }
        rows.push(f.into_iter().map(|x| { let t = x.trim(); if is_null(t) { None } else { Some(t.to_string()) } }).collect());
    }
    Loaded { header, rows, delim, sniffed: true, dropped }
}

/// A number as the sniffing loader reads it (thousands separators, a decimal comma after a `;` delimiter).
pub fn clean_num(s: &str, delim: u8) -> Option<f64> {
    let t = s.trim();
    if let Ok(x) = t.parse::<f64>() {
        return Some(x);
    }
    let digits_groups = |x: &str| {
        let (ip, fp) = x.split_once('.').map_or((x, None), |(a, b)| (a, Some(b)));
        let g: Vec<&str> = ip.split(',').collect();
        g.len() > 1 && !g[0].is_empty() && g[0].len() <= 3 && g.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())) && g[1..].iter().all(|p| p.len() == 3) && fp.map_or(true, |f| f.chars().all(|c| c.is_ascii_digit()))
    };
    if digits_groups(t) {
        return t.replace(',', "").parse().ok();
    }
    if delim == b';' {
        if let Some((a, b)) = t.split_once(',') {
            if !a.is_empty() && a.trim_start_matches('-').chars().all(|c| c.is_ascii_digit()) && b.chars().all(|c| c.is_ascii_digit()) && !b.is_empty() {
                return format!("{a}.{b}").parse().ok();
            }
        }
    }
    None
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

/// Round num/den half up (den > 0).
fn round_half_up(num: i64, den: i64) -> i64 {
    let (n, d) = (num as i128, den as i128);
    ((2 * n + d).div_euclid(2 * d)) as i64
}

// ---- the native executor ----

pub fn native(t: Tmpl, v: &ViewT, l: &Loaded) -> Result<String, String> {
    let money = |row: &Vec<Option<String>>, j: usize| -> Result<Option<i64>, String> {
        match row.get(j).cloned().flatten() {
            None => Ok(None),
            Some(s) => num(l, &s).map(|x| Some(cents(x))).ok_or_else(|| format!("not a number: {s:?}")),
        }
    };
    match t {
        Tmpl::ColCount => Ok(l.rows.len().to_string()),
        Tmpl::ColSum | Tmpl::ColMax | Tmpl::ColMean => {
            let j = col(l, &v.cols[0])?;
            let mut xs = vec![];
            for row in &l.rows {
                if let Some(c) = money(row, j)? {
                    xs.push(c);
                }
            }
            if xs.is_empty() {
                return Err("no values".into());
            }
            Ok(match t {
                Tmpl::ColSum => super::data::money_canon(xs.iter().sum()),
                Tmpl::ColMax => super::data::money_canon(*xs.iter().max().unwrap()),
                _ => super::data::money_canon(round_half_up(xs.iter().sum(), xs.len() as i64)),
            })
        }
        Tmpl::ColDistinct => {
            let j = col(l, &v.cols[1])?;
            let s: std::collections::BTreeSet<String> = l.rows.iter().filter_map(|r| r.get(j).cloned().flatten()).collect();
            Ok(s.len().to_string())
        }
        Tmpl::Lookup => {
            let (k, j) = (col(l, &v.cols[2])?, col(l, &v.cols[0])?);
            let row = l.rows.iter().find(|r| r.get(k).cloned().flatten().as_deref() == Some(v.param.as_str())).ok_or("key not found")?;
            money(row, j)?.map(super::data::money_canon).ok_or_else(|| "the value is null".into())
        }
        _ => Err("the native executor has no filters or groups".into()),
    }
}

// ---- the SQL executor (KORE SQL, read-only, in-memory copy) ----

pub fn to_block(l: &Loaded) -> Result<DataBlock, String> {
    let mut cols = vec![];
    for (j, h) in l.header.iter().enumerate() {
        let vals: Vec<Option<String>> = l.rows.iter().map(|r| r.get(j).cloned().flatten()).collect();
        let nn: Vec<&String> = vals.iter().flatten().collect();
        if !nn.is_empty() && nn.iter().all(|s| s.parse::<i64>().is_ok()) {
            cols.push(Column::int64(h, vals.iter().map(|x| x.as_ref().and_then(|s| s.parse().ok())).collect()));
        } else if !nn.is_empty() && nn.iter().all(|s| num(l, s).is_some()) {
            cols.push(Column::float64(h, vals.iter().map(|x| x.as_ref().and_then(|s| num(l, s))).collect()));
        } else {
            cols.push(Column::str_col(h, vals));
        }
    }
    DataBlock::new(cols).map_err(|e| format!("cannot build the table: {e}"))
}

pub fn sql_lit(s: &str) -> Result<String, String> {
    if s.contains('\'') || s.contains('\\') {
        return Err("a quote in a literal".into());
    }
    Ok(format!("'{s}'"))
}

pub fn sql_text(t: Tmpl, v: &ViewT) -> Result<String, String> {
    let c = &v.cols;
    Ok(match t {
        Tmpl::ColSum => format!("SELECT SUM({}) AS v FROM t", c[0]),
        Tmpl::ColMax => format!("SELECT MAX({}) AS v FROM t", c[0]),
        // COUNT(col) of the engine counts NULLs (docs/world_t.md, "Engine needs"): count the non-null rows instead
        Tmpl::ColMean => format!("SELECT SUM({0}) AS s, COUNT(*) AS n FROM t WHERE {0} IS NOT NULL", c[0]),
        Tmpl::ColCount => "SELECT COUNT(*) AS v FROM t".into(),
        Tmpl::ColDistinct => format!("SELECT COUNT(DISTINCT {}) AS v FROM t", c[1]),
        Tmpl::GrpSumWhere => format!("SELECT SUM({}) AS v FROM t WHERE {} = {}", c[0], c[1], sql_lit(&v.param)?),
        Tmpl::GrpTop => format!("SELECT {1}, SUM({0}) AS s FROM t WHERE {0} IS NOT NULL GROUP BY {1} ORDER BY s DESC LIMIT 2", c[0], c[1]),
        Tmpl::GrpCountGt => format!("SELECT COUNT(*) AS v FROM t WHERE {} > {}", c[3], v.param.parse::<i64>().map_err(|_| "bad threshold")?),
        Tmpl::Lookup => format!("SELECT {} AS v FROM t WHERE {} = {} LIMIT 1", c[0], c[2], sql_lit(&v.param)?),
        Tmpl::LogCount => format!("SELECT COUNT(*) AS v FROM t WHERE UPPER(level) = 'ERROR' AND service = {}", sql_lit(&v.param)?),
        Tmpl::LogTop => "SELECT service, COUNT(*) AS c FROM t WHERE UPPER(level) = 'ERROR' GROUP BY service ORDER BY c DESC LIMIT 2".into(),
        _ => return Err("no SQL for this template".into()),
    })
}

/// Limits of every query (controller keys `world_t_sql_timeout_ms`, `world_t_sql_max_rows`; see `ctl.rs`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SqlLimits {
    pub timeout_ms: u64,
    pub max_rows: usize,
}

pub const DEFAULT_SQL_TIMEOUT_MS: u64 = 2000;
pub const DEFAULT_SQL_MAX_ROWS: usize = 1000;
pub const HARD_SQL_TIMEOUT_MS: u64 = 60_000;
pub const HARD_SQL_MAX_ROWS: usize = 1_000_000;

impl Default for SqlLimits {
    fn default() -> Self {
        SqlLimits { timeout_ms: DEFAULT_SQL_TIMEOUT_MS, max_rows: DEFAULT_SQL_MAX_ROWS }
    }
}

impl SqlLimits {
    /// What a missing or invalid controller means: no query may run.
    pub fn closed() -> SqlLimits {
        SqlLimits { timeout_ms: 0, max_rows: 0 }
    }
}

/// One query the being generated: its text, what happened, and a digest of its result.
#[derive(Debug, Clone, PartialEq)]
pub struct SqlRecord {
    pub sql: String,
    pub ok: bool,
    /// Why it was refused: `sql_read_only`, `sql_timeout_ms`, `sql_max_rows`; `sql_error` for an engine error.
    pub cap: Option<String>,
    pub rows: usize,
    /// First 12 hex digits of sha256 over the result rows (empty when there is no result).
    pub digest: String,
    pub why: String,
    pub limits: SqlLimits,
}

impl SqlRecord {
    /// The controller refusal of a refused query (None for a query that ran, or an engine error).
    pub fn refusal(&self, step: u64, what: &str) -> Option<crate::being::controller::Refusal> {
        let cap = self.cap.as_deref().filter(|c| *c != "sql_error")?;
        let (lim, req) = match cap {
            "sql_timeout_ms" => (self.limits.timeout_ms, self.limits.timeout_ms.saturating_add(1)),
            "sql_max_rows" => (self.limits.max_rows as u64, self.rows as u64),
            _ => (0, 1),
        };
        Some(crate::being::controller::Refusal::new(cap, lim, req, format!("step {step}, {what}: the query {:?} was {}", self.sql, self.why)))
    }
}

pub fn digest_of(b: &DataBlock) -> String {
    let rows = crate::versioned::sql::block_strings(b);
    crate::versioned::sha256_hex(&[format!("{rows:?}").as_bytes()])[..12].to_string()
}

/// Run one query read-only: the statement must pass `check_readonly` (anything that writes is refused
/// before the engine sees it); it runs on a fresh context holding only an in-memory copy of the loaded
/// table (`t`); its result is discarded and the query refused if it took `timeout_ms` or longer (a
/// timeout of 0 refuses every query) or returned more than `max_rows` rows. The engine call itself is
/// not interrupted (the being has no threads): the timeout is enforced on the result.
pub fn guarded_sql(block: DataBlock, sql: &str, lim: SqlLimits) -> (Option<DataBlock>, SqlRecord) {
    let mut rec = SqlRecord { sql: sql.to_string(), ok: false, cap: None, rows: 0, digest: String::new(), why: String::new(), limits: lim };
    let clean = match crate::versioned::sql::check_readonly(sql) {
        Ok(c) => c,
        Err(e) => {
            rec.cap = Some("sql_read_only".into());
            rec.why = format!("refused: {e}");
            return (None, rec);
        }
    };
    if lim.timeout_ms == 0 {
        rec.cap = Some("sql_timeout_ms".into());
        rec.why = "refused: the query timeout is 0 ms (no query may run)".into();
        return (None, rec);
    }
    let mut ctx = KqlContext::new();
    ctx.register("t", block);
    let t0 = std::time::Instant::now();
    let r = ctx.query(&clean);
    let ms = t0.elapsed().as_millis() as u64;
    match r {
        Err(e) => {
            rec.cap = Some("sql_error".into());
            rec.why = format!("query error: {e}");
            (None, rec)
        }
        Ok(_) if ms >= lim.timeout_ms => {
            rec.cap = Some("sql_timeout_ms".into());
            rec.why = format!("refused: the query ran past the timeout of {} ms; its result is discarded", lim.timeout_ms);
            (None, rec)
        }
        Ok(b) => {
            rec.rows = b.num_rows;
            rec.digest = digest_of(&b);
            if b.num_rows > lim.max_rows {
                rec.cap = Some("sql_max_rows".into());
                rec.why = format!("refused: {} result rows, the limit is {}; the result is discarded", b.num_rows, lim.max_rows);
                return (None, rec);
            }
            rec.ok = true;
            rec.why = format!("{} rows", b.num_rows);
            (Some(b), rec)
        }
    }
}

/// `guarded_sql` with the default limits (tests and probes).
pub fn run_sql(block: DataBlock, sql: &str) -> Result<DataBlock, String> {
    let (b, rec) = guarded_sql(block, sql, SqlLimits::default());
    b.ok_or(rec.why)
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
    let money = |v: Value| as_f64(&v).map(|x| super::data::money_canon(cents(x))).ok_or_else(|| format!("not a number: {v:?}"));
    let count = |v: Value| match v {
        Value::Int(i) => Ok(i.to_string()),
        Value::Float(f) if f.fract() == 0.0 => Ok((f as i64).to_string()),
        v => Err(format!("not a count: {v:?}")),
    };
    match t {
        Tmpl::ColSum | Tmpl::ColMax | Tmpl::GrpSumWhere | Tmpl::Lookup => money(cell(b, 0, 0)),
        Tmpl::ColMean => {
            let s = as_f64(&cell(b, 0, 0)).ok_or("no sum")?;
            let n = as_f64(&cell(b, 1, 0)).ok_or("no count")? as i64;
            if n == 0 {
                return Err("no values".into());
            }
            Ok(super::data::money_canon(round_half_up(cents(s), n)))
        }
        Tmpl::ColCount | Tmpl::ColDistinct | Tmpl::GrpCountGt | Tmpl::LogCount => count(cell(b, 0, 0)),
        Tmpl::GrpTop | Tmpl::LogTop => {
            let (a, x) = (cell(b, 1, 0), cell(b, 1, 1));
            if b.num_rows > 1 && as_f64(&a) == as_f64(&x) {
                return Err("a tie for the top".into());
            }
            match cell(b, 0, 0) {
                Value::Str(s) => Ok(s),
                v => Err(format!("not a name: {v:?}")),
            }
        }
        _ => Err("no SQL answer for this template".into()),
    }
}

fn table_plan(plan: Plan, v: &ViewT, bytes: &[u8], lim: SqlLimits) -> ToolOut {
    let e = plan.energy(v.rows);
    let l = if matches!(plan, Plan::NaiveNative | Plan::NaiveSql) { naive_load(bytes) } else { sniff_load(bytes) };
    let what = format!("{} loader: {} columns, {} rows{}, delimiter {}", if l.sniffed { "sniffing" } else { "naive" }, l.header.len(), l.rows.len(), if l.dropped > 0 { format!(" ({} lines dropped)", l.dropped) } else { String::new() }, super::data::delim_name(l.delim));
    if plan.uses_sql() {
        let sql = match sql_text(v.tmpl, v) {
            Ok(s) => s,
            Err(x) => return ToolOut::fail(e, format!("{what}; {x}")),
        };
        let block = match to_block(&l) {
            Ok(b) => b,
            Err(x) => return ToolOut::fail(e, format!("{what}; {x}")),
        };
        let (res, rec) = guarded_sql(block, &sql, lim);
        let r = res.ok_or_else(|| rec.why.clone()).and_then(|b| sql_answer(v.tmpl, &b));
        let mut o = match r {
            Ok(a) => ToolOut::ok(e, a, format!("{what}; KORE SQL (read-only, in-memory copy): {sql}")),
            Err(x) => ToolOut::fail(e, format!("{what}; KORE SQL: {sql}: {x}")),
        };
        o.sql.push(rec);
        o
    } else {
        match native(v.tmpl, v, &l) {
            Ok(a) => ToolOut::ok(e, a, format!("{what}; native statistic")),
            Err(x) => ToolOut::fail(e, format!("{what}; native statistic: {x}")),
        }
    }
}

// ---- series ----

pub fn series_values(bytes: &[u8]) -> Vec<i64> {
    clean_text(bytes).lines().skip(1).filter_map(|l| l.split(',').nth(1).and_then(|x| x.trim().parse().ok())).collect()
}

pub fn ts_sig(v: &ViewT) -> u8 {
    100 + v.ctx.idx() as u8 + if v.tmpl == Tmpl::TsNear { 20 } else { 0 }
}

/// The terms the rule search sees (the last 10 of the visible prefix).
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
        return ToolOut::fail(e, "fewer than 3 values");
    }
    let season = |k: usize| -> Option<i64> {
        if n < 2 * k {
            return None;
        }
        Some(x[n - k] + (mean_i(&x[n - k..]) - mean_i(&x[n - 2 * k..n - k])).round() as i64)
    };
    let (a, how): (Option<i64>, String) = match plan {
        Plan::TsLast => (Some(x[n - 1]), "the last value".into()),
        Plan::TsDrift => (Some(2 * x[n - 1] - x[n - 2]), "last value plus the last change".into()),
        Plan::TsSeason7 => (season(7), "the value 7 back plus the change of the 7-value mean".into()),
        Plan::TsSeason24 => (season(24), "the value 24 back plus the change of the 24-value mean".into()),
        Plan::TsMean => (Some(mean_i(&x[n.saturating_sub(7)..]).round() as i64), "the mean of the last 7 values".into()),
        Plan::TsRule => {
            let t = ts_terms(&x);
            match discover::apply(rules, ts_sig(v), &t) {
                Some((val, r)) => {
                    let mut o = ToolOut::ok(e, val.to_string(), format!("own rule {} ({}) applied to the last {} values", r.id, r.form.dsl(), t.len()));
                    o.rule_id = Some(r.id.clone());
                    return o;
                }
                None => return ToolOut::fail(e, "no rule of the being applies to this prefix"),
            }
        }
        _ => (None, String::new()),
    };
    match a {
        Some(a) => ToolOut::ok(e, a.to_string(), format!("{n} values read; {how}")),
        None => ToolOut::fail(e, format!("{n} values are too few for {}", plan.name())),
    }
}

// ---- derived columns: a verified search over a small grammar ----

/// Forms of the column-rule grammar, smallest first (codes are logged). `a`, `b`, `d` are the second,
/// third and fourth columns, the target the fifth; one constant is fitted per file.
pub const COL_FORMS: [&str; 6] = ["a*b", "a*b+k", "round(a*b*(100-d)/100)", "round(a*b*r/100)", "round(a*b*(100-d)/100)+k", "round(round(a*b*(100-d)/100)*r/100)"];

fn disc(ab: i128, d: i128) -> i128 {
    (2 * ab * (100 - d) + 100).div_euclid(200)
}

fn col_form(f: u8, a: i64, b: i64, d: i64, k: i64) -> i64 {
    let (ab, d, k) = (a as i128 * b as i128, d as i128, k as i128);
    let v = match f {
        0 => ab,
        1 => ab + k,
        2 => disc(ab, d),
        3 => (2 * ab * k + 100).div_euclid(200),
        4 => disc(ab, d) + k,
        _ => (2 * disc(ab, d) * k + 100).div_euclid(200),
    };
    v as i64
}

fn fit_const(f: u8, a: i64, b: i64, d: i64, t: i64) -> i64 {
    let ab = a as i128 * b as i128;
    match f {
        1 => t - ab as i64,
        3 => if ab == 0 { 0 } else { ((t as i128 * 200 + ab) / (2 * ab)) as i64 },
        4 => t - disc(ab, d as i128) as i64,
        5 => {
            let q = disc(ab, d as i128);
            if q == 0 { 0 } else { ((t as i128 * 200 + q) / (2 * q)) as i64 }
        }
        _ => 0,
    }
}

pub struct DerivedRows {
    pub known: Vec<(i64, i64, i64, i64)>,
    pub target: Option<(i64, i64, i64)>,
}

pub fn derived_rows(bytes: &[u8], key: &str) -> Result<DerivedRows, String> {
    let text = clean_text(bytes);
    let mut known = vec![];
    let mut target = None;
    for l in text.lines().skip(1) {
        let f: Vec<&str> = l.split(',').collect();
        if f.len() != 5 {
            continue;
        }
        let a: i64 = f[1].trim().parse().map_err(|_| "bad number")?;
        let b = parse_cents(f[2].trim()).ok_or("bad money")?;
        let d: i64 = f[3].trim().parse().map_err(|_| "bad number")?;
        if f[0] == key {
            target = Some((a, b, d));
        }
        if let Some(t) = parse_cents(f[4].trim()) {
            known.push((a, b, d, t));
        }
    }
    Ok(DerivedRows { known, target })
}

/// The verified search: forms in `order` (all six, each once); a form is accepted for the file only if,
/// with its constant fitted on the first known row, it reproduces every known row exactly (at least 3
/// training rows and 1 held-out row: every third known row is held out). Returns (form, constant, tried).
pub fn col_search(rows: &DerivedRows, order: &[u8]) -> (Option<(u8, i64)>, u32) {
    let mut tried = 0;
    let Some(&(a0, b0, d0, t0)) = rows.known.first() else { return (None, 0) };
    for &f in order {
        tried += 1;
        let k = fit_const(f, a0, b0, d0, t0);
        if (f == 3 || f == 5) && !(50..=300).contains(&k) {
            continue;
        }
        let (mut tr, mut va, mut bad) = (0, 0, false);
        for (i, (a, b, d, t)) in rows.known.iter().enumerate() {
            if col_form(f, *a, *b, *d, k) == *t {
                if i % 3 == 2 { va += 1 } else { tr += 1 }
            } else {
                bad = true;
                break;
            }
        }
        if !bad && tr >= 3 && va >= 1 {
            return (Some((f, k)), tried);
        }
    }
    (None, tried)
}

/// Default order (smallest first), or the being's: forms accepted more often before in this context first.
pub fn search_order(counts: &[u32; 6]) -> Vec<u8> {
    let mut o: Vec<u8> = (0..6).collect();
    o.sort_by_key(|f| (std::cmp::Reverse(counts[*f as usize]), *f));
    o
}

fn derived_plan(plan: Plan, v: &ViewT, bytes: &[u8], order: &[u8]) -> ToolOut {
    let e0 = plan.energy(v.rows);
    let rows = match derived_rows(bytes, &v.param) {
        Ok(r) => r,
        Err(x) => return ToolOut::fail(e0, x),
    };
    let Some((a, b, d)) = rows.target else { return ToolOut::fail(e0, "the row is not in the file") };
    match plan {
        Plan::ColProduct => ToolOut::ok(e0, super::data::money_canon(a * b), format!("{} * {}", v.cols[1], v.cols[3])),
        Plan::ColRatio => {
            let mut rs: Vec<i64> = rows.known.iter().filter(|r| r.0 * r.1 != 0).map(|r| fit_const(3, r.0, r.1, r.2, r.3)).collect();
            if rs.is_empty() {
                return ToolOut::fail(e0, "no known rows");
            }
            rs.sort();
            let r = rs[rs.len() / 2];
            ToolOut::ok(e0, super::data::money_canon(col_form(3, a, b, d, r)), format!("median ratio {r}/100 over {} known rows", rs.len()))
        }
        _ => {
            let (found, tried) = col_search(&rows, order);
            let e = 2 + (tried as i64 + 1) / 2;
            match found {
                Some((f, k)) => {
                    let mut o = ToolOut::ok(e, super::data::money_canon(col_form(f, a, b, d, k)), format!("verified form {} (constant {k}) reproduces all {} known rows; {tried} forms tried", COL_FORMS[f as usize], rows.known.len()));
                    o.form = Some(f);
                    o.tried = tried;
                    o
                }
                None => {
                    let mut o = ToolOut::fail(e, format!("no form of the grammar reproduces all {} known rows; {tried} forms tried", rows.known.len()));
                    o.tried = tried;
                    o
                }
            }
        }
    }
}

// ---- malformed lines ----

fn is_int(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty() && t.trim_start_matches('-').chars().all(|c| c.is_ascii_digit())
}

fn is_date(s: &str) -> bool {
    let p: Vec<&str> = s.trim().split('-').collect();
    if p.len() != 3 || p[0].len() != 4 || p[1].len() != 2 || p[2].len() != 2 {
        return false;
    }
    let (Ok(y), Ok(m), Ok(d)) = (p[0].parse::<i64>(), p[1].parse::<i64>(), p[2].parse::<i64>()) else { return false };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let dim = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    (1..=12).contains(&m) && d >= 1 && d <= dim[(m - 1) as usize]
}

#[derive(Clone, Copy, PartialEq)]
enum CT {
    Int,
    Num,
    Date,
    Text,
}

fn fix_plan(plan: Plan, v: &ViewT, bytes: &[u8]) -> ToolOut {
    let e = plan.energy(v.rows);
    let text = clean_text(bytes);
    let d = sniff_delim(&text);
    let lines: Vec<Vec<String>> = text.lines().map(|l| split_quoted(l, d)).collect();
    let Some(h) = lines.first().map(|x| x.len()) else { return ToolOut::fail(e, "empty file") };
    if plan == Plan::FixFields {
        for (i, f) in lines.iter().enumerate().skip(1) {
            if f.len() != h {
                return ToolOut::ok(e, (i + 1).to_string(), format!("line {} has {} fields, the header {h}", i + 1, f.len()));
            }
        }
        return ToolOut::fail(e, "every line has the header's field count");
    }
    let mut types = vec![CT::Text; h];
    for (j, t) in types.iter_mut().enumerate() {
        let vals: Vec<&str> = lines.iter().skip(1).filter(|f| f.len() == h).map(|f| f[j].trim()).filter(|s| !is_null(s)).collect();
        let share = |p: &dyn Fn(&str) -> bool| vals.iter().filter(|s| p(s)).count() * 10 >= vals.len() * 9 && !vals.is_empty();
        *t = if share(&is_int) { CT::Int } else if share(&|s| clean_num(s, d).is_some()) { CT::Num } else if share(&|s| s.len() == 10 && s.as_bytes()[4] == b'-') { CT::Date } else { CT::Text };
    }
    for (i, f) in lines.iter().enumerate().skip(1) {
        if f.len() != h {
            return ToolOut::ok(e, (i + 1).to_string(), format!("line {} has {} fields, the header {h}", i + 1, f.len()));
        }
        for (j, x) in f.iter().enumerate() {
            let x = x.trim();
            if is_null(x) {
                continue;
            }
            let ok = match types[j] {
                CT::Int => is_int(x),
                CT::Num => clean_num(x, d).is_some(),
                CT::Date => is_date(x),
                CT::Text => true,
            };
            if !ok {
                return ToolOut::ok(e, (i + 1).to_string(), format!("line {}: {x:?} does not fit column {}", i + 1, lines[0].get(j).map_or("?", |s| s.as_str())));
            }
        }
    }
    ToolOut::fail(e, "no line breaks the field count or the column types")
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
        return ToolOut::ok(e, format!("{}/header", super::data::delim_name(d)), format!("the extension .{ext} suggests {}; a header is assumed", super::data::delim_name(d)));
    }
    let text = clean_text(bytes);
    let d = sniff_delim(&text);
    let mut ls = text.lines();
    let (l1, l2) = (split_quoted(ls.next().unwrap_or(""), d), split_quoted(ls.next().unwrap_or(""), d));
    let numeric = |f: &Vec<String>| f.iter().filter(|x| clean_num(x, d).is_some() || is_date(x)).count();
    let header = numeric(&l1) == 0 && numeric(&l2) > 0;
    ToolOut::ok(e, format!("{}/{}", super::data::delim_name(d), if header { "header" } else { "noheader" }), format!("sniffed {}; first line has {} numeric fields, second {}", super::data::delim_name(d), numeric(&l1), numeric(&l2)))
}

// ---- logs ----

/// (level, service) of each entry; continuation lines (starting with white space) are skipped.
pub fn parse_log(bytes: &[u8]) -> Vec<(String, String)> {
    let text = clean_text(bytes);
    let mut out = vec![];
    for l in text.lines() {
        if l.starts_with(char::is_whitespace) || l.trim().is_empty() {
            continue;
        }
        let mut t = l.split_whitespace();
        let (_, lv, sv) = (t.next(), t.next(), t.next());
        if let (Some(lv), Some(sv)) = (lv, sv) {
            out.push((lv.to_string(), sv.trim_start_matches('[').trim_end_matches(']').to_string()));
        }
    }
    out
}

fn log_plan(plan: Plan, v: &ViewT, bytes: &[u8], lim: SqlLimits) -> ToolOut {
    let e = plan.energy(v.rows);
    let text = clean_text(bytes);
    match plan {
        Plan::LogGrep => {
            let errs: Vec<&str> = text.lines().filter(|l| l.contains(" ERROR ")).collect();
            if v.tmpl == Tmpl::LogCount {
                let tag = format!("[{}]", v.param);
                let n = errs.iter().filter(|l| l.contains(&tag)).count();
                return ToolOut::ok(e, n.to_string(), format!("{n} lines contain \" ERROR \" and {tag}"));
            }
            let mut m: std::collections::BTreeMap<String, usize> = Default::default();
            for l in &errs {
                if let (Some(a), Some(b)) = (l.find('['), l.find(']')) {
                    if a < b {
                        *m.entry(l[a + 1..b].to_string()).or_default() += 1;
                    }
                }
            }
            let best = m.values().max().copied().unwrap_or(0);
            let tops: Vec<&String> = m.iter().filter(|(_, c)| **c == best).map(|(k, _)| k).collect();
            if tops.len() != 1 {
                return ToolOut::fail(e, "no single top service among lines with \" ERROR \"");
            }
            ToolOut::ok(e, tops[0].clone(), format!("{} lines contain \" ERROR \"; counted per bracketed service", errs.len()))
        }
        Plan::LogParse => {
            let p = parse_log(bytes);
            let n = p.iter().filter(|(l, s)| l.eq_ignore_ascii_case("ERROR") && *s == v.param).count();
            ToolOut::ok(e, n.to_string(), format!("{} entries parsed (continuation lines skipped); {n} at level ERROR for {}", p.len(), v.param))
        }
        _ => {
            let p = parse_log(bytes);
            let block = DataBlock::new(vec![
                Column::str_col("level", p.iter().map(|x| Some(x.0.clone())).collect()),
                Column::str_col("service", p.iter().map(|x| Some(x.1.clone())).collect()),
            ]);
            let sql = match sql_text(v.tmpl, v) {
                Ok(s) => s,
                Err(x) => return ToolOut::fail(e, x),
            };
            let block = match block {
                Ok(b) => b,
                Err(x) => return ToolOut::fail(e, x.to_string()),
            };
            let (res, rec) = guarded_sql(block, &sql, lim);
            let mut o = match res.ok_or_else(|| rec.why.clone()).and_then(|b| sql_answer(v.tmpl, &b)) {
                Ok(a) => ToolOut::ok(e, a, format!("{} entries parsed; KORE SQL (read-only, in-memory copy): {sql}", p.len())),
                Err(x) => ToolOut::fail(e, format!("{} entries parsed; KORE SQL: {sql}: {x}", p.len())),
            };
            o.sql.push(rec);
            o
        }
    }
}

/// What the tools may use of the being's state.
pub struct ToolCtx<'a> {
    pub rules: &'a [Rule],
    /// Column-rule search order (from the being's own record in this context).
    pub order: Vec<u8>,
    pub sql: SqlLimits,
}

/// Run one plan on the bytes of a task.
pub fn run_plan(plan: Plan, v: &ViewT, bytes: &[u8], cx: &ToolCtx) -> ToolOut {
    match plan {
        Plan::NaiveNative | Plan::SniffNative | Plan::NaiveSql | Plan::SniffSql => table_plan(plan, v, bytes, cx.sql),
        Plan::TsLast | Plan::TsDrift | Plan::TsSeason7 | Plan::TsSeason24 | Plan::TsMean | Plan::TsRule => ts_plan(plan, v, bytes, cx.rules),
        Plan::ColProduct | Plan::ColRatio | Plan::ColSearch => derived_plan(plan, v, bytes, &cx.order),
        Plan::FixFields | Plan::FixTypes => fix_plan(plan, v, bytes),
        Plan::FmtExt | Plan::FmtSniff => fmt_plan(plan, v, bytes),
        Plan::LogGrep | Plan::LogParse | Plan::LogSql => log_plan(plan, v, bytes, cx.sql),
    }
}
