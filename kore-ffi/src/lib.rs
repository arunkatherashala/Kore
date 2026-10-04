//! KORE Layer 24 — C ABI for multi-language bindings.
//!
//! Exposes opaque handles for DataBlock and ML models via a stable C ABI.
//! Compile to:  cdylib → libkore_ffi.so / kore_ffi.dll / libkore_ffi.dylib
//!              staticlib → libkore_ffi.a / kore_ffi.lib
//!
//! Use the generated `include/kore.h` header to call from any C-compatible language.

#![allow(clippy::missing_safety_doc)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_int, c_longlong};

use kore_core::{Column, ColumnData, DataBlock};
use kore_join::{HashJoin, JoinConfig};
use kore_core::JoinType;
use kore_ml2::{GradientBoostingRegressor, RandomForestClassifier, RandomForestRegressor};
use kore_ml3::{KNearestNeighbors, LinearRegressor, LogisticRegressor, LinearSVM};
extern crate kore_io;
extern crate kore_parquet;

// ─── Error buffer ─────────────────────────────────────────────────────────────

thread_local! {
    static LAST_ERROR: std::cell::RefCell<Option<CString>> = std::cell::RefCell::new(None);
}

fn set_error(msg: impl Into<Vec<u8>>) {
    LAST_ERROR.with(|e| *e.borrow_mut() = Some(CString::new(msg).unwrap_or_default()));
}

/// Returns a pointer to the last error message, or NULL if no error.
/// The pointer is valid until the next KORE call on this thread.
#[no_mangle]
pub extern "C" fn kore_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ref().map_or(std::ptr::null(), |s| s.as_ptr()))
}

// ─── DataBlock handle ─────────────────────────────────────────────────────────

pub struct KoreBlock {
    pub inner: DataBlock,
}

/// Create an empty DataBlock.
#[no_mangle]
pub extern "C" fn kore_block_new() -> *mut KoreBlock {
    Box::into_raw(Box::new(KoreBlock {
        inner: DataBlock { columns: vec![], num_rows: 0 },
    }))
}

/// Free a DataBlock handle.
#[no_mangle]
pub unsafe extern "C" fn kore_block_free(ptr: *mut KoreBlock) {
    if !ptr.is_null() { drop(Box::from_raw(ptr)); }
}

/// Returns the number of rows in the block.
#[no_mangle]
pub unsafe extern "C" fn kore_block_num_rows(ptr: *const KoreBlock) -> u64 {
    if ptr.is_null() { return 0; }
    (*ptr).inner.num_rows as u64
}

/// Returns the number of columns in the block.
#[no_mangle]
pub unsafe extern "C" fn kore_block_num_cols(ptr: *const KoreBlock) -> u32 {
    if ptr.is_null() { return 0; }
    (*ptr).inner.columns.len() as u32
}

/// Add an f64 column to a block.
/// `data` is a pointer to `len` doubles (NaN = null).
#[no_mangle]
pub unsafe extern "C" fn kore_block_add_f64(
    ptr:  *mut KoreBlock,
    name: *const c_char,
    data: *const c_double,
    len:  u64,
) -> c_int {
    if ptr.is_null() || name.is_null() || data.is_null() { return -1; }
    let name = match CStr::from_ptr(name).to_str() { Ok(s) => s.to_string(), Err(_) => return -1 };
    let slice = std::slice::from_raw_parts(data, len as usize);
    let vals: Vec<Option<f64>> = slice.iter().map(|&v| if v.is_nan() { None } else { Some(v) }).collect();
    let num_rows = vals.len();
    (*ptr).inner.columns.push(Column { name, data: ColumnData::Float64(vals) });
    (*ptr).inner.num_rows = num_rows;
    0
}

