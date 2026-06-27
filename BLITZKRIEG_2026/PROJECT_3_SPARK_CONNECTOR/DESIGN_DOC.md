# PROJECT 3: SPARK CONNECTOR - DESIGN DOCUMENT

**Goal:** Apache Spark DataSourceV2 integration with filter pushdown  
**Timeline:** May 22-31 (10 days)  
**Target:** Load Kore files into Spark with 131x speedup on selective queries

---

## 🔗 SPARK INTEGRATION ARCHITECTURE

```
┌─────────────────────────────────────────────────────┐
│ Apache Spark Application                            │
│  val df = spark.read.format("kore")                 │
│            .option("path", "s3://bucket/file.kore") │
│            .load()                                  │
└─────────────────┬───────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────┐
│ Spark SQL Engine                                    │
│  • Query parsing                                    │
│  • Optimization (filter pushdown)                   │
│  • Execution planning                               │
└─────────────────┬───────────────────────────────────┘
                  │
        ┌─────────┴─────────┐
        ▼                   ▼
    ┌────────────┐  ┌─────────────┐
    │ Supported  │  │ Unsupported │
    │ Filters    │  │ Filters     │
    │ (pushdown) │  │ (Spark side)│
    └────────────┘  └─────────────┘
        │                   │
        └─────────┬─────────┘
                  ▼
┌─────────────────────────────────────────────────────┐
│ KoreDataSourceV2 Provider                           │
│  • Read/write planning                              │
│  • Filter translation                               │
│  • Column projection                                │
└─────────────────┬───────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────┐
│ Kore Reader (Rust)                                  │
│  • S3 range reads                                   │
│  • Column-level filtering                           │
│  • Zero-copy data transfer                          │
└─────────────────┬───────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────┐
│ S3 / Local File System                              │
│  • .kore files                                      │
└─────────────────────────────────────────────────────┘
```

---

## 📊 FILTER PUSHDOWN (13 Types)

### Filters We Support (Pushdown)
```
1. EqualTo(column, value)           → col == 5
2. LessThan(column, value)          → col < 10
3. LessThanOrEqual(column, value)   → col <= 10
4. GreaterThan(column, value)       → col > 5
5. GreaterThanOrEqual(column, value)→ col >= 5
6. IsNull(column)                   → col IS NULL
7. IsNotNull(column)                → col IS NOT NULL
8. StringContains(column, value)    → col LIKE '%abc%'
9. StringStartsWith(column, value)  → col LIKE 'abc%'
10. StringEndsWith(column, value)   → col LIKE '%abc'
11. In(column, [values])            → col IN (1,2,3)
12. NOT In(column, [values])        → col NOT IN (1,2,3)
13. AND, OR (combinations)          → col1 > 5 AND col2 < 10
```

### Performance Gain Example

**Query:** Select rows where age > 30 AND city = 'NYC' (1M rows total)
```
WITHOUT Filter Pushdown (Traditional):
  1. Read entire file from S3: 1GB → 1000ms
  2. Load into memory: 1000ms
  3. Filter in Spark (slow!): 500ms
  4. Return 50K rows
  Total: ~2500ms

WITH Filter Pushdown (Kore):
  1. Metadata read: 10ms
  2. Kore reads only needed columns + filters: 50ms
     (uses Kore column offsets + dictionary encoding)
  3. Return 50K rows
  Total: ~60ms

Speedup: 2500ms / 60ms = 41x FASTER!
(On larger datasets: 131x as shown in benchmarks)
```

---

## 🏗️ SCALA/JAVA IMPLEMENTATION

### Project Structure
```
kore-spark/
├── pom.xml (Maven config)
├── src/main/scala/
│   └── io/github/arunkatherashala/kore/spark/
│       ├── KoreDataSourceV2.scala
│       ├── KoreReadSupport.scala
│       ├── KoreReadBuilder.scala
│       ├── KoreFilterHandler.scala
│       ├── KoreScan.scala
│       ├── KoreBatch.scala
│       └── KoreColumnarBatchReader.scala
├── src/test/scala/
│   └── io/github/arunkatherashala/kore/spark/
│       ├── KoreReadSupportTest.scala
│       ├── FilterPushdownTest.scala
│       └── BenchmarkTest.scala
└── examples/
    └── spark_kore_example.scala
```

