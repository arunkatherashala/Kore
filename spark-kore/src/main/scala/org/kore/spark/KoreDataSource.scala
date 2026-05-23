package org.kore.spark

import org.apache.spark.sql.sources.v2.DataSourceV2
import org.apache.spark.sql.sources.v2.reader.DataSourceReader
import org.apache.spark.sql.sources.v2.writer.DataSourceWriter
import org.apache.spark.sql.types.StructType
import org.slf4j.LoggerFactory

/**
 * Spark DataSource V2 implementation for Kore file format
 * 
 * Usage:
 *   spark.read.format("kore").load("path/to/file.kore")
 *   spark.write.format("kore").save("output.kore")
 */
class KoreDataSource extends DataSourceV2 {
  private val log = LoggerFactory.getLogger(classOf[KoreDataSource])

  override def shortName(): String = "kore"

  override def createReader(options: java.util.Map[String, String]): DataSourceReader = {
    log.info("Creating Kore DataSourceReader")
    val filePath = options.get("path")
    if (filePath == null) {
      throw new IllegalArgumentException("Path parameter is required")
    }
    new KoreDataSourceReader(filePath, options)
  }

  override def createWriter(
    jobId: String,
    schema: StructType,
    mode: org.apache.spark.sql.SaveMode,
    options: java.util.Map[String, String]
  ): java.util.Optional[DataSourceWriter] = {
    log.info(s"Creating Kore DataSourceWriter with mode: $mode")
    val filePath = options.get("path")
    if (filePath == null) {
      throw new IllegalArgumentException("Path parameter is required")
    }
    java.util.Optional.of(new KoreDataSourceWriter(filePath, schema, mode, options))
  }
}