/// Add an i64 column (i64::MIN = null sentinel).
#[no_mangle]
pub unsafe extern "C" fn kore_block_add_i64(
    ptr:  *mut KoreBlock,
    name: *const c_char,
    data: *const c_longlong,
    len:  u64,
) -> c_int {
    if ptr.is_null() || name.is_null() || data.is_null() { return -1; }
    let name = match CStr::from_ptr(name).to_str() { Ok(s) => s.to_string(), Err(_) => return -1 };
    let slice = std::slice::from_raw_parts(data, len as usize);
    let vals: Vec<Option<i64>> = slice.iter()
        .map(|&v| if v == i64::MIN { None } else { Some(v) }).collect();
    let num_rows = vals.len();
    (*ptr).inner.columns.push(Column { name, data: ColumnData::Int64(vals) });
    (*ptr).inner.num_rows = num_rows;
    0
}

/// Read f64 column values into a caller-provided buffer.  Returns number of values written.
#[no_mangle]
pub unsafe extern "C" fn kore_block_get_f64(
    ptr:    *const KoreBlock,
    col:    *const c_char,
    out:    *mut c_double,
    maxlen: u64,
) -> i64 {
    if ptr.is_null() || col.is_null() || out.is_null() { return -1; }
    let col_name = match CStr::from_ptr(col).to_str() { Ok(s) => s, Err(_) => return -1 };
    let block = &(*ptr).inner;
    let column = match block.columns.iter().find(|c| c.name == col_name) {
        Some(c) => c, None => { set_error(format!("column not found: {col_name}")); return -1; }
    };
    match &column.data {
        ColumnData::Float64(v) => {
            let n = v.len().min(maxlen as usize);
            let out_slice = std::slice::from_raw_parts_mut(out, n);
            for (i, val) in v[..n].iter().enumerate() {
                out_slice[i] = val.unwrap_or(f64::NAN);
            }
            n as i64
        }
        _ => { set_error("column is not f64"); -1 }
    }
}

// ─── HashJoin ─────────────────────────────────────────────────────────────────

/// Perform a HashJoin. join_type: 0=inner, 1=left, 2=full.
/// Returns a new KoreBlock (caller owns it — must call kore_block_free).
#[no_mangle]
pub unsafe extern "C" fn kore_hash_join(
    left:       *const KoreBlock,
    right:      *const KoreBlock,
    left_key:   *const c_char,
    right_key:  *const c_char,
    join_type:  c_int,
) -> *mut KoreBlock {
    if left.is_null() || right.is_null() || left_key.is_null() || right_key.is_null() {
        set_error("null pointer");
        return std::ptr::null_mut();
    }
    let lk = match CStr::from_ptr(left_key).to_str()  { Ok(s) => s.to_string(), Err(_) => { set_error("bad lk"); return std::ptr::null_mut(); } };
    let rk = match CStr::from_ptr(right_key).to_str() { Ok(s) => s.to_string(), Err(_) => { set_error("bad rk"); return std::ptr::null_mut(); } };
    let jt = match join_type { 1 => JoinType::Left, 2 => JoinType::Full, _ => JoinType::Inner };
    let cfg = JoinConfig { left_key: lk, right_key: rk, join_type: jt };
    match HashJoin::join(&(*left).inner, &(*right).inner, &cfg) {
        Ok(block) => Box::into_raw(Box::new(KoreBlock { inner: block })),
        Err(e)    => { set_error(format!("{e}")); std::ptr::null_mut() }
    }
}

// ─── ML model handle ──────────────────────────────────────────────────────────

pub enum KoreModelInner {
    RfReg(RandomForestRegressor),
    RfClf(RandomForestClassifier),
    GBM(GradientBoostingRegressor),
    LinReg(LinearRegressor),
    Logistic(LogisticRegressor),
    KNN(KNearestNeighbors),
    SVM(LinearSVM),
}

pub struct KoreModel {
    pub inner: KoreModelInner,
}

