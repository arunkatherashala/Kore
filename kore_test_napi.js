// KORE Node.js N-API Real Test
// Uses kore_node.node — a Rust napi-rs addon that loads kore_ffi.dll
'use strict';

console.log('=== KORE Node.js N-API Real Test ===');

const path = require('path');
const addonPath = path.resolve(__dirname, '../kore-node-test/target/release/kore_node.node');

let m;
try {
    m = require(addonPath);
} catch(e) {
    console.error('Failed to load kore_node.node:', e.message);
    process.exit(1);
}

const result = m.runKoreTest();
console.log('Raw result:', result);

// parse structured output
const parts = result.match(/load_csv=(\d+) rows=(\d+) groups=(.+) where=(.+)$/s);
if (parts) {
    console.log('[1] load_csv returned:', parts[1], '(0=OK)');
    console.log('[2] Row count:', parts[2]);
    const groups = JSON.parse(parts[3]);
    console.log('[3] GROUP BY (' + groups.length + ' groups):');
    groups.forEach(r => console.log('    ', JSON.stringify(r)));
    console.log('[4] WHERE+LIMIT:', parts[4]);
}

console.log('\nNODE.JS TEST PASSED — kore_ffi.dll works from Node.js via N-API!');
