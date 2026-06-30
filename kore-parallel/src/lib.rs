//! KORE Layer 35 — Parallel Query Execution
//!
//! Provides Rayon-powered parallel versions of the core operations:
//! - Parallel WHERE filter (predicate evaluation across partitions)
//! - Parallel column aggregation (SUM, COUNT, MIN, MAX, AVG)
//! - Parallel sort (parallel merge-sort via Rayon)
//! - Parallel hash-group-by (concurrent bucket accumulation)
//! - Parallel map (apply a closure to every column element)

use rayon::prelude::*;
use kore_core::{Column, ColumnData, DataBlock, KoreError, Value};

// ─── Parallel filter ──────────────────────────────────────────────────────────

/// Filter rows in parallel using a predicate closure.
/// The predicate receives a row index and all column values for that row.
pub fn par_filter<F>(block: &DataBlock, pred: F) -> DataBlock
where
    F: Fn(usize) -> bool + Sync + Send,
{
    let n = block.num_rows;
    let keep: Vec<bool> = (0..n).into_par_iter().map(|r| pred(r)).collect();
    let indices: Vec<usize> = keep.iter().enumerate()
        .filter_map(|(i, &k)| if k { Some(i) } else { None })
        .collect();
    block.select_rows(&indices)
}

// ─── Parallel aggregation ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum ParAggOp { Sum, Count, Min, Max, Avg }

#[derive(Debug, Clone)]
pub struct AggResult {
    pub col:   String,
    pub op:    ParAggOp,
    pub value: f64,
}

/// Compute multiple aggregations over a DataBlock in parallel.
pub fn par_aggregate(block: &DataBlock, specs: &[(String, ParAggOp)]) -> Vec<AggResult> {
    specs.par_iter().map(|(col_name, op)| {
        let col = block.columns.iter().find(|c| {
            c.name == *col_name || c.name.ends_with(&format!(".{}", col_name))
        });
        let vals: Vec<f64> = col.map(|c| extract_f64(&c.data)).unwrap_or_default();
        let value = match op {
            ParAggOp::Count => vals.len() as f64,
            ParAggOp::Sum   => vals.par_iter().sum(),
            ParAggOp::Min   => vals.par_iter().copied().reduce(|| f64::INFINITY, f64::min),
            ParAggOp::Max   => vals.par_iter().copied().reduce(|| f64::NEG_INFINITY, f64::max),
            ParAggOp::Avg   => {
                if vals.is_empty() { 0.0 }
                else { vals.par_iter().sum::<f64>() / vals.len() as f64 }
            }
        };
        AggResult { col: col_name.clone(), op: op.clone(), value }
    }).collect()
}

// ─── Parallel sort ────────────────────────────────────────────────────────────

/// Sort rows by a column in parallel (unstable).  Returns a new sorted block.
pub fn par_sort(block: &DataBlock, col_name: &str, desc: bool) -> Result<DataBlock, KoreError> {
    let col = block.columns.iter().find(|c| {
        c.name == col_name || c.name.ends_with(&format!(".{}", col_name))
    }).ok_or_else(|| KoreError::InvalidArgument(format!("sort column not found: {col_name}")))?;

    let mut indices: Vec<usize> = (0..block.num_rows).collect();
    // Use par_sort_unstable_by for parallel sort
    let keys: Vec<SortKey> = (0..block.num_rows).map(|r| extract_sort_key(&col.data, r)).collect();
    indices.par_sort_unstable_by(|&a, &b| {
        let ord = keys[a].partial_cmp(&keys[b]).unwrap_or(std::cmp::Ordering::Equal);
        if desc { ord.reverse() } else { ord }
    });

    Ok(block.select_rows(&indices))
}

#[derive(PartialEq, PartialOrd)]
struct SortKey(f64);   // NaN sorts last

fn extract_sort_key(data: &ColumnData, row: usize) -> SortKey {
    let v = match data {
        ColumnData::Int64(v)   => v.get(row).and_then(|x| *x).map(|i| i as f64),
        ColumnData::Float64(v) => v.get(row).and_then(|x| *x),
        ColumnData::Bool(v)    => v.get(row).and_then(|x| *x).map(|b| b as i64 as f64),
        ColumnData::Str(_)     => None,
        ColumnData::StrDict { .. } => None,
    };
    SortKey(v.unwrap_or(f64::NAN))
}

// ─── Parallel map (scalar transform) ─────────────────────────────────────────

/// Apply a transform to every element of a numeric column in parallel.
/// Returns a new Float64 column.
pub fn par_map<F>(col: &Column, f: F) -> Column
where
    F: Fn(Option<f64>) -> Option<f64> + Sync + Send,
{
    let vals: Vec<Option<f64>> = match &col.data {
        ColumnData::Int64(v)   => v.par_iter().map(|x| f(x.map(|i| i as f64))).collect(),
        ColumnData::Float64(v) => v.par_iter().map(|x| f(*x)).collect(),
        ColumnData::Bool(v)    => v.par_iter().map(|x| f(x.map(|b| b as i64 as f64))).collect(),
        ColumnData::Str(_)              => vec![None; col.data.len()],
        ColumnData::StrDict { codes, .. } => vec![None; codes.len()],
    };
    Column { name: col.name.clone(), data: ColumnData::Float64(vals) }
}

// ─── Parallel hash group-by ───────────────────────────────────────────────────

