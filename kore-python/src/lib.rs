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

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};

use kore_core::{Column, DataBlock};
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
                kore_core::Value::Array(_) => serde_json::Value::Array(vec![]),
                kore_core::Value::Map(_)   => serde_json::Value::Object(serde_json::Map::new()),
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
    let columns: Vec<Column> = headers.iter().enumerate().map(|(ci, hdr)| {
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

/// Load a Parquet file and register it as a table.
/// Returns 0 on success, -1 on error.
///
/// # Safety
/// Both pointers must be valid null-terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn kore_load_parquet(
    sess: *mut KoreSession,
    table_name: *const c_char,
    path: *const c_char,
) -> c_int {
    let sess = match sess_ref(sess) { Some(s) => s, None => return -1 };
    let tname = match CStr::from_ptr(table_name).to_str() { Ok(s) => s, Err(_) => return -1 };
    let fpath = match CStr::from_ptr(path).to_str() { Ok(s) => s, Err(_) => return -1 };
    match kore_parquet::ParquetReader::new(fpath).read() {
        Ok(block) => { sess.ctx.register(tname, block); 0 }
        Err(_) => -1,
    }
}

/// Write a query result to a Parquet file.
/// Returns 0 on success, -1 on error.
///
/// # Safety
/// Both pointers must be valid null-terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn kore_write_parquet(
    sess: *mut KoreSession,
    sql: *const c_char,
    path: *const c_char,
) -> c_int {
    let sess = match sess_ref(sess) { Some(s) => s, None => return -1 };
    let sql_str = match CStr::from_ptr(sql).to_str() { Ok(s) => s, Err(_) => return -1 };
    let fpath = match CStr::from_ptr(path).to_str() { Ok(s) => s, Err(_) => return -1 };
    match sess.ctx.query(sql_str) {
        Ok(block) => {
            match kore_parquet::ParquetWriter::write_file(&block, fpath) {
                Ok(_) => 0,
                Err(_) => -1,
            }
        }
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

// ─── DataFrame API (Phase 4B) ────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum DfOp {
    Filter(String),
    Select(Vec<String>),
    GroupBy(Vec<String>),
    Agg(Vec<(String, String)>),
    Join {
        other_table: String,
        left_key: String,
        right_key: String,
        join_type: String,
    },
    OrderBy(Vec<(String, bool)>),
    Limit(usize),
}

pub struct KoreDataFrame {
    session: *mut KoreSession,
    table_name: String,
    operations: Vec<DfOp>,
}

impl KoreDataFrame {
    pub fn new(session: *mut KoreSession, table_name: &str) -> Self {
        Self {
            session,
            table_name: table_name.to_string(),
            operations: Vec::new(),
        }
    }

    pub fn filter(mut self, predicate: &str) -> Self {
        self.operations.push(DfOp::Filter(predicate.to_string()));
        self
    }

    pub fn select(mut self, cols: &[&str]) -> Self {
        self.operations
            .push(DfOp::Select(cols.iter().map(|s| s.to_string()).collect()));
        self
    }

    pub fn group_by(mut self, keys: &[&str]) -> Self {
        self.operations
            .push(DfOp::GroupBy(keys.iter().map(|s| s.to_string()).collect()));
        self
    }

    pub fn agg(mut self, aggregations: &[(&str, &str)]) -> Self {
        self.operations.push(DfOp::Agg(
            aggregations
                .iter()
                .map(|(c, f)| (c.to_string(), f.to_string()))
                .collect(),
        ));
        self
    }

    pub fn join(mut self, other: &str, left_key: &str, right_key: &str) -> Self {
        self.operations.push(DfOp::Join {
            other_table: other.to_string(),
            left_key: left_key.to_string(),
            right_key: right_key.to_string(),
            join_type: "INNER".to_string(),
        });
        self
    }

    pub fn join_type(
        mut self,
        other: &str,
        left_key: &str,
        right_key: &str,
        join_type: &str,
    ) -> Self {
        self.operations.push(DfOp::Join {
            other_table: other.to_string(),
            left_key: left_key.to_string(),
            right_key: right_key.to_string(),
            join_type: join_type.to_uppercase(),
        });
        self
    }

    pub fn order_by(mut self, col: &str, desc: bool) -> Self {
        self.operations
            .push(DfOp::OrderBy(vec![(col.to_string(), desc)]));
        self
    }

    pub fn limit(mut self, n: usize) -> Self {
        self.operations.push(DfOp::Limit(n));
        self
    }

    pub fn build_sql(&self) -> String {
        let mut select_clause = String::from("*");
        let mut from_clause = self.table_name.clone();
        let mut where_parts: Vec<String> = Vec::new();
        let mut group_parts: Vec<String> = Vec::new();
        let mut order_parts: Vec<String> = Vec::new();
        let mut limit_clause: Option<usize> = None;

        for op in &self.operations {
            match op {
                DfOp::Select(cols) => {
                    select_clause = cols.join(", ");
                }
                DfOp::Filter(pred) => {
                    where_parts.push(pred.clone());
                }
                DfOp::GroupBy(keys) => {
                    group_parts = keys.clone();
                }
                DfOp::Agg(pairs) => {
                    let agg_exprs: Vec<String> = pairs
                        .iter()
                        .map(|(col, func)| format!("{}({}) AS {}_{}", func, col, func.to_lowercase(), col))
                        .collect();
                    if group_parts.is_empty() {
                        select_clause = agg_exprs.join(", ");
                    } else {
                        let group_select = group_parts.join(", ");
                        select_clause =
                            format!("{}, {}", group_select, agg_exprs.join(", "));
                    }
                }
                DfOp::Join {
                    other_table,
                    left_key,
                    right_key,
                    join_type,
                } => {
                    from_clause = format!(
                        "{} {} JOIN {} ON {}.{} = {}.{}",
                        from_clause,
                        join_type,
                        other_table,
                        self.table_name,
                        left_key,
                        other_table,
                        right_key
                    );
                }
                DfOp::OrderBy(cols) => {
                    for (col, desc) in cols {
                        let dir = if *desc { "DESC" } else { "ASC" };
                        order_parts.push(format!("{} {}", col, dir));
                    }
                }
                DfOp::Limit(n) => {
                    limit_clause = Some(*n);
                }
            }
        }

        let mut sql = format!("SELECT {} FROM {}", select_clause, from_clause);
        if !where_parts.is_empty() {
            sql.push_str(&format!(" WHERE {}", where_parts.join(" AND ")));
        }
        if !group_parts.is_empty() {
            sql.push_str(&format!(" GROUP BY {}", group_parts.join(", ")));
        }
        if !order_parts.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", order_parts.join(", ")));
        }
        if let Some(n) = limit_clause {
            sql.push_str(&format!(" LIMIT {}", n));
        }
        sql
    }

    pub fn collect(&self) -> Result<String, String> {
        let sql = self.build_sql();
        let sess = unsafe {
            sess_ref(self.session).ok_or_else(|| "null session pointer".to_string())?
        };
        let block = sess.ctx.query(&sql).map_err(|e| e.to_string())?;
        Ok(datablock_to_json(&block))
    }

    pub fn show(&self) -> String {
        let sql = self.build_sql();
        let sess = match unsafe { sess_ref(self.session) } {
            Some(s) => s,
            None => return "ERROR: null session".to_string(),
        };
        let block = match sess.ctx.query(&sql) {
            Ok(b) => b,
            Err(e) => return format!("ERROR: {}", e),
        };
        format_table(&block)
    }
}

