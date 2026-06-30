//! KORE Layer 49 — DML Operations
//!
//! Closes the SQL completeness gap with full DML:
//! - `INSERT INTO table SELECT ...` / `INSERT INTO table VALUES (...)`
//! - `UPDATE table SET col = expr WHERE cond`
//! - `DELETE FROM table WHERE cond`
//! - `CREATE TABLE AS SELECT` (CTAS)
//! - `MERGE INTO target USING source ON cond WHEN MATCHED ... WHEN NOT MATCHED ...`
//!
//! All mutations go through the kore-delta transaction log, giving
//! every DML statement ACID semantics and time-travel for free.

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use kore_core::{Column, ColumnData, DataBlock, KoreError};
use kore_delta::{DeltaTable, SchemaField};
use kore_sql::executor::KqlContext;

// ─── DML result ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DmlResult {
    pub operation:     String,
    pub rows_affected: usize,
    pub new_version:   u64,
}

// ─── INSERT INTO ──────────────────────────────────────────────────────────────

/// Insert rows from a DataBlock into a DeltaTable.
pub fn insert_into(table: &mut DeltaTable, data: DataBlock) -> Result<DmlResult, KoreError> {
    let rows = data.num_rows;
    let version = table.insert(data)?;
    Ok(DmlResult { operation: "INSERT".into(), rows_affected: rows, new_version: version })
}

/// INSERT INTO table SELECT ... — evaluate query then append result.
pub fn insert_select(
    table: &mut DeltaTable,
    ctx:   &KqlContext,
    sql:   &str,
) -> Result<DmlResult, KoreError> {
    let result = ctx.query(sql)?;
    insert_into(table, result)
}

/// INSERT INTO table VALUES (v1, v2, ...), (v3, v4, ...) — parse inline values.
pub fn insert_values(
    table:   &mut DeltaTable,
    schema:  &[SchemaField],
    values:  Vec<Vec<serde_json::Value>>,
) -> Result<DmlResult, KoreError> {
    if values.is_empty() { return Ok(DmlResult { operation: "INSERT".into(), rows_affected: 0, new_version: table.version() }); }
    let n = values.len();
    let mut columns: Vec<Column> = schema.iter().enumerate().map(|(ci, f)| {
        let data = match f.dtype.to_uppercase().as_str() {
            "INT64" | "INT" | "INTEGER" | "BIGINT" => ColumnData::Int64(
                values.iter().map(|row| row.get(ci).and_then(|v| v.as_i64())).collect()
            ),
            "FLOAT64" | "DOUBLE" | "FLOAT" => ColumnData::Float64(
                values.iter().map(|row| row.get(ci).and_then(|v| v.as_f64())).collect()
            ),
            "BOOL" | "BOOLEAN" => ColumnData::Bool(
                values.iter().map(|row| row.get(ci).and_then(|v| v.as_bool())).collect()
            ),
            _ => ColumnData::Str(
                values.iter().map(|row| row.get(ci).and_then(|v| v.as_str()).map(|s| s.to_string())).collect()
            ),
        };
        Column { name: f.name.clone(), data }
    }).collect();
    let block = DataBlock { columns, num_rows: n };
    insert_into(table, block)
}

// ─── UPDATE ───────────────────────────────────────────────────────────────────

