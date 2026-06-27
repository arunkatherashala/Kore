package com.kore.spark

import org.apache.spark.sql.connector.catalog.SupportsRead
import org.apache.spark.sql.connector.catalog.Table
import org.apache.spark.sql.connector.catalog.TableCapability
import org.apache.spark.sql.connector.expressions.Transform
import org.apache.spark.sql.connector.read.ScanBuilder
import org.apache.spark.sql.types.StructType
import org.apache.spark.sql.util.CaseInsensitiveStringMap

import java.util

/**
 * KoreTable: Represents a Kore file as a Spark SQL table
 */
class KoreTable(
    val path: String,
    val options: CaseInsensitiveStringMap,
    val transforms: Array[Transform]) extends Table with SupportsRead {

  override def name(): String = s"kore_${path.hashCode}"

  override def schema(): StructType = {
    // TODO: Infer schema from Kore file header
    new StructType()
  }

  override def capabilities(): util.Set[TableCapability] = {
    util.EnumSet.of(
      TableCapability.BATCH_READ,
      TableCapability.ACCEPT_ANY_SCHEMA
    )
  }

  override def newScanBuilder(options: CaseInsensitiveStringMap): ScanBuilder = {
    new KoreScanBuilder(path, schema(), options)
  }

  override def toString: String = s"KoreTable($path)"
}