fn format_table(block: &DataBlock) -> String {
    if block.columns.is_empty() {
        return "(empty)".to_string();
    }

    let headers: Vec<String> = block
        .columns
        .iter()
        .map(|c| {
            c.name
                .rfind('.')
                .map(|i| c.name[i + 1..].to_string())
                .unwrap_or_else(|| c.name.clone())
        })
        .collect();

    let mut col_data: Vec<Vec<String>> = Vec::new();
    for col in &block.columns {
        let mut vals = Vec::with_capacity(block.num_rows);
        for r in 0..block.num_rows {
            let v = col.data.get_value(r);
            vals.push(match v {
                kore_core::Value::Int(i) => i.to_string(),
                kore_core::Value::Float(f) => format!("{:.2}", f),
                kore_core::Value::Bool(b) => b.to_string(),
                kore_core::Value::Str(s) => s,
                kore_core::Value::Array(a) => format!("{:?}", a),
                kore_core::Value::Map(m) => format!("{:?}", m),
                kore_core::Value::Null => "NULL".to_string(),
            });
        }
        col_data.push(vals);
    }

    let widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(ci, h)| {
            let max_data = col_data[ci].iter().map(|v| v.len()).max().unwrap_or(0);
            h.len().max(max_data)
        })
        .collect();

    let mut out = String::new();
    let sep: String = widths.iter().map(|w| "-".repeat(*w + 2)).collect::<Vec<_>>().join("+");
    out.push('+');
    out.push_str(&sep);
    out.push_str("+\n");

    out.push('|');
    for (i, h) in headers.iter().enumerate() {
        out.push_str(&format!(" {:>width$} |", h, width = widths[i]));
    }
    out.push('\n');

    out.push('+');
    out.push_str(&sep);
    out.push_str("+\n");

    for r in 0..block.num_rows {
        out.push('|');
        for ci in 0..headers.len() {
            out.push_str(&format!(" {:>width$} |", col_data[ci][r], width = widths[ci]));
        }
        out.push('\n');
    }

    out.push('+');
    out.push_str(&sep);
    out.push_str("+\n");
    out
}

