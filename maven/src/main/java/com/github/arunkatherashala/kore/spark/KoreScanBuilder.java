package com.github.arunkatherashala.kore.spark;

import org.apache.spark.sql.connector.read.Batch;
import org.apache.spark.sql.connector.read.Scan;
import org.apache.spark.sql.connector.read.ScanBuilder;
import org.apache.spark.sql.connector.read.SupportsPushDownRequiredColumns;
import org.apache.spark.sql.types.StructType;

public class KoreScanBuilder implements ScanBuilder, SupportsPushDownRequiredColumns {

    private final String path;
    private StructType schema;
    private StructType requiredSchema;

    public KoreScanBuilder(String path, StructType schema) {
        this.path = path;
        this.schema = schema;
        this.requiredSchema = schema;
    }

    @Override
    public void pruneColumns(StructType requiredSchema) {
        this.requiredSchema = requiredSchema;
    }

    @Override
    public Scan build() {
        return new KoreScan(path, requiredSchema);
    }
}

class KoreScan implements Scan {

    private final String path;
    private final StructType schema;

    KoreScan(String path, StructType schema) {
        this.path = path;
        this.schema = schema;
    }

    @Override
    public StructType readSchema() { return schema; }

    @Override
    public Batch toBatch() {
        return new KoreBatch(path, schema);
    }
}
