r"""
KORE vs Apache Spark (PySpark 4.1.2) -- Real local benchmark.
Same 1M-row TPC-H lineitem dataset, same machine, same queries.

Run with:
  %LOCALAPPDATA%\miniconda3\python.exe bench_spark.py

Environment vars are set inside this script -- no separate activation needed.
"""

import os, sys, time, csv, random, ctypes
from pathlib import Path

# Force UTF-8 output on Windows
if sys.stdout.encoding and sys.stdout.encoding.lower() != 'utf-8':
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    sys.stderr.reconfigure(encoding='utf-8', errors='replace')

# ── 0. Environment (must happen before PySpark import) ────────────────────────
os.environ.setdefault("JAVA_HOME",     r"C:\Java\jdk-17")
os.environ.setdefault("HADOOP_HOME",   r"C:\Users\skathera\Downloads\pyspark-local-setup 1\pyspark-local\hadoop")
os.environ.setdefault("SPARK_LOCAL_IP","127.0.0.1")
_conda = os.path.join(os.environ.get("LOCALAPPDATA",""), "miniconda3", "python.exe")
if os.path.exists(_conda):
    os.environ["PYSPARK_PYTHON"]        = _conda
    os.environ["PYSPARK_DRIVER_PYTHON"] = _conda

# ── 1. Constants ──────────────────────────────────────────────────────────────
KORE_DLL = r"C:\Users\skathera\Downloads\asistent\kore\target\release\kore_ffi.dll"
CSV_FILE = r"C:\Users\skathera\Downloads\asistent\kore\tpch_1m.csv"
ROWS     = 1_000_000
ITERS    = 3   # median of 3 runs

# ── 2. Generate 1M-row CSV ────────────────────────────────────────────────────
def gen_csv():
    p = Path(CSV_FILE)
    if p.exists():
        print(f"  Reusing {CSV_FILE} ({p.stat().st_size // 1_000_000}MB)")
        return
    print(f"  Generating {ROWS:,} rows -> {CSV_FILE} ...")
    t0 = time.perf_counter()
    rng = random.Random(42)
    disc_vals   = [0.00,0.01,0.02,0.04,0.05,0.06,0.08,0.09,0.10]
    tax_vals    = [0.00,0.02,0.04,0.06,0.08]
    flag_vals   = ["R","A","N"]
    status_vals = ["O","F"]
    date_vals   = ["1992-01-15","1993-05-20","1994-08-30","1995-11-11","1996-03-14","1997-07-04"]
    with open(CSV_FILE, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["l_orderkey","l_partkey","l_linenumber","l_quantity","l_extendedprice",
                    "l_discount","l_tax","l_returnflag","l_linestatus","l_shipdate","l_comment"])
        for i in range(ROWS):
            w.writerow([
                i+1, (i%200000)+1, (i%7)+1,
                round(rng.uniform(1,50),2),
                round(rng.uniform(1000,100000),2),
                rng.choice(disc_vals),
                rng.choice(tax_vals),
                rng.choice(flag_vals),
                rng.choice(status_vals),
                rng.choice(date_vals),
                f"c{i}",
            ])
    print(f"  Done in {time.perf_counter()-t0:.1f}s  ({p.stat().st_size//1_000_000}MB)")

# ── 3. KORE wrapper ───────────────────────────────────────────────────────────
def _make_lib():
    lib = ctypes.CDLL(KORE_DLL)
    lib.kore_session_new.restype           = ctypes.c_void_p
    lib.kore_session_free.argtypes         = [ctypes.c_void_p]
    lib.kore_session_load_csv.argtypes     = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
    lib.kore_session_load_csv.restype      = ctypes.c_int
    lib.kore_session_query.argtypes        = [ctypes.c_void_p, ctypes.c_char_p]
    lib.kore_session_query.restype         = ctypes.c_void_p   # use c_void_p to avoid auto-decode
    lib.kore_free_string.argtypes          = [ctypes.c_void_p]
    lib.kore_last_error.restype            = ctypes.c_char_p
    return lib

_KORE_LIB = None

