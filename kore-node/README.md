# KORE FileFormat — Node.js / TypeScript

**Version 1.7.7** | [npm](https://www.npmjs.com/package/kore-fileformat) | [GitHub](https://github.com/arunkatherashala/Kore)

World's fastest human-readable columnar format. `.kore` v3 opens in any text editor AND reads 12x faster than CSV.

## Install

```bash
npm install kore-fileformat
```

## .kore v3 Format

```
KORE2 offset=0000000455
# KORE Format v3.0
# Rows: 100,000  Columns: 3
# Compressed: 28,500 bytes (Rust ZSTD/LZ4)
# Schema:
#   price                F64
#   qty                  I64
# Preview (first 5 rows):
#   [price=10.5 | qty=100]
[binary compressed data]
```

## Quick Start

```javascript
const kore = require('kore-fileformat');

// Write
const block = {
  columns: [
    { name: 'price', dtype: 2, data: [10.5, 20.0, 30.75] },
    { name: 'qty',   dtype: 1, data: [100,  200,  300]   },
  ],
  numRows: 3
};
kore.writeFile('data.kore', block);

// Read
const result = kore.readFile('data.kore');
console.log(`${result.numRows} rows, ${result.columns.length} cols`);

// Inspect header only (instant, no data load)
const header = kore.readHeader('data.kore');
console.log(header);
```

## API Reference

| Function | Description |
|----------|-------------|
| `writeFile(path, block)` | Write .kore v3 (compressed + human-readable header) |
| `readFile(path)` | Read .kore → DataBlock |
| `readHeader(path)` | Read text header only (no data load) |
| `crc32(data: Buffer)` | CRC32 checksum → number |

## Benchmark

| Format | Read | File Size |
|--------|------|-----------|
| **KORE .kore** | **79 ns/row** | **305 KB** |
| JSON | 1,096 ns/row | 6,786 KB |
| CSV | 1,252 ns/row | 3,368 KB |

## Install

```bash
npm install kore-fileformat@1.7.7
```

Requires `kore_ffi.dll` / `libkore_ffi.so` (included in npm package or build from source):
```bash
cargo build --release -p kore-ffi
```

## Quick Start (JavaScript)

```javascript
const kore = require('./kore_ffi.js');

// --- Write ---
const block = {
  columns: [
    { name: 'price',    dtype: 2, data: [10.5, 20.0, 30.75] },
    { name: 'quantity', dtype: 1, data: [100,  200,  300]   },
  ],
  numRows: 3
};
kore.writeFile('data.kore', block);

// --- Read ---
const result = kore.readFile('data.kore');
console.log(`${result.numRows} rows, ${result.columns.length} columns`);

// --- CRC32 ---
const checksum = kore.crc32(Buffer.from('hello kore'));
console.log(`crc32 = 0x${checksum.toString(16)}`);  // 0x4b029b4b
```

## Quick Start (TypeScript)

```typescript
import * as kore from 'kore-fileformat';

const block = new kore.DataBlock();
block.addColumn('price', kore.DataType.F64, [10.5, 20.0, 30.75]);
block.addColumn('qty',   kore.DataType.I64, [100,  200,  300]);

await kore.writeFile('data.kore', block);
const result = await kore.readFile('data.kore');
console.log(result.numRows);  // 3
```

## API Reference

| Function | Description |
|----------|-------------|
| `crc32(data: Buffer)` | CRC32 checksum → number |
| `writeFile(path, block)` | Write DataBlock to .kore |
| `readFile(path)` | Read .kore → DataBlock |

## Run Tests

```bash
npx jest kore_fileformat.test.ts
node test_phase3.test.js
```
