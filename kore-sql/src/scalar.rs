//! Scalar SQL function library (Spark SQL semantics). Every function here is a pure function of its
//! already-evaluated arguments; NULL in generally means NULL out. Control-flow functions that must not
//! evaluate all their arguments (COALESCE, IF, ...) live in the executor.

use std::cmp::Ordering;

use crate::datetime::{self as dt, Dt};
use crate::executor::ExprVal;

type V = ExprVal;

// ─── conversions ──────────────────────────────────────────────────────────────

/// Spark's double-to-string: `2.0`, `1.5`, `1.0E-5`, `NaN`, `Infinity`.
pub fn fmt_f64(f: f64) -> String {
    if f.is_nan() { return "NaN".into(); }
    if f.is_infinite() { return if f > 0.0 { "Infinity".into() } else { "-Infinity".into() }; }
    let a = f.abs();
    if a != 0.0 && (a >= 1e7 || a < 1e-3) {
        let s = format!("{:e}", f); // 1.5e-5 / 1e20
        let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
        let m = if m.contains('.') { m.to_string() } else { format!("{m}.0") };
        return format!("{m}E{e}");
    }
    if f == f.trunc() { return format!("{:.1}", f); }
    format!("{}", f)
}

pub fn to_str(v: &V) -> Option<String> {
    match v {
        V::Str(s) => Some(s.clone()),
        V::Int(i) => Some(i.to_string()),
        V::Float(f) => Some(fmt_f64(*f)),
        V::Bool(b) => Some(b.to_string()),
        V::Null => None,
    }
}

