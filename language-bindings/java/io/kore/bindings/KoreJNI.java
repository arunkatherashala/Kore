package io.kore.bindings;

/**
 * Java Native Interface (JNI) bindings for Kore Rust core.
 * 
 * Provides zero-copy access to Kore format from Java/Scala applications.
 * Compiled to liboro_jni.so/.dll
 * 
 * Build:
 *   gcc -shared -fPIC -I/usr/lib/jvm/java-11-openjdk/include \
 *       -I/usr/lib/jvm/java-11-openjdk/include/linux \
 *       kore_jni.c -o liboro_jni.so
 */

public class KoreJNI {
    // Load native library
    static {
        try {
            System.loadLibrary("kore_jni");
        } catch (UnsatisfiedLinkError e) {
            System.err.println("Failed to load Kore JNI library: " + e.getMessage());
            throw new ExceptionInInitializerError(e);
        }
    }

    /**
     * Read entire Kore file into memory.
     * 
     * @param filePath path to .kore file
     * @return 2D String array [columns][rows]
     * @throws Exception on I/O error
     */
    public static native String[][] readFile(String filePath) throws Exception;

    /**
     * Read single column from Kore file (zero-copy).
     * 
     * @param filePath path to .kore file
     * @param columnIndex column index (0-based)
     * @return String array of column values
     * @throws Exception on I/O error
     */
    public static native String[] readColumn(String filePath, int columnIndex) throws Exception;

    /**
     * Get file statistics without reading data.
     * 
     * @param filePath path to .kore file
     * @return statistics map with keys: rows, columns, chunks, fileSize, compressionRatio
     * @throws Exception on I/O error
     */
    public static native java.util.Map<String, Object> getStats(String filePath) throws Exception;

    /**
     * Process multiple files in parallel using Rust Rayon.
     * 
     * @param filePaths array of .kore file paths
     * @param operation "row_count", "column_count", "compress", "decompress"
     * @return results per file
     * @throws Exception on error
     */
    public static native java.util.Map<String, Object>[] processBatch(
        String[] filePaths, 
        String operation
    ) throws Exception;

    /**
     * Write Kore file from 2D String array.
     * 
     * @param filePath output file path
     * @param columns 2D array [columns][rows]
     * @param columnNames column headers
     * @throws Exception on I/O error
     */
    public static native void writeFile(
        String filePath,
        String[][] columns,
        String[] columnNames
    ) throws Exception;

    /**
     * Stream-read Kore file in chunks (memory efficient).
     * 
     * @param filePath path to .kore file
     * @param chunkSize rows per chunk
     * @param callback invoked per chunk with String[][] data
     * @throws Exception on I/O error
     */
    public static native void readFileChunked(
        String filePath,
        int chunkSize,
        ChunkCallback callback
    ) throws Exception;

    /**
     * Get Kore file version and format info.
     * 
     * @param filePath path to .kore file
     * @return version string (e.g., "2.0.0")
     * @throws Exception on I/O error
     */
    public static native String getFileVersion(String filePath) throws Exception;


    /**
     * Callback interface for chunked reading.
     */
    public interface ChunkCallback {
        /**
         * Called for each chunk during streaming read.
         * 
         * @param data chunk data as 2D String array
         * @param chunkIndex 0-based chunk number
         * @param totalChunks total number of chunks in file
         * @return false to stop reading, true to continue
         */
        boolean onChunk(String[][] data, int chunkIndex, int totalChunks);
    }


    /**
     * Test JNI bindings
     */
    public static void main(String[] args) throws Exception {
        if (args.length < 1) {
            System.out.println("Usage: java KoreJNI <kore_file>");
            System.exit(1);
        }

        String filePath = args[0];

        try {
            // Get stats
            System.out.println("Reading: " + filePath);
            var stats = getStats(filePath);
            System.out.println("Stats: " + stats);

            // Read file
            String[][] data = readFile(filePath);
            System.out.println("Loaded: " + data.length + " columns × " 
                + (data.length > 0 ? data[0].length : 0) + " rows");

            // Show sample
            if (data.length > 0 && data[0].length > 0) {
                System.out.println("\nFirst row sample:");
                for (int i = 0; i < Math.min(5, data.length); i++) {
                    System.out.println("  Col " + i + ": " + data[i][0]);
                }
            }

            // Test chunked reading
            System.out.println("\nTesting chunked read:");
            readFileChunked(filePath, 1000, (chunk, idx, total) -> {
                System.out.println("  Chunk " + idx + "/" + total + ": " 
                    + chunk.length + " cols × " + chunk[0].length + " rows");
                return true;
            });

        } catch (Exception e) {
            System.err.println("Error: " + e.getMessage());
            e.printStackTrace();
            System.exit(1);
        }
    }
}

/**
 * High-level Kore API for Java applications
 */
class KoreReader {
    private String filePath;
    private String[][] data;
    private java.util.Map<String, Object> stats;

    public KoreReader(String filePath) throws Exception {
        this.filePath = filePath;
        this.stats = KoreJNI.getStats(filePath);
    }

    public String[][] read() throws Exception {
        if (data == null) {
            data = KoreJNI.readFile(filePath);
        }
        return data;
    }

    public String[] readColumn(int index) throws Exception {
        return KoreJNI.readColumn(filePath, index);
    }

    public long getRowCount() {
        return ((Number) stats.get("rows")).longValue();
    }

    public int getColumnCount() {
        return ((Number) stats.get("columns")).intValue();
    }

    public int getChunkCount() {
        return ((Number) stats.get("chunks")).intValue();
    }

    public double getCompressionRatio() {
        return ((Number) stats.get("compressionRatio")).doubleValue();
    }

    public void streamRead(int chunkSize, KoreJNI.ChunkCallback callback) throws Exception {
        KoreJNI.readFileChunked(filePath, chunkSize, callback);
    }
}

/**
 * Write Kore files from Java
 */
class KoreWriter {
    private String filePath;
    private String[] columns;
    private java.util.List<String[]> rows;

    public KoreWriter(String filePath, String[] columns) {
        this.filePath = filePath;
        this.columns = columns;
        this.rows = new java.util.ArrayList<>();
    }

    public void addRow(String... values) {
        if (values.length != columns.length) {
            throw new IllegalArgumentException(
                "Expected " + columns.length + " values, got " + values.length
            );
        }
        rows.add(values);
    }

    public void write() throws Exception {
        // Convert rows to 2D array by columns
        String[][] data = new String[columns.length][rows.size()];
        
        for (int colIdx = 0; colIdx < columns.length; colIdx++) {
            for (int rowIdx = 0; rowIdx < rows.size(); rowIdx++) {
                data[colIdx][rowIdx] = rows.get(rowIdx)[colIdx];
            }
        }

        KoreJNI.writeFile(filePath, data, columns);
    }
}