/// model_type: 0=RF-reg  1=RF-clf  2=GBM  3=LinReg  4=Logistic  5=KNN-reg  6=KNN-clf  7=SVM
#[no_mangle]
pub extern "C" fn kore_model_new(model_type: c_int, param1: c_int, param2: c_int) -> *mut KoreModel {
    let inner = match model_type {
        0 => KoreModelInner::RfReg(RandomForestRegressor::new(param1 as usize, param2 as usize)),
        1 => KoreModelInner::RfClf(RandomForestClassifier::new(param1 as usize, param2 as usize)),
        2 => KoreModelInner::GBM(GradientBoostingRegressor::new(param1 as usize, 0.1, param2 as usize)),
        3 => KoreModelInner::LinReg(LinearRegressor::new(1e-8)),
        4 => KoreModelInner::Logistic(LogisticRegressor::new(0.1, param1 as usize, 32, 1e-4)),
        5 => KoreModelInner::KNN(KNearestNeighbors::new_regressor(param1 as usize)),
        6 => KoreModelInner::KNN(KNearestNeighbors::new_classifier(param1 as usize)),
        7 => KoreModelInner::SVM(LinearSVM::new(0.01, param1 as usize)),
        _ => { set_error(format!("unknown model_type {model_type}")); return std::ptr::null_mut(); }
    };
    Box::into_raw(Box::new(KoreModel { inner }))
}

#[no_mangle]
pub unsafe extern "C" fn kore_model_free(ptr: *mut KoreModel) {
    if !ptr.is_null() { drop(Box::from_raw(ptr)); }
}

/// Fit a model.  x_flat is a row-major flat array of n_rows×n_cols doubles.
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn kore_model_fit(
    model:  *mut KoreModel,
    x_flat: *const c_double,
    n_rows: u64,
    n_cols: u64,
    y:      *const c_double,
) -> c_int {
    if model.is_null() || x_flat.is_null() || y.is_null() { return -1; }
    let nr = n_rows as usize;
    let nc = n_cols as usize;
    let x_raw = std::slice::from_raw_parts(x_flat, nr * nc);
    let y_raw = std::slice::from_raw_parts(y, nr);
    let x: Vec<Vec<f64>> = (0..nr).map(|i| x_raw[i*nc..(i+1)*nc].to_vec()).collect();
    let y_vec: Vec<f64>  = y_raw.to_vec();
    let m = &mut (*model).inner;
    match m {
        KoreModelInner::RfReg(m)   => m.fit_raw(&x, &y_vec),
        KoreModelInner::RfClf(m)   => m.fit_raw(&x, &y_vec),
        KoreModelInner::GBM(m)     => m.fit_raw(&x, &y_vec),
        KoreModelInner::LinReg(m)  => m.fit_raw(&x, &y_vec),
        KoreModelInner::Logistic(m)=> m.fit_raw(&x, &y_vec),
        KoreModelInner::KNN(m)     => m.fit_raw(&x, &y_vec),
        KoreModelInner::SVM(m)     => m.fit_raw(&x, &y_vec),
    }
    0
}

/// Predict.  out must have space for n_rows doubles.  Returns 0 on success.
#[no_mangle]
pub unsafe extern "C" fn kore_model_predict(
    model:  *const KoreModel,
    x_flat: *const c_double,
    n_rows: u64,
    n_cols: u64,
    out:    *mut c_double,
) -> c_int {
    if model.is_null() || x_flat.is_null() || out.is_null() { return -1; }
    let nr = n_rows as usize;
    let nc = n_cols as usize;
    let x_raw = std::slice::from_raw_parts(x_flat, nr * nc);
    let x: Vec<Vec<f64>> = (0..nr).map(|i| x_raw[i*nc..(i+1)*nc].to_vec()).collect();
    let preds = match &(*model).inner {
        KoreModelInner::RfReg(m)   => m.predict_raw(&x),
        KoreModelInner::RfClf(m)   => m.predict_raw(&x),
        KoreModelInner::GBM(m)     => m.predict_raw(&x),
        KoreModelInner::LinReg(m)  => m.predict_raw(&x),
        KoreModelInner::Logistic(m)=> m.predict_raw(&x),
        KoreModelInner::KNN(m)     => m.predict_raw(&x),
        KoreModelInner::SVM(m)     => m.predict_raw(&x),
    };
    let out_slice = std::slice::from_raw_parts_mut(out, nr);
    out_slice.copy_from_slice(&preds);
    0
}

// ══════════════════════════════════════════════════════════════════════════════
//  SQL SESSION API  —  Universal query interface for all language bindings
//  Same logic as kore-python but exposed through the unified kore_ffi library.
//  All 7 languages (Python, Java, Node.js, Go, C#, R, Ruby) use these calls.
// ══════════════════════════════════════════════════════════════════════════════

