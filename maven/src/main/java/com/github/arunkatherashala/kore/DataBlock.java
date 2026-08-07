package com.github.arunkatherashala.kore;

import java.util.ArrayList;
import java.util.List;

/**
 * Core KORE data structure: a collection of columns with row count.
 * Represents a batch of data that can be serialized to/from KORE format.
 * 
 * All columns must have the same number of rows for consistency.
 */
public class DataBlock {
    private final List<ColumnData> columns;
    private final long numRows;

    /**
     * Create a DataBlock with columns and row count.
     * @param columns List of column data
     * @param numRows Number of rows (must match all column lengths)
     */
    public DataBlock(List<ColumnData> columns, long numRows) {
        this.columns = new ArrayList<>(columns);
        this.numRows = numRows;
        validate();
    }

    /**
     * Create an empty DataBlock.
     */
    public DataBlock() {
        this.columns = new ArrayList<>();
        this.numRows = 0;
    }

    /**
     * Validate that all columns have consistent row counts.
     */
    private void validate() {
        for (ColumnData col : columns) {
            long colLength = getColumnLength(col);
            if (colLength != numRows) {
                throw new IllegalArgumentException(
                    String.format("Column '%s' has %d rows but DataBlock expects %d",
                        col.getName(), colLength, numRows)
                );
            }
        }
    }

    /**
     * Get the length of a column based on its data type.
     * @param col Column data
     * @return Number of rows in this column
     */
    private long getColumnLength(ColumnData col) {
        Object data = col.getData();
        if (data instanceof long[]) return ((long[]) data).length;
        if (data instanceof double[]) return ((double[]) data).length;
        if (data instanceof boolean[]) return ((boolean[]) data).length;
        if (data instanceof List) return ((List<?>) data).size();
        return 0;
    }

    public List<ColumnData> getColumns() { return new ArrayList<>(columns); }
    public long getNumRows() { return numRows; }
    public int getNumColumns() { return columns.size(); }

    /**
     * Get a column by name.
     * @param name Column name
     * @return ColumnData if found
     * @throws IllegalArgumentException if column not found
     */
    public ColumnData getColumn(String name) {
        return columns.stream()
            .filter(c -> c.getName().equals(name))
            .findFirst()
            .orElseThrow(() -> new IllegalArgumentException("Column not found: " + name));
    }

    /**
     * Add a column to this DataBlock.
     * @param column Column data to add
     */
    public void addColumn(ColumnData column) {
        if (!columns.isEmpty() && getColumnLength(column) != numRows) {
            throw new IllegalArgumentException(
                String.format("Column '%s' has %d rows but DataBlock has %d",
                    column.getName(), getColumnLength(column), numRows)
            );
        }
        columns.add(column);
    }

    @Override
    public String toString() {
        return String.format("DataBlock[rows=%d, cols=%d]", numRows, columns.size());
    }
}
