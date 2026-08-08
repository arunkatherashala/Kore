/**
 * kore_ffi.js — koffi-based FFI bridge to Rust kore_ffi DLL
 * Loaded by kore_fileformat.ts at runtime (no compilation needed).
 */
'use strict';

const path = require('path');
const fs   = require('fs');

// Search for the DLL in multiple locations
function findDll() {
  const name = process.platform === 'win32'
    ? 'kore_ffi.dll'
    : process.platform === 'darwin' ? 'libkore_ffi.dylib' : 'libkore_ffi.so';

  const candidates = [
    path.join(__dirname, '..', 'target', 'release', name),
    path.join(__dirname, name),
    name,
  ];
  for (const c of candidates) {
    if (fs.existsSync(c)) return c;
  }
  throw new Error(
    `kore-ffi DLL not found. Build with: cargo build --release -p kore-ffi\nTried: ${candidates.join(', ')}`
  );
}

let _lib = null;
let _koffi = null;

function getLib() {
  if (_lib) return _lib;
  _koffi = require('koffi');
  const dllPath = findDll();
  _lib = _koffi.load(dllPath);
  return _lib;
}

// ── Exported FFI functions ────────────────────────────────────────────────────

function crc32(data) {
  const lib = getLib();
  const fn = lib.func('kore_crc32', 'uint32', ['uint8 *', 'size_t']);
  const buf = Buffer.isBuffer(data) ? data : Buffer.from(data);
  return fn(buf, buf.length);
}

function writeFile(filePath, dataBlock) {
  const lib = getLib();
  const blockNew   = lib.func('kore_block_new',    'void *', []);
  const blockFree  = lib.func('kore_block_free',   'void',   ['void *']);
  const addF64     = lib.func('kore_block_add_f64','int',    ['void *', 'str', 'double *', 'size_t']);
  const addI64     = lib.func('kore_block_add_i64','int',    ['void *', 'str', 'int64 *',  'size_t']);
  const writeFileFn= lib.func('kore_write_file',   'int',    ['str', 'void *']);

  const handle = blockNew();
  try {
    for (const col of dataBlock.columns) {
      if (col.dtype === 2 /* F64 */) {
        const arr = new Float64Array(col.data.map(Number));
        addF64(handle, col.name, Buffer.from(arr.buffer), col.data.length);
      } else if (col.dtype === 1 /* I64 */) {
        const arr = new BigInt64Array(col.data.map(BigInt));
        addI64(handle, col.name, Buffer.from(arr.buffer), col.data.length);
      }
      // STR columns: TODO kore_block_add_str
    }
    const rc = writeFileFn(filePath, handle);
    if (rc !== 0) throw new Error(`kore_write_file failed (rc=${rc})`);
  } finally {
    blockFree(handle);
  }
}

function readFile(filePath) {
  const lib = getLib();
  const readFileFn = lib.func('kore_read_file',     'void *', ['str']);
  const blockFree  = lib.func('kore_block_free',    'void',   ['void *']);
  const numRows    = lib.func('kore_block_num_rows', 'uint64', ['void *']);
  const numCols    = lib.func('kore_block_num_cols', 'uint32', ['void *']);
  const colName    = lib.func('kore_block_col_name', 'str',    ['void *', 'size_t']);
  const getF64     = lib.func('kore_block_get_f64',  'int64',  ['void *', 'str', 'double *', 'uint64']);

  const handle = readFileFn(filePath);
  if (!handle) throw new Error(`kore_read_file failed: ${filePath}`);

  try {
    const nrows = Number(numRows(handle));
    const ncols = Number(numCols(handle));
    const columns = [];

    for (let ci = 0; ci < ncols; ci++) {
      const name = colName(handle, ci);
      const buf  = new Float64Array(nrows);
      const n    = Number(getF64(handle, name, Buffer.from(buf.buffer), nrows));
      columns.push({ name, dtype: 2, data: Array.from(buf.subarray(0, n)) });
    }

    return { columns, numRows: nrows };
  } finally {
    blockFree(handle);
  }
}

module.exports = { crc32, writeFile, readFile };
