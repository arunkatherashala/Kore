//! ARRAY values. The engine has no array column type, so an array travels through the query pipeline as text:
//! a marker character followed by the elements as a JSON list. Scalar functions recognise the marker, the
//! top-level `KqlContext::query` renders it the way Spark prints arrays (`[a, b, c]`).

use std::cell::Cell;

use crate::executor::ExprVal;
use crate::scalar::{cmp_vals, eq_vals, fmt_f64, to_str};

type V = ExprVal;

pub const MARK: char = '\u{1}';

thread_local! {
    static USED: Cell<bool> = Cell::new(false);
}

/// Did the current statement create any array? (lets the final rendering step be skipped otherwise)
pub fn take_used() -> bool { USED.with(|u| u.replace(false)) }

fn to_json(v: &V) -> serde_json::Value {
    match v {
        V::Null => serde_json::Value::Null,
        V::Int(i) => serde_json::Value::from(*i),
        V::Float(f) => serde_json::Number::from_f64(*f).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null),
        V::Bool(b) => serde_json::Value::Bool(*b),
        V::Str(s) => serde_json::Value::String(s.clone()),
    }
}

fn from_json(j: &serde_json::Value) -> V {
    match j {
        serde_json::Value::Null => V::Null,
        serde_json::Value::Bool(b) => V::Bool(*b),
        serde_json::Value::Number(n) => match n.as_i64() { Some(i) => V::Int(i), None => V::Float(n.as_f64().unwrap_or(f64::NAN)) },
        serde_json::Value::String(s) => V::Str(s.clone()),
        other => V::Str(other.to_string()),
    }
}

pub fn encode(items: &[V]) -> V {
    USED.with(|u| u.set(true));
    // elements share one type, as in Spark: 1 and 2.5 become 1.0 and 2.5, numbers next to text become text
    let (mut has_float, mut has_int, mut has_str, mut has_bool) = (false, false, false, false);
    for v in items {
        match v { V::Float(_) => has_float = true, V::Int(_) => has_int = true, V::Str(_) => has_str = true, V::Bool(_) => has_bool = true, V::Null => {} }
    }
    let list: Vec<serde_json::Value> = if has_str && (has_float || has_int || has_bool) {
        items.iter().map(|v| match v { V::Null => serde_json::Value::Null, o => serde_json::Value::String(to_str(o).unwrap_or_default()) }).collect()
    } else if has_float && has_int {
        items.iter().map(|v| match v { V::Int(i) => to_json(&V::Float(*i as f64)), o => to_json(o) }).collect()
    } else {
        items.iter().map(to_json).collect()
    };
    V::Str(format!("{MARK}{}", serde_json::Value::Array(list)))
}

pub fn is_array(v: &V) -> bool { matches!(v, V::Str(s) if s.starts_with(MARK)) }

pub fn decode(v: &V) -> Option<Vec<V>> {
    let V::Str(s) = v else { return None };
    let body = s.strip_prefix(MARK)?;
    match serde_json::from_str::<serde_json::Value>(body).ok()? {
        serde_json::Value::Array(a) => Some(a.iter().map(from_json).collect()),
        _ => None,
    }
}

/// `[a, b, c]` as Spark prints it.
pub fn render(s: &str) -> String {
    fn show(v: &V) -> String {
        match v {
            V::Null => "null".into(),
            V::Str(s) if s.starts_with(MARK) => render(s),
            V::Float(f) => fmt_f64(*f),
            other => to_str(other).unwrap_or_default(),
        }
    }
    match decode(&V::Str(s.to_string())) {
        Some(items) => format!("[{}]", items.iter().map(show).collect::<Vec<_>>().join(", ")),
        None => s.to_string(),
    }
}

fn total(a: &V, b: &V) -> std::cmp::Ordering { crate::aggs::total_cmp(a, b) }

fn dedupe(items: Vec<V>) -> Vec<V> {
    let mut seen = std::collections::HashSet::new();
    items.into_iter().filter(|v| seen.insert(crate::aggs::row_key(&[v.clone()]))).collect()
}