pub fn parse_f64(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() { return None; }
    match t.to_ascii_lowercase().as_str() {
        "nan" => return Some(f64::NAN),
        "inf" | "+inf" | "infinity" | "+infinity" => return Some(f64::INFINITY),
        "-inf" | "-infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    if t.chars().any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E') { return None; }
    t.parse::<f64>().ok()
}

pub fn num(v: &V) -> Option<f64> {
    match v {
        V::Int(i) => Some(*i as f64),
        V::Float(f) => Some(*f),
        V::Str(s) => parse_f64(s),
        V::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        V::Null => None,
    }
}

fn int(v: &V) -> Option<i64> {
    match v {
        V::Int(i) => Some(*i),
        V::Float(f) => if f.is_nan() { None } else { Some(*f as i64) },
        V::Str(s) => str_to_int(s, 64),
        V::Bool(b) => Some(*b as i64),
        V::Null => None,
    }
}

/// Integer part of a numeric string (`" 12 "`, `"1.9"` -> 1); None when not numeric or out of range.
pub fn str_to_int(s: &str, bits: u32) -> Option<i64> {
    let t = s.trim();
    let (neg, body) = match t.strip_prefix('-') { Some(r) => (true, r), None => (false, t.strip_prefix('+').unwrap_or(t)) };
    let (ip, fp) = match body.split_once('.') { Some((a, b)) => (a, b), None => (body, "") };
    if (ip.is_empty() && fp.is_empty()) || !ip.chars().all(|c| c.is_ascii_digit()) || !fp.chars().all(|c| c.is_ascii_digit()) { return None; }
    let mag: i128 = if ip.is_empty() { 0 } else { ip.parse().ok()? };
    let val = if neg { -mag } else { mag };
    let (lo, hi) = (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1);
    if val < lo || val > hi { None } else { Some(val as i64) }
}

fn f2v(f: f64) -> V { V::Float(f) }

/// Total-ish ordering of two non-NULL values (numbers numerically, strings lexically).
pub fn cmp_vals(a: &V, b: &V) -> Option<Ordering> {
    match (a, b) {
        (V::Int(x), V::Int(y)) => Some(x.cmp(y)),
        (V::Str(x), V::Str(y)) => Some(x.cmp(y)),
        (V::Bool(x), V::Bool(y)) => Some(x.cmp(y)),
        (V::Null, _) | (_, V::Null) => None,
        (V::Str(s), n) | (n, V::Str(s)) if !matches!(n, V::Str(_)) => {
            let x = parse_f64(s)?;
            let y = num(n)?;
            let o = x.partial_cmp(&y)?;
            Some(if matches!(a, V::Str(_)) { o } else { o.reverse() })
        }
        _ => num(a)?.partial_cmp(&num(b)?),
    }
}

/// SQL equality (None = NULL).
pub fn eq_vals(a: &V, b: &V) -> Option<bool> {
    cmp_vals(a, b).map(|o| o == Ordering::Equal)
}

// ─── CAST ─────────────────────────────────────────────────────────────────────

/// `CAST(v AS ty)`; `try_` only matters for ANSI semantics, so both return NULL on failure.
pub fn cast(v: V, ty: &str, p: Option<i64>, s: Option<i64>) -> V {
    if matches!(v, V::Null) { return V::Null; }
    let ty = ty.to_ascii_uppercase();
    match ty.as_str() {
        "INT" | "INTEGER" | "SMALLINT" | "SHORT" | "TINYINT" | "BYTE" | "BIGINT" | "LONG" => {
            let bits = match ty.as_str() { "INT" | "INTEGER" => 32, "SMALLINT" | "SHORT" => 16, "TINYINT" | "BYTE" => 8, _ => 64 };
            match v {
                V::Int(i) => if bits == 64 { V::Int(i) } else { V::Int(wrap(i, bits)) },
                V::Float(f) => if f.is_nan() { V::Int(0) } else if bits == 64 { V::Int(f as i64) } else { V::Int(sat(f, bits)) },
                V::Bool(b) => V::Int(b as i64),
                V::Str(s) => str_to_int(&s, bits).map(V::Int).unwrap_or(V::Null),
                V::Null => V::Null,
            }
        }
        "FLOAT" | "DOUBLE" | "REAL" => match v {
            V::Str(s) => parse_f64(&s).map(V::Float).unwrap_or(V::Null),
            other => num(&other).map(V::Float).unwrap_or(V::Null),
        },
        "DECIMAL" | "NUMERIC" | "DEC" => {
            let scale = s.unwrap_or(0);
            let x = match &v { V::Str(st) => parse_f64(st), other => num(other) };
            match x {
                Some(x) if x.is_finite() => {
                    let r = round_half_up(x, scale);
                    // overflow of the declared precision gives NULL
                    let max = 10f64.powi((p.unwrap_or(10) - scale).max(0) as i32);
                    if r.abs() >= max { V::Null } else { V::Float(r) }
                }
                _ => V::Null,
            }
        }
        "STRING" | "VARCHAR" | "CHAR" | "TEXT" | "CHARACTER" => {
            let st = to_str(&v).unwrap_or_default();
            match (ty.as_str(), p) {
                ("VARCHAR" | "CHAR" | "CHARACTER", Some(n)) if n >= 0 => V::Str(st.chars().take(n as usize).collect()),
                _ => V::Str(st),
            }
        }
        "BOOLEAN" | "BOOL" => match v {
            V::Bool(b) => V::Bool(b),
            V::Int(i) => V::Bool(i != 0),
            V::Float(f) => V::Bool(f != 0.0),
            V::Str(s) => match s.trim().to_ascii_lowercase().as_str() {
                "t" | "true" | "y" | "yes" | "1" => V::Bool(true),
                "f" | "false" | "n" | "no" | "0" => V::Bool(false),
                _ => V::Null,
            },
            V::Null => V::Null,
        },
        "DATE" => to_dt(&v).map(|d| V::Str(dt::fmt_date(d.days))).unwrap_or(V::Null),
        "TIMESTAMP" | "TIMESTAMP_NTZ" | "DATETIME" => to_dt(&v).map(|d| V::Str(dt::fmt_ts(d.days, d.secs, d.nanos))).unwrap_or(V::Null),
        _ => v,
    }
}

fn wrap(i: i64, bits: u32) -> i64 {
    match bits { 32 => i as i32 as i64, 16 => i as i16 as i64, 8 => i as i8 as i64, _ => i }
}

fn sat(f: f64, bits: u32) -> i64 {
    let (lo, hi) = (-(1i64 << (bits - 1)) as f64, ((1i64 << (bits - 1)) - 1) as f64);
    f.trunc().max(lo).min(hi) as i64
}

// ─── rounding ─────────────────────────────────────────────────────────────────

/// Round to `scale` decimal places, ties away from zero, working on the shortest decimal
/// representation of the double (so 1.005 rounds to 1.01 as Spark's BigDecimal does).
pub fn round_half_up(f: f64, scale: i64) -> f64 { round_dec(f, scale, false) }
pub fn round_half_even(f: f64, scale: i64) -> f64 { round_dec(f, scale, true) }

fn round_dec(f: f64, scale: i64, even: bool) -> f64 {
    if !f.is_finite() || f == 0.0 { return f; }
    let neg = f < 0.0;
    let s = format!("{:e}", f.abs());
    let (mant, exp) = s.split_once('e').unwrap();
    let e: i64 = exp.parse().unwrap();
    let digits: Vec<u8> = mant.bytes().filter(|b| b.is_ascii_digit()).map(|b| b - b'0').collect();
    let keep = e + 1 + scale; // number of leading digits kept
    let k: u128 = if keep < 0 {
        0
    } else if keep as usize >= digits.len() {
        return f;
    } else {
        let keep = keep as usize;
        let mut k: u128 = digits[..keep].iter().fold(0u128, |a, &d| a * 10 + d as u128);
        let next = digits[keep];
        let rest_zero = digits[keep + 1..].iter().all(|&d| d == 0);
        let up = if next > 5 || (next == 5 && !rest_zero) { true } else if next == 5 { if even { k % 2 == 1 } else { true } } else { false };
        if up { k += 1; }
        k
    };
    let r = k as f64 / 10f64.powi(scale as i32);
    let r = if scale < 0 { k as f64 * 10f64.powi((-scale) as i32) } else { r };
    if neg { -r } else { r }
}

// ─── dates ────────────────────────────────────────────────────────────────────

pub fn to_dt(v: &V) -> Option<Dt> {
    match v {
        V::Str(s) => dt::parse(s),
        V::Int(i) => dt::from_yyyymmdd(*i),
        _ => None,
    }
}

fn date_v(d: Dt) -> V { V::Str(dt::fmt_date(d.days)) }
fn ts_v(d: Dt) -> V { V::Str(dt::fmt_ts(d.days, d.secs, d.nanos)) }

pub fn date_part(field: &str, d: &Dt) -> Option<V> {
    let (y, m, day) = d.ymd();
    Some(V::Int(match field.trim().to_ascii_lowercase().as_str() {
        "year" | "years" | "y" | "yyyy" | "yy" | "yr" => y,
        "quarter" | "qtr" => (m - 1) / 3 + 1,
        "month" | "months" | "mon" | "mm" => m,
        "week" | "weeks" | "w" => d.iso_week(),
        "day" | "days" | "d" | "dd" | "dayofmonth" => day,
        "dayofweek" | "dow" => (d.weekday() + 1) % 7 + 1,
        "dayofweek_iso" | "isodow" => d.weekday() + 1,
        "doy" | "dayofyear" => d.day_of_year(),
        "hour" | "hours" | "h" | "hh" => d.secs / 3600,
        "minute" | "minutes" | "min" | "mi" => (d.secs / 60) % 60,
        "second" | "seconds" | "sec" | "s" | "ss" => d.secs % 60,
        "epoch" => d.epoch_secs(),
        _ => return None,
    }))
}

// ─── strings ──────────────────────────────────────────────────────────────────

fn substr(s: &str, start: i64, len: Option<i64>) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len() as i64;
    // 1-based; 0 behaves like 1; negative counts from the end
    let from = if start > 0 { start - 1 } else if start < 0 { n + start } else { 0 };
    let (lo, hi) = match len {
        Some(l) if l <= 0 => return String::new(),
        Some(l) => (from, from.saturating_add(l)),
        None => (from, n),
    };
    let (lo, hi) = (lo.max(0), hi.min(n));
    if lo >= hi { String::new() } else { chars[lo as usize..hi as usize].iter().collect() }
}

fn pad(s: &str, len: i64, p: &str, left: bool) -> String {
    if len <= 0 { return String::new(); }
    let len = len as usize;
    let n = s.chars().count();
    if n >= len { return s.chars().take(len).collect(); }
    if p.is_empty() { return s.to_string(); }
    let fill: String = p.chars().cycle().take(len - n).collect();
    if left { format!("{fill}{s}") } else { format!("{s}{fill}") }
}

fn trim_chars(s: &str, set: &str, lead: bool, trail: bool) -> String {
    let f = |c: char| set.contains(c);
    let mut t: &str = s;
    if lead { t = t.trim_start_matches(f); }
    if trail { t = t.trim_end_matches(f); }
    t.to_string()
}

fn regex_of(p: &str) -> Option<regex::Regex> {
    thread_local! {
        static CACHE: std::cell::RefCell<std::collections::HashMap<String, Option<regex::Regex>>> = std::cell::RefCell::new(Default::default());
    }
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(r) = c.get(p) { return r.clone(); }
        let r = regex::Regex::new(p).ok();
        if c.len() > 256 { c.clear(); }
        c.insert(p.to_string(), r.clone());
        r
    })
}

