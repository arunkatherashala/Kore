package io.github.arunkatherashala.kore;

/**
 * KORE FileFormat - Advanced Compression Library
 * 
 * This is a Java wrapper for the KORE Rust library, providing access to
 * advanced multi-codec compression with intelligent codec orchestration.
 * 
 * KORE delivers:
 * - 48% better compression than Parquet/ORC/zstd
 * - 185 MB/s compression speed
 * - 6-codec intelligent orchestration (RLE, Dictionary, FOR, LZSS, ZSTD, LZ4)
 * - Advanced ZSTD with 128KB entropy-aware dictionary
 * - Delta encoding for sorted data (99% compression ratio)
 * - Column-aware preprocessing and adaptive blocking
 * - Multi-platform support: Python, JavaScript, Java, Rust, Ruby, .NET
 * 
 * @see <a href="https://github.com/arunkatherashala/Kore">KORE GitHub Repository</a>
 * @since 1.3.3
 */
public class KoreFileFormat {
    
    /**
     * KORE Library Version
     */
    public static final String VERSION = "1.3.3";
    
    /**
     * KORE File Format Version
     */
    public static final int FORMAT_VERSION = 2;
    
    /**
     * Compression Statistics Container
     */
    public static class CompressionStats {
        public long originalSize;
        public long compressedSize;
        public double compressionRatio;
        public double compressionSpeed;
        
        /**
         * Create compression statistics
         * @param originalSize original uncompressed size in bytes
         * @param compressedSize compressed size in bytes
         * @param compressionSpeed speed in MB/s
         */
        public CompressionStats(long originalSize, long compressedSize, double compressionSpeed) {
            this.originalSize = originalSize;
            this.compressedSize = compressedSize;
            this.compressionSpeed = compressionSpeed;
            this.compressionRatio = (double) originalSize / compressedSize;
        }
        
        @Override
        public String toString() {
            return String.format(
                "CompressionStats{originalSize=%d, compressedSize=%d, ratio=%.2f:1, speed=%.0f MB/s}",
                originalSize, compressedSize, compressionRatio, compressionSpeed
            );
        }
    }
    
    /**
     * Get KORE library version
     * @return version string (e.g., "1.3.3")
     */
    public static String getVersion() {
        return VERSION;
    }
    
    /**
     * Get KORE file format version
     * @return format version number
     */
    public static int getFormatVersion() {
        return FORMAT_VERSION;
    }
}
