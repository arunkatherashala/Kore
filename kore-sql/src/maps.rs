//! MAP values. Like arrays (see `arrays.rs`) a map travels through the engine as marked text: a marker
//! character followed by a JSON list of `[key, value]` pairs. The top-level `KqlContext::query` renders it the
//! way Spark prints maps (`{a -> 1, b -> 2}`).

use crate::arrays;
use crate::executor::ExprVal;
use crate::scalar::{eq_vals, fmt_f64, to_str};

type V = ExprVal;

pub const MARK: char = '\u{2}';

pub fn is_map(v: &V) -> bool { matches!(v, V::Str(s) if s.starts_with(MARK)) }

pub fn encode(pairs: &[(V, V)]) -> V {
    arrays::mark_used();
    // later duplicates replace earlier ones (Spark's LAST_WIN policy)
    let mut out: Vec<(V, V)> = Vec::new();
    for (k, v) in pairs {
        match out.iter().position(|(ok, _)| eq_vals(ok, k) == Some(true)) {
            Some(i) => out[i].1 = v.clone(),
            None => out.push((k.clone(), v.clone())),
        }
    }
    let list: Vec<serde_json::Value> = out.iter().map(|(k, v)| serde_json::Value::Array(vec![arrays::to_json(k), arrays::to_json(v)])).collect();
    V::Str(format!("{MARK}{}", serde_json::Value::Array(list)))
}

pub fn decode(v: &V) -> Option<Vec<(V, V)>> {
    let V::Str(s) = v else { return None };
    let body = s.strip_prefix(MARK)?;
    match serde_json::from_str::<serde_json::Value>(body).ok()? {
        serde_json::Value::Array(a) => Some(a.iter().filter_map(|p| match p {
            serde_json::Value::Array(kv) if kv.len() == 2 => Some((arrays::from_json(&kv[0]), arrays::from_json(&kv[1]))),
            _ => None,
        }).collect()),
        _ => None,
    }
}

/// `{a -> 1, b -> 2}` as Spark prints it.
pub fn render(s: &str) -> String {
    fn show(v: &V) -> String {
        match v {
            V::Null => "null".into(),
            V::Str(s) if s.starts_with(MARK) => render(s),
            V::Str(s) if s.starts_with(arrays::MARK) => arrays::render(s),
            V::Float(f) => fmt_f64(*f),
            other => to_str(other).unwrap_or_default(),
        }
    }
    match decode(&V::Str(s.to_string())) {
        Some(items) => format!("{{{}}}", items.iter().map(|(k, v)| format!("{} -> {}", show(k), show(v))).collect::<Vec<_>>().join(", ")),
        None => s.to_string(),
    }
}

pub fn lookup(pairs: &[(V, V)], key: &V) -> V {
    pairs.iter().find(|(k, _)| eq_vals(k, key) == Some(true)).map(|(_, v)| v.clone()).unwrap_or(V::Null)
}

/// Map functions; None = not applicable (the caller falls through to the array / scalar functions).
pub fn call(name: &str, a: &[V]) -> Option<V> {
    let arg = |i: usize| a.get(i).unwrap_or(&V::Null);
    match name {
        "MAP" => {
            if a.len() % 2 != 0 { crate::scalar::set_error("map() needs an even number of arguments (key, value pairs)"); return Some(V::Null); }
            let mut pairs = Vec::new();
            for kv in a.chunks(2) {
                if matches!(kv[0], V::Null) { crate::scalar::set_error("map keys cannot be NULL"); return Some(V::Null); }
                pairs.push((kv[0].clone(), kv[1].clone()));
            }
            Some(encode(&pairs))
        }
        "MAP_KEYS" => Some(match decode(arg(0)) { Some(p) => arrays::encode(&p.into_iter().map(|(k, _)| k).collect::<Vec<_>>()), None => V::Null }),
        "MAP_VALUES" => Some(match decode(arg(0)) { Some(p) => arrays::encode(&p.into_iter().map(|(_, v)| v).collect::<Vec<_>>()), None => V::Null }),
        "MAP_ENTRIES" => Some(match decode(arg(0)) {
            Some(p) => arrays::encode(&p.into_iter().map(|(k, v)| arrays::encode(&[k, v])).collect::<Vec<_>>()),
            None => V::Null,
        }),
        "MAP_CONTAINS_KEY" => Some(match decode(arg(0)) { Some(p) => V::Bool(p.iter().any(|(k, _)| eq_vals(k, arg(1)) == Some(true))), None => V::Null }),
        "MAP_FROM_ARRAYS" => Some(match (arrays::decode(arg(0)), arrays::decode(arg(1))) {
            (Some(k), Some(v)) if k.len() == v.len() => encode(&k.into_iter().zip(v).collect::<Vec<_>>()),
            (Some(_), Some(_)) => { crate::scalar::set_error("map_from_arrays: the key and value arrays must have the same length"); V::Null }
            _ => V::Null,
        }),
        "MAP_FROM_ENTRIES" => Some(match arrays::decode(arg(0)) {
            Some(es) => {
                let mut pairs = Vec::new();
                for e in es {
                    match arrays::decode(&e) { Some(kv) if kv.len() == 2 => pairs.push((kv[0].clone(), kv[1].clone())), _ => return Some(V::Null) }
                }
                encode(&pairs)
            }
            None => V::Null,
        }),
        "MAP_CONCAT" => {
            let mut all = Vec::new();
            for m in a { match decode(m) { Some(p) => all.extend(p), None => return Some(V::Null) } }
            Some(encode(&all))
        }
        "STR_TO_MAP" => {
            let Some(text) = to_str(arg(0)) else { return Some(V::Null) };
            let pd = if a.len() > 1 { to_str(arg(1)).unwrap_or_else(|| ",".into()) } else { ",".into() };
            let kd = if a.len() > 2 { to_str(arg(2)).unwrap_or_else(|| ":".into()) } else { ":".into() };
            let re = |d: &str| regex::Regex::new(d).ok();
            let (Some(pre), Some(kre)) = (re(&pd), re(&kd)) else { return Some(V::Null) };
            let pairs: Vec<(V, V)> = pre.split(&text).map(|p| {
                let mut it = kre.splitn(p, 2);
                let k = it.next().unwrap_or("").to_string();
                (V::Str(k), it.next().map(|v| V::Str(v.to_string())).unwrap_or(V::Null))
            }).collect();
            Some(encode(&pairs))
        }
        // functions shared with arrays: only when the first argument is a map
        "SIZE" | "CARDINALITY" if is_map(arg(0)) => Some(V::Int(decode(arg(0))?.len() as i64)),
        "ELEMENT_AT" if is_map(arg(0)) => Some(lookup(&decode(arg(0))?, arg(1))),
        "__SUBSCRIPT" if is_map(arg(0)) => Some(lookup(&decode(arg(0))?, arg(1))),
        _ => None,
    }
}

pub fn names() -> &'static [&'static str] {
    &["MAP_KEYS", "MAP_VALUES", "MAP_ENTRIES", "MAP_CONTAINS_KEY", "MAP_FROM_ARRAYS", "MAP_FROM_ENTRIES", "MAP_CONCAT", "STR_TO_MAP", "__SUBSCRIPT"]
}