use kore_sql::KqlContext;

pub struct KoreSession {
    ctx: KqlContext,
}

/// Create a new SQL session.  Returns an opaque handle; free with kore_session_free.
#[no_mangle]
pub extern "C" fn kore_session_new() -> *mut KoreSession {
    Box::into_raw(Box::new(KoreSession { ctx: KqlContext::new() }))
}

/// Free a session created by kore_session_new.
#[no_mangle]
pub unsafe extern "C" fn kore_session_free(ptr: *mut KoreSession) {
    if !ptr.is_null() { drop(Box::from_raw(ptr)); }
}

/// Load a CSV file as a named table.  Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn kore_session_load_csv(
    sess:  *mut KoreSession,
    table: *const c_char,
    path:  *const c_char,
) -> c_int {
    let s = match (ptr_to_str(table), ptr_to_str(path)) {
        (Some(t), Some(p)) => (t, p),
        _ => { set_error("null pointer in kore_session_load_csv"); return -1; }
    };
    if sess.is_null() { set_error("null session"); return -1; }
    match load_csv_into(&mut (*sess).ctx, s.0, s.1) {
        Ok(_)  => 0,
        Err(e) => { set_error(e); -1 }
    }
}

/// Load a .kore native binary file as a named table. Returns 0 on success, -1 on error.
/// The .kore format is KORE's own columnar binary format — much faster to load than CSV.
#[no_mangle]
pub unsafe extern "C" fn kore_session_load_kore(
    sess:  *mut KoreSession,
    table: *const c_char,
    path:  *const c_char,
) -> c_int {
    let s = match (ptr_to_str(table), ptr_to_str(path)) {
        (Some(t), Some(p)) => (t, p),
        _ => { set_error("null pointer in kore_session_load_kore"); return -1; }
    };
    if sess.is_null() { set_error("null session"); return -1; }
    match kore_io::KoreReader::read_file(std::path::Path::new(s.1)) {
        Ok(block) => { (*sess).ctx.register(s.0, block); 0 }
        Err(e) => { set_error(format!("{e}")); -1 }
    }
}

/// Save a registered table to .kore native binary format. Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn kore_session_save_kore(
    sess:  *const KoreSession,
    table: *const c_char,
    path:  *const c_char,
) -> c_int {
    let s = match (ptr_to_str(table), ptr_to_str(path)) {
        (Some(t), Some(p)) => (t, p),
        _ => { set_error("null pointer in kore_session_save_kore"); return -1; }
    };
    if sess.is_null() { set_error("null session"); return -1; }
    match (*sess).ctx.get(s.0) {
        None => { set_error(format!("table not found: {}", s.0)); -1 }
        Some(block) => {
            match kore_io::KoreWriter::write_file(std::path::Path::new(s.1), block) {
                Ok(_)  => 0,
                Err(e) => { set_error(format!("{e}")); -1 }
            }
        }
    }
}

/// Load an Apache Parquet file as a named table. Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn kore_session_load_parquet(
    sess:  *mut KoreSession,
    table: *const c_char,
    path:  *const c_char,
) -> c_int {
    let s = match (ptr_to_str(table), ptr_to_str(path)) {
        (Some(t), Some(p)) => (t, p),
        _ => { set_error("null pointer in kore_session_load_parquet"); return -1; }
    };
    if sess.is_null() { set_error("null session"); return -1; }
    match kore_parquet::ParquetReader::new(s.1).read() {
        Ok(block) => { (*sess).ctx.register(s.0, block); 0 }
        Err(e) => { set_error(format!("{e}")); -1 }
    }
}

/// Register a DataBlock as a named table inside a session.
/// The session takes a COPY of the block data.
#[no_mangle]
pub unsafe extern "C" fn kore_session_register_block(
    sess:  *mut KoreSession,
    table: *const c_char,
    block: *const KoreBlock,
) -> c_int {
    if sess.is_null() || block.is_null() { return -1; }
    let name = match ptr_to_str(table) { Some(s) => s, None => return -1 };
    (*sess).ctx.register(name, (*block).inner.clone());
    0
}

