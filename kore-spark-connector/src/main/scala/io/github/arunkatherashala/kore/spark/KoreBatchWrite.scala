package io.github.arunkatherashala.kore.spark

import org.apache.spark.sql.catalyst.InternalRow
import org.apache.spark.sql.connector.write.{BatchWrite, DataWriter, DataWriterFactory, PhysicalWriteInfo, WriterCommitMessage}
import org.apache.spark.sql.types._
import java.io.{File, FileWriter, BufferedWriter}
import java.util.UUID

/**
 * Batch Write implementation for Kore files
 * Handles writing Spark DataFrame partitions to Kore format
 */
class KoreBatchWrite(
  path: String,
  schema: StructType
) extends BatchWrite {
  
  override def createBatchWriterFactory(physicalWriteInfo: PhysicalWriteInfo): DataWriterFactory = {
    KoreDataWriterFactory(path, schema)
  }
  
  override def commit(messages: Array[WriterCommitMessage]): Unit = {
    // In production, would merge partial files and validate checksums
    println(s"[KoreBatchWrite] Committed ${messages.length} partitions to $path")
  }
  
  override def abort(messages: Array[WriterCommitMessage]): Unit = {
    // Clean up partial files on failure
    val outputDir = new File(path)
    if (outputDir.exists() && outputDir.isDirectory) {
      outputDir.listFiles().filter(f => 
        f.getName.endsWith(".tmp") || f.getName.startsWith("part-")
      ).foreach(_.delete())
    }
  }
}

/**
 * Factory for creating data writers for each partition
 */
case class KoreDataWriterFactory(
  path: String,
  schema: StructType
) extends DataWriterFactory {
  
  override def createWriter(partitionId: Int, taskId: Long): DataWriter[InternalRow] = {
    new KoreDataWriter(path, partitionId, taskId, schema)
  }
}

/**
 * Data writer for individual partitions
 * Converts Spark InternalRow objects to Kore format
 */
class KoreDataWriter(
  basePath: String,
  partitionId: Int,
  taskId: Long,
  schema: StructType
) extends DataWriter[InternalRow] {
  
  private val outputDir = new File(basePath)
  if (!outputDir.exists()) {
    outputDir.mkdirs()
  }
  
  private val outputFile = new File(
    outputDir,
    s"part-${partitionId}-${taskId}-${UUID.randomUUID().toString.take(8)}.kore"
  )
  
  private val fileWriter = new BufferedWriter(new FileWriter(outputFile))
  private var rowCount: Long = 0
  
  override def write(row: InternalRow): Unit = {
    try {
      // Convert row to Kore format
      val koreData = rowToKoreFormat(row)
      fileWriter.write(koreData)
      fileWriter.write("\n")
      rowCount += 1
    } catch {
      case e: Exception =>
        throw new RuntimeException(s"Failed to write row to Kore file: ${e.getMessage}", e)
    }
  }
  
  override def commit(): WriterCommitMessage = {
    try {
      fileWriter.flush()
      fileWriter.close()
      KoreWriteCommitMessage(
        filePath = outputFile.getAbsolutePath,
        rowCount = rowCount,
        fileSize = outputFile.length()
      )
    } catch {
      case e: Exception =>
        throw new RuntimeException(s"Failed to commit Kore write: ${e.getMessage}", e)
    }
  }
  
  override def abort(): Unit = {
    try {
      fileWriter.close()
      if (outputFile.exists()) {
        outputFile.delete()
      }
    } catch {
      case e: Exception =>
        println(s"[KoreDataWriter] Failed to abort write: ${e.getMessage}")
    }
  }
  
  /**
   * Convert Spark InternalRow to Kore format string
   * Uses CSV-like format; in production would use binary Kore encoding
   */
  private def rowToKoreFormat(row: InternalRow): String = {
    schema.fields.zipWithIndex.map { case (field, idx) =>
      val value = row.get(idx, field.dataType)
      formatValueForKore(value, field.dataType)
    }.mkString(",")
  }
  
  /**
   * Format value according to Kore type specifications
   */
  private def formatValueForKore(value: Any, dataType: DataType): String = {
    if (value == null) {
      "null"
    } else {
      dataType match {
        case ByteType | ShortType | IntegerType | LongType => value.toString
        case FloatType | DoubleType => value.toString
        case BooleanType => value.toString
        case StringType => s""""${value.toString.replace("\"", "\\\"")}""""
        case BinaryType => value.asInstanceOf[Array[Byte]].map("%02x".format(_)).mkString
        case DateType => value.toString
        case TimestampType => value.toString
        case _ => value.toString
      }
    }
  }
}

/**
 * Commit message containing write metadata
 */
case class KoreWriteCommitMessage(
  filePath: String,
  rowCount: Long,
  fileSize: Long
) extends WriterCommitMessage
