"""Run TPC-H queries in PySpark for comparison with KORE."""
from pyspark.sql import SparkSession
import time, sys, os

spark = SparkSession.builder.appName("TPC-H").master("local[*]").getOrCreate()
data_dir = sys.argv[1] if len(sys.argv) > 1 else "tpch_parquet"

# Load tables
tables = ["lineitem", "orders", "part", "supplier", "partsupp", "customer", "nation", "region"]
for t in tables:
    path = os.path.join(data_dir, f"{t}.parquet")
    if os.path.exists(path):
        spark.read.parquet(path).createOrReplaceTempView(t)
        print(f"Loaded {t}: {spark.table(t).count()} rows")

# TPC-H queries
queries = {
    "Q1": "SELECT l_returnflag, l_linestatus, SUM(l_quantity), SUM(l_extendedprice) FROM lineitem WHERE l_shipdate <= 19980901 GROUP BY l_returnflag, l_linestatus",
    "Q6": "SELECT SUM(l_extendedprice * l_discount) FROM lineitem WHERE l_shipdate >= 19940101 AND l_shipdate < 19950101 AND l_discount >= 0.05 AND l_discount <= 0.07 AND l_quantity < 24",
}

for name, sql in queries.items():
    start = time.time()
    result = spark.sql(sql)
    result.show()
    elapsed = (time.time() - start) * 1000
    print(f"{name}: {elapsed:.0f}ms ({result.count()} rows)")

spark.stop()