/// Java-style `$1` group references in a replacement string become Rust `${1}`.
fn java_replacement(r: &str) -> String {
    let mut out = String::new();
    let cs: Vec<char> = r.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '\\' && i + 1 < cs.len() { if cs[i + 1] == '$' { out.push_str("$$"); } else { out.push(cs[i + 1]); } i += 2; continue; }
        if cs[i] == '$' && i + 1 < cs.len() && cs[i + 1].is_ascii_digit() { out.push_str("${"); out.push(cs[i + 1]); out.push('}'); i += 2; continue; }
        out.push(cs[i]); i += 1;
    }
    out
}

fn levenshtein(a: &str, b: &str) -> i64 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + if a[i - 1] == b[j - 1] { 0 } else { 1 });
        }
        prev = cur;
    }
    prev[b.len()] as i64
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for ch in bytes.chunks(3) {
        let n = (ch[0] as u32) << 16 | (*ch.get(1).unwrap_or(&0) as u32) << 8 | *ch.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if ch.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if ch.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn java_format(fmt: &str, args: &[V]) -> String {
    let cs: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let (mut i, mut ai) = (0, 0);
    while i < cs.len() {
        if cs[i] != '%' { out.push(cs[i]); i += 1; continue; }
        i += 1;
        if i >= cs.len() { break; }
        if cs[i] == '%' { out.push('%'); i += 1; continue; }
        if cs[i] == 'n' { out.push('\n'); i += 1; continue; }
        let mut flags = String::new();
        while i < cs.len() && "-+0 ,#(".contains(cs[i]) { flags.push(cs[i]); i += 1; }
        let mut width = String::new();
        while i < cs.len() && cs[i].is_ascii_digit() { width.push(cs[i]); i += 1; }
        let mut prec: Option<usize> = None;
        if i < cs.len() && cs[i] == '.' { i += 1; let mut p = String::new(); while i < cs.len() && cs[i].is_ascii_digit() { p.push(cs[i]); i += 1; } prec = p.parse().ok(); }
        let Some(&conv) = cs.get(i) else { break };
        i += 1;
        let arg = args.get(ai).cloned().unwrap_or(V::Null);
        ai += 1;
        let mut body = match conv {
            'd' => match int(&arg) { Some(n) => { let mut s = n.abs().to_string(); if flags.contains(',') { s = group3(&s); } if n < 0 { format!("-{s}") } else if flags.contains('+') { format!("+{s}") } else { s } } None => "null".into() },
            'f' => match num(&arg) { Some(x) => { let mut s = format!("{:.*}", prec.unwrap_or(6), x.abs()); if flags.contains(',') { let (a, b) = s.split_once('.').map(|(a, b)| (a.to_string(), format!(".{b}"))).unwrap_or((s.clone(), String::new())); s = format!("{}{}", group3(&a), b); } if x < 0.0 { format!("-{s}") } else if flags.contains('+') { format!("+{s}") } else { s } } None => "null".into() },
            'e' | 'E' => num(&arg).map(|x| format!("{:.*e}", prec.unwrap_or(6), x)).unwrap_or("null".into()),
            's' | 'S' => { let s = to_str(&arg).unwrap_or("null".into()); let s = match prec { Some(p) => s.chars().take(p).collect(), None => s }; if conv == 'S' { s.to_uppercase() } else { s } }
            'x' => int(&arg).map(|n| format!("{:x}", n)).unwrap_or("null".into()),
            'X' => int(&arg).map(|n| format!("{:X}", n)).unwrap_or("null".into()),
            'c' => int(&arg).and_then(|n| char::from_u32(n as u32)).map(|c| c.to_string()).unwrap_or("null".into()),
            'b' | 'B' => match &arg { V::Null => "false".into(), V::Bool(b) => b.to_string(), _ => "true".into() },
            _ => String::new(),
        };
        if let Ok(w) = width.parse::<usize>() {
            let n = body.chars().count();
            if n < w {
                let fill = w - n;
                if flags.contains('-') { body.push_str(&" ".repeat(fill)); }
                else if flags.contains('0') && matches!(conv, 'd' | 'f') {
                    let (sign, digits) = if body.starts_with('-') || body.starts_with('+') { body.split_at(1) } else { ("", body.as_str()) };
                    body = format!("{sign}{}{digits}", "0".repeat(fill));
                } else { body = format!("{}{body}", " ".repeat(fill)); }
            }
        }
        out.push_str(&body);
    }
    out
}

fn group3(digits: &str) -> String {
    let n = digits.len();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (n - i) % 3 == 0 { out.push(','); }
        out.push(c);
    }
    out
}

// ─── the function table ───────────────────────────────────────────────────────

/// Functions that must see their arguments unevaluated (handled by the executor).
pub fn is_lazy(name: &str) -> bool {
    matches!(name, "COALESCE" | "NVL" | "IFNULL" | "IF" | "IIF" | "NVL2" | "ELEMENT_AT" | "SIZE" | "CARDINALITY" | "ARRAY_CONTAINS" | "INTERVAL" | "GROUPING" | "GROUPING_ID")
}

