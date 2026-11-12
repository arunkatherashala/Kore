//! Flat result rows (one JSON object per row, as a harness prints them) stored as a KORE file.
//!
//! Owner rule: every runtime-persisted file is KORE, never `.json` / `.jsonl`. Harnesses that used to append a
//! line to a `.jsonl` file append a row here instead (`append`), and read the rows back with `read`.
//!
//! Layout: one KORE file (`KoreWriter`), one column per top-level key, in first-seen key order. A column is
//! `Int64` when every non-null value is an integer that fits i64, `Float64` when every non-null value is a number,
//! `Bool` when every non-null value is a boolean, and `Str` otherwise: a column whose values are all strings keeps
//! its name; any other mix (objects, arrays, mixed types) is a `Str` column named `<key>:json` holding the JSON
//! text of each value. A missing key or `null` is a null cell; `read` leaves null cells out of the row object. Only
//! top-level objects are rows. The file is rewritten whole through a `.kore.tmp` file and a rename (harness-sized
//! files: a few hundred rows).

#![allow(dead_code)]

use kore_core::{Column, ColumnData, DataBlock};
use kore_store::{KoreReader, KoreWriter};
use serde_json::{Map, Value};
use std::path::Path;

const JSON_SUFFIX: &str = ":json";

/// All rows of the file (empty when it does not exist).
pub fn read(path: &Path) -> Result<Vec<Value>, String> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let b = KoreReader::read_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut rows: Vec<Map<String, Value>> = (0..b.num_rows).map(|_| Map::new()).collect();
    for c in &b.columns {
        let (name, json) = match c.name.strip_suffix(JSON_SUFFIX) {
            Some(n) => (n.to_string(), true),
            None => (c.name.clone(), false),
        };
        for (i, row) in rows.iter_mut().enumerate() {
            let v = match &c.data {
                ColumnData::Int64(v) => v[i].map(Value::from),
                ColumnData::Float64(v) => v[i].map(Value::from),
                ColumnData::Bool(v) => v[i].map(Value::from),
                _ => match c.data.get_str(i) {
                    Some(s) if json => Some(serde_json::from_str(s).map_err(|e| format!("{}: column {}: {e}", path.display(), c.name))?),
                    Some(s) => Some(Value::from(s)),
                    None => None,
                },
            };
            if let Some(v) = v {
                row.insert(name.clone(), v);
            }
        }
    }
    Ok(rows.into_iter().map(Value::Object).collect())
}

/// The rows as one KORE block (see the module comment for the column types).
pub fn block(rows: &[Value]) -> Result<DataBlock, String> {
    let mut keys: Vec<String> = vec![];
    for r in rows {
        let o = r.as_object().ok_or("a row must be a JSON object")?;
        for k in o.keys() {
            if k.ends_with(JSON_SUFFIX) {
                return Err(format!("key {k}: the suffix {JSON_SUFFIX} is reserved"));
            }
            if !keys.contains(k) {
                keys.push(k.clone());
            }
        }
    }
    let mut cols = vec![];
    for k in &keys {
        let vals: Vec<Option<&Value>> = rows.iter().map(|r| r.get(k).filter(|v| !v.is_null())).collect();
        let all = |f: &dyn Fn(&Value) -> bool| vals.iter().flatten().all(|v| f(v));
        let col = if all(&|v| v.is_i64() || (v.is_u64() && v.as_u64().unwrap() <= i64::MAX as u64)) {
            Column::int64(k, vals.iter().map(|v| v.and_then(|v| v.as_i64())).collect())
        } else if all(&|v| v.is_number()) {
            Column::float64(k, vals.iter().map(|v| v.and_then(|v| v.as_f64())).collect())
        } else if all(&|v| v.is_boolean()) {
            Column::bool_col(k, vals.iter().map(|v| v.and_then(|v| v.as_bool())).collect())
        } else if all(&|v| v.is_string()) {
            Column::str_col(k, vals.iter().map(|v| v.and_then(|v| v.as_str()).map(str::to_string)).collect())
        } else {
            Column::str_col(&format!("{k}{JSON_SUFFIX}"), vals.iter().map(|v| v.map(|v| v.to_string())).collect())
        };
        cols.push(col);
    }
    if cols.is_empty() {
        // a KORE block needs a column: rows without any key
        cols.push(Column::bool_col("_row", vec![None; rows.len()]));
    }
    DataBlock::new(cols).map_err(|e| e.to_string())
}

/// Write `rows` as the whole file (through `<file>.kore.tmp` and a rename).
pub fn write(path: &Path, rows: &[Value]) -> Result<(), String> {
    let b = block(rows)?;
    let tmp = path.with_extension("kore.tmp");
    KoreWriter::write_file(&tmp, &b).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Append one row (the file is read, extended and rewritten).
pub fn append(path: &Path, row: Value) -> Result<(), String> {
    let mut rows = read(path)?;
    rows.push(row);
    write(path, &rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rows_round_trip_as_a_kore_file_and_no_json_file_is_written() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("restart.kore");
        assert!(read(&p).unwrap().is_empty());
        let rows = vec![
            json!({ "type": "restart", "chunk": 5000, "is_error": false, "call_secs": 1.25, "kill_ms": null, "index_runs": { "runs": 2, "maintain": { "ms": 3.5 } }, "text": "" }),
            json!({ "type": "restart", "chunk": 5000, "is_error": true, "call_secs": 2.0, "kill_ms": 100, "kill_lag_ms": 12.5, "index_runs": null, "text": "x\ty\n\"q\"" }),
            json!({ "type": "restart", "chunk": 1, "is_error": false, "call_secs": 3, "stopped": ["a", 1] }),
        ];
        for r in &rows {
            append(&p, r.clone()).unwrap();
        }
        let back = read(&p).unwrap();
        assert_eq!(back.len(), 3);
        // nulls are left out; an integer in a float column reads back as that float
        let mut want = rows.clone();
        for w in want.iter_mut() {
            w.as_object_mut().unwrap().retain(|_, v| !v.is_null());
        }
        want[2]["call_secs"] = json!(3.0);
        assert_eq!(back, want);
        // the file is a KORE file with typed columns
        let b = KoreReader::read_file(&p).unwrap();
        let ty = |n: &str| b.columns.iter().find(|c| c.name == n).map(|c| std::mem::discriminant(&c.data));
        assert_eq!(ty("chunk"), Some(std::mem::discriminant(&ColumnData::Int64(vec![]))));
        assert_eq!(ty("call_secs"), Some(std::mem::discriminant(&ColumnData::Float64(vec![]))));
        assert_eq!(ty("is_error"), Some(std::mem::discriminant(&ColumnData::Bool(vec![]))));
        assert!(ty("index_runs:json").is_some() && ty("stopped:json").is_some());
        // owner rule: storage is KORE only; the only file left is the .kore file (no .json / .jsonl, no tmp)
        let names: Vec<String> = std::fs::read_dir(d.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["restart.kore".to_string()]);
        // not an object / a reserved name: refused, the file unchanged
        assert!(append(&p, json!([1, 2])).is_err());
        assert!(append(&p, json!({ "a:json": 1 })).is_err());
        assert_eq!(read(&p).unwrap().len(), 3);
    }
}
