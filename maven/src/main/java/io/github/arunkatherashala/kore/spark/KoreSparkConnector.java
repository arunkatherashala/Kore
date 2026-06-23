/// TRACK B: Spark DataSourceV2 Connector for Kore
/// 
/// Integrates Kore file format with Apache Spark as a native data source,
/// enabling parallel read/write with pushdown predicates and partition pruning.

package io.github.arunkatherashala.kore.spark;

import org.apache.spark.sql.connector.catalog.Table;
import org.apache.spark.sql.connector.catalog.TableProvider;
import org.apache.spark.sql.connector.expressions.Transform;
import org.apache.spark.sql.connector.read.ScanBuilder;
import org.apache.spark.sql.connector.write.LogicalWriteInfo;
import org.apache.spark.sql.connector.write.WriteBuilder;
import org.apache.spark.sql.types.StructType;
import org.apache.spark.sql.util.CaseInsensitiveStringMap;

import java.util.Map;

/**
 * KoreTableProvider - Spark DataSourceV2 provider for Kore format
 * 
 * Enables usage:
 *   spark.read
 *     .format("io.github.arunkatherashala.kore")
 *     .option("path", "s3://bucket/data.kore")
 *     .option("predicates", "age > 30 AND city = 'NYC'")
 *     .load()
 * 
 * Or with Spark SQL:
 *   CREATE TABLE my_table
 *   USING io.github.arunkatherashala.kore
 *   LOCATION 's3://bucket/data.kore'
 */
public class KoreTableProvider implements TableProvider {
    
    private CaseInsensitiveStringMap options;
    private StructType schema;
    
    /**
     * Called by Spark to short circuit logical inference
     */
    @Override
    public StructType inferSchema(CaseInsensitiveStringMap options) {
        this.options = options;
        String path = options.get("path");
        if (path == null) {
            throw new IllegalArgumentException("path option required for Kore format");
        }
        
        // Read Kore file metadata to infer schema
        return KoreMetadataReader.inferSchema(path);
    }
    
    /**
     * Create table instance with schema
     */
    @Override
    public Table getTable(StructType schema, Transform[] partitioning, Map<String, String> properties) {
        this.schema = schema;
        String path = options.get("path");
        
        return new KoreTable(
            path,
            schema,
            properties,
            new KoreScanBuilder(schema, options),
            new KoreWriteBuilder(schema, options)
        );
    }
    
    @Override
    public void validateOptions(CaseInsensitiveStringMap options) {
        String path = options.get("path");
        if (path == null || path.isEmpty()) {
            throw new IllegalArgumentException("path option is required");
        }
    }
}

/**
 * KoreTable - Represents a Kore file as a Spark table
 */
public class KoreTable implements Table {
    private String path;
    private StructType schema;
    private Map<String, String> properties;
    private ScanBuilder scanBuilder;
    private WriteBuilder writeBuilder;
    
    public KoreTable(String path, StructType schema, Map<String, String> properties,
                     ScanBuilder scanBuilder, WriteBuilder writeBuilder) {
        this.path = path;
        this.schema = schema;
        this.properties = properties;
        this.scanBuilder = scanBuilder;
        this.writeBuilder = writeBuilder;
    }
    
    @Override
    public String name() {
        return "Kore[" + path + "]";
    }
    
    @Override
    public StructType schema() {
        return schema;
    }
    
    @Override
    public ScanBuilder newScanBuilder(CaseInsensitiveStringMap options) {
        return scanBuilder;
    }
    
    @Override
    public WriteBuilder newWriteBuilder(LogicalWriteInfo info) {
        return writeBuilder;
    }
    
    @Override
    public Partitioning[] partitioning() {
        // Extract partitions from Kore metadata
        return KoreMetadataReader.getPartitions(path, schema);
    }
    
    @Override
    public Map<String, String> properties() {
        return properties;
    }
    
    @Override
    public boolean supportsDelete() {
        return true; // Support DELETE via ACID transactions
    }
    
    @Override
    public boolean supportsUpdate() {
        return true; // Support UPDATE via ACID transactions
    }
}

/**
 * KoreScanBuilder - Optimizes scans with pushdown predicates and partition pruning
 */
public class KoreScanBuilder extends ScanBuilder {
    private StructType schema;
    private CaseInsensitiveStringMap options;
    private Predicate[] predicates;
    
    public KoreScanBuilder(StructType schema, CaseInsensitiveStringMap options) {
        this.schema = schema;
        this.options = options;
    }
    
    /**
     * Apply filter predicates - push down to Kore for early filtering
     */
    @Override
    public ScanBuilder pushPredicates(Predicate[] predicates) {
        this.predicates = predicates;
        return this;
    }
    
