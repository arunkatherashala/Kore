package com.github.arunkatherashala.kore;

/**
 * Bloom Filter implementation for cardinality checks.
 * Feature 7: Bloom filters - enables fast "does this value exist?" queries.
 * 
 * Uses k independent hash functions to minimize false positives.
 * Trade-off: small space overhead for fast lookups without full scan.
 */
public class BloomFilter {
    private final byte[] bitmap;
    private final int k; // number of hash functions
    private final int m; // bitmap size in bits

    /**
     * Create a Bloom filter with expected items and false positive probability.
     * @param expectedItems Expected number of items
     * @param fpp False positive probability (e.g., 0.01 for 1%)
     */
    public BloomFilter(int expectedItems, double fpp) {
        // m = -1 / ln(2)^2 * n * ln(p)
        this.m = (int) (-1 / (Math.log(2) * Math.log(2)) * expectedItems * Math.log(fpp));
        // k = ln(2) * m / n
        this.k = Math.max(1, (int) (Math.log(2) * m / expectedItems));
        this.bitmap = new byte[(m + 7) / 8]; // Round up to nearest byte
    }

    /**
     * Create Bloom filter from serialized bytes.
     * @param data Serialized bloom filter
     */
    public BloomFilter(byte[] data) {
        this.bitmap = data.clone();
        this.m = data.length * 8;
        this.k = 3; // Default hash functions
    }

    /**
     * Insert a value into the Bloom filter.
     * @param value String value to insert
     */
    public void insert(String value) {
        for (int i = 0; i < k; i++) {
            int bitIndex = Math.abs(hash(value, i)) % m;
            int byteIndex = bitIndex / 8;
            int bitOffset = bitIndex % 8;
            bitmap[byteIndex] |= (byte) (1 << bitOffset);
        }
    }

    /**
     * Check if a value might be in the filter (may have false positives).
     * @param value String value to check
     * @return true if value might be present, false if definitely absent
     */
    public boolean contains(String value) {
        for (int i = 0; i < k; i++) {
            int bitIndex = Math.abs(hash(value, i)) % m;
            int byteIndex = bitIndex / 8;
            int bitOffset = bitIndex % 8;
            if ((bitmap[byteIndex] & (1 << bitOffset)) == 0) {
                return false; // Definitely not present
            }
        }
        return true; // Probably present
    }

    /**
     * Serialize bloom filter to bytes.
     * @return Serialized bitmap
     */
    public byte[] toBytes() {
        return bitmap.clone();
    }

    /**
     * Hash function with seed for k independent hashes.
     * @param value String value to hash
     * @param seed Hash function index
     * @return Hash value
     */
    private int hash(String value, int seed) {
        int h = 5381;
        byte[] bytes = value.getBytes();
        for (byte b : bytes) {
            h = ((h << 5) + h) ^ (b + seed);
        }
        return h;
    }
}
