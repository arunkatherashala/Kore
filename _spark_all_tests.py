"""
All Spark limitation tests in ONE JVM session.
Run by bench_limitations_v2.py
"""
import sys, time, json
from pyspark.sql import SparkSession
from pyspark.sql.functions import *
from pyspark.sql.window import Window

CSV = sys.argv[1]

spark = SparkSession.builder.appName("limits_all") \
    .master("local[*]") \
    .config("spark.ui.enabled","false") \
    .config("spark.driver.memory","4g") \
    .config("spark.sql.shuffle.partitions","8") \
    .getOrCreate()
spark.sparkContext.setLogLevel("ERROR")

print("SPARK_INIT:ok", flush=True)

df = spark.read.option("header","true").option("inferSchema","true").csv(CSV)
df.cache()
df.count()
print("SPARK_CACHED:ok", flush=True)

# Small sample for JOIN tests — avoids cartesian explosion on low-cardinality keys
df_j = df.orderBy("l_orderkey").limit(50_000)
df_j.cache(); df_j.count()
print("SPARK_SAMPLE:ok", flush=True)

def test(name, code_fn):
    try:
        code_fn()
        print(f"SPARK_TEST:{name}:PASS", flush=True)
    except Exception as e:
        print(f"SPARK_TEST:{name}:FAIL:{str(e)[:100]}", flush=True)

# 1. Basic SQL
test("COUNT(*)",          lambda: df.select(count("*")).collect())
test("AVG()",             lambda: df.select(avg("l_extendedprice")).collect())
test("GROUP_BY_ORDER_BY", lambda: df.groupBy("l_returnflag").count().orderBy("l_returnflag").collect())

# 2. JOINs (on 50K sample to avoid cartesian explosion)
test("INNER_JOIN",       lambda: df_j.alias("a").join(df_j.filter(col("l_quantity")>40).alias("b"),"l_orderkey").select("l_orderkey").count())
test("LEFT_JOIN",        lambda: df_j.alias("a").join(df_j.alias("b"), ["l_orderkey"], "left").limit(5).collect())
test("FULL_OUTER_JOIN",  lambda: df_j.alias("a").join(df_j.alias("b"), ["l_orderkey"], "full").limit(5).collect())

# 3. Window Functions
test("ROW_NUMBER_OVER",  lambda: df.withColumn("rn", row_number().over(Window.partitionBy("l_returnflag").orderBy("l_extendedprice"))).limit(10).collect())
test("LAG_LEAD",         lambda: df.withColumn("prev", lag("l_extendedprice").over(Window.partitionBy("l_returnflag").orderBy("l_orderkey"))).limit(10).collect())
test("NTILE",            lambda: df.withColumn("q", ntile(4).over(Window.orderBy("l_extendedprice"))).limit(10).collect())

# 4. Subqueries / CTEs
test("CTE_equiv",        lambda: df.groupBy("l_returnflag").agg(avg("l_extendedprice").alias("avg_p")).filter(col("avg_p") > 50000).collect())
test("SUBQUERY_WHERE",   lambda: df.filter(col("l_extendedprice") > df.agg(avg("l_extendedprice")).collect()[0][0]).select("l_returnflag").limit(5).collect())
test("SUBQUERY_FROM",    lambda: df.filter(col("l_quantity") > 30).groupBy("l_returnflag").count().collect())

# 5. Scale
test("SORT_6M_ROWS",     lambda: df.orderBy(col("l_extendedprice").desc()).limit(100).collect())

spark.stop()
print("SPARK_DONE:ok", flush=True)
