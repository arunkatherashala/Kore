package com.kore.cloud;

/**
 * Java wrapper for Rust S3Reader
 * 
 * Usage:
 * <pre>
 * S3Reader reader = new S3Reader("us-east-1");
 * byte[] data = reader.readFile("my-bucket", "path/to/file");
 * reader.close();
 * </pre>
 */
public class S3Reader {
    private long nativePtr;

    static {
        System.loadLibrary("kore_java");
    }

    /**
     * Create a new S3Reader for the specified region
     * 
     * @param region AWS region (e.g., "us-east-1", "eu-west-1")
     */
    public S3Reader(String region) {
        this.nativePtr = newInstance(region);
        if (nativePtr == 0) {
            throw new RuntimeException("Failed to initialize S3Reader");
        }
    }

    /**
     * Read file from S3
     * 
     * @param bucket S3 bucket name
     * @param key Object key/path
     * @return File contents as byte array
     * @throws IOException If read fails
     */
    public native byte[] readFile(String bucket, String key) throws IOException;

    /**
     * Write file to S3
     * 
     * @param bucket S3 bucket name
     * @param key Object key/path
     * @param data File contents to write
     * @throws IOException If write fails
     */
    public native void writeFile(String bucket, String key, byte[] data) throws IOException;

    /**
     * List objects in S3 bucket
     * 
     * @param bucket S3 bucket name
     * @param prefix Optional prefix to filter results
     * @return Array of object keys
     * @throws IOException If list fails
     */
    public native String[] listFiles(String bucket, String prefix) throws IOException;

    /**
     * Enable local caching for S3 reads
     * 
     * @param cacheDir Directory to store cached files
     * @return This reader for chaining
     */
    public native S3Reader withCache(String cacheDir);

    /**
     * Get metadata for an S3 object
     * 
     * @param bucket S3 bucket name
     * @param key Object key
     * @return Metadata object containing size, modified time, etag, content type
     * @throws IOException If metadata fetch fails
     */
    public native S3ObjectMetadata getMetadata(String bucket, String key) throws IOException;

    /**
     * Close and release native resources
     */
    public void close() {
        if (nativePtr != 0) {
            cleanup(nativePtr);
            nativePtr = 0;
        }
    }

    @Override
    protected void finalize() throws Throwable {
        close();
        super.finalize();
    }

    private native long newInstance(String region);
    private native void cleanup(long ptr);

    /**
     * Metadata for S3 objects
     */
    public static class S3ObjectMetadata {
        public long size;
        public String lastModified;
        public String etag;
        public String contentType;

        public S3ObjectMetadata(long size, String lastModified, String etag, String contentType) {
            this.size = size;
            this.lastModified = lastModified;
            this.etag = etag;
            this.contentType = contentType;
        }
    }
}
