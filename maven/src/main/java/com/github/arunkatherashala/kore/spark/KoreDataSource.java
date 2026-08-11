package com.github.arunkatherashala.kore.spark;

import org.apache.spark.sql.connector.catalog.Table;
import org.apache.spark.sql.connector.catalog.TableProvider;
import org.apache.spark.sql.connector.expressions.Transform;
import org.apache.spark.sql.types.StructType;
import org.apache.spark.sql.util.CaseInsensitiveStringMap;

import java.util.Map;

/**
 * Spark DataSourceV2 connector for .kore files.
 * Usage: spark.read.format("kore").load("path/to/data.kore")
 */
public class KoreDataSource implements TableProvider {

    @Override
    public StructType inferSchema(CaseInsensitiveStringMap options) {
        String path = options.get("path");
        if (path == null) throw new IllegalArgumentException("path is required");
        return KoreSparkUtils.inferSchema(path);
    }

    @Override
    public Table getTable(StructType schema, Transform[] partitioning, Map<String, String> properties) {
        String path = properties.get("path");
        return new KoreTable(path, schema);
    }

    @Override
    public boolean supportsExternalMetadata() {
        return true;
    }
}
