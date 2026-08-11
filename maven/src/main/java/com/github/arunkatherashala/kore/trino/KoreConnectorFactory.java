package com.github.arunkatherashala.kore.trino;

import io.trino.spi.connector.*;

import java.util.Map;

public class KoreConnectorFactory implements ConnectorFactory {

    @Override
    public String getName() { return "kore"; }

    @Override
    public Connector create(String catalogName, Map<String, String> config, ConnectorContext context) {
        String basePath = config.getOrDefault("kore.base-path", "/tmp/kore");
        return new KoreConnector(basePath);
    }
}
