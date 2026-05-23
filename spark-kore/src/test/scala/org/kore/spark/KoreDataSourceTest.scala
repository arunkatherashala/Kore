package org.kore.spark

import org.scalatest.FunSuite
import org.apache.spark.sql.types._
import java.io.File
import java.nio.file.Files

class KoreDataSourceTest extends FunSuite {
  
  val testDir = new File(System.getProperty("java.io.tmpdir"), "kore-spark-tests")
  val testFile = new File(testDir, "test.kore")
  
  override def beforeAll(): Unit = {
    super.beforeAll()
    if (!testDir.exists()) {
      testDir.mkdirs()
    }
  }
  
  override def afterAll(): Unit = {
    super.afterAll()
    // Clean up test files
    if (testFile.exists()) {
      Files.delete(testFile.toPath)
    }
    if (testDir.exists() && testDir.listFiles().isEmpty) {
      Files.delete(testDir.toPath)
    }
  }
  
  // Test 1: DataSource short name
  test("KoreDataSource.shortName should return 'kore'") {
    val ds = new KoreDataSource()
    assert(ds.shortName() === "kore")
  }
  
  // Test 2: Reader creation without path should fail
  test("KoreDataSourceReader creation without path parameter should fail") {
    val ds = new KoreDataSource()
    val options = new java.util.HashMap[String, String]()
    
    assertThrows[IllegalArgumentException] {
      ds.createReader(options)
    }
  }
  
  // Test 3: Reader creation with non-existent file should fail
  test("KoreDataSourceReader with non-existent file should fail") {
    val ds = new KoreDataSource()
    val options = new java.util.HashMap[String, String]()
    options.put("path", "/non/existent/path/test.kore")
    
    assertThrows[IllegalArgumentException] {
      ds.createReader(options)
    }
  }
  
  // Test 4: Filter pushdown detection
  test("KoreFilterPushdown should identify pushable filters") {
    val pushdown = new TestFilterPushdown()
    
    val filter1 = org.apache.spark.sql.sources.EqualTo("id", 42)
    assert(pushdown.canPushFilter(filter1) === true)
    
    val filter2 = org.apache.spark.sql.sources.GreaterThan("age", 30)
    assert(pushdown.canPushFilter(filter2) === true)
    
    val filter3 = org.apache.spark.sql.sources.IsNotNull("name")
    assert(pushdown.canPushFilter(filter3) === true)
  }
  
  // Test 5: Selectivity estimation
  test("KoreFilterPushdown should estimate filter selectivity") {
    val pushdown = new TestFilterPushdown()
    
    val eqFilter = org.apache.spark.sql.sources.EqualTo("id", 42)
    val selectivity = pushdown.estimateSelectivity(eqFilter)
    assert(selectivity === 0.01) // 1% for equality
    
    val gtFilter = org.apache.spark.sql.sources.GreaterThan("age", 30)
    val gtSelectivity = pushdown.estimateSelectivity(gtFilter)
    assert(gtSelectivity === 0.33) // 1/3 for greater than
  }
  
  // Test 6: Write mode validation - Overwrite
  test("KoreDataSourceWriter in Overwrite mode should delete existing file") {
    // Create a test file
    testFile.createNewFile()
    assert(testFile.exists())
    
    val schema = StructType(Seq(
      StructField("id", LongType, true),
      StructField("value", StringType, true)
    ))
    
    val options = new java.util.HashMap[String, String]()
    val writer = new KoreDataSourceWriter(testFile.getPath, schema, org.apache.spark.sql.SaveMode.Overwrite, options)
    
    // File should be deleted in overwrite mode
    assert(!testFile.exists())
  }
  
  // Test 7: Write mode validation - ErrorIfExists
  test("KoreDataSourceWriter in ErrorIfExists mode should throw if file exists") {
    testFile.createNewFile()
    assert(testFile.exists())
    
    val schema = StructType(Seq(
      StructField("id", LongType, true),
      StructField("value", StringType, true)
    ))
    
    val options = new java.util.HashMap[String, String]()
    
    assertThrows[RuntimeException] {
      new KoreDataSourceWriter(testFile.getPath, schema, org.apache.spark.sql.SaveMode.ErrorIfExists, options)
    }
  }
  
  // Test 8: Partition reader creation
  test("KoreReadTask should create partition readers") {
    val schema = StructType(Seq(
      StructField("id", LongType, true),
      StructField("value", StringType, true)
    ))
    
    val task = new KoreReadTask(testFile.getPath, schema, List(), List())
    val reader = task.createPartitionedReader(0)
    
    assert(reader !== null)
    assert(reader.isInstanceOf[KorePartitionReader])
  }
  
  // Test 9: Partition reader operations
  test("KorePartitionReader should handle basic operations") {
    val schema = StructType(Seq(
      StructField("id", LongType, true),
      StructField("value", StringType, true)
    ))
    
    val reader = new KorePartitionReader(testFile.getPath, schema, List(), List())
    
    // First call to next() should return true
    assert(reader.next() === true)
    
    // Second call should return false
    assert(reader.next() === false)
    
    // Should be able to close
    reader.close()
  }
  
