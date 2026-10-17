//! Grouping sets (ROLLUP / CUBE / GROUPING SETS) in one pass: the finest grouping is computed once over the
//! input rows; every coarser set is derived from the finest groups by merging partial aggregates (COUNT, SUM,
//! MIN, MAX, AVG). Aggregates that cannot be merged (DISTINCT, statistics, ordered ones, ...) are computed per
//! set over the rows of the merged finest groups, still without hashing the input again.

use std::collections::HashMap;

use kore_core::{Column, ColumnData, KoreError};

use crate::aggs::{aggregate, total_cmp, AggInput};
use crate::executor::ExprVal;
use crate::scalar::num;

type V = ExprVal;

/// Result of one grouping set: for every output group a representative input row (first row of the group, the
/// source of the key values) and the aggregate values `aggs[agg][group]`.
pub struct SetResult {
    pub rep_rows: Vec<usize>,
    pub aggs: Vec<Vec<V>>,
}

#[derive(Hash, PartialEq, Eq)]
enum K<'a> { Null, Num(i64), Fl(u64), Nan, Str(&'a str), B(bool) }

fn fkey<'a>(f: f64) -> K<'a> {
    if f.is_nan() { K::Nan } else if f.fract() == 0.0 && f.abs() < 9.0e15 { K::Num(f as i64) } else { K::Fl(f.to_bits()) }
}

/// A grouping key column: evaluated values, or a column of the input read in place (no copy of text values).
pub enum KeySrc<'a> { Vals(&'a [V]), Col(&'a Column) }

impl KeySrc<'_> {
    pub fn value_at(&self, r: usize) -> V {
        match self {
            KeySrc::Vals(v) => v[r].clone(),
            KeySrc::Col(c) => match &c.data {
                ColumnData::Int64(v) => v[r].map_or(V::Null, V::Int),
                ColumnData::Float64(v) => v[r].map_or(V::Null, V::Float),
                ColumnData::Bool(v) => v[r].map_or(V::Null, V::Bool),
                ColumnData::Str(v) => v[r].as_ref().map_or(V::Null, |s| V::Str(s.clone())),
                ColumnData::StrDict { codes, dict } => if codes[r] == u8::MAX { V::Null } else { dict.get(codes[r] as usize).map_or(V::Null, |s| V::Str(s.clone())) },
            },
        }
    }

    /// Dense ids (order of first appearance) of the values, equal where `aggs::key_of` would be equal.
    fn ids(&self) -> Vec<u32> {
        fn run<'a>(it: impl Iterator<Item = K<'a>>, n: usize) -> Vec<u32> {
            let mut map: HashMap<K, u32> = HashMap::new();
            let mut out = Vec::with_capacity(n);
            for k in it {
                let next = map.len() as u32;
                out.push(*map.entry(k).or_insert(next));
            }
            out
        }
        match self {
            KeySrc::Vals(vals) => run(vals.iter().map(|v| match v {
                V::Null => K::Null,
                V::Int(i) => K::Num(*i),
                V::Float(f) => fkey(*f),
                V::Str(s) => K::Str(s.as_str()),
                V::Bool(b) => K::B(*b),
            }), vals.len()),
            KeySrc::Col(c) => match &c.data {
                ColumnData::Int64(v) => run(v.iter().map(|x| x.map_or(K::Null, K::Num)), v.len()),
                ColumnData::Float64(v) => run(v.iter().map(|x| x.map_or(K::Null, fkey)), v.len()),
                ColumnData::Bool(v) => run(v.iter().map(|x| x.map_or(K::Null, K::B)), v.len()),
                ColumnData::Str(v) => run(v.iter().map(|x| x.as_deref().map_or(K::Null, K::Str)), v.len()),
                ColumnData::StrDict { codes, dict } => run(codes.iter().map(|&c| if c == u8::MAX { K::Null } else { dict.get(c as usize).map_or(K::Null, |s| K::Str(s.as_str())) }), codes.len()),
            },
        }
    }
}

