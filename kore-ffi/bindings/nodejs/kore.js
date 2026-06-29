/**
 * kore.js — Node.js bindings for the KORE engine.
 *
 * Uses `ffi-napi` + `ref-napi` to call the libkore_ffi shared library.
 *
 * Install deps:
 *   npm install ffi-napi ref-napi ref-array-di
 *
 * Usage:
 *   const { KoreBlock, KoreModel, ModelType } = require('./kore');
 *   const block = new KoreBlock();
 *   block.addF64('score', [1.0, 2.0, 3.0]);
 *   const model = new KoreModel(ModelType.LINEAR_REGRESSOR);
 *   model.fit(X, y);
 *   const preds = model.predict(X);
 */

'use strict';

const ffi  = require('ffi-napi');
const ref  = require('ref-napi');
const path = require('path');
const os   = require('os');

// ── Library path ──────────────────────────────────────────────────────────────

function findLib() {
  if (process.env.KORE_LIB) return process.env.KORE_LIB;
  const root = path.resolve(__dirname, '../../..');
  const ext  = { win32: '.dll', darwin: '.dylib', linux: '.so' }[os.platform()] || '.so';
  const prefix = os.platform() === 'win32' ? '' : 'lib';
  return path.join(root, 'target/release', `${prefix}kore_ffi${ext}`);
}

// ── FFI bindings ──────────────────────────────────────────────────────────────

const voidPtr = ref.refType(ref.types.void);
const dblPtr  = ref.refType(ref.types.double);
const i64Ptr  = ref.refType(ref.types.int64);

const lib = ffi.Library(findLib(), {
  kore_last_error:     ['string', []],
  kore_block_new:      [voidPtr, []],
  kore_block_free:     ['void',  [voidPtr]],
  kore_block_num_rows: ['uint64', [voidPtr]],
  kore_block_num_cols: ['uint32', [voidPtr]],
  kore_block_add_f64:  ['int', [voidPtr, 'string', dblPtr, 'uint64']],
  kore_block_add_i64:  ['int', [voidPtr, 'string', i64Ptr, 'uint64']],
  kore_block_get_f64:  ['int64', [voidPtr, 'string', dblPtr, 'uint64']],
  kore_hash_join:      [voidPtr, [voidPtr, voidPtr, 'string', 'string', 'int']],
  kore_model_new:      [voidPtr, ['int', 'int', 'int']],
  kore_model_free:     ['void',  [voidPtr]],
  kore_model_fit:      ['int', [voidPtr, dblPtr, 'uint64', 'uint64', dblPtr]],
  kore_model_predict:  ['int', [voidPtr, dblPtr, 'uint64', 'uint64', dblPtr]],
});

function checkError() {
  const msg = lib.kore_last_error();
  if (msg) throw new Error(`KORE: ${msg}`);
}

// ── KoreBlock ─────────────────────────────────────────────────────────────────

class KoreBlock {
  constructor(_ptr = null) {
    this._ptr = _ptr || lib.kore_block_new();
    if (!this._ptr) { checkError(); throw new Error('KoreBlock creation failed'); }
  }

  get numRows() { return Number(lib.kore_block_num_rows(this._ptr)); }
  get numCols() { return Number(lib.kore_block_num_cols(this._ptr)); }

  addF64(name, data) {
    const arr = Buffer.alloc(data.length * 8);
    data.forEach((v, i) => arr.writeDoubleLe(isNaN(v) ? NaN : v, i * 8));
    const ptr = ref.alloc(dblPtr, arr);
    if (lib.kore_block_add_f64(this._ptr, name, arr, data.length) !== 0) checkError();
    return this;
  }

  addI64(name, data) {
    const arr = Buffer.alloc(data.length * 8);
    data.forEach((v, i) => arr.writeBigInt64LE(BigInt(v === null ? Number.MIN_SAFE_INTEGER : v), i * 8));
    if (lib.kore_block_add_i64(this._ptr, name, arr, data.length) !== 0) checkError();
    return this;
  }

  getF64(col) {
    const n = this.numRows;
    const buf = Buffer.alloc(n * 8);
    const read = lib.kore_block_get_f64(this._ptr, col, buf, n);
    if (read < 0) { checkError(); return []; }
    const out = [];
    for (let i = 0; i < read; i++) out.push(buf.readDoubleLe(i * 8));
    return out;
  }

  join(right, leftKey, rightKey, how = 'inner') {
    const jt = { inner: 0, left: 1, full: 2 }[how] ?? 0;
    const ptr = lib.kore_hash_join(this._ptr, right._ptr, leftKey, rightKey, jt);
    if (!ptr) { checkError(); throw new Error('join failed'); }
    return new KoreBlock(ptr);
  }

  free() { if (this._ptr) { lib.kore_block_free(this._ptr); this._ptr = null; } }
  toString() { return `KoreBlock(rows=${this.numRows}, cols=${this.numCols})`; }
}

// ── KoreModel ─────────────────────────────────────────────────────────────────

const ModelType = Object.freeze({
  RF_REGRESSOR:     0,
  RF_CLASSIFIER:    1,
  GBM_REGRESSOR:    2,
  LINEAR_REGRESSOR: 3,
  LOGISTIC:         4,
  KNN_REGRESSOR:    5,
  KNN_CLASSIFIER:   6,
  SVM:              7,
});

class KoreModel {
  constructor(type, param1 = 100, param2 = 3) {
    this._ptr = lib.kore_model_new(type, param1, param2);
    if (!this._ptr) { checkError(); throw new Error('model creation failed'); }
  }

  /** X: flat Float64Array, rows×cols;  y: Float64Array */
  fit(X, y, nRows, nCols) {
    const xBuf = Buffer.from(X.buffer);
    const yBuf = Buffer.from(y.buffer);
    if (lib.kore_model_fit(this._ptr, xBuf, nRows, nCols, yBuf) !== 0) checkError();
    return this;
  }

  predict(X, nRows, nCols) {
    const xBuf  = Buffer.from(X.buffer);
    const out   = new Float64Array(nRows);
    const oBuf  = Buffer.from(out.buffer);
    if (lib.kore_model_predict(this._ptr, xBuf, nRows, nCols, oBuf) !== 0) checkError();
    return out;
  }

  free() { if (this._ptr) { lib.kore_model_free(this._ptr); this._ptr = null; } }
}

module.exports = { KoreBlock, KoreModel, ModelType };
