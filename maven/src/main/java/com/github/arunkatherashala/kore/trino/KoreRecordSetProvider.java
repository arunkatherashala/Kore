package com.github.arunkatherashala.kore.trino;

import com.github.arunkatherashala.kore.DataBlock;
import com.github.arunkatherashala.kore.ColumnData;
import com.github.arunkatherashala.kore.KoreReader;
import io.trino.spi.connector.*;
import io.trino.spi.type.Type;
import io.trino.spi.type.VarcharType;
import io.airlift.slice.Slices;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.List;

public class KoreRecordSetProvider implements ConnectorRecordSetProvider {

    @Override
    public RecordSet getRecordSet(
            ConnectorTransactionHandle transaction,
            ConnectorSession session,
            ConnectorSplit split,
            ConnectorTableHandle table,
            List<? extends ColumnHandle> columns) {

        KoreSplit koreSplit = (KoreSplit) split;
        List<KoreColumnHandle> koreColumns = columns.stream()
            .map(c -> (KoreColumnHandle) c)
            .toList();

        return new KoreRecordSet(koreSplit.getFilePath(), koreColumns);
    }
}

class KoreRecordSet implements RecordSet {
    private final String filePath;
    private final List<KoreColumnHandle> columns;

    KoreRecordSet(String filePath, List<KoreColumnHandle> columns) {
        this.filePath = filePath;
        this.columns = columns;
    }

    @Override
    public List<Type> getColumnTypes() {
        return columns.stream().map(KoreColumnHandle::getType).toList();
    }

    @Override
    public RecordCursor cursor() {
        try {
            byte[] data = Files.readAllBytes(Paths.get(filePath));
            DataBlock block = KoreReader.fromBytes(data);
            return new KoreRecordCursor(block, columns);
        } catch (IOException e) {
            throw new RuntimeException("Failed to read .kore: " + filePath, e);
        }
    }
}

class KoreRecordCursor implements RecordCursor {
    private final DataBlock block;
    private final List<KoreColumnHandle> columns;
    private int currentRow = -1;

    KoreRecordCursor(DataBlock block, List<KoreColumnHandle> columns) {
        this.block = block;
        this.columns = columns;
    }

    @Override
    public long getCompletedBytes() { return 0; }
    @Override
    public long getReadTimeNanos() { return 0; }
    @Override
    public Type getType(int field) { return columns.get(field).getType(); }
    @Override
    public boolean advanceNextPosition() { return ++currentRow < block.getNumRows(); }

    @Override
    public boolean getBoolean(int field) {
        ColumnData col = block.getColumn(columns.get(field).getName());
        return col.getBoolValues()[(int) currentRow];
    }

    @Override
    public long getLong(int field) {
        ColumnData col = block.getColumn(columns.get(field).getName());
        return col.getI64Values()[(int) currentRow];
    }

    @Override
    public double getDouble(int field) {
        ColumnData col = block.getColumn(columns.get(field).getName());
        return col.getF64Values()[(int) currentRow];
    }

    @Override
    public io.airlift.slice.Slice getSlice(int field) {
        ColumnData col = block.getColumn(columns.get(field).getName());
        String val = col.getStrValues()[(int) currentRow];
        return val != null ? Slices.utf8Slice(val) : Slices.EMPTY_SLICE;
    }

    @Override
    public Object getObject(int field) { return null; }
    @Override
    public boolean isNull(int field) { return false; }
    @Override
    public void close() {}
}
