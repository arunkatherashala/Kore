"""
PySpark + Kore Integration Examples
"""

from pyspark.sql import SparkSession
from pyspark.sql.types import StructType, StructField, StringType, IntegerType, DoubleType

# Import Kore connector
from kore import register_kore_datasource
from kore.reader import KoreDataFrameReader
from kore.writer import KoreDataFrameWriter


def example_1_read_kore_file():
    """
    Example 1: Read a Kore file into a PySpark DataFrame
    """
    print("\n" + "="*60)
    print("EXAMPLE 1: Read Kore File")
    print("="*60)
    
    spark = SparkSession.builder \
        .appName("KoreExample1") \
        .getOrCreate()
    
    reader = KoreDataFrameReader(spark)
    df = reader.load("sample.kore")
    
    print(f"✅ Loaded {df.count()} rows")
    df.show(5)
    
    spark.stop()


def example_2_write_spark_df_to_kore():
    """
    Example 2: Write a Spark DataFrame to Kore format
    """
    print("\n" + "="*60)
    print("EXAMPLE 2: Write Spark DataFrame to Kore")
    print("="*60)
    
    spark = SparkSession.builder \
        .appName("KoreExample2") \
        .getOrCreate()
    
    # Create sample DataFrame
    data = [
        ("Alice", 28, 95000.50, "Engineering"),
        ("Bob", 35, 105000.00, "Engineering"),
        ("Carol", 42, 115000.75, "Management"),
    ]
    
    schema = StructType([
        StructField("name", StringType(), True),
        StructField("age", IntegerType(), True),
        StructField("salary", DoubleType(), True),
        StructField("dept", StringType(), True),
    ])
    
    df = spark.createDataFrame(data, schema=schema)
    
    print("Original DataFrame:")
    df.show()
    
    # Write to Kore
    writer = KoreDataFrameWriter(df)
    writer.mode("overwrite").save("output.kore")
    
    spark.stop()


def example_3_spark_sql_integration():
    """
    Example 3: Use Spark SQL with Kore files (requires registration)
    """
    print("\n" + "="*60)
    print("EXAMPLE 3: Spark SQL with Kore Format")
    print("="*60)
    
    spark = SparkSession.builder \
        .appName("KoreExample3") \
        .getOrCreate()
    
    # Register Kore as a data source (Spark 3.5+)
    try:
        register_kore_datasource(spark)
        
        # Now you can use Spark SQL with Kore
        df = spark.read.format("kore").load("data.kore")
        df.createOrReplaceTempView("kore_data")
        
        result = spark.sql("SELECT * FROM kore_data WHERE age > 30")
        print("Query result (age > 30):")
        result.show()
    except Exception as e:
        print(f"⚠️  Spark SQL integration not available: {e}")
        print("   Your Spark version may not support custom DataSources")
    
    spark.stop()


def example_4_csv_to_kore_to_parquet():
    """
    Example 4: Full pipeline - CSV → Kore → Parquet
    Demonstrates format conversion using Kore
    """
    print("\n" + "="*60)
    print("EXAMPLE 4: CSV → Kore → Parquet Pipeline")
    print("="*60)
    
    spark = SparkSession.builder \
        .appName("KoreExample4") \
        .getOrCreate()
    
    # Step 1: Read CSV
    print("\n1️⃣  Reading CSV...")
    df_csv = spark.read.option("inferSchema", True).csv("input.csv", header=True)
    print(f"   Loaded {df_csv.count()} rows from CSV")
    
    # Step 2: Write to Kore (38% compression!)
    print("\n2️⃣  Writing to Kore...")
    writer = KoreDataFrameWriter(df_csv)
    writer.mode("overwrite").save("intermediate.kore")
    
    # Step 3: Read from Kore
    print("\n3️⃣  Reading from Kore...")
    reader = KoreDataFrameReader(spark)
    df_kore = reader.load("intermediate.kore")
    print(f"   Loaded {df_kore.count()} rows from Kore")
    
    # Step 4: Write to Parquet
    print("\n4️⃣  Writing to Parquet...")
    df_kore.write.mode("overwrite").parquet("output.parquet")
    
    print("\n✅ Pipeline complete: CSV → Kore → Parquet")
    
    spark.stop()


def example_5_batch_processing():
    """
    Example 5: Batch processing with Kore - process multiple files
    """
    print("\n" + "="*60)
    print("EXAMPLE 5: Batch Processing")
    print("="*60)
    
    spark = SparkSession.builder \
        .appName("KoreExample5") \
        .getOrCreate()
    
    # Process multiple Kore files
    files = ["data1.kore", "data2.kore", "data3.kore"]
    
    reader = KoreDataFrameReader(spark)
    all_dfs = []
    
    for file in files:
        try:
            df = reader.load(file)
            all_dfs.append(df)
            print(f"✅ Loaded {file}: {df.count()} rows")
        except FileNotFoundError:
            print(f"⚠️  File not found: {file}")
    
    if all_dfs:
        # Combine all DataFrames
        combined = all_dfs[0]
        for df in all_dfs[1:]:
            combined = combined.union(df)
        
        print(f"\n📊 Combined total: {combined.count()} rows")
        combined.show(5)
    
    spark.stop()


if __name__ == "__main__":
    print("\n" + "🎯 KORE + SPARK INTEGRATION EXAMPLES 🎯")
    
    # Uncomment the example you want to run:
    
    # example_1_read_kore_file()
    # example_2_write_spark_df_to_kore()
    # example_3_spark_sql_integration()
    # example_4_csv_to_kore_to_parquet()
    # example_5_batch_processing()
    
    print("\n" + "="*60)
    print("To run an example, uncomment it in the main block")
    print("="*60)
