package com.kore.spark;

import org.apache.spark.sql.*;
import org.apache.spark.sql.types.*;
import org.apache.spark.sql.sources.v2.*;
import org.apache.spark.sql.sources.v2.reader.*;
import org.apache.spark.sql.vectorized.ColumnarBatch;

import com.github.arunkatherashala.kore.KoreFileFormat;
import com.github.arunkatherashala.kore.DataBlock;
import com.github.arunkatherashala.kore.Column;

import java.util.*;

/**
 * KORE Spark DataSource V2 connector.
 *
 * Usage from PySpark:
 * <pre>
 *   df = spark.read.format("kore").load("data.kore")
 *   df.createOrReplaceTempView("sales")
 *   spark.sql("SELECT region, SUM(amount) FROM sales GROUP BY region").show()
 * </pre>
 *
 * Usage from Scala/Java:
 * <pre>
 *   Dataset[Row] df = spark.read().format("kore").load("data.kore");
 * </pre>
 */
public class KoreDataSource implements DataSourceV2, ReadSupport {

    @Override
    public DataSourceReader createReader(DataSourceOptions options) {
        String path = options.get("path").orElseThrow(() ->
            new IllegalArgumentException("KORE DataSource requires 'path' option"));
        return new KoreDataSourceReader(path);
    }

    static class KoreDataSourceReader implements DataSourceReader {
        private final String path;
        private DataBlock block;

        KoreDataSourceReader(String path) {
            this.path = path;
            try {
                this.block = KoreFileFormat.readFile(path);
            } catch (Exception e) {
                throw new RuntimeException("Failed to read KORE file: " + path, e);
            }
        }

        @Override
        public StructType readSchema() {
            List<StructField> fields = new ArrayList<>();
            for (Column col : block.getColumns()) {
                DataType sparkType = switch (col.getDataType()) {
                    case F64 -> DataTypes.DoubleType;
                    case I64 -> DataTypes.LongType;
                    case BOOL -> DataTypes.BooleanType;
                    default -> DataTypes.StringType;
                };
                fields.add(DataTypes.createStructField(col.getName(), sparkType, true));
            }
            return DataTypes.createStructType(fields);
        }

        @Override
        public List<InputPartition<Row>> planInputPartitions() {
            return Collections.singletonList(new KorePartition(block));
        }
    }

    static class KorePartition implements InputPartition<Row> {
        private final DataBlock block;
        KorePartition(DataBlock block) { this.block = block; }

        @Override
        public InputPartitionReader<Row> createPartitionReader() {
            return new KorePartitionReader(block);
        }
    }

    static class KorePartitionReader implements InputPartitionReader<Row> {
        private final DataBlock block;
        private int rowIdx = -1;

        KorePartitionReader(DataBlock block) { this.block = block; }

        @Override
        public boolean next() { return ++rowIdx < block.getNumRows(); }

        @Override
        public Row get() {
            List<Object> values = new ArrayList<>();
            for (Column col : block.getColumns()) {
                switch (col.getDataType()) {
                    case F64  -> values.add(col.getDouble(rowIdx));
                    case I64  -> values.add(col.getLong(rowIdx));
                    case BOOL -> values.add(col.getLong(rowIdx) != 0);
                    default   -> values.add(col.getString(rowIdx));
                }
            }
            return RowFactory.create(values.toArray());
        }

        @Override
        public void close() {}
    }
}
