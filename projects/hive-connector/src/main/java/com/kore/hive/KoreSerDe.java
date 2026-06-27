package com.kore.hive;

import org.apache.hadoop.hive.serde2.AbstractSerDe;
import org.apache.hadoop.hive.serde2.SerDeException;
import org.apache.hadoop.hive.serde2.SerDeStats;
import org.apache.hadoop.hive.serde2.objectinspector.ObjectInspector;
import org.apache.hadoop.hive.serde2.objectinspector.ObjectInspectorFactory;
import org.apache.hadoop.hive.serde2.objectinspector.StructObjectInspector;
import org.apache.hadoop.hive.serde2.objectinspector.primitive.PrimitiveObjectInspectorFactory;
import org.apache.hadoop.io.Text;
import org.apache.hadoop.io.Writable;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Properties;

/**
 * SerDe (Serializer/Deserializer) for Apache Hive with Kore format support.
 * Enables Hive tables to read and write Kore compressed files.
 *
 * Usage in Hive:
 *   CREATE TABLE kore_table (
 *     id BIGINT,
 *     name STRING,
 *     value DOUBLE
 *   )
 *   ROW FORMAT SERDE 'com.kore.hive.KoreSerDe'
 *   STORED AS INPUTFORMAT 'com.kore.hadoop.KoreInputFormat'
 *              OUTPUTFORMAT 'com.kore.hadoop.KoreOutputFormat';
 */
public class KoreSerDe extends AbstractSerDe {

    private static final Logger LOG = LoggerFactory.getLogger(KoreSerDe.class);

    // SerDe properties
    private Properties tbl;
    private StructObjectInspector rowOI;
    private List<String> columnNames;
    private List<ObjectInspector> columnOIs;

    // Statistics
    private SerDeStats stats;
    private long deserializedRows = 0;
    private long serializedRows = 0;

    /**
     * Initializes the SerDe from table properties.
     *
     * @param conf Hive configuration
     * @param tbl Table properties
     * @param partitionProperties Partition properties (unused)
     * @throws SerDeException If initialization fails
     */
    @Override
    public void initialize(
            org.apache.hadoop.conf.Configuration conf,
            Properties tbl,
            Properties partitionProperties) throws SerDeException {

        this.tbl = tbl;
        
        try {
            // Parse column information from properties
            String columnNamesStr = tbl.getProperty("columns");
            String columnTypesStr = tbl.getProperty("columns.types");

            if (columnNamesStr == null || columnNamesStr.isEmpty()) {
                throw new SerDeException("Column names not specified");
            }

            // Split column names
            columnNames = new ArrayList<>();
            for (String name : columnNamesStr.split(",")) {
                columnNames.add(name.trim());
            }

            // Initialize column object inspectors
            columnOIs = new ArrayList<>();
            for (String name : columnNames) {
                // Simple mapping: treat all columns as strings by default
                // In production, use columnTypesStr to determine actual types
                columnOIs.add(PrimitiveObjectInspectorFactory.javaStringObjectInspector);
            }

            // Create struct object inspector
            rowOI = ObjectInspectorFactory.getStandardStructObjectInspector(
                columnNames, columnOIs
            );

            stats = new SerDeStats();
            
            LOG.info("KoreSerDe initialized: {} columns", columnNames.size());

        } catch (Exception e) {
            LOG.error("Failed to initialize KoreSerDe", e);
            throw new SerDeException("Initialization failed: " + e.getMessage(), e);
        }
    }

    /**
     * Deserializes a Writable object to a Java object.
     *
     * @param blob The Writable object from InputFormat
     * @return Deserialized object
     * @throws SerDeException If deserialization fails
     */
    @Override
    public Object deserialize(Writable blob) throws SerDeException {
        try {
            if (blob instanceof Text) {
                Text text = (Text) blob;
                deserializedRows++;

                // Parse Kore record format
                // Format: "col1_value|col2_value|col3_value"
                String[] values = text.toString().split("\\|");

                if (values.length != columnNames.size()) {
                    LOG.warn("Column count mismatch: expected {}, got {}", 
                        columnNames.size(), values.length);
                }

                List<Object> record = new ArrayList<>();
                for (int i = 0; i < columnNames.size(); i++) {
                    if (i < values.length) {
                        record.add(values[i]);
                    } else {
                        record.add(null);
                    }
                }

                return record;

            } else {
                throw new SerDeException("Expected Text, got " + blob.getClass().getName());
            }

        } catch (Exception e) {
            LOG.error("Deserialization failed", e);
            throw new SerDeException("Deserialization failed: " + e.getMessage(), e);
        }
    }

    /**
     * Serializes a Java object to a Writable object.
     *
     * @param obj The object to serialize (List of column values)
     * @return Serialized Writable
     * @throws SerDeException If serialization fails
     */
    @Override
    public Writable serialize(Object obj, ObjectInspector objInspector) throws SerDeException {
        try {
            serializedRows++;

            if (obj instanceof List) {
                List<?> list = (List<?>) obj;

                // Format: "col1_value|col2_value|col3_value"
                StringBuilder sb = new StringBuilder();
                for (int i = 0; i < columnNames.size(); i++) {
                    if (i > 0) {
                        sb.append("|");
                    }
                    if (i < list.size() && list.get(i) != null) {
                        sb.append(list.get(i).toString());
                    }
                }

                return new Text(sb.toString());

            } else {
                throw new SerDeException("Expected List, got " + obj.getClass().getName());
            }

        } catch (Exception e) {
            LOG.error("Serialization failed", e);
            throw new SerDeException("Serialization failed: " + e.getMessage(), e);
        }
    }

    /**
     * Gets the object inspector for the row type.
     *
     * @return StructObjectInspector for the row
     */
    @Override
    public ObjectInspector getObjectInspector() {
        return rowOI;
    }

    /**
     * Gets serialization statistics.
     *
     * @return SerDeStats with serialization metrics
     */
    @Override
    public SerDeStats getSerDeStats() {
        if (stats == null) {
            stats = new SerDeStats();
        }
        stats.setRawDataSize(deserializedRows * 100); // Rough estimate
        stats.setRowCount(deserializedRows);
        return stats;
    }

    /**
     * Gets the class of serialized objects.
     *
     * @return Text class for serialized form
     */
    @Override
    public Class<? extends Writable> getSerializedClass() {
        return Text.class;
    }

    /**
     * Returns string representation.
     *
     * @return String info
     */
    @Override
    public String toString() {
        return String.format(
            "KoreSerDe(columns=%d, deserialized=%d, serialized=%d)",
            columnNames.size(), deserializedRows, serializedRows
        );
    }
}
