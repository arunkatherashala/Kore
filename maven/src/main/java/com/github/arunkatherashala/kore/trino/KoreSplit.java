package com.github.arunkatherashala.kore.trino;

import io.trino.spi.connector.ConnectorSplit;

import java.util.List;
import java.util.Map;

public class KoreSplit implements ConnectorSplit {
    private final String filePath;

    public KoreSplit(String filePath) {
        this.filePath = filePath;
    }

    public String getFilePath() { return filePath; }

    @Override
    public Map<String, String> getSplitInfo() {
        return Map.of("filePath", filePath);
    }

    @Override
    public long getRetainedSizeInBytes() { return 0; }
}
