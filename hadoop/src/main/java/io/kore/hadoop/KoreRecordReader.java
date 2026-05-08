package io.kore.hadoop;

import org.apache.hadoop.fs.FSDataInputStream;
import org.apache.hadoop.fs.FileSystem;
import org.apache.hadoop.fs.Path;
import org.apache.hadoop.mapreduce.InputSplit;
import org.apache.hadoop.mapreduce.RecordReader;
import org.apache.hadoop.mapreduce.TaskAttemptContext;
import org.apache.hadoop.mapreduce.lib.input.FileSplit;

import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.ArrayList;
import java.util.List;

/**
 * Reads Kore file chunks as (LongWritable, Text) records for Hadoop processing.
 * Each chunk is processed as separate splits, enabling parallelization.
 */
public class KoreRecordReader extends RecordReader<Long, String> {
    private FileSplit split;
    private FSDataInputStream inputStream;
    private long currentKey;
    private String currentValue;
    private long currentRow;
    private long rowsInChunk;
    private long rowsRead;
    private boolean hasMore;
    
    // Chunk metadata
    private long chunkStartOffset;
    private long chunkLength;
    private int numColumns;
    
    // Column data for current row
    private List<String> columnValues;

    @Override
    public void initialize(InputSplit split, TaskAttemptContext context) throws IOException {
        this.split = (FileSplit) split;
        
        // Open file
        Path path = this.split.getPath();
        FileSystem fs = path.getFileSystem(context.getConfiguration());
        inputStream = fs.open(path);
        
        // Seek to chunk start
        chunkStartOffset = this.split.getStart();
        chunkLength = this.split.getLength();
        
        inputStream.seek(chunkStartOffset);
        
        // Read chunk header (if at file start, read global header)
        if (chunkStartOffset == 0) {
            readGlobalHeader();
        }
        
        // Initialize row tracking
        currentRow = chunkStartOffset;
        rowsRead = 0;
        hasMore = true;
        columnValues = new ArrayList<>();
    }

    /**
     * Read global file header to get column count
     */
    private void readGlobalHeader() throws IOException {
        byte[] header = new byte[64];
        int bytesRead = inputStream.read(header);
        
        if (bytesRead < 16) {
            hasMore = false;
            return;
        }
        
        // Validate magic bytes
        String magic = new String(header, 0, 4);
        if (!magic.equals("KORE")) {
            throw new IOException("Invalid Kore file format: bad magic bytes");
        }
        
        // Extract column count from byte 6-8
        numColumns = ByteBuffer.wrap(header, 6, 2)
            .order(ByteOrder.LITTLE_ENDIAN)
            .getShort() & 0xFFFF;
        
        // Extract row count from byte 8-16
        long totalRows = ByteBuffer.wrap(header, 8, 8)
            .order(ByteOrder.LITTLE_ENDIAN)
            .getLong();
        
        // Calculate rows in this chunk (usually 65536, but last chunk may be smaller)
        long totalChunks = (totalRows + 65535) / 65536;  // CHUNK_ROWS = 65536
        long chunkIndex = chunkStartOffset / (totalRows / totalChunks);
        rowsInChunk = Math.min(65536, totalRows - (chunkIndex * 65536));
    }

    @Override
    public boolean nextKeyValue() throws IOException {
        if (!hasMore || rowsRead >= rowsInChunk) {
            return false;
        }
        
        // Set current key (row offset)
        currentKey = currentRow++;
        
        // Read row data (simplified - reads until newline or column separator)
        try {
            currentValue = readRowData();
            rowsRead++;
            return true;
        } catch (IOException e) {
            hasMore = false;
            return false;
        }
    }

    /**
     * Read and parse a single row from the binary chunk
     * Format: [col1_len][col1_data][col2_len][col2_data]...
     */
    private String readRowData() throws IOException {
        columnValues.clear();
        
        for (int col = 0; col < numColumns; col++) {
            // Read column value length (variable-length encoding)
            int valueLength = readVarInt();
            
            if (valueLength == 0xFFFFFFFF) {
                // NULL marker
                columnValues.add("NULL");
            } else if (valueLength > 0) {
                // Read column data
                byte[] data = new byte[valueLength];
                int bytesRead = inputStream.read(data);
                if (bytesRead != valueLength) {
                    throw new IOException("Unexpected EOF while reading column data");
                }
                columnValues.add(new String(data, "UTF-8"));
            } else {
                columnValues.add("");
            }
        }
        
        // Join columns with pipe separator
        return String.join("|", columnValues);
    }

    /**
     * Read variable-length integer encoding
     * Single byte: 0x00-0x7F
     * Multi-byte: 0x80 + continuation
     */
    private int readVarInt() throws IOException {
        int result = 0;
        int shift = 0;
        byte b;
        
        do {
            byte[] singleByte = new byte[1];
            int n = inputStream.read(singleByte);
            if (n != 1) {
                throw new IOException("Unexpected EOF");
            }
            b = singleByte[0];
            
            result |= (b & 0x7F) << shift;
            shift += 7;
        } while ((b & 0x80) != 0);
        
        return result;
    }

    @Override
    public Long getCurrentKey() {
        return currentKey;
    }

    @Override
    public String getCurrentValue() {
        return currentValue;
    }

    @Override
    public float getProgress() {
        if (rowsInChunk == 0) {
            return 0.0f;
        }
        return (float) rowsRead / rowsInChunk;
    }

    @Override
    public void close() throws IOException {
        if (inputStream != null) {
            inputStream.close();
        }
    }
}
