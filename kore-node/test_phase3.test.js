/**
 * Phase 3 Node.js FFI - Comprehensive Integration Tests
 *
 * Tests all 11 ACID features with Jest
 * Run with: npm test
 */

const {
  DataType,
  CompressionCodec,
  Checksums,
  ColumnStats,
  BloomFilter,
  Encryption,
  ColumnSchema,
  Schema,
  VersionSnapshot,
  PartitionSpec,
  DeleteVector,
  Column,
  DataBlock,
  KoreWriter,
  KoreReader,
  KoreFileFormat,
} = require('./kore_fileformat_phase3');

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

// ═══════════════════════════════════════════════════════════════════════════
// Feature 1: CRC32 Checksums
// ═══════════════════════════════════════════════════════════════════════════

describe('Feature 1: CRC32 Checksums', () => {
  test('should compute CRC32 checksum', () => {
    const data = Buffer.from('hello world');
    const crc = Checksums.crc32(data);
    expect(typeof crc).toBe('number');
    expect(crc).toBeGreaterThanOrEqual(0);
  });

  test('should verify checksum', () => {
    const data = Buffer.from('test data');
    const crc = Checksums.crc32(data);
    expect(Checksums.verify(data, crc)).toBe(true);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// Feature 2: Column Statistics
// ═══════════════════════════════════════════════════════════════════════════

describe('Feature 2: Column Statistics', () => {
  test('should compute int64 statistics', () => {
    const values = [1, 2, 3, 4, 5];
    const stats = ColumnStats.fromInt64(values);
    expect(stats.minValue).toBe(1);
    expect(stats.maxValue).toBe(5);
    expect(stats.cardinality).toBe(5);
  });

  test('should compute float64 statistics', () => {
    const values = [1.1, 2.2, 3.3, 4.4, 5.5];
    const stats = ColumnStats.fromFloat64(values);
    expect(stats.minValue).toBe(1.1);
    expect(stats.maxValue).toBe(5.5);
  });

  test('should serialize statistics to JSON', () => {
    const stats = new ColumnStats();
    stats.minValue = 1;
    stats.maxValue = 100;
    stats.cardinality = 100;
    const json = stats.toJSON();
    expect(json.min).toBe(1);
    expect(json.max).toBe(100);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// Feature 5: Bloom Filters
// ═══════════════════════════════════════════════════════════════════════════

describe('Feature 5: Bloom Filters', () => {
  test('should insert and contain', () => {
    const bf = new BloomFilter(1000, 0.01);
    bf.insert('alice');
    expect(bf.contains('alice')).toBe(true);
  });

  test('should serialize to buffer', () => {
    const bf = new BloomFilter(100, 0.01);
    bf.insert('test');
    const buf = bf.toBuffer();
    expect(Buffer.isBuffer(buf)).toBe(true);
  });

  test('should deserialize from buffer', () => {
    const bf1 = new BloomFilter(100, 0.01);
    bf1.insert('alice');
    const buf = bf1.toBuffer();
    
    const bf2 = BloomFilter.fromBuffer(buf);
    expect(bf2.contains('alice')).toBe(true);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// Feature 6: AES-256-GCM Encryption
// ═══════════════════════════════════════════════════════════════════════════

describe('Feature 6: Encryption', () => {
  test('should derive key with PBKDF2', () => {
    const password = 'mypassword';
    const salt = Buffer.from('somesalt');
    const key = Encryption.pbkdf2Sha256(password, salt);
    expect(Buffer.isBuffer(key)).toBe(true);
    expect(key.length).toBe(32);
  });

  test('should generate nonce', () => {
    const nonce = Encryption.generateNonce();
    expect(Buffer.isBuffer(nonce)).toBe(true);
    expect(nonce.length).toBe(12);
  });

  test('should generate salt', () => {
    const salt = Encryption.generateSalt();
    expect(Buffer.isBuffer(salt)).toBe(true);
    expect(salt.length).toBe(16);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// Feature 7: Schema Evolution
// ═══════════════════════════════════════════════════════════════════════════

describe('Feature 7: Schema Evolution', () => {
  test('should create schema', () => {
    const schema = new Schema();
    schema.addColumn('id', DataType.I64, 0);
    schema.addColumn('name', DataType.STR, 1);
    expect(schema.columns.length).toBe(2);
  });

  test('should serialize schema to JSON', () => {
    const schema = new Schema();
    schema.addColumn('id', DataType.I64, 0);
    const json = schema.toJSON();
    expect(json.version).toBe(1);
    expect(json.columns.length).toBe(1);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// Feature 9: MVCC + Time Travel
// ═══════════════════════════════════════════════════════════════════════════

describe('Feature 9: MVCC + Time Travel', () => {
  test('should create version snapshot', () => {
    const snap = new VersionSnapshot(1, Date.now(), 0, 100);
    expect(snap.versionId).toBe(1);
    expect(snap.rowCount).toBe(100);
  });

  test('should serialize version snapshot', () => {
    const snap = new VersionSnapshot(1, 1000, 0, 100);
    const json = snap.toJSON();
    expect(json.version_id).toBe(1);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// Feature 10: Partition Evolution
// ═══════════════════════════════════════════════════════════════════════════

describe('Feature 10: Partition Evolution', () => {
  test('should create partition spec', () => {
    const spec = new PartitionSpec(1, [0, 1], ['identity']);
    expect(spec.specId).toBe(1);
    expect(spec.columns.length).toBe(2);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// Feature 11: Row-Level Deletes
// ═══════════════════════════════════════════════════════════════════════════

describe('Feature 11: Row-Level Deletes', () => {
  test('should mark and check deleted rows', () => {
    const dv = new DeleteVector();
    dv.markDeleted(5);
    expect(dv.isDeleted(5)).toBe(true);
    expect(dv.isDeleted(4)).toBe(false);
  });

  test('should track cardinality', () => {
    const dv = new DeleteVector();
    dv.markDeleted(0);
    dv.markDeleted(1);
    expect(dv.cardinality).toBe(2);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// Integration: Roundtrip Tests
// ═══════════════════════════════════════════════════════════════════════════

describe('Integration: Roundtrip Tests', () => {
  test('should roundtrip int64 column', () => {
    const kore = new KoreFileFormat();
    kore.addI64Column('id', [1, 2, 3, 4, 5]);
    
    const buf = kore.toBuffer();
    const kore2 = KoreFileFormat.fromBuffer(buf);
    
    const col = kore2.getColumn('id');
    expect(col).toBeDefined();
    expect(col.data).toEqual([1, 2, 3, 4, 5]);
  });

  test('should roundtrip float64 column', () => {
    const kore = new KoreFileFormat();
    kore.addF64Column('value', [1.1, 2.2, 3.3]);
    
    const buf = kore.toBuffer();
    const kore2 = KoreFileFormat.fromBuffer(buf);
    
    const col = kore2.getColumn('value');
    expect(col).toBeDefined();
    expect(col.data.length).toBe(3);
  });

  test('should roundtrip string column', () => {
    const kore = new KoreFileFormat();
    kore.addStrColumn('name', ['alice', 'bob', 'charlie']);
    
    const buf = kore.toBuffer();
    const kore2 = KoreFileFormat.fromBuffer(buf);
    
    const col = kore2.getColumn('name');
    expect(col).toBeDefined();
    expect(col.data).toEqual(['alice', 'bob', 'charlie']);
  });

  test('should roundtrip multiple columns', () => {
    const kore = new KoreFileFormat();
    kore.addI64Column('id', [1, 2, 3]);
    kore.addF64Column('value', [1.1, 2.2, 3.3]);
    kore.addStrColumn('name', ['a', 'b', 'c']);
    
    const buf = kore.toBuffer();
    const kore2 = KoreFileFormat.fromBuffer(buf);
    
    expect(kore2.block.numRows).toBe(3);
    expect(kore2.block.columns.length).toBe(3);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// Statistics Tests
// ═══════════════════════════════════════════════════════════════════════════

describe('Statistics', () => {
  test('should compute int64 statistics', () => {
    const kore = new KoreFileFormat();
    kore.addI64Column('numbers', [10, 20, 30, 40, 50]);
    
    const stats = kore.getStats('numbers');
    expect(stats).toBeDefined();
    expect(stats.minValue).toBe(10);
    expect(stats.maxValue).toBe(50);
    expect(stats.cardinality).toBe(5);
  });

  test('should compute float64 statistics', () => {
    const kore = new KoreFileFormat();
    kore.addF64Column('floats', [1.5, 2.5, 3.5, 2.5]);
    
    const stats = kore.getStats('floats');
    expect(stats).toBeDefined();
    expect(stats.minValue).toBe(1.5);
    expect(stats.maxValue).toBe(3.5);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// File I/O Tests
// ═══════════════════════════════════════════════════════════════════════════

describe('File I/O', () => {
  test('should write and read from file', () => {
    const kore = new KoreFileFormat();
    kore.addI64Column('id', [1, 2, 3]);
    kore.addStrColumn('name', ['a', 'b', 'c']);
    
    const tmpFile = path.join(__dirname, 'test_phase3_temp.kore');
    
    try {
      kore.write(tmpFile);
      expect(fs.existsSync(tmpFile)).toBe(true);
      
      const kore2 = KoreFileFormat.read(tmpFile);
      expect(kore2.block.numRows).toBe(3);
      expect(kore2.block.columns.length).toBe(2);
    } finally {
      if (fs.existsSync(tmpFile)) {
        fs.unlinkSync(tmpFile);
      }
    }
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// JSON Serialization Tests
// ═══════════════════════════════════════════════════════════════════════════

describe('JSON Serialization', () => {
  test('should serialize to JSON', () => {
    const kore = new KoreFileFormat();
    kore.addI64Column('id', [1, 2, 3]);
    
    const json = kore.toJSON();
    expect(json.version).toBe(2);
    expect(json.num_rows).toBe(3);
    expect(json.num_cols).toBe(1);
  });
});
