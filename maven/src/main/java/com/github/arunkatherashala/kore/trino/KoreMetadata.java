package com.github.arunkatherashala.kore.trino;

import com.github.arunkatherashala.kore.DataBlock;
import com.github.arunkatherashala.kore.ColumnData;
import com.github.arunkatherashala.kore.KoreReader;
import io.trino.spi.connector.*;
import io.trino.spi.type.Type;
import io.trino.spi.type.BigintType;
import io.trino.spi.type.DoubleType;
import io.trino.spi.type.BooleanType;
import io.trino.spi.type.VarcharType;

import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.*;
import java.util.stream.Collectors;

public class KoreMetadata implements ConnectorMetadata {

    private final String basePath;

    public KoreMetadata(String basePath) {
        this.basePath = basePath;
    }

    @Override
    public List<String> listSchemaNames(ConnectorSession session) {
        return List.of("default");
    }

    @Override
    public ConnectorTableHandle getTableHandle(ConnectorSession session, SchemaTableName tableName, Optional<ConnectorTableVersion> startVersion, Optional<ConnectorTableVersion> endVersion) {
        String filePath = basePath + "/" + tableName.getTableName() + ".kore";
        if (!new File(filePath).exists()) return null;
        return new KoreTableHandle(tableName.getSchemaName(), tableName.getTableName(), filePath);
    }

    @Override
    public ConnectorTableMetadata getTableMetadata(ConnectorSession session, ConnectorTableHandle table) {
        KoreTableHandle koreTable = (KoreTableHandle) table;
        List<ColumnMetadata> columns = readColumns(koreTable.getFilePath());
        return new ConnectorTableMetadata(
            new SchemaTableName(koreTable.getSchemaName(), koreTable.getTableName()),
            columns
        );
    }

    @Override
    public Map<String, ColumnHandle> getColumnHandles(ConnectorSession session, ConnectorTableHandle tableHandle) {
        KoreTableHandle koreTable = (KoreTableHandle) tableHandle;
        List<ColumnMetadata> columns = readColumns(koreTable.getFilePath());
        Map<String, ColumnHandle> handles = new LinkedHashMap<>();
        for (int i = 0; i < columns.size(); i++) {
            ColumnMetadata col = columns.get(i);
            handles.put(col.getName(), new KoreColumnHandle(col.getName(), col.getType(), i));
        }
        return handles;
    }

    @Override
    public ColumnMetadata getColumnMetadata(ConnectorSession session, ConnectorTableHandle tableHandle, ColumnHandle columnHandle) {
        KoreColumnHandle koreCol = (KoreColumnHandle) columnHandle;
        return new ColumnMetadata(koreCol.getName(), koreCol.getType());
    }

    @Override
    public List<SchemaTableName> listTables(ConnectorSession session, Optional<String> schemaName) {
        File dir = new File(basePath);
        if (!dir.isDirectory()) return List.of();
        File[] files = dir.listFiles((d, name) -> name.endsWith(".kore"));
        if (files == null) return List.of();
        return Arrays.stream(files)
            .map(f -> new SchemaTableName("default", f.getName().replace(".kore", "")))
            .collect(Collectors.toList());
    }

    private List<ColumnMetadata> readColumns(String filePath) {
        try {
            byte[] data = Files.readAllBytes(Paths.get(filePath));
            DataBlock block = KoreReader.fromBytes(data);
            List<ColumnMetadata> cols = new ArrayList<>();
            for (ColumnData col : block.getColumns()) {
                cols.add(new ColumnMetadata(col.getName(), toTrinoType(col.getType())));
            }
            return cols;
        } catch (IOException e) {
            throw new RuntimeException("Failed to read .kore schema: " + filePath, e);
        }
    }

    static Type toTrinoType(com.github.arunkatherashala.kore.DataType koreType) {
        switch (koreType) {
            case I64:      return BigintType.BIGINT;
            case F64:      return DoubleType.DOUBLE;
            case BOOL:     return BooleanType.BOOLEAN;
            case STR:
            case STR_DICT: return VarcharType.VARCHAR;
            default:       return VarcharType.VARCHAR;
        }
    }
}
