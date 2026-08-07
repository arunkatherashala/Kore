/**
 * KORE Node.js FFI Integration Tests
 * ===================================
 *
 * Tests for Node.js N-API wrapper around Rust kore-ffi library.
 */

import * as kore from './kore_fileformat';
import * as fs from 'fs';
import * as path from 'path';
import * as assert from 'assert';

// ─────────────────────────────────────────────────────────────────────────────
// DATA TYPES
// ─────────────────────────────────────────────────────────────────────────────

describe('DataTypes', () => {
  it('should have correct DataType enum values', () => {
    assert.strictEqual(kore.DataType.I64, 1);
    assert.strictEqual(kore.DataType.F64, 2);
    assert.strictEqual(kore.DataType.BOOL, 3);
    assert.strictEqual(kore.DataType.STR, 4);
    assert.strictEqual(kore.DataType.STR_DICT, 5);
    assert.strictEqual(kore.DataType.ARRAY, 6);
    assert.strictEqual(kore.DataType.STRUCT, 7);
  });

  it('should have correct Compression enum values', () => {
    assert.strictEqual(kore.Compression.RAW, 0);
    assert.strictEqual(kore.Compression.RLE, 1);
    assert.strictEqual(kore.Compression.DELTA, 2);
    assert.strictEqual(kore.Compression.DICT, 3);
    assert.strictEqual(kore.Compression.NAN_RAW, 4);
    assert.strictEqual(kore.Compression.DEFLATE, 5);
    assert.strictEqual(kore.Compression.ZSTD, 6);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// DATA BLOCK
// ─────────────────────────────────────────────────────────────────────────────

describe('DataBlock', () => {
  it('should create empty data block', () => {
    const block = kore.createDataBlock();
    assert.strictEqual(block.numRows, 0);
    assert.strictEqual(block.numColumns, 0);
    assert.strictEqual(block.columns.length, 0);
  });

  it('should add single column', () => {
    const block = kore.createDataBlock();
    block.addColumn('numbers', kore.DataType.I64, [1, 2, 3, 4, 5]);

    assert.strictEqual(block.numRows, 5);
    assert.strictEqual(block.numColumns, 1);
    assert(block.getColumn('numbers') !== undefined);
  });

  it('should add multiple columns', () => {
    const block = kore.createDataBlock();
    block.addColumn('numbers', kore.DataType.I64, [1, 2, 3]);
    block.addColumn('names', kore.DataType.STR, ['a', 'b', 'c']);

    assert.strictEqual(block.numRows, 3);
    assert.strictEqual(block.numColumns, 2);
  });

  it('should reject mismatched row counts', () => {
    const block = kore.createDataBlock();
    block.addColumn('numbers', kore.DataType.I64, [1, 2, 3]);

    assert.throws(() => {
      block.addColumn('names', kore.DataType.STR, ['a', 'b']); // 2 rows != 3
    });
  });

  it('should retrieve column by name', () => {
    const block = kore.createDataBlock();
    block.addColumn('test', kore.DataType.I64, [10, 20, 30]);

    const col = block.getColumn('test');
    assert(col !== undefined);
    assert.strictEqual(col!.name, 'test');
    assert.strictEqual(col!.dtype, kore.DataType.I64);
    assert.deepStrictEqual(col!.data, [10, 20, 30]);
  });

  it('should return undefined for non-existent column', () => {
    const block = kore.createDataBlock();
    block.addColumn('test', kore.DataType.I64, [1, 2, 3]);

    assert.strictEqual(block.getColumn('nonexistent'), undefined);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// ROUNDTRIP (Phase 3 placeholder - JSON fallback)
// ─────────────────────────────────────────────────────────────────────────────

describe('Roundtrip', () => {
  it('should write and read data block', async () => {
    const block = kore.createDataBlock();
    block.addColumn('numbers', kore.DataType.I64, [1, 2, 3, 4, 5]);
    block.addColumn('names', kore.DataType.STR, ['a', 'b', 'c', 'd', 'e']);

    const tmpFile = path.join('/tmp', `kore_test_${Date.now()}.kore`);

    try {
      // Write
      await kore.writeFile(tmpFile, block);
      assert(fs.existsSync(tmpFile));

      // Read
      const restored = await kore.readFile(tmpFile);

      assert.strictEqual(restored.numRows, 5);
      assert.strictEqual(restored.numColumns, 2);

      // Verify columns
      const numbersCol = restored.getColumn('numbers');
      assert(numbersCol !== undefined);
      assert.deepStrictEqual(numbersCol!.data, [1, 2, 3, 4, 5]);

      const namesCol = restored.getColumn('names');
      assert(namesCol !== undefined);
      assert.deepStrictEqual(namesCol!.data, ['a', 'b', 'c', 'd', 'e']);
    } finally {
      if (fs.existsSync(tmpFile)) {
        fs.unlinkSync(tmpFile);
      }
    }
  });

  it('should write and read float column', async () => {
    const block = kore.createDataBlock();
    block.addColumn('decimals', kore.DataType.F64, [1.1, 2.2, 3.3]);

    const tmpFile = path.join('/tmp', `kore_test_float_${Date.now()}.kore`);

    try {
      await kore.writeFile(tmpFile, block);
      const restored = await kore.readFile(tmpFile);

      const col = restored.getColumn('decimals');
      assert(col !== undefined);
      assert.strictEqual(col!.data.length, 3);
    } finally {
      if (fs.existsSync(tmpFile)) {
        fs.unlinkSync(tmpFile);
      }
    }
  });

  it('should write and read boolean column', async () => {
    const block = kore.createDataBlock();
    block.addColumn('flags', kore.DataType.BOOL, [true, false, true]);

    const tmpFile = path.join('/tmp', `kore_test_bool_${Date.now()}.kore`);

    try {
      await kore.writeFile(tmpFile, block);
      const restored = await kore.readFile(tmpFile);

      const col = restored.getColumn('flags');
      assert(col !== undefined);
      assert.deepStrictEqual(col!.data, [true, false, true]);
    } finally {
      if (fs.existsSync(tmpFile)) {
        fs.unlinkSync(tmpFile);
      }
    }
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// COLUMN STATS (Phase 3)
// ─────────────────────────────────────────────────────────────────────────────

describe('ColumnStats', () => {
  it('should create column statistics', () => {
    const stats: kore.ColumnStats = {
      minValue: 1,
      maxValue: 100,
      nullCount: 0,
      cardinality: 50,
      crc32: 0xdeadbeef,
    };

    assert.strictEqual(stats.minValue, 1);
    assert.strictEqual(stats.maxValue, 100);
    assert.strictEqual(stats.nullCount, 0);
    assert.strictEqual(stats.cardinality, 50);
    assert.strictEqual(stats.crc32, 0xdeadbeef);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// VERSION CONTROL (Phase 3)
// ─────────────────────────────────────────────────────────────────────────────

describe('VersionControl', () => {
  it('should read at version (Phase 3: pending)', () => {
    // Phase 3: Implement once version snapshots integrated
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// ENCRYPTION (Phase 3)
// ─────────────────────────────────────────────────────────────────────────────

describe('Encryption', () => {
  it('should encrypt and decrypt (Phase 3: pending)', () => {
    // Phase 3: Implement once crypto FFI exposed
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// BLOOM FILTERS (Phase 3)
// ─────────────────────────────────────────────────────────────────────────────

describe('BloomFilters', () => {
  it('should retrieve Bloom filter (Phase 3: pending)', () => {
    // Phase 3: Implement once filter APIs exposed
  });
});

// Run tests if this is main module
if (require.main === module) {
  console.log('Run with: npx mocha kore_fileformat.test.ts');
}
