package io.github.arunkatherashala.kore.spark

import org.apache.spark.sql.types._

/**
 * Kore Spark Integration Utilities
 * Maps between Kore data types and Spark types
 */
object KoreSparkTypes {

  /**
   * Map Spark DataType to Kore type string
   */
  def sparkTypeToKore(sparkType: DataType): String = {
    sparkType match {
      case ByteType => "i8"
      case ShortType => "i16"
      case IntegerType => "i32"
      case LongType => "i64"
      case FloatType => "f32"
      case DoubleType => "f64"
      case BooleanType => "bool"
      case StringType => "string"
      case BinaryType => "binary"
      case DateType => "date"
      case TimestampType => "timestamp"
      case _ => "unknown"
    }
  }

  /**
   * Map Kore type string to Spark DataType
   */
  def koreTypeToSpark(koreType: String): DataType = {
    koreType match {
      case "i8" => ByteType
      case "i16" => ShortType
      case "i32" => IntegerType
      case "i64" => LongType
      case "f32" => FloatType
      case "f64" => DoubleType
      case "bool" => BooleanType
      case "string" => StringType
      case "binary" => BinaryType
      case "date" => DateType
      case "timestamp" => TimestampType
      case _ => StringType  // Default fallback
    }
  }

  /**
   * Estimate compression ratio for type
   */
  def estimateCompressionRatio(dataType: DataType, rowCount: Long): Double = {
    dataType match {
      case ByteType | BooleanType => 0.8  // Highly compressible
      case ShortType | IntegerType => 0.75
      case LongType | DateType => 0.70
      case FloatType | DoubleType => 0.65
      case StringType => 0.40  // Variable compression
      case _ => 0.50
    }
  }
}

/**
 * Filter push-down support
 */
object FilterPushdown {

  /**
   * Supported filter types for Kore
   */
  val SUPPORTED_FILTERS = Set(
    "EqualTo",
    "Not",
    "GreaterThan",
    "GreaterThanOrEqual",
    "LessThan",
    "LessThanOrEqual",
    "In",
    "IsNull",
    "IsNotNull",
    "StringContains",
    "StringStartsWith",
    "StringEndsWith",
    "And",
    "Or"
  )

  /**
   * Check if filter type is supported
   */
  def isFilterSupported(filterType: String): Boolean = {
    SUPPORTED_FILTERS.contains(filterType)
  }

  /**
   * Estimate selectivity of filter
   */
  def estimateSelectivity(filterType: String): Double = {
    filterType match {
      case "EqualTo" => 0.01
      case "In" => 0.05
      case "StringContains" => 0.10
      case "GreaterThan" | "LessThan" => 0.50
      case "And" => 0.005
      case "Or" => 0.20
      case _ => 0.50
    }
  }
}

/**
 * Kore Spark Connector Configuration
 */
case class KoreConnectorConfig(
  filePath: String,
  compression: String = "hybrid",
  cacheMetadata: Boolean = true,
  enableFilterPushdown: Boolean = true,
  enableColumnPruning: Boolean = true,
  parallelism: Int = 4
)
