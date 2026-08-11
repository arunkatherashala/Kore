package com.github.arunkatherashala.kore.trino;

import io.trino.spi.connector.*;
import io.trino.spi.transaction.IsolationLevel;

public class KoreConnector implements Connector {

    private final String basePath;

    public KoreConnector(String basePath) {
        this.basePath = basePath;
    }

    @Override
    public ConnectorTransactionHandle beginTransaction(IsolationLevel isolationLevel, boolean readOnly, boolean autoCommit) {
        return KoreTransactionHandle.INSTANCE;
    }

    @Override
    public ConnectorMetadata getMetadata(ConnectorSession session, ConnectorTransactionHandle transaction) {
        return new KoreMetadata(basePath);
    }

    @Override
    public ConnectorSplitManager getSplitManager() {
        return new KoreSplitManager();
    }

    @Override
    public ConnectorRecordSetProvider getRecordSetProvider() {
        return new KoreRecordSetProvider();
    }
}

enum KoreTransactionHandle implements ConnectorTransactionHandle {
    INSTANCE
}
