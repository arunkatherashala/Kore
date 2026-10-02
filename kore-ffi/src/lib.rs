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

/// Add a string column. `data` is an array of C string pointers (NULL = null).
#[no_mangle]
pub unsafe extern "C" fn kore_block_add_str(
    ptr:  *mut KoreBlock,
    name: *const c_char,
    data: *const *const c_char,
    len:  u64,
) -> c_int {
    if ptr.is_null() || name.is_null() || data.is_null() { return -1; }
    let name = match CStr::from_ptr(name).to_str() { Ok(s) => s.to_string(), Err(_) => return -1 };
    let ptrs = std::slice::from_raw_parts(data, len as usize);
    let vals: Vec<Option<String>> = ptrs.iter().map(|&p| {
        if p.is_null() { None }
        else { CStr::from_ptr(p).to_str().ok().map(|s| s.to_string()) }
    }).collect();
    let num_rows = vals.len();
    (*ptr).inner.columns.push(Column { name, data: ColumnData::Str(vals) });
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

/// Read a string column value at index. Returns a C string (caller must free with kore_free_string).
#[no_mangle]
pub unsafe extern "C" fn kore_block_get_str(
    ptr: *const KoreBlock,
    col: *const c_char,
    idx: u64,
) -> *mut c_char {
    if ptr.is_null() || col.is_null() { return std::ptr::null_mut(); }
    let col_name = match CStr::from_ptr(col).to_str() { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let block = &(*ptr).inner;
    let column = match block.columns.iter().find(|c| c.name == col_name) {
        Some(c) => c, None => return std::ptr::null_mut(),
    };
    let i = idx as usize;
    let s = match &column.data {
        ColumnData::Str(v) => v.get(i).and_then(|o| o.as_ref()),
        ColumnData::StrDict { codes, dict } => codes.get(i).and_then(|&c| if c == u8::MAX { None } else { dict.get(c as usize) }),
        _ => None,
    };
    match s {
        Some(val) => CString::new(val.as_str()).map(|cs| cs.into_raw()).unwrap_or(std::ptr::null_mut()),
        None => std::ptr::null_mut(),
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

// ── File I/O — kore_write_file / kore_read_file / kore_crc32 ─────────────────

/// Write a DataBlock to a .kore binary file.
/// Returns 0 on success, -1 on error (call kore_last_error() for message).
#[no_mangle]
pub unsafe extern "C" fn kore_write_file(
    path: *const c_char,
    block: *const KoreBlock,
) -> c_int {
    let path = match ptr_to_str(path) {
        Some(p) => p,
        None => { set_error("kore_write_file: null path"); return -1; }
    };
    let block = match block.as_ref() {
        Some(b) => &b.inner,
        None    => { set_error("kore_write_file: null block"); return -1; }
    };
    match kore_store::KoreWriter::write_file(std::path::Path::new(path), block) {
        Ok(_)  => 0,
        Err(e) => { set_error(e.to_string()); -1 }
    }
}

/// Read a .kore binary file into a new DataBlock handle.
/// Returns NULL on error (call kore_last_error() for message).
/// Caller must free with kore_block_free().
#[no_mangle]
pub unsafe extern "C" fn kore_read_file(path: *const c_char) -> *mut KoreBlock {
    let path = match ptr_to_str(path) {
        Some(p) => p,
        None => { set_error("kore_read_file: null path"); return std::ptr::null_mut(); }
    };
    match kore_store::reader::KoreReader::read_file(std::path::Path::new(path)) {
        Ok(block)  => Box::into_raw(Box::new(KoreBlock { inner: block })),
        Err(e)     => { set_error(e.to_string()); std::ptr::null_mut() }
    }
}

/// Write a DataBlock to bytes in memory.
/// Returns a malloc'd byte buffer; caller must free with kore_free_bytes().
/// Sets *out_len to the number of bytes written.
#[no_mangle]
pub unsafe extern "C" fn kore_write_bytes(
    block: *const KoreBlock,
    out_len: *mut usize,
) -> *mut u8 {
    let block = match block.as_ref() {
        Some(b) => &b.inner,
        None    => { set_error("kore_write_bytes: null block"); return std::ptr::null_mut(); }
    };
    let bytes = kore_store::KoreWriter::to_bytes(block);
    let len = bytes.len();
    if !out_len.is_null() { *out_len = len; }
    let mut v = bytes.into_boxed_slice();
    let ptr = v.as_mut_ptr();
    std::mem::forget(v);
    ptr
}

/// Read a DataBlock from a byte buffer.
/// Returns NULL on error. Caller must free with kore_block_free().
#[no_mangle]
pub unsafe extern "C" fn kore_read_bytes(
    data: *const u8,
    len: usize,
) -> *mut KoreBlock {
    if data.is_null() { set_error("kore_read_bytes: null data"); return std::ptr::null_mut(); }
    let slice = std::slice::from_raw_parts(data, len);
    match kore_store::reader::KoreReader::from_bytes(slice) {
        Ok(block)  => Box::into_raw(Box::new(KoreBlock { inner: block })),
        Err(e)     => { set_error(e.to_string()); std::ptr::null_mut() }
    }
}

/// Encrypt bytes with AES-256-GCM (PBKDF2 key). Returns a buffer to free with kore_free_bytes.
#[no_mangle]
pub unsafe extern "C" fn kore_encrypt_bytes(
    password: *const u8, pw_len: usize,
    data: *const u8, len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    if password.is_null() || data.is_null() || out_len.is_null() {
        set_error("kore_encrypt_bytes: null argument");
        return std::ptr::null_mut();
    }
    let r = kore_store::KoreWriter::encrypt_blob(
        std::slice::from_raw_parts(data, len),
        std::slice::from_raw_parts(password, pw_len));
    leak_result(r.map_err(|e| e.to_string()), out_len)
}

/// Decrypt a buffer produced by kore_encrypt_bytes. Returns NULL on wrong password or corrupt data.
#[no_mangle]
pub unsafe extern "C" fn kore_decrypt_bytes(
    password: *const u8, pw_len: usize,
    data: *const u8, len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    if password.is_null() || data.is_null() || out_len.is_null() {
        set_error("kore_decrypt_bytes: null argument");
        return std::ptr::null_mut();
    }
    let r = kore_store::reader::KoreReader::decrypt_blob(
        std::slice::from_raw_parts(data, len),
        std::slice::from_raw_parts(password, pw_len));
    leak_result(r.map_err(|e| e.to_string()), out_len)
}

unsafe fn leak_result(r: Result<Vec<u8>, String>, out_len: *mut usize) -> *mut u8 {
    match r {
        Ok(v) => {
            let mut b = v.into_boxed_slice();
            *out_len = b.len();
            let p = b.as_mut_ptr();
            std::mem::forget(b);
            p
        }
        Err(e) => { set_error(e); std::ptr::null_mut() }
    }
}

/// Append `entry` (a complete .kore file) as a new version to a version log.
/// `existing` may be NULL (new log), a log, or a plain .kore file. Timestamps must increase.
/// Returns a buffer to free with kore_free_bytes, or NULL on error.
#[no_mangle]
pub unsafe extern "C" fn kore_version_append(
    existing: *const u8, existing_len: usize,
    entry: *const u8, entry_len: usize,
    timestamp: u64,
    out_len: *mut usize,
) -> *mut u8 {
    if entry.is_null() || out_len.is_null() {
        set_error("kore_version_append: null argument");
        return std::ptr::null_mut();
    }
    let ex = if existing.is_null() { None } else { Some(std::slice::from_raw_parts(existing, existing_len)) };
    let r = kore_store::versioned::append_raw(ex, std::slice::from_raw_parts(entry, entry_len), timestamp);
    leak_result(r.map_err(|e| e.to_string()), out_len)
}

/// Copy out the newest version with timestamp <= `target`. Free with kore_free_bytes.
#[no_mangle]
pub unsafe extern "C" fn kore_version_select(
    data: *const u8, len: usize, target: u64, out_len: *mut usize,
) -> *mut u8 {
    if data.is_null() || out_len.is_null() {
        set_error("kore_version_select: null argument");
        return std::ptr::null_mut();
    }
    let r = kore_store::versioned::select_raw(std::slice::from_raw_parts(data, len), target);
    leak_result(r.map(|b| b.to_vec()).map_err(|e| e.to_string()), out_len)
}

/// Free a byte buffer returned by kore_write_bytes.
#[no_mangle]
pub unsafe extern "C" fn kore_free_bytes(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(Vec::from_raw_parts(ptr, len, len));
    }
}

/// Compute CRC32 checksum of a byte buffer.
#[no_mangle]
pub unsafe extern "C" fn kore_crc32(data: *const u8, len: usize) -> u32 {
    if data.is_null() { return 0; }
    let slice = std::slice::from_raw_parts(data, len);
    // Simple CRC32 using the standard polynomial
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in slice {
        crc ^= u32::from(b);
        for _ in 0..8 {
            if crc & 1 != 0 { crc = (crc >> 1) ^ 0xEDB8_8320; }
            else             { crc >>= 1; }
        }
    }
    !crc
}

/// Get column name by index from a DataBlock.
/// Returns NULL if index out of range. Caller must free with kore_free_string().
#[no_mangle]
pub unsafe extern "C" fn kore_block_col_name(
    block: *const KoreBlock,
    idx: usize,
) -> *mut c_char {
    let block = match block.as_ref() { Some(b) => b, None => return std::ptr::null_mut() };
    match block.inner.columns.get(idx) {
        Some(col) => CString::new(col.name.clone()).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut()),
        None      => std::ptr::null_mut(),
    }
}

// ── Nullable column import/export (explicit validity, NaN stays NaN) ───────────
//
// `validity` is one byte per row (1 = valid, 0 = null) or NULL when every row is valid.
// Strings are passed as `len + 1` byte offsets into one UTF-8 buffer.

#[inline]
unsafe fn is_valid(validity: *const u8, i: usize) -> bool {
    validity.is_null() || *validity.add(i) != 0
}

unsafe fn col_name_arg(name: *const c_char) -> Option<String> {
    if name.is_null() { return None; }
    CStr::from_ptr(name).to_str().ok().map(|s| s.to_string())
}

unsafe fn push_column(ptr: *mut KoreBlock, name: String, data: ColumnData, rows: usize) -> c_int {
    (*ptr).inner.columns.push(Column { name, data });
    (*ptr).inner.num_rows = rows;
    0
}

/// Add an f64 column; a zero byte in `validity` marks null, NaN values are kept as NaN.
#[no_mangle]
pub unsafe extern "C" fn kore_block_add_f64_v(
    ptr: *mut KoreBlock, name: *const c_char,
    data: *const c_double, validity: *const u8, len: u64,
) -> c_int {
    if ptr.is_null() || data.is_null() { set_error("kore_block_add_f64_v: null argument"); return -1; }
    let Some(name) = col_name_arg(name) else { set_error("invalid column name"); return -1 };
    let v = std::slice::from_raw_parts(data, len as usize);
    let vals = v.iter().enumerate().map(|(i, &x)| if is_valid(validity, i) { Some(x) } else { None }).collect();
    push_column(ptr, name, ColumnData::Float64(vals), len as usize)
}

/// Add an i64 column with explicit validity (i64::MIN is an ordinary value here).
#[no_mangle]
pub unsafe extern "C" fn kore_block_add_i64_v(
    ptr: *mut KoreBlock, name: *const c_char,
    data: *const c_longlong, validity: *const u8, len: u64,
) -> c_int {
    if ptr.is_null() || data.is_null() { set_error("kore_block_add_i64_v: null argument"); return -1; }
    let Some(name) = col_name_arg(name) else { set_error("invalid column name"); return -1 };
    let v = std::slice::from_raw_parts(data, len as usize);
    let vals = v.iter().enumerate().map(|(i, &x)| if is_valid(validity, i) { Some(x) } else { None }).collect();
    push_column(ptr, name, ColumnData::Int64(vals), len as usize)
}

/// Add a bool column; `data` holds one byte per row (0 or 1).
#[no_mangle]
pub unsafe extern "C" fn kore_block_add_bool_v(
    ptr: *mut KoreBlock, name: *const c_char,
    data: *const u8, validity: *const u8, len: u64,
) -> c_int {
    if ptr.is_null() || data.is_null() { set_error("kore_block_add_bool_v: null argument"); return -1; }
    let Some(name) = col_name_arg(name) else { set_error("invalid column name"); return -1 };
    let v = std::slice::from_raw_parts(data, len as usize);
    let vals = v.iter().enumerate().map(|(i, &x)| if is_valid(validity, i) { Some(x != 0) } else { None }).collect();
    push_column(ptr, name, ColumnData::Bool(vals), len as usize)
}

/// Add a string column from `len + 1` offsets into `bytes` (UTF-8). Null rows ignore their bytes.
#[no_mangle]
pub unsafe extern "C" fn kore_block_add_str_v(
    ptr: *mut KoreBlock, name: *const c_char,
    offsets: *const u32, bytes: *const u8, bytes_len: u64,
    validity: *const u8, len: u64,
) -> c_int {
    if ptr.is_null() || offsets.is_null() { set_error("kore_block_add_str_v: null argument"); return -1; }
    let Some(name) = col_name_arg(name) else { set_error("invalid column name"); return -1 };
    let n = len as usize;
    let offs = std::slice::from_raw_parts(offsets, n + 1);
    let buf: &[u8] = if bytes.is_null() { &[] } else { std::slice::from_raw_parts(bytes, bytes_len as usize) };
    let mut vals = Vec::with_capacity(n);
    for i in 0..n {
        if !is_valid(validity, i) { vals.push(None); continue; }
        let (a, b) = (offs[i] as usize, offs[i + 1] as usize);
        match buf.get(a..b).filter(|_| a <= b).map(std::str::from_utf8) {
            Some(Ok(s)) => vals.push(Some(s.to_string())),
            Some(Err(_)) => { set_error("string column contains invalid UTF-8"); return -1; }
            None => { set_error("string offsets out of range"); return -1; }
        }
    }
    push_column(ptr, name, ColumnData::Str(vals), n)
}

unsafe fn nth_column<'a>(ptr: *const KoreBlock, idx: usize) -> Option<&'a Column> {
    ptr.as_ref()?.inner.columns.get(idx)
}

unsafe fn write_validity<T>(vals: &[Option<T>], validity_out: *mut u8) {
    if validity_out.is_null() { return; }
    for (i, v) in vals.iter().enumerate() { *validity_out.add(i) = v.is_some() as u8; }
}

/// Copy an f64 column (by index) and its validity. Null slots hold NaN in `out`. Returns row count or -1.
#[no_mangle]
pub unsafe extern "C" fn kore_block_get_f64_v(
    ptr: *const KoreBlock, idx: usize, out: *mut c_double, validity_out: *mut u8, maxlen: u64,
) -> i64 {
    let Some(col) = nth_column(ptr, idx) else { set_error("column index out of range"); return -1 };
    let ColumnData::Float64(v) = &col.data else { set_error("column is not f64"); return -1 };
    if out.is_null() || (v.len() as u64) > maxlen { set_error("output buffer too small"); return -1; }
    for (i, x) in v.iter().enumerate() { *out.add(i) = x.unwrap_or(f64::NAN); }
    write_validity(v, validity_out);
    v.len() as i64
}

/// Copy an i64 column (by index) and its validity. Null slots hold 0. Returns row count or -1.
#[no_mangle]
pub unsafe extern "C" fn kore_block_get_i64_v(
    ptr: *const KoreBlock, idx: usize, out: *mut c_longlong, validity_out: *mut u8, maxlen: u64,
) -> i64 {
    let Some(col) = nth_column(ptr, idx) else { set_error("column index out of range"); return -1 };
    let ColumnData::Int64(v) = &col.data else { set_error("column is not i64"); return -1 };
    if out.is_null() || (v.len() as u64) > maxlen { set_error("output buffer too small"); return -1; }
    for (i, x) in v.iter().enumerate() { *out.add(i) = x.unwrap_or(0); }
    write_validity(v, validity_out);
    v.len() as i64
}

/// Copy a bool column (by index) as bytes 0/1 and its validity. Returns row count or -1.
#[no_mangle]
pub unsafe extern "C" fn kore_block_get_bool_v(
    ptr: *const KoreBlock, idx: usize, out: *mut u8, validity_out: *mut u8, maxlen: u64,
) -> i64 {
    let Some(col) = nth_column(ptr, idx) else { set_error("column index out of range"); return -1 };
    let ColumnData::Bool(v) = &col.data else { set_error("column is not bool"); return -1 };
    if out.is_null() || (v.len() as u64) > maxlen { set_error("output buffer too small"); return -1; }
    for (i, x) in v.iter().enumerate() { *out.add(i) = x.unwrap_or(false) as u8; }
    write_validity(v, validity_out);
    v.len() as i64
}

fn str_at(data: &ColumnData, i: usize) -> Option<&str> {
    match data {
        ColumnData::Str(v) => v.get(i).and_then(|o| o.as_deref()),
        ColumnData::StrDict { codes, dict } => codes.get(i)
            .and_then(|&c| if c == u8::MAX { None } else { dict.get(c as usize).map(|s| s.as_str()) }),
        _ => None,
    }
}

fn str_rows(data: &ColumnData) -> Option<usize> {
    match data {
        ColumnData::Str(v) => Some(v.len()),
        ColumnData::StrDict { codes, .. } => Some(codes.len()),
        _ => None,
    }
}

/// Total UTF-8 bytes of a string (or string-dict) column, or -1 if it is not a string column.
#[no_mangle]
pub unsafe extern "C" fn kore_block_str_bytes(ptr: *const KoreBlock, idx: usize) -> i64 {
    let Some(col) = nth_column(ptr, idx) else { return -1 };
    let Some(n) = str_rows(&col.data) else { return -1 };
    (0..n).map(|i| str_at(&col.data, i).map_or(0, |s| s.len() as i64)).sum()
}

/// Export a string column: `offsets_out` gets rows + 1 entries, `bytes_out` the concatenated UTF-8
/// (size it with kore_block_str_bytes), `validity_out` one byte per row. Returns row count or -1.
#[no_mangle]
pub unsafe extern "C" fn kore_block_get_str_v(
    ptr: *const KoreBlock, idx: usize,
    offsets_out: *mut u32, bytes_out: *mut u8, bytes_cap: u64, validity_out: *mut u8,
) -> i64 {
    let Some(col) = nth_column(ptr, idx) else { set_error("column index out of range"); return -1 };
    let Some(n) = str_rows(&col.data) else { set_error("column is not a string column"); return -1 };
    if offsets_out.is_null() { set_error("null offsets buffer"); return -1; }
    let mut pos: usize = 0;
    *offsets_out = 0;
    for i in 0..n {
        let s = str_at(&col.data, i);
        if let Some(s) = s {
            if pos + s.len() > bytes_cap as usize || pos + s.len() > u32::MAX as usize {
                set_error("string buffer too small or column exceeds 4 GiB");
                return -1;
            }
            if !bytes_out.is_null() { std::ptr::copy_nonoverlapping(s.as_ptr(), bytes_out.add(pos), s.len()); }
            pos += s.len();
        }
        if !validity_out.is_null() { *validity_out.add(i) = s.is_some() as u8; }
        *offsets_out.add(i + 1) = pos as u32;
    }
    n as i64
}

/// Add a dictionary-encoded string column: one `codes` byte per row (255 = null) indexing into a
/// dictionary of at most 254 strings given as `dict_len + 1` offsets into `dict_bytes` (UTF-8).
#[no_mangle]
pub unsafe extern "C" fn kore_block_add_strdict_v(
    ptr: *mut KoreBlock, name: *const c_char,
    codes: *const u8, len: u64,
    dict_offsets: *const u32, dict_bytes: *const u8, dict_bytes_len: u64, dict_len: u64,
) -> c_int {
    if ptr.is_null() || codes.is_null() || dict_offsets.is_null() {
        set_error("kore_block_add_strdict_v: null argument");
        return -1;
    }
    let Some(name) = col_name_arg(name) else { set_error("invalid column name"); return -1 };
    if dict_len > 254 { set_error("dictionary has more than 254 entries"); return -1; }
    let offs = std::slice::from_raw_parts(dict_offsets, dict_len as usize + 1);
    let buf: &[u8] = if dict_bytes.is_null() { &[] } else { std::slice::from_raw_parts(dict_bytes, dict_bytes_len as usize) };
    let mut dict = Vec::with_capacity(dict_len as usize);
    for i in 0..dict_len as usize {
        let (a, b) = (offs[i] as usize, offs[i + 1] as usize);
        match buf.get(a..b).filter(|_| a <= b).map(std::str::from_utf8) {
            Some(Ok(s)) => dict.push(s.to_string()),
            Some(Err(_)) => { set_error("dictionary contains invalid UTF-8"); return -1; }
            None => { set_error("dictionary offsets out of range"); return -1; }
        }
    }
    let codes = std::slice::from_raw_parts(codes, len as usize).to_vec();
    if codes.iter().any(|&c| c != u8::MAX && c as usize >= dict.len()) {
        set_error("string code out of dictionary range");
        return -1;
    }
    push_column(ptr, name, ColumnData::StrDict { codes, dict }, len as usize)
}

/// For a dictionary-encoded string column returns 1 and writes the entry count and total dictionary
/// bytes; returns 0 for any other column and -1 if the index is out of range.
#[no_mangle]
pub unsafe extern "C" fn kore_block_strdict_info(
    ptr: *const KoreBlock, idx: usize, dict_len_out: *mut u64, dict_bytes_out: *mut u64,
) -> c_int {
    let Some(col) = nth_column(ptr, idx) else { return -1 };
    match &col.data {
        ColumnData::StrDict { dict, .. } => {
            if !dict_len_out.is_null() { *dict_len_out = dict.len() as u64; }
            if !dict_bytes_out.is_null() { *dict_bytes_out = dict.iter().map(|s| s.len() as u64).sum(); }
            1
        }
        _ => 0,
    }
}

/// Export a dictionary-encoded string column: `codes_out` one byte per row (255 = null),
/// `dict_offsets_out` dict_len + 1 entries, `dict_bytes_out` the concatenated entries.
/// Size the buffers with kore_block_strdict_info. Returns row count or -1.
#[no_mangle]
pub unsafe extern "C" fn kore_block_get_strdict_v(
    ptr: *const KoreBlock, idx: usize,
    codes_out: *mut u8, dict_offsets_out: *mut u32, dict_bytes_out: *mut u8,
) -> i64 {
    let Some(col) = nth_column(ptr, idx) else { set_error("column index out of range"); return -1 };
    let ColumnData::StrDict { codes, dict } = &col.data else { set_error("column is not dictionary-encoded"); return -1 };
    if codes_out.is_null() || dict_offsets_out.is_null() { set_error("null output buffer"); return -1; }
    std::ptr::copy_nonoverlapping(codes.as_ptr(), codes_out, codes.len());
    let mut pos = 0usize;
    *dict_offsets_out = 0;
    for (i, s) in dict.iter().enumerate() {
        if !dict_bytes_out.is_null() { std::ptr::copy_nonoverlapping(s.as_ptr(), dict_bytes_out.add(pos), s.len()); }
        pos += s.len();
        *dict_offsets_out.add(i + 1) = pos as u32;
    }
    codes.len() as i64
}

/// Column type by index: 0=int64, 1=float64, 2=bool, 3=string, 4=string-dict; -1 if out of range.
#[no_mangle]
pub unsafe extern "C" fn kore_block_col_type(block: *const KoreBlock, idx: usize) -> c_int {
    let block = match block.as_ref() { Some(b) => b, None => return -1 };
    match block.inner.columns.get(idx).map(|c| &c.data) {
        Some(ColumnData::Int64(_))   => 0,
        Some(ColumnData::Float64(_)) => 1,
        Some(ColumnData::Bool(_))    => 2,
        Some(ColumnData::Str(_))     => 3,
        Some(ColumnData::StrDict { .. }) => 4,
        None => -1,
    }
}

/// Get Int64 column data by name.
/// Returns number of values written, or -1 on error.
#[no_mangle]
pub unsafe extern "C" fn kore_block_get_i64(
    ptr:    *const KoreBlock,
    col:    *const c_char,
    out:    *mut c_longlong,
    maxlen: u64,
) -> i64 {
    if ptr.is_null() || col.is_null() || out.is_null() { return -1; }
    let col_name = match CStr::from_ptr(col).to_str() { Ok(s) => s, Err(_) => return -1 };
    let block = &(*ptr).inner;
    let column = match block.columns.iter().find(|c| c.name == col_name) {
        Some(c) => c, None => { set_error(format!("column not found: {col_name}")); return -1; }
    };
    match &column.data {
        ColumnData::Int64(v) => {
            let n = v.len().min(maxlen as usize);
            let out_slice = std::slice::from_raw_parts_mut(out, n);
            for (i, val) in v[..n].iter().enumerate() {
                out_slice[i] = val.unwrap_or(0);
            }
            n as i64
        }
        ColumnData::Float64(v) => {
            // Allow reading float column as i64 (truncated)
            let n = v.len().min(maxlen as usize);
            let out_slice = std::slice::from_raw_parts_mut(out, n);
            for (i, val) in v[..n].iter().enumerate() {
                out_slice[i] = val.unwrap_or(0.0) as c_longlong;
            }
            n as i64
        }
        _ => { set_error("column is not numeric"); -1 }
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

unsafe fn ptr_to_str<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() { return None; }
    CStr::from_ptr(p).to_str().ok()
}

fn load_csv_into(ctx: &mut KqlContext, table_name: &str, path: &str) -> Result<(), String> {
    use std::io::{BufRead, BufReader};
    use std::collections::HashMap;

    // ── Pass 1: read header + 2 000 sample rows → determine column types ──────
    let file1 = std::fs::File::open(path).map_err(|e| format!("open {path}: {e}"))?;
    let mut r1 = BufReader::new(file1);

    let mut hdr = String::new();
    r1.read_line(&mut hdr).map_err(|e| e.to_string())?;
    let headers: Vec<String> = hdr.trim_end().split(',')
        .map(|h| h.trim().trim_matches('"').to_string()).collect();
    let nc = headers.len();

    const SAMPLE: usize = 2_000;
    // For each column: track if all non-empty values parse as i64, f64, and cardinality.
    let mut all_i64:   Vec<bool> = vec![true; nc];
    let mut all_f64:   Vec<bool> = vec![true; nc];
    let mut seen:      Vec<HashMap<[u8; 16], ()>> = (0..nc).map(|_| HashMap::new()).collect();
    let mut seen_many: Vec<bool> = vec![false; nc];  // >255 unique
    let mut any_val:   Vec<bool> = vec![false; nc];
    let mut nsampled = 0usize;

    let mut line = String::new();
    while nsampled < SAMPLE {
        line.clear();
        if r1.read_line(&mut line).map_err(|e| e.to_string())? == 0 { break; }
        let trimmed = line.trim_end();
        if trimmed.is_empty() { continue; }
        let vals: Vec<&str> = trimmed.splitn(nc, ',').collect();
        for i in 0..nc {
            let v = vals.get(i).map(|s| s.trim().trim_matches('"')).unwrap_or("");
            if v.is_empty() { continue; }
            any_val[i] = true;
            if all_i64[i] && v.parse::<i64>().is_err() { all_i64[i] = false; }
            if all_f64[i] && v.parse::<f64>().is_err() { all_f64[i] = false; }
            if !seen_many[i] {
                // cheap 16-byte key from first 16 bytes of value
                let mut key = [0u8; 16];
                let b = v.as_bytes();
                let n = b.len().min(16);
                key[..n].copy_from_slice(&b[..n]);
                key[15] = b.len() as u8;
                seen[i].insert(key, ());
                if seen[i].len() > 255 { seen_many[i] = true; }
            }
        }
        nsampled += 1;
    }

    #[derive(Clone, PartialEq)]
    enum CT { Int64, Float64, StrDict, Str }
    let col_types: Vec<CT> = (0..nc).map(|i| {
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

    let cap = 1_100_000usize;
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
        let vals: Vec<&str> = trimmed.splitn(nc, ',').collect();
        for i in 0..nc {
            let v = vals.get(i).map(|s| s.trim().trim_matches('"')).unwrap_or("");
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
                        // cardinality overflowed sample estimate — treat as plain Str
                        sd_codes[i].push(0);
                    }
                }
            }
        }
    }

    // ── Build DataBlock ───────────────────────────────────────────────────────
    let mut columns = vec![];
    for (i, name) in headers.iter().enumerate() {
        let data = match col_types[i] {
            CT::Int64   => ColumnData::Int64(std::mem::take(&mut i64_cols[i])),
            CT::Float64 => ColumnData::Float64(std::mem::take(&mut f64_cols[i])),
            CT::Str     => ColumnData::Str(std::mem::take(&mut str_cols[i])),
            CT::StrDict => ColumnData::StrDict {
                codes: std::mem::take(&mut sd_codes[i]),
                dict:  std::mem::take(&mut sd_dict[i]),
            },
        };
        columns.push(Column { name: name.clone(), data });
    }
    ctx.register(table_name, DataBlock { columns, num_rows: nr });
    Ok(())
}

fn block_to_json_stripped(block: &DataBlock) -> String {
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
                kore_core::Value::Null     => serde_json::Value::Null,
            };
            obj.insert(key, jv);
        }
        rows.push(serde_json::Value::Object(obj));
    }
    serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into())
}