    /**
     * Prune columns - only read needed columns
     */
    @Override
    public ScanBuilder pruneColumns(StructType prunedSchema) {
        this.schema = prunedSchema;
        return this;
    }
    
    /**
     * Build the actual scan
     */
    @Override
    public Scan build() {
        return new KoreScan(
            options.get("path"),
            schema,
            predicates,
            options
        );
    }
}

/**
 * KoreScan - Executes parallel scan with predicates
 */
public class KoreScan extends Scan {
    private String path;
    private StructType schema;
    private Predicate[] predicates;
    private CaseInsensitiveStringMap options;
    
    public KoreScan(String path, StructType schema, Predicate[] predicates,
                    CaseInsensitiveStringMap options) {
        this.path = path;
        this.schema = schema;
        this.predicates = predicates;
        this.options = options;
    }
    
    @Override
    public StructType readSchema() {
        return schema;
    }
    
    /**
     * Return task readers for parallel execution
     */
    @Override
    public InputPartition<ColumnarBatch>[] planInputPartitions() {
        // Get partitions from Kore metadata
        String[] partitionPaths = KoreMetadataReader.getPartitionPaths(path);
        
        InputPartition<ColumnarBatch>[] partitions = 
            new InputPartition[partitionPaths.length];
        
        for (int i = 0; i < partitionPaths.length; i++) {
            partitions[i] = new KoreInputPartition(
                partitionPaths[i],
                schema,
                predicates,
                options
            );
        }
        
        return partitions;
    }
    
    /**
     * Return factory for creating partition readers
     */
    @Override
    public PartitionReaderFactory createReaderFactory() {
        return new KorePartitionReaderFactory(schema, predicates, options);
    }
}

/**
 * KoreInputPartition - Represents a single partition to read
 */
public class KoreInputPartition implements InputPartition<ColumnarBatch> {
    private String partitionPath;
    private StructType schema;
    private Predicate[] predicates;
    private CaseInsensitiveStringMap options;
    
    public KoreInputPartition(String partitionPath, StructType schema,
                              Predicate[] predicates, CaseInsensitiveStringMap options) {
        this.partitionPath = partitionPath;
        this.schema = schema;
        this.predicates = predicates;
        this.options = options;
    }
    
    public String getPartitionPath() {
        return partitionPath;
    }
    
    public StructType getSchema() {
        return schema;
    }
    
    public Predicate[] getPredicates() {
        return predicates;
    }
    
    public CaseInsensitiveStringMap getOptions() {
        return options;
    }
}

/**
 * KorePartitionReaderFactory - Creates readers for each partition
 */
public class KorePartitionReaderFactory implements PartitionReaderFactory {
    private StructType schema;
    private Predicate[] predicates;
    private CaseInsensitiveStringMap options;
    
    public KorePartitionReaderFactory(StructType schema, Predicate[] predicates,
                                      CaseInsensitiveStringMap options) {
        this.schema = schema;
        this.predicates = predicates;
        this.options = options;
    }
    
    @Override
    public PartitionReader<ColumnarBatch> createReader(InputPartition partition) {
        KoreInputPartition korePartition = (KoreInputPartition) partition;
        
        return new KorePartitionReader(
            korePartition.getPartitionPath(),
            schema,
            predicates,
            options
        );
    }
}

/**
 * KorePartitionReader - Reads a single partition with predicate filtering
 */
public class KorePartitionReader implements PartitionReader<ColumnarBatch> {
    private String partitionPath;
    private StructType schema;
    private Predicate[] predicates;
    private CaseInsensitiveStringMap options;
    private KoreFileReader reader;
    private ColumnarBatch currentBatch;
    private boolean hasNext;
    
    public KorePartitionReader(String partitionPath, StructType schema,
                               Predicate[] predicates, CaseInsensitiveStringMap options) {
        this.partitionPath = partitionPath;
        this.schema = schema;
        this.predicates = predicates;
        this.options = options;
        this.reader = new KoreFileReader(partitionPath, schema);
        this.hasNext = true;
    }
    
    @Override
    public boolean next() throws IOException {
        while (hasNext) {
            ColumnarBatch batch = reader.readNextBatch();
            if (batch == null) {
                hasNext = false;
                return false;
            }
            
            // Apply predicate filters
            if (predicates != null && predicates.length > 0) {
                batch = applyPredicates(batch, predicates);
            }
            
            if (batch.numRows() > 0) {
                this.currentBatch = batch;
                return true;
            }
        }
        return false;
    }
    
    @Override
    public ColumnarBatch get() {
        return currentBatch;
    }
    
    @Override
    public void close() throws IOException {
        if (reader != null) {
            reader.close();
        }
    }
    
