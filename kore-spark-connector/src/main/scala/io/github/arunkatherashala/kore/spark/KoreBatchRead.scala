package io.github.arunkatherashala.kore.spark

import org.apache.spark.sql.catalyst.InternalRow
import org.apache.spark.sql.connector.read.{Batch, InputPartition, PartitionReader, PartitionReaderFactory}
import org.apache.spark.sql.types._
import scala.io.Source
import java.io.File

/**
 * Batch Read implementation for Kore files
 * Handles reading Kore format files and converting to Spark rows
 */
class KoreBatch(
  path: String,
  schema: StructType
) extends Batch {

  override def planInputPartitions(): Array[InputPartition] = {
    // For simplicity, treat each file as one partition
    // In production, would split large files
    val file = new File(path)
    if (file.isDirectory) {
      file.listFiles().filter(_.getName.endsWith(".kore"))
        .map(f => KoreInputPartition(f.getAbsolutePath, schema))
    } else {
      Array(KoreInputPartition(path, schema))
    }
  }

  override def createReaderFactory(): PartitionReaderFactory = {
    KorePartitionReaderFactory(schema)
  }
}

/**
 * Input partition representing a Kore file
 */
case class KoreInputPartition(filePath: String, schema: StructType) extends InputPartition

/**
 * Factory for creating partition readers
 */
case class KorePartitionReaderFactory(schema: StructType) extends PartitionReaderFactory {
  
  override def createReader(partition: InputPartition): PartitionReader[InternalRow] = {
    val korePartition = partition.asInstanceOf[KoreInputPartition]
    new KorePartitionReader(korePartition.filePath, schema)
  }
}

/**
 * Partition reader for Kore files
 * Converts Kore file content into Spark InternalRow objects
 */
class KorePartitionReader(filePath: String, schema: StructType) extends PartitionReader[InternalRow] {
  
  private var currentRow: Option[InternalRow] = None
  private var fileSource: Source = _
  private var lineIterator: Iterator[String] = _
  private var hasStarted: Boolean = false
  
  private def initializeReader(): Unit = {
    if (!hasStarted) {
      try {
        fileSource = Source.fromFile(filePath)
        lineIterator = fileSource.getLines()
        hasStarted = true
      } catch {
        case e: Exception =>
          throw new RuntimeException(s"Failed to open Kore file: $filePath", e)
      }
    }
  }
  
  override def next(): Boolean = {
    initializeReader()
    
    if (lineIterator.hasNext) {
      val line = lineIterator.next()
      currentRow = Some(parseKoreLineToRow(line))
      true
    } else {
      false
    }
  }
  
  override def get(): InternalRow = {
    currentRow.getOrElse(throw new RuntimeException("No current row"))
  }
  
  override def close(): Unit = {
    if (fileSource != null) {
      fileSource.close()
    }
  }
  
  /**
   * Parse a Kore file line into a Spark InternalRow
   * For now, simple CSV-like parsing; in production would parse Kore binary format
   */
  private def parseKoreLineToRow(line: String): InternalRow = {
    val values = line.split(",")
    val convertedValues = schema.fields.zipWithIndex.map { case (field, idx) =>
      if (idx < values.length) {
        convertStringToType(values(idx), field.dataType)
      } else {
        null
      }
    }
    InternalRow(convertedValues: _*)
  }
  
  /**
   * Convert string value to appropriate Spark type
   */
  private def convertStringToType(value: String, dataType: DataType): Any = {
    if (value == null || value.isEmpty || value == "null") {
      null
    } else {
      try {
        dataType match {
          case ByteType => value.toByte
          case ShortType => value.toShort
          case IntegerType => value.toInt
          case LongType => value.toLong
          case FloatType => value.toFloat
          case DoubleType => value.toDouble
          case BooleanType => value.toBoolean
          case StringType => value
          case BinaryType => value.getBytes()
          case DateType => java.sql.Date.valueOf(value)
          case TimestampType => java.sql.Timestamp.valueOf(value)
          case _ => value
        }
      } catch {
        case _: Exception => null
      }
    }
  }
}
