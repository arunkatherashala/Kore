package com.kore.spark

import org.apache.spark.sql.connector.catalog.Table
import org.apache.spark.sql.connector.expressions.Transform
import org.apache.spark.sql.connector.read.ScanBuilder
import org.apache.spark.sql.sources.DataSourceRegister
import org.apache.spark.sql.types.StructType
import org.apache.spark.sql.util.CaseInsensitiveStringMap

/**
 * KoreDataSource: Spark SQL connector for Kore file format
 * 
 * Enables reading Kore files using Spark SQL:
 * df = spark.read.format("kore").load("/path/to/file.kore")
 */
class KoreDataSource extends DataSourceRegister {

  override def shortName(): String = "kore"

  override def getTable(
      options: CaseInsensitiveStringMap,
      transforms: Array[Transform]): Table = {
    
    val path = options.get("path")
    if (path == null || path.isEmpty) {
      throw new IllegalArgumentException("'path' option is required for Kore DataSource")
    }
    
    new KoreTable(path, options, transforms)
  }
}
