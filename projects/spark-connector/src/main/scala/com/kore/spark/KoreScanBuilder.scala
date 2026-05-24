package com.kore.spark

import org.apache.spark.sql.connector.read.Scan
import org.apache.spark.sql.connector.read.ScanBuilder
import org.apache.spark.sql.connector.read.SupportsPushDownFilters
import org.apache.spark.sql.connector.read.SupportsPushDownRequiredColumns
import org.apache.spark.sql.sources.Filter
import org.apache.spark.sql.types.StructType
import org.apache.spark.sql.util.CaseInsensitiveStringMap

/**
 * KoreScanBuilder: Builds a scan plan with filter pushdown and projection pushdown
 */
class KoreScanBuilder(
    val path: String,
    val schema: StructType,
    val options: CaseInsensitiveStringMap) 
    extends ScanBuilder 
    with SupportsPushDownFilters 
    with SupportsPushDownRequiredColumns {

  private var filters: Array[Filter] = Array()
  private var projection: StructType = schema

  /**
   * Push column projection to reduce I/O
   */
  override def pruneColumns(requiredSchema: StructType): Unit = {
    this.projection = requiredSchema
  }

  /**
   * Push filters for 13 different filter types:
   * - EqualTo, LessThan, LessThanOrEqual, GreaterThan, GreaterThanOrEqual
   * - In, IsNull, IsNotNull
   * - StringStartsWith, StringEndsWith, StringContains
   * - And, Or, Not
   */
  override def pushFilters(filters: Array[Filter]): Array[Filter] = {
    val (pushable, unpushable) = filters.partition(isPushable)
    this.filters = pushable
    unpushable
  }

  override def pushedFilters(): Array[Filter] = this.filters

  /**
   * Determine if a filter is pushable to Kore
   */
  private def isPushable(filter: Filter): Boolean = {
    filter match {
      case _: org.apache.spark.sql.sources.EqualTo => true
      case _: org.apache.spark.sql.sources.LessThan => true
      case _: org.apache.spark.sql.sources.LessThanOrEqual => true
      case _: org.apache.spark.sql.sources.GreaterThan => true
      case _: org.apache.spark.sql.sources.GreaterThanOrEqual => true
      case _: org.apache.spark.sql.sources.In => true
      case _: org.apache.spark.sql.sources.IsNull => true
      case _: org.apache.spark.sql.sources.IsNotNull => true
      case _: org.apache.spark.sql.sources.StringStartsWith => true
      case _: org.apache.spark.sql.sources.StringEndsWith => true
      case _: org.apache.spark.sql.sources.StringContains => true
      case _: org.apache.spark.sql.sources.And => true
      case _: org.apache.spark.sql.sources.Or => true
      case _: org.apache.spark.sql.sources.Not => true
      case _ => false
    }
  }

  override def build(): Scan = {
    new KoreScan(path, projection, Array(), filters, options)
  }
}
