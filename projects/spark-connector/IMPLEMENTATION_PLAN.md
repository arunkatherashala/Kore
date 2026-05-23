# Spark Connector: Implementation Plan

## Phase 1: Foundation (Week 1)

### 1.1 Project Setup
- [ ] Initialize Scala/Maven project
- [ ] Spark 3.3 dependency
- [ ] Kore library dependency
- [ ] Scala + Java cross-compilation

### 1.2 Dependency Stack
```xml
<!-- pom.xml dependencies -->
<dependency>
  <groupId>org.apache.spark</groupId>
  <artifactId>spark-sql_2.12</artifactId>
  <version>3.3.0</version>
</dependency>
<dependency>
  <groupId>com.kore</groupId>
  <artifactId>kore-fileformat</artifactId>
  <version>1.2.2</version>
</dependency>
```

### 1.3 Build Configuration
- [ ] Maven build (JAR packaging)
- [ ] Scala/Java compilation
- [ ] Shade dependencies (shadow jar)
- [ ] Compatibility matrix (Spark 3.0-3.3)

---

## Phase 2: DataSourceV2 Core (Week 2)

### 2.1 Implement DataSource Interface
```scala
class KoreDataSource extends DataSource {
  override def shortName(): String = "kore"
  override def inferSchema(options: CaseInsensitiveStringMap): StructType
  override def getTable(options: CaseInsensitiveStringMap): Table
}
```

### 2.2 Implement Table Interface
```scala
class KoreTable(path: String) extends Table {
  override def name(): String
  override def schema(): StructType
  override def capabilities(): Set[TableCapability]
  override def newScanBuilder(options: CaseInsensitiveStringMap): ScanBuilder
}
```

### 2.3 Implement ScanBuilder + Scan
```scala
class KoreScanBuilder extends ScanBuilder {
  override def build(): Scan
  override def pushProjection(projection: Array[Array[Int]]): Boolean
  override def pushFilters(filters: Array[Filter]): Array[Filter]
}
```

---

## Phase 3: Filter Pushdown (Week 3)

### 3.1 Filter Type Implementation (13 total)

**Comparison Filters:**
- [ ] EqualTo(col, val)
- [ ] LessThan(col, val)
- [ ] LessThanOrEqual(col, val)
- [ ] GreaterThan(col, val)
- [ ] GreaterThanOrEqual(col, val)

**Containment Filters:**
- [ ] In(col, [values])
- [ ] StringStartsWith(col, prefix)
- [ ] StringEndsWith(col, suffix)
- [ ] StringContains(col, substring)

**Null Filters:**
- [ ] IsNull(col)
- [ ] IsNotNull(col)

**Logical Filters:**
- [ ] And(left, right)
- [ ] Or(left, right)
- [ ] Not(child)

### 3.2 Filter Optimization
```scala
def pushFilters(filters: Array[Filter]): Array[Filter] = {
  val (pushable, rest) = filters.partition(isPushable)
  // Translate pushable filters to Kore predicates
  predicates = translateFilters(pushable)
  rest // Return unpushable filters for Spark to handle
}
```

### 3.3 Performance Optimization
- [ ] Skip entire row groups if predicate eliminates all
- [ ] Combine AND filters for multi-column predicates
- [ ] Short-circuit OR evaluation

---

## Phase 4: Vectorized Reading (Week 4)

### 4.1 Batch Read Implementation
```scala
class KorePartitionReader(path: String, schema: StructType) {
  override def next(): Boolean
  override def get(): ColumnarBatch
  override def close(): Unit
}
```

### 4.2 Arrow/Columnar Format
- [ ] Read Kore columns into Arrow vectors
- [ ] Maintain column order for Spark optimization
- [ ] Handle null/missing value encoding

### 4.3 Memory Management
- [ ] Batches: 10K rows per batch (configurable)
- [ ] Off-heap buffers for large columns
- [ ] Garbage collection optimization

---

## Phase 5: Testing & Performance (Week 5)

### 5.1 Unit Tests
- [ ] Filter translation tests (all 13 types)
- [ ] Schema inference tests
- [ ] Partition reader tests
- [ ] Error handling tests

### 5.2 Integration Tests
- [ ] Spark SQL: `SELECT * FROM kore.'/path/to/file.kore'`
- [ ] Filter evaluation: `WHERE id = 100`
- [ ] Column projection: `SELECT id, name FROM ...`
- [ ] Complex queries: `WHERE (id > 50 AND status = 'active') OR created > '2026-01-01'`

### 5.3 Performance Benchmarks
- [ ] Read 1GB Kore file
- [ ] Filter selectivity: 10%, 50%, 90%
- [ ] Projection: all columns vs 2 of 10 columns
- [ ] vs Parquet connector (baseline)

### 5.4 Compatibility Testing
- [ ] Spark 3.0, 3.1, 3.2, 3.3
- [ ] PySpark (via py4j)
- [ ] Scala shell
- [ ] Jupyter/Databricks

---

## Phase 6: Documentation (Week 6)

### 6.1 Developer Docs
- [ ] Architecture guide
- [ ] Filter implementation reference
- [ ] Performance tuning guide

### 6.2 User Docs
- [ ] Installation guide
- [ ] SQL examples
- [ ] PySpark examples
- [ ] Scala examples

### 6.3 Examples
- [ ] 10MB test file + queries
- [ ] Real-world analytics workload
- [ ] Time-series data with filter pushdown

---

## Acceptance Criteria
- ✅ All 13 filter types implemented
- ✅ Spark 3.0-3.3 compatibility
- ✅ Read throughput >5GB/sec
- ✅ Filter pushdown reduces I/O >90%
- ✅ PySpark + Scala + SQL working
- ✅ Zero data loss roundtrip
- ✅ Performance competitive with Parquet

## Status: Ready for Phase 1