/// Execute a SQL query and return the result as a JSON UTF-8 string.
/// The caller MUST free the returned string with kore_free_string.
/// Returns NULL on error (check kore_last_error()).
#[no_mangle]
pub unsafe extern "C" fn kore_session_query(
    sess: *mut KoreSession,
    sql:  *const c_char,
) -> *mut c_char {
    if sess.is_null() { set_error("null session"); return std::ptr::null_mut(); }
    let sql_str = match ptr_to_str(sql) { Some(s) => s, None => { set_error("null sql"); return std::ptr::null_mut(); } };
    match kore_sql::query(sql_str, &(*sess).ctx) {
        Ok(block)  => {
            let json = block_to_json_stripped(&block);
            drop(block);
            // moves the buffer into the CString (no second copy); fails only on an interior NUL, which
            // serde_json never emits (it escapes control characters)
            CString::new(json).map(|cs| cs.into_raw()).unwrap_or(std::ptr::null_mut())
        }
        Err(e) => { set_error(format!("{e}")); std::ptr::null_mut() }
    }
}

/// Return the row count of a registered table, or -1 if not found.
#[no_mangle]
pub unsafe extern "C" fn kore_session_row_count(
    sess:  *const KoreSession,
    table: *const c_char,
) -> i64 {
    if sess.is_null() { return -1; }
    let name = match ptr_to_str(table) { Some(s) => s, None => return -1 };
    (*sess).ctx.get(name).map(|b| b.num_rows as i64).unwrap_or(-1)
}

/// Free a string returned by kore_session_query.
#[no_mangle]
pub unsafe extern "C" fn kore_free_string(s: *mut c_char) {
    if !s.is_null() { drop(CString::from_raw(s)); }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

unsafe fn ptr_to_str<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() { return None; }
    CStr::from_ptr(p).to_str().ok()
}

/// Split one CSV record. Fields may be double-quoted (commas inside, `""` for a quote); unquoted
/// fields are trimmed. Records do not span lines.
fn split_csv<'a>(line: &'a str, out: &mut Vec<std::borrow::Cow<'a, str>>) {
    use std::borrow::Cow;
    out.clear();
    let b = line.as_bytes();
    let mut i = 0;
    loop {
        if i < b.len() && b[i] == b'"' {
            let mut j = i + 1;
            let mut escaped = false;
            loop {
                match b[j.min(b.len())..].iter().position(|&c| c == b'"') {
                    None => { j = b.len(); break; }
                    Some(p) => {
                        j += p;
                        if j + 1 < b.len() && b[j + 1] == b'"' { escaped = true; j += 2; continue; }
                        break;
                    }
                }
            }
            let inner = &line[i + 1..j.min(b.len())];
            out.push(if escaped { Cow::Owned(inner.replace("\"\"", "\"")) } else { Cow::Borrowed(inner) });
            let mut k = (j + 1).min(b.len());
            while k < b.len() && b[k] != b',' { k += 1; }
            i = k;
        } else {
            let end = b[i.min(b.len())..].iter().position(|&c| c == b',').map_or(b.len(), |p| i + p);
            out.push(Cow::Borrowed(line[i.min(b.len())..end].trim()));
            i = end;
        }
        if i >= b.len() { break; }
        i += 1;
        if i == b.len() { out.push(Cow::Borrowed("")); break; }
    }
}

