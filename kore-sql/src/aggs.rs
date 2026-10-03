//! Row-based aggregate functions over ExprVal columns (used by the general aggregation and window paths).
//! Semantics follow Spark SQL: NULL inputs are skipped, SUM/AVG/MIN/MAX of nothing is NULL, COUNT of nothing is 0.

use std::cmp::Ordering;

use kore_core::KoreError;

use crate::ast::{AggFunc, Expr};
use crate::executor::ExprVal;
use crate::scalar::{cmp_vals, num, to_str};

type V = ExprVal;

/// Argument values of one aggregate call evaluated for every row of the input block.
pub struct AggInput {
    pub name: String,
    /// `args[k][row]`
    pub args: Vec<Vec<V>>,
    pub filter: Option<Vec<bool>>,
    pub distinct: bool,
    /// `COUNT(*)`
    pub star: bool,
}

/// Canonical text key of a value for grouping / DISTINCT (integral floats equal ints, NULLs equal each other).
pub fn key_of(v: &V, out: &mut String) {
    match v {
        V::Null => out.push('n'),
        V::Int(i) => { out.push('i'); out.push_str(&i.to_string()); }
        V::Float(f) => {
            if f.is_nan() { out.push_str("fnan"); }
            else if f.fract() == 0.0 && f.abs() < 9.0e15 { out.push('i'); out.push_str(&(*f as i64).to_string()); }
            else { out.push('f'); out.push_str(&format!("{:?}", f)); }
        }
        V::Str(s) => { out.push('s'); out.push_str(&s.len().to_string()); out.push(':'); out.push_str(s); }
        V::Bool(b) => out.push(if *b { 't' } else { 'F' }),
    }
    out.push('\u{1}');
}

pub fn row_key(vals: &[V]) -> String {
    let mut s = String::new();
    for v in vals { key_of(v, &mut s); }
    s
}

/// Ordering used by MIN / MAX / sorting for non-NULL values; NaN sorts above all numbers.
pub fn total_cmp(a: &V, b: &V) -> Ordering {
    if let (V::Float(x), V::Float(y)) = (a, b) {
        return x.total_cmp(y);
    }
    cmp_vals(a, b).unwrap_or_else(|| {
        // incomparable kinds (string vs number that does not parse): order by type then text
        let rank = |v: &V| match v { V::Bool(_) => 0, V::Int(_) | V::Float(_) => 1, V::Str(_) => 2, V::Null => 3 };
        rank(a).cmp(&rank(b)).then_with(|| to_str(a).cmp(&to_str(b)))
    })
}

pub fn percentile_interp(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() || !(0.0..=1.0).contains(&p) { return None; }
    let pos = (sorted.len() - 1) as f64 * p;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    Some(sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64))
}

fn moments(xs: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    let m2 = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>();
    (mean, m2)
}

