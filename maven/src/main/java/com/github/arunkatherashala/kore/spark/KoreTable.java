package com.github.arunkatherashala.kore.spark;

import org.apache.spark.sql.connector.catalog.SupportsRead;
import org.apache.spark.sql.connector.catalog.SupportsWrite;
import org.apache.spark.sql.connector.catalog.Table;
import org.apache.spark.sql.connector.catalog.TableCapability;
import org.apache.spark.sql.connector.read.ScanBuilder;
import org.apache.spark.sql.connector.write.LogicalWriteInfo;
import org.apache.spark.sql.connector.write.WriteBuilder;
import org.apache.spark.sql.types.StructType;
import org.apache.spark.sql.util.CaseInsensitiveStringMap;

import java.util.HashSet;
import java.util.Set;

public class KoreTable implements Table, SupportsRead, SupportsWrite {

    private final String path;
    private final StructType schema;

    public KoreTable(String path, StructType schema) {
        this.path = path;
        this.schema = schema;
    }

    @Override
    public String name() { return "kore:" + path; }

    @Override
    public StructType schema() { return schema; }

    @Override
    public Set<TableCapability> capabilities() {
        Set<TableCapability> caps = new HashSet<>();
        caps.add(TableCapability.BATCH_READ);
        caps.add(TableCapability.BATCH_WRITE);
        caps.add(TableCapability.TRUNCATE);
        return caps;
    }

    @Override
    public ScanBuilder newScanBuilder(CaseInsensitiveStringMap options) {
        return new KoreScanBuilder(path, schema);
    }

    @Override
    public WriteBuilder newWriteBuilder(LogicalWriteInfo info) {
        return new KoreWriteBuilder(path, info.schema());
    }
}