fn load_csv_into(ctx: &mut KqlContext, table_name: &str, path: &str) -> Result<(), String> {
    use std::io::{BufRead, BufReader};
    use std::collections::HashMap;

    // ── Pass 1: read header + 2 000 sample rows → determine column types ──────
    let file1 = std::fs::File::open(path).map_err(|e| format!("open {path}: {e}"))?;
    let mut r1 = BufReader::new(file1);

    let mut hdr = String::new();
    r1.read_line(&mut hdr).map_err(|e| e.to_string())?;
    let headers: Vec<String> = {
        let mut f = Vec::new();
        split_csv(hdr.trim_end(), &mut f);
        f.iter().map(|h| h.to_string()).collect()
    };
    let nc = headers.len();

    const SAMPLE: usize = 2_000;
    // For each column: track if all non-empty values parse as i64, f64, and cardinality.
    let mut all_i64:   Vec<bool> = vec![true; nc];
    let mut all_f64:   Vec<bool> = vec![true; nc];
    let mut seen:      Vec<std::collections::HashSet<u64>> = (0..nc).map(|_| Default::default()).collect();
    let mut seen_many: Vec<bool> = vec![false; nc];  // >255 unique
    let mut any_val:   Vec<bool> = vec![false; nc];
    let mut nsampled = 0usize;

    let hdr_bytes = hdr.len();
    let mut sample_bytes = 0usize;
    let mut sample_eof = false;
    let mut line = String::new();
    while nsampled < SAMPLE {
        line.clear();
        if r1.read_line(&mut line).map_err(|e| e.to_string())? == 0 { sample_eof = true; break; }
        sample_bytes += line.len();
        let trimmed = line.trim_end();
        if trimmed.is_empty() { continue; }
        let mut vals = Vec::with_capacity(nc);
        split_csv(trimmed, &mut vals);
        for i in 0..nc {
            let v: &str = vals.get(i).map(|s| s.as_ref()).unwrap_or("");
            if v.is_empty() { continue; }
            any_val[i] = true;
            if all_i64[i] && v.parse::<i64>().is_err() { all_i64[i] = false; }
            if all_f64[i] && v.parse::<f64>().is_err() { all_f64[i] = false; }
            if !seen_many[i] {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                v.hash(&mut h);
                seen[i].insert(h.finish());
                if seen[i].len() > 255 { seen_many[i] = true; }
            }
        }
        nsampled += 1;
    }

    #[derive(Clone, PartialEq)]
    enum CT { Int64, Float64, StrDict, Str }
    let mut col_types: Vec<CT> = (0..nc).map(|i| {
        if !any_val[i]       { return CT::Str; }
        if all_i64[i]        { return CT::Int64; }
        if all_f64[i]        { return CT::Float64; }
        if !seen_many[i]     { return CT::StrDict; }
        CT::Str
    }).collect();

    // ── Pass 2: read full file directly into typed columns ───────────────────
    // No intermediate String storage for numeric columns — direct parse.
    let file2 = std::fs::File::open(path).map_err(|e| format!("open {path}: {e}"))?;
    let mut r2 = BufReader::new(file2);
    let mut skip = String::new();
    r2.read_line(&mut skip).map_err(|e| e.to_string())?;   // skip header

    // Size every column once from the file length and the sampled line length. A fixed small capacity
    // that doubles on demand leaves up to 2x slack per column (and copies the column on each growth).
    let cap = if sample_eof || nsampled == 0 {
        nsampled
    } else {
        let file_len = std::fs::metadata(path).map(|m| m.len() as usize).unwrap_or(0);
        let avg = (sample_bytes as f64 / nsampled as f64).max(1.0);
        let est = (file_len.saturating_sub(hdr_bytes) as f64 / avg * 1.03) as usize;
        est.max(nsampled) + 1024
    };
    let mut i64_cols:  Vec<Vec<Option<i64>>>    = (0..nc).map(|i| if col_types[i]==CT::Int64   { Vec::with_capacity(cap) } else { vec![] }).collect();
    let mut f64_cols:  Vec<Vec<Option<f64>>>    = (0..nc).map(|i| if col_types[i]==CT::Float64 { Vec::with_capacity(cap) } else { vec![] }).collect();
    let mut str_cols:  Vec<Vec<Option<String>>> = (0..nc).map(|i| if col_types[i]==CT::Str     { Vec::with_capacity(cap) } else { vec![] }).collect();
    // StrDict state per column
    let mut sd_codes:  Vec<Vec<u8>>             = (0..nc).map(|i| if col_types[i]==CT::StrDict { Vec::with_capacity(cap) } else { vec![] }).collect();
    let mut sd_dict:   Vec<Vec<String>>         = vec![vec![]; nc];
    let mut sd_map:    Vec<HashMap<String, u8>> = vec![HashMap::new(); nc];

    let mut line2 = String::new();
    let mut nr = 0usize;
    loop {
        line2.clear();
        if r2.read_line(&mut line2).map_err(|e| e.to_string())? == 0 { break; }
        let trimmed = line2.trim_end();
        if trimmed.is_empty() { continue; }
        nr += 1;
        let mut vals = Vec::with_capacity(nc);
        split_csv(trimmed, &mut vals);
        for i in 0..nc {
            let v: &str = vals.get(i).map(|s| s.as_ref()).unwrap_or("");
            match col_types[i] {
                CT::Int64   => i64_cols[i].push(if v.is_empty() { None } else { v.parse().ok() }),
                CT::Float64 => f64_cols[i].push(if v.is_empty() { None } else { v.parse().ok() }),
                CT::Str     => str_cols[i].push(if v.is_empty() { None } else { Some(v.to_string()) }),
                CT::StrDict => {
                    if v.is_empty() {
                        sd_codes[i].push(u8::MAX);
                    } else if let Some(&code) = sd_map[i].get(v) {
                        sd_codes[i].push(code);
                    } else if sd_dict[i].len() < 255 {
                        let code = sd_dict[i].len() as u8;
                        sd_map[i].insert(v.to_string(), code);
                        sd_dict[i].push(v.to_string());
                        sd_codes[i].push(code);
                    } else {
                        // more distinct values than the sample suggested: this column is plain text after all.
                        // Rebuild what was read so far as strings and carry on in Str mode (never reuse a code).
                        let mut all: Vec<Option<String>> = sd_codes[i].iter()
                            .map(|&c| if c == u8::MAX { None } else { Some(sd_dict[i][c as usize].clone()) })
                            .collect();
                        all.push(Some(v.to_string()));
                        str_cols[i] = all;
                        sd_codes[i] = Vec::new();
                        sd_dict[i].clear();
                        sd_map[i].clear();
                        col_types[i] = CT::Str;
                    }
                }
            }
        }
    }

    // ── Build DataBlock ───────────────────────────────────────────────────────
    let mut columns = vec![];
    for (i, name) in headers.iter().enumerate() {
        let data = match col_types[i] {
            CT::Int64   => { let mut v = std::mem::take(&mut i64_cols[i]); v.shrink_to_fit(); ColumnData::Int64(v) }
            CT::Float64 => { let mut v = std::mem::take(&mut f64_cols[i]); v.shrink_to_fit(); ColumnData::Float64(v) }
            CT::Str     => { let mut v = std::mem::take(&mut str_cols[i]); v.shrink_to_fit(); ColumnData::Str(v) }
            CT::StrDict => {
                let mut codes = std::mem::take(&mut sd_codes[i]);
                codes.shrink_to_fit();
                ColumnData::StrDict { codes, dict: std::mem::take(&mut sd_dict[i]) }
            }
        };
        columns.push(Column { name: name.clone(), data });
    }
    ctx.register(table_name, DataBlock { columns, num_rows: nr });
    Ok(())
}

