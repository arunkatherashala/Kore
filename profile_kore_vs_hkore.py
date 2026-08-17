"""Profile: fair 3-way comparison of .kore vs .hkore vs .parquet read pipeline.
Splits read into:
  [A] file->native (DataBlock or PyArrow Table) — pure format cost
  [C] native->Python rows                       — format-independent
  [D] rows->Spark DataFrame                      — Pyspark cost, not the format
  [E] native Spark parquet reader                — the honest parquet path
Cold (first call) vs Warm (min-of-3) both measured."""
import sys, os, time
os.environ['HADOOP_HOME'] = 'C:\\hadoop'
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), 'kore-python'))
import kore_fileformat as kore

os.makedirs('C:/tmp', exist_ok=True)
N = 100_000
KORE_PATH    = 'C:/tmp/perf.kore'
HKORE_PATH   = 'C:/tmp/perf.hkore'
PARQUET_PATH = 'C:/tmp/perf.parquet'

BAR = "=" * 84
print(BAR)
print(f"PROFILE: .kore vs .hkore vs .parquet read pipeline (N={N:,} rows)")
print(BAR)

# --- write fresh files ---
print("\n[SETUP] Writing fresh files...")
prices  = [float(i) * 1.5 for i in range(N)]
volumes = list(range(N))
regions = ['East','West','North','South'] * (N // 4)
b = kore.DataBlock()
b.add_column('price',  kore.DataType.F64, prices)
b.add_column('volume', kore.DataType.I64, volumes)
b.add_column('region', kore.DataType.STR, regions)

t0 = time.perf_counter(); kore.write_file(KORE_PATH, b);   kw = (time.perf_counter()-t0)*1000
t0 = time.perf_counter(); kore.write_hybrid(HKORE_PATH, b); hw = (time.perf_counter()-t0)*1000

import pyarrow as pa, pyarrow.parquet as pq
pa_table = pa.table({"price": prices, "volume": volumes, "region": regions})
t0 = time.perf_counter(); pq.write_table(pa_table, PARQUET_PATH); pw = (time.perf_counter()-t0)*1000

k_sz = os.path.getsize(KORE_PATH); h_sz = os.path.getsize(HKORE_PATH); p_sz = os.path.getsize(PARQUET_PATH)
print(f"  .kore    write: {kw:>8.1f}ms  size: {k_sz/1024:>7.1f}KB")
print(f"  .hkore   write: {hw:>8.1f}ms  size: {h_sz/1024:>7.1f}KB")
print(f"  .parquet write: {pw:>8.1f}ms  size: {p_sz/1024:>7.1f}KB")

# --- force FFI reload to expose true cold-start ---
try:
    kore.KoreFFI._lib = None
except Exception:
    pass

# --- COLD read (first call) ---
print("\n[A-cold] file -> native (first call, includes any lib init)")
t0 = time.perf_counter(); k_blk    = kore.read_file(KORE_PATH);    k_cold = (time.perf_counter()-t0)*1000
t0 = time.perf_counter(); h_blk    = kore.read_hybrid(HKORE_PATH); h_cold = (time.perf_counter()-t0)*1000
t0 = time.perf_counter(); pq_table = pq.read_table(PARQUET_PATH);  p_cold = (time.perf_counter()-t0)*1000
print(f"  .kore    cold: {k_cold:>10.2f}ms   (DataBlock)")
print(f"  .hkore   cold: {h_cold:>10.2f}ms   (DataBlock)")
print(f"  .parquet cold: {p_cold:>10.2f}ms   (pyarrow.Table)")

# --- WARM read (min-of-3 after warmup) ---
def timed(fn, warmup=1, runs=3):
    for _ in range(warmup): fn()
    ts=[]
    for _ in range(runs):
        t0=time.perf_counter(); fn(); ts.append((time.perf_counter()-t0)*1000)
    return min(ts)

print("\n[A-warm] file -> native (min-of-3 warm)")
k_warm = timed(lambda: kore.read_file(KORE_PATH))
h_warm = timed(lambda: kore.read_hybrid(HKORE_PATH))
p_warm = timed(lambda: pq.read_table(PARQUET_PATH))
print(f"  .kore    warm: {k_warm:>10.2f}ms")
print(f"  .hkore   warm: {h_warm:>10.2f}ms")
print(f"  .parquet warm: {p_warm:>10.2f}ms")

# --- native -> Python rows ---
def block_to_rows(blk):
    c0,c1,c2 = blk.columns[0].data, blk.columns[1].data, blk.columns[2].data
    return [(c0[i], c1[i], c2[i]) for i in range(blk.num_rows)]

def pqtable_to_rows(t):
    price, volume, region = t['price'], t['volume'], t['region']
    return [(float(price[i].as_py()), int(volume[i].as_py()), str(region[i].as_py())) for i in range(len(t))]

print("\n[C] native -> Python list of tuples")
k_rows_ms = timed(lambda: block_to_rows(k_blk))
h_rows_ms = timed(lambda: block_to_rows(h_blk))
p_rows_ms = timed(lambda: pqtable_to_rows(pq_table))
print(f"  .kore    : {k_rows_ms:>10.2f}ms")
print(f"  .hkore   : {h_rows_ms:>10.2f}ms")
print(f"  .parquet : {p_rows_ms:>10.2f}ms   <-- pyarrow .as_py() per cell is slow")

k_rows = block_to_rows(k_blk); h_rows = block_to_rows(h_blk); p_rows = pqtable_to_rows(pq_table)

# --- Spark setup ---
print("\n[D] Starting Spark...")
from pyspark.sql import SparkSession
from pyspark.sql.types import StructType, StructField, StringType, DoubleType, LongType
spark = SparkSession.builder.master("local[*]").appName("KORE-Profile") \
    .config("spark.ui.enabled","false").config("spark.driver.host","localhost").getOrCreate()
spark.sparkContext.setLogLevel("ERROR")
schema = StructType([
    StructField("price",  DoubleType(), False),
    StructField("volume", LongType(),   False),
    StructField("region", StringType(), False),
])

# --- rows -> Spark DataFrame (Python bridge path) ---
def to_spark(rows):
    df = spark.createDataFrame(rows, schema)
    df.cache().count()
    df.unpersist()

print("[D] Python rows -> Spark DataFrame + cache().count() (Python bridge — 3-way)")
k_spark_ms = timed(lambda: to_spark(k_rows))
h_spark_ms = timed(lambda: to_spark(h_rows))
p_spark_ms = timed(lambda: to_spark(p_rows))
print(f"  .kore    : {k_spark_ms:>10.2f}ms")
print(f"  .hkore   : {h_spark_ms:>10.2f}ms")
print(f"  .parquet : {p_spark_ms:>10.2f}ms")

# --- Native Spark parquet reader (the fair path for parquet) ---
def native_spark_parquet():
    df = spark.read.parquet(PARQUET_PATH)
    df.cache().count()
    df.unpersist()

print("\n[E] NATIVE Spark parquet reader (spark.read.parquet — no Python bridge)")
try:
    native_cold_t0 = time.perf_counter()
    native_spark_parquet()
    p_native_cold = (time.perf_counter() - native_cold_t0) * 1000
    p_native_warm = timed(native_spark_parquet)
    print(f"  .parquet native cold: {p_native_cold:>10.2f}ms")
    print(f"  .parquet native warm: {p_native_warm:>10.2f}ms  <-- honest parquet perf")
    native_ok = True
except Exception as e:
    print(f"  FAILED: {e}")
    p_native_cold = p_native_warm = float('nan')
    native_ok = False

# --- Summary ---
print("\n"+BAR)
print(f"{'Stage':<40} {'.kore':>12} {'.hkore':>12} {'.parquet':>12}")
print(BAR)
def row(label, k, h, p, unit="ms"):
    print(f"{label:<40} {k:>10.2f}{unit} {h:>10.2f}{unit} {p:>10.2f}{unit}")
row("[SETUP] write",                   kw,           hw,           pw)
row("[A-cold] file -> native (1st)",   k_cold,       h_cold,       p_cold)
row("[A-warm] file -> native (steady)", k_warm,      h_warm,       p_warm)
row("[C] native -> Python rows",       k_rows_ms,    h_rows_ms,    p_rows_ms)
row("[D] rows -> Spark (Py bridge)",   k_spark_ms,   h_spark_ms,   p_spark_ms)
print(BAR)
row("TOTAL read (Py bridge, warm)",
    k_warm+k_rows_ms+k_spark_ms,
    h_warm+h_rows_ms+h_spark_ms,
    p_warm+p_rows_ms+p_spark_ms)
print(BAR)

if native_ok:
    print(f"[E] NATIVE Spark parquet reader (warm):   {p_native_warm:.2f}ms   <-- real parquet perf, no Python bridge")
    py_bridge_parquet_total = p_warm+p_rows_ms+p_spark_ms
    print(f"    vs Python-bridge parquet total ({py_bridge_parquet_total:.0f}ms) -> native is {py_bridge_parquet_total/p_native_warm:.0f}x faster")

# --- File sizes ---
print(f"\nFile sizes:")
print(f"  .kore    : {k_sz/1024:>8.1f} KB")
print(f"  .hkore   : {h_sz/1024:>8.1f} KB  ({h_sz/k_sz:.1f}x .kore)")
print(f"  .parquet : {p_sz/1024:>8.1f} KB  ({p_sz/k_sz:.1f}x .kore)")

spark.stop()
print("\nDone!")