    /**
     * Apply predicates to filter rows efficiently
     * Kore ACID layer handles conflict-free filtering
     */
    private ColumnarBatch applyPredicates(ColumnarBatch batch, Predicate[] predicates) {
        // Filter rows based on predicates
        // Uses Kore's read-set to track predicate columns
        return KorePredicateFilter.filter(batch, schema, predicates);
    }
}

/**
 * KoreWriteBuilder - Handles writes with ACID transaction support
 */
public class KoreWriteBuilder extends WriteBuilder {
    private StructType schema;
    private CaseInsensitiveStringMap options;
    
    public KoreWriteBuilder(StructType schema, CaseInsensitiveStringMap options) {
        this.schema = schema;
        this.options = options;
    }
    
    @Override
    public WriteBuilder withInputDataSchema(StructType schema) {
        this.schema = schema;
        return this;
    }
    
    @Override
    public BatchWrite buildForBatch() {
        return new KoreBatchWrite(
            options.get("path"),
            schema,
            options
        );
    }
}

/**
 * KoreBatchWrite - Executes batch write with ACID transactions
 */
public class KoreBatchWrite extends BatchWrite {
    private String path;
    private StructType schema;
    private CaseInsensitiveStringMap options;
    
    public KoreBatchWrite(String path, StructType schema, CaseInsensitiveStringMap options) {
        this.path = path;
        this.schema = schema;
        this.options = options;
    }
    
    @Override
    public DataWriter<Row>[] createBatchWriters() {
        int numPartitions = Integer.parseInt(
            options.getOrDefault("numPartitions", "4")
        );
        
        DataWriter<Row>[] writers = new DataWriter[numPartitions];
        
        for (int i = 0; i < numPartitions; i++) {
            writers[i] = new KoreDataWriter(
                path + "/partition_" + i,
                schema,
                i,
                options
            );
        }
        
        return writers;
    }
    
    @Override
    public void commit(WriterCommitMessage[] messages) {
        // All partitions committed together with Kore ACID transaction
        KoreTransactionCoordinator.commitBatch(path, messages);
    }
    
    @Override
    public void abort(WriterCommitMessage[] messages) {
        // Automatic rollback via ACID transaction
        KoreTransactionCoordinator.abortBatch(path, messages);
    }
}

/**
 * KoreDataWriter - Writes partition data with ACID support
 */
public class KoreDataWriter implements DataWriter<Row> {
    private String partitionPath;
    private StructType schema;
    private int partitionId;
    private CaseInsensitiveStringMap options;
    private KoreFileWriter fileWriter;
    private long txnId;
    
    public KoreDataWriter(String partitionPath, StructType schema,
                         int partitionId, CaseInsensitiveStringMap options) {
        this.partitionPath = partitionPath;
        this.schema = schema;
        this.partitionId = partitionId;
        this.options = options;
        
        // Get ACID transaction ID from Kore
        this.txnId = KoreAcidManager.beginTransaction(partitionId);
        this.fileWriter = new KoreFileWriter(partitionPath, schema, txnId);
    }
    
    @Override
    public void write(Row row) throws IOException {
        fileWriter.write(row);
    }
    
    @Override
    public WriterCommitMessage commit() throws IOException {
        fileWriter.flush();
        
        // Commit in ACID transaction
        KoreAcidManager.commitTransaction(txnId);
        
        return new KoreWriterCommitMessage(partitionId, txnId);
    }
    
    @Override
    public void abort() throws IOException {
        if (fileWriter != null) {
            fileWriter.close();
        }
        
        // Rollback ACID transaction
        KoreAcidManager.rollbackTransaction(txnId);
    }
}

/**
 * KoreMetadataReader - Reads Kore file metadata
 */
public class KoreMetadataReader {
    public static StructType inferSchema(String path) {
        // Read schema from Kore manifest
        return KoreManifestReader.readSchema(path);
    }
    
    public static Partitioning[] getPartitions(String path, StructType schema) {
        // Extract partition info from Kore metadata
        return KoreManifestReader.readPartitions(path, schema);
    }
    
    public static String[] getPartitionPaths(String path) {
        // List all partition paths in Kore file
        return KoreManifestReader.listPartitionPaths(path);
    }
}

/**
 * KoreAcidManager - Manages ACID transactions for Spark writes
 */
public class KoreAcidManager {
    public static long beginTransaction(int partitionId) {
        // Get transaction ID from Kore's lock-free generator
        return KoreConcurrentTransactionManager.allocateTransactionId();
    }
    
    public static void commitTransaction(long txnId) {
        // Commit with read/write conflict detection
        KoreConflictResolver.commit(txnId);
    }
    
    public static void rollbackTransaction(long txnId) {
        // Rollback with WAL undo
        KoreTransactionRollback.rollback(txnId);
    }
}
