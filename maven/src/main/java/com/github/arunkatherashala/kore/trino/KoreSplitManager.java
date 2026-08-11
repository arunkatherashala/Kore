package com.github.arunkatherashala.kore.trino;

import io.trino.spi.connector.*;

import java.util.List;

public class KoreSplitManager implements ConnectorSplitManager {

    @Override
    public ConnectorSplitSource getSplits(
            ConnectorTransactionHandle transaction,
            ConnectorSession session,
            ConnectorTableHandle table,
            DynamicFilter dynamicFilter,
            Constraint constraint) {

        KoreTableHandle koreTable = (KoreTableHandle) table;
        ConnectorSplit split = new KoreSplit(koreTable.getFilePath());
        return new FixedSplitSource(List.of(split));
    }
}
