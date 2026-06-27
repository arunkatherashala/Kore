# Phase 6: Additional Platform Connectors

## Overview
Expanding Kore format support to 4 additional major platforms:
1. **Presto** - Distributed SQL query engine
2. **Trino** - Presto fork (SQL engine)
3. **Elasticsearch** - Search and analytics engine
4. **Cassandra** - Distributed NoSQL database

---

## 6.1 Presto Connector

### Project Structure
```
projects/presto-connector/
├── pom.xml
├── README.md
├── src/main/java/com/kore/presto/
│   ├── KorePlugin.java
│   ├── KoreConnectorFactory.java
│   ├── KoreConnector.java
│   ├── KoreMetadata.java
│   ├── KoreSplit.java
│   ├── KoreRecordSet.java
│   └── KoreRecordCursor.java
└── src/main/resources/
    └── META-INF/services/
        └── com.facebook.presto.spi.Plugin
```

### Key Implementation (KorePlugin.java - ~80 lines)
```java
package com.kore.presto;

import com.facebook.presto.spi.Plugin;
import com.facebook.presto.spi.connector.ConnectorFactory;
import java.util.Set;
import com.google.common.collect.ImmutableSet;

public class KorePlugin implements Plugin {
    
    @Override
    public Set<Class<?>> getServices() {
        return ImmutableSet.<Class<?>>builder()
            .add(KoreConnectorFactory.class)
            .build();
    }
    
    @Override
    public Iterable<ConnectorFactory> getConnectorFactories() {
        return ImmutableSet.of(new KoreConnectorFactory());
    }
}
```

### Connection URL
```
CREATE CATALOG kore USING kore WITH (path='/data/kore');
SELECT * FROM kore.default.my_table;
```

---

## 6.2 Trino Connector

**Status**: Compatible with Presto connector with minimal changes
- Update service provider: `io.trino.spi.Plugin`
- API compatibility: Trino 350+ (same as Presto)

**Key File**: `src/main/resources/META-INF/services/io.trino.spi.Plugin`

---

## 6.3 Elasticsearch Connector

### Project Structure
```
projects/elasticsearch-connector/
├── pom.xml
├── README.md
└── src/main/java/com/kore/elasticsearch/
    ├── KoreRepository.java          (~100 lines)
    ├── KoreIndexer.java             (~80 lines)
    ├── KoreTransformer.java         (~60 lines)
    └── KoreIngestProcessor.java     (~100 lines)
```

### Key Implementation (KoreIngestProcessor.java - ~100 lines)
```java
package com.kore.elasticsearch;

import org.elasticsearch.ingest.AbstractProcessor;
import org.elasticsearch.ingest.IngestDocument;
import java.io.IOException;

public class KoreIngestProcessor extends AbstractProcessor {
    
    public KoreIngestProcessor(String tag, String description, Map<String, Object> config) {
        super(tag, description);
        this.config = config;
    }
    
    @Override
    public IngestDocument execute(IngestDocument ingestDocument) throws Exception {
        // Read Kore file
        String korePath = (String) config.get("kore_path");
        KoreReader reader = new KoreReader(korePath);
        
        // Transform Kore data to ES document
        Map<String, Object> doc = KoreTransformer.toElasticsearchDoc(
            reader.readNext()
        );
        
        // Enrich ingest document
        ingestDocument.getSourceAndMetadata().putAll(doc);
        
        return ingestDocument;
    }
    
    @Override
    public String getType() {
        return "kore";
    }
}
```

### Elasticsearch Pipeline Usage
```json
PUT _ingest/pipeline/kore-pipeline
{
  "processors": [
    {
      "kore": {
        "kore_path": "/data/kore/input.kore"
      }
    },
    {
      "set": {
        "field": "indexed_at",
        "value": "{{_ingest.timestamp}}"
      }
    }
  ]
}

POST /kore-index/_bulk?pipeline=kore-pipeline
{"index": {}}
{"data": "from kore"}
```

---

## 6.4 Cassandra Connector

### Project Structure
```
projects/cassandra-connector/
├── pom.xml
├── README.md
└── src/main/java/com/kore/cassandra/
    ├── KoreLoadStrategy.java       (~90 lines)
    ├── KoreMappingStrategy.java    (~70 lines)
    ├── KoreTypeMapper.java         (~60 lines)
    └── KoreMigration.java          (~100 lines)
```

