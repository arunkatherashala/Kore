package com.kore.spark

import org.apache.spark.sql.connector.catalog.Table
import org.apache.spark.sql.connector.catalog.TableProvider
import org.apache.spark.sql.connector.expressions.Transform
import org.apache.spark.sql.types.StructType
import org.apache.spark.sql.util.CaseInsensitiveStringMap

/**
 * KoreDataSource: Spark SQL connector for Kore file format
 * 
 * Enables reading Kore files using Spark SQL:
 * df = spark.read.format("kore").load("/path/to/file.kore")
 */
class KoreDataSource extends TableProvider {

  override def inferSchema(options: CaseInsensitiveStringMap): StructType = {
    // TODO: Infer schema from Kore file
    new StructType()
  }

  override def getTable(
      schema: StructType,
      partitioning: Array[Transform],
      properties: java.util.Map[String, String]): Table = {
    
    val opts = new CaseInsensitiveStringMap(properties)
    val path = opts.get("path")
    if (path == null || path.isEmpty) {
      throw new IllegalArgumentException("'path' option is required for Kore DataSource")
    }
    
    new KoreTable(path, opts, partitioning)
  }
}