// ─── DataFrame C ABI ─────────────────────────────────────────────────────────

/// Create a new DataFrame bound to a session and table.
///
/// # Safety
/// `sess` must be a valid session pointer. `table_name` must be null-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn kore_dataframe_new(
    sess: *mut KoreSession,
    table_name: *const c_char,
) -> *mut KoreDataFrame {
    let tname = match CStr::from_ptr(table_name).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    Box::into_raw(Box::new(KoreDataFrame::new(sess, tname)))
}

/// Add a WHERE filter to the DataFrame. Returns a new DataFrame; frees the old one.
///
/// # Safety
/// `df` must be a valid DataFrame pointer. `predicate` must be null-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn kore_dataframe_filter(
    df: *mut KoreDataFrame,
    predicate: *const c_char,
) -> *mut KoreDataFrame {
    if df.is_null() {
        return std::ptr::null_mut();
    }
    let pred = match CStr::from_ptr(predicate).to_str() {
        Ok(s) => s,
        Err(_) => return df,
    };
    let old = Box::from_raw(df);
    Box::into_raw(Box::new(old.filter(pred)))
}

/// Set the SELECT columns. `cols_json` is a JSON array of column names, e.g. `["a","b"]`.
///
/// # Safety
/// `df` must be valid. `cols_json` must be null-terminated UTF-8 JSON array.
#[no_mangle]
pub unsafe extern "C" fn kore_dataframe_select(
    df: *mut KoreDataFrame,
    cols_json: *const c_char,
) -> *mut KoreDataFrame {
    if df.is_null() {
        return std::ptr::null_mut();
    }
    let json_str = match CStr::from_ptr(cols_json).to_str() {
        Ok(s) => s,
        Err(_) => return df,
    };
    let cols: Vec<String> = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return df,
    };
    let col_refs: Vec<&str> = cols.iter().map(|s| s.as_str()).collect();
    let old = Box::from_raw(df);
    Box::into_raw(Box::new(old.select(&col_refs)))
}

/// Set GROUP BY keys. `keys_json` is a JSON array of column names.
///
/// # Safety
/// `df` must be valid. `keys_json` must be null-terminated UTF-8 JSON array.
#[no_mangle]
pub unsafe extern "C" fn kore_dataframe_group_by(
    df: *mut KoreDataFrame,
    keys_json: *const c_char,
) -> *mut KoreDataFrame {
    if df.is_null() {
        return std::ptr::null_mut();
    }
    let json_str = match CStr::from_ptr(keys_json).to_str() {
        Ok(s) => s,
        Err(_) => return df,
    };
    let keys: Vec<String> = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return df,
    };
    let key_refs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
    let old = Box::from_raw(df);
    Box::into_raw(Box::new(old.group_by(&key_refs)))
}

