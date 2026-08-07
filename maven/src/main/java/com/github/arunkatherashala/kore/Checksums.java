package com.github.arunkatherashala.kore;

import java.util.zip.CRC32;

/**
 * Checksums utility: CRC32 for data integrity.
 * Feature 3: Checksums (CRC32) - enabled in footer section.
 */
public class Checksums {
    /**
     * Compute CRC32 checksum for data integrity validation.
     * @param data The data to checksum
     * @return 32-bit CRC32 value
     */
    public static long crc32(byte[] data) {
        CRC32 crc = new CRC32();
        crc.update(data);
        return crc.getValue();
    }

    /**
     * Compute CRC32 checksum for data integrity validation.
     * @param data The data to checksum
     * @param offset Starting offset
     * @param length Number of bytes to checksum
     * @return 32-bit CRC32 value
     */
    public static long crc32(byte[] data, int offset, int length) {
        CRC32 crc = new CRC32();
        crc.update(data, offset, length);
        return crc.getValue();
    }

    /**
     * Verify checksum against expected value.
     * @param data The data to verify
     * @param expected Expected CRC32 value
     * @return true if checksum matches
     */
    public static boolean verify(byte[] data, long expected) {
        return crc32(data) == expected;
    }
}
