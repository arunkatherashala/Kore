package io.kore.hadoop;

import org.apache.hadoop.fs.*;
import org.apache.hadoop.mapred.*;
import org.apache.hadoop.conf.Configuration;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.util.*;

public class KoreInputFormat implements InputFormat<KoreKey, KoreValue> {

    private static final int CHUNK_ROWS = 65536;
    private static final byte[] KORE_MAGIC = {'K', 'O', 'R', 'E'};

    @Override
    public InputSplit[] getSplits(JobConf job, int numSplits) throws IOException {
        FileSystem fs = FileSystem.get(job);
        Path[] inputPaths = FileInputFormat.getInputPaths(job);
        List<InputSplit> splits = new ArrayList<>();

        for (Path path : inputPaths) {
            if (fs.isFile(path)) {
                // Read Kore file header to get metadata
                FSDataInputStream in = fs.open(path);
                byte[] header = new byte[16];
                in.readFully(0, header);
                
                // Parse header: magic(4) + version(1) + reserved(1) + ncols(2) + nrows(8)
                int numCols = (header[6] & 0xFF) | ((header[7] & 0xFF) << 8);
                long numRows = ByteBuffer.wrap(header, 8, 8).getLong();
                
                // Calculate number of chunks (65536 rows per chunk)
                int numChunks = (int)((numRows + CHUNK_ROWS - 1) / CHUNK_ROWS);
                long fileSize = fs.getFileStatus(path).getLen();
                long bytesPerChunk = fileSize / Math.max(numChunks, 1);
                
                // Create one split per chunk
                for (int i = 0; i < numChunks; i++) {
                    long offset = i * bytesPerChunk;
                    long length = Math.min(bytesPerChunk, fileSize - offset);
                    String[] hosts = {"localhost"};  // TODO: Get actual HDFS hosts
                    splits.add(new FileSplit(path, offset, length, hosts));
                }
                in.close();\n            }
        }
        
        return splits.toArray(new InputSplit[0]);
    }

    @Override
    public RecordReader<KoreKey, KoreValue> getRecordReader(
            InputSplit split,
            JobConf job,
            Reporter reporter) throws IOException {
        return new KoreRecordReader((FileSplit) split, job);
    }
}