/// Stream the rows straight into one buffer (no per-row serde_json maps). Keys are written in sorted
/// order, exactly what the map-based path produced. Falls back to that path if stripped names collide.
fn block_to_json_stripped(block: &DataBlock) -> Vec<u8> {
    let mut keys: Vec<(String, usize)> = block.columns.iter().enumerate()
        .map(|(i, col)| (col.name.rfind('.').map(|p| &col.name[p+1..]).unwrap_or(&col.name).to_string(), i))
        .collect();
    keys.sort();
    if keys.windows(2).any(|w| w[0].0 == w[1].0) {
        return block_to_json_via_map(block).into_bytes();
    }
    let key_json: Vec<Vec<u8>> = keys.iter().map(|(k, _)| {
        let mut b = serde_json::to_vec(k).unwrap_or_else(|_| b"\"\"".to_vec());
        b.push(b':');
        b
    }).collect();
    let mut out: Vec<u8> = Vec::with_capacity(64 + block.num_rows * 16 * keys.len().max(1));
    out.push(b'[');
    for r in 0..block.num_rows {
        if r > 0 { out.push(b','); }
        out.push(b'{');
        for (n, (_, ci)) in keys.iter().enumerate() {
            if n > 0 { out.push(b','); }
            out.extend_from_slice(&key_json[n]);
            match block.columns[*ci].data.get_value(r) {
                kore_core::Value::Int(i)   => { let _ = serde_json::to_writer(&mut out, &i); }
                kore_core::Value::Float(f) => { let _ = serde_json::to_writer(&mut out, &f); }
                kore_core::Value::Bool(b)  => { let _ = serde_json::to_writer(&mut out, &b); }
                kore_core::Value::Str(s)   => { let _ = serde_json::to_writer(&mut out, &s); }
                kore_core::Value::Array(_) => out.extend_from_slice(b"[]"),
                kore_core::Value::Map(_)   => out.extend_from_slice(b"{}"),
                kore_core::Value::Null     => out.extend_from_slice(b"null"),
            }
        }
        out.push(b'}');
    }
    out.push(b']');
    out
}