/// Aggregate `rows` (indices into the argument vectors).
pub fn aggregate(inp: &AggInput, rows: &[usize]) -> Result<V, KoreError> {
    let name = inp.name.as_str();
    // rows that pass the FILTER clause
    let rows: Vec<usize> = match &inp.filter {
        Some(m) => rows.iter().copied().filter(|&r| m[r]).collect(),
        None => rows.to_vec(),
    };
    let arg = |k: usize, r: usize| -> &V { &inp.args[k][r] };
    let nargs = inp.args.len();

    if name == "COUNT" {
        if inp.star || nargs == 0 {
            return Ok(V::Int(rows.len() as i64));
        }
        // COUNT(a, b): rows where every argument is non-NULL
        let live = rows.iter().copied().filter(|&r| (0..nargs).all(|k| !matches!(arg(k, r), V::Null)));
        return Ok(V::Int(if inp.distinct {
            let mut seen = std::collections::HashSet::new();
            live.filter(|&r| { let vals: Vec<V> = (0..nargs).map(|k| arg(k, r).clone()).collect(); seen.insert(row_key(&vals)) }).count() as i64
        } else { live.count() as i64 }));
    }
    if nargs == 0 {
        return Err(KoreError::InvalidArgument(format!("{name}() needs an argument")));
    }

    // rows with a non-NULL first argument, DISTINCT applied on it
    let mut live: Vec<usize> = rows.iter().copied().filter(|&r| !matches!(arg(0, r), V::Null)).collect();
    if inp.distinct {
        let mut seen = std::collections::HashSet::new();
        live.retain(|&r| seen.insert(row_key(&[arg(0, r).clone()])));
    }
    let floats = |live: &[usize]| -> Vec<f64> { live.iter().filter_map(|&r| num(arg(0, r))).collect() };

    Ok(match name {
        "SUM" => {
            if live.is_empty() { return Ok(V::Null); }
            if live.iter().all(|&r| matches!(arg(0, r), V::Int(_))) {
                V::Int(live.iter().fold(0i64, |a, &r| if let V::Int(i) = arg(0, r) { a.wrapping_add(*i) } else { a }))
            } else {
                let xs = floats(&live);
                if xs.is_empty() { V::Null } else { V::Float(xs.iter().sum()) }
            }
        }
        "AVG" => { let xs = floats(&live); if xs.is_empty() { V::Null } else { V::Float(xs.iter().sum::<f64>() / xs.len() as f64) } }
        "MIN" | "MAX" => {
            let mut best: Option<&V> = None;
            for &r in &live {
                let v = arg(0, r);
                best = match best {
                    None => Some(v),
                    Some(b) => {
                        let o = total_cmp(v, b);
                        if (name == "MIN" && o == Ordering::Less) || (name == "MAX" && o == Ordering::Greater) { Some(v) } else { Some(b) }
                    }
                };
            }
            best.cloned().unwrap_or(V::Null)
        }
        "STDDEV" | "STDDEV_POP" | "VARIANCE" | "VAR_POP" => {
            let xs = floats(&live);
            let pop = name.ends_with("_POP");
            if xs.is_empty() || (!pop && xs.len() < 2) { return Ok(V::Null); }
            let (_, m2) = moments(&xs);
            let var = m2 / if pop { xs.len() as f64 } else { (xs.len() - 1) as f64 };
            V::Float(if name.starts_with("STD") { var.sqrt() } else { var })
        }
        "MEDIAN" | "PERCENTILE" | "PERCENTILE_CONT" | "PERCENTILE_DISC" | "PERCENTILE_APPROX" => {
            let p = if name == "MEDIAN" { 0.5 } else {
                match inp.args.get(1).and_then(|a| a.first()).and_then(num) {
                    Some(p) => p,
                    None => return Err(KoreError::InvalidArgument(format!("{name} needs a percentage"))),
                }
            };
            if !(0.0..=1.0).contains(&p) { return Err(KoreError::InvalidArgument(format!("{name}: percentage must be between 0 and 1"))); }
            let mut xs = floats(&live);
            if xs.is_empty() { return Ok(V::Null); }
            xs.sort_by(|a, b| a.total_cmp(b));
            match name {
                "PERCENTILE_DISC" | "PERCENTILE_APPROX" => {
                    // smallest value whose cumulative share reaches p
                    let idx = ((p * xs.len() as f64).ceil() as usize).clamp(1, xs.len()) - 1;
                    let v = xs[idx];
                    // keep integers integral
                    if live.iter().all(|&r| matches!(arg(0, r), V::Int(_))) { V::Int(v as i64) } else { V::Float(v) }
                }
                _ => V::Float(percentile_interp(&xs, p).unwrap_or(f64::NAN)),
            }
        }
        "STRING_AGG" => {
            if live.is_empty() { return Ok(V::Null); }
            let sep = inp.args.get(1).and_then(|a| a.first()).and_then(to_str).unwrap_or_else(|| ",".into());
            V::Str(live.iter().filter_map(|&r| to_str(arg(0, r))).collect::<Vec<_>>().join(&sep))
        }
        "COLLECT_LIST" | "COLLECT_SET" => {
            let mut items: Vec<String> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for &r in &live {
                if name == "COLLECT_SET" && !seen.insert(row_key(&[arg(0, r).clone()])) { continue; }
                items.push(to_str(arg(0, r)).unwrap_or_default());
            }
            V::Str(format!("[{}]", items.join(",")))
        }
        "FIRST" | "ANY_VALUE" => {
            let ignore = name == "ANY_VALUE" || inp.args.get(1).and_then(|a| a.first()).map_or(false, |v| matches!(v, V::Bool(true)));
            let pool: Vec<usize> = if ignore { live.clone() } else { rows.clone() };
            pool.first().map(|&r| arg(0, r).clone()).unwrap_or(V::Null)
        }
        "LAST" => {
            let ignore = inp.args.get(1).and_then(|a| a.first()).map_or(false, |v| matches!(v, V::Bool(true)));
            let pool: &Vec<usize> = if ignore { &live } else { &rows };
            pool.last().map(|&r| arg(0, r).clone()).unwrap_or(V::Null)
        }
        "MODE" => {
            // most frequent value; ties go to the one seen first
            let mut counts: Vec<(V, usize)> = Vec::new();
            let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for &r in &live {
                let k = row_key(&[arg(0, r).clone()]);
                match index.get(&k) {
                    Some(&i) => counts[i].1 += 1,
                    None => { index.insert(k, counts.len()); counts.push((arg(0, r).clone(), 1)); }
                }
            }
            let mut best: Option<&(V, usize)> = None;
            for c in &counts { if best.map_or(true, |b| c.1 > b.1) { best = Some(c); } }
            best.map(|b| b.0.clone()).unwrap_or(V::Null)
        }
        "COUNT_IF" => V::Int(rows.iter().filter(|&&r| matches!(arg(0, r), V::Bool(true))).count() as i64),
        "BOOL_AND" | "BOOL_OR" => {
            let bs: Vec<bool> = live.iter().filter_map(|&r| match arg(0, r) { V::Bool(b) => Some(*b), V::Int(i) => Some(*i != 0), _ => None }).collect();
            if bs.is_empty() { V::Null } else if name == "BOOL_AND" { V::Bool(bs.iter().all(|b| *b)) } else { V::Bool(bs.iter().any(|b| *b)) }
        }
        "MAX_BY" | "MIN_BY" => {
            let mut best: Option<usize> = None;
            for &r in &rows {
                if matches!(arg(1, r), V::Null) { continue; }
                best = match best {
                    None => Some(r),
                    Some(b) => {
                        let o = total_cmp(arg(1, r), arg(1, b));
                        if (name == "MAX_BY" && o == Ordering::Greater) || (name == "MIN_BY" && o == Ordering::Less) { Some(r) } else { Some(b) }
                    }
                };
            }
            best.map(|r| arg(0, r).clone()).unwrap_or(V::Null)
        }
        "APPROX_COUNT_DISTINCT" => {
            let mut seen = std::collections::HashSet::new();
            V::Int(live.iter().filter(|&&r| seen.insert(row_key(&[arg(0, r).clone()]))).count() as i64)
        }
        "CORR" | "COVAR_POP" | "COVAR_SAMP" => {
            if nargs < 2 { return Err(KoreError::InvalidArgument(format!("{name} needs two arguments"))); }
            let pairs: Vec<(f64, f64)> = rows.iter().filter_map(|&r| Some((num(arg(0, r))?, num(arg(1, r))?))).collect();
            let n = pairs.len() as f64;
            if pairs.is_empty() || (name == "COVAR_SAMP" && pairs.len() < 2) { return Ok(V::Null); }
            let (mx, my) = (pairs.iter().map(|p| p.0).sum::<f64>() / n, pairs.iter().map(|p| p.1).sum::<f64>() / n);
            let cov = pairs.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum::<f64>();
            match name {
                "COVAR_POP" => V::Float(cov / n),
                "COVAR_SAMP" => V::Float(cov / (n - 1.0)),
                _ => {
                    let (sx, sy) = (pairs.iter().map(|p| (p.0 - mx).powi(2)).sum::<f64>(), pairs.iter().map(|p| (p.1 - my).powi(2)).sum::<f64>());
                    if sx == 0.0 || sy == 0.0 { V::Null } else { V::Float(cov / (sx * sy).sqrt()) }
                }
            }
        }
        "SKEWNESS" | "KURTOSIS" => {
            let xs = floats(&live);
            if xs.is_empty() { return Ok(V::Null); }
            let n = xs.len() as f64;
            let (mean, m2s) = moments(&xs);
            let m2 = m2s / n;
            if m2 == 0.0 { return Ok(V::Null); }
            if name == "SKEWNESS" {
                let m3 = xs.iter().map(|x| (x - mean).powi(3)).sum::<f64>() / n;
                V::Float(m3 / m2.powf(1.5))
            } else {
                let m4 = xs.iter().map(|x| (x - mean).powi(4)).sum::<f64>() / n;
                V::Float(m4 / (m2 * m2) - 3.0)
            }
        }
        other => return Err(KoreError::InvalidArgument(format!("unsupported aggregate function: {other}"))),
    })
}

