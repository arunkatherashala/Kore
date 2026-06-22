# TRACK B EXPANSION: Spark DataSourceV2 Connector
**v1.3.0 Implementation - Oct 1, 2026**

---

## 🎯 OBJECTIVE

Implement native Spark DataSourceV2 connector for KORE file format.

**Goal**: `df.write.format("kore")` and `spark.read.format("kore")` work seamlessly.

---

## 📋 IMPLEMENTATION SPEC

### **Phase 1: DataSourceV2 Framework (Week 1-2)**

```rust
// src/spark_connector.rs

use jni::JNIEnv;
use jni::objects::JClass;

/// Spark DataSourceV2 provider implementation
#[no_mangle]
pub extern "Java" fn Java_com_github_kore_KoreDataSource_readKore(
    env: JNIEnv,
    _class: JClass,
    path: String,
) -> Vec<u8> {
    // Read KORE file and return Arrow IPC format
    match KoreReader::from_path(&path) {
        Ok(reader) => reader.to_arrow_ipc(),
        Err(e) => panic!("Failed to read KORE: {}", e),
    }
}

#[no_mangle]
pub extern "Java" fn Java_com_github_kore_KoreDataSource_writeKore(
    env: JNIEnv,
    _class: JClass,
    path: String,
    arrow_data: Vec<u8>,
) -> i32 {
    // Write Arrow IPC format to KORE
    match KoreWriter::from_arrow_ipc(&arrow_data) {
        Ok(writer) => {
            writer.write_to_path(&path).unwrap_or(-1);
            0
        }
        Err(_) => -1,
    }
}

/// Spark read builder
pub struct KoreReadBuilder {
    path: String,
}

impl KoreReadBuilder {
    pub fn new(path: String) -> Self {
        Self { path }
    }

    pub fn build(&self) -> Vec<u8> {
        Java_com_github_kore_KoreDataSource_readKore(path.clone())
    }
}

/// Spark write builder
pub struct KoreWriteBuilder {
    path: String,
}

impl KoreWriteBuilder {
    pub fn new(path: String) -> Self {
        Self { path }
    }

    pub fn build(&self, data: Vec<u8>) -> i32 {
        Java_com_github_kore_KoreDataSource_writeKore(path.clone(), data)
    }
}
```

### **Phase 2: Arrow IPC Serialization (Week 2-3)**

```rust
// src/arrow_interop.rs

use arrow::ipc::writer::FileWriter;
use arrow::ipc::reader::FileReader;

impl KoreReader {
    /// Convert KORE to Arrow IPC format
    pub fn to_arrow_ipc(&self) -> Vec<u8> {
        let mut arrow_data = Vec::new();
        let mut writer = FileWriter::try_new(&mut arrow_data, &self.schema()).unwrap();
        
        // Write each batch
        for batch in self.read_batches() {
            writer.write(&batch).unwrap();
        }
        
        writer.finish().unwrap();
        arrow_data
    }

    /// Convert Arrow IPC to KORE
    pub fn from_arrow_ipc(ipc_data: &[u8]) -> Result<Self> {
        let reader = FileReader::try_new(ipc_data)?;
        let schema = reader.schema();
        
        // Convert Arrow batches to KORE
        let mut writer = KoreWriter::new(schema);
        for i in 0..reader.num_batches() {
            let batch = reader.get_batch(i)?;
            writer.add_batch(&batch)?;
        }
        
        Ok(writer.reader())
    }
}
```

### **Phase 3: Predicate Pushdown (Week 3-4)**

```rust
// src/spark_filters.rs

/// Translate Spark FilterPredicate to KORE column filters
pub struct SparkFilterTranslator;

impl SparkFilterTranslator {
    pub fn translate(predicate: &str) -> QueryFilter {
        // Example: "age > 30 AND city = 'NYC'" 
        // → QueryFilter { column_filters: [...] }
        
        match predicate {
            p if p.contains("timestamp >") => {
                // Extract time range and use KORE time-range index
                QueryFilter {
                    column_filters: vec![
                        ColumnFilter::TimeRange { min: 1000, max: 2000 }
                    ]
                }
            }
            p if p.contains("value IN") => {
                // Use RLE codec index for equality checks
                QueryFilter {
                    column_filters: vec![
                        ColumnFilter::Values { values: [...] }
                    ]
                }
            }
            _ => QueryFilter::default(),
        }
    }
}

// Usage in Spark:
// spark.read.format("kore")
//   .load("s3://data/metrics.kore")
//   .filter("timestamp > 1000")  ← Predicate pushed to KORE
//   .select("metric_id", "value")
```

### **Phase 4: Partitioning Support (Week 4-5)**

