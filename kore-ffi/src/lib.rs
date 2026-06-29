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

