//! KORE Layer 59 — SQL v2: DISTINCT, EXCEPT, INTERSECT, ROLLUP, CUBE, GROUPING SETS
//!
//! Closes the remaining SQL completeness gap vs Spark SQL / ANSI SQL:
//!
//! | Feature          | Example                                      | Status |
//! |------------------|----------------------------------------------|--------|
//! | SELECT DISTINCT  | `SELECT DISTINCT region FROM sales`          | ✅ New |
//! | EXCEPT           | `SELECT id FROM a EXCEPT SELECT id FROM b`   | ✅ New |
//! | INTERSECT        | `SELECT id FROM a INTERSECT SELECT id FROM b`| ✅ New |
//! | ROLLUP           | `GROUP BY ROLLUP(year, month, day)`           | ✅ New |
//! | CUBE             | `GROUP BY CUBE(region, product)`              | ✅ New |
//! | GROUPING SETS    | `GROUP BY GROUPING SETS ((a,b),(a),(b),())`  | ✅ New |
//! | LATERAL JOIN     | `CROSS JOIN LATERAL (SELECT ...)`            | 🔄 Planned |

use std::collections::{HashMap, HashSet};
use kore_core::{Column, ColumnData, DataBlock, KoreError};
use kore_sql::executor::KqlContext;

// ─── SELECT DISTINCT ──────────────────────────────────────────────────────────

/// Apply DISTINCT to a DataBlock — remove duplicate rows.
/// Uses a hash-based deduplication over all column values.
pub fn apply_distinct(block: &DataBlock) -> DataBlock {
    let n = block.num_rows;
    let mut seen: HashSet<String> = HashSet::new();
    let mut keep: Vec<usize> = Vec::new();

    for r in 0..n {
        let key = row_key(block, r);
        if seen.insert(key) {
            keep.push(r);
        }
    }
    block.select_rows(&keep)
}

/// Run a query and apply DISTINCT to the result.
pub fn select_distinct(ctx: &KqlContext, sql: &str) -> Result<DataBlock, KoreError> {
    let result = ctx.query(sql)?;
    Ok(apply_distinct(&result))
}

// ─── EXCEPT / INTERSECT ───────────────────────────────────────────────────────

/// EXCEPT: rows in `left` that are NOT in `right` (set difference).
/// Equivalent to Spark's `DataFrame.exceptAll()` / SQL `EXCEPT ALL`.
pub fn except(left: &DataBlock, right: &DataBlock) -> DataBlock {
    let right_keys: HashSet<String> = (0..right.num_rows)
        .map(|r| row_key(right, r))
        .collect();

    let keep: Vec<usize> = (0..left.num_rows)
        .filter(|&r| !right_keys.contains(&row_key(left, r)))
        .collect();
    left.select_rows(&keep)
}

/// EXCEPT ALL: like EXCEPT but preserves duplicates (removes one copy per match).
pub fn except_all(left: &DataBlock, right: &DataBlock) -> DataBlock {
    // Build a multiset of right keys
    let mut right_counts: HashMap<String, usize> = HashMap::new();
    for r in 0..right.num_rows {
        *right_counts.entry(row_key(right, r)).or_insert(0) += 1;
    }

    let mut keep = Vec::new();
    for l in 0..left.num_rows {
        let k = row_key(left, l);
        match right_counts.get_mut(&k) {
            Some(count) if *count > 0 => { *count -= 1; }  // consumed one match
            _ => keep.push(l),
        }
    }
    left.select_rows(&keep)
}

/// INTERSECT: rows in BOTH `left` AND `right` (set intersection).
pub fn intersect(left: &DataBlock, right: &DataBlock) -> DataBlock {
    let right_keys: HashSet<String> = (0..right.num_rows)
        .map(|r| row_key(right, r))
        .collect();

    // Deduplicate result (INTERSECT = distinct intersection)
    let mut seen = HashSet::new();
    let keep: Vec<usize> = (0..left.num_rows)
        .filter(|&r| {
            let k = row_key(left, r);
            right_keys.contains(&k) && seen.insert(k)
        })
        .collect();
    left.select_rows(&keep)
}

