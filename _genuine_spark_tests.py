"""
Genuine Spark limitations test — all in ONE JVM session.
Called by genuine_limits_test.py
Output format per test: SPARK_TEST:<name>:PASS|FAIL[:<note>[_MS:<ms>]]
"""
import sys, time, traceback
from pyspark.sql import SparkSession
from pyspark.sql.functions import (
    count, avg, sum as _sum, min as _min, max as _max,
    col, lag, lead, row_number, rank, dense_rank, ntile, coalesce,
    upper, lower, when, lit, round as spark_round, count_distinct
)
from pyspark.sql.window import Window

CSV = sys.argv[1]

spark = (SparkSession.builder
         .appName("genuine_limits")
         .master("local[*]")
         .config("spark.ui.enabled", "false")
         .config("spark.driver.memory", "4g")
         .config("spark.sql.shuffle.partitions", "8")
         .getOrCreate())
spark.sparkContext.setLogLevel("ERROR")

print("SPARK_INIT:ok", flush=True)

df = spark.read.option("header", "true").option("inferSchema", "true").csv(CSV)
df.cache()
df.count()
print("SPARK_CACHED:ok", flush=True)

# 50K sample for join tests to avoid O(n²) explosion
df_j = df.orderBy("l_orderkey").limit(50_000)
df_j.cache(); df_j.count()

def test(name, fn):
    t0 = time.perf_counter()
    try:
        fn()
        ms = (time.perf_counter() - t0) * 1000
        print(f"SPARK_TEST:{name}:PASS:_MS:{ms:.0f}", flush=True)
    except Exception as e:
        ms = (time.perf_counter() - t0) * 1000
        print(f"SPARK_TEST:{name}:FAIL:{str(e)[:100]}_MS:{ms:.0f}", flush=True)

# ── 1. Basic SQL ──────────────────────────────────────────────────────────────
test("COUNT_STAR",    lambda: df.select(count("*").alias("n")).collect())
test("AVG",           lambda: df.select(avg("l_extendedprice")).collect())
test("GROUP_ORDER",   lambda: df.groupBy("l_returnflag").count().orderBy("l_returnflag").collect())
test("HAVING",        lambda: df.groupBy("l_returnflag").agg(count("*").alias("n")).filter(col("n") > 1_000_000).collect())
test("DISTINCT",      lambda: df.select("l_returnflag").distinct().collect())

# ── 2. JOINs ──────────────────────────────────────────────────────────────────
test("INNER_JOIN",    lambda: df_j.alias("a").join(df_j.filter(col("l_quantity") > 40).alias("b"), "l_orderkey").select("l_orderkey").count())
test("LEFT_JOIN",     lambda: df_j.alias("a").join(df_j.filter(col("l_quantity") > 40).alias("b"), ["l_orderkey"], "left").limit(5).collect())
test("FULL_OUTER",    lambda: df_j.alias("a").join(df_j.filter(col("l_quantity") > 40).alias("b"), ["l_orderkey"], "full").limit(5).collect())

# ── 3. Window functions ───────────────────────────────────────────────────────
w_part = Window.partitionBy("l_returnflag").orderBy("l_extendedprice")
w_all  = Window.orderBy("l_extendedprice")

test("ROW_NUMBER",    lambda: df.withColumn("rn", row_number().over(w_part)).limit(5).collect())
test("RANK",          lambda: df.withColumn("rk", rank().over(w_part)).limit(5).collect())
test("LAG_LEAD",      lambda: df.withColumn("prev", lag("l_extendedprice", 1).over(w_part)).limit(5).collect())
test("NTILE",         lambda: df.withColumn("q", ntile(4).over(w_all)).limit(5).collect())
test("CUM_SUM",       lambda: df.withColumn("cs", _sum("l_extendedprice").over(w_part.rowsBetween(Window.unboundedPreceding, Window.currentRow))).limit(5).collect())

# ── 4. Subqueries ─────────────────────────────────────────────────────────────
def _scalar_subq():
    avg_val = df.agg(avg("l_extendedprice")).collect()[0][0]
    df.filter(col("l_extendedprice") > avg_val).select("l_returnflag").limit(5).collect()

def _in_subq():
    high_qty = df.filter(col("l_quantity") > 40).select("l_returnflag").distinct()
    df.join(high_qty, "l_returnflag").select("l_returnflag").distinct().limit(5).collect()

def _exists_subq():
    has_high = df.filter(col("l_quantity") > 45).select("l_returnflag").distinct()
    df.join(has_high, "l_returnflag").select("l_returnflag").limit(5).collect()

def _subq_from():
    inner = df.filter(col("l_quantity") > 30)
    inner.groupBy("l_returnflag").count().collect()

test("SCALAR_SUBQ",   _scalar_subq)
test("IN_SUBQ",       _in_subq)
test("EXISTS_SUBQ",   _exists_subq)
test("SUBQ_FROM",     _subq_from)

# ── 5. CTEs & UNION ───────────────────────────────────────────────────────────
def _cte():
    # Spark DataFrames ARE CTEs
    agg = df.groupBy("l_returnflag").agg(avg("l_extendedprice").alias("avg_p"))
    agg.filter(col("avg_p") > 50_000).collect()

def _union_all():
    a = df.filter(col("l_quantity") > 45).select("l_returnflag")
    b = df.filter(col("l_quantity") < 5).select("l_returnflag")
    a.union(b).limit(10).collect()

def _union_dist():
    a = df.filter(col("l_quantity") > 45).select("l_returnflag")
    b = df.filter(col("l_quantity") < 5).select("l_returnflag")
    a.union(b).distinct().collect()

test("CTE",           _cte)
test("UNION_ALL",     _union_all)
test("UNION_DIST",    _union_dist)

# ── 6. Expressions ────────────────────────────────────────────────────────────
test("CASE_WHEN",     lambda: df.withColumn("label", when(col("l_returnflag") == "A", "accepted").when(col("l_returnflag") == "R", "rejected").otherwise("other")).limit(5).collect())
test("LIKE",          lambda: df.filter(col("l_comment").like("%special%")).limit(5).collect())
test("BETWEEN",       lambda: df.filter(col("l_quantity").between(10, 20)).limit(5).collect())
test("IN_LIST",       lambda: df.filter(col("l_returnflag").isin("A", "R")).limit(5).collect())
test("IS_NULL",       lambda: df.filter(col("l_comment").isNull()).limit(5).collect())

# ── 7. Scale ──────────────────────────────────────────────────────────────────
test("SORT_6M",       lambda: df.orderBy(col("l_extendedprice").desc()).limit(100).collect())
test("MULTI_AGG_6M",  lambda: df.groupBy("l_returnflag").agg(count("*"), _sum("l_extendedprice"), avg("l_extendedprice"), _min("l_extendedprice"), _max("l_extendedprice")).collect())

# ── 10. SQL edge cases ────────────────────────────────────────────────────────
test("COUNT_DISTINCT",lambda: df.groupBy("l_returnflag").agg(count_distinct("l_orderkey").alias("uniq")).collect())
test("COALESCE",      lambda: df.withColumn("safe", coalesce(col("l_comment"), lit("n/a"))).limit(5).collect())
test("STRING_FUNCS",  lambda: df.withColumn("up", upper("l_returnflag")).withColumn("lo", lower("l_comment")).limit(5).collect())
test("ROUND",         lambda: df.withColumn("r", spark_round("l_extendedprice", 2)).limit(5).collect())

spark.stop()
print("SPARK_DONE:ok", flush=True)