#[cfg(test)]
mod native_column_tests {
    use super::*;

    fn cname(s: &str) -> CString { CString::new(s).unwrap() }

    unsafe fn add_strs(b: *mut KoreBlock, name: &str, vals: &[Option<&str>]) {
        let mut offs = vec![0u32];
        let mut bytes = Vec::new();
        let mut valid = Vec::new();
        for v in vals {
            if let Some(s) = v { bytes.extend_from_slice(s.as_bytes()); }
            valid.push(v.is_some() as u8);
            offs.push(bytes.len() as u32);
        }
        let n = cname(name);
        assert_eq!(kore_block_add_str_v(b, n.as_ptr(), offs.as_ptr(), bytes.as_ptr(), bytes.len() as u64,
                                        valid.as_ptr(), vals.len() as u64), 0);
    }

    unsafe fn read_strs(b: *const KoreBlock, idx: usize) -> Vec<Option<String>> {
        let total = kore_block_str_bytes(b, idx);
        assert!(total >= 0);
        let n = kore_block_num_rows(b) as usize;
        let mut offs = vec![0u32; n + 1];
        let mut bytes = vec![0u8; total as usize + 1];
        let mut valid = vec![0u8; n];
        assert_eq!(kore_block_get_str_v(b, idx, offs.as_mut_ptr(), bytes.as_mut_ptr(), bytes.len() as u64,
                                        valid.as_mut_ptr()), n as i64);
        (0..n).map(|i| if valid[i] == 0 { None } else {
            Some(String::from_utf8(bytes[offs[i] as usize..offs[i + 1] as usize].to_vec()).unwrap())
        }).collect()
    }

