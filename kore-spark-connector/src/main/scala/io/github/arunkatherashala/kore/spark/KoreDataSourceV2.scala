package io.github.arunkatherashala.kore.spark

import org.apache.spark.sql.connector.catalog.{Table, TableProvider}
import org.apache.spark.sql.connector.expressions.Transform
import org.apache.spark.sql.types.StructType
import org.apache.spark.sql.util.CaseInsensitiveStringMap
import scala.collection.JavaConverters._

/**
 * Kore DataSourceV2 Provider - Entry point for Spark integration
 * Handles Kore file format registration and table creation
 */
class KoreDataSourceProvider extends TableProvider {

  /**
   * Infer schema from Kore files
   * In a real implementation, this would read Kore metadata
   */
  override def inferSchema(options: CaseInsensitiveStringMap): StructType = {
    StructType(Seq())  // Placeholder - would read actual schema
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
   * Check if this provider can support the given schema
   */
  override def supportsExternalMetadata: Boolean = true
}

/**
 * Kore Table - Represents a Kore file as a Spark table
 */
class KoreTable(
  val schema: StructType,
  val properties: Map[String, String]
) extends Table {

  override def name(): String = "kore"

  override def schema(): StructType = schema

  override def partitioning(): Array[Transform] = Array()

  override def properties(): java.util.Map[String, String] = {
    import scala.collection.JavaConverters._
    (properties ++ Map(
      "format" -> "kore",
      "version" -> "0.1.0"
    )).asJava
  }

  override def capabilities(): java.util.Set[org.apache.spark.sql.connector.catalog.TableCapability] = {
    import scala.collection.JavaConverters._
    Seq(
      org.apache.spark.sql.connector.catalog.TableCapability.BATCH_READ,
      org.apache.spark.sql.connector.catalog.TableCapability.BATCH_WRITE
    ).asJava.asInstanceOf[java.util.Set[org.apache.spark.sql.connector.catalog.TableCapability]]
  }
}

// DataSourceV2 marker for Spark
class KoreScan extends org.apache.spark.sql.connector.read.Scan {
  override def toMessage: String = "KoreScan"
}
