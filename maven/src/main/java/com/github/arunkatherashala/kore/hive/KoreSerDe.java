package com.github.arunkatherashala.kore.hive;

import org.apache.hadoop.hive.serde2.AbstractSerDe;
import org.apache.hadoop.hive.serde2.SerDeException;
import org.apache.hadoop.hive.serde2.SerDeStats;
import org.apache.hadoop.hive.serde2.objectinspector.ObjectInspector;
import org.apache.hadoop.hive.serde2.objectinspector.ObjectInspectorFactory;
import org.apache.hadoop.hive.serde2.objectinspector.primitive.PrimitiveObjectInspectorFactory;
import org.apache.hadoop.io.BytesWritable;
import org.apache.hadoop.io.Writable;
import org.apache.hadoop.conf.Configuration;

import com.github.arunkatherashala.kore.DataBlock;
import com.github.arunkatherashala.kore.ColumnData;
import com.github.arunkatherashala.kore.KoreReader;

import java.util.*;

/**
 * Hive SerDe for KORE format files.
 * Usage:
 *   CREATE TABLE sales ROW FORMAT SERDE 'com.github.arunkatherashala.kore.hive.KoreSerDe'
 *   STORED AS INPUTFORMAT 'com.github.arunkatherashala.kore.hive.KoreInputFormat'
 *   OUTPUTFORMAT 'com.github.arunkatherashala.kore.hive.KoreOutputFormat'
 *   LOCATION '/data/kore/sales';
 */
public class KoreSerDe extends AbstractSerDe {

    private List<String> columnNames;
    private List<ObjectInspector> columnOIs;
    private ObjectInspector rowOI;

    @Override
    public void initialize(Configuration conf, Properties tbl) throws SerDeException {
        String colNamesStr = tbl.getProperty("columns", "");
        String colTypesStr = tbl.getProperty("columns.types", "");

        columnNames = Arrays.asList(colNamesStr.split(","));
        String[] colTypes = colTypesStr.split(":");

        columnOIs = new ArrayList<>();
        for (String type : colTypes) {
            switch (type.trim().toLowerCase()) {
                case "bigint":
                    columnOIs.add(PrimitiveObjectInspectorFactory.javaLongObjectInspector);
                    break;
                case "double":
                    columnOIs.add(PrimitiveObjectInspectorFactory.javaDoubleObjectInspector);
                    break;
                case "boolean":
                    columnOIs.add(PrimitiveObjectInspectorFactory.javaBooleanObjectInspector);
                    break;
                default:
                    columnOIs.add(PrimitiveObjectInspectorFactory.javaStringObjectInspector);
                    break;
            }
        }

        rowOI = ObjectInspectorFactory.getStandardStructObjectInspector(columnNames, columnOIs);
    }

    @Override
    public ObjectInspector getObjectInspector() { return rowOI; }

    @Override
    public Class<? extends Writable> getSerializedClass() { return BytesWritable.class; }

    @Override
    public Object deserialize(Writable blob) throws SerDeException {
        // Row-level deserialization delegated to InputFormat
        if (blob instanceof BytesWritable) {
            return ((BytesWritable) blob).getBytes();
        }
        throw new SerDeException("Expected BytesWritable");
    }

    @Override
    public Writable serialize(Object obj, ObjectInspector oi) throws SerDeException {
        throw new SerDeException("Write not supported yet");
    }

    @Override
    public SerDeStats getSerDeStats() { return new SerDeStats(); }
}
