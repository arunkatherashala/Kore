/**
 * KORE JavaScript Tests
 * Comprehensive test suite for all KORE operations
 */

const { Kore } = require('./index.js');

describe('KORE JavaScript Bindings', () => {
  const testFile = 'test_output.kore';
  const schema = {
    fields: [
      { name: 'id', type: 'int64' },
      { name: 'name', type: 'string' },
      { name: 'value', type: 'float64' }
    ]
  };
  const data = [
    { id: 1, name: 'test1', value: 10.5 },
    { id: 2, name: 'test2', value: 20.3 },
    { id: 3, name: 'test3', value: 30.1 }
  ];

  describe('Functional API', () => {
    test('should write data to KORE file', async () => {
      const result = await Kore.write(testFile, schema, data);
      expect(result).toContain('Written');
    });

    test('should read data from KORE file', async () => {
      const result = await Kore.read(testFile);
      expect(Array.isArray(result)).toBe(true);
      expect(result.length).toBe(3);
    });

    test('should read specific column', async () => {
      const result = await Kore.readColumn(testFile, 'name');
      expect(Array.isArray(result)).toBe(true);
      expect(result).toContain('test1');
    });

    test('should get file statistics', async () => {
      const stats = await Kore.getStats(testFile);
      expect(stats.rowCount).toBe(3);
      expect(stats.columnCount).toBe(3);
      expect(stats.fileSize).toBeGreaterThan(0);
    });
  });

  describe('Class API', () => {
    const kore = new Kore();

    test('should create new KORE instance', () => {
      expect(kore).toBeDefined();
    });

    test('should load KORE file', async () => {
      const result = await kore.load(testFile);
      expect(result).toBeDefined();
    });

    test('should get row count', async () => {
      const count = await kore.getRowCount();
      expect(count).toBe(3);
    });

    test('should get column count', async () => {
      const count = await kore.getColumnCount();
      expect(count).toBe(3);
    });

    test('should get column names', async () => {
      const cols = await kore.getColumnNames();
      expect(Array.isArray(cols)).toBe(true);
      expect(cols).toContain('id');
      expect(cols).toContain('name');
      expect(cols).toContain('value');
    });

    test('should read all data', async () => {
      const result = await kore.readAll();
      expect(Array.isArray(result)).toBe(true);
      expect(result.length).toBe(3);
    });

    test('should read single column', async () => {
      const column = await kore.readColumn('id');
      expect(Array.isArray(column)).toBe(true);
    });

    test('should get statistics', async () => {
      const stats = await kore.getStats();
      expect(stats.rowCount).toBe(3);
    });

    test('should save to new file', async () => {
      const result = await kore.save('test_copy.kore');
      expect(result).toContain('Saved');
    });
  });

  describe('Error Handling', () => {
    test('should handle non-existent file', async () => {
      expect(async () => {
        await Kore.read('nonexistent.kore');
      }).rejects.toThrow();
    });

    test('should handle invalid column name', async () => {
      expect(async () => {
        await Kore.readColumn(testFile, 'nonexistent_column');
      }).rejects.toThrow();
    });
  });

  describe('Performance', () => {
    test('write should complete in reasonable time', async () => {
      const start = Date.now();
      await Kore.write('perf_test.kore', schema, data);
      const duration = Date.now() - start;
      expect(duration).toBeLessThan(5000); // Should be fast
    });

    test('read should be fast', async () => {
      const start = Date.now();
      await Kore.read('perf_test.kore');
      const duration = Date.now() - start;
      expect(duration).toBeLessThan(5000);
    });
  });
});
