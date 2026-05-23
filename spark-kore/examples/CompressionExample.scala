package org.kore.spark.examples

import org.apache.spark.sql.SparkSession
import java.io.File

/**
 * Example 3: Compression Options
 * 
 * Demonstrates different compression codecs available in Kore
 */
object CompressionExample {
  def main(args: Array[String]): Unit = {
    val spark = SparkSession.builder()
      .appName("Kore Compression Example")
      .master("local[*]")
      .getOrCreate()
    
    import spark.implicits._
    
    println("=" * 80)
    println("EXAMPLE 3: Compression Options")
    println("=" * 80)
    
    // Create sample data with different patterns
    val data = (1 to 10000).map { i =>
      val status = if (i % 100 < 60) "active" else if (i % 100 < 90) "inactive" else "deleted"
      val category = (i % 10).toString
      (i.toLong, s"Item$i", status, category, i * 1.5, i.toString)
    }
    
    val df = data.toDF("id", "name", "status", "category", "price", "description")
    
    println("\nOriginal data:")
    println(s"Rows: 10,000")
    df.printSchema()
    df.show(3)
    
    // Test different compression codecs
    val codecs = Seq("none", "rle", "dictionary", "for", "lzss", "auto")
    
    println("\n" + "=" * 80)
    println("COMPRESSION COMPARISON")
    println("=" * 80)
    println(f"${"Codec":<15} ${"File Size":<15} ${"Ratio":<10} ${"Time (ms)":<10}")
    println("-" * 80)
    
    val originalSize = new File("/tmp/example3_none.kore").length()
    
    for (codec <- codecs) {
      val path = s"/tmp/example3_$codec.kore"
      
      // Write with specific compression
      val t1 = System.currentTimeMillis()
      df.write
        .format("kore")
        .option("compression", codec)
        .mode("overwrite")
        .save(path)
      val elapsed = System.currentTimeMillis() - t1
      
      // Get file size
      val file = new File(path)
      val fileSize = file.length()
      val ratio = if (originalSize > 0) 
        ((fileSize.toDouble / originalSize) * 100).toInt
      else
        0
      
      println(f"$codec%-15s ${formatBytes(fileSize)}%-15s ${ratio}%%-10d ${elapsed}%-10d")
    }
    
    println("\n" + "=" * 80)
    println("CODEC RECOMMENDATIONS")
    println("=" * 80)
    println("• Use 'auto' for automatic codec selection (recommended)")
    println("• Use 'rle' for highly repetitive data")
    println("• Use 'dictionary' for low-cardinality categorical data (status, category)")
    println("• Use 'for' for numeric ranges (prices, amounts)")
    println("• Use 'lzss' for mixed/text data (descriptions)")
    println("• Use 'none' for already-compressed data")
    
    // Read back with 'auto' codec to verify round-trip
    println("\n" + "=" * 80)
    println("VERIFICATION: Round-trip with 'auto' codec")
    println("=" * 80)
    
    val readBack = spark.read
      .format("kore")
      .load("/tmp/example3_auto.kore")
    
    println(s"✓ Read back ${readBack.count()} rows from Kore file")
    readBack.show(3)
    
    spark.stop()
  }
  
  def formatBytes(bytes: Long): String = {
    if (bytes <= 0) "0 B"
    else {
      val k = 1024L
      val sizes = Array("B", "KB", "MB", "GB")
      val i = (Math.log(bytes) / Math.log(k)).toInt
      f"${bytes / Math.pow(k, i)%.1f} ${sizes(i)}"
    }
  }
}