  // Test 10: Writer factory creation
  test("KoreWriterFactory should create data writers") {
    val schema = StructType(Seq(
      StructField("id", LongType, true),
      StructField("value", StringType, true)
    ))
    
    val factory = new KoreWriterFactory(testFile.getPath, schema, "auto")
    val writer = factory.createDataWriter(0, 0, 0)
    
    assert(writer !== null)
    assert(writer.isInstanceOf[KoreDataWriter])
  }
  
  // Test 11: Compression options parsing
  test("KoreDataSourceWriter should parse compression options") {
    val schema = StructType(Seq(
      StructField("id", LongType, true),
      StructField("value", StringType, true)
    ))
    
    val options = new java.util.HashMap[String, String]()
    options.put("compression", "gzip")
    
    // Create a non-existent file path
    val newTestFile = new File(testDir, "test-compress.kore")
    val writer = new KoreDataSourceWriter(newTestFile.getPath, schema, org.apache.spark.sql.SaveMode.Overwrite, options)
    
    assert(writer !== null)
  }
  
  // Test 12: Partition preference
  test("KoreReadTask should return empty preferred locations") {
    val schema = StructType(Seq(
      StructField("id", LongType, true),
      StructField("value", StringType, true)
    ))
    
    val task = new KoreReadTask(testFile.getPath, schema, List(), List())
    val locations = task.preferredLocations()
    
    assert(locations.length === 0)
  }
  
  // Test 13: And filter pushdown
  test("KoreFilterPushdown should handle And filters") {
    val pushdown = new TestFilterPushdown()
    
    val filter = org.apache.spark.sql.sources.And(
      org.apache.spark.sql.sources.GreaterThan("age", 30),
      org.apache.spark.sql.sources.LessThan("age", 60)
    )
    
    assert(pushdown.canPushFilter(filter) === true)
  }
  
  // Test 14: Or filter pushdown
  test("KoreFilterPushdown should handle Or filters") {
    val pushdown = new TestFilterPushdown()
    
    val filter = org.apache.spark.sql.sources.Or(
      org.apache.spark.sql.sources.EqualTo("status", "active"),
      org.apache.spark.sql.sources.EqualTo("status", "pending")
    )
    
    assert(pushdown.canPushFilter(filter) === true)
  }
  
  // Test 15: Not filter pushdown
  test("KoreFilterPushdown should handle Not filters") {
    val pushdown = new TestFilterPushdown()
    
    val filter = org.apache.spark.sql.sources.Not(
      org.apache.spark.sql.sources.IsNull("email")
    )
    
    assert(pushdown.canPushFilter(filter) === true)
  }
  
  // Test 16: String filter pushdown
  test("KoreFilterPushdown should handle String filters") {
    val pushdown = new TestFilterPushdown()
    
    val filter1 = org.apache.spark.sql.sources.StringStartsWith("name", "John")
    assert(pushdown.canPushFilter(filter1) === true)
    
    val filter2 = org.apache.spark.sql.sources.StringEndsWith("email", ".com")
    assert(pushdown.canPushFilter(filter2) === true)
    
    val filter3 = org.apache.spark.sql.sources.StringContains("description", "urgent")
    assert(pushdown.canPushFilter(filter3) === true)
  }
  
  // Test 17: In filter pushdown
  test("KoreFilterPushdown should handle In filters") {
    val pushdown = new TestFilterPushdown()
    
    val filter = org.apache.spark.sql.sources.In("status", Array("active", "pending", "review"))
    assert(pushdown.canPushFilter(filter) === true)
  }
  
  // Test 18: Schema inference
  test("KoreDataSourceReader should infer schema") {
    testFile.createNewFile()
    
    val options = new java.util.HashMap[String, String]()
    options.put("path", testFile.getPath)
    
    val reader = new KoreDataSourceReader(testFile.getPath, options)
    val schema = reader.readSchema()
    
    assert(schema !== null)
    assert(schema.fields.length > 0)
  }
  
  // Test 19: Statistics estimation
  test("KoreDataSourceReader should estimate statistics") {
    testFile.createNewFile()
    
    val options = new java.util.HashMap[String, String]()
    options.put("path", testFile.getPath)
    
    val reader = new KoreDataSourceReader(testFile.getPath, options)
    val stats = reader.estimateStatistics()
    
    assert(stats !== null)
    assert(stats.sizeInBytes() >= 0)
  }
  
  // Test 20: Multiple read tasks
  test("KoreDataSourceReader should create read tasks") {
    testFile.createNewFile()
    
    val options = new java.util.HashMap[String, String]()
    options.put("path", testFile.getPath)
    
    val reader = new KoreDataSourceReader(testFile.getPath, options)
    val tasks = reader.createReadTasks()
    
    assert(tasks !== null)
    assert(tasks.size() > 0)
  }
}

// Test helper class
class TestFilterPushdown extends KoreFilterPushdown