    #[test]
    fn nullable_columns_roundtrip_through_the_file_format() {
        unsafe {
            let n = 3000usize;
            let ints: Vec<i64> = (0..n as i64).map(|i| if i == 5 { i64::MIN } else { i * 3 }).collect();
            let int_valid: Vec<u8> = (0..n).map(|i| (i % 11 != 0) as u8).collect();
            let floats: Vec<f64> = (0..n).map(|i| if i % 13 == 1 { f64::NAN } else { i as f64 * 0.5 }).collect();
            let float_valid: Vec<u8> = (0..n).map(|i| (i % 9 != 0) as u8).collect();
            let bools: Vec<u8> = (0..n).map(|i| (i % 2) as u8).collect();
            let bool_valid: Vec<u8> = (0..n).map(|i| (i % 7 != 0) as u8).collect();
            let unique: Vec<String> = (0..n).map(|i| format!("user-{i}-\u{e9}\u{1f600}")).collect();
            let uniq_refs: Vec<Option<&str>> = unique.iter().enumerate()
                .map(|(i, s)| if i % 5 == 0 { None } else { Some(s.as_str()) }).collect();
            let low_refs: Vec<Option<&str>> = (0..n).map(|i| match i % 5 { 0 => None, 1 => Some("US"), 2 => Some(""), _ => Some("EU") }).collect();

            let b = kore_block_new();
            assert_eq!(kore_block_add_i64_v(b, cname("i").as_ptr(), ints.as_ptr(), int_valid.as_ptr(), n as u64), 0);
            assert_eq!(kore_block_add_f64_v(b, cname("f").as_ptr(), floats.as_ptr(), float_valid.as_ptr(), n as u64), 0);
            assert_eq!(kore_block_add_bool_v(b, cname("b").as_ptr(), bools.as_ptr(), bool_valid.as_ptr(), n as u64), 0);
            add_strs(b, "uniq", &uniq_refs);
            add_strs(b, "low", &low_refs);

            let mut len = 0usize;
            let bytes = kore_write_bytes(b, &mut len);
            assert!(!bytes.is_null());
            let r = kore_read_bytes(bytes, len);
            assert!(!r.is_null());
            kore_free_bytes(bytes, len);
            kore_block_free(b);

            let mut iv = vec![0i64; n]; let mut ivalid = vec![0u8; n];
            assert_eq!(kore_block_get_i64_v(r, 0, iv.as_mut_ptr(), ivalid.as_mut_ptr(), n as u64), n as i64);
            assert_eq!(ivalid, int_valid);
            for i in 0..n { if int_valid[i] == 1 { assert_eq!(iv[i], ints[i]); } }
            assert_eq!(iv[5], i64::MIN, "i64::MIN must be an ordinary value");

            let mut fv = vec![0f64; n]; let mut fvalid = vec![0u8; n];
            assert_eq!(kore_block_get_f64_v(r, 1, fv.as_mut_ptr(), fvalid.as_mut_ptr(), n as u64), n as i64);
            assert_eq!(fvalid, float_valid);
            for i in 0..n {
                if float_valid[i] == 1 {
                    if floats[i].is_nan() { assert!(fv[i].is_nan() && fvalid[i] == 1, "row {i}: NaN lost"); }
                    else { assert_eq!(fv[i], floats[i]); }
                }
            }

            let mut bv = vec![0u8; n]; let mut bvalid = vec![0u8; n];
            assert_eq!(kore_block_get_bool_v(r, 2, bv.as_mut_ptr(), bvalid.as_mut_ptr(), n as u64), n as i64);
            assert_eq!(bvalid, bool_valid);
            for i in 0..n { if bool_valid[i] == 1 { assert_eq!(bv[i], bools[i]); } }

            let got_u = read_strs(r, 3);
            let got_l = read_strs(r, 4);
            for i in 0..n {
                assert_eq!(got_u[i].as_deref(), uniq_refs[i], "unique row {i}");
                assert_eq!(got_l[i].as_deref(), low_refs[i], "low-cardinality row {i}");
            }
            kore_block_free(r);
        }
    }

