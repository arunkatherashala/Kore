r"""
KORE vs Spark -- Format Benchmark
Tests how fast each engine loads and queries data using different file formats:

  Format     KORE load    Spark load
  ------     ---------    ----------
  CSV        kore FFI     spark.read.csv
  Parquet    kore FFI     spark.read.parquet
  .kore      kore FFI     N/A (KORE native binary)

Run with:  %LOCALAPPDATA%\miniconda3\python.exe bench_formats.py
"""

import os, sys, time, csv, random, ctypes, subprocess
from pathlib import Path

if sys.stdout.encoding and sys.stdout.encoding.lower() != 'utf-8':
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    sys.stderr.reconfigure(encoding='utf-8', errors='replace')

# ── 0. Environment ─────────────────────────────────────────────────────────────
os.environ.setdefault("JAVA_HOME",     r"C:\Java\jdk-17")
os.environ.setdefault("HADOOP_HOME",   r"C:\Users\skathera\Downloads\pyspark-local-setup 1\pyspark-local\hadoop")
os.environ.setdefault("SPARK_LOCAL_IP","127.0.0.1")
_conda = os.path.join(os.environ.get("LOCALAPPDATA",""), "miniconda3", "python.exe")
if os.path.exists(_conda):
    os.environ["PYSPARK_PYTHON"]        = _conda
    os.environ["PYSPARK_DRIVER_PYTHON"] = _conda

# ── 1. Paths ───────────────────────────────────────────────────────────────────
KORE_DLL    = r"C:\Users\skathera\Downloads\asistent\kore\target\release\kore_ffi.dll"
DATA_DIR    = r"C:\Users\skathera\Downloads\asistent\kore"
CSV_FILE    = os.path.join(DATA_DIR, "tpch_1m.csv")
PARQUET_FILE= os.path.join(DATA_DIR, "tpch_1m.parquet")
KORE_FILE   = os.path.join(DATA_DIR, "tpch_1m.kore")
DUCKDB_EXE  = r"C:\tools\duckdb\duckdb.exe"
ROWS        = 1_000_000
ITERS       = 3

# ── 2. Generate CSV ────────────────────────────────────────────────────────────
def gen_csv():
    p = Path(CSV_FILE)
    if p.exists():
        print(f"  Reusing CSV ({p.stat().st_size//1_000_000}MB)")
        return
    print(f"  Generating {ROWS:,} rows -> {CSV_FILE} ...")
    t0 = time.perf_counter()
    rng = random.Random(42)
    disc_vals = [0.00,0.01,0.02,0.04,0.05,0.06,0.08,0.09,0.10]
    tax_vals  = [0.00,0.02,0.04,0.06,0.08]
    with open(CSV_FILE, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["l_orderkey","l_partkey","l_linenumber","l_quantity","l_extendedprice",
                    "l_discount","l_tax","l_returnflag","l_linestatus","l_shipdate","l_comment"])
        for i in range(ROWS):
            w.writerow([i+1,(i%200000)+1,(i%7)+1,
                round(rng.uniform(1,50),2), round(rng.uniform(1000,100000),2),
                rng.choice(disc_vals), rng.choice(tax_vals),
                rng.choice(["R","A","N"]), rng.choice(["O","F"]),
                rng.choice(["1992-01-15","1993-05-20","1994-08-30","1995-11-11"]),
                f"c{i}"])
    print(f"  Generated in {time.perf_counter()-t0:.1f}s ({p.stat().st_size//1_000_000}MB)")