/// UPDATE table SET assignments WHERE predicate.
///
/// Each assignment is `(column_name, new_value_expr_str)`.
/// Predicate is a SQL WHERE clause fragment.
pub fn update(
    table:       &mut DeltaTable,
    ctx:         &KqlContext,
    table_name:  &str,
    assignments: &[(String, String)],    // (col, value_sql)
    where_sql:   Option<&str>,
) -> Result<DmlResult, KoreError> {
    // 1. Read current data
    let current = table.read()?;
    let n = current.num_rows;

    // 2. Determine which rows match the WHERE clause
    let mask: Vec<bool> = if let Some(wh) = where_sql {
        let mut ctx2 = ctx.clone();
        ctx2.register(table_name, current.clone());
        let sql = format!("SELECT * FROM {table_name} WHERE {wh}");
        // Re-read and mark matching rows
        let matched = ctx2.query(&sql)?;
        // Build a set of row indices that match by comparing first column values
        let matched_ids: std::collections::HashSet<String> = if let Some(col) = matched.columns.first() {
            (0..matched.num_rows).filter_map(|r| {
                match &col.data {
                    ColumnData::Int64(v)   => v.get(r).and_then(|x| *x).map(|i| i.to_string()),
                    ColumnData::Str(v)     => v.get(r).and_then(|x| x.clone()),
                    _ => None,
                }
            }).collect()
        } else { std::collections::HashSet::new() };

        if let Some(id_col) = current.columns.first() {
            (0..n).map(|r| match &id_col.data {
                ColumnData::Int64(v) => v.get(r).and_then(|x| *x).map(|i| matched_ids.contains(&i.to_string())).unwrap_or(false),
                ColumnData::Str(v)   => v.get(r).and_then(|x| x.as_deref()).map(|s| matched_ids.contains(s)).unwrap_or(false),
                _ => false,
            }).collect()
        } else { vec![false; n] }
    } else {
        vec![true; n] // no WHERE = update all
    };

    let updated_count = mask.iter().filter(|&&b| b).count();

    // 3. Apply assignments to matching rows — build new DataBlock
    let mut new_cols: Vec<Column> = current.columns.clone();
    for (col_name, val_sql) in assignments {
        // Parse value_sql as a literal or simple expression
        let new_val = eval_simple_val(val_sql);
        if let Some(col_idx) = new_cols.iter().position(|c| &c.name == col_name) {
            match &mut new_cols[col_idx].data {
                ColumnData::Int64(v) => {
                    if let Some(i) = new_val.as_i64() {
                        for (r, &update) in mask.iter().enumerate() {
                            if update { v[r] = Some(i); }
                        }
                    }
                }
                ColumnData::Float64(v) => {
                    if let Some(f) = new_val.as_f64() {
                        for (r, &update) in mask.iter().enumerate() {
                            if update { v[r] = Some(f); }
                        }
                    }
                }
                ColumnData::Str(v) => {
                    if let Some(s) = new_val.as_str() {
                        for (r, &update) in mask.iter().enumerate() {
                            if update { v[r] = Some(s.to_string()); }
                        }
                    }
                }
                ColumnData::Bool(v) => {
                    if let Some(b) = new_val.as_bool() {
                        for (r, &update) in mask.iter().enumerate() {
                            if update { v[r] = Some(b); }
                        }
                    }
                }
                ColumnData::StrDict { .. } => {} // read-only dict; skip in-place update
            }
        }
    }

    // 4. Delete all + re-insert updated block via Delta
    let updated = DataBlock { columns: new_cols, num_rows: n };
    table.delete(|_, _| true)?;   // erase all current data
    let ver = table.insert(updated)?;;
    Ok(DmlResult { operation: format!("UPDATE ({updated_count} rows)"), rows_affected: updated_count, new_version: ver })
}

// ─── DELETE ───────────────────────────────────────────────────────────────────

/// DELETE FROM table WHERE predicate.
pub fn delete(
    table:      &mut DeltaTable,
    ctx:        &KqlContext,
    table_name: &str,
    where_sql:  &str,
) -> Result<DmlResult, KoreError> {
    let mut ctx2 = ctx.clone();
    let current = table.read()?;
    ctx2.register(table_name, current.clone());

    // Get rows to KEEP (inverse of WHERE)
    let keep_sql = format!("SELECT * FROM {table_name} WHERE NOT ({where_sql})");
    let kept = ctx2.query(&keep_sql)?;
    let removed = current.num_rows - kept.num_rows;

    table.delete(|_, _| true)?;   // erase all current data
    let ver = table.insert(kept)?;;
    Ok(DmlResult { operation: format!("DELETE ({removed} rows)"), rows_affected: removed, new_version: ver })
}

// ─── CREATE TABLE AS SELECT ───────────────────────────────────────────────────

