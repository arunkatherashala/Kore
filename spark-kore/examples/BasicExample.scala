package org.kore.spark.examples

import org.apache.spark.sql.SparkSession

/**
 * Example 1: Basic Read and Write
 */
object BasicExample {
  def main(args: Array[String]): Unit = {
    val spark = SparkSession.builder()
      .appName("Kore Basic Example")
      .master("local[*]")
      .getOrCreate()
    
    import spark.implicits._
    
    println("=" * 80)
    println("EXAMPLE 1: Basic Kore Read/Write")
    println("=" * 80)
    
    // Create sample data
    val data = Seq(
      (1L, "Alice", 25, 5000.0),
      (2L, "Bob", 30, 6000.0),
      (3L, "Charlie", 35, 7000.0),
      (4L, "Diana", 28, 5500.0),
      (5L, "Eve", 32, 7500.0)
    )
    
    val df = data.toDF("id", "name", "age", "salary")
    
    println("\nOriginal DataFrame:")
    df.show()
    
    // Write to Kore format
    println("\nWriting to Kore format...")
    df.write
      .format("kore")
      .mode("overwrite")
      .save("/tmp/example1.kore")
    
    println("✓ Written to /tmp/example1.kore")
    
    // Read back
    println("\nReading from Kore format...")
    val koreDF = spark.read
      .format("kore")
      .load("/tmp/example1.kore")
    
    println("✓ Read from Kore file")
    koreDF.show()
    
    spark.stop()
  }
}
