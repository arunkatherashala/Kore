package com.kore.spark

import org.apache.spark.sql.connector.read.Batch
import org.apache.spark.sql.connector.read.Scan
import org.apache.spark.sql.connector.read.Statistics
import org.apache.spark.sql.sources.Filter
import org.apache.spark.sql.types.StructType
import org.apache.spark.sql.util.CaseInsensitiveStringMap

/**
 * KoreScan: Represents a scan plan for Kore files with filter/projection pushdown
 */
class KoreScan(
    val path: String,
    val schema: StructType,
    val projection: Array[Array[Int]],
    val filters: Array[Filter],
    val options: CaseInsensitiveStringMap) extends Scan {

  override def readSchema(): StructType = {
    if (projection != null && projection.nonEmpty) {
      // Apply column projection
      val projectedColumns = projection.map(proj => schema.fields(proj(0)))
      new StructType(projectedColumns)
    } else {
      schema
    }
  }

  override def toBatch: Batch = {
    new KoreBatch(path, readSchema(), filters, options)
  }

  override def getStatistics: Statistics = {
    // TODO: Extract statistics from Kore file header
    new Statistics() {
      override def sizeInBytes(): Long = Long.MaxValue
      override def numRows(): Long = Long.MaxValue
    }
  }

  override def description(): String = {
    val filterStr = if (filters.nonEmpty) s", filters: ${filters.mkString("[", ", ", "]")}" else ""
    val projStr = if (projection.nonEmpty) s", projection: ${projection.length} columns" else ""
    s"KoreScan($path$filterStr$projStr)"
  }
}