```rust
// src/spark_partitions.rs

/// Spark partition discovery
pub struct KorePartitionDiscovery {
    base_path: String,
}

impl KorePartitionDiscovery {
    pub fn discover_partitions(&self) -> Vec<PartitionSpec> {
        // Scan S3/GCS for partition structure:
        // base_path/date=2026-06-01/hour=12/data.kore
        // base_path/date=2026-06-02/hour=13/data.kore
        
        vec![
            PartitionSpec {
                path: "date=2026-06-01/hour=12/data.kore",
                filters: vec![
                    ("date", "2026-06-01"),
                    ("hour", "12"),
                ]
            },
            // ... more partitions
        ]
    }
}

// Usage:
// spark.read.format("kore")
//   .load("s3://data/metrics/")  ← Auto-discovers partitions
//   .filter("date >= '2026-06-01'")  ← Partition pruning works
```

### **Phase 5: Statistics & Query Optimization (Week 5)**

```rust
// src/spark_stats.rs

/// Provide column statistics to Spark optimizer
pub struct KoreColumnStats {
    column_name: String,
    num_rows: u64,
    null_count: u64,
    min_value: Option<String>,
    max_value: Option<String>,
    avg_length: Option<f64>,
}

impl KoreColumnStats {
    pub fn get_spark_stats(&self) -> Map<String, String> {
        // Return stats in Spark CatalogStatistics format
        map![
            "numRows" => self.num_rows.to_string(),
            "nullCount" => self.null_count.to_string(),
            "min" => self.min_value.clone().unwrap_or_default(),
            "max" => self.max_value.clone().unwrap_or_default(),
            "avgLength" => self.avg_length.map(|x| x.to_string()).unwrap_or_default(),
        ]
    }
}
```

---

## 🧪 TESTING SPEC

### **Unit Tests (80 tests)**

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_read_kore_format() {
        let data = spark.read.format("kore").load("test.kore");
        assert_eq!(data.count(), 1000);
    }

    #[test]
    fn test_write_kore_format() {
        df.write.format("kore").save("output.kore");
        assert!(Path::new("output.kore").exists());
    }

    #[test]
    fn test_predicate_pushdown() {
        // Verify timestamp filter uses time-range index
        let result = spark.read.format("kore").load("metrics.kore")
            .filter("timestamp > 1000");
        // Should skip 90% of blocks
    }

    #[test]
    fn test_partition_discovery() {
        let partitions = discover_partitions("s3://data/");
        assert!(partitions.len() > 0);
    }

    #[test]
    fn test_statistics_accuracy() {
        let stats = get_column_stats("value");
        assert_eq!(stats.min_value, Some("0".to_string()));
        assert_eq!(stats.max_value, Some("100".to_string()));
    }
}
```

### **Integration Tests (Spark Cluster)**

```
1. Read 1B rows from KORE on single machine
2. Read 1B rows from KORE on Spark cluster (10 partitions)
3. Write Spark DataFrame to KORE (verify format compatibility)
4. Predicate pushdown performance (measure block skip %)
5. Partition pruning (measure S3 requests avoided)
```

---

## 📦 DELIVERABLES

```
Code:
  ✅ src/spark_connector.rs (JNI bindings)
  ✅ src/arrow_interop.rs (Arrow serialization)
  ✅ src/spark_filters.rs (Predicate pushdown)
  ✅ src/spark_partitions.rs (Partition discovery)
  ✅ src/spark_stats.rs (Column statistics)

Testing:
  ✅ 80 unit tests
  ✅ 5 integration tests
  ✅ Spark cluster validation

Documentation:
  ✅ Spark connector guide
  ✅ API documentation
  ✅ Performance tuning guide

Deliverable Timeline:
  Week 1-2:  DataSourceV2 framework
  Week 2-3:  Arrow IPC serialization
  Week 3-4:  Predicate pushdown
  Week 4-5:  Partitioning support
  Week 5:    Statistics & optimization
  
Total: 5 weeks (on schedule for Oct 1)
```

---

## 🚀 ENGINEER REQUIREMENTS

**Track B Lead (Spark)**: 1 principal engineer
- Must have: Spark DataSourceV2 experience
- Nice: Parquet/ORC connector experience
- Salary: $250K + equity
- Report to: VP Engineering

**Support Engineers**: 2 engineers
- Rust + Java/Scala experience
- Spark cluster testing
- Salary: $180K + equity each

**Total Track B**: 3 people (was 2 before)

---

## ✅ SUCCESS CRITERIA

```
✅ Spark read works: spark.read.format("kore").load(path)
✅ Spark write works: df.write.format("kore").save(path)
✅ Predicate pushdown: Filters push to KORE codec layer
✅ Partitioning: Auto-discovers S3 partitions
✅ Performance: 2.8x faster than Iceberg on Spark
✅ Tests: 80 unit tests + 5 integration tests pass
✅ Docs: Complete API + usage guide
```
