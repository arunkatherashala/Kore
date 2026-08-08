
import sys, time, json
from pyspark.sql import SparkSession
from pyspark.sql.functions import col, sum as fsum, count, avg

CSV = sys.argv[1]
ITERS = 3

spark = SparkSession.builder.appName("kore_bench") \
    .master("local[*]") \
    .config("spark.ui.enabled","false") \
    .config("spark.driver.memory","4g") \
    .config("spark.sql.shuffle.partitions","8") \
    .getOrCreate()
spark.sparkContext.setLogLevel("ERROR")

df = spark.read.option("header","true").option("inferSchema","true").csv(CSV)
df.cache(); df.count()

results = {}
def med(lst): s=sorted(lst); return s[len(s)//2]

# Q1 — group by aggregation
t=[]; [(t.append(time.perf_counter()), df.groupBy("l_returnflag","l_linestatus").agg(count("*"),avg("l_extendedprice"),avg("l_quantity")).orderBy("l_returnflag").collect()) for _ in range(ITERS)]
q1_t=[((t[i+1]-t[i])*1000 if i+1<len(t) else 0) for i in range(0,len(t)-1,2)]
times=[]; 
for _ in range(ITERS):
    t0=time.perf_counter()
    df.groupBy("l_returnflag","l_linestatus").agg(count("*"),avg("l_extendedprice"),avg("l_quantity")).orderBy("l_returnflag").collect()
    times.append((time.perf_counter()-t0)*1000)
results["Q1_GroupBy"]=med(times)

# Q6 — filter + sum
times=[]
for _ in range(ITERS):
    t0=time.perf_counter()
    df.filter((col("l_shipdate")>="1994-01-01")&(col("l_shipdate")<"1995-01-01")&(col("l_discount").between(0.05,0.07))&(col("l_quantity")<24)).agg(fsum("l_extendedprice")).collect()
    times.append((time.perf_counter()-t0)*1000)
results["Q6_Filter"]=med(times)

# Q3 — top-K
times=[]
for _ in range(ITERS):
    t0=time.perf_counter()
    df.groupBy("l_orderkey").agg(fsum(col("l_extendedprice")*(1-col("l_discount"))).alias("rev")).orderBy(col("rev").desc()).limit(10).collect()
    times.append((time.perf_counter()-t0)*1000)
results["Q3_TopK"]=med(times)

print(json.dumps(results))
spark.stop()
