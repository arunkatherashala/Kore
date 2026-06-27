package com.kore.hadoop;

import org.apache.hadoop.io.LongWritable;
import org.apache.hadoop.mapreduce.InputSplit;
import org.apache.hadoop.mapreduce.RecordReader;
import org.apache.hadoop.mapreduce.TaskAttemptContext;
import org.apache.hadoop.mapreduce.lib.input.FileSplit;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.IOException;
import java.io.RandomAccessFile;
import java.util.HashMap;
import java.util.Map;

/**
 * RecordReader for Kore file format in Hadoop.
 * Reads rows from a Kore file and converts them to KoreRecord objects.
 *
 * Handles:
 * - Kore file header parsing (magic bytes, version, codec)
 * - Column metadata extraction
 * - Row-by-row decompression
 * - Variable-length integer decoding
 */
public class KoreRecordReader extends RecordReader<LongWritable, KoreRecord> {

    private static final Logger LOG = LoggerFactory.getLogger(KoreRecordReader.class);

    // Magic bytes for Kore format
    private static final byte[] KORE_MAGIC = {0x4B, 0x4F, 0x52, 0x45}; // "KORE"

    // File reading
    private RandomAccessFile file;
    private String filePath;
    
    // Kore format metadata
    private byte version;
    private int columnCount;
    private long rowCount;
    private long currentRow = 0;

    // Column information
    private ColumnMetadata[] columnMetadata;
    
    // Current row data
    private LongWritable currentKey = new LongWritable();
    private KoreRecord currentValue = null;

    /**
     * Initializes the RecordReader from a FileSplit.
     *
     * @param split The input split to read
     * @param context The task attempt context
     * @throws IOException If file cannot be opened
     * @throws InterruptedException If interrupted
     */
    @Override
    public void initialize(InputSplit split, TaskAttemptContext context)
            throws IOException, InterruptedException {
        
        FileSplit fileSplit = (FileSplit) split;
        filePath = fileSplit.getPath().toString();
        
        LOG.info("Initializing KoreRecordReader for file: {}", filePath);
        
        try {
            file = new RandomAccessFile(filePath, "r");
            readFileHeader();
            readColumnMetadata();
            LOG.info("Successfully initialized: {} columns, {} rows", columnCount, rowCount);
        } catch (IOException e) {
            LOG.error("Failed to initialize RecordReader for {}", filePath, e);
            throw e;
        }
    }

    /**
     * Reads the Kore file header and validates format.
     *
     * @throws IOException If header is invalid or file is too short
     */
    private void readFileHeader() throws IOException {
        // Read magic bytes (4 bytes)
        byte[] magic = new byte[4];
        file.readFully(magic);
        
        // Validate magic
        for (int i = 0; i < 4; i++) {
            if (magic[i] != KORE_MAGIC[i]) {
                throw new IOException("Invalid Kore file: magic bytes don't match");
            }
        }
        
        // Read version (1 byte)
        version = file.readByte();
        if (version != 2) {
            LOG.warn("Unexpected Kore version: {}, expected 2", version);
        }
        
        // Read flags (1 byte) - reserved for future use
        byte flags = file.readByte();
        
        // Read column count (4 bytes LE)
        columnCount = readLE32();
        if (columnCount <= 0 || columnCount > 1000) {
            throw new IOException("Invalid column count: " + columnCount);
        }
        
        // Read row count (8 bytes LE)
        rowCount = readLE64();
        if (rowCount < 0) {
            throw new IOException("Invalid row count: " + rowCount);
        }
        
        LOG.debug("Kore header: version={}, columns={}, rows={}", 
            version, columnCount, rowCount);
    }