/// INTERSECT ALL: like INTERSECT but preserves duplicates.
pub fn intersect_all(left: &DataBlock, right: &DataBlock) -> DataBlock {
    let mut right_counts: HashMap<String, usize> = HashMap::new();
    for r in 0..right.num_rows {
        *right_counts.entry(row_key(right, r)).or_insert(0) += 1;
    }

    let mut keep = Vec::new();
    for l in 0..left.num_rows {
        let k = row_key(left, l);
        if let Some(count) = right_counts.get_mut(&k) {
            if *count > 0 { *count -= 1; keep.push(l); }
        }
    }
    left.select_rows(&keep)
}

// ─── ROLLUP ───────────────────────────────────────────────────────────────────

/// ROLLUP(a, b, c) generates GROUP BY combinations:
///   (a, b, c), (a, b), (a), ()
///
/// Returns a Vec of DataBlocks, one per grouping level.
/// The caller can UNION ALL them together.
pub fn rollup(
    block:    &DataBlock,
    ctx:      &KqlContext,
    table:    &str,
    cols:     &[String],
    agg_sql:  &str,   // e.g. "SUM(revenue) AS total"
) -> Result<DataBlock, KoreError> {
    let mut parts: Vec<DataBlock> = Vec::new();

    // Generate grouping sets: (c0,c1,c2), (c0,c1), (c0), ()
    for len in (0..=cols.len()).rev() {
        let group_cols = &cols[..len];
        let sql = build_agg_sql(table, group_cols, agg_sql);
        let mut ctx2 = ctx.clone();
        ctx2.register(table, block.clone());
        let result = ctx2.query(&sql)?;
        // Add NULL for missing grouping columns
        let padded = pad_grouping_cols(result, cols, len);
        parts.push(padded);
    }

    DataBlock::concat(parts)
}

/// CUBE(a, b) generates all 2^n combinations of GROUP BY:
///   (a, b), (a), (b), ()
pub fn cube(
    block:   &DataBlock,
    ctx:     &KqlContext,
    table:   &str,
    cols:    &[String],
    agg_sql: &str,
) -> Result<DataBlock, KoreError> {
    let n = cols.len();
    let mut parts = Vec::new();

    // All 2^n subsets
    for mask in 0..(1u32 << n) {
        let group_cols: Vec<String> = (0..n)
            .filter(|&i| (mask >> i) & 1 == 1)
            .map(|i| cols[i].clone())
            .collect();
        let sql = build_agg_sql(table, &group_cols, agg_sql);
        let mut ctx2 = ctx.clone();
        ctx2.register(table, block.clone());
        let result = ctx2.query(&sql)?;
        let padded = pad_grouping_cols(result, cols, group_cols.len());
        parts.push(padded);
    }

    DataBlock::concat(parts)
}