/// Array functions. None = not an array function.
pub fn call(name: &str, a: &[V]) -> Option<V> {
    let arg = |i: usize| a.get(i).unwrap_or(&V::Null);
    let arr = |i: usize| decode(arg(i));
    macro_rules! need { ($e:expr) => { match $e { Some(v) => v, None => return Some(V::Null) } } }
    Some(match name {
        "ARRAY" => encode(a),
        "SIZE" | "CARDINALITY" | "ARRAY_SIZE" => match arr(0) { Some(x) => V::Int(x.len() as i64), None => if matches!(arg(0), V::Null) { V::Int(-1) } else { V::Null } },
        "ELEMENT_AT" => {
            let items = need!(arr(0));
            let i = need!(crate::scalar::num(arg(1))) as i64;
            let n = items.len() as i64;
            let pos = if i > 0 { i - 1 } else if i < 0 { n + i } else { return Some(V::Null) };
            if pos < 0 || pos >= n { V::Null } else { items[pos as usize].clone() }
        }
        "ARRAY_CONTAINS" => {
            let items = need!(arr(0));
            if matches!(arg(1), V::Null) { return Some(V::Null); }
            let mut unknown = false;
            for it in &items {
                match eq_vals(it, arg(1)) { Some(true) => return Some(V::Bool(true)), None => unknown = true, _ => {} }
            }
            if unknown { V::Null } else { V::Bool(false) }
        }
        "ARRAY_POSITION" => {
            let items = need!(arr(0));
            V::Int(items.iter().position(|it| eq_vals(it, arg(1)) == Some(true)).map(|p| p as i64 + 1).unwrap_or(0))
        }
        "ARRAY_JOIN" => {
            let items = need!(arr(0));
            let sep = need!(to_str(arg(1)));
            let repl = if a.len() > 2 { to_str(arg(2)) } else { None };
            V::Str(items.iter().filter_map(|v| match v { V::Null => repl.clone(), o => to_str(o) }).collect::<Vec<_>>().join(&sep))
        }
        "SORT_ARRAY" | "ARRAY_SORT" => {
            let mut items = need!(arr(0));
            let asc = !matches!(arg(1), V::Bool(false));
            items.sort_by(|x, y| match (x, y) {
                (V::Null, V::Null) => std::cmp::Ordering::Equal,
                (V::Null, _) => if asc { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater },
                (_, V::Null) => if asc { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Less },
                _ => { let o = total(x, y); if asc { o } else { o.reverse() } }
            });
            encode(&items)
        }
        "ARRAY_DISTINCT" => encode(&dedupe(need!(arr(0)))),
        "ARRAY_UNION" => { let mut x = need!(arr(0)); x.extend(need!(arr(1))); encode(&dedupe(x)) }
        "ARRAY_INTERSECT" => {
            let (x, y) = (need!(arr(0)), need!(arr(1)));
            let ys: std::collections::HashSet<String> = y.iter().map(|v| crate::aggs::row_key(&[v.clone()])).collect();
            encode(&dedupe(x.into_iter().filter(|v| ys.contains(&crate::aggs::row_key(&[v.clone()]))).collect()))
        }
        "ARRAY_EXCEPT" => {
            let (x, y) = (need!(arr(0)), need!(arr(1)));
            let ys: std::collections::HashSet<String> = y.iter().map(|v| crate::aggs::row_key(&[v.clone()])).collect();
            encode(&dedupe(x.into_iter().filter(|v| !ys.contains(&crate::aggs::row_key(&[v.clone()]))).collect()))
        }
        "ARRAYS_OVERLAP" => {
            let (x, y) = (need!(arr(0)), need!(arr(1)));
            V::Bool(x.iter().any(|p| !matches!(p, V::Null) && y.iter().any(|q| eq_vals(p, q) == Some(true))))
        }
        "ARRAY_MAX" | "ARRAY_MIN" => {
            let items = need!(arr(0));
            let mut best: Option<V> = None;
            for it in items.into_iter().filter(|v| !matches!(v, V::Null)) {
                best = match best {
                    None => Some(it),
                    Some(b) => { let o = cmp_vals(&it, &b).unwrap_or(std::cmp::Ordering::Equal); if (name == "ARRAY_MAX" && o.is_gt()) || (name == "ARRAY_MIN" && o.is_lt()) { Some(it) } else { Some(b) } }
                };
            }
            best.unwrap_or(V::Null)
        }
        "ARRAY_REMOVE" => { let items = need!(arr(0)); encode(&items.into_iter().filter(|v| eq_vals(v, arg(1)) != Some(true)).collect::<Vec<_>>()) }
        "ARRAY_COMPACT" => { let items = need!(arr(0)); encode(&items.into_iter().filter(|v| !matches!(v, V::Null)).collect::<Vec<_>>()) }
        "ARRAY_APPEND" => { let mut x = need!(arr(0)); x.push(arg(1).clone()); encode(&x) }
        "ARRAY_PREPEND" => { let mut x = need!(arr(0)); x.insert(0, arg(1).clone()); encode(&x) }
        "ARRAY_REPEAT" => { let n = need!(crate::scalar::num(arg(1))).max(0.0) as usize; encode(&vec![arg(0).clone(); n.min(1_000_000)]) }
        "SLICE" => {
            let items = need!(arr(0));
            let (start, len) = (need!(crate::scalar::num(arg(1))) as i64, need!(crate::scalar::num(arg(2))) as i64);
            if start == 0 || len < 0 { return Some(V::Null); }
            let n = items.len() as i64;
            let from = if start > 0 { start - 1 } else { n + start };
            if from < 0 || from >= n { encode(&[]) } else { encode(&items[from as usize..((from + len).min(n)) as usize]) }
        }
        "FLATTEN" => {
            let items = need!(arr(0));
            let mut out = Vec::new();
            for it in items {
                match decode(&it) { Some(inner) => out.extend(inner), None => return Some(V::Null) }
            }
            encode(&out)
        }
        "REVERSE" if is_array(arg(0)) => { let mut x = need!(arr(0)); x.reverse(); encode(&x) }
        "SEQUENCE" => return None, // needs lazy evaluation of the optional INTERVAL step (see the executor)
        "STRING_TO_ARRAY" => return None,
        _ => return None,
    })
}

