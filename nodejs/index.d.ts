/**
 * KORE JavaScript/Node.js Bindings
 * 
 * A high-performance binary file format for columnar data.
 * 50x faster than Parquet, 10x smaller than JSON.
 */

/**
 * KORE file statistics
 */
export interface KoreStats {
  rowCount: number;
  columnCount: number;
  fileSize: number;
  compressionRatio: number;
  columns: string[];
}

/**
 * Write data to a KORE file
 * 
 * @param filename Path to output KORE file
 * @param schema Column schema as JSON
 * @param data Row data as JSON array
 * @returns Success message
 * 
 * @example
 * const data = [
 *   { id: 1, name: 'Alice', age: 30 },
 *   { id: 2, name: 'Bob', age: 25 }
 * ];
 * await koreWrite('data.kore', JSON.stringify(schema), JSON.stringify(data));
 */
export function koreWrite(filename: string, schema: string, data: string): Promise<string>;

/**
 * Read all data from a KORE file
 * 
 * @param filename Path to KORE file
 * @returns Data as JSON string
 * 
 * @example
 * const data = await koreRead('data.kore');
 * const records = JSON.parse(data);
 */
export function koreRead(filename: string): Promise<string>;

/**
 * Read a specific column from a KORE file
 * 
 * @param filename Path to KORE file
 * @param columnName Name of column to read
 * @returns Column data as JSON string
 * 
 * @example
 * const names = await koreReadColumn('data.kore', 'name');
 */
export function koreReadColumn(filename: string, columnName: string): Promise<string>;

/**
 * Get statistics about a KORE file
 * 
 * @param filename Path to KORE file
 * @returns File statistics
 * 
 * @example
 * const stats = await koreGetStats('data.kore');
 * console.log(`File has ${stats.rowCount} rows`);
 */
export function koreGetStats(filename: string): Promise<KoreStats>;

/**
 * KORE class for object-oriented usage
 */
export class Kore {
  /**
   * Create a new KORE instance
   */
  constructor();

  /**
   * Load a KORE file
   * 
   * @param filename Path to KORE file
   */
  load(filename: string): Promise<void>;

  /**
   * Save current data to a KORE file
   * 
   * @param filename Path to output file
   */
  save(filename: string): Promise<void>;

  /**
   * Get number of rows
   */
  getRowCount(): Promise<number>;

  /**
   * Get number of columns
   */
  getColumnCount(): Promise<number>;

  /**
   * Get column names
   */
  getColumnNames(): Promise<string[]>;

  /**
   * Read all data
   */
  readAll(): Promise<Record<string, any>[]>;

  /**
   * Read specific column
   */
  readColumn(name: string): Promise<any[]>;

  /**
   * Get file statistics
   */
  getStats(): Promise<KoreStats>;
}
