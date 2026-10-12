//! STRUCT values. Like arrays and maps (see `arrays.rs`, `maps.rs`) a struct travels through the engine as marked
//! text: a marker character followed by a JSON list of `[field name, value]` pairs. The top-level
//! `KqlContext::query` renders it the way Spark prints structs (`{1, a}`). Field access is `s.field`
//! (resolved in the evaluator when `s` is a column holding a struct) or the internal `__GETFIELD(expr, 'field')`.

use crate::arrays;
use crate::executor::ExprVal;
use crate::scalar::{fmt_f64, to_str};

type V = ExprVal;

pub const MARK: char = '\u{3}';

pub fn is_struct(v: &V) -> bool { matches!(v, V::Str(s) if s.starts_with(MARK)) }

pub fn encode(fields: &[(String, V)]) -> V {
    arrays::mark_used();
    let list: Vec<serde_json::Value> = fields.iter()
        .map(|(n, v)| serde_json::Value::Array(vec![serde_json::Value::String(n.clone()), arrays::to_json(v)]))
        .collect();
    V::Str(format!("{MARK}{}", serde_json::Value::Array(list)))
}

pub fn decode(v: &V) -> Option<Vec<(String, V)>> {
    let V::Str(s) = v else { return None };
    let body = s.strip_prefix(MARK)?;
    match serde_json::from_str::<serde_json::Value>(body).ok()? {
        serde_json::Value::Array(a) => Some(a.iter().filter_map(|p| match p {
            serde_json::Value::Array(kv) if kv.len() == 2 => Some((kv[0].as_str().unwrap_or("").to_string(), arrays::from_json(&kv[1]))),
            _ => None,
        }).collect()),
        _ => None,
    }
}

/// `{1, a}` as Spark prints a struct.
pub fn render(s: &str) -> String {
    fn show(v: &V) -> String {
        match v {
            V::Null => "null".into(),
            V::Str(s) if s.starts_with(MARK) => render(s),
            V::Str(s) if s.starts_with(arrays::MARK) => arrays::render(s),
            V::Str(s) if s.starts_with(crate::maps::MARK) => crate::maps::render(s),
            V::Float(f) => fmt_f64(*f),
            other => to_str(other).unwrap_or_default(),
        }
    }
    match decode(&V::Str(s.to_string())) {
        Some(items) => format!("{{{}}}", items.iter().map(|(_, v)| show(v)).collect::<Vec<_>>().join(", ")),
        None => s.to_string(),
    }
}

/// Field of a struct value by name (case-insensitive); `Err` when the value is a struct without that field.
pub fn field(v: &V, name: &str) -> Option<V> {
    decode(v)?.into_iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, x)| x)
}

