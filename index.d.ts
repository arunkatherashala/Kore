/**
 * TypeScript definitions for Kore Cloud Connectors
 */

/**
 * S3Reader - Read/write data from AWS S3
 */
export class S3Reader {
  /**
   * Create a new S3Reader for the specified region
   * @param region AWS region (e.g., "us-east-1")
   */
  constructor(region: string);

  /**
   * Enable local caching for S3 reads
   * @param cacheDir Directory to store cached files
   * @returns This reader for method chaining
   */
  withCache(cacheDir: string): S3Reader;

  /**
   * Read a file from S3
   * @param bucket S3 bucket name
   * @param key Object key/path
   * @returns File contents as Buffer
   */
  readFile(bucket: string, key: string): Promise<Buffer>;

  /**
   * Write a file to S3
   * @param bucket S3 bucket name
   * @param key Object key/path
   * @param data Data to write
   */
  writeFile(bucket: string, key: string, data: Buffer): Promise<void>;

  /**
   * List objects in an S3 bucket
   * @param bucket S3 bucket name
   * @param prefix Optional prefix to filter results
   * @returns Array of object keys
   */
  listFiles(bucket: string, prefix?: string): Promise<string[]>;

  /**
   * Get metadata for an S3 object
   * @param bucket S3 bucket name
   * @param key Object key
   * @returns Object metadata (size, lastModified, etag, contentType)
   */
  getMetadata(
    bucket: string,
    key: string,
  ): Promise<S3Metadata>;
}

export interface S3Metadata {
  size: number;
  last_modified: string;
  etag: string;
  content_type: string;
}

/**
 * AzureBlobReader - Read/write data from Azure Blob Storage
 */
export class AzureBlobReader {
  /**
   * Create a new AzureBlobReader
   * @param storageAccount Azure storage account name
   * @param accountKey Azure storage account key
   */
  constructor(storageAccount: string, accountKey: string);

  /**
   * Enable local caching for Azure reads
   * @param cacheDir Directory to store cached files
   * @returns This reader for method chaining
   */
  withCache(cacheDir: string): AzureBlobReader;

  /**
   * Read a blob from Azure Blob Storage
   * @param container Container name
   * @param blobPath Blob path/name
   * @returns Blob contents as Buffer
   */
  readFile(container: string, blobPath: string): Promise<Buffer>;

  /**
   * Write a blob to Azure Blob Storage
   * @param container Container name
   * @param blobPath Blob path/name
   * @param data Data to write
   */
  writeFile(container: string, blobPath: string, data: Buffer): Promise<void>;

  /**
   * List blobs in an Azure container
   * @param container Container name
   * @param prefix Optional prefix to filter results
   * @returns Array of blob paths
   */
  listBlobs(container: string, prefix?: string): Promise<string[]>;

  /**
   * Get metadata for an Azure blob
   * @param container Container name
   * @param blobPath Blob path
   * @returns Blob metadata (size, lastModified, etag, contentType)
   */
  getMetadata(
    container: string,
    blobPath: string,
  ): Promise<AzureMetadata>;
}

export interface AzureMetadata {
  size: number;
  last_modified: string;
  etag: string;
  content_type: string;
}

/**
 * GcsReader - Read/write data from Google Cloud Storage
 */
export class GcsReader {
  /**
   * Create a new GcsReader
   * @param projectId Google Cloud project ID
   */
  constructor(projectId: string);

  /**
   * Enable local caching for GCS reads
   * @param cacheDir Directory to store cached files
   * @returns This reader for method chaining
   */
  withCache(cacheDir: string): GcsReader;

  /**
   * Read an object from Google Cloud Storage
   * @param bucket GCS bucket name
   * @param objectPath Object path/name
   * @returns Object contents as Buffer
   */
  readFile(bucket: string, objectPath: string): Promise<Buffer>;

  /**
   * Write an object to Google Cloud Storage
   * @param bucket GCS bucket name
   * @param objectPath Object path/name
   * @param data Data to write
   */
  writeFile(bucket: string, objectPath: string, data: Buffer): Promise<void>;

  /**
   * List objects in a GCS bucket
   * @param bucket GCS bucket name
   * @param prefix Optional prefix to filter results
   * @returns Array of object paths
   */
  listObjects(bucket: string, prefix?: string): Promise<string[]>;

  /**
   * Get metadata for a GCS object
   * @param bucket GCS bucket name
   * @param objectPath Object path
   * @returns Object metadata (size, updated, generation, contentType)
   */
  getMetadata(
    bucket: string,
    objectPath: string,
  ): Promise<GcsMetadata>;
}

export interface GcsMetadata {
  size: number;
  updated: string;
  generation: string;
  content_type: string;
}
