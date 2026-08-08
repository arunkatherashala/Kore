
import sys, time, json
from pyspark.sql import SparkSession
from pyspark.sql.functions import col, sum as fsum, count, avg, row_number, lag
from pyspark.sql.window import Window

CSV = sys.argv[1]
ITERS = 3

spark = SparkSession.builder.appName("battle_bench") \
    .master("local[*]") \
    .config("spark.ui.enabled","false") \
    .config("spark.driver.memory","4g") \
    .config("spark.sql.shuffle.partitions","8") \
    .getOrCreate()
spark.sparkContext.setLogLevel("ERROR")

df = spark.read.option("header","true").option("inferSchema","true").csv(CSV)
df.cache(); df.count()
print("SPARK_READY", flush=True)

def bench(name, fn):
    times = []
    for _ in range(ITERS):
        t0 = time.perf_counter()
        fn()
        times.append((time.perf_counter()-t0)*1000)
    s = sorted(times)
    m = s[len(s)//2]
    print(f"BENCH:{name}:{m:.1f}", flush=True)

bench("Q1 GROUP BY agg", lambda: df.groupBy("l_returnflag","l_linestatus")
    .agg(count("*"),avg("l_extendedprice"),avg("l_quantity")).orderBy("l_returnflag").collect())

bench("Q6 Filter+SUM", lambda: df.filter(
    (col("l_shipdate")>="1994-01-01") & (col("l_shipdate")<"1995-01-01") &
    (col("l_discount").between(0.05,0.07)) & (col("l_quantity")<24))
    .agg(fsum(col("l_extendedprice")*col("l_discount"))).collect())

bench("Q3 Top-K join", lambda: df.groupBy("l_orderkey")
    .agg(fsum(col("l_extendedprice")*(1-col("l_discount"))).alias("rev"))
    .orderBy(col("rev").desc()).limit(10).collect())

bench("S1 Sort 6M", lambda: df.orderBy(col("l_extendedprice").desc()).limit(100).collect())

bench("W1 Window fn", lambda: df.withColumn("rn",
    row_number().over(Window.partitionBy("l_returnflag").orderBy(col("l_extendedprice").desc())))
    .limit(20).collect())

spark.stop()
print("SPARK_DONE", flush=True)