### Key Implementation (KoreMigration.java - ~100 lines)
```java
package com.kore.cassandra;

import com.datastax.driver.core.Session;
import com.datastax.driver.core.Cluster;
import java.io.File;

public class KoreMigration {
    
    private Session session;
    private KoreReader reader;
    
    public void migrate(String korePath, String cassandraCluster, String keyspace, String table) 
            throws Exception {
        
        // Connect to Cassandra
        Cluster cluster = Cluster.builder()
            .addContactPoint(cassandraCluster)
            .build();
        session = cluster.connect(keyspace);
        
        // Read Kore file
        reader = new KoreReader(new File(korePath));
        
        // Batch insert into Cassandra
        while (reader.hasNext()) {
            Map<String, Object> row = reader.readNext();
            insertRow(table, row);
        }
        
        session.close();
    }
    
    private void insertRow(String table, Map<String, Object> row) {
        StringBuilder query = new StringBuilder("INSERT INTO ").append(table).append(" (");
        StringBuilder values = new StringBuilder("VALUES (");
        
        List<Object> bindValues = new ArrayList<>();
        for (Map.Entry<String, Object> entry : row.entrySet()) {
            query.append(entry.getKey()).append(",");
            values.append("?,");
            bindValues.add(entry.getValue());
        }
        
        query.setLength(query.length() - 1);
        values.setLength(values.length() - 1);
        query.append(") ").append(values).append(")");
        
        session.execute(query.toString(), bindValues.toArray());
    }
}
```

### Usage
```bash
# Migrate Kore file to Cassandra table
java -cp kore-cassandra-connector.jar \
  com.kore.cassandra.KoreMigration \
  --kore /data/input.kore \
  --cassandra localhost:9042 \
  --keyspace mydb \
  --table mytable
```

---

## Build & Deploy

### Maven Configuration (pom.xml template for Presto)
```xml
<?xml version="1.0" encoding="UTF-8"?>
<project>
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.kore</groupId>
    <artifactId>kore-presto-connector</artifactId>
    <version>1.0.0</version>
    
    <dependencies>
        <dependency>
            <groupId>com.facebook.presto</groupId>
            <artifactId>presto-spi</artifactId>
            <version>0.250</version>
            <scope>provided</scope>
        </dependency>
        <dependency>
            <groupId>io.github.arunkatherashala</groupId>
            <artifactId>kore-fileformat</artifactId>
            <version>1.2.2</version>
        </dependency>
    </dependencies>
    
    <build>
        <plugins>
            <plugin>
                <groupId>org.apache.maven.plugins</groupId>
                <artifactId>maven-assembly-plugin</artifactId>
                <configuration>
                    <descriptorRefs>
                        <descriptorRef>jar-with-dependencies</descriptorRef>
                    </descriptorRefs>
                </configuration>
                <executions>
                    <execution>
                        <phase>package</phase>
                        <goals>
                            <goal>single</goal>
                        </goals>
                    </execution>
                </executions>
            </plugin>
        </plugins>
    </build>
</project>
```

### Build All Connectors
```bash
for connector in presto trino elasticsearch cassandra; do
  cd projects/${connector}-connector
  mvn clean package -DskipTests
  echo "✓ $connector connector built"
done
```

---

## Testing Connectors

### Presto Test
```sql
-- Connect to Presto
presto> CREATE CATALOG kore USING kore 
  WITH (path='/data/kore');

presto> SELECT * FROM kore.default.my_table LIMIT 10;
```

### Elasticsearch Test
```bash
# Ingest Kore data to ES
curl -X POST "localhost:9200/kore-index/_bulk?pipeline=kore-pipeline" \
  -H 'Content-Type: application/json' \
  -d '{"index": {"_index": "kore"}}
{"data": "from kore"}'
```

### Cassandra Test
```bash
# Verify data migrated
cqlsh> SELECT COUNT(*) FROM mydb.mytable;
```

---

## Expected Deliverables

| Connector | JAR Size | Build Time | Lines of Code |
|-----------|----------|-----------|---------------|
| Presto | 15 MB | 20s | 350 |
| Trino | 15 MB | 20s | 350 |
| Elasticsearch | 8 MB | 15s | 240 |
| Cassandra | 12 MB | 18s | 320 |
| **Total** | **50 MB** | **73s** | **1,260** |

---

## Additional Platform Options

### 7. Flink (Stream Processing)
- Connector type: SourceFunction, SinkFunction
- Use case: Real-time Kore data ingestion
- Estimated size: 10 MB, 250 LOC

### 8. Kafka Connect (Message Queue)
- Connector type: SourceConnector, TransformSMT
- Use case: Stream Kore data to Kafka topics
- Estimated size: 12 MB, 280 LOC

### 9. MongoDB Connector
- Connector type: InsertOneModel wrapper
- Use case: Load Kore data into MongoDB
- Estimated size: 8 MB, 200 LOC

### 10. PostgreSQL JDBC Extension
- Connector type: FDW (Foreign Data Wrapper)
- Use case: Query Kore files from PostgreSQL
- Estimated size: 6 MB, 180 LOC

---

## Summary

**Phase 6 Deliverables**:
✅ Presto connector (SQL query engine)
✅ Trino connector (SQL fork compatibility)
✅ Elasticsearch connector (search & analytics)
✅ Cassandra connector (NoSQL migration)

**Total New Code**: 1,260 lines
**Platform Support**: 8 total (Hadoop, Spark, Hive, DuckDB, Presto, Trino, ES, Cassandra)
**Status**: Ready for implementation

---

**Next**: Phase 7 - Advanced Algorithm Optimization
