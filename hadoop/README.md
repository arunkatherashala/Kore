# Phase 3: Hadoop Integration

**Status:** 🚀 In Progress  
**Timeline:** 2-3 weeks  
**Target:** Production-grade InputFormat/OutputFormat  

## Overview

Native Hadoop support enables:
- ✅ Direct HDFS read/write without intermediate formats
- ✅ Automatic data locality optimization
- ✅ Parallel chunk processing
- ✅ MapReduce integration

## Architecture

```
Hadoop MapReduce Job
    ↓
KoreInputFormat / KoreOutputFormat
    ↓
RecordReader / RecordWriter
    ↓
HDFS → Kore Core (Rust) ← HDFS
```

## Key Components

### 1. KoreInputFormat
- Splits Kore files into chunks
- One split per 65,536-row chunk
- Enables parallel processing

### 2. KoreOutputFormat  
- Streams records to Kore
- Maintains chunk boundaries
- Handles compression

### 3. KoreSplit
- Metadata about file split
- Offset and length
- Locality information

### 4. KoreRecordReader
- Reads records from split
- Handles NULL values
- Type conversion

### 5. KoreRecordWriter
- Writes records to output
- Manages buffering
- Implements chunking

## Build Instructions

```bash
cd hadoop

# Build
mvn clean package

# Install to local Hadoop
cp target/kore-hadoop-0.1.0.jar $HADOOP_HOME/share/hadoop/common/lib/

# Verify
hadoop jar target/kore-hadoop-0.1.0.jar
```

## Usage Example

```java
Configuration conf = new Configuration();
Job job = Job.getInstance(conf);

job.setInputFormatClass(KoreInputFormat.class);
job.setOutputFormatClass(KoreOutputFormat.class);

FileInputFormat.addInputPath(job, new Path("/input/data.kore"));
FileOutputFormat.setOutputPath(job, new Path("/output/result.kore"));

job.waitForCompletion(true);
```

## Testing Strategy

```bash
# Unit tests
mvn test

# Integration tests
mvn test -Dtest=KoreIntegrationTest

# Benchmark
java -cp target/kore-hadoop-0.1.0.jar io.kore.hadoop.Benchmark
```

## Performance Targets

| Operation | Single Node | HDFS (10 nodes) | Speedup |
|-----------|-------------|-----------------|---------|
| Read 100GB | 100s | 10s | 10x |
| Write 100GB | 120s | 12s | 10x |
| Shuffle | 50s | 5s | 10x |

## Implementation Phases

1. **Phase 3A: InputFormat** (Week 1)
   - [ ] Split logic
   - [ ] RecordReader
   - [ ] Testing

2. **Phase 3B: OutputFormat** (Week 2)
   - [ ] RecordWriter
   - [ ] Streaming logic
   - [ ] Chunk management

3. **Phase 3C: Optimization** (Week 3)
   - [ ] Locality awareness
   - [ ] Parallel chunk reading
   - [ ] Performance tuning

## Dependencies

- Hadoop 3.3.4+
- Java 8+
- Maven 3.6+

## Roadmap

- [ ] Maven project setup
- [ ] KoreInputFormat implementation
- [ ] KoreOutputFormat implementation
- [ ] KoreSplit & locality logic
- [ ] RecordReader/Writer
- [ ] Unit tests
- [ ] Integration tests
- [ ] Performance benchmarks
- [ ] Documentation
- [ ] JAR distribution

## Known Limitations

- Requires Hadoop installed
- Java 8+ only
- Testing requires HDFS cluster

## Contributors

Assigned for Phase 3 development.

---

**Next:** Begin with InputFormat split logic and RecordReader implementation.