/// CTAS: Create a new DeltaTable from a SQL query result.
pub fn create_table_as_select(
    path:       impl AsRef<Path>,
    ctx:        &KqlContext,
    sql:        &str,
) -> Result<(DeltaTable, DmlResult), KoreError> {
    let result = ctx.query(sql)?;
    let rows   = result.num_rows;

    // Infer schema from result
    let schema: Vec<SchemaField> = result.columns.iter().map(|c| SchemaField {
        name:     c.name.clone(),
        dtype:    match &c.data {
            ColumnData::Int64(_)   => "INT64".into(),
            ColumnData::Float64(_) => "FLOAT64".into(),
            ColumnData::Bool(_)    => "BOOL".into(),
            ColumnData::Str(_)      => "VARCHAR".into(),
            ColumnData::StrDict { .. } => "VARCHAR".into(),
        },
        nullable: true,
    }).collect();

    let mut table = DeltaTable::create(path, schema)?;
    let version   = table.insert(result)?;
    let dml       = DmlResult { operation: "CTAS".into(), rows_affected: rows, new_version: version };
    Ok((table, dml))
}

// ─── MERGE INTO (upsert) ─────────────────────────────────────────────────────

/// MERGE INTO target USING source ON key — upsert semantics.
///
/// - Matching rows: update target with source values
/// - Non-matching source rows: insert into target
pub fn merge_into(
    target:     &mut DeltaTable,
    source:     &DataBlock,
    join_key:   &str,
) -> Result<DmlResult, KoreError> {
    let current = target.read()?;

    // Build key → row index in current (target)
    let mut target_keys: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    if let Some(col) = current.columns.iter().find(|c| c.name == join_key) {
        for r in 0..current.num_rows {
            let k = match &col.data {
                ColumnData::Int64(v)   => v.get(r).and_then(|x| *x).map(|i| i.to_string()).unwrap_or_default(),
                ColumnData::Str(v)     => v.get(r).and_then(|x| x.clone()).unwrap_or_default(),
                _ => format!("{r}"),
            };
            target_keys.insert(k, r);
        }
    }

    let mut updated = 0usize;
    let mut inserted = 0usize;
    let mut result_cols = current.columns.clone();

    // Process source rows
    let src_key_col = source.columns.iter().find(|c| c.name == join_key);
    for sr in 0..source.num_rows {
        let sk = src_key_col.map(|c| match &c.data {
            ColumnData::Int64(v)   => v.get(sr).and_then(|x| *x).map(|i| i.to_string()).unwrap_or_default(),
            ColumnData::Str(v)     => v.get(sr).and_then(|x| x.clone()).unwrap_or_default(),
            _ => format!("{sr}"),
        }).unwrap_or_default();

        if let Some(&tr) = target_keys.get(&sk) {
            // MATCHED: update target row with source values
            for src_col in &source.columns {
                if let Some(tgt_col) = result_cols.iter_mut().find(|c| c.name == src_col.name) {
                    match (&mut tgt_col.data, &src_col.data) {
                        (ColumnData::Int64(t), ColumnData::Int64(s))     => { if tr < t.len() { t[tr] = s.get(sr).copied().flatten(); } }
                        (ColumnData::Float64(t), ColumnData::Float64(s)) => { if tr < t.len() { t[tr] = s.get(sr).copied().flatten(); } }
                        (ColumnData::Str(t), ColumnData::Str(s))         => { if tr < t.len() { t[tr] = s.get(sr).cloned().flatten(); } }
                        (ColumnData::Bool(t), ColumnData::Bool(s))       => { if tr < t.len() { t[tr] = s.get(sr).copied().flatten(); } }
                        _ => {}
                    }
                }
            }
            updated += 1;
        } else {
            // NOT MATCHED: insert new row
            for src_col in &source.columns {
                if let Some(tgt_col) = result_cols.iter_mut().find(|c| c.name == src_col.name) {
                    match (&mut tgt_col.data, &src_col.data) {
                        (ColumnData::Int64(t), ColumnData::Int64(s))     => t.push(s.get(sr).copied().flatten()),
                        (ColumnData::Float64(t), ColumnData::Float64(s)) => t.push(s.get(sr).copied().flatten()),
                        (ColumnData::Str(t), ColumnData::Str(s))         => t.push(s.get(sr).cloned().flatten()),
                        (ColumnData::Bool(t), ColumnData::Bool(s))       => t.push(s.get(sr).copied().flatten()),
                        _ => {}
                    }
                }
            }
            inserted += 1;
        }
    }

    let new_n = current.num_rows + inserted;
    let merged = DataBlock { columns: result_cols, num_rows: new_n };
    target.delete(|_, _| true)?;   // erase all current data
    let ver = target.insert(merged)?;
    Ok(DmlResult {
        operation: format!("MERGE (updated={updated}, inserted={inserted})"),
        rows_affected: updated + inserted,
        new_version: ver,
    })
}