/// Add aggregations. `agg_json` = `[["col","FUNC"],...]`.
///
/// # Safety
/// `df` must be a valid DataFrame pointer, `agg_json` a valid UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn kore_dataframe_agg(
    df: *mut KoreDataFrame,
    agg_json: *const c_char,
) -> *mut KoreDataFrame {
    if df.is_null() { return std::ptr::null_mut(); }
    let json_str = match CStr::from_ptr(agg_json).to_str() { Ok(s) => s, Err(_) => return df };
    let pairs: Vec<(String, String)> = match serde_json::from_str(json_str) { Ok(v) => v, Err(_) => return df };
    let refs: Vec<(&str, &str)> = pairs.iter().map(|(c, f)| (c.as_str(), f.as_str())).collect();
    let old = Box::from_raw(df);
    Box::into_raw(Box::new(old.agg(&refs)))
}

/// Join with another table. `join_json` = `{"table":"t","left":"col","right":"col","type":"INNER"}`.
///
/// # Safety
/// Both pointers must be valid.
#[no_mangle]
pub unsafe extern "C" fn kore_dataframe_join(
    df: *mut KoreDataFrame,
    join_json: *const c_char,
) -> *mut KoreDataFrame {
    if df.is_null() { return std::ptr::null_mut(); }
    let json_str = match CStr::from_ptr(join_json).to_str() { Ok(s) => s, Err(_) => return df };
    let v: serde_json::Value = match serde_json::from_str(json_str) { Ok(v) => v, Err(_) => return df };
    let table = v["table"].as_str().unwrap_or("");
    let left = v["left"].as_str().unwrap_or("");
    let right = v["right"].as_str().unwrap_or(left);
    let jtype = v["type"].as_str().unwrap_or("INNER");
    let old = Box::from_raw(df);
    Box::into_raw(Box::new(old.join_type(table, left, right, jtype)))
}

/// Set ORDER BY. `order_json` = `[["col", false], ...]` (col, desc).
///
/// # Safety
/// Both pointers must be valid.
#[no_mangle]
pub unsafe extern "C" fn kore_dataframe_order_by(
    df: *mut KoreDataFrame,
    order_json: *const c_char,
) -> *mut KoreDataFrame {
    if df.is_null() { return std::ptr::null_mut(); }
    let json_str = match CStr::from_ptr(order_json).to_str() { Ok(s) => s, Err(_) => return df };
    let pairs: Vec<(String, bool)> = match serde_json::from_str(json_str) { Ok(v) => v, Err(_) => return df };
    let mut old = Box::from_raw(df);
    for (col, desc) in &pairs {
        old = Box::new(old.order_by(col, *desc));
    }
    Box::into_raw(old)
}

/// Set LIMIT.
///
/// # Safety
/// `df` must be a valid DataFrame pointer.
#[no_mangle]
pub unsafe extern "C" fn kore_dataframe_limit(
    df: *mut KoreDataFrame,
    n: c_int,
) -> *mut KoreDataFrame {
    if df.is_null() { return std::ptr::null_mut(); }
    let old = Box::from_raw(df);
    Box::into_raw(Box::new(old.limit(n as usize)))
}

