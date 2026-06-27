/**
 * Kore Cloud Connectors for Node.js
 * 
 * Unified API for accessing data in AWS S3, Azure Blob Storage, and Google Cloud Storage
 * 
 * @example
 * ```javascript
 * const { S3Reader } = require('@kore/cloud');
 * 
 * const reader = new S3Reader('us-east-1');
 * const data = await reader.readFile('my-bucket', 'path/to/file.kore');
 * reader.close();
 * ```
 */

const kore = require('./index.node');

module.exports = {
  S3Reader: kore.S3Reader,
  AzureBlobReader: kore.AzureBlobReader,
  GcsReader: kore.GcsReader,
};