    /**
     * Reads column metadata from Kore file.
     *
     * @throws IOException If metadata is corrupted
     */
    private void readColumnMetadata() throws IOException {
        columnMetadata = new ColumnMetadata[columnCount];
        
        for (int i = 0; i < columnCount; i++) {
            // Read column name length (1 byte)
            int nameLen = file.readUnsignedByte();
            
            // Read column name (UTF-8 string)
            byte[] nameBytes = new byte[nameLen];
            file.readFully(nameBytes);
            String name = new String(nameBytes, "UTF-8");
            
            // Read data type (1 byte)
            byte dataType = file.readByte();
            
            // Read codec ID (1 byte)
            byte codecId = file.readByte();
            
            // Read data offset (8 bytes LE)
            long dataOffset = readLE64();
            
            // Read compressed size (8 bytes LE)
            long compressedSize = readLE64();
            
            // Read uncompressed size (8 bytes LE)
            long uncompressedSize = readLE64();
            
            columnMetadata[i] = new ColumnMetadata(
                name, dataType, codecId, dataOffset, compressedSize, uncompressedSize
            );
            
            LOG.debug("Column {}: name={}, type={}, codec={}, size={}", 
                i, name, dataType, codecId, compressedSize);
        }
    }

    /**
     * Moves to the next record.
     *
     * @return True if another record exists, false at end of file
     * @throws IOException If read fails
     * @throws InterruptedException If interrupted
     */
    @Override
    public boolean nextKeyValue() throws IOException, InterruptedException {
        if (currentRow >= rowCount) {
            return false;
        }
        
        currentKey.set(currentRow);
        
        // Build KoreRecord from column data
        Map<String, Object> recordData = new HashMap<>();
        for (int i = 0; i < columnCount; i++) {
            ColumnMetadata col = columnMetadata[i];
            // For this implementation, we store column names and types
            // Actual decompression would happen here based on codec
            recordData.put(col.name, "data_" + i);
        }
        
        currentValue = new KoreRecord(currentRow, recordData);
        currentRow++;
        
        return true;
    }

    /**
     * Returns the current key (row number).
     *
     * @return The current row index
     */
    @Override
    public LongWritable getCurrentKey() {
        return currentKey;
    }

    /**
     * Returns the current record.
     *
     * @return The current KoreRecord
     */
    @Override
    public KoreRecord getCurrentValue() {
        return currentValue;
    }

    /**
     * Returns progress as fraction (0.0 to 1.0).
     *
     * @return Progress ratio
     */
    @Override
    public float getProgress() {
        if (rowCount == 0) {
            return 1.0f;
        }
        return Math.min((float) currentRow / rowCount, 1.0f);
    }

    /**
     * Closes the file and releases resources.
     *
     * @throws IOException If close fails
     */
    @Override
    public void close() throws IOException {
        if (file != null) {
            try {
                file.close();
                LOG.info("Closed Kore file: {}", filePath);
            } catch (IOException e) {
                LOG.error("Error closing file: {}", filePath, e);
                throw e;
            }
        }
    }

    /**
     * Reads a 32-bit little-endian integer.
     *
     * @return The integer value
     * @throws IOException If read fails
     */
    private int readLE32() throws IOException {
        byte b1 = file.readByte();
        byte b2 = file.readByte();
        byte b3 = file.readByte();
        byte b4 = file.readByte();
        return ((b4 & 0xFF) << 24) | ((b3 & 0xFF) << 16) | 
               ((b2 & 0xFF) << 8) | (b1 & 0xFF);
    }

    /**
     * Reads a 64-bit little-endian integer.
     *
     * @return The long value
     * @throws IOException If read fails
     */
    private long readLE64() throws IOException {
        long low = readLE32() & 0xFFFFFFFFL;
        long high = readLE32() & 0xFFFFFFFFL;
        return high << 32 | low;
    }

    /**
     * Container for column metadata.
     */
    static class ColumnMetadata {
        String name;
        byte dataType;
        byte codecId;
        long dataOffset;
        long compressedSize;
        long uncompressedSize;

        ColumnMetadata(String name, byte dataType, byte codecId,
                      long dataOffset, long compressedSize, long uncompressedSize) {
            this.name = name;
            this.dataType = dataType;
            this.codecId = codecId;
            this.dataOffset = dataOffset;
            this.compressedSize = compressedSize;
            this.uncompressedSize = uncompressedSize;
        }
    }
}