/// Execute the DataFrame query and return JSON. Caller must free with `kore_free_string`.
///
/// # Safety
/// `df` must be a valid DataFrame pointer.
#[no_mangle]
pub unsafe extern "C" fn kore_dataframe_collect(
    df: *mut KoreDataFrame,
) -> *mut c_char {
    if df.is_null() {
        return std::ptr::null_mut();
    }
    let df_ref = &*df;
    match df_ref.collect() {
        Ok(json) => match CString::new(json) {
            Ok(cs) => cs.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        Err(_) => std::ptr::null_mut(),
    }
}

/// Execute the DataFrame query and return a formatted table string.
/// Caller must free with `kore_free_string`.
///
/// # Safety
/// `df` must be a valid DataFrame pointer.
#[no_mangle]
pub unsafe extern "C" fn kore_dataframe_show(
    df: *mut KoreDataFrame,
) -> *mut c_char {
    if df.is_null() {
        return std::ptr::null_mut();
    }
    let df_ref = &*df;
    let table = df_ref.show();
    match CString::new(table) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free a DataFrame created by `kore_dataframe_new` or returned by chaining methods.
///
/// # Safety
/// `df` must have been returned by a `kore_dataframe_*` function and not yet freed.
#[no_mangle]
pub unsafe extern "C" fn kore_dataframe_free(df: *mut KoreDataFrame) {
    if !df.is_null() {
        drop(Box::from_raw(df));
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

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

    // ── DataFrame SQL builder tests ──────────────────────────────────────

    #[test]
    fn df_simple_select() {
        let df = KoreDataFrame::new(std::ptr::null_mut(), "orders");
        assert_eq!(df.build_sql(), "SELECT * FROM orders");
    }

    #[test]
    fn df_select_columns() {
        let df = KoreDataFrame::new(std::ptr::null_mut(), "orders")
            .select(&["id", "total"]);
        assert_eq!(df.build_sql(), "SELECT id, total FROM orders");
    }

    #[test]
    fn df_filter() {
        let df = KoreDataFrame::new(std::ptr::null_mut(), "orders")
            .filter("total > 100");
        assert_eq!(df.build_sql(), "SELECT * FROM orders WHERE total > 100");
    }

    #[test]
    fn df_multiple_filters() {
        let df = KoreDataFrame::new(std::ptr::null_mut(), "orders")
            .filter("total > 100")
            .filter("status = 'shipped'");
        assert_eq!(
            df.build_sql(),
            "SELECT * FROM orders WHERE total > 100 AND status = 'shipped'"
        );
    }

    #[test]
    fn df_group_by_agg() {
        let df = KoreDataFrame::new(std::ptr::null_mut(), "orders")
            .group_by(&["region"])
            .agg(&[("sales", "SUM"), ("cnt", "COUNT")]);
        assert_eq!(
            df.build_sql(),
            "SELECT region, SUM(sales) AS sum_sales, COUNT(cnt) AS count_cnt FROM orders GROUP BY region"
        );
    }

    #[test]
    fn df_order_by_limit() {
        let df = KoreDataFrame::new(std::ptr::null_mut(), "orders")
            .select(&["id", "total"])
            .order_by("total", true)
            .limit(10);
        assert_eq!(
            df.build_sql(),
            "SELECT id, total FROM orders ORDER BY total DESC LIMIT 10"
        );
    }

    #[test]
    fn df_join() {
        let df = KoreDataFrame::new(std::ptr::null_mut(), "orders")
            .join("customers", "customer_id", "id");
        assert_eq!(
            df.build_sql(),
            "SELECT * FROM orders INNER JOIN customers ON orders.customer_id = customers.id"
        );
    }

    #[test]
    fn df_full_chain() {
        let df = KoreDataFrame::new(std::ptr::null_mut(), "orders")
            .filter("year = 2024")
            .group_by(&["region"])
            .agg(&[("revenue", "SUM")])
            .order_by("sum_revenue", true)
            .limit(5);
        assert_eq!(
            df.build_sql(),
            "SELECT region, SUM(revenue) AS sum_revenue FROM orders WHERE year = 2024 GROUP BY region ORDER BY sum_revenue DESC LIMIT 5"
        );
    }

    #[test]
    fn df_left_join() {
        let df = KoreDataFrame::new(std::ptr::null_mut(), "orders")
            .join_type("customers", "customer_id", "id", "LEFT");
        assert_eq!(
            df.build_sql(),
            "SELECT * FROM orders LEFT JOIN customers ON orders.customer_id = customers.id"
        );
    }

    #[test]
    fn df_agg_without_group() {
        let df = KoreDataFrame::new(std::ptr::null_mut(), "orders")
            .agg(&[("total", "SUM")]);
        assert_eq!(
            df.build_sql(),
            "SELECT SUM(total) AS sum_total FROM orders"
        );
    }
}
