package com.kore.cloud;
import java.io.IOException;

/**
 * Java wrapper for Rust GcsReader
 * 
 * Usage:
 * <pre>
 * GcsReader reader = new GcsReader("my-project-id");
 * byte[] data = reader.readFile("my-bucket", "path/to/object");
 * reader.close();
 * </pre>
 */
public class GcsReader {
    private long nativePtr;

    static {
        System.loadLibrary("kore_java");
    }

    /**
     * Create a new GcsReader
     * 
     * @param projectId Google Cloud project ID
     */
    public GcsReader(String projectId) {
        this.nativePtr = newInstance(projectId);
        if (nativePtr == 0) {
            throw new RuntimeException("Failed to initialize GcsReader");
        }
    }

    /**
     * Read object from Google Cloud Storage
     * 
     * @param bucket GCS bucket name
     * @param objectPath Object path/name
     * @return Object contents as byte array
     * @throws IOException If read fails
     */
    public native byte[] readFile(String bucket, String objectPath) throws IOException;

    /**
     * Write object to Google Cloud Storage
     * 
     * @param bucket GCS bucket name
     * @param objectPath Object path/name
     * @param data Object contents to write
     * @throws IOException If write fails
     */
    public native void writeFile(String bucket, String objectPath, byte[] data) throws IOException;

    /**
     * List objects in GCS bucket
     * 
     * @param bucket GCS bucket name
     * @param prefix Optional prefix to filter results
     * @return Array of object paths
     * @throws IOException If list fails
     */
    public native String[] listObjects(String bucket, String prefix) throws IOException;

    /**
     * Enable local caching for GCS reads
     * 
     * @param cacheDir Directory to store cached files
     * @return This reader for chaining
     */
    public native GcsReader withCache(String cacheDir);

    /**
     * Get metadata for a GCS object
     * 
     * @param bucket GCS bucket name
     * @param objectPath Object path
     * @return Metadata object containing size, updated time, generation, content type
     * @throws IOException If metadata fetch fails
     */
    public native GcsObjectMetadata getMetadata(String bucket, String objectPath) throws IOException;

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

    private native long newInstance(String projectId);
    private native void cleanup(long ptr);

    /**
     * Metadata for GCS objects
     */
    public static class GcsObjectMetadata {
        public long size;
        public String updated;
        public String generation;
        public String contentType;

        public GcsObjectMetadata(long size, String updated, String generation, String contentType) {
            this.size = size;
            this.updated = updated;
            this.generation = generation;
            this.contentType = contentType;
        }
    }
}