/// Combine id columns into group ids (order of first appearance) over `n` items; returns (ids, group count).
fn combine(cols: &[&[u32]], n: usize) -> (Vec<u32>, usize) {
    if cols.is_empty() { return (vec![0; n], if n > 0 { 1 } else { 0 }); }
    let mut cur: Vec<u32> = cols[0].to_vec();
    // ids of one column are already dense and in order of first appearance
    let mut count = cur.iter().copied().max().map_or(0, |m| m as usize + 1);
    for c in &cols[1..] {
        let mut map: HashMap<u64, u32> = HashMap::new();
        for i in 0..n {
            let key = ((cur[i] as u64) << 32) | c[i] as u64;
            let next = map.len() as u32;
            cur[i] = *map.entry(key).or_insert(next);
        }
        count = map.len();
    }
    (cur, count)
}

#[derive(Clone, Copy, PartialEq)]
enum Plan { Count, Sum, MinMax(bool), Avg, Fallback }

fn plan_of(inp: &AggInput) -> Plan {
    if inp.distinct && inp.name != "MIN" && inp.name != "MAX" { return Plan::Fallback; }
    match inp.name.as_str() {
        "COUNT" => Plan::Count,
        "SUM" if inp.args.len() == 1 => Plan::Sum,
        "AVG" if inp.args.len() == 1 => Plan::Avg,
        "MIN" if inp.args.len() == 1 => Plan::MinMax(true),
        "MAX" if inp.args.len() == 1 => Plan::MinMax(false),
        _ => Plan::Fallback,
    }
}

/// Partial state of one aggregate over one group of rows.
enum Part { V(V), Avg(f64, i64) }

fn avg_part(inp: &AggInput, rows: &[usize]) -> Part {
    let (mut s, mut c) = (0.0f64, 0i64);
    for &r in rows {
        if let Some(f) = &inp.filter { if !f[r] { continue; } }
        let v = &inp.args[0][r];
        if matches!(v, V::Null) { continue; }
        if let Some(x) = num(v) { s += x; c += 1; }
    }
    Part::Avg(s, c)
}

fn merge(plan: Plan, parts: &[&Part]) -> V {
    match plan {
        Plan::Count => V::Int(parts.iter().map(|p| match p { Part::V(V::Int(i)) => *i, _ => 0 }).fold(0i64, |a, b| a.wrapping_add(b))),
        Plan::Sum => {
            let mut int_acc: Option<i64> = None;
            let mut fl_acc: Option<f64> = None;
            for p in parts {
                match p {
                    Part::V(V::Int(i)) => {
                        if let Some(f) = fl_acc.as_mut() { *f += *i as f64; } else { int_acc = Some(int_acc.unwrap_or(0).wrapping_add(*i)); }
                    }
                    Part::V(V::Float(x)) => {
                        let base = fl_acc.unwrap_or_else(|| int_acc.take().map_or(0.0, |i| i as f64));
                        fl_acc = Some(base + *x);
                    }
                    _ => {}
                }
            }
            match (int_acc, fl_acc) { (_, Some(f)) => V::Float(f), (Some(i), None) => V::Int(i), _ => V::Null }
        }
        Plan::MinMax(min) => {
            let mut best: Option<&V> = None;
            for p in parts {
                let Part::V(v) = p else { continue };
                if matches!(v, V::Null) { continue; }
                best = match best {
                    None => Some(v),
                    Some(b) => {
                        let o = total_cmp(v, b);
                        if (min && o == std::cmp::Ordering::Less) || (!min && o == std::cmp::Ordering::Greater) { Some(v) } else { Some(b) }
                    }
                };
            }
            best.cloned().unwrap_or(V::Null)
        }
        Plan::Avg => {
            let (mut s, mut c) = (0.0f64, 0i64);
            for p in parts { if let Part::Avg(x, n) = p { s += *x; c += *n; } }
            if c == 0 { V::Null } else { V::Float(s / c as f64) }
        }
        Plan::Fallback => V::Null,
    }
}