/// Split `s` at top-level commas (not inside `<..>` or `(..)`).
pub fn split_top(s: &str) -> Vec<String> {
    let (mut depth, mut cur, mut out) = (0i32, String::new(), Vec::new());
    for c in s.chars() {
        match c {
            '<' | '(' => { depth += 1; cur.push(c); }
            '>' | ')' => { depth -= 1; cur.push(c); }
            ',' if depth == 0 => { out.push(cur.trim().to_string()); cur.clear(); }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() { out.push(cur.trim().to_string()); }
    out
}

/// Fields of a struct schema: `struct<a:int,b:string>` or the DDL form `a INT, b STRING`.
pub fn parse_schema(schema: &str) -> Option<Vec<(String, String)>> {
    let t = schema.trim();
    let lower = t.to_ascii_lowercase();
    let inner = if lower.starts_with("struct<") && t.ends_with('>') { &t[7..t.len() - 1] } else { t };
    let mut out = Vec::new();
    for f in split_top(inner) {
        let f = f.trim();
        let cut = f.find(|c: char| c == ':' || c.is_whitespace())?;
        let name = f[..cut].trim().trim_matches('`').to_string();
        let ty = f[cut..].trim_start_matches(|c: char| c == ':' || c.is_whitespace()).trim().to_string();
        if name.is_empty() || ty.is_empty() { return None; }
        out.push((name, ty));
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Convert parsed JSON to a value of the declared type (NULL where the JSON does not fit the type).
pub fn json_to_value(j: &serde_json::Value, ty: &str) -> V {
    let ty = ty.trim();
    let lt = ty.to_ascii_lowercase();
    if lt.starts_with("array<") && lt.ends_with('>') {
        let inner = &ty[6..ty.len() - 1];
        return match j { serde_json::Value::Array(a) => arrays::encode(&a.iter().map(|x| json_to_value(x, inner)).collect::<Vec<_>>()), _ => V::Null };
    }
    if lt.starts_with("map<") && lt.ends_with('>') {
        let parts = split_top(&ty[4..ty.len() - 1]);
        let vt = parts.get(1).cloned().unwrap_or_else(|| "string".into());
        return match j {
            serde_json::Value::Object(o) => crate::maps::encode(&o.iter().map(|(k, x)| (V::Str(k.clone()), json_to_value(x, &vt))).collect::<Vec<_>>()),
            _ => V::Null,
        };
    }
    if lt.starts_with("struct<") && lt.ends_with('>') {
        let Some(fields) = parse_schema(ty) else { return V::Null };
        return match j {
            serde_json::Value::Object(o) => encode(&fields.iter().map(|(n, t)| (n.clone(), o.get(n).map(|x| json_to_value(x, t)).unwrap_or(V::Null))).collect::<Vec<_>>()),
            _ => V::Null,
        };
    }
    let base = lt.split('(').next().unwrap_or("").trim();
    match (base, j) {
        (_, serde_json::Value::Null) => V::Null,
        ("int" | "integer" | "bigint" | "long" | "smallint" | "short" | "tinyint" | "byte", serde_json::Value::Number(n)) => n.as_i64().map(V::Int).unwrap_or(V::Null),
        ("double" | "float" | "decimal" | "numeric", serde_json::Value::Number(n)) => n.as_f64().map(V::Float).unwrap_or(V::Null),
        ("boolean", serde_json::Value::Bool(b)) => V::Bool(*b),
        ("string", serde_json::Value::String(s)) => V::Str(s.clone()),
        ("string", other) => V::Str(other.to_string()),
        ("date", serde_json::Value::String(s)) => crate::scalar::cast(V::Str(s.clone()), "DATE", None, None),
        ("timestamp", serde_json::Value::String(s)) => crate::scalar::cast(V::Str(s.clone()), "TIMESTAMP", None, None),
        _ => V::Null,
    }
}

/// JSON text for a value, with nested arrays / maps / structs expanded (struct fields that are NULL are omitted,
/// as Spark's `to_json` does by default).
pub fn value_to_json(v: &V) -> serde_json::Value {
    if let Some(items) = arrays::decode(v) { return serde_json::Value::Array(items.iter().map(value_to_json).collect()); }
    if let Some(pairs) = crate::maps::decode(v) {
        return serde_json::Value::Object(pairs.iter().map(|(k, x)| (to_str(k).unwrap_or_default(), value_to_json(x))).collect());
    }
    if let Some(fields) = decode(v) {
        return serde_json::Value::Object(fields.iter().filter(|(_, x)| !matches!(x, V::Null)).map(|(n, x)| (n.clone(), value_to_json(x))).collect());
    }
    arrays::to_json(v)
}

/// Struct functions; None = not applicable.
pub fn call(name: &str, a: &[V]) -> Option<V> {
    match name {
        "STRUCT" => Some(encode(&a.iter().enumerate().map(|(i, v)| (format!("col{}", i + 1), v.clone())).collect::<Vec<_>>())),
        "NAMED_STRUCT" => {
            if a.len() % 2 != 0 { crate::scalar::set_error("named_struct() needs an even number of arguments (name, value pairs)"); return Some(V::Null); }
            let mut fields = Vec::new();
            for kv in a.chunks(2) {
                let Some(n) = (match &kv[0] { V::Str(s) => Some(s.clone()), _ => None }) else {
                    crate::scalar::set_error("named_struct() field names must be string literals");
                    return Some(V::Null);
                };
                fields.push((n, kv[1].clone()));
            }
            Some(encode(&fields))
        }
        // window(ts, '10 minutes' [, slide [, start offset]]): the tumbling time window of ts as struct<start, end>
        "WINDOW" => {
            let ts = a.first()?;
            if matches!(ts, V::Null) { return Some(V::Null); }
            let (Some(dur), slide, off) = (interval_secs(a.get(1)), a.get(2).map(|x| interval_secs(Some(x))), a.get(3).map(|x| interval_secs(Some(x)))) else {
                crate::scalar::set_error("window(): the duration must be a fixed-length interval string such as '10 minutes'");
                return Some(V::Null);
            };
            if dur <= 0 { crate::scalar::set_error("window(): the duration must be positive"); return Some(V::Null); }
            if let Some(sl) = slide { if sl != Some(dur) { crate::scalar::set_error("window(): sliding windows (slide different from the duration) are not supported"); return Some(V::Null); } }
            let off = match off { None => 0, Some(Some(o)) => o, Some(None) => { crate::scalar::set_error("window(): bad start offset"); return Some(V::Null); } };
            let Some(d) = crate::scalar::to_dt(ts) else { return Some(V::Null) };
            let secs = d.epoch_secs();
            let start = secs - (secs - off).rem_euclid(dur);
            let f = |e: i64| { let t = crate::datetime::Dt::from_epoch_secs(e); V::Str(crate::datetime::fmt_ts(t.days, t.secs, 0)) };
            Some(encode(&[("start".to_string(), f(start)), ("end".to_string(), f(start + dur))]))
        }
        "__GETFIELD" => {
            let (s, f) = (a.first()?, a.get(1)?);
            if matches!(s, V::Null) { return Some(V::Null); }
            let V::Str(fname) = f else { return Some(V::Null) };
            match field(s, fname) {
                Some(v) => Some(v),
                None => { crate::scalar::set_error(format!("no such struct field {fname}")); Some(V::Null) }
            }
        }
        _ => None,
    }
}

/// Seconds of a fixed-length interval string (`10 minutes`, `1 hour 30 minutes`, `interval 2 days`); None for months / years.
fn interval_secs(v: Option<&V>) -> Option<i64> {
    let V::Str(s) = v? else { return None };
    let t = s.trim().to_ascii_lowercase();
    let t = t.strip_prefix("interval").unwrap_or(&t).trim().to_string();
    let words: Vec<&str> = t.split_whitespace().collect();
    if words.is_empty() || words.len() % 2 != 0 { return None; }
    let mut total = 0i64;
    for p in words.chunks(2) {
        let n: i64 = p[0].parse().ok()?;
        let unit = crate::datetime::norm_unit(p[1])?;
        total += n * match unit { "second" => 1, "minute" => 60, "hour" => 3600, "day" => 86400, "week" => 604800, _ => return None };
    }
    Some(total)
}

pub fn names() -> &'static [&'static str] { &["STRUCT", "NAMED_STRUCT", "__GETFIELD", "WINDOW"] }
