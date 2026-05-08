package io.kore.spark

import org.apache.spark.sql.connector.catalog.{Table, TableProvider}
import org.apache.spark.sql.connector.expressions.Transform
import org.apache.spark.sql.sources.DataSourceRegister
import org.apache.spark.sql.types.{StructType, StructField, StringType}
import org.apache.spark.sql.util.CaseInsensitiveStringMap
import scala.collection.JavaConverters._
import java.nio.ByteBuffer

/**
 * Kore DataSourceV2 Provider
 * 
 * Enables: spark.read.format("kore").load("file.kore")
 *          df.write.format("kore").save("output.kore")
 */
class KoreDataSource extends TableProvider with DataSourceRegister {

  override def shortName(): String = "kore"

  override def inferSchema(options: CaseInsensitiveStringMap): StructType = {
    val path = options.get("path")
    if (path == null || path.isEmpty) {
      throw new RuntimeException("path option required for schema inference")
    }
    readKoreSchema(path)
  }

  override def getTable(
      schema: StructType,
      partitioning: Array[Transform],
      properties: java.util.Map[String, String]): Table = {
    val path = properties.get("path")
    if (path == null || path.isEmpty) {
      throw new RuntimeException("path parameter required")
    }
    new KoreTable(path, schema, new CaseInsensitiveStringMap(properties))
  }

  /**
   * Read Kore file header to infer schema
   */
  private def readKoreSchema(path: String): StructType = {
    val file = new java.io.File(path)
    if (!file.exists()) {
      throw new RuntimeException(s"File not found: $path")
    }

    val input = new java.io.FileInputStream(file)
    val header = new Array[Byte](16)
    input.read(header, 0, 16)
    input.close()

    // Validate magic bytes "KORE"
    if (header(0) != 'K' || header(1) != 'O' || header(2) != 'R' || header(3) != 'E') {
      throw new RuntimeException("Invalid Kore file format")
    }

    // Parse header: magic(4) + version(1) + reserved(1) + ncols(2) + nrows(8)
    val version = header(4)
    val numCols = ((header(6) & 0xFF) | ((header(7) & 0xFF) << 8))
    val numRows = ByteBuffer.wrap(header, 8, 8).getLong()

    // Build schema with generic String columns
    // TODO: Parse actual column names and types from metadata section
    val fields = (0 until numCols).map { i =>
      StructField(s"col_$i", StringType, nullable = true)
    }.toArray

    StructType(fields)
  }
  }
}
