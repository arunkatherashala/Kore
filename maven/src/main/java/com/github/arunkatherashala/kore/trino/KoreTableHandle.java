package com.github.arunkatherashala.kore.trino;

import io.trino.spi.connector.*;

import java.io.Serializable;

public class KoreTableHandle implements ConnectorTableHandle, Serializable {
    private final String schemaName;
    private final String tableName;
    private final String filePath;

    public KoreTableHandle(String schemaName, String tableName, String filePath) {
        this.schemaName = schemaName;
        this.tableName = tableName;
        this.filePath = filePath;
    }

    public String getSchemaName() { return schemaName; }
    public String getTableName() { return tableName; }
    public String getFilePath() { return filePath; }
}
