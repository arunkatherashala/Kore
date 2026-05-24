package com.kore.spark

import java.io.RandomAccessFile
import org.apache.spark.sql.connector.catalog.Table
import org.apache.spark.sql.connector.catalog.TableProvider
import org.apache.spark.sql.connector.expressions.Transform
import org.apache.spark.sql.types.{StructType, StructField, LongType, DoubleType, StringType, BooleanType, IntegerType, DataType}
import org.apache.spark.sql.util.CaseInsensitiveStringMap

/**
 * KoreDataSource: Spark SQL connector for Kore file format
 * 
 * Enables reading Kore files using Spark SQL:
 * df = spark.read.format("kore").load("/path/to/file.kore")
 */
class KoreDataSource extends TableProvider {

  override def inferSchema(options: CaseInsensitiveStringMap): StructType = {
    val path = options.get("path")
    if (path == null || path.isEmpty) {
      // Return default schema if no path provided
      return new StructType()
    }

    try {
      inferSchemaFromFile(path)
    } catch {
      case e: Exception =>
        // Fall back to default schema if inference fails
        System.err.println(s"Warning: Could not infer schema from $path: ${e.getMessage}")
        new StructType()
    }
  }

  /**
   * Infer schema from Kore file header and column metadata
   */
  private def inferSchemaFromFile(path: String): StructType = {
    val file = new RandomAccessFile(path, "r")
    try {
      // Read header
      val headerBytes = new Array[Byte](16)
      file.read(headerBytes)

      // Validate magic bytes
      val magic = new String(headerBytes.slice(0, 4))
      if (magic != "KORE") {
        throw new Exception("Invalid Kore file: bad magic bytes")
      }

      val version = headerBytes(4) & 0xFF
      val flags = headerBytes(5) & 0xFF
      val columnCount = headerBytes(6) & 0xFF

      // Read column metadata
      val fields = new scala.collection.mutable.ArrayBuffer[StructField]()
      
      for (i <- 0 until columnCount) {
        val (name, sparkType) = readColumnMetadataForSchema(file)
        fields += StructField(name, sparkType, nullable = true)
      }

      new StructType(fields.toArray)
    } finally {
      file.close()
    }
  }

  /**
   * Read column metadata and return (name, SparkType)
   */
  private def readColumnMetadataForSchema(file: RandomAccessFile): (String, DataType) = {
    // Read name (variable length)
    val nameLen = readVarInt(file)
    val nameBytes = new Array[Byte](nameLen)
    file.read(nameBytes)
    val name = new String(nameBytes, "UTF-8")

    // Read type (1 byte)
    val typeVal = file.readByte()

    // Read compression codec (1 byte) - skip for schema inference
    val codec = file.readByte()

    // Read row count (varint) - skip for schema inference
    val rowCount = readVarInt(file)

    // Read data offset (varint) - skip for schema inference
    val dataOffset = readVarInt(file)

    // Map Kore type to Spark type
    val sparkType = typeVal match {
      case 0 => LongType       // i64
      case 1 => DoubleType     // f64
      case 2 => StringType     // string
      case 3 => BooleanType    // bool
      case 4 => StringType     // bytes (represented as string)
      case _ => StringType     // default to string for unknown types
    }

    (name, sparkType)
  }

  /**
   * Read variable-length integer (LEB128 encoding)
   */
  private def readVarInt(file: RandomAccessFile): Int = {
    var result = 0
    var shift = 0
    var b = 0

    do {
      b = file.readByte() & 0xFF
      result |= (b & 0x7F) << shift
      shift += 7
    } while ((b & 0x80) != 0)

    result
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
