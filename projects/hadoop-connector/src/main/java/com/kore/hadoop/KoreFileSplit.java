package com.kore.hadoop;

import org.apache.hadoop.fs.Path;
import org.apache.hadoop.mapreduce.lib.input.FileSplit;

import java.io.IOException;

/**
 * InputSplit for a Kore file or file segment.
 * Extends FileSplit to add Kore-specific metadata.
 */
public class KoreFileSplit extends FileSplit {
    
    private int columnCount;
    private long rowCount;
    private int codecFlags;

    /**
     * Creates a new KoreFileSplit.
     *
     * @param path Path to the Kore file
     * @param start Starting byte offset
     * @param length Number of bytes in split
     * @param hosts Host names where split data resides
     */
    public KoreFileSplit(Path path, long start, long length, String[] hosts) {
        super(path, start, length, hosts);
        this.columnCount = 0;
        this.rowCount = 0;
        this.codecFlags = 0;
    }

    /**
     * Sets Kore-specific metadata.
     *
     * @param columnCount Number of columns
     * @param rowCount Number of rows in split
     * @param codecFlags Codec flags (bit field)
     */
    public void setKoreMetadata(int columnCount, long rowCount, int codecFlags) {
        this.columnCount = columnCount;
        this.rowCount = rowCount;
        this.codecFlags = codecFlags;
    }

    /**
     * Gets the number of columns.
     *
     * @return Column count
     */
    public int getColumnCount() {
        return columnCount;
    }

    /**
     * Gets the number of rows in this split.
     *
     * @return Row count
     */
    public long getRowCount() {
        return rowCount;
    }

    /**
     * Gets codec flags.
     *
     * @return Codec flags (bit field)
     */
    public int getCodecFlags() {
        return codecFlags;
    }

    /**
     * Returns string representation.
     *
     * @return String representation of split
     */
    @Override
    public String toString() {
        return String.format(
            "KoreFileSplit(path=%s, offset=%d, length=%d, columns=%d, rows=%d)",
            getPath(), getStart(), getLength(), columnCount, rowCount
        );
    }
}
