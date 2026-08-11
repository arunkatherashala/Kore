package com.github.arunkatherashala.kore.hive;

import org.apache.hadoop.fs.FileSystem;
import org.apache.hadoop.fs.Path;
import org.apache.hadoop.io.BytesWritable;
import org.apache.hadoop.io.LongWritable;
import org.apache.hadoop.mapred.*;

import com.github.arunkatherashala.kore.DataBlock;
import com.github.arunkatherashala.kore.ColumnData;
import com.github.arunkatherashala.kore.KoreReader;

import java.io.IOException;
import java.nio.file.Files;

/**
 * Hive InputFormat for reading .kore files.
 */
public class KoreInputFormat implements InputFormat<LongWritable, BytesWritable> {

    @Override
    public InputSplit[] getSplits(JobConf job, int numSplits) throws IOException {
        Path[] paths = FileInputFormat.getInputPaths(job);
        InputSplit[] splits = new InputSplit[paths.length];
        for (int i = 0; i < paths.length; i++) {
            FileSystem fs = paths[i].getFileSystem(job);
            long len = fs.getFileStatus(paths[i]).getLen();
            splits[i] = new FileSplit(paths[i], 0, len, (String[]) null);
        }
        return splits;
    }

    @Override
    public RecordReader<LongWritable, BytesWritable> getRecordReader(InputSplit split, JobConf job, Reporter reporter) throws IOException {
        return new KoreRecordReader((FileSplit) split, job);
    }
}

class KoreRecordReader implements RecordReader<LongWritable, BytesWritable> {
    private final DataBlock block;
    private final int totalRows;
    private int currentRow = -1;
    private final String[] colNames;

    KoreRecordReader(FileSplit split, JobConf conf) throws IOException {
        Path path = split.getPath();
        FileSystem fs = path.getFileSystem(conf);
        byte[] data;
        try (var in = fs.open(path)) {
            data = in.readAllBytes();
        }
        this.block = KoreReader.fromBytes(data);
        this.totalRows = (int) block.getNumRows();
        this.colNames = block.getColumns().stream().map(ColumnData::getName).toArray(String[]::new);
    }

    @Override
    public boolean next(LongWritable key, BytesWritable value) {
        currentRow++;
        if (currentRow >= totalRows) return false;
        key.set(currentRow);
        // Serialize row as simple tab-separated for SerDe
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < colNames.length; i++) {
            if (i > 0) sb.append('\t');
            ColumnData col = block.getColumn(colNames[i]);
            sb.append(getValueAsString(col, currentRow));
        }
        byte[] rowBytes = sb.toString().getBytes();
        value.set(rowBytes, 0, rowBytes.length);
        return true;
    }

    private String getValueAsString(ColumnData col, int row) {
        switch (col.getType()) {
            case I64: return String.valueOf(col.getI64Values()[row]);
            case F64: return String.valueOf(col.getF64Values()[row]);
            case BOOL: return String.valueOf(col.getBoolValues()[row]);
            case STR:
            case STR_DICT:
                String s = col.getStrValues()[row];
                return s != null ? s : "\\N";
            default: return "\\N";
        }
    }

    @Override
    public LongWritable createKey() { return new LongWritable(); }

    @Override
    public BytesWritable createValue() { return new BytesWritable(); }

    @Override
    public long getPos() { return currentRow; }

    @Override
    public float getProgress() { return totalRows > 0 ? (float) currentRow / totalRows : 0; }

    @Override
    public void close() {}
}
