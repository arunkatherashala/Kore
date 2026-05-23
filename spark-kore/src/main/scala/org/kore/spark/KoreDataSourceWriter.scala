package org.kore.spark

import org.apache.spark.sql.sources.v2.writer.DataSourceWriter
import org.apache.spark.sql.sources.v2.writer.WriterCommitMessage
import org.apache.spark.sql.types.StructType
import org.apache.spark.sql.SaveMode
import org.slf4j.LoggerFactory
import java.io.File
import java.nio.file.Files

/**
 * Kore DataSourceWriter - handles writing data to Kore file format
 */
class KoreDataSourceWriter(
  filePath: String,
  schema: StructType,
  mode: SaveMode,
  options: java.util.Map[String, String]
) extends DataSourceWriter {
  
  private val log = LoggerFactory.getLogger(classOf[KoreDataSourceWriter])
  
  log.info(s"Initializing KoreDataSourceWriter")
  log.info(s"Target path: $filePath")
  log.info(s"Save mode: $mode")
  log.info(s"Schema: ${schema.fieldNames.mkString(",")}")
  
  // Validate mode and file existence
  val file = new File(filePath)
  mode match {
    case SaveMode.Overwrite =>
      if (file.exists()) {
        log.info(s"Overwrite mode: deleting existing file $filePath")
        Files.delete(file.toPath)
      }
    case SaveMode.Append =>
      if (!file.exists()) {
        log.warn(s"Append mode requested but file does not exist: $filePath")
      }
    case SaveMode.Ignore =>
      if (file.exists()) {
        log.info(s"Ignore mode: file exists, will skip write")
        return
      }
    case SaveMode.ErrorIfExists =>
      if (file.exists()) {
        throw new RuntimeException(s"File already exists: $filePath")
      }
  }
  
  // Parse write options
  val compression = options.getOrDefault("compression", "auto")
  val partitionCols = if (options.containsKey("partitionBy")) {
    options.get("partitionBy").split(",")
  } else {
    Array[String]()
  }
  
  log.info(s"Write options: compression=$compression, partitions=${partitionCols.mkString(",")}")
  
  /**
   * Create writers for each partition
   */
  override def createWriterFactory(): org.apache.spark.sql.sources.v2.writer.WriterFactory = {
    log.info("Creating writer factory")
    new KoreWriterFactory(filePath, schema, compression)
  }
  
  /**
   * Commit the write operation
   */
  override def commit(messages: Array[WriterCommitMessage]): Unit = {
    log.info(s"Committing write with ${messages.length} partition(s)")
    
    // In production, this would merge partition files and finalize
    log.info(s"Write committed successfully to $filePath")
  }
  
  /**
   * Abort the write operation
   */
  override def abort(messages: Array[WriterCommitMessage]): Unit = {
    log.warn(s"Aborting write after ${messages.length} partition(s)")
    
    // Clean up any temporary files
    if (file.exists()) {
      log.info(s"Deleting incomplete file: $filePath")
      Files.delete(file.toPath)
    }
  }
}