# ── 3. KORE FFI setup ──────────────────────────────────────────────────────────
def _make_lib():
    lib = ctypes.CDLL(KORE_DLL)
    lib.kore_session_new.restype       = ctypes.c_void_p
    lib.kore_session_free.argtypes     = [ctypes.c_void_p]
    lib.kore_session_load_csv.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
    lib.kore_session_load_csv.restype  = ctypes.c_int
    lib.kore_session_load_kore.argtypes= [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
    lib.kore_session_load_kore.restype = ctypes.c_int
    lib.kore_session_save_kore.argtypes= [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
    lib.kore_session_save_kore.restype = ctypes.c_int
    lib.kore_session_load_parquet.argtypes=[ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
    lib.kore_session_load_parquet.restype=ctypes.c_int
    lib.kore_session_query.argtypes    = [ctypes.c_void_p, ctypes.c_char_p]
    lib.kore_session_query.restype     = ctypes.c_void_p
    lib.kore_free_string.argtypes      = [ctypes.c_void_p]
    lib.kore_last_error.restype        = ctypes.c_char_p
    return lib

_LIB = None
def get_lib():
    global _LIB
    if _LIB is None: _LIB = _make_lib()
    return _LIB

def kore_query_with_load(load_fn, sql):
    """Returns (load_ms, query_ms)."""
    lib = get_lib()
    times_load, times_query = [], []
    for _ in range(ITERS):
        sess = lib.kore_session_new()
        t0 = time.perf_counter()
        ret = load_fn(lib, sess)
        load_ms = (time.perf_counter() - t0) * 1000
        if ret != 0:
            err = lib.kore_last_error() or b"?"
            lib.kore_session_free(sess)
            raise RuntimeError(f"load failed: {ctypes.string_at(err).decode()}")
        t1 = time.perf_counter()
        ptr = lib.kore_session_query(sess, sql.encode())
        query_ms = (time.perf_counter() - t1) * 1000
        if not ptr:
            lib.kore_session_free(sess)
            raise RuntimeError("query returned null")
        lib.kore_free_string(ptr)
        lib.kore_session_free(sess)
        times_load.append(load_ms)
        times_query.append(query_ms)
    times_load.sort(); times_query.sort()
    return times_load[ITERS//2], times_query[ITERS//2]

def load_csv(lib, sess):
    return lib.kore_session_load_csv(sess, b"lineitem", CSV_FILE.encode())

def load_kore_native(lib, sess):
    return lib.kore_session_load_kore(sess, b"lineitem", KORE_FILE.encode())

def load_parquet(lib, sess):
    return lib.kore_session_load_parquet(sess, b"lineitem", PARQUET_FILE.encode())

# ── 4. Spark setup ─────────────────────────────────────────────────────────────
_SPARK = None
def ensure_spark():
    global _SPARK
    if _SPARK: return _SPARK
    from pyspark.sql import SparkSession
    spark = (SparkSession.builder.master("local[*]").appName("fmt_bench")
             .config("spark.sql.shuffle.partitions","4")
             .config("spark.driver.memory","2g")
             .config("spark.ui.enabled","false").getOrCreate())
    spark.sparkContext.setLogLevel("ERROR")
    _SPARK = spark
    return spark

def spark_query_with_load(load_format, sql, path):
    """Returns (load_ms, query_ms)."""
    spark = ensure_spark()
    times_load, times_query = [], []
    for _ in range(ITERS):
        spark.catalog.clearCache()
        t0 = time.perf_counter()
        if load_format == "csv":
            df = spark.read.option("header","true").option("inferSchema","true").csv(path)
        else:  # parquet
            df = spark.read.parquet(path)  # reads single .parquet file
        df.createOrReplaceTempView("lineitem")
        df.cache(); df.count()
        load_ms = (time.perf_counter() - t0) * 1000

        t1 = time.perf_counter()
        spark.sql(sql).collect()
        query_ms = (time.perf_counter() - t1) * 1000

        times_load.append(load_ms); times_query.append(query_ms)
    times_load.sort(); times_query.sort()
    return times_load[ITERS//2], times_query[ITERS//2]

# ── 5. Queries ─────────────────────────────────────────────────────────────────
Q1 = """SELECT l_returnflag, l_linestatus,
    COUNT(*) AS cnt, SUM(l_quantity) AS sum_qty,
    SUM(l_extendedprice) AS sum_price, SUM(l_discount) AS sum_disc
FROM lineitem GROUP BY l_returnflag, l_linestatus"""

Q6 = """SELECT COUNT(*) AS cnt, SUM(l_discount) AS total_disc
FROM lineitem
WHERE l_discount >= 0.05 AND l_discount <= 0.07 AND l_quantity < 24"""

# ── 6. Main ────────────────────────────────────────────────────────────────────
def main():
    print("=" * 72)
    print("  KORE vs Spark -- Format Benchmark  (1M rows, median of 3 runs)")
    print("  Formats: CSV | Apache Parquet | KORE native (.kore binary)")
    print("=" * 72)

    # ── Step 1: generate CSV ─────────────────────────────────────────
    print("\nStep 1: Data...")
    gen_csv()

    # ── Step 2: generate Parquet via DuckDB ─────────────────────────
    print("\nStep 2: Generating Parquet (via DuckDB)...")
    if not Path(PARQUET_FILE).exists():
        print("  Writing Parquet via DuckDB...", end="", flush=True)
        t0 = time.perf_counter()
        sql_parquet = f"COPY (SELECT * FROM read_csv_auto('{CSV_FILE.replace(chr(92), '/')}', header=true)) TO '{PARQUET_FILE.replace(chr(92), '/')}' (FORMAT PARQUET, COMPRESSION SNAPPY);"
        proc = subprocess.run([DUCKDB_EXE, ":memory:"], input=sql_parquet,
                              capture_output=True, text=True, timeout=120)
        if proc.returncode != 0:
            print(f" FAILED: {proc.stderr[:200]}")
        else:
            sz = Path(PARQUET_FILE).stat().st_size
            print(f" {time.perf_counter()-t0:.1f}s  ({sz//1_000_000}MB)")
    else:
        print(f"  Reusing existing Parquet ({Path(PARQUET_FILE).stat().st_size//1_000_000}MB)")

    # ── Step 3: generate .kore native file ──────────────────────────
    print("\nStep 3: Generating .kore native binary...")
    if not Path(KORE_FILE).exists():
        print("  Loading CSV into KORE + saving .kore...", end="", flush=True)
        t0 = time.perf_counter()
        lib = get_lib()
        sess = lib.kore_session_new()
        ret = lib.kore_session_load_csv(sess, b"lineitem", CSV_FILE.encode())
        if ret != 0:
            print(" FAILED")
        else:
            ret2 = lib.kore_session_save_kore(sess, b"lineitem", KORE_FILE.encode())
            lib.kore_session_free(sess)
            sz = Path(KORE_FILE).stat().st_size if Path(KORE_FILE).exists() else 0
            print(f" {time.perf_counter()-t0:.1f}s  ({sz//1_000_000}MB)")
    else:
        print(f"  Reusing {KORE_FILE} ({Path(KORE_FILE).stat().st_size//1_000_000}MB)")

    # ── Step 4: start Spark ──────────────────────────────────────────
    print("\nStep 4: Starting Spark...")
    spark = ensure_spark()

    # ── Step 5: Benchmark ────────────────────────────────────────────
    print("\nStep 5: Running benchmarks...\n")
    fmt_hdr = f"  {'Engine+Format':<28} {'Load ms':>9} {'Query ms':>9} {'Total ms':>10}"
    sep = "  " + "-" * 58
    for qname, sql in [("Q1 (GROUP BY)", Q1), ("Q6 (Filter+SUM)", Q6)]:
        print(f"  === {qname} ===")
        print(fmt_hdr); print(sep)
        rows = []

        # KORE + CSV
        try:
            ld, qr = kore_query_with_load(load_csv, sql)
            rows.append(("KORE  + CSV",      ld, qr))
        except Exception as e: rows.append(("KORE  + CSV",      -1, -1)); print(f"    KORE CSV ERROR: {e}")

        # KORE + Parquet
        if Path(PARQUET_FILE).exists():
            try:
                ld, qr = kore_query_with_load(load_parquet, sql)
                rows.append(("KORE  + Parquet",  ld, qr))
            except Exception as e: rows.append(("KORE  + Parquet",  -1, -1)); print(f"    KORE Parquet ERROR: {e}")

        # KORE + .kore native
        if Path(KORE_FILE).exists():
            try:
                ld, qr = kore_query_with_load(load_kore_native, sql)
                rows.append(("KORE  + .kore native", ld, qr))
            except Exception as e: rows.append(("KORE  + .kore native", -1, -1)); print(f"    KORE native ERROR: {e}")

        # Spark + CSV
        try:
            ld, qr = spark_query_with_load("csv",     sql, CSV_FILE)
            rows.append(("Spark + CSV",      ld, qr))
        except Exception as e: rows.append(("Spark + CSV",      -1, -1)); print(f"    Spark CSV ERROR: {e}")

        # Spark + Parquet
        if Path(PARQUET_FILE).exists():
            try:
                ld, qr = spark_query_with_load("parquet", sql, PARQUET_FILE)
                rows.append(("Spark + Parquet",  ld, qr))
            except Exception as e: rows.append(("Spark + Parquet", -1, -1)); print(f"    Spark Parquet ERROR: {e}")

        best_total = min(r[1]+r[2] for r in rows if r[1] > 0)
        for name, ld, qr in rows:
            if ld < 0: continue
            total = ld + qr
            marker = f"  <- FASTEST by {total/best_total*100-100:.0f}% faster" if total == best_total else f"  ({total/best_total:.1f}x slower)"
            print(f"  {name:<28} {ld:>9.1f} {qr:>9.1f} {total:>10.1f}{marker}")
        print()

    if _SPARK: _SPARK.stop()
    print("=" * 72)

if __name__ == "__main__":
    main()
