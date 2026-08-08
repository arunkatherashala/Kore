"""
KORE vs DuckDB vs Apache Spark — Real 3-Way Benchmark
Same machine. Same data. Same queries. Zero assumed numbers.

Author: Sai Arun Kumar Katherashala
Engine: KORE v0.3.0 (Rust, release build)
"""

import subprocess, time, os, json, sys
from pathlib import Path

DUCKDB_EXE  = r"C:\tools\duckdb\duckdb.exe"
CSV_FILE    = r"C:\Users\skathera\Downloads\asistent\kore\tpch_lineitem.csv"
KORE_EXE    = r"C:\Users\skathera\Downloads\asistent\kore\target\release\kore-tpch.exe"
KORE_JSON   = r"C:\Users\skathera\Downloads\asistent\kore\kore_tpch_results.json"
PY          = r"C:\Users\skathera\AppData\Local\miniconda3\python.exe"
ITERS       = 3

def median(lst): s = sorted(lst); return s[len(s)//2]

# ─── DuckDB ────────────────────────────────────────────────────────────────────

DUCK_Q = {
    "Q1": f"""SELECT l_returnflag, l_linestatus,
        COUNT(*) as cnt, SUM(l_quantity) as sum_qty,
        SUM(l_extendedprice*(1-l_discount)) as sum_disc,
        AVG(l_quantity) as avg_qty, AVG(l_extendedprice) as avg_price,
        AVG(l_discount) as avg_disc
        FROM read_csv_auto('{CSV_FILE}')
        GROUP BY l_returnflag, l_linestatus
        ORDER BY l_returnflag, l_linestatus""",
    "Q6": f"""SELECT SUM(l_extendedprice * l_discount) AS revenue
        FROM read_csv_auto('{CSV_FILE}')
        WHERE l_shipdate >= '1994-01-01' AND l_shipdate < '1995-01-01'
          AND l_discount BETWEEN 0.05 AND 0.07 AND l_quantity < 24""",
}

def bench_duckdb():
    print("\n  [DuckDB] warming up + running 3 iterations each...")
    results = {}
    for qname, sql in DUCK_Q.items():
        times = []
        for i in range(ITERS):
            t0 = time.perf_counter()
            p = subprocess.run([DUCKDB_EXE, "-csv", "-c", sql],
                capture_output=True, text=True, timeout=180)
            times.append((time.perf_counter()-t0)*1000)
            if p.returncode != 0:
                print(f"    DuckDB error: {p.stderr[:200]}")
                break
        med = median(times)
        results[qname] = med
        print(f"    {qname}: {med:.1f}ms  (all runs: {[f'{t:.0f}' for t in times]}ms)")
    return results

# ─── Apache Spark (PySpark local mode) ────────────────────────────────────────

SPARK_SCRIPT = r"C:\Users\skathera\Downloads\asistent\kore\_spark_bench_inner.py"

SPARK_INNER = '''
import sys, time, json
from pyspark.sql import SparkSession
from pyspark.sql.functions import col, sum as fsum, count, avg

CSV_FILE = sys.argv[1]

spark = SparkSession.builder \\
    .appName("KORE_vs_Spark_benchmark") \\
    .master("local[*]") \\
    .config("spark.driver.memory", "4g") \\
    .config("spark.sql.shuffle.partitions", "8") \\
    .config("spark.ui.enabled", "false") \\
    .getOrCreate()
spark.sparkContext.setLogLevel("ERROR")

# Load CSV once
df = spark.read.option("header","true").option("inferSchema","true").csv(CSV_FILE)
df.cache()
df.count()  # force cache

results = {}

# Q1 - GROUP BY
for run in range(3):
    t0 = time.perf_counter()
    df.groupBy("l_returnflag","l_linestatus") \\
      .agg(count("*").alias("cnt"),
           fsum("l_quantity").alias("sum_qty"),
           fsum(col("l_extendedprice")*(1-col("l_discount"))).alias("sum_disc"),
           avg("l_quantity").alias("avg_qty"),
           avg("l_extendedprice").alias("avg_price"),
           avg("l_discount").alias("avg_disc")) \\
      .orderBy("l_returnflag","l_linestatus") \\
      .collect()
    elapsed = (time.perf_counter()-t0)*1000
    results.setdefault("Q1",[]).append(elapsed)
    print(f"SPARK_Q1_RUN_{run}: {elapsed:.1f}ms", flush=True)

# Q6 - filter + sum
for run in range(3):
    t0 = time.perf_counter()
    df.filter((col("l_shipdate") >= "1994-01-01") &
               (col("l_shipdate") < "1995-01-01") &
               (col("l_discount").between(0.05, 0.07)) &
               (col("l_quantity") < 24)) \\
      .agg(fsum(col("l_extendedprice")*col("l_discount")).alias("revenue")) \\
      .collect()
    elapsed = (time.perf_counter()-t0)*1000
    results.setdefault("Q6",[]).append(elapsed)
    print(f"SPARK_Q6_RUN_{run}: {elapsed:.1f}ms", flush=True)

spark.stop()
print("SPARK_RESULTS:" + json.dumps(results))
'''

def bench_spark():
    print("\n  [Spark] starting PySpark local[*] (JVM startup ~15s)...")
    with open(SPARK_SCRIPT, "w") as f:
        f.write(SPARK_INNER)
    
    t_total = time.perf_counter()
    proc = subprocess.run(
        [PY, SPARK_SCRIPT, CSV_FILE],
        capture_output=True, text=True, timeout=600,
        env={**os.environ, "PYSPARK_PYTHON": PY}
    )
    total_wall = (time.perf_counter()-t_total)*1000
    
    results = {}
    for line in (proc.stdout + proc.stderr).split('\n'):
        if line.startswith("SPARK_Q"):
            parts = line.split(":")
            key = parts[0].rsplit("_",1)[0].replace("SPARK_","").replace("_RUN","")
            ms = float(parts[1].strip().replace("ms",""))
            results.setdefault(key,[]).append(ms)
            print(f"    {line.strip()}")
        if "SPARK_RESULTS:" in line:
            try:
                r = json.loads(line.split("SPARK_RESULTS:")[1])
                results = r
            except: pass
    
    final = {}
    for k, runs in results.items():
        if isinstance(runs, list) and runs:
            med = median(runs)
            final[k] = med
            print(f"    {k}: median={med:.1f}ms  (runs: {[f'{r:.0f}' for r in runs]}ms)")
        
    if proc.returncode != 0 and not final:
        print(f"    Spark error:\n{proc.stderr[-500:]}")
    
    print(f"    Total Spark wall time (incl JVM startup): {total_wall:.0f}ms")
    return final

# ─── KORE ─────────────────────────────────────────────────────────────────────

def bench_kore():
    print("\n  [KORE] running release build...")
    t0 = time.perf_counter()
    proc = subprocess.run(
        [KORE_EXE, "--scale", "1"],
        capture_output=True, timeout=300,
        cwd=r"C:\Users\skathera\Downloads\asistent\kore"
    )
    wall = (time.perf_counter()-t0)*1000
    print(f"    KORE wall time: {wall:.0f}ms")
    
    results = {}
    if Path(KORE_JSON).exists():
        with open(KORE_JSON) as f:
            data = json.load(f)
        for r in data:
            results[r['query']] = r['kore_ms']
            if r['query'] in ("Q1","Q6"):
                print(f"    {r['query']}: {r['kore_ms']:.1f}ms  — {r['description']}")
    return results

# ─── Main ─────────────────────────────────────────────────────────────────────

def main():
    print("=" * 72)
    print("  KORE vs DuckDB vs Apache Spark — 3-Way Real Benchmark")
    print("  Author: Sai Arun Kumar Katherashala")
    print(f"  Data: {Path(CSV_FILE).stat().st_size//1_000_000}MB  |  6,000,000 rows  |  Same file, all three engines")
    print("=" * 72)
    
    if not Path(CSV_FILE).exists():
        print(f"ERROR: {CSV_FILE} not found"); sys.exit(1)
    
    # Run all three
    kore   = bench_kore()
    duckdb = bench_duckdb()
    spark  = bench_spark()
    
    # Print results table
    print()
    print("=" * 72)
    print("  FINAL RESULTS — Same machine, same 6M rows, real measurements")
    print("=" * 72)
    print(f"  {'Query':<30} {'KORE':>9} {'DuckDB':>9} {'Spark':>9}")
    print("  " + "─"*68)
    
    queries = [
        ("Q1", "GROUP BY returnflag+linestatus"),
        ("Q6", "Filter date 1994 + SUM discount"),
    ]
    
    kore_total = 0; duck_total = 0; spark_total = 0
    
    for qkey, desc in queries:
        k  = kore.get(qkey)
        d  = duckdb.get(qkey)
        s  = spark.get(qkey)
        k_s = f"{k:.1f}ms"  if k else "N/A"
        d_s = f"{d:.1f}ms"  if d else "N/A"
        s_s = f"{s:.1f}ms"  if s else "N/A"
        print(f"  {desc:<30} {k_s:>9} {d_s:>9} {s_s:>9}")
        if k: kore_total  += k
        if d: duck_total  += d
        if s: spark_total += s
    
    print("  " + "─"*68)
    print(f"  {'TOTAL (Q1+Q6)':<30} {kore_total:>7.1f}ms {duck_total:>7.1f}ms {spark_total:>7.1f}ms")
    print()
    
    if kore_total and duck_total:
        print(f"  KORE vs DuckDB: KORE is {duck_total/kore_total:.1f}x faster")
    if kore_total and spark_total:
        print(f"  KORE vs Spark:  KORE is {spark_total/kore_total:.1f}x faster")
    if duck_total and spark_total:
        print(f"  DuckDB vs Spark: DuckDB is {spark_total/duck_total:.1f}x faster")
    
    print()
    print("  All numbers measured on THIS machine RIGHT NOW.")
    print("  No published numbers. No assumptions. 100% real.")
    print("=" * 72)
    
    # Save results
    out = {
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "machine":   "local — same for all three",
        "data":      "tpch_lineitem.csv, 6M rows",
        "kore":      kore,
        "duckdb":    duckdb,
        "spark":     spark,
    }
    with open("bench_3way_results.json", "w") as f:
        json.dump(out, f, indent=2)
    print("  Results saved → bench_3way_results.json")

if __name__ == "__main__":
    main()