/// Name an aggregate expression is known by (classic aggregates map onto the same implementations).
pub fn agg_parts(e: &Expr) -> Option<(String, Vec<Expr>, bool, Option<Expr>)> {
    match e {
        Expr::AggX { name, args, distinct, filter } => Some((name.clone(), args.clone(), *distinct, filter.as_deref().cloned())),
        Expr::Agg { func, expr } => {
            let (name, distinct) = match func {
                AggFunc::Count => ("COUNT", false),
                AggFunc::CountDistinct => ("COUNT", true),
                AggFunc::Sum => ("SUM", false),
                AggFunc::Avg => ("AVG", false),
                AggFunc::Min => ("MIN", false),
                AggFunc::Max => ("MAX", false),
                AggFunc::Stddev => ("STDDEV", false),
                AggFunc::Variance => ("VARIANCE", false),
                AggFunc::Median => ("MEDIAN", false),
                AggFunc::StringAgg { .. } => ("STRING_AGG", false),
                AggFunc::Percentile { .. } => ("PERCENTILE", false),
            };
            let mut args = vec![expr.as_ref().clone()];
            match func {
                AggFunc::StringAgg { sep } => args.push(Expr::Str(sep.clone())),
                AggFunc::Percentile { p } => args.push(Expr::Float(p.parse().unwrap_or(0.5))),
                _ => {}
            }
            Some((name.to_string(), args, distinct, None))
        }
        _ => None,
    }
}

/// Evaluate the argument and FILTER expressions of an aggregate for every row of the input.
pub fn prepare(e: &Expr, n: usize, ev: &mut dyn FnMut(&Expr, usize) -> V) -> Option<AggInput> {
    let (name, args, distinct, filter) = agg_parts(e)?;
    let star = args.first().map_or(false, |a| matches!(a, Expr::Col(c) if c == "*") || matches!(a, Expr::Star));
    let mut vals: Vec<Vec<V>> = Vec::new();
    if !star {
        for a in &args {
            // constant arguments (percentage, separator) are evaluated once and repeated cheaply
            if matches!(a, Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Null) {
                let v = ev(a, 0);
                vals.push(vec![v; n.max(1)]);
            } else {
                vals.push((0..n).map(|r| ev(a, r)).collect());
            }
        }
    }
    let filter = filter.map(|f| (0..n).map(|r| matches!(ev(&f, r), V::Bool(true))).collect());
    Some(AggInput { name, args: vals, filter, distinct, star })
}
