// KORE FileFormat v1.6.0 — Node.js test via koffi FFI
// Run: node test_v160_node.js
// Requires: koffi in node_modules, kore_ffi.dll in target/release/

'use strict';
const path   = require('path');
const fs     = require('fs');
const assert = require('assert');

const REPO_ROOT = path.join(__dirname);
const DLL_PATH  = path.join(REPO_ROOT, 'target', 'release', 'kore_ffi.dll');

console.log('======================================================================');
console.log('  KORE FileFormat v1.6.0 — Node.js Test');
console.log(`  Run at: ${new Date().toISOString()}`);
console.log('======================================================================');

let passed = 0, failed = 0;
function check(label, ok, note = '') {
  const s = ok ? ' PASS ' : ' FAIL ';
  console.log(`  [${s}] ${label}${note ? ' — ' + note : ''}`);
  ok ? passed++ : failed++;
  return ok;
}

// ── Load koffi + DLL ──────────────────────────────────────────────────────────
let koffi, lib;
try {
  koffi = require(path.join(REPO_ROOT, 'node_modules', 'koffi'));
  lib   = koffi.load(DLL_PATH);
  check('Load kore_ffi.dll via koffi', true, DLL_PATH);
} catch (e) {
  check('Load kore_ffi.dll via koffi', false, String(e).slice(0, 80));
  process.exit(1);
}

// ── Declare FFI functions ──────────────────────────────────────────────────────
const kore_crc32        = lib.func('kore_crc32',         'uint32',  ['uint8 *', 'size_t']);
const kore_block_new    = lib.func('kore_block_new',     'void *',  []);
const kore_block_free   = lib.func('kore_block_free',    'void',    ['void *']);
const kore_block_add_f64= lib.func('kore_block_add_f64', 'int32',   ['void *', 'str', 'double *', 'size_t']);
const kore_block_add_i64= lib.func('kore_block_add_i64', 'int32',   ['void *', 'str', 'int64 *',  'size_t']);
const kore_write_file   = lib.func('kore_write_file',    'int32',   ['str', 'void *']);
const kore_read_file    = lib.func('kore_read_file',     'void *',  ['str']);
const kore_num_rows     = lib.func('kore_block_num_rows','uint64',  ['void *']);
const kore_num_cols     = lib.func('kore_block_num_cols','uint32',  ['void *']);
const kore_col_name     = lib.func('kore_block_col_name','str',     ['void *', 'size_t']);
const kore_get_f64      = lib.func('kore_block_get_f64', 'int64',   ['void *', 'str', 'double *', 'uint64']);

// ── TEST 1: CRC32 ─────────────────────────────────────────────────────────────
console.log('\n  [1] CRC32 via koffi');
const data  = Buffer.from('hello kore v1.6.0');
const crc   = kore_crc32(data, data.length);
const PY_CRC = 0x5946aaf8;  // Known value from Python test
check('crc32 non-zero',               crc !== 0);
check('crc32 matches Python result',  crc === PY_CRC, `0x${crc.toString(16)} == 0x${PY_CRC.toString(16)}`);

// ── TEST 2: Write ─────────────────────────────────────────────────────────────
console.log('\n  [2] Write real data');
const ORDER_IDS  = new BigInt64Array([1001n,1002n,1003n,1004n,1005n,1006n,1007n,1008n,1009n,1010n]);
const PRICES     = new Float64Array([10.5, 20.0, 30.75, 15.0, 45.99, 8.25, 99.0, 55.5, 12.0, 33.33]);
const TIMESTAMPS = new BigInt64Array(Array.from({length:10},(_,i)=>BigInt(Date.now()+i*60000)));

const handle = kore_block_new();
check('kore_block_new()',            handle !== null && handle !== 0);

kore_block_add_i64(handle, 'order_id',     Buffer.from(ORDER_IDS.buffer),  10);
kore_block_add_f64(handle, 'price',        Buffer.from(PRICES.buffer),     10);
kore_block_add_i64(handle, 'timestamp_ms', Buffer.from(TIMESTAMPS.buffer), 10);

const koreFile = path.join(REPO_ROOT, 'test_v160_node.kore');
const rc = kore_write_file(koreFile, handle);
kore_block_free(handle);

check('write_file rc=0',            rc === 0,   `rc=${rc}`);
const bytes = fs.existsSync(koreFile) ? fs.statSync(koreFile).size : 0;
check('file created',               bytes > 0,  `${bytes} bytes`);

// ── TEST 3: Read back ─────────────────────────────────────────────────────────
console.log('\n  [3] Read back + cross-language compat');

// Read the Python-generated file first
const pyFile = path.join(REPO_ROOT, 'test_v160_orders.kore');
if (fs.existsSync(pyFile)) {
  const pyHandle = kore_read_file(pyFile);
  const pyRows   = Number(kore_num_rows(pyHandle));
  const pyCols   = Number(kore_num_cols(pyHandle));
  kore_block_free(pyHandle);
  check('Reads Python-written .kore', pyRows === 10, `${pyRows} rows`);
  check('Column count matches',       pyCols === 4,  `${pyCols} cols`);
} else {
  check('Python .kore file exists', false, 'run Python test first');
}

// Read the Node-written file
const readHandle = kore_read_file(koreFile);
const nrows = Number(kore_num_rows(readHandle));
const ncols = Number(kore_num_cols(readHandle));
check('Node-written: 10 rows',    nrows === 10, `${nrows}`);
check('Node-written: 3 columns',  ncols === 3,  `${ncols}`);

// Read price values back
const priceOut = new Float64Array(10);
const n = Number(kore_get_f64(readHandle, 'price', Buffer.from(priceOut.buffer), 10));
check('price column readable',    n > 0,                      `${n} values`);
check('price[0] = 10.5',          Math.abs(priceOut[0]-10.5) < 0.001, `${priceOut[0].toFixed(2)}`);
kore_block_free(readHandle);

fs.unlinkSync(koreFile);

// ── SUMMARY ───────────────────────────────────────────────────────────────────
console.log();
console.log('======================================================================');
console.log(`  Node.js v${process.version} | koffi FFI | kore_ffi.dll`);
console.log(`  TOTAL: ${passed}/${passed+failed} passed  |  ${failed} failed`);
console.log('======================================================================');
process.exit(failed > 0 ? 1 : 0);
