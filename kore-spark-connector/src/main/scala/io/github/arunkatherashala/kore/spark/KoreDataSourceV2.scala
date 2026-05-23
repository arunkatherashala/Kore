package io.github.arunkatherashala.kore.spark

import org.apache.spark.sql.connector.catalog.{Table, TableProvider, SupportsRead, SupportsWrite}
import org.apache.spark.sql.connector.expressions.Transform
import org.apache.spark.sql.connector.read.ScanBuilder
import org.apache.spark.sql.connector.write.WriteBuilder
import org.apache.spark.sql.types.StructType
import org.apache.spark.sql.util.CaseInsensitiveStringMap
import scala.collection.JavaConverters._

/**
 * Kore DataSourceV2 Provider - Entry point for Spark integration
 * Handles Kore file format registration and table creation
 * Supports both BATCH_READ and BATCH_WRITE operations
 */
class KoreDataSourceProvider extends TableProvider {

  /**
   * Infer schema from Kore files
   * In a real implementation, this would read Kore metadata from file
   */
  override def inferSchema(options: CaseInsensitiveStringMap): StructType = {
    // For now, return empty schema - user must provide explicit schema
    // In production, would parse Kore header to extract metadata
    StructType(Seq())
  }

  /**
   * Get table for reading/writing Kore files
   */
  override def getTable(
    schema: StructType,
    partitioning: Array[Transform],
    properties: java.util.Map[String, String]
  ): Table = {
    new KoreTable(schema, properties.asScala.toMap)
  }

  /**
   * Support external metadata - user can provide schema
   */
  override def supportsExternalMetadata: Boolean = true
}

/**
 * Kore Table - Represents a Kore file as a Spark table
 * Implements both read and write capabilities
 */
class KoreTable(
  val schema: StructType,
  val properties: Map[String, String]
) extends Table with SupportsRead with SupportsWrite {

  override def name(): String = "kore"

  override def schema(): StructType = schema

  override def partitioning(): Array[Transform] = Array()

  override def properties(): java.util.Map[String, String] = {
    (properties ++ Map(
      "format" -> "kore",
      "version" -> "1.0.0"
    )).asJava
  }

  override def capabilities(): java.util.Set[org.apache.spark.sql.connector.catalog.TableCapability] = {
    import org.apache.spark.sql.connector.catalog.TableCapability._
    Set(BATCH_READ, BATCH_WRITE).asJava
  }

  /**
   * Build scan for read operations
   */
  override def newScanBuilder(options: CaseInsensitiveStringMap): ScanBuilder = {
    val path = options.get("path")
    if (path == null) {
      throw new IllegalArgumentException("'path' parameter required for reading Kore files")
    }
    new KoreScanBuilder(schema, options.asScala.toMap)
  }

  /**
   * Build write for write operations
   */
  override def newWriteBuilder(options: CaseInsensitiveStringMap): WriteBuilder = {
    val path = options.get("path")
    if (path == null) {
      throw new IllegalArgumentException("'path' parameter required for writing Kore files")
    }
    new KoreWriteBuilder(schema, options.asScala.toMap)
  }
}