def kore_bench(sql: str) -> tuple:
    global _KORE_LIB
    if _KORE_LIB is None:
        _KORE_LIB = _make_lib()
    lib = _KORE_LIB

    times = []
    result = ""
    for _ in range(ITERS):
        # Load CSV before timing (not counted)
        sess = lib.kore_session_new()
        ret = lib.kore_session_load_csv(sess, b"lineitem", CSV_FILE.encode())
        if ret != 0:
            err = lib.kore_last_error() or b"unknown"
            lib.kore_session_free(sess)
            raise RuntimeError(f"KORE load_csv failed: {ctypes.string_at(err).decode()}")

        # Time only the query
        t0 = time.perf_counter()
        ptr = lib.kore_session_query(sess, sql.encode())
        elapsed_ms = (time.perf_counter() - t0) * 1000

        if not ptr:
            err = lib.kore_last_error() or b"unknown"
            lib.kore_session_free(sess)
            raise RuntimeError(f"KORE query failed: {ctypes.string_at(err).decode()}")

        result = ctypes.string_at(ptr).decode("utf-8", errors="replace")
        lib.kore_free_string(ptr)
        lib.kore_session_free(sess)
        times.append(elapsed_ms)

    times.sort()
    return times[ITERS // 2], result

# ── 4. Spark wrapper ──────────────────────────────────────────────────────────
_SPARK = None
_SPARK_LOADED = False

def _ensure_spark():
    global _SPARK, _SPARK_LOADED
    if _SPARK is not None:
        return _SPARK
    from pyspark.sql import SparkSession
    spark = (SparkSession.builder
             .master("local[*]")
             .appName("kore_bench")
             .config("spark.sql.shuffle.partitions", "4")
             .config("spark.driver.memory", "2g")
             .config("spark.ui.enabled", "false")
             .getOrCreate())
    spark.sparkContext.setLogLevel("ERROR")

    # Pre-load and cache (not timed)
    print("  Loading CSV into Spark + caching...", end="", flush=True)
    t0 = time.perf_counter()
    df = (spark.read.option("header","true")
               .option("inferSchema","true")
               .csv(CSV_FILE))
    df.createOrReplaceTempView("lineitem")
    df.cache()
    df.count()   # force full cache
    print(f" {time.perf_counter()-t0:.1f}s")

    _SPARK = spark
    return spark

def spark_bench(sql: str) -> tuple:
    spark = _ensure_spark()
    times = []
    result = None
    for _ in range(ITERS):
        t0 = time.perf_counter()
        rows = spark.sql(sql).collect()
        times.append((time.perf_counter() - t0) * 1000)
        result = rows
    times.sort()
    return times[ITERS // 2], result

# ── 5. Queries (using only simple SUM/COUNT — no computed cols in aggregation) ─
# Note: KORE's group_by_agg extracts the column name from SUM(col) as a string.
#       SUM(col * expr) would resolve to col_name="" -> null result.
#       These queries use only direct column references inside aggregate functions.

Q1_SQL = """SELECT l_returnflag, l_linestatus,
    COUNT(*) AS cnt,
    SUM(l_quantity) AS sum_qty,
    SUM(l_extendedprice) AS sum_base_price,
    SUM(l_discount) AS sum_disc
FROM lineitem
GROUP BY l_returnflag, l_linestatus"""

Q6_SQL = """SELECT COUNT(*) AS cnt, SUM(l_discount) AS total_disc
FROM lineitem
WHERE l_discount >= 0.05 AND l_discount <= 0.07
  AND l_quantity < 24"""

# ── 6. Main ───────────────────────────────────────────────────────────────────
def main():
    print("=" * 65)
    print("  KORE vs Apache Spark (PySpark 4.1.2) — Real Local Benchmark")
    print(f"  Dataset: {ROWS:,} rows  |  Median of {ITERS} iterations")
    print("=" * 65)

    print("\nStep 1: Preparing data...")
    gen_csv()

    print("\nStep 2: Starting Spark session...")
    try:
        _ensure_spark()
    except Exception as e:
        print(f"  ERROR: Could not start Spark: {e}")
        return

    print("\nStep 3: Running benchmarks...\n")
    hdr = f"  {'Query':<6} {'Description':<34} {'KORE ms':>9} {'Spark ms':>9} {'Winner':>14}"
    sep = "  " + "-" * 74
    print(hdr)
    print(sep)

    results = []
    for qname, desc, sql in [
        ("Q1", "GROUP BY 2 str cols (6 groups)", Q1_SQL),
        ("Q6", "Filter + SUM (no GROUP BY)",     Q6_SQL),
    ]:
        print(f"  {qname}: KORE...", end="", flush=True)
        try:
            kore_ms, kore_out = kore_bench(sql)
            print(f" {kore_ms:.1f}ms", end="", flush=True)
        except Exception as e:
            print(f"\n  KORE ERROR: {e}")
            continue

        print(f"  Spark...", end="", flush=True)
        try:
            spark_ms, _ = spark_bench(sql)
            print(f" {spark_ms:.1f}ms", end="", flush=True)
        except Exception as e:
            print(f"\n  Spark ERROR: {e}")
            continue

        if kore_ms < spark_ms:
            winner = f"KORE  {spark_ms/kore_ms:.1f}x"
        else:
            winner = f"Spark {kore_ms/spark_ms:.1f}x"

        print(f"    -> {winner}")
        results.append((qname, desc, kore_ms, spark_ms))

    if not results:
        return

    # Summary table
    print()
    print(hdr)
    print(sep)
    for qname, desc, km, sm in results:
        if km < sm:
            w = f"KORE  {sm/km:.1f}x"
        else:
            w = f"Spark {km/sm:.1f}x"
        print(f"  {qname:<6} {desc:<34} {km:>9.1f} {sm:>9.1f} {w:>14}")

    kore_total  = sum(r[2] for r in results)
    spark_total = sum(r[3] for r in results)
    print(sep)
    if kore_total < spark_total:
        verdict = f"KORE overall {spark_total/kore_total:.1f}x faster"
    else:
        verdict = f"Spark overall {kore_total/spark_total:.1f}x faster"
    print(f"\n  KORE total: {kore_total:.0f}ms   Spark total: {spark_total:.0f}ms")
    print(f"  => {verdict}")

    print()
    print("  Notes:")
    print("    KORE  : time = query only (CSV pre-loaded into in-memory DataBlock)")
    print("    Spark : time = .collect() on cached DataFrame (data already in RAM)")
    print("    Both engines operate on identical CSV data, same machine.")
    print("=" * 65)

    # Stop Spark cleanly
    if _SPARK:
        _SPARK.stop()

if __name__ == "__main__":
    main()
