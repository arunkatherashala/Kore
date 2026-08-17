"""Test KORE + PySpark integration — read .kore data into Spark DataFrame"""
import sys, os, time
sys.path.insert(0, os.path.join(os.path.dirname(__file__), 'kore-python'))
import kore_fileformat as kore

# Step 1: Write test .hkore file
print("=== KORE + Spark Integration Test ===\n")

b = kore.DataBlock()
b.add_column('region', kore.DataType.STR, ['East','West','East','North','West','South','East','North','West','South'])
b.add_column('product', kore.DataType.STR, ['Widget','Gadget','Widget','Gizmo','Widget','Gadget','Gizmo','Widget','Gadget','Gizmo'])
b.add_column('amount', kore.DataType.F64, [100.0, 250.0, 150.0, 300.0, 200.0, 175.0, 325.0, 125.0, 275.0, 350.0])
b.add_column('quantity', kore.DataType.I64, [10, 25, 15, 30, 20, 17, 32, 12, 27, 35])

kore.write_hybrid('C:/tmp/spark_test.hkore', b)
print("1. Written test .hkore file (10 rows x 4 cols)")

# Step 2: Read with KORE
t0 = time.perf_counter()
data = kore.read_hybrid('C:/tmp/spark_test.hkore')
kore_ms = (time.perf_counter() - t0) * 1000
print(f"2. KORE read: {kore_ms:.2f}ms — {data.num_rows} rows x {data.num_columns} cols")

# Step 3: Load into PySpark
from pyspark.sql import SparkSession
from pyspark.sql.types import StructType, StructField, StringType, DoubleType, LongType

print("3. Starting Spark session...")
spark = SparkSession.builder \
    .master("local[*]") \
    .appName("KORE-Spark-Test") \
    .config("spark.ui.enabled", "false") \
    .config("spark.driver.host", "localhost") \
    .getOrCreate()
spark.sparkContext.setLogLevel("ERROR")

# Convert KORE DataBlock → Spark DataFrame
rows = []
for i in range(data.num_rows):
    rows.append(tuple(c.data[i] for c in data.columns))

schema = StructType([
    StructField("region", StringType(), True),
    StructField("product", StringType(), True),
    StructField("amount", DoubleType(), True),
    StructField("quantity", LongType(), True),
])

df = spark.createDataFrame(rows, schema)
df.createOrReplaceTempView("sales")

print(f"4. Spark DataFrame created: {df.count()} rows")
print()

# Step 4: Run SQL queries on KORE data in Spark
print("=== Spark SQL on KORE data ===")
print()
print("Query 1: SELECT * FROM sales LIMIT 5")
spark.sql("SELECT * FROM sales LIMIT 5").show()

print("Query 2: Revenue by region")
spark.sql("SELECT region, SUM(amount) as revenue, SUM(quantity) as units FROM sales GROUP BY region ORDER BY revenue DESC").show()

print("Query 3: Top products")
spark.sql("SELECT product, AVG(amount) as avg_amount, COUNT(*) as orders FROM sales GROUP BY product ORDER BY avg_amount DESC").show()

# Step 5: Write Spark result back to KORE
result = spark.sql("SELECT region, SUM(amount) as revenue FROM sales GROUP BY region ORDER BY revenue DESC").collect()
out = kore.DataBlock()
out.add_column('region', kore.DataType.STR, [r['region'] for r in result])
out.add_column('revenue', kore.DataType.F64, [r['revenue'] for r in result])
kore.write_hybrid('C:/tmp/spark_result.hkore', out)

print("5. Spark result written to .hkore!")
header = kore.read_hybrid_header('C:/tmp/spark_result.hkore')
print(header)

spark.stop()
print("\nDONE — KORE + Spark integration works!")
