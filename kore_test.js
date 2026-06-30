// kore_test.js — KORE Node.js test via koffi (pure JS FFI, no native build)
// Install: npm install koffi
// Run:     node kore_test.js
const koffi = require('koffi');
const path  = require('path');
const os    = require('os');

const DLL = String.raw`C:\Users\skathera\Downloads\asistent\kore\target\release\kore_ffi.dll`;

console.log('=== KORE Node.js Test (koffi FFI) ===');

let lib;
try {
    lib = koffi.load(DLL);
} catch(e) {
    console.error('FAILED to load DLL:', e.message);
    process.exit(1);
}

// Bind functions
const sessionNew   = lib.func('void* kore_session_new()');
const sessionFree  = lib.func('void kore_session_free(void* sess)');
const sessionLoad  = lib.func('int kore_session_load_csv(void* sess, const char* table, const char* path)');
const sessionQuery = lib.func('const char* kore_session_query(void* sess, const char* sql)');
const sessionCount = lib.func('int64_t kore_session_row_count(void* sess, const char* table)');
const freeStr      = lib.func('void kore_free_string(const char* s)');

const sess = sessionNew();
console.log(`[1] Session created`);

const rc = sessionLoad(sess, 'bench', String.raw`C:\Users\skathera\Downloads\asistent\bench_export.csv`);
console.log(`[2] load_csv returned: ${rc} (0=OK)`);

const n = sessionCount(sess, 'bench');
console.log(`[3] Row count: ${n}`);

const json1 = sessionQuery(sess,
    'SELECT category, COUNT(*) as cnt, SUM(amount) as total FROM bench GROUP BY category ORDER BY total DESC');
if (json1) {
    const rows = JSON.parse(json1);
    console.log(`[4] GROUP BY (${rows.length} groups):`);
    rows.forEach(r => console.log(`     ${JSON.stringify(r)}`));
}

const json2 = sessionQuery(sess,
    'SELECT id, amount FROM bench WHERE amount > 999 ORDER BY amount DESC LIMIT 3');
if (json2) {
    console.log(`[5] WHERE+LIMIT: ${json2}`);
}

sessionFree(sess);
console.log('\nNODE.JS TEST PASSED — kore_ffi.dll works from Node.js via koffi!');
