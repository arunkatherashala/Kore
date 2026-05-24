package com.kore.hadoop;

import java.io.Serializable;
import java.util.Map;

/**
 * Represents a single row in a Kore file as read by Hadoop.
 * Contains row number and column data.
 */
public class KoreRecord implements Serializable {
    
    private static final long serialVersionUID = 1L;

    private long rowNumber;
    private Map<String, Object> columnData;

    /**
     * Creates a new KoreRecord.
     *
     * @param rowNumber The row index
     * @param columnData Map of column name to data value
     */
    public KoreRecord(long rowNumber, Map<String, Object> columnData) {
        this.rowNumber = rowNumber;
        this.columnData = columnData;
    }

    /**
     * Gets the row number.
     *
     * @return Row index
     */
    public long getRowNumber() {
        return rowNumber;
    }

    /**
     * Gets the column data.
     *
     * @return Map of column values
     */
    public Map<String, Object> getColumnData() {
        return columnData;
    }

    /**
     * Gets value for a specific column.
     *
     * @param columnName The column name
     * @return The column value, or null if not found
     */
    public Object getColumnValue(String columnName) {
        return columnData.get(columnName);
    }

    /**
     * Returns string representation.
     *
     * @return String representation of record
     */
    @Override
    public String toString() {
        return String.format("KoreRecord(row=%d, columns=%d)", 
            rowNumber, columnData.size());
    }
}
