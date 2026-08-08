//! KORE Layer 70 — Python C ABI bridge
//!
//! Exposes KORE to Python via `ctypes` without needing PyO3.
//!
//! # Python usage example
//! ```python
//! import ctypes, json, sys
//! lib = ctypes.CDLL("./libkore_python.so")
//! lib.kore_session_new.restype = ctypes.c_void_p
//! lib.kore_query.restype = ctypes.c_char_p
//!
//! sess = lib.kore_session_new()
//! lib.kore_load_csv(sess, b"orders", b"/data/orders.csv")
//! raw = lib.kore_query(sess, b"SELECT * FROM orders LIMIT 5")
//! rows = json.loads(raw)
//! lib.kore_session_free(sess)
//! ```

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};

use kore_core::{Column, ColumnData, DataBlock};
use kore_sql::KqlContext;

// ─── Session ──────────────────────────────────────────────────────────────────

/// Opaque session struct exposed to C/Python callers.
pub struct KoreSession {
    ctx: KqlContext,
}

impl KoreSession {
    fn new() -> Self {
        Self { ctx: KqlContext::new() }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Safety: pointer must have been returned by `kore_session_new`.
unsafe fn sess_ref<'a>(ptr: *mut KoreSession) -> Option<&'a mut KoreSession> {
    if ptr.is_null() { None } else { Some(&mut *ptr) }
}

fn datablock_to_json(block: &DataBlock) -> String {
    let mut rows: Vec<serde_json::Value> = Vec::with_capacity(block.num_rows);
    for r in 0..block.num_rows {
        let mut obj = serde_json::Map::new();
        for col in &block.columns {
            let val = col.data.get_value(r);
            let jval = match val {
                kore_core::Value::Int(i)   => serde_json::Value::Number(i.into()),
                kore_core::Value::Float(f) => {
                    serde_json::Number::from_f64(f)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null)
                }
                kore_core::Value::Bool(b)  => serde_json::Value::Bool(b),
                kore_core::Value::Str(s)   => serde_json::Value::String(s),
                kore_core::Value::Null     => serde_json::Value::Null,
            };
            // Strip table qualifier: "orders.total" → "total"
            let key = col.name.rfind('.').map(|i| &col.name[i+1..]).unwrap_or(&col.name);
            obj.insert(key.to_string(), jval);
        }
        rows.push(serde_json::Value::Object(obj));
    }
    serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into())
}

fn load_csv_impl(ctx: &mut KqlContext, table_name: &str, path: &str) -> Result<(), String> {
    use std::io::{BufRead, BufReader};
    let file = std::fs::File::open(path)
        .map_err(|e| format!("open {path}: {e}"))?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let header_line = lines.next()
        .ok_or("empty file")?
        .map_err(|e| e.to_string())?;
    let headers: Vec<String> = header_line.split(',')
        .map(|s| s.trim().to_string())
        .collect();

    // Collect all rows first so we can infer types
    let mut raw: Vec<Vec<String>> = Vec::new();
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        let fields: Vec<String> = line.split(',')
            .map(|s| s.trim().to_string())
            .collect();
        if fields.len() == headers.len() {
            raw.push(fields);
        }
    }

    // Infer column types: try i64 → f64 → str
    let mut columns: Vec<Column> = headers.iter().enumerate().map(|(ci, hdr)| {
        let all_int = raw.iter().all(|r| {
            let v = &r[ci];
            v.is_empty() || v.parse::<i64>().is_ok()
        });
        let all_float = !all_int && raw.iter().all(|r| {
            let v = &r[ci];
            v.is_empty() || v.parse::<f64>().is_ok()
        });

        if all_int {
            let data: Vec<Option<i64>> = raw.iter()
                .map(|r| if r[ci].is_empty() { None } else { r[ci].parse().ok() })
                .collect();
            Column::int64(hdr, data)
        } else if all_float {
            let data: Vec<Option<f64>> = raw.iter()
                .map(|r| if r[ci].is_empty() { None } else { r[ci].parse().ok() })
                .collect();
            Column::float64(hdr, data)
        } else {
            let data: Vec<Option<String>> = raw.iter()
                .map(|r| if r[ci].is_empty() { None } else { Some(r[ci].clone()) })
                .collect();
            Column::str_col(hdr, data)
        }
    }).collect();

    let block = DataBlock::new(columns)
        .map_err(|e| e.to_string())?;
    ctx.register(table_name, block);
    Ok(())
}

