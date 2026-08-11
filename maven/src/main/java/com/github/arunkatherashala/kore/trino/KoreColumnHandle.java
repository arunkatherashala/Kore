package com.github.arunkatherashala.kore.trino;

import io.trino.spi.connector.ColumnHandle;
import io.trino.spi.type.Type;

import java.io.Serializable;

public class KoreColumnHandle implements ColumnHandle, Serializable {
    private final String name;
    private final Type type;
    private final int ordinal;

    public KoreColumnHandle(String name, Type type, int ordinal) {
        this.name = name;
        this.type = type;
        this.ordinal = ordinal;
    }

    public String getName() { return name; }
    public Type getType() { return type; }
    public int getOrdinal() { return ordinal; }
}
