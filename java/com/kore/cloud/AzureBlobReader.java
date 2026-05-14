package com.kore.cloud;
import java.io.IOException;

/**
 * Java wrapper for Rust AzureBlobReader
 * 
 * Usage:
 * <pre>
 * AzureBlobReader reader = new AzureBlobReader("myaccount", "mykey");
 * byte[] data = reader.readFile("mycontainer", "path/to/blob");
 * reader.close();
 * </pre>
 */
public class AzureBlobReader {
    private long nativePtr;

    static {
        System.loadLibrary("kore_java");
    }

    /**
     * Create a new AzureBlobReader
     * 
     * @param storageAccount Azure storage account name
     * @param accountKey Azure storage account key
     */
    public AzureBlobReader(String storageAccount, String accountKey) {
        this.nativePtr = newInstance(storageAccount, accountKey);
        if (nativePtr == 0) {
            throw new RuntimeException("Failed to initialize AzureBlobReader");
        }
    }

    /**
     * Read blob from Azure Blob Storage
     * 
     * @param container Container name
     * @param blobPath Blob path/name
     * @return Blob contents as byte array
     * @throws IOException If read fails
     */
    public native byte[] readFile(String container, String blobPath) throws IOException;

    /**
     * Write blob to Azure Blob Storage
     * 
     * @param container Container name
     * @param blobPath Blob path/name
     * @param data Blob contents to write
     * @throws IOException If write fails
     */
    public native void writeFile(String container, String blobPath, byte[] data) throws IOException;

    /**
     * List blobs in Azure container
     * 
     * @param container Container name
     * @param prefix Optional prefix to filter results
     * @return Array of blob paths
     * @throws IOException If list fails
     */
    public native String[] listBlobs(String container, String prefix) throws IOException;

    /**
     * Enable local caching for Azure reads
     * 
     * @param cacheDir Directory to store cached files
     * @return This reader for chaining
     */
    public native AzureBlobReader withCache(String cacheDir);

    /**
     * Get metadata for an Azure blob
     * 
     * @param container Container name
     * @param blobPath Blob path
     * @return Metadata object containing size, modified time, etag, content type
     * @throws IOException If metadata fetch fails
     */
    public native AzureBlobMetadata getMetadata(String container, String blobPath) throws IOException;

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

    private native long newInstance(String storageAccount, String accountKey);
    private native void cleanup(long ptr);

    /**
     * Metadata for Azure blobs
     */
    public static class AzureBlobMetadata {
        public long size;
        public String lastModified;
        public String etag;
        public String contentType;

        public AzureBlobMetadata(long size, String lastModified, String etag, String contentType) {
            this.size = size;
            this.lastModified = lastModified;
            this.etag = etag;
            this.contentType = contentType;
        }
    }
}
