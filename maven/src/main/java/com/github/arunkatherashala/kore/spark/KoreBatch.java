package com.github.arunkatherashala.kore.spark;

import org.apache.spark.sql.catalyst.InternalRow;
import org.apache.spark.sql.catalyst.expressions.GenericInternalRow;
import org.apache.spark.sql.connector.read.Batch;
import org.apache.spark.sql.connector.read.InputPartition;
import org.apache.spark.sql.connector.read.PartitionReader;
import org.apache.spark.sql.connector.read.PartitionReaderFactory;
import org.apache.spark.unsafe.types.UTF8String;
import org.apache.spark.sql.types.StructType;
import org.apache.spark.sql.types.StructField;

import com.github.arunkatherashala.kore.DataBlock;
import com.github.arunkatherashala.kore.ColumnData;
import com.github.arunkatherashala.kore.KoreReader;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Paths;

public class KoreBatch implements Batch {

    private final String path;
    private final StructType schema;

    public KoreBatch(String path, StructType schema) {
        this.path = path;
        this.schema = schema;
    }

    @Override
    public InputPartition[] planInputPartitions() {
        return new InputPartition[]{ new KoreInputPartition(path) };
    }

    @Override
    public PartitionReaderFactory createReaderFactory() {
        return new KoreReaderFactory(schema);
    }
}

class KoreInputPartition implements InputPartition, java.io.Serializable {
    final String path;
    KoreInputPartition(String path) { this.path = path; }
}

class KoreReaderFactory implements PartitionReaderFactory, java.io.Serializable {
    private final StructType schema;
    KoreReaderFactory(StructType schema) { this.schema = schema; }

    @Override
    public PartitionReader<InternalRow> createReader(InputPartition partition) {
        return new KorePartitionReader(((KoreInputPartition) partition).path, schema);
    }
}

class KorePartitionReader implements PartitionReader<InternalRow> {

    private final DataBlock block;
    private final StructType schema;
    private int currentRow = -1;

    KorePartitionReader(String path, StructType schema) {
        this.schema = schema;
        try {
            byte[] data = Files.readAllBytes(Paths.get(path));
            this.block = KoreReader.fromBytes(data);
        } catch (IOException e) {
            throw new RuntimeException("Failed to read .kore file: " + path, e);
        }
    }

    @Override
    public boolean next() {
        currentRow++;
        return currentRow < block.getNumRows();
    }

    @Override
    public InternalRow get() {
        Object[] values = new Object[schema.fields().length];
        StructField[] fields = schema.fields();

        for (int i = 0; i < fields.length; i++) {
            ColumnData col = block.getColumn(fields[i].name());
            if (col == null) { values[i] = null; continue; }

            switch (col.getType()) {
                case I64:
                    values[i] = col.getI64Values()[(int) currentRow];
                    break;
                case F64:
                    values[i] = col.getF64Values()[(int) currentRow];
                    break;
                case BOOL:
                    values[i] = col.getBoolValues()[(int) currentRow];
                    break;
                case STR:
                case STR_DICT:
                    String s = col.getStrValues()[(int) currentRow];
                    values[i] = s != null ? UTF8String.fromString(s) : null;
                    break;
                default:
                    values[i] = null;
            }
        }
        return new GenericInternalRow(values);
    }

    @Override
    public void close() {}
}
