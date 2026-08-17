"""KORE vs Parquet — Spark Performance Test (100K rows)"""
import sys, os, time
os.environ['HADOOP_HOME'] = 'C:\\hadoop'
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), 'kore-python'))
import kore_fileformat as kore

os.makedirs('C:/tmp', exist_ok=True)

for N in [10_000_000]:
    print(f"\n{'='*70}")
    print(f"  {N:,} ROWS x 3 cols (price F64, volume I64, region STR)")
    print(f"{'='*70}\n")

print(f"=== Spark Performance: .kore vs .hkore vs .parquet ({N:,} rows) ===\n")

# Generate data
prices = [float(i) * 1.5 for i in range(N)]
volumes = list(range(N))
regions = ['East','West','North','South'] * (N // 4)

# Build DataBlock once, reuse for .kore and .hkore
b = kore.DataBlock()
b.add_column('price', kore.DataType.F64, prices)
b.add_column('volume', kore.DataType.I64, volumes)
b.add_column('region', kore.DataType.STR, regions)

# Write .kore (pure kore v3 — text header + Rust binary)
t0 = time.perf_counter()
kore.write_file('C:/tmp/perf.kore', b)
kore_write = (time.perf_counter() - t0) * 1000
kore_size = os.path.getsize('C:/tmp/perf.kore')
print(f"[1] .kore  write: {kore_write:.1f}ms  size: {kore_size/1024:.0f}KB")

# Write .hkore (hybrid kore)
t0 = time.perf_counter()
kore.write_hybrid('C:/tmp/perf.hkore', b)
hkore_write = (time.perf_counter() - t0) * 1000
hkore_size = os.path.getsize('C:/tmp/perf.hkore')
print(f"[2] .hkore write: {hkore_write:.1f}ms  size: {hkore_size/1024:.0f}KB")

# Start Spark
from pyspark.sql import SparkSession
from pyspark.sql.types import StructType, StructField, StringType, DoubleType, LongType

print("\n[3] Starting Spark...")
spark = SparkSession.builder \
    .master("local[*]") \
    .appName("KORE-Perf") \
    .config("spark.ui.enabled", "false") \
    .config("spark.driver.host", "localhost") \
    .getOrCreate()
spark.sparkContext.setLogLevel("ERROR")
print("[3] Spark ready!\n")

schema = StructType([
    StructField("price", DoubleType(), False),
    StructField("volume", LongType(), False),
    StructField("region", StringType(), False),
])

# Write .parquet via PyArrow (avoids Spark/Hadoop Windows issue)
import pyarrow as pa, pyarrow.parquet as pq
pa_table = pa.table({"price": prices, "volume": volumes, "region": regions})
t0 = time.perf_counter()
pq.write_table(pa_table, "C:/tmp/perf.parquet")
parquet_write = (time.perf_counter() - t0) * 1000
pq_size = os.path.getsize("C:/tmp/perf.parquet")
print(f"[4] .parquet write: {parquet_write:.1f}ms  size: {pq_size/1024:.0f}KB")

# === READ PERFORMANCE ===
print("\n--- READ PERFORMANCE ---")

def block_to_rows(blk):
    c0, c1, c2 = blk.columns[0].data, blk.columns[1].data, blk.columns[2].data
    return [(c0[i], c1[i], c2[i]) for i in range(blk.num_rows)]

# Read .kore -> Spark
t0 = time.perf_counter()
k_data = kore.read_file('C:/tmp/perf.kore')
k_rows = block_to_rows(k_data)
df_kore = spark.createDataFrame(k_rows, schema)
df_kore.cache().count()
kore_read = (time.perf_counter() - t0) * 1000
print(f"  .kore    -> Spark: {kore_read:.1f}ms ({N:,} rows)")

# Read .hkore -> Spark
t0 = time.perf_counter()
h_data = kore.read_hybrid('C:/tmp/perf.hkore')
h_rows = block_to_rows(h_data)
df_hkore = spark.createDataFrame(h_rows, schema)
df_hkore.cache().count()
hkore_read = (time.perf_counter() - t0) * 1000
print(f"  .hkore   -> Spark: {hkore_read:.1f}ms ({N:,} rows)")

# Read .parquet -> Spark (via PyArrow bridge)
t0 = time.perf_counter()
pq_data = pq.read_table("C:/tmp/perf.parquet")
pq_rows = [(float(pq_data['price'][i].as_py()), int(pq_data['volume'][i].as_py()), str(pq_data['region'][i].as_py())) for i in range(len(pq_data))]
df_pq = spark.createDataFrame(pq_rows, schema)
df_pq.cache().count()
parquet_read = (time.perf_counter() - t0) * 1000
print(f"  .parquet -> Spark: {parquet_read:.1f}ms ({N:,} rows)")

# === QUERY PERFORMANCE ===
print("\n--- QUERY: SUM(price), AVG(volume) GROUP BY region ---")

df_kore.createOrReplaceTempView("kore_data")
df_hkore.createOrReplaceTempView("hkore_data")
df_pq.createOrReplaceTempView("parquet_data")

QUERY = "SELECT region, SUM(price) as total, AVG(volume) as avg_vol FROM {} GROUP BY region"

for view in ("kore_data", "hkore_data", "parquet_data"):
    spark.sql(QUERY.format(view)).collect()

RUNS = 3

def bench(view):
    ts = []
    for _ in range(RUNS):
        t0 = time.perf_counter()
        spark.sql(QUERY.format(view)).collect()
        ts.append((time.perf_counter() - t0) * 1000)
    return min(ts)

kore_query = bench("kore_data")
hkore_query = bench("hkore_data")
parquet_query = bench("parquet_data")

print(f"  .kore    query: {kore_query:.1f}ms")
print(f"  .hkore   query: {hkore_query:.1f}ms")
print(f"  .parquet query: {parquet_query:.1f}ms")

# === FINAL RESULTS ===
FMT_NAMES = (".kore", ".hkore", ".parquet")
WIN_NAMES = ("KORE", "HKORE", "Parquet")

def winner(vals, lower_is_better=True):
    idx = min(range(3), key=lambda i: vals[i]) if lower_is_better else max(range(3), key=lambda i: vals[i])
    return WIN_NAMES[idx]

writes = (kore_write, hkore_write, parquet_write)
reads = (kore_read, hkore_read, parquet_read)
queries = (kore_query, hkore_query, parquet_query)
sizes = (kore_size / 1024, hkore_size / 1024, pq_size / 1024)

BAR = "=" * 72
print("\n" + BAR)
print(f"{'Metric':<22} {FMT_NAMES[0]:>10} {FMT_NAMES[1]:>10} {FMT_NAMES[2]:>10} {'Winner':>12}")
print(BAR)
print(f"{'Write time (ms)':<22} {writes[0]:>10.1f} {writes[1]:>10.1f} {writes[2]:>10.1f} {winner(writes):>12}")
print(f"{'Read->Spark (ms)':<22} {reads[0]:>10.1f} {reads[1]:>10.1f} {reads[2]:>10.1f} {winner(reads):>12}")
print(f"{'SQL query (ms)':<22} {queries[0]:>10.1f} {queries[1]:>10.1f} {queries[2]:>10.1f} {winner(queries):>12}")
print(f"{'File size (KB)':<22} {sizes[0]:>10.0f} {sizes[1]:>10.0f} {sizes[2]:>10.0f} {winner(sizes):>12}")
print(f"{'Human readable':<22} {'YES':>10} {'YES':>10} {'NO':>10} {'KORE/HKORE':>12}")
print(BAR)

spark.stop()
print("\nDone!")
