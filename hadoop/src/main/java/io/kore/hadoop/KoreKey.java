package io.kore.hadoop;

import org.apache.hadoop.io.WritableComparable;
import java.io.DataInput;
import java.io.DataOutput;
import java.io.IOException;

/**
 * Key class for Kore records
 * Contains row ID and chunk metadata
 */
public class KoreKey implements WritableComparable<KoreKey> {
    private long chunkId;
    private long rowId;

    public KoreKey() {}

    public KoreKey(long chunkId, long rowId) {
        this.chunkId = chunkId;
        this.rowId = rowId;
    }

    @Override
    public void write(DataOutput out) throws IOException {
        out.writeLong(chunkId);
        out.writeLong(rowId);
    }

    @Override
    public void readFields(DataInput in) throws IOException {
        chunkId = in.readLong();
        rowId = in.readLong();
    }

    @Override
    public int compareTo(KoreKey other) {
        if (this.chunkId != other.chunkId) {
            return Long.compare(this.chunkId, other.chunkId);
        }
        return Long.compare(this.rowId, other.rowId);
    }

    @Override
    public String toString() {
        return String.format("KoreKey(chunk=%d, row=%d)", chunkId, rowId);
    }
}