/// Aggregate every grouping set of `sets` (index lists into `key_vals`) over `n_in` input rows.
pub fn grouping_sets(keys: &[KeySrc], agg_inputs: &[AggInput], sets: &[Vec<usize>], n_in: usize) -> Result<Vec<SetResult>, KoreError> {
    // finest grouping: every key any set uses
    let mut used: Vec<usize> = sets.iter().flatten().copied().collect();
    used.sort_unstable();
    used.dedup();
    let __t = std::time::Instant::now();
    let ids: Vec<Vec<u32>> = if n_in >= 50_000 && used.len() > 1 {
        use rayon::prelude::*;
        used.par_iter().map(|&k| keys[k].ids()).collect()
    } else {
        used.iter().map(|&k| keys[k].ids()).collect()
    };
    let prof = crate::executor::prof_on();
    if prof { eprintln!("[prof] {:>8.1} ms    rollup key ids", __t.elapsed().as_secs_f64() * 1000.0); }
    let id_refs: Vec<&[u32]> = ids.iter().map(|v| v.as_slice()).collect();
    let (gid, g_count) = combine(&id_refs, n_in);

    if prof { eprintln!("[prof] {:>8.1} ms    rollup finest groups={}", __t.elapsed().as_secs_f64() * 1000.0, g_count); }
    let mut rows_of: Vec<Vec<usize>> = vec![Vec::new(); g_count];
    for (r, &g) in gid.iter().enumerate() { rows_of[g as usize].push(r); }
    // first row and per-key ids of every finest group
    let first_row: Vec<usize> = rows_of.iter().map(|v| v[0]).collect();
    let g_ids: Vec<Vec<u32>> = ids.iter().map(|c| first_row.iter().map(|&r| c[r]).collect()).collect();

    if prof { eprintln!("[prof] {:>8.1} ms    rollup rows_of", __t.elapsed().as_secs_f64() * 1000.0); }
    let plans: Vec<Plan> = agg_inputs.iter().map(plan_of).collect();
    // partials of the mergeable aggregates per finest group
    let mut partials: Vec<Vec<Part>> = Vec::with_capacity(agg_inputs.len());
    for (inp, &pl) in agg_inputs.iter().zip(&plans) {
        partials.push(match pl {
            Plan::Fallback => Vec::new(),
            Plan::Avg => rows_of.iter().map(|rows| avg_part(inp, rows)).collect(),
            _ => rows_of.iter().map(|rows| aggregate(inp, rows).map(Part::V)).collect::<Result<_, _>>()?,
        });
    }

    if prof { eprintln!("[prof] {:>8.1} ms    rollup finest partials", __t.elapsed().as_secs_f64() * 1000.0); }
    let mut out = Vec::with_capacity(sets.len());
    for set in sets {
        // coarse group of every finest group
        let (cid, c_count) = if set.is_empty() {
            (vec![0u32; g_count], 1usize)
        } else {
            let cols: Vec<&[u32]> = set.iter().map(|k| g_ids[used.binary_search(k).unwrap()].as_slice()).collect();
            combine(&cols, g_count)
        };
        let mut members: Vec<Vec<usize>> = vec![Vec::new(); c_count];
        for (g, &c) in cid.iter().enumerate() { members[c as usize].push(g); }
        let rep_rows: Vec<usize> = members.iter().map(|m| m.first().map_or(0, |&g| first_row[g])).collect();
        let mut aggs: Vec<Vec<V>> = Vec::with_capacity(agg_inputs.len());
        for (j, inp) in agg_inputs.iter().enumerate() {
            let mut col = Vec::with_capacity(c_count);
            for m in &members {
                if plans[j] == Plan::Fallback {
                    let mut rows: Vec<usize> = m.iter().flat_map(|&g| rows_of[g].iter().copied()).collect();
                    if m.len() > 1 { rows.sort_unstable(); }
                    col.push(aggregate(inp, &rows)?);
                } else {
                    let ps: Vec<&Part> = m.iter().map(|&g| &partials[j][g]).collect();
                    col.push(merge(plans[j], &ps));
                }
            }
            aggs.push(col);
        }
        out.push(SetResult { rep_rows, aggs });
    }
    Ok(out)
}
