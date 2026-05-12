/**
 * KORE JavaScript/Node.js Wrapper
 * Provides user-friendly async API for KORE file operations
 */

const nativeModule = require('../index.node');

/**
 * Write data to a KORE file
 * 
 * @example
 * const data = [
 *   { id: 1, name: 'Alice', age: 30 },
 *   { id: 2, name: 'Bob', age: 25 }
 * ];
 * 
 * const schema = {
 *   fields: [
 *     { name: 'id', type: 'int64' },
 *     { name: 'name', type: 'string' },
 *     { name: 'age', type: 'int32' }
 *   ]
 * };
 * 
 * await Kore.write('data.kore', schema, data);
 */
async function write(filename, schema, data) {
  return nativeModule.kore_write(
    filename,
    JSON.stringify(schema),
    JSON.stringify(data)
  );
}

/**
 * Read all data from a KORE file
 * 
 * @example
 * const data = await Kore.read('data.kore');
 * console.log(data); // Array of records
 */
async function read(filename) {
  const result = await nativeModule.kore_read(filename);
  return JSON.parse(result);
}

/**
 * Read a specific column
 * 
 * @example
 * const names = await Kore.readColumn('data.kore', 'name');
 * console.log(names); // ['Alice', 'Bob', ...]
 */
async function readColumn(filename, columnName) {
  const result = await nativeModule.kore_read_column(filename, columnName);
  return JSON.parse(result);
}

/**
 * Get file statistics
 * 
 * @example
 * const stats = await Kore.getStats('data.kore');
 * console.log(`Rows: ${stats.rowCount}, Size: ${stats.fileSize} bytes`);
 */
async function getStats(filename) {
  return nativeModule.kore_get_stats(filename);
}

module.exports = {
  write,
  read,
  readColumn,
  getStats,
  Kore: nativeModule.KoreNodeJs
};