// ─── Helper ───────────────────────────────────────────────────────────────────

fn eval_simple_val(expr: &str) -> serde_json::Value {
    let s = expr.trim();
    if let Ok(i) = s.parse::<i64>()   { return serde_json::json!(i); }
    if let Ok(f) = s.parse::<f64>()   { return serde_json::json!(f); }
    if s.eq_ignore_ascii_case("true")  { return serde_json::json!(true); }
    if s.eq_ignore_ascii_case("false") { return serde_json::json!(false); }
    if s.eq_ignore_ascii_case("null")  { return serde_json::Value::Null; }
    // String literal: strip quotes
    let stripped = s.trim_matches('\'').trim_matches('"');
    serde_json::json!(stripped)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData, DataBlock};
    use kore_delta::{DeltaTable, SchemaField};
    use kore_sql::executor::KqlContext;

    fn schema() -> Vec<SchemaField> {
        vec![
            SchemaField { name: "id".into(),    dtype: "INT64".into(),   nullable: false },
            SchemaField { name: "name".into(),  dtype: "VARCHAR".into(), nullable: true  },
            SchemaField { name: "score".into(), dtype: "FLOAT64".into(), nullable: true  },
        ]
    }

    fn tmp(s: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("kore_dml_{s}"));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    fn sample() -> DataBlock {
        DataBlock {
            num_rows: 3,
            columns: vec![
                Column { name: "id".into(),    data: ColumnData::Int64(vec![Some(1),Some(2),Some(3)]) },
                Column { name: "name".into(),  data: ColumnData::Str(vec![Some("A".into()),Some("B".into()),Some("C".into())]) },
                Column { name: "score".into(), data: ColumnData::Float64(vec![Some(10.0),Some(20.0),Some(30.0)]) },
            ],
        }
    }

    #[test]
    fn test_insert_into() {
        let dir = tmp("ins");
        let mut t = DeltaTable::create(&dir, schema()).unwrap();
        let r = insert_into(&mut t, sample()).unwrap();
        assert_eq!(r.rows_affected, 3);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_insert_select() {
        let dir = tmp("isel");
        let mut t = DeltaTable::create(&dir, schema()).unwrap();
        let mut ctx = KqlContext::new();
        ctx.register("src", sample());
        let r = insert_select(&mut t, &ctx, "SELECT * FROM src WHERE score > 15").unwrap();
        assert_eq!(r.rows_affected, 2);
        let data = t.read().unwrap();
        assert_eq!(data.num_rows, 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_delete() {
        let dir = tmp("del");
        let mut t = DeltaTable::create(&dir, schema()).unwrap();
        insert_into(&mut t, sample()).unwrap();
        let ctx = KqlContext::new();
        let r = delete(&mut t, &ctx, "tbl", "score > 15").unwrap();
        assert_eq!(r.rows_affected, 2);
        let data = t.read().unwrap();
        assert_eq!(data.num_rows, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_ctas() {
        let dir = tmp("ctas");
        let mut ctx = KqlContext::new();
        ctx.register("src", sample());
        let (t, r) = create_table_as_select(&dir, &ctx, "SELECT * FROM src WHERE score >= 20").unwrap();
        assert_eq!(r.rows_affected, 2);
        assert_eq!(t.read().unwrap().num_rows, 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_merge_into() {
        let dir = tmp("merge");
        let mut t = DeltaTable::create(&dir, schema()).unwrap();
        insert_into(&mut t, sample()).unwrap();

        // Source: update id=1 (score→99), insert id=4 (new)
        let source = DataBlock {
            num_rows: 2,
            columns: vec![
                Column { name: "id".into(),    data: ColumnData::Int64(vec![Some(1),Some(4)]) },
                Column { name: "name".into(),  data: ColumnData::Str(vec![Some("A_updated".into()),Some("D".into())]) },
                Column { name: "score".into(), data: ColumnData::Float64(vec![Some(99.0),Some(40.0)]) },
            ],
        };
        let r = merge_into(&mut t, &source, "id").unwrap();
        assert!(r.rows_affected >= 2);
        let data = t.read().unwrap();
        assert_eq!(data.num_rows, 4); // 3 original + 1 new
        std::fs::remove_dir_all(&dir).ok();
    }
}