    #[test]
    fn dictionary_strings_roundtrip_through_the_file_format() {
        unsafe {
            let n = 5000usize;
            let dict = ["US", "EU", "", "caf\u{e9}\u{1f600}"];
            let mut offs = vec![0u32];
            let mut bytes = Vec::new();
            for s in dict { bytes.extend_from_slice(s.as_bytes()); offs.push(bytes.len() as u32); }
            let codes: Vec<u8> = (0..n).map(|i| if i % 6 == 0 { 255 } else { (i % 4) as u8 }).collect();

            let b = kore_block_new();
            assert_eq!(kore_block_add_strdict_v(b, cname("r").as_ptr(), codes.as_ptr(), n as u64,
                offs.as_ptr(), bytes.as_ptr(), bytes.len() as u64, dict.len() as u64), 0);
            // out-of-range code is rejected
            let bad = [9u8];
            assert_eq!(kore_block_add_strdict_v(b, cname("x").as_ptr(), bad.as_ptr(), 1,
                offs.as_ptr(), bytes.as_ptr(), bytes.len() as u64, dict.len() as u64), -1);

            let mut len = 0usize;
            let out = kore_write_bytes(b, &mut len);
            let r = kore_read_bytes(out, len);
            assert!(!r.is_null());
            kore_free_bytes(out, len);
            kore_block_free(b);

            let (mut dl, mut db) = (0u64, 0u64);
            assert_eq!(kore_block_strdict_info(r, 0, &mut dl, &mut db), 1);
            assert_eq!((dl, db), (4, bytes.len() as u64));
            let mut got_codes = vec![0u8; n];
            let mut got_offs = vec![0u32; dl as usize + 1];
            let mut got_bytes = vec![0u8; db as usize];
            assert_eq!(kore_block_get_strdict_v(r, 0, got_codes.as_mut_ptr(), got_offs.as_mut_ptr(), got_bytes.as_mut_ptr()), n as i64);
            assert_eq!(got_codes, codes);
            assert_eq!(got_offs, offs);
            assert_eq!(got_bytes, bytes);
            // the generic string reader sees the same column
            let rows = read_strs(r, 0);
            assert_eq!(rows[0], None);
            assert_eq!(rows[3].as_deref(), Some("caf\u{e9}\u{1f600}"));
            kore_block_free(r);
        }
    }

    #[test]
    fn rejects_invalid_utf8_and_bad_offsets() {
        unsafe {
            let b = kore_block_new();
            let bad = [0xffu8, 0xfe];
            let offs = [0u32, 2];
            let n = cname("s");
            assert_eq!(kore_block_add_str_v(b, n.as_ptr(), offs.as_ptr(), bad.as_ptr(), 2, std::ptr::null(), 1), -1);
            let offs = [0u32, 99];
            assert_eq!(kore_block_add_str_v(b, n.as_ptr(), offs.as_ptr(), bad.as_ptr(), 2, std::ptr::null(), 1), -1);
            kore_block_free(b);
        }
    }
}