// ─── C ABI ────────────────────────────────────────────────────────────────────

/// Create a new KORE session. Returns an opaque pointer; free with `kore_session_free`.
#[no_mangle]
pub extern "C" fn kore_session_new() -> *mut KoreSession {
    Box::into_raw(Box::new(KoreSession::new()))
}

/// Free a session created by `kore_session_new`.
///
/// # Safety
/// `sess` must have been returned by `kore_session_new` and not yet freed.
#[no_mangle]
pub unsafe extern "C" fn kore_session_free(sess: *mut KoreSession) {
    if !sess.is_null() {
        drop(Box::from_raw(sess));
    }
}

/// Load a CSV file as a named table.
/// Returns 0 on success, -1 on error.
///
/// # Safety
/// Both C strings must be valid UTF-8 and null-terminated.
#[no_mangle]
pub unsafe extern "C" fn kore_load_csv(
    sess: *mut KoreSession,
    table_name: *const c_char,
    path: *const c_char,
) -> c_int {
    let sess = match sess_ref(sess) { Some(s) => s, None => return -1 };
    let tname = match CStr::from_ptr(table_name).to_str() { Ok(s) => s, Err(_) => return -1 };
    let fpath = match CStr::from_ptr(path).to_str()        { Ok(s) => s, Err(_) => return -1 };
    match load_csv_impl(&mut sess.ctx, tname, fpath) {
        Ok(_)  => 0,
        Err(_) => -1,
    }
}

/// Execute a SQL query and return a JSON string of rows.
/// Returns NULL on error. Caller must free the string with `kore_free_string`.
///
/// # Safety
/// `sql` must be a valid null-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn kore_query(
    sess: *mut KoreSession,
    sql: *const c_char,
) -> *mut c_char {
    let sess = match sess_ref(sess) { Some(s) => s, None => return std::ptr::null_mut() };
    let sql_str = match CStr::from_ptr(sql).to_str() { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    match sess.ctx.query(sql_str) {
        Ok(block) => {
            let json = datablock_to_json(&block);
            match CString::new(json) {
                Ok(cs) => cs.into_raw(),
                Err(_) => std::ptr::null_mut(),
            }
        }
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free a string returned by `kore_query`.
///
/// # Safety
/// `s` must have been returned by `kore_query` and not yet freed.
#[no_mangle]
pub unsafe extern "C" fn kore_free_string(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}

/// Return the number of rows in a named table, or -1 if not found.
///
/// # Safety
/// `table_name` must be a valid null-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn kore_row_count(
    sess: *mut KoreSession,
    table_name: *const c_char,
) -> i64 {
    let sess = match sess_ref(sess) { Some(s) => s, None => return -1 };
    let tname = match CStr::from_ptr(table_name).to_str() { Ok(s) => s, Err(_) => return -1 };
    match sess.ctx.get(tname) {
        Some(block) => block.num_rows as i64,
        None        => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_lifecycle() {
        let sess = kore_session_new();
        assert!(!sess.is_null());
        unsafe { kore_session_free(sess) };
    }

    #[test]
    fn query_empty_context() {
        let sess = kore_session_new();
        let sql = std::ffi::CString::new("SELECT 1 AS val").unwrap();
        let res = unsafe { kore_query(sess, sql.as_ptr()) };
        // SELECT 1 uses __dual__ — always succeeds even on empty context
        assert!(!res.is_null());
        unsafe { kore_free_string(res) };
        // unknown table → NULL
        let sql2 = std::ffi::CString::new("SELECT * FROM no_such_table").unwrap();
        let res2 = unsafe { kore_query(sess, sql2.as_ptr()) };
        assert!(res2.is_null());
        unsafe { kore_session_free(sess) };
    }
}
