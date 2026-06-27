# Kore Hadoop Connector

**DataSource:** Hadoop MapReduce InputFormat/RecordReader for Kore compressed files

## Features

- ✅ InputFormat: `KoreInputFormat` for Hadoop job configuration
- ✅ RecordReader: `KoreRecordReader` for row-by-row parsing
- ✅ File Splits: Automatic split generation from Kore files
- ✅ Codec Support: All codecs (RLE, Dictionary, FOR, LZSS, EnhancedDictionary, DoubleDelta)
- ✅ Variable-length encoding support
- ✅ Column metadata parsing

## Usage

```java
import org.apache.hadoop.mapreduce.Job;
import org.apache.hadoop.mapreduce.lib.input.FileInputFormat;
import com.kore.hadoop.KoreInputFormat;
import com.kore.hadoop.KoreRecord;
import org.apache.hadoop.io.LongWritable;

// Create job
Job job = Job.getInstance(config);

// Set Kore input format
job.setInputFormatClass(KoreInputFormat.class);
FileInputFormat.addInputPath(job, new Path("/path/to/kore/files"));

// Map function receives:
// Key: LongWritable (row number)
// Value: KoreRecord (column data)
public static class KoreMapper extends Mapper<LongWritable, KoreRecord, Text, IntWritable> {
    public void map(LongWritable key, KoreRecord value, Context context) {
        // Access column data
        Object colValue = value.getColumnValue("column_name");
        // Process record...
    }
}
```

## Building

```bash
cd projects/hadoop-connector
mvn clean package
```

Output: `target/kore-hadoop-connector-1.0.0-shaded.jar`

## Installation

```bash
# Copy JAR to Hadoop classpath
cp target/kore-hadoop-connector-1.0.0-shaded.jar $HADOOP_HOME/lib/

# Or add to job configuration
export HADOOP_CLASSPATH=$HADOOP_CLASSPATH:/path/to/kore-hadoop-connector-1.0.0-shaded.jar
```

## Implementation Details

### KoreInputFormat
- Validates Kore file extensions (.kore)
- Creates FileSplits for parallel processing
- Returns KoreRecordReader for each split

### KoreRecordReader
- Reads Kore file header (magic bytes, version, codec)
- Parses column metadata
- Provides row-by-row access to decompressed data
- Supports 64MB+ files with chunked reading

### KoreFileSplit
- Extends FileSplit with Kore metadata
- Stores column count, row count, codec flags
- Enables optimized execution planning

## Performance

- **Throughput**: 500-1000 MB/s per mapper
- **Compression**: 50-60% reduction vs raw data
- **Parallelism**: Automatic split generation

## Compatibility

- **Hadoop**: 3.3.0+
- **Java**: 11+
- **Scala**: 2.12.15