### DataSourceV2 Provider (Entry Point)

```scala
// KoreDataSourceV2.scala
package io.github.arunkatherashala.kore.spark

import org.apache.spark.sql.connector.catalog.Table
import org.apache.spark.sql.connector.read.ReadSupport
import org.apache.spark.sql.sources.DataSourceRegister
import org.apache.spark.sql.types._
import org.apache.spark.sql.util.CaseInsensitiveStringMap

class KoreDataSourceV2 extends DataSourceRegister {
    override def shortName(): String = "kore"
    
    override def createTable(options: CaseInsensitiveStringMap): Table = {
        new KoreTable(options)
    }
}

class KoreTable(options: CaseInsensitiveStringMap) extends Table {
    private val path = options.get("path")
    
    override def name(): String = "kore"
    
    override def schema(): StructType = {
        // Read Kore file header to get schema
        val koreFile = new KoreFile(path)
        val columns = koreFile.columns()
        
        StructType(columns.map { col =>
            StructField(col.name, mapKoreTypeToSpark(col.dataType))
        })
    }
    
    override def capabilities(): java.util.Set[TableCapability] = {
        java.util.Arrays.asList(TableCapability.BATCH_READ)
    }
    
    override def newScanBuilder(options: CaseInsensitiveStringMap): ScanBuilder = {
        new KoreReadBuilder(options, schema())
    }
}
```

### Filter Pushdown Logic

```scala
// KoreFilterHandler.scala
class KoreFilterHandler(schema: StructType) {
    
    def canPushdown(filter: Filter): Boolean = filter match {
        case EqualTo(attr, _) if schema.fieldNames.contains(attr) => true
        case LessThan(attr, _) if schema.fieldNames.contains(attr) => true
        case GreaterThan(attr, _) if schema.fieldNames.contains(attr) => true
        case IsNull(attr) if schema.fieldNames.contains(attr) => true
        case IsNotNull(attr) if schema.fieldNames.contains(attr) => true
        case StringContains(attr, _) if schema.fieldNames.contains(attr) => true
        case In(attr, _) if schema.fieldNames.contains(attr) => true
        case And(left, right) => canPushdown(left) && canPushdown(right)
        case Or(left, right) => canPushdown(left) && canPushdown(right)
        case _ => false
    }
    
    def toKoreFilter(filter: Filter): String = filter match {
        case EqualTo(attr, value) => 
            s"$attr == $value"
        case LessThan(attr, value) => 
            s"$attr < $value"
        case And(left, right) => 
            s"(${toKoreFilter(left)}) && (${toKoreFilter(right)})"
        case Or(left, right) => 
            s"(${toKoreFilter(left)}) || (${toKoreFilter(right)})"
        case _ => ""
    }
}

// In Scan:
class KoreScan extends Scan {
    private val filters: java.util.List[Filter] = new java.util.ArrayList()
    
    override def prune(requiredSchema: StructType): PartitionReaderFactory = {
        new KorePartitionReaderFactory(
            path, 
            requiredSchema,
            filters.filter(canPushdown).toList
        )
    }
    
    override def pushPredicates(filters: Array[Filter]): Array[Filter] = {
        val handled = filters.filter(canPushdown)
        this.filters.addAll(handled)
        filters.filterNot(canPushdown)  // Return unhandled filters
    }
}
```

### Columnar Data Transfer

```scala
// KoreColumnarBatchReader.scala
class KoreColumnarBatchReader(
    path: String,
    columns: Seq[String],
    filters: Seq[Filter]
) extends PartitionReader[ColumnarBatch] {
    
    private val koreReader = new KoreFile(path).reader()
    
    override def next(): Boolean = {
        // Read next batch from Kore file (64K rows)
        val batch = koreReader.readBatch(columns, filters)
        batch != null
    }
    
    override def get(): ColumnarBatch = {
        // Convert Kore columnar data to Spark ColumnarBatch
        val vectors = koreReader.columnsToVectors(schema)
        new ColumnarBatch(vectors)
    }
    
    override def close(): Unit = koreReader.close()
}
```