/// GROUPING SETS — execute exactly the specified grouping combinations.
pub fn grouping_sets(
    block:    &DataBlock,
    ctx:      &KqlContext,
    table:    &str,
    sets:     &[Vec<String>],   // each inner Vec is one grouping
    agg_sql:  &str,
) -> Result<DataBlock, KoreError> {
    let all_cols: Vec<String> = {
        let mut seen = std::collections::LinkedList::new();
        let mut set  = HashSet::new();
        for group in sets {
            for c in group {
                if set.insert(c.clone()) { seen.push_back(c.clone()); }
            }
        }
        seen.into_iter().collect()
    };

    let mut parts = Vec::new();
    for group in sets {
        let sql = build_agg_sql(table, group, agg_sql);
        let mut ctx2 = ctx.clone();
        ctx2.register(table, block.clone());
        let result = ctx2.query(&sql)?;
        let padded = pad_grouping_cols(result, &all_cols, group.len());
        parts.push(padded);
    }

    DataBlock::concat(parts)
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn row_key(block: &DataBlock, row: usize) -> String {
    block.columns.iter().map(|col| match &col.data {
        ColumnData::Int64(v)   => v.get(row).and_then(|x| *x).map(|i| i.to_string()).unwrap_or_default(),
        ColumnData::Float64(v) => v.get(row).and_then(|x| *x).map(|f| format!("{f:.10}")).unwrap_or_default(),
        ColumnData::Str(v)     => v.get(row).and_then(|x| x.clone()).unwrap_or_default(),
        ColumnData::Bool(v)    => v.get(row).and_then(|x| *x).map(|b| b.to_string()).unwrap_or_default(),
        ColumnData::StrDict { codes, dict } => {
            let c = codes.get(row).copied().unwrap_or(u8::MAX);
            if c == u8::MAX { String::new() } else { dict.get(c as usize).cloned().unwrap_or_default() }
        }
    }).collect::<Vec<_>>().join("\x01")
}

fn build_agg_sql(table: &str, group_cols: &[String], agg_expr: &str) -> String {
    if group_cols.is_empty() {
        format!("SELECT {agg_expr} FROM {table}")
    } else {
        let cols = group_cols.join(", ");
        format!("SELECT {cols}, {agg_expr} FROM {table} GROUP BY {cols}")
    }
}

fn pad_grouping_cols(mut block: DataBlock, all_cols: &[String], n_present: usize) -> DataBlock {
    // Add NULL columns for grouping dimensions that weren't in this grouping set
    let n = block.num_rows;
    for col_name in all_cols.iter().skip(n_present) {
        if !block.columns.iter().any(|c| c.name == *col_name) {
            block.columns.insert(0, Column {
                name: col_name.clone(),
                data: ColumnData::Str(vec![None; n]),
            });
        }
    }
    block
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};
    use kore_sql::executor::KqlContext;

    fn regions() -> DataBlock {
        DataBlock {
            num_rows: 6,
            columns: vec![
                Column { name: "region".into(), data: ColumnData::Str(vec![
                    Some("EU".into()),Some("US".into()),Some("EU".into()),
                    Some("AP".into()),Some("US".into()),Some("EU".into()),
                ])},
                Column { name: "revenue".into(), data: ColumnData::Float64(vec![
                    Some(100.0),Some(200.0),Some(150.0),Some(80.0),Some(300.0),Some(50.0),
                ])},
            ],
        }
    }

    fn numbers(vals: Vec<i64>) -> DataBlock {
        DataBlock {
            num_rows: vals.len(),
            columns: vec![
                Column { name: "id".into(), data: ColumnData::Int64(vals.into_iter().map(Some).collect()) },
            ],
        }
    }

    #[test]
    fn test_distinct() {
        let block = DataBlock {
            num_rows: 5,
            columns: vec![Column { name: "v".into(), data: ColumnData::Str(vec![
                Some("a".into()),Some("b".into()),Some("a".into()),Some("c".into()),Some("b".into()),
            ])}],
        };
        let d = apply_distinct(&block);
        assert_eq!(d.num_rows, 3);
    }

    #[test]
    fn test_except() {
        let a = numbers(vec![1, 2, 3, 4, 5]);
        let b = numbers(vec![3, 4]);
        let r = except(&a, &b);
        assert_eq!(r.num_rows, 3);  // 1, 2, 5
    }

    #[test]
    fn test_intersect() {
        let a = numbers(vec![1, 2, 3, 4]);
        let b = numbers(vec![2, 4, 6]);
        let r = intersect(&a, &b);
        assert_eq!(r.num_rows, 2);  // 2, 4
    }

    #[test]
    fn test_except_all_duplicates() {
        let a = numbers(vec![1, 1, 2, 3]);
        let b = numbers(vec![1]);
        let r = except_all(&a, &b);
        // Removes ONE occurrence of 1 → [1, 2, 3]
        assert_eq!(r.num_rows, 3);
    }

    #[test]
    fn test_rollup() {
        let mut ctx = KqlContext::new();
        let block = regions();
        let result = rollup(&block, &ctx, "sales", &["region".to_string()], "SUM(revenue) AS total").unwrap();
        // ROLLUP(region): GROUP BY region + GROUP BY () = 2 levels
        assert!(result.num_rows >= 3 + 1); // 3 regions + 1 grand total
    }

    #[test]
    fn test_select_distinct_via_ctx() {
        let mut ctx = KqlContext::new();
        ctx.register("t", regions());
        let result = select_distinct(&ctx, "SELECT region FROM t").unwrap();
        assert_eq!(result.num_rows, 3); // EU, US, AP
    }
}