/// Parallel group-by: returns unique key values and their row-index lists.
/// Uses a concurrent thread-local approach: split rows across Rayon threads,
/// each thread builds a local map, then merge.
pub fn par_group_by(block: &DataBlock, key_col: &str) -> Vec<(String, Vec<usize>)> {
    use std::collections::HashMap;
    let col = block.columns.iter().find(|c| {
        c.name == key_col || c.name.ends_with(&format!(".{}", key_col))
    });
    let n = block.num_rows;
    let keys: Vec<String> = (0..n).map(|r| {
        col.map(|c| cell_to_str(&c.data, r)).unwrap_or_default()
    }).collect();

    // Parallel fold + reduce to build the group map
    let map: std::collections::HashMap<String, Vec<usize>> = (0..n).into_par_iter()
        .fold(
            || std::collections::HashMap::<String, Vec<usize>>::new(),
            |mut acc, r| { acc.entry(keys[r].clone()).or_default().push(r); acc }
        )
        .reduce(
            || std::collections::HashMap::<String, Vec<usize>>::new(),
            |mut a, b| {
                for (k, v) in b { a.entry(k).or_default().extend(v); }
                a
            }
        );

    let mut result: Vec<(String, Vec<usize>)> = map.into_iter().collect();
    result.sort_by(|a, b| a.0.cmp(&b.0));   // deterministic order
    result
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn extract_f64(data: &ColumnData) -> Vec<f64> {
    match data {
        ColumnData::Int64(v)   => v.iter().filter_map(|x| x.map(|i| i as f64)).collect(),
        ColumnData::Float64(v) => v.iter().filter_map(|x| *x).collect(),
        ColumnData::Bool(v)    => v.iter().filter_map(|x| x.map(|b| b as i64 as f64)).collect(),
        ColumnData::Str(_)     => vec![],
        ColumnData::StrDict { .. } => vec![],
    }
}

fn cell_to_str(data: &ColumnData, row: usize) -> String {
    match data {
        ColumnData::Int64(v)   => v.get(row).and_then(|x| *x).map(|i| i.to_string()).unwrap_or_default(),
        ColumnData::Float64(v) => v.get(row).and_then(|x| *x).map(|f| format!("{f:.6}")).unwrap_or_default(),
        ColumnData::Bool(v)    => v.get(row).and_then(|x| *x).map(|b| b.to_string()).unwrap_or_default(),
        ColumnData::Str(v)     => v.get(row).and_then(|x| x.clone()).unwrap_or_default(),
        ColumnData::StrDict { codes, dict } => {
            let c = codes.get(row).copied().unwrap_or(u8::MAX);
            if c == u8::MAX { String::new() } else { dict.get(c as usize).cloned().unwrap_or_default() }
        }
    }
}

// ─── ColumnData len helper ────────────────────────────────────────────────────

trait ColLen { fn len(&self) -> usize; }
impl ColLen for ColumnData {
    fn len(&self) -> usize {
        match self {
            ColumnData::Int64(v)   => v.len(),
            ColumnData::Float64(v) => v.len(),
            ColumnData::Bool(v)    => v.len(),
            ColumnData::Str(v)              => v.len(),
            ColumnData::StrDict { codes, .. } => codes.len(),
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};

    fn make_block() -> DataBlock {
        DataBlock {
            num_rows: 6,
            columns: vec![
                Column { name: "id".into(), data: ColumnData::Int64(vec![
                    Some(1),Some(2),Some(3),Some(4),Some(5),Some(6)]) },
                Column { name: "val".into(), data: ColumnData::Float64(vec![
                    Some(10.0),Some(20.0),Some(30.0),Some(40.0),Some(50.0),Some(60.0)]) },
                Column { name: "cat".into(), data: ColumnData::Str(vec![
                    Some("A".into()),Some("B".into()),Some("A".into()),
                    Some("B".into()),Some("A".into()),Some("C".into())]) },
            ],
        }
    }

    #[test]
    fn test_par_filter() {
        let b = make_block();
        let r = par_filter(&b, |row| {
            if let ColumnData::Float64(v) = &b.columns[1].data {
                v[row].unwrap_or(0.0) > 30.0
            } else { false }
        });
        assert_eq!(r.num_rows, 3); // 40, 50, 60
    }

    #[test]
    fn test_par_aggregate() {
        let b = make_block();
        let specs = vec![
            ("val".into(), ParAggOp::Sum),
            ("val".into(), ParAggOp::Max),
            ("id".into(),  ParAggOp::Count),
        ];
        let results = par_aggregate(&b, &specs);
        assert_eq!(results.len(), 3);
        let sum = results.iter().find(|r| r.op == ParAggOp::Sum).unwrap();
        assert!((sum.value - 210.0).abs() < 0.001);
        let max = results.iter().find(|r| r.op == ParAggOp::Max).unwrap();
        assert!((max.value - 60.0).abs() < 0.001);
    }

    #[test]
    fn test_par_sort() {
        let b = make_block();
        let sorted = par_sort(&b, "val", true).unwrap();  // DESC
        assert_eq!(sorted.num_rows, 6);
        if let ColumnData::Float64(v) = &sorted.columns[1].data {
            assert_eq!(v[0], Some(60.0));
            assert_eq!(v[5], Some(10.0));
        }
    }

    #[test]
    fn test_par_group_by() {
        let b = make_block();
        let groups = par_group_by(&b, "cat");
        // A=3 rows, B=2 rows, C=1 row
        assert_eq!(groups.len(), 3);
        let a = groups.iter().find(|(k,_)| k=="A").unwrap();
        assert_eq!(a.1.len(), 3);
    }

    #[test]
    fn test_par_map() {
        let b = make_block();
        let doubled = par_map(&b.columns[1], |v| v.map(|x| x * 2.0));
        if let ColumnData::Float64(v) = &doubled.data {
            assert_eq!(v[0], Some(20.0));
            assert_eq!(v[5], Some(120.0));
        }
    }
}
