package com.kore.spark

import org.apache.spark.sql.connector.read.Batch
import org.apache.spark.sql.connector.read.InputPartition
import org.apache.spark.sql.connector.read.PartitionReader
import org.apache.spark.sql.connector.read.PartitionReaderFactory
import org.apache.spark.sql.sources.Filter
import org.apache.spark.sql.types.StructType
import org.apache.spark.sql.util.CaseInsensitiveStringMap

/**
 * KoreBatch: Executes a batch read from Kore files
 */
class KoreBatch(
    val path: String,
    val schema: StructType,
    val filters: Array[Filter],
    val options: CaseInsensitiveStringMap) extends Batch {

  override def planInputPartitions(): Array[InputPartition] = {
    // For now, treat entire file as single partition
    Array(new KoreInputPartition(path, schema, filters))
  }

  override def createReaderFactory(): PartitionReaderFactory = {
    new PartitionReaderFactory {
      override def createReader(partition: InputPartition): PartitionReader[org.apache.spark.sql.catalyst.InternalRow] = {
        val korePartition = partition.asInstanceOf[KoreInputPartition]
        new KorePartitionReader(korePartition.path, korePartition.schema, korePartition.filters)
      }
    }
  }
}

/**
 * Input partition representing a Kore file
 */
case class KoreInputPartition(
    path: String,
    schema: StructType,
    filters: Array[Filter]) extends InputPartition