/// Evaluate a pure scalar function. None = not a function of this library.
pub fn call(name: &str, a: &[V]) -> Option<V> {
    let arg = |i: usize| a.get(i).unwrap_or(&V::Null);
    let n = a.len();
    let s = |i: usize| to_str(arg(i));
    let f = |i: usize| num(arg(i));
    let k = |i: usize| int(arg(i));
    // NULL-propagating helper: all listed arguments must be non-NULL
    macro_rules! nn { ($($i:expr),*) => { $( if matches!(arg($i), V::Null) { return Some(V::Null); } )* } }
    macro_rules! get { ($e:expr) => { match $e { Some(v) => v, None => return Some(V::Null) } } }
    macro_rules! math1 { ($fun:expr) => {{ nn!(0); let x = get!(f(0)); V::Float($fun(x)) }} }

    Some(match name {
        // ── strings ──
        "UPPER" | "UCASE" => { nn!(0); V::Str(get!(s(0)).to_uppercase()) }
        "LOWER" | "LCASE" => { nn!(0); V::Str(get!(s(0)).to_lowercase()) }
        "LENGTH" | "CHAR_LENGTH" | "CHARACTER_LENGTH" | "LEN" => { nn!(0); V::Int(get!(s(0)).chars().count() as i64) }
        "OCTET_LENGTH" => { nn!(0); V::Int(get!(s(0)).len() as i64) }
        "BIT_LENGTH" => { nn!(0); V::Int(get!(s(0)).len() as i64 * 8) }
        "TRIM" | "BTRIM" => { nn!(0); if n >= 2 { nn!(1); V::Str(trim_chars(&get!(s(0)), &get!(s(1)), true, true)) } else { V::Str(trim_chars(&get!(s(0)), " ", true, true)) } }
        // Spark: ltrim(trimStr, str) / rtrim(trimStr, str), btrim(str, trimStr)
        "LTRIM" => { nn!(0); if n >= 2 { nn!(1); V::Str(trim_chars(&get!(s(1)), &get!(s(0)), true, false)) } else { V::Str(trim_chars(&get!(s(0)), " ", true, false)) } }
        "RTRIM" => { nn!(0); if n >= 2 { nn!(1); V::Str(trim_chars(&get!(s(1)), &get!(s(0)), false, true)) } else { V::Str(trim_chars(&get!(s(0)), " ", false, true)) } }
        "__TRIM_BOTH" | "__TRIM_LEADING" | "__TRIM_TRAILING" => {
            // TRIM(BOTH|LEADING|TRAILING chars FROM str) is rewritten by the parser to (str, chars)
            nn!(0, 1);
            V::Str(trim_chars(&get!(s(0)), &get!(s(1)), name != "__TRIM_TRAILING", name != "__TRIM_LEADING"))
        }
        "SUBSTR" | "SUBSTRING" | "MID" => {
            nn!(0, 1);
            let st = get!(s(0));
            let start = get!(k(1));
            let len = if n >= 3 { if matches!(arg(2), V::Null) { return Some(V::Null); } Some(get!(k(2))) } else { None };
            V::Str(substr(&st, start, len))
        }
        "LEFT" => { nn!(0, 1); let l = get!(k(1)); V::Str(if l <= 0 { String::new() } else { get!(s(0)).chars().take(l as usize).collect() }) }
        "RIGHT" => { nn!(0, 1); let l = get!(k(1)); let st = get!(s(0)); let c: Vec<char> = st.chars().collect(); V::Str(if l <= 0 { String::new() } else { c[c.len().saturating_sub(l as usize)..].iter().collect() }) }
        "CONCAT" => {
            let mut out = String::new();
            for v in a { match to_str(v) { Some(x) => out.push_str(&x), None => return Some(V::Null) } }
            V::Str(out)
        }
        "CONCAT_WS" => {
            nn!(0);
            let sep = get!(s(0));
            V::Str(a[1..].iter().filter_map(to_str).collect::<Vec<_>>().join(&sep))
        }
        "REPLACE" => { nn!(0, 1); let from = get!(s(1)); let to = if n >= 3 { nn!(2); get!(s(2)) } else { String::new() }; let st = get!(s(0)); V::Str(if from.is_empty() { st } else { st.replace(&from, &to) }) }
        "REPEAT" => { nn!(0, 1); let c = get!(k(1)); V::Str(if c <= 0 { String::new() } else { get!(s(0)).repeat(c as usize) }) }
        "REVERSE" => { nn!(0); V::Str(get!(s(0)).chars().rev().collect()) }
        "LPAD" => { nn!(0, 1); let p = if n >= 3 { get!(s(2)) } else { " ".into() }; V::Str(pad(&get!(s(0)), get!(k(1)), &p, true)) }
        "RPAD" => { nn!(0, 1); let p = if n >= 3 { get!(s(2)) } else { " ".into() }; V::Str(pad(&get!(s(0)), get!(k(1)), &p, false)) }
        "INSTR" | "STRPOS" => { nn!(0, 1); let h = get!(s(0)); let nd = get!(s(1)); V::Int(h.find(&nd).map(|p| h[..p].chars().count() as i64 + 1).unwrap_or(0)) }
        "CHARINDEX" => { nn!(0, 1); let nd = get!(s(0)); let h = get!(s(1)); V::Int(h.find(&nd).map(|p| h[..p].chars().count() as i64 + 1).unwrap_or(0)) }
        "LOCATE" | "POSITION" => {
            nn!(0, 1);
            let nd = get!(s(0));
            let h = get!(s(1));
            let from = if n >= 3 { get!(k(2)).max(1) as usize - 1 } else { 0 };
            let hc: Vec<char> = h.chars().collect();
            if from > hc.len() { V::Int(0) } else {
                let tail: String = hc[from..].iter().collect();
                V::Int(tail.find(&nd).map(|p| (from + tail[..p].chars().count()) as i64 + 1).unwrap_or(0))
            }
        }
        "INITCAP" => {
            nn!(0);
            let mut out = String::new();
            let mut start = true;
            for c in get!(s(0)).chars() {
                if c.is_whitespace() { start = true; out.push(c); }
                else if start { out.extend(c.to_uppercase()); start = false; }
                else { out.extend(c.to_lowercase()); }
            }
            V::Str(out)
        }
        "SPACE" => { nn!(0); V::Str(" ".repeat(get!(k(0)).max(0) as usize)) }
        "ASCII" | "ORD" => { nn!(0); V::Int(get!(s(0)).chars().next().map(|c| c as i64).unwrap_or(0)) }
        "CHR" | "CHAR" => { nn!(0); let c = get!(k(0)); V::Str(if c < 0 { String::new() } else { char::from_u32((c % 256) as u32).map(|c| c.to_string()).unwrap_or_default() }) }
        "TRANSLATE" => {
            nn!(0, 1, 2);
            let (from, to): (Vec<char>, Vec<char>) = (get!(s(1)).chars().collect(), get!(s(2)).chars().collect());
            V::Str(get!(s(0)).chars().filter_map(|c| match from.iter().position(|&x| x == c) { Some(i) => to.get(i).copied(), None => Some(c) }).collect())
        }
        "SPLIT_PART" => {
            nn!(0, 1, 2);
            let (st, sep, idx) = (get!(s(0)), get!(s(1)), get!(k(2)));
            if idx == 0 { return Some(V::Null); }
            let parts: Vec<&str> = if sep.is_empty() { vec![st.as_str()] } else { st.split(sep.as_str()).collect() };
            let i = if idx > 0 { idx - 1 } else { parts.len() as i64 + idx };
            V::Str(if i >= 0 { parts.get(i as usize).copied().unwrap_or("").to_string() } else { String::new() })
        }
        "SUBSTRING_INDEX" => {
            nn!(0, 1, 2);
            let (st, d, c) = (get!(s(0)), get!(s(1)), get!(k(2)));
            if d.is_empty() || c == 0 { return Some(V::Str(String::new())); }
            let parts: Vec<&str> = st.split(d.as_str()).collect();
            let m = parts.len() as i64;
            V::Str(if c > 0 { if c >= m { st } else { parts[..c as usize].join(&d) } } else if -c >= m { st } else { parts[(m + c) as usize..].join(&d) })
        }
        "OVERLAY" => {
            nn!(0, 1, 2);
            let (st, rep, pos) = (get!(s(0)), get!(s(1)), get!(k(2)));
            let len = if n >= 4 { get!(k(3)) } else { rep.chars().count() as i64 };
            let c: Vec<char> = st.chars().collect();
            let p = (pos - 1).clamp(0, c.len() as i64) as usize;
            let e = (p as i64 + len.max(0)).min(c.len() as i64) as usize;
            V::Str(format!("{}{}{}", c[..p].iter().collect::<String>(), rep, c[e..].iter().collect::<String>()))
        }
        "STARTSWITH" => { nn!(0, 1); V::Bool(get!(s(0)).starts_with(&get!(s(1)))) }
        "ENDSWITH" => { nn!(0, 1); V::Bool(get!(s(0)).ends_with(&get!(s(1)))) }
        "CONTAINS" => { nn!(0, 1); V::Bool(get!(s(0)).contains(&get!(s(1)))) }
        "LEVENSHTEIN" => { nn!(0, 1); V::Int(levenshtein(&get!(s(0)), &get!(s(1)))) }
        "FORMAT_STRING" | "PRINTF" => { nn!(0); V::Str(java_format(&get!(s(0)), &a[1..])) }
        "FORMAT_NUMBER" => {
            nn!(0, 1);
            let (x, d) = (get!(f(0)), get!(k(1)).max(0) as usize);
            let r = format!("{:.*}", d, round_half_up(x.abs(), d as i64));
            let (ip, fp) = r.split_once('.').map(|(a, b)| (a.to_string(), format!(".{b}"))).unwrap_or((r.clone(), String::new()));
            V::Str(format!("{}{}{}", if x < 0.0 { "-" } else { "" }, group3(&ip), fp))
        }
        "BASE64" => { nn!(0); V::Str(base64(get!(s(0)).as_bytes())) }
        "HEX" => { nn!(0); match arg(0) { V::Int(i) => V::Str(format!("{:X}", i)), o => V::Str(get!(to_str(o)).bytes().map(|b| format!("{:02X}", b)).collect()) } }
        "UNHEX" => { nn!(0); let h = get!(s(0)); let bytes: Option<Vec<u8>> = (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).ok()).collect(); V::Str(String::from_utf8_lossy(&get!(bytes)).to_string()) }
        "TO_CHAR" | "TO_VARCHAR" => {
            nn!(0);
            if n >= 2 {
                if let Some(d) = to_dt(arg(0)) { return Some(V::Str(dt::format_pattern(&d, &get!(s(1))))); }
            }
            V::Str(get!(s(0)))
        }
        // ── regular expressions ──
        "REGEXP_REPLACE" => {
            nn!(0, 1);
            let rep = if n >= 3 { get!(s(2)) } else { String::new() };
            let re = get!(regex_of(&get!(s(1))));
            V::Str(re.replace_all(&get!(s(0)), java_replacement(&rep).as_str()).to_string())
        }
        "REGEXP_EXTRACT" => {
            nn!(0, 1);
            let idx = if n >= 3 { get!(k(2)).max(0) as usize } else { 1 };
            let re = get!(regex_of(&get!(s(1))));
            V::Str(re.captures(&get!(s(0))).and_then(|c| c.get(idx).map(|m| m.as_str().to_string())).unwrap_or_default())
        }
        "REGEXP_LIKE" | "RLIKE" | "REGEXP" => { nn!(0, 1); V::Bool(get!(regex_of(&get!(s(1)))).is_match(&get!(s(0)))) }
        "REGEXP_COUNT" => { nn!(0, 1); V::Int(get!(regex_of(&get!(s(1)))).find_iter(&get!(s(0))).count() as i64) }
        "REGEXP_INSTR" => { nn!(0, 1); let st = get!(s(0)); V::Int(get!(regex_of(&get!(s(1)))).find(&st).map(|m| st[..m.start()].chars().count() as i64 + 1).unwrap_or(0)) }
        // ── math ──
        "ABS" => match arg(0) { V::Int(i) => V::Int(i.wrapping_abs()), V::Null => V::Null, o => V::Float(get!(num(o)).abs()) },
        "SIGN" | "SIGNUM" => { nn!(0); let x = get!(f(0)); V::Float(if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { x }) }
        "ROUND" | "BROUND" => {
            nn!(0);
            let sc = if n >= 2 { nn!(1); get!(k(1)) } else { 0 };
            let even = name == "BROUND";
            match arg(0) {
                V::Int(i) => if sc >= 0 { V::Int(*i) } else { V::Int(round_dec(*i as f64, sc, even) as i64) },
                o => { let x = get!(num(o)); V::Float(round_dec(x, sc, even)) }
            }
        }
        "FLOOR" | "CEIL" | "CEILING" => {
            nn!(0);
            if n >= 2 {
                let sc = get!(k(1));
                let x = get!(f(0));
                let m = 10f64.powi(sc as i32);
                return Some(V::Float(if name == "FLOOR" { (x * m).floor() / m } else { (x * m).ceil() / m }));
            }
            match arg(0) {
                V::Int(i) => V::Int(*i),
                o => { let x = get!(num(o)); if x.is_finite() { V::Int(if name == "FLOOR" { x.floor() } else { x.ceil() } as i64) } else { V::Float(x) } }
            }
        }
        "TRUNC" if matches!(arg(0), V::Int(_) | V::Float(_)) => { nn!(0); let sc = if n >= 2 { get!(k(1)) } else { 0 }; let m = 10f64.powi(sc as i32); V::Float((get!(f(0)) * m).trunc() / m) }
        "TRUNCATE" => { nn!(0); let sc = if n >= 2 { get!(k(1)) } else { 0 }; let m = 10f64.powi(sc as i32); V::Float((get!(f(0)) * m).trunc() / m) }
        "SQRT" => math1!(f64::sqrt),
        "CBRT" => math1!(f64::cbrt),
        "EXP" => math1!(f64::exp),
        "EXPM1" => math1!(f64::exp_m1),
        "LN" => { nn!(0); let x = get!(f(0)); if x <= 0.0 { V::Null } else { V::Float(x.ln()) } }
        "LOG" if n == 1 => { nn!(0); let x = get!(f(0)); if x <= 0.0 { V::Null } else { V::Float(x.ln()) } }
        "LOG" => { nn!(0, 1); let (b, x) = (get!(f(0)), get!(f(1))); if b <= 0.0 || x <= 0.0 { V::Null } else { V::Float(x.ln() / b.ln()) } }
        "LOG10" => { nn!(0); let x = get!(f(0)); if x <= 0.0 { V::Null } else { V::Float(x.log10()) } }
        "LOG2" => { nn!(0); let x = get!(f(0)); if x <= 0.0 { V::Null } else { V::Float(x.log2()) } }
        "LOG1P" => math1!(f64::ln_1p),
        "POWER" | "POW" => { nn!(0, 1); V::Float(get!(f(0)).powf(get!(f(1)))) }
        "SIN" => math1!(f64::sin), "COS" => math1!(f64::cos), "TAN" => math1!(f64::tan),
        "ASIN" => math1!(f64::asin), "ACOS" => math1!(f64::acos), "ATAN" => math1!(f64::atan),
        "SINH" => math1!(f64::sinh), "COSH" => math1!(f64::cosh), "TANH" => math1!(f64::tanh),
        "COT" => math1!(|x: f64| 1.0 / x.tan()),
        "DEGREES" => math1!(f64::to_degrees), "RADIANS" => math1!(f64::to_radians),
        "ATAN2" => { nn!(0, 1); V::Float(get!(f(0)).atan2(get!(f(1)))) }
        "HYPOT" => { nn!(0, 1); V::Float(get!(f(0)).hypot(get!(f(1)))) }
        "PI" => V::Float(std::f64::consts::PI),
        "E" => V::Float(std::f64::consts::E),
        "FACTORIAL" => { nn!(0); let x = get!(k(0)); if !(0..=20).contains(&x) { V::Null } else { V::Int((1..=x).product()) } }
        "MOD" | "PMOD" | "REMAINDER" => {
            nn!(0, 1);
            let pm = name == "PMOD";
            match (arg(0), arg(1)) {
                (V::Int(x), V::Int(y)) => if *y == 0 { V::Null } else { let r = x.wrapping_rem(*y); V::Int(if pm && r < 0 { (r + y).wrapping_rem(*y) } else { r }) },
                _ => { let (x, y) = (get!(f(0)), get!(f(1))); if y == 0.0 { V::Null } else { let r = x % y; V::Float(if pm && r < 0.0 { (r + y) % y } else { r }) } }
            }
        }
        "DIV" => { nn!(0, 1); let (x, y) = (get!(f(0)), get!(f(1))); if y == 0.0 { V::Null } else { V::Int((x / y).trunc() as i64) } }
        "TRY_DIVIDE" => { nn!(0, 1); let (x, y) = (get!(f(0)), get!(f(1))); if y == 0.0 { V::Null } else { V::Float(x / y) } }
        "TRY_ADD" | "TRY_SUBTRACT" | "TRY_MULTIPLY" => {
            nn!(0, 1);
            match (arg(0), arg(1)) {
                (V::Int(x), V::Int(y)) => match name { "TRY_ADD" => x.checked_add(*y), "TRY_SUBTRACT" => x.checked_sub(*y), _ => x.checked_mul(*y) }.map(V::Int).unwrap_or(V::Null),
                _ => { let (x, y) = (get!(f(0)), get!(f(1))); V::Float(match name { "TRY_ADD" => x + y, "TRY_SUBTRACT" => x - y, _ => x * y }) }
            }
        }
        "MD5" => { nn!(0); V::Str(crate::hashes::md5(get!(s(0)).as_bytes())) }
        "SHA1" | "SHA" => { nn!(0); V::Str(crate::hashes::sha1(get!(s(0)).as_bytes())) }
        "SHA2" => { nn!(0, 1); match crate::hashes::sha2(get!(s(0)).as_bytes(), get!(k(1))) { Some(h) => V::Str(h), None => V::Null } }
        "CRC32" => { nn!(0); V::Int(crate::hashes::crc32(get!(s(0)).as_bytes()) as i64) }
        "BITAND" => { nn!(0, 1); V::Int(get!(k(0)) & get!(k(1))) }
        "BITOR" => { nn!(0, 1); V::Int(get!(k(0)) | get!(k(1))) }
        "BITXOR" => { nn!(0, 1); V::Int(get!(k(0)) ^ get!(k(1))) }
        "BITNOT" => { nn!(0); V::Int(!get!(k(0))) }
        "SHIFTLEFT" => { nn!(0, 1); V::Int(get!(k(0)).wrapping_shl(get!(k(1)) as u32)) }
        "SHIFTRIGHT" => { nn!(0, 1); V::Int(get!(k(0)).wrapping_shr(get!(k(1)) as u32)) }
        "BIT_COUNT" => { nn!(0); V::Int(get!(k(0)).count_ones() as i64) }
        "WIDTH_BUCKET" => {
            nn!(0, 1, 2, 3);
            let (v, lo, hi, nb) = (get!(f(0)), get!(f(1)), get!(f(2)), get!(k(3)));
            if nb <= 0 || lo == hi { return Some(V::Null); }
            let b = if lo < hi {
                if v < lo { 0 } else if v >= hi { nb + 1 } else { ((v - lo) / (hi - lo) * nb as f64).floor() as i64 + 1 }
            } else if v > lo { 0 } else if v <= hi { nb + 1 } else { ((lo - v) / (lo - hi) * nb as f64).floor() as i64 + 1 };
            V::Int(b)
        }
        "RAND" | "RANDOM" | "UUID" => {
            use std::time::{SystemTime, UNIX_EPOCH};
            use std::sync::atomic::{AtomicU64, Ordering as O};
            static CTR: AtomicU64 = AtomicU64::new(0x9E3779B97F4A7C15);
            let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().subsec_nanos() as u64;
            let mut x = CTR.fetch_add(0x9E3779B97F4A7C15, O::Relaxed) ^ t.wrapping_mul(0xBF58476D1CE4E5B9);
            x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB);
            x ^= x >> 31;
            if name == "UUID" { V::Str(format!("{:08x}-{:04x}-4{:03x}-a{:03x}-{:012x}", x as u32, (x >> 32) as u16, (x >> 48) as u16 & 0xfff, (x >> 20) as u16 & 0xfff, x & 0xffff_ffff_ffff)) }
            else { V::Float((x >> 11) as f64 / (1u64 << 53) as f64) }
        }
        // ── comparison-style ──
        "GREATEST" | "LEAST" => {
            let mut best: Option<&V> = None;
            for v in a {
                if matches!(v, V::Null) { continue; }
                best = match best {
                    None => Some(v),
                    Some(b) => match cmp_vals(v, b) {
                        Some(Ordering::Greater) if name == "GREATEST" => Some(v),
                        Some(Ordering::Less) if name == "LEAST" => Some(v),
                        _ => Some(b),
                    },
                };
            }
            best.cloned().unwrap_or(V::Null)
        }
        "NULLIF" => { if matches!(arg(0), V::Null) { V::Null } else if eq_vals(arg(0), arg(1)) == Some(true) { V::Null } else { arg(0).clone() } }
        "ISNULL" if n == 1 => V::Bool(matches!(arg(0), V::Null)),
        "ISNULL" => if matches!(arg(0), V::Null) { arg(1).clone() } else { arg(0).clone() },
        "ISNOTNULL" => V::Bool(!matches!(arg(0), V::Null)),
        "ISNAN" => V::Bool(matches!(arg(0), V::Float(x) if x.is_nan())),
        "NANVL" => if matches!(arg(0), V::Float(x) if x.is_nan()) { arg(1).clone() } else { arg(0).clone() },
        "TYPEOF" => V::Str(match arg(0) { V::Int(_) => "bigint", V::Float(_) => "double", V::Str(_) => "string", V::Bool(_) => "boolean", V::Null => "void" }.into()),
        "CAST_TO" => V::Null,
        "ASSERT_TRUE" => arg(0).clone(),
        // ── dates and times ──
        "YEAR" | "MONTH" | "DAY" | "DAYOFMONTH" | "DAYOFWEEK" | "DAYOFYEAR" | "QUARTER" | "WEEKOFYEAR" | "HOUR" | "MINUTE" | "SECOND" | "WEEK" => {
            let d = get!(to_dt(arg(0)));
            let field = match name { "WEEKOFYEAR" => "week", "DAYOFMONTH" => "day", o => o };
            get!(date_part(field, &d))
        }
        "WEEKDAY" => V::Int(get!(to_dt(arg(0))).weekday()),
        "EXTRACT" | "DATE_PART" | "DATEPART" => { nn!(0, 1); let d = get!(to_dt(arg(1))); get!(date_part(&get!(s(0)), &d)) }
        "LAST_DAY" => { let d = get!(to_dt(arg(0))); let (y, m, _) = d.ymd(); date_v(Dt { days: dt::days_from_civil(y, m, dt::days_in_month(y, m)), secs: 0, nanos: 0, has_time: false }) }
        "ADD_MONTHS" => { nn!(0, 1); date_v(get!(to_dt(arg(0))).add_months(get!(k(1)))) }
        "DATE_ADD" | "DATEADD" | "TIMESTAMPADD" | "DAYS_ADD" if n == 3 || (n == 2 && name == "DAYS_ADD") => {
            nn!(0, 1, 2);
            let unit = get!(dt::norm_unit(&get!(s(0))));
            let d = get!(to_dt(arg(2)));
            let r = get!(dt::add_unit(&d, unit, get!(k(1))));
            if d.has_time || matches!(unit, "hour" | "minute" | "second") { ts_v(r) } else { date_v(r) }
        }
        "DATE_ADD" | "DAYS_ADD" | "ADDDATE" => { nn!(0, 1); date_v(get!(to_dt(arg(0))).add_days(get!(k(1)))) }
        "DATE_SUB" | "SUBDATE" => { nn!(0, 1); date_v(get!(to_dt(arg(0))).add_days(-get!(k(1)))) }
        "DATEDIFF" | "TIMESTAMPDIFF" | "DATE_DIFF" if n == 3 => {
            nn!(0, 1, 2);
            let unit = get!(dt::norm_unit(&get!(s(0))));
            V::Int(get!(dt::diff_units(unit, &get!(to_dt(arg(1))), &get!(to_dt(arg(2))))))
        }
        "DATEDIFF" | "DATE_DIFF" => { nn!(0, 1); V::Int(get!(to_dt(arg(0))).days - get!(to_dt(arg(1))).days) }
        "MONTHS_BETWEEN" => {
            nn!(0, 1);
            let round = if n >= 3 { matches!(arg(2), V::Bool(true)) } else { true };
            V::Float(dt::months_between(&get!(to_dt(arg(0))), &get!(to_dt(arg(1))), round))
        }
        "NEXT_DAY" => { nn!(0, 1); date_v(get!(dt::next_day(&get!(to_dt(arg(0))), &get!(s(1))))) }
        "TRUNC" => { nn!(0, 1); let d = get!(to_dt(arg(0))); let u = get!(dt::norm_unit(&get!(s(1)))); if matches!(u, "year" | "quarter" | "month" | "week") { date_v(get!(dt::trunc(&d, u))) } else { V::Null } }
        "DATE_TRUNC" => { nn!(0, 1); let u = get!(dt::norm_unit(&get!(s(0)))); let d = get!(to_dt(arg(1))); ts_v(get!(dt::trunc(&d, u))) }
        "MAKE_DATE" => {
            nn!(0, 1, 2);
            let (y, m, d) = (get!(k(0)), get!(k(1)), get!(k(2)));
            if !dt::valid_ymd(y, m, d) { return Some(V::Null); }
            V::Str(dt::fmt_date(dt::days_from_civil(y, m, d)))
        }
        "TO_DATE" | "DATE" => {
            nn!(0);
            if n >= 2 { nn!(1); let d = get!(dt::parse_pattern(&get!(s(0)), &get!(s(1)))); date_v(d) }
            else { date_v(get!(to_dt(arg(0)))) }
        }
        "TO_TIMESTAMP" | "TIMESTAMP" | "TO_TIMESTAMP_NTZ" => {
            nn!(0);
            if n >= 2 { nn!(1); ts_v(get!(dt::parse_pattern(&get!(s(0)), &get!(s(1))))) }
            else { ts_v(get!(to_dt(arg(0)))) }
        }
        "DATE_FORMAT" => { nn!(0, 1); V::Str(dt::format_pattern(&get!(to_dt(arg(0))), &get!(s(1)))) }
        "UNIX_TIMESTAMP" | "TO_UNIX_TIMESTAMP" => {
            if n == 0 { return Some(V::Int(dt::now().epoch_secs())); }
            nn!(0);
            let d = if n >= 2 { get!(dt::parse_pattern(&get!(s(0)), &get!(s(1)))) } else { get!(to_dt(arg(0))) };
            V::Int(d.epoch_secs())
        }
        "FROM_UNIXTIME" => {
            nn!(0);
            let d = Dt::from_epoch_secs(get!(k(0)));
            V::Str(dt::format_pattern(&d, &if n >= 2 { get!(s(1)) } else { "yyyy-MM-dd HH:mm:ss".into() }))
        }
        "CURRENT_DATE" | "CURDATE" | "TODAY" => date_v(dt::now()),
        "NOW" | "CURRENT_TIMESTAMP" | "LOCALTIMESTAMP" | "GETDATE" => { let mut d = dt::now(); d.nanos = 0; ts_v(d) }
        "DAYNAME" => V::Str(dt::DAY_NAMES[get!(to_dt(arg(0))).weekday() as usize].to_string()),
        "MONTHNAME" => V::Str(dt::MONTH_NAMES[(get!(to_dt(arg(0))).ymd().1 - 1) as usize].to_string()),
        // ── casts spelled as functions ──
        "STRING" => cast(arg(0).clone(), "STRING", None, None),
        "INT" | "INTEGER" => cast(arg(0).clone(), "INT", None, None),
        "BIGINT" | "LONG" => cast(arg(0).clone(), "BIGINT", None, None),
        "DOUBLE" | "FLOAT" => cast(arg(0).clone(), "DOUBLE", None, None),
        "BOOLEAN" => cast(arg(0).clone(), "BOOLEAN", None, None),
        _ => return None,
    })
}

