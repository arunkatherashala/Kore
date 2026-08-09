/**
 * KORE Multi-Engine Connectors for Node.js
 * 
 * Bridges KORE format to Apache Arrow, DuckDB, and Apache Spark via REST.
 * 
 * Install: npm install kore-fileformat @apache-arrow duckdb
 */

const kore = require('./kore_ffi.js');

/**
 * Convert a .kore file to Apache Arrow IPC format (works with Spark, DuckDB, Polars).
 * 
 * @example
 * const { tableToIPC } = require('@apache-arrow');
 * const table = await kore.toArrow('data.kore');
 * // Use with DuckDB:
 * const db = new Database(':memory:');
 * db.register_buffer('sales', [tableToIPC(table)], true);
 * db.all('SELECT SUM(amount) FROM sales', callback);
 */
async function toArrow(korePath) {
  const { Table, Float64, Int64, Utf8, Field, Schema, makeVector } = require('@apache-arrow');
  const block = await kore.readFile(korePath);
  
  const fields = block.columns.map(col => {
    const arrowType = col.type === 'F64' ? new Float64() :
                      col.type === 'I64' ? new Int64() : new Utf8();
    return new Field(col.name, arrowType, true);
  });
  
  const schema = new Schema(fields);
  const vectors = block.columns.map(col => makeVector(col.data));
  return new Table(schema, vectors);
}

/**
 * Register a .kore file as a DuckDB table for SQL queries.
 * 
 * @example
 * const conn = await kore.toDuckDB('sales.kore', 'sales');
 * const result = conn.all('SELECT region, SUM(amount) FROM sales GROUP BY region');
 */
async function toDuckDB(korePath, tableName = 'kore_table') {
  const Database = require('duckdb');
  const db = new Database(':memory:');
  const conn = db.connect();
  
  // Use Arrow IPC for zero-copy transfer
  const arrowTable = await toArrow(korePath);
  const { tableToIPC } = require('@apache-arrow');
  const ipc = tableToIPC(arrowTable);
  conn.register_buffer(tableName, [ipc], true);
  return conn;
}

/**
 * Export .kore to Parquet for Apache Spark native reading.
 * 
 * @example
 * await kore.toParquet('data.kore', 'data.parquet');
 * // In PySpark: spark.read.parquet('data.parquet')
 */
async function toParquet(korePath, parquetPath) {
  const { tableToIPC } = require('@apache-arrow');
  const { ParquetWriter, ParquetSchema } = require('parquetjs');
  const table = await toArrow(korePath);
  // Convert Arrow → Parquet via schema mapping
  const schema = {};
  table.schema.fields.forEach(f => {
    schema[f.name] = { type: f.type.toString().includes('Float') ? 'DOUBLE' : 'INT64' };
  });
  const writer = await ParquetWriter.openFile(new ParquetSchema(schema), parquetPath);
  for (let i = 0; i < table.numRows; i++) {
    const row = {};
    table.schema.fields.forEach((f, fi) => {
      row[f.name] = table.getColumn(fi).get(i);
    });
    await writer.appendRow(row);
  }
  await writer.close();
}

/**
 * Spark REST API integration — submit a Spark job that reads KORE files.
 * Requires a running Spark Thrift Server or Spark REST API.
 * 
 * @example
 * const result = await kore.sparkSQL(
 *   'SELECT region, SUM(amount) FROM kore_table GROUP BY region',
 *   'hdfs://cluster/data/sales.kore',
 *   'http://spark-master:6066'
 * );
 */
async function sparkSQL(sql, korePath, sparkRestUrl) {
  // Register file with Spark via REST API
  const response = await fetch(`${sparkRestUrl}/v1/submissions/create`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      action: 'CreateSubmissionRequest',
      mainClass: 'com.kore.spark.KoreSparkSQL',
      appArgs: [korePath, sql],
      sparkProperties: {
        'spark.jars': 'kore-spark.jar',
        'spark.master': sparkRestUrl.replace(':6066', ':7077')
      }
    })
  });
  return response.json();
}

module.exports = { toArrow, toDuckDB, toParquet, sparkSQL };
