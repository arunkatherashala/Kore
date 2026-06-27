# Phase 4: Spark SQL DataSourceV2

**Status:** 🚀 In Progress  
**Timeline:** 2-3 weeks  
**Target:** Production-grade Spark SQL integration  

## Overview

Full DataSourceV2 API implementation enables:
- ✅ Native Spark SQL support: `spark.read.format("kore")`
- ✅ Query optimization (partition pruning, column pushdown)
- ✅ Streaming support (optional)
- ✅ Catalog integration

## Architecture

```
Spark SQL Query
    ↓
KoreDataSource (Provider)
    ↓
KoreTable + KoreScan/KoreWrite
    ↓
KorePartitionReader / PartitionWriter
    ↓
Kore Core (Rust)
```

## Key Components

### 1. KoreDataSource
- Implements `TableProvider`
- Schema inference
- Table creation

### 2. KoreTable
- Implements `Table`
- Read/Write builders
- Capabilities definition

### 3. KoreScan
- Partition discovery
- Column/predicate pushdown
- Statistics

### 4. KorePartitionReader
- Read individual partitions
- Row iteration
- Type conversion

### 5. KorePartitionWriter
- Write partitions
- Transactional semantics
- Error handling

## Build Instructions

```bash
cd spark-scala

# Build
sbt clean package

# Test
sbt test

# Integration test
sbt "testOnly io.kore.spark.KoreDataSourceTest"
```

## Usage Example

```scala
// Register data source
spark.read.format("kore").load("data.kore").show()

// Write DataFrame
df.write.format("kore").mode("overwrite").save("output.kore")

// Spark SQL
spark.sql("SELECT * FROM kore.`/path/to/data.kore` WHERE age > 30")
```

## Query Optimization

### Column Pushdown
```scala
// Only reads 'name' column
spark.read.format("kore").load("data.kore").select("name")
```

### Predicate Pushdown
```scala
// Filter applied at read layer
spark.read.format("kore").load("data.kore").filter("age > 30")
```

### Partition Pruning
```scala
// Only reads relevant partitions
df.where("date >= '2024-01-01'")
```

## Testing Strategy

```bash
# Unit tests
sbt test

# Integration with real Spark
sbt "testOnly io.kore.spark.IntegrationTest"

# Performance benchmarks
sbt "run -Dbenchmark=true"
```

## Performance Targets

| Query | DataFrame API | Spark SQL | Speedup |
|-------|---|---|---|
| Full read | 1.0s | 1.0s | baseline |
| Column select | 0.2s | 0.2s | 5x |
| Filtered read | 0.3s | 0.3s | 3x |
| Aggregation | 0.5s | 0.5s | 2x |

## Implementation Phases

1. **Phase 4A: Core API** (Week 1)
   - [ ] DataSource provider
   - [ ] Table implementation
   - [ ] Basic read/write

2. **Phase 4B: Optimization** (Week 2)
   - [ ] Column pushdown
   - [ ] Predicate pushdown
   - [ ] Partition discovery

3. **Phase 4C: Testing** (Week 3)
   - [ ] Unit tests
   - [ ] Integration tests
   - [ ] Benchmarks

## Dependencies

- Spark 3.5.0+
- Scala 2.12.x
- Hadoop 3.3.4+
- SBT 1.9+

## Roadmap

- [ ] SBT project setup
- [ ] KoreDataSource implementation
- [ ] KoreTable implementation
- [ ] KoreScan implementation
- [ ] KorePartitionReader
- [ ] KorePartitionWriter
- [ ] Column pushdown
- [ ] Predicate pushdown
- [ ] Unit tests
- [ ] Integration tests
- [ ] Benchmarks
- [ ] JAR distribution

## Known Limitations

- Spark 3.5+ required
- Scala 2.12.x only
- No streaming yet (Phase future)

## Contributors

Assigned for Phase 4 development.

---

**Next:** Begin with DataSource provider and basic Table implementation.
