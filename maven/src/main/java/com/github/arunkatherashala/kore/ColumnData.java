package com.github.arunkatherashala.kore;

import java.util.List;
import java.util.Optional;

/**
 * Column data container with optional statistics for predicate pushdown.
 * Supports all data types: i64, f64, bool, str, array, struct.
 */
public class ColumnData {
    private final String name;
    private final DataType type;
    private final Object data;
    private final ColumnStats stats;

    /**
     * Create a column with data and statistics.
     * @param name Column name
     * @param type Column data type
     * @param data Column values (List, long[], double[], boolean[], etc.)
     * @param stats Optional statistics for predicate pushdown
     */
    public ColumnData(String name, DataType type, Object data, ColumnStats stats) {
        this.name = name;
        this.type = type;
        this.data = data;
        this.stats = stats;
    }

    public String getName() { return name; }
    public DataType getType() { return type; }
    public Object getData() { return data; }
    public Optional<ColumnStats> getStats() { return Optional.ofNullable(stats); }

    /**
     * Statistics for predicate pushdown (Feature 2 + Feature 4).
     * Enables query optimization: skip blocks without matching values.
     */
    public static class ColumnStats {
        public final Long minValue;      // For i64
        public final Double minValueF;   // For f64
        public final Long maxValue;      // For i64
        public final Double maxValueF;   // For f64
        public final long nullCount;
        public final long cardinality;
        public final long crc32;         // Feature 3: checksums

        public ColumnStats(Long minValue, Long maxValue, long nullCount, long cardinality, long crc32) {
            this.minValue = minValue;
            this.maxValue = maxValue;
            this.minValueF = null;
            this.maxValueF = null;
            this.nullCount = nullCount;
            this.cardinality = cardinality;
            this.crc32 = crc32;
        }

        public ColumnStats(Double minValueF, Double maxValueF, long nullCount, long cardinality, long crc32) {
            this.minValue = null;
            this.maxValue = null;
            this.minValueF = minValueF;
            this.maxValueF = maxValueF;
            this.nullCount = nullCount;
            this.cardinality = cardinality;
            this.crc32 = crc32;
        }

        @Override
        public String toString() {
            if (minValue != null) {
                return String.format("ColumnStats[i64: %d..%d, nulls=%d, card=%d, crc32=%d]",
                    minValue, maxValue, nullCount, cardinality, crc32);
            } else {
                return String.format("ColumnStats[f64: %f..%f, nulls=%d, card=%d, crc32=%d]",
                    minValueF, maxValueF, nullCount, cardinality, crc32);
            }
        }
    }
}
