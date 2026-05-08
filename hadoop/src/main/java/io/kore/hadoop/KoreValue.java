package io.kore.hadoop;

import org.apache.hadoop.io.Writable;
import java.io.DataInput;
import java.io.DataOutput;
import java.io.IOException;
import java.util.*;

/**
 * Value class for Kore records
 * Contains row data as generic map of column values
 */
public class KoreValue implements Writable {
    private Map<String, Object> values = new HashMap<>();

    public KoreValue() {}

    public KoreValue(Map<String, Object> values) {
        this.values = new HashMap<>(values);
    }

    public Map<String, Object> getValues() {
        return values;
    }

    public Object get(String column) {
        return values.get(column);
    }

    public void put(String column, Object value) {
        values.put(column, value);
    }

    @Override
    public void write(DataOutput out) throws IOException {
        // TODO: Serialize map to Kore format
        throw new RuntimeException("Not yet implemented");
    }

    @Override
    public void readFields(DataInput in) throws IOException {
        // TODO: Deserialize map from Kore format
        throw new RuntimeException("Not yet implemented");
    }

    @Override
    public String toString() {
        return values.toString();
    }
}
