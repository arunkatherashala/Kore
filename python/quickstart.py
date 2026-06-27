#!/usr/bin/env python3
"""
Quick Start: Kore + Spark Integration

Run this script to verify your Kore + Spark setup is working correctly.
"""

def main():
    print("\n" + "="*70)
    print("🎯 KORE + SPARK QUICK START VERIFICATION")
    print("="*70)
    
    # Test 1: Import Spark
    print("\n[1/5] Testing PySpark import...")
    try:
        from pyspark.sql import SparkSession
        print("     ✅ PySpark imported successfully")
    except ImportError as e:
        print(f"     ❌ PySpark import failed: {e}")
        print("     → Install: pip install pyspark>=3.1.0")
        return False
    
    # Test 2: Import Kore
    print("\n[2/5] Testing Kore import...")
    try:
        from kore import KoreDataFrameReader, KoreDataFrameWriter
        print("     ✅ Kore modules imported successfully")
    except ImportError as e:
        print(f"     ❌ Kore import failed: {e}")
        print("     → Install: pip install -e /path/to/Kore/python")
        return False
    
    # Test 3: Create Spark session
    print("\n[3/5] Creating Spark session...")
    try:
        spark = SparkSession.builder \
            .appName("KoreQuickStart") \
            .master("local[*]") \
            .getOrCreate()
        print("     ✅ Spark session created successfully")
    except Exception as e:
        print(f"     ❌ Spark session creation failed: {e}")
        return False
    
    # Test 4: Create and write sample DataFrame
    print("\n[4/5] Testing DataFrame creation and write...")
    try:
        from pyspark.sql.types import StructType, StructField, StringType, IntegerType
        
        # Create schema
        schema = StructType([
            StructField("name", StringType(), True),
            StructField("age", IntegerType(), True),
        ])
        
        # Create data
        data = [("Alice", 28), ("Bob", 35), ("Carol", 42)]
        df = spark.createDataFrame(data, schema=schema)
        
        print(f"     ✅ Created DataFrame with {df.count()} rows")
        print("       Schema:", schema)
        
        # Write to Kore
        writer = KoreDataFrameWriter(df)
        writer.mode("overwrite").save("/tmp/test_kore.kore")
        print("     ✅ Written to Kore format successfully")
    except Exception as e:
        print(f"     ❌ Write test failed: {e}")
        return False
    
    # Test 5: Read from Kore
    print("\n[5/5] Testing read from Kore...")
    try:
        reader = KoreDataFrameReader(spark)
        df_read = reader.load("/tmp/test_kore.kore")
        print(f"     ✅ Read {df_read.count()} rows from Kore")
        print("\n     Data preview:")
        df_read.show()
    except Exception as e:
        print(f"     ❌ Read test failed: {e}")
        return False
    finally:
        spark.stop()
    
    # Success
    print("\n" + "="*70)
    print("✅ ALL TESTS PASSED!")
    print("="*70)
    print("\nYour Kore + Spark setup is working correctly.")
    print("\nNext steps:")
    print("  1. Check out examples: python/examples/spark_examples.py")
    print("  2. Read documentation: python/README.md")
    print("  3. Try with your own data!")
    print("\nFor questions, visit: https://github.com/arunkatherashala/Kore")
    print("="*70 + "\n")
    
    return True


if __name__ == "__main__":
    import sys
    success = main()
    sys.exit(0 if success else 1)