/// Names that exist as scalar functions, for "unknown function" detection.
pub fn is_known(name: &str) -> bool {
    is_lazy(name) || matches!(name,
        "CAST" | "TRY_CAST" | "CONVERT" | "MAP" | "UPPER" | "UCASE" | "LOWER" | "LCASE" | "LENGTH" | "CHAR_LENGTH" | "CHARACTER_LENGTH" | "LEN"
        | "OCTET_LENGTH" | "BIT_LENGTH" | "TRIM" | "BTRIM" | "LTRIM" | "RTRIM" | "__TRIM_BOTH" | "__TRIM_LEADING" | "__TRIM_TRAILING"
        | "SUBSTR" | "SUBSTRING" | "MID" | "LEFT" | "RIGHT" | "CONCAT" | "CONCAT_WS" | "REPLACE" | "REPEAT" | "REVERSE" | "LPAD" | "RPAD"
        | "INSTR" | "STRPOS" | "CHARINDEX" | "LOCATE" | "POSITION" | "INITCAP" | "SPACE" | "ASCII" | "ORD" | "CHR" | "CHAR" | "TRANSLATE"
        | "SPLIT_PART" | "SUBSTRING_INDEX" | "OVERLAY" | "STARTSWITH" | "ENDSWITH" | "CONTAINS" | "LEVENSHTEIN" | "FORMAT_STRING" | "PRINTF"
        | "FORMAT_NUMBER" | "BASE64" | "HEX" | "UNHEX" | "TO_CHAR" | "TO_VARCHAR" | "REGEXP_REPLACE" | "REGEXP_EXTRACT" | "REGEXP_LIKE"
        | "RLIKE" | "REGEXP" | "REGEXP_COUNT" | "REGEXP_INSTR" | "ABS" | "SIGN" | "SIGNUM" | "ROUND" | "BROUND" | "FLOOR" | "CEIL" | "CEILING"
        | "TRUNC" | "TRUNCATE" | "SQRT" | "CBRT" | "EXP" | "EXPM1" | "LN" | "LOG" | "LOG10" | "LOG2" | "LOG1P" | "POWER" | "POW" | "SIN" | "COS"
        | "TAN" | "ASIN" | "ACOS" | "ATAN" | "SINH" | "COSH" | "TANH" | "COT" | "DEGREES" | "RADIANS" | "ATAN2" | "HYPOT" | "PI" | "E" | "FACTORIAL"
        | "MD5" | "SHA1" | "SHA" | "SHA2" | "CRC32" | "TRY_DIVIDE" | "TRY_ADD" | "TRY_SUBTRACT" | "TRY_MULTIPLY" | "MOD" | "PMOD" | "REMAINDER" | "DIV" | "BITAND" | "BITOR" | "BITXOR" | "BITNOT" | "SHIFTLEFT" | "SHIFTRIGHT" | "BIT_COUNT" | "WIDTH_BUCKET" | "RAND" | "RANDOM" | "UUID" | "GREATEST" | "LEAST" | "NULLIF" | "ISNULL"
        | "ISNOTNULL" | "ISNAN" | "NANVL" | "TYPEOF" | "ASSERT_TRUE" | "YEAR" | "MONTH" | "DAY" | "DAYOFMONTH" | "DAYOFWEEK" | "DAYOFYEAR"
        | "QUARTER" | "WEEKOFYEAR" | "HOUR" | "MINUTE" | "SECOND" | "WEEK" | "WEEKDAY" | "EXTRACT" | "DATE_PART" | "DATEPART" | "LAST_DAY"
        | "ADD_MONTHS" | "DATE_ADD" | "DATEADD" | "TIMESTAMPADD" | "DAYS_ADD" | "ADDDATE" | "DATE_SUB" | "SUBDATE" | "DATEDIFF" | "TIMESTAMPDIFF"
        | "DATE_DIFF" | "MONTHS_BETWEEN" | "NEXT_DAY" | "DATE_TRUNC" | "MAKE_DATE" | "TO_DATE" | "DATE" | "TO_TIMESTAMP" | "TIMESTAMP"
        | "TO_TIMESTAMP_NTZ" | "DATE_FORMAT" | "UNIX_TIMESTAMP" | "TO_UNIX_TIMESTAMP" | "FROM_UNIXTIME" | "CURRENT_DATE" | "CURDATE" | "TODAY"
        | "NOW" | "CURRENT_TIMESTAMP" | "LOCALTIMESTAMP" | "GETDATE" | "DAYNAME" | "MONTHNAME" | "STRING" | "INT" | "INTEGER" | "BIGINT" | "LONG"
        | "DOUBLE" | "FLOAT" | "BOOLEAN" | "SPLIT" | "STRFTIME" | "FORMAT_DATE" | "EXTRACT_YEAR" | "EXTRACT_MONTH" | "EXTRACT_DAY" | "CHARINDEX_"
        | "ISNUMERIC" | "PROPERCASE" | "POSITION_OF" | "ARRAY" | "EXPLODE")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(x: &str) -> V { V::Str(x.into()) }
    fn st(v: Option<V>) -> String { to_str(&v.unwrap()).unwrap_or_else(|| "NULL".into()) }

    #[test]
    fn rounding_follows_decimal_representation() {
        assert_eq!(round_half_up(1.005, 2), 1.01);
        assert_eq!(round_half_up(2.5, 0), 3.0);
        assert_eq!(round_half_up(-2.5, 0), -3.0);
        assert_eq!(round_half_even(2.5, 0), 2.0);
        assert_eq!(round_half_up(1234.0, -2), 1200.0);
        assert_eq!(round_half_up(0.5, 0), 1.0);
    }
    #[test]
    fn substr_edges() {
        assert_eq!(substr("Hello", -3, None), "llo");
        assert_eq!(substr("Hello", 0, Some(2)), "He");
        assert_eq!(substr("Hello", 2, Some(100)), "ello");
        assert_eq!(substr("Hello", 9, Some(2)), "");
    }
    #[test]
    fn float_text() {
        assert_eq!(fmt_f64(2.0), "2.0");
        assert_eq!(fmt_f64(1.5), "1.5");
        assert_eq!(fmt_f64(0.00001), "1.0E-5");
    }
    #[test]
    fn dates() {
        assert_eq!(st(call("DATE_ADD", &[s("2024-01-31"), V::Int(1)])), "2024-02-01");
        assert_eq!(st(call("ADD_MONTHS", &[s("2024-01-31"), V::Int(1)])), "2024-02-29");
        assert_eq!(st(call("DATEDIFF", &[s("2024-03-01"), s("2024-02-01")])), "29");
        assert_eq!(st(call("DATE_FORMAT", &[s("2024-03-05"), s("yyyy/MM/dd")])), "2024/03/05");
    }
}
