package io.github.arunkatherashala.kore.spark

import org.apache.spark.sql.connector.read.{ScanBuilder, Scan}
import org.apache.spark.sql.connector.write.{WriteBuilder, Write, WriterCommitMessage}
import org.apache.spark.sql.types.StructType
import org.apache.spark.sql.util.CaseInsensitiveStringMap

/**
 * ScanBuilder for Kore read operations
 * Builds a Scan object for reading Kore files
 */
class KoreScanBuilder(
  schema: StructType,
  properties: Map[String, String]
) extends ScanBuilder {
  
  override def build(): Scan = {
    val path = properties.getOrElse(
      "path",
      throw new IllegalArgumentException("path property required for Kore read")
    )
    
    new KoreReadScan(path, schema, properties)
  }
}

/**
 * Scan implementation for Kore files
 * Represents the read operation and produces InputPartitions
 */
class KoreReadScan(
  path: String,
  schema: StructType,
  properties: Map[String, String]
) extends Scan {
  
  override def readSchema(): StructType = schema
  
  override def toBatch(): org.apache.spark.sql.connector.read.Batch = {
    new KoreBatch(path, schema)
  }
  
  override def description(): String = {
    s"KoreRead(path=$path, schema=${schema.fields.map(_.name).mkString("[", ",", "]")})"
  }
  
  override def toMessage: String = description()
}

/**
 * WriteBuilder for Kore write operations
 * Builds a Write object for writing Spark data to Kore format
 */
class KoreWriteBuilder(
  schema: StructType,
  properties: Map[String, String]
) extends WriteBuilder {
  
  override def build(): Write = {
    val path = properties.getOrElse(
      "path",
      throw new IllegalArgumentException("path property required for Kore write")
    )
    
    new KoreWrite(path, schema, properties)
  }
}

/**
 * Write implementation for Kore files
 * Represents the write operation and produces DataWriters
 */
class KoreWrite(
  path: String,
  schema: StructType,
  properties: Map[String, String]
) extends Write {
  
  override def toBatch(): org.apache.spark.sql.connector.write.BatchWrite = {
    new KoreBatchWrite(path, schema)
  }
  
  override def description(): String = {
    s"KoreWrite(path=$path, schema=${schema.fields.map(_.name).mkString("[", ",", "]")})"
  }
}
