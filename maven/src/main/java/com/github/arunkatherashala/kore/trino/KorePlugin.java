package com.github.arunkatherashala.kore.trino;

import io.trino.spi.Plugin;
import io.trino.spi.connector.ConnectorFactory;

import java.util.List;

/**
 * Trino plugin entry point — registers "kore" connector.
 * Usage: CREATE TABLE kore.default.sales (price DOUBLE, qty BIGINT)
 *        WITH (location = '/data/sales.kore');
 *        SELECT * FROM kore.default.sales;
 */
public class KorePlugin implements Plugin {
    @Override
    public Iterable<ConnectorFactory> getConnectorFactories() {
        return List.of(new KoreConnectorFactory());
    }
}
