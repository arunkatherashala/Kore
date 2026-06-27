package io.github.arunkatherashala.kore.spark

import org.apache.spark.sql.{DataFrame, SparkSession}
import org.apache.spark.sql.types._

/**
 * Example usage of Kore Spark Connector
 * Demonstrates reading and writing Kore format files using Spark SQL
 */
object KoreSparkExample {
  
  def main(args: Array[String]): Unit = {
    val spark = SparkSession.builder()
      .appName("KoreSparkConnectorExample")
      .master("local[*]")
      .config("spark.sql.extensions", "io.github.arunkatherashala.kore.spark.KoreDataSourceProvider")
      .getOrCreate()
    
    println("=== Kore Spark Connector Example ===\n")
    
    // Example 1: Create a sample DataFrame
    println("1. Creating sample DataFrame...")
    val sampleDF = createSampleDataFrame(spark)
    sampleDF.show()
    println(s"Schema: ${sampleDF.schema}")
    
    // Example 2: Write DataFrame to Kore format
    println("\n2. Writing DataFrame to Kore format...")
    val outputPath = "/tmp/kore_output"
    writeToKore(spark, sampleDF, outputPath)
    
    // Example 3: Read from Kore format
    println("\n3. Reading from Kore format...")
    val readDF = readFromKore(spark, outputPath, sampleDF.schema)
    readDF.show()
    
    // Example 4: Demonstrate partitioning
    println("\n4. Reading with partitioned load...")
    readDF.repartition(2).write
      .format("kore")
      .mode("overwrite")
      .option("path", s"$outputPath/partitioned")
      .save()
    
    println("\n✓ Kore Spark Connector operations completed successfully!")
    spark.stop()
  }
  
  /**
   * Create a sample DataFrame with various data types
   */
  def createSampleDataFrame(spark: SparkSession): DataFrame = {
    import spark.implicits._
    
    val data = Seq(
      (1, "Alice", 25, 1000.50, true),
      (2, "Bob", 30, 2500.75, false),
      (3, "Charlie", 35, 3500.25, true),
      (4, "Diana", 28, 1800.00, false),
      (5, "Eve", 32, 4200.99, true)
    )
    
    val schema = StructType(Seq(
      StructField("id", IntegerType, false),
      StructField("name", StringType, false),
      StructField("age", IntegerType, false),
      StructField("salary", DoubleType, false),
      StructField("active", BooleanType, false)
    ))
    
    data.toDF("id", "name", "age", "salary", "active")
  }
  
  /**
   * Write DataFrame to Kore format
   */
  def writeToKore(spark: SparkSession, df: DataFrame, path: String): Unit = {
    df.write
      .format("kore")
      .mode("overwrite")
      .option("path", path)
      .save()
    println(s"✓ Successfully wrote ${df.count()} rows to Kore format at $path")
  }
  
  /**
   * Read DataFrame from Kore format
   */
  def readFromKore(spark: SparkSession, path: String, schema: StructType): DataFrame = {
    spark.read
      .format("kore")
      .schema(schema)
      .option("path", path)
      .load()
  }
  
  /**
   * Example: Compress and partition during write
   */
  def writeWithCompression(
    spark: SparkSession,
    df: DataFrame,
    path: String,
    compression: String = "hybrid"
  ): Unit = {
    df.repartition(4)
      .write
      .format("kore")
      .mode("overwrite")
      .option("path", path)
      .option("compression", compression)
      .option("compressionLevel", "9")
      .save()
    println(s"✓ Wrote ${df.count()} rows to Kore with $compression compression")
  }
  
  /**
   * Example: Filter and transform while reading
   */
  def readAndTransform(spark: SparkSession, path: String, schema: StructType): DataFrame = {
    spark.read
      .format("kore")
      .schema(schema)
      .option("path", path)
      .load()
      .filter("age > 25")
      .select("name", "salary")
  }
}
