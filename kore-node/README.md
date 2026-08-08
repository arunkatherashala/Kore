# KORE FileFormat — Node.js / TypeScript

**Version 1.6.5** | [npm](https://www.npmjs.com/package/kore-fileformat) | [GitHub](https://github.com/arunkatherashala/Kore)

High-performance columnar format with 11 ACID features. Uses `koffi` for zero-compilation FFI to Rust `kore_ffi.dll`.

## Install

```bash
npm install kore-fileformat@1.6.5
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