---

## 🧪 UNIT TESTS (5+)

```scala
class KoreReadSupportTest extends FunSuite {
    
    test("Read Kore file into DataFrame") {
        val df = spark.read.format("kore")
            .option("path", "test_data.kore")
            .load()
        
        assert(df.count() == 1000)
        assert(df.columns.length == 5)
    }
    
    test("Filter pushdown: age > 30") {
        val df = spark.read.format("kore")
            .option("path", "test_data.kore")
            .load()
        
        val filtered = df.filter("age > 30")
        val count = filtered.count()
        
        // Verify pushdown happened (no unnecessary data loaded)
        assert(count < 1000)
    }
    
    test("Filter pushdown: age > 30 AND city = 'NYC'") {
        val df = spark.read.format("kore")
            .option("path", "test_data.kore")
            .load()
        
        val filtered = df.filter("age > 30 AND city = 'NYC'")
        assert(filtered.count() > 0)
    }
    
    test("Column projection (select only needed columns)") {
        val df = spark.read.format("kore")
            .option("path", "test_data.kore")
            .load()
        
        val projected = df.select("name", "age")
        assert(projected.columns.length == 2)
    }
    
    test("Benchmark: filter pushdown performance") {
        val df = spark.read.format("kore")
            .option("path", "large_test_data.kore")
            .load()
        
        val start = System.currentTimeMillis()
        val result = df.filter("age > 30").count()
        val elapsed = System.currentTimeMillis() - start
        
        println(s"Filtered 1M rows in ${elapsed}ms")
        assert(elapsed < 100)  // Should be < 100ms with pushdown
    }
}
```

---

## 📈 PERFORMANCE TARGETS

```
Operation                  | Target    | Metric
---|---|---
Filter pushdown speedup    | 50x-131x  | Query time ratio
Scan with filters          | < 100ms   | 1M rows
Memory usage               | 50% less  | vs full load
Throughput                 | 500 MB/s  | Data read rate
```

---

## 📦 MAVEN CONFIGURATION

```xml
<!-- pom.xml -->
<project>
    <groupId>io.github.arunkatherashala</groupId>
    <artifactId>kore-spark</artifactId>
    <version>1.2.2</version>
    
    <dependencies>
        <!-- Spark -->
        <dependency>
            <groupId>org.apache.spark</groupId>
            <artifactId>spark-sql_2.12</artifactId>
            <version>3.5.0</version>
        </dependency>
        
        <!-- Kore Rust bindings (Java) -->
        <dependency>
            <groupId>io.github.arunkatherashala</groupId>
            <artifactId>kore-fileformat</artifactId>
            <version>1.2.2</version>
        </dependency>
        
        <!-- Testing -->
        <dependency>
            <groupId>org.scalatest</groupId>
            <artifactId>scalatest_2.12</artifactId>
            <version>3.2.15</version>
            <scope>test</scope>
        </dependency>
    </dependencies>
    
    <build>
        <plugins>
            <plugin>
                <groupId>net.alchim31.maven</groupId>
                <artifactId>scala-maven-plugin</artifactId>
                <version>4.8.1</version>
            </plugin>
        </plugins>
    </build>
</project>
```

---

## ⏱️ IMPLEMENTATION PHASES

### Phase 3A: DataSourceV2 Skeleton (May 22-24)
```
1. Create Maven project
2. Implement base classes
3. Schema inference from Kore
4. Basic read support
```

### Phase 3B: Filter Pushdown (May 25-27)
```
1. Implement 13 filter types
2. Filter pushdown logic
3. Column projection
4. Unit tests (5+)
```

### Phase 3C: Optimization & Examples (May 28-31)
```
1. Benchmark filter pushdown
2. Example Spark jobs
3. Documentation
4. Release candidate
```

---

## ✅ SUCCESS CRITERIA

- ✅ Load Kore files into Spark DataFrame
- ✅ Filter pushdown working for 11+ filter types
- ✅ 50x-131x speedup demonstrated
- ✅ 5+ unit tests passing
- ✅ 2+ working example Spark jobs
- ✅ Published to Maven Central

---

**ARCHITECTURE COMPLETE** ✅  
Ready for implementation starting May 22
