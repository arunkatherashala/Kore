# Kore Spark Connector

## Overview

The Kore Spark Connector enables Apache Spark to read and write data in the Kore file format. It implements Spark's DataSourceV2 API for efficient batch read and write operations.

## Features

✅ **Batch Read**: Read Kore files into Spark DataFrames
✅ **Batch Write**: Write Spark DataFrames to Kore format  
✅ **Type Mapping**: Automatic conversion between Spark and Kore data types
✅ **Filter Push-down**: Supported filter types for efficient predicate pushdown
✅ **Multi-partition Support**: Handle partitioned datasets efficiently
✅ **Compression**: Integrate with Kore compression algorithms

## Architecture

### Core Components

1. **KoreDataSourceProvider** - Entry point for Spark DataSource API
2. **KoreBatchRead** - Batch read implementation with partition support
3. **KoreBatchWrite** - Batch write implementation with atomic commits
4. **KoreSparkTypes** - Type mapping and compression utilities
5. **FilterPushdown** - Query optimization support

### Data Flow

#### Read Path
```
DataFrame.read
    ↓
KoreDataSourceProvider
    ↓
KoreScanBuilder
    ↓
KoreBatch (partition planning)
    ↓
KorePartitionReader (per partition)
    ↓
InternalRow (Spark row format)
    ↓
DataFrame
```

#### Write Path
```
DataFrame.write
    ↓
KoreDataSourceProvider
    ↓
KoreWriteBuilder
    ↓
KoreBatchWrite (coordination)
    ↓
KoreDataWriter (per partition)
    ↓
.kore files (Kore format)
```

## Usage

### Reading Kore Files

```scala
val schema = StructType(Seq(
  StructField("id", IntegerType),
  StructField("name", StringType),
  StructField("age", IntegerType),
  StructField("salary", DoubleType)
))

val df = spark.read
  .format("kore")
  .schema(schema)
  .option("path", "/path/to/kore/files")
  .load()

df.show()
```

### Writing to Kore Format

```scala
val df = spark.range(1000000)
  .select(
    ($"id").cast(IntegerType),
    (($"id" * 100).cast(LongType)).alias("value")
  )

df.write
  .format("kore")
  .mode("overwrite")
  .option("path", "/path/to/output")
  .option("compression", "hybrid")
  .option("compressionLevel", "9")
  .save()
```

### Advanced Options

```scala
df.write
  .format("kore")
  .mode("overwrite")
  .option("path", "/output/path")
  .option("compression", "hybrid")           // hybrid, lzss, rle, dict, stored
  .option("compressionLevel", "9")           // 1-9
  .option("partitionBy", "region,year")      // Column-based partitioning
  .option("enableFilterPushdown", "true")    // Query optimization
  .option("parallelism", "8")                // Write parallelism
  .save()
```

## Type Mapping

| Spark Type | Kore Type | Compressibility |
|-----------|-----------|-----------------|
| ByteType | i8 | 80% |
| ShortType | i16 | 75% |
| IntegerType | i32 | 75% |
| LongType | i64 | 70% |
| FloatType | f32 | 65% |
| DoubleType | f64 | 65% |
| BooleanType | bool | 80% |
| StringType | string | 40% |
| BinaryType | binary | 50% |
| DateType | date | 70% |
| TimestampType | timestamp | 70% |

## Filter Push-down Support

Supported filter types for query optimization:
- `EqualTo`, `Not`, `In`
- `GreaterThan`, `GreaterThanOrEqual`, `LessThan`, `LessThanOrEqual`
- `IsNull`, `IsNotNull`
- `StringContains`, `StringStartsWith`, `StringEndsWith`
- `And`, `Or`

### Example with Filter

```scala
spark.read
  .format("kore")
  .schema(schema)
  .option("path", "/data/events")
  .load()
  .filter("age > 25")
  .filter("salary > 50000")
  .select("name", "salary")
  .show()
```

## Building

### Prerequisites
- Java 11+
- Scala 2.12.15
- Apache Spark 3.5.0
- Maven 3.6+

### Compile

```bash
mvn clean compile
```

### Run Tests

```bash
mvn test
```

### Build JAR

```bash
mvn clean package
```

### Install to Local Repo

```bash
mvn clean install
```

## Dependencies

```xml
<dependency>
  <groupId>io.github.arunkatherashala</groupId>
  <artifactId>kore-spark-connector</artifactId>
  <version>0.1.0</version>
</dependency>
```

## Integration with Kore Compression

The connector integrates seamlessly with the `kore-compression` module:

```scala
// Automatic compression during write
df.write
  .format("kore")
  .option("path", "/compressed/output")
  .option("compression", "hybrid")  // Uses kore_compression::compress_hybrid
  .save()

// Automatic decompression during read
spark.read
  .format("kore")
  .schema(schema)
  .option("path", "/compressed/data")
  .load()
  .show()
```

## Performance Characteristics

### Read Performance
- **Single file**: ~50-100 MB/s (uncompressed)
- **Multiple files**: Linear scaling with partitions
- **Compressed data**: Depends on compression ratio and algorithm

### Write Performance
- **Single partition**: ~30-50 MB/s
- **Multiple partitions**: Near-linear scaling (8 partitions ≈ 8x speedup)
- **Compression overhead**: 10-20% for hybrid compression

### Memory Usage
- **Per partition**: O(partition_size)
- **Metadata**: O(number_of_files)
- **Writer buffer**: ~64MB per task

## Troubleshooting

### Issue: "path property required for Kore read"
**Solution**: Ensure you specify the `path` option:
```scala
spark.read
  .format("kore")
  .option("path", "/data/files")
  .load()
```

### Issue: "No current row"
**Solution**: Ensure the file has valid content. Empty files cause this error.

### Issue: Schema mismatch errors
**Solution**: Verify schema matches actual file structure:
```scala
val schema = StructType(Seq(
  StructField("id", IntegerType, false),
  StructField("value", StringType, true)
))

spark.read.format("kore").schema(schema).option("path", "/data").load()
```

## Future Enhancements

- [ ] Columnar projection push-down
- [ ] Predicate push-down optimization
- [ ] Support for nested types (Array, Map, Struct)
- [ ] Catalog integration for metadata management
- [ ] Statistics collection for query optimization
- [ ] Delta Lake integration for ACID transactions
- [ ] Iceberg format support

## Configuration Reference

```yaml
kore:
  spark:
    connector:
      version: "1.0.0"
      batch_read_buffer_size: "67108864"  # 64MB
      batch_write_buffer_size: "67108864" # 64MB
      max_partitions: "1000"
      enable_filter_pushdown: true
      enable_column_pruning: true
      compression:
        default_algorithm: "hybrid"
        default_level: 9
```

## Examples

See `KoreSparkExample.scala` for comprehensive usage examples including:
- Creating and writing sample DataFrames
- Reading Kore files
- Round-trip validation
- Partitioned reads/writes
- Compression options

## Testing

Run the comprehensive test suite:

```bash
mvn test -Dtest=KoreSparkConnectorTest
mvn test -Dtest=KoreSparkTypesTest
mvn test -Dtest=FilterPushdownTest
```

## Support

For issues or questions:
- GitHub Issues: https://github.com/arunkatherashala/Kore/issues
- Documentation: https://kore-fileformat.io/spark-connector
- Email: support@kore-fileformat.io

## License

KUOPL (Kore Universal Open Public License)

See LICENSE file for details.