pub fn names() -> &'static [&'static str] {
    &["ARRAY", "SIZE", "CARDINALITY", "ARRAY_SIZE", "ELEMENT_AT", "ARRAY_CONTAINS", "ARRAY_POSITION", "ARRAY_JOIN", "SORT_ARRAY", "ARRAY_SORT",
      "ARRAY_DISTINCT", "ARRAY_UNION", "ARRAY_INTERSECT", "ARRAY_EXCEPT", "ARRAYS_OVERLAP", "ARRAY_MAX", "ARRAY_MIN", "ARRAY_REMOVE",
      "ARRAY_COMPACT", "ARRAY_APPEND", "ARRAY_PREPEND", "ARRAY_REPEAT", "SLICE", "FLATTEN"]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_render() {
        let a = encode(&[V::Int(1), V::Str("x".into()), V::Null, V::Float(2.0)]);
        assert!(is_array(&a));
        assert_eq!(decode(&a).unwrap().len(), 4);
        if let V::Str(s) = &a { assert_eq!(render(s), "[1, x, null, 2.0]"); }
    }
    #[test]
    fn functions() {
        let a = encode(&[V::Int(3), V::Int(1), V::Int(2), V::Int(1)]);
        assert!(matches!(call("SIZE", &[a.clone()]), Some(V::Int(4))));
        let sorted = call("SORT_ARRAY", &[a.clone()]).unwrap();
        assert_eq!(decode(&sorted).unwrap().len(), 4);
        assert!(matches!(call("ARRAY_JOIN", &[call("ARRAY_DISTINCT", &[a.clone()]).unwrap(), V::Str("-".into())]), Some(V::Str(ref t)) if t == "3-1-2"));
        assert!(matches!(call("ELEMENT_AT", &[a.clone(), V::Int(-1)]), Some(V::Int(1))));
    }
}
