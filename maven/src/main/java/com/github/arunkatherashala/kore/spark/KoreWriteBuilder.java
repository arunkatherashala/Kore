package com.github.arunkatherashala.kore.spark;

import org.apache.spark.sql.catalyst.InternalRow;
import org.apache.spark.sql.connector.write.*;
import org.apache.spark.sql.types.StructType;
import org.apache.spark.sql.types.StructField;

import com.github.arunkatherashala.kore.DataBlock;
import com.github.arunkatherashala.kore.ColumnData;
import com.github.arunkatherashala.kore.KoreWriter;
import com.github.arunkatherashala.kore.DataType;

import java.io.IOException;
import java.util.*;

public class KoreWriteBuilder implements WriteBuilder {
    private final String path;
    private final StructType schema;

    public KoreWriteBuilder(String path, StructType schema) {
        this.path = path;
        this.schema = schema;
    }

    @Override
    public Write build() {
        return new KoreWrite(path, schema);
    }
}

class KoreWrite implements Write {
    private final String path;
    private final StructType schema;

    KoreWrite(String path, StructType schema) {
        this.path = path;
        this.schema = schema;
    }

    @Override
    public BatchWrite toBatchWrite() {
        return new KoreBatchWrite(path, schema);
    }
}

class KoreBatchWrite implements BatchWrite {
    private final String path;
    private final StructType schema;

    KoreBatchWrite(String path, StructType schema) {
        this.path = path;
        this.schema = schema;
    }

    @Override
    public DataWriterFactory createBatchWriterFactory(PhysicalWriteInfo info) {
        return new KoreDataWriterFactory(path, schema);
    }

    @Override
    public void commit(WriterCommitMessage[] messages) {}

    @Override
    public void abort(WriterCommitMessage[] messages) {}
}

class KoreDataWriterFactory implements DataWriterFactory, java.io.Serializable {
    private final String path;
    private final StructType schema;

    KoreDataWriterFactory(String path, StructType schema) {
        this.path = path;
        this.schema = schema;
    }

    @Override
    public DataWriter<InternalRow> createWriter(int partitionId, long taskId) {
        return new KoreDataWriter(path, schema, partitionId);
    }
}

class KoreDataWriter implements DataWriter<InternalRow> {
    private final String path;
    private final StructType schema;
    private final int partitionId;
    private final List<InternalRow> buffer = new ArrayList<>();

    KoreDataWriter(String path, StructType schema, int partitionId) {
        this.path = path;
        this.schema = schema;
        this.partitionId = partitionId;
    }

    @Override
    public void write(InternalRow row) {
        buffer.add(row.copy());
    }

    @Override
    public WriterCommitMessage commit() throws IOException {
        if (buffer.isEmpty()) return new KoreWriterCommitMessage();

        StructField[] fields = schema.fields();
        List<ColumnData> columns = new ArrayList<>();

        for (int c = 0; c < fields.length; c++) {
            String name = fields[c].name();
            String sparkType = fields[c].dataType().typeName();

            if ("long".equals(sparkType)) {
                long[] vals = new long[buffer.size()];
                for (int r = 0; r < buffer.size(); r++) vals[r] = buffer.get(r).getLong(c);
                columns.add(ColumnData.fromI64(name, vals));
            } else if ("double".equals(sparkType)) {
                double[] vals = new double[buffer.size()];
                for (int r = 0; r < buffer.size(); r++) vals[r] = buffer.get(r).getDouble(c);
                columns.add(ColumnData.fromF64(name, vals));
            } else if ("boolean".equals(sparkType)) {
                boolean[] vals = new boolean[buffer.size()];
                for (int r = 0; r < buffer.size(); r++) vals[r] = buffer.get(r).getBoolean(c);
                columns.add(ColumnData.fromBool(name, vals));
            } else {
                String[] vals = new String[buffer.size()];
                for (int r = 0; r < buffer.size(); r++) {
                    var utf8 = buffer.get(r).getUTF8String(c);
                    vals[r] = utf8 != null ? utf8.toString() : null;
                }
                columns.add(ColumnData.fromStr(name, vals));
            }
        }

        DataBlock block = new DataBlock(columns, buffer.size());
        String outPath = partitionId == 0 ? path : path.replace(".kore", "_part" + partitionId + ".kore");
        KoreWriter.toFile(block, outPath);
        return new KoreWriterCommitMessage();
    }

    @Override
    public void abort() {}

    @Override
    public void close() {}
}

class KoreWriterCommitMessage implements WriterCommitMessage, java.io.Serializable {}