fn block_to_json_via_map(block: &DataBlock) -> String {
    let mut rows = vec![];
    for r in 0..block.num_rows {
        let mut obj = serde_json::Map::new();
        for col in &block.columns {
            let key = col.name.rfind('.').map(|i| &col.name[i+1..]).unwrap_or(&col.name).to_string();
            let val = col.data.get_value(r);
            let jv = match val {
                kore_core::Value::Int(i)   => serde_json::json!(i),
                kore_core::Value::Float(f) => serde_json::json!(f),
                kore_core::Value::Bool(b)  => serde_json::json!(b),
                kore_core::Value::Str(s)   => serde_json::json!(s),
                kore_core::Value::Array(_) => serde_json::json!([]),
                kore_core::Value::Map(_)   => serde_json::json!({}),
                kore_core::Value::Null     => serde_json::Value::Null,
            };
            obj.insert(key, jv);
        }
        rows.push(serde_json::Value::Object(obj));
    }
    serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into())
}

#[cfg(test)]
mod csv_loader_tests {
    use super::*;

    #[test]
    fn csv_loader_never_replaces_values_and_handles_quoting() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!("kore_csv_loader_{}.csv", std::process::id()));
        {
            let mut f = std::fs::File::create(&path).unwrap();
            writeln!(f, "id,name,grp,note").unwrap();
            for i in 1..=3000 {
                // names share a 16-byte prefix; grp looks low-cardinality for 2500 rows, then explodes
                let grp = if i <= 2500 { format!("g{}", i % 5) } else { format!("late-{i}") };
                let note = match i % 3 { 0 => "\"a, b\"".to_string(), 1 => "\"say \"\"hi\"\"\"".to_string(), _ => String::new() };
                writeln!(f, "{i},\"Customer#{i:09}\",{grp},{note}").unwrap();
            }
        }
        let mut ctx = KqlContext::new();
        load_csv_into(&mut ctx, "t", path.to_str().unwrap()).unwrap();
        let text = |sql: &str| -> Option<String> {
            let b = ctx.query(sql).unwrap();
            match &b.columns[0].data {
                ColumnData::Str(v) => v[0].clone(),
                ColumnData::StrDict { codes, dict } => dict.get(codes[0] as usize).cloned(),
                other => panic!("unexpected column type {other:?}"),
            }
        };
        assert_eq!(text("select name from t where id = 1").as_deref(), Some("Customer#000000001"));
        assert_eq!(text("select name from t where id = 2999").as_deref(), Some("Customer#000002999"));
        let distinct = ctx.query("select count(distinct name) as d from t").unwrap();
        // COUNT is a whole number (BIGINT in Spark); older builds returned it as DOUBLE
        assert!(matches!(&distinct.columns[0].data, ColumnData::Int64(v) if v[0] == Some(3000))
                || matches!(&distinct.columns[0].data, ColumnData::Float64(v) if v[0] == Some(3000.0)),
                "all 3000 names must stay distinct");
        assert_eq!(text("select grp from t where id = 2999").as_deref(), Some("late-2999"));
        assert_eq!(text("select grp from t where id = 7").as_deref(), Some("g2"));
        assert_eq!(text("select note from t where id = 3").as_deref(), Some("a, b"));
        assert_eq!(text("select note from t where id = 4").as_deref(), Some("say \"hi\""));
        std::fs::remove_file(path).ok();
    }
}

