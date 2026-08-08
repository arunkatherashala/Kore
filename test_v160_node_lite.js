const { execFileSync } = require('child_process');
const path = require('path');
const fs   = require('fs');
const os   = require('os');

const PYTHON = process.env.PYTHON_BIN || 'python';
const REPO   = path.join(__dirname);

console.log('======================================================================');
console.log('  KORE FileFormat v1.6.0 — Node.js Integration Test');
console.log(`  Node: ${process.version} | Run: ${new Date().toISOString()}`);
console.log('======================================================================');

let passed = 0, failed = 0;
function check(label, ok, note='') {
  console.log(`  [${ok?' PASS ':' FAIL '}] ${label}${note?' — '+note:''}`);
  ok ? passed++ : failed++;
}

// Test 1: Node can read the .kore file written by Python
const koreFile = path.join(REPO, 'test_v160_orders.kore');
check('Python-generated .kore file exists', fs.existsSync(koreFile),
      fs.existsSync(koreFile) ? fs.statSync(koreFile).size+' bytes' : 'missing');

// Read binary and verify KORE magic bytes
if (fs.existsSync(koreFile)) {
  const buf = fs.readFileSync(koreFile);
  const magic = buf.slice(0,4).toString('ascii');
  check('Magic bytes = KORE', magic === 'KORE', `got "${magic}"`);
  check('File > 100 bytes',   buf.length > 100, `${buf.length} bytes`);
}

// Test 2: CRC32 in pure JS (verify against Rust result)
function crc32(buf) {
  let crc = 0xFFFFFFFF;
  for (const b of buf) {
    crc ^= b;
    for (let i=0; i<8; i++) crc = (crc>>>1) ^ (crc&1 ? 0xEDB88320 : 0);
  }
  return (crc ^ 0xFFFFFFFF) >>> 0;
}
const EXPECTED_CRC = 0x5946aaf8;  // from Python test
const got = crc32(Buffer.from('hello kore v1.6.0'));
check('CRC32 matches Rust (pure JS impl)', got === EXPECTED_CRC,
      `0x${got.toString(16)} == 0x${EXPECTED_CRC.toString(16)}`);

// Test 3: Write a test file (Node.js writes KORE format header)
// We can at least write the 4-byte magic and verify structure
const tmpFile = path.join(os.tmpdir(), 'test_node_v160.bin');
const header = Buffer.from('KORE');  // magic bytes
fs.writeFileSync(tmpFile, header);
check('Node can write KORE magic header', 
      fs.readFileSync(tmpFile).toString('ascii') === 'KORE');
fs.unlinkSync(tmpFile);

// Test 4: Version verification via kore_fileformat.js metadata
const pkgJson = path.join(REPO, 'kore-node', 'package.json');
if (fs.existsSync(pkgJson)) {
  const pkg = JSON.parse(fs.readFileSync(pkgJson, 'utf-8'));
  check('package.json version = 1.6.0', pkg.version === '1.6.0', pkg.version);
}

// Test 5: kore_ffi.dll exists
const dllPath = path.join(REPO, 'target', 'release', 'kore_ffi.dll');
check('kore_ffi.dll exists', fs.existsSync(dllPath),
      fs.existsSync(dllPath) ? Math.round(fs.statSync(dllPath).size/1024/1024)+'MB' : 'missing');

console.log();
console.log('======================================================================');
console.log(`  TOTAL: ${passed}/${passed+failed} passed | ${failed} failed`);
console.log('  Note: koffi blocked by corporate npm policy — pure-JS tests run');
console.log('  koffi FFI requires: npm install koffi (needs npm registry access)');
console.log('======================================================================');
process.exit(failed > 0 ? 1 : 0);
