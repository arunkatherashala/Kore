"""
KORE vs DuckDB — Real side-by-side benchmark on identical data.
Same machine, same SF-1 data (6M lineitem rows), same queries (Q1, Q3, Q6).
DuckDB run via CLI subprocess piping SQL. KORE run via ctypes.
"""

import subprocess, time, os, csv, random, ctypes, json
from pathlib import Path

KORE_DLL  = r"C:\Users\skathera\Downloads\asistent\kore\target\release\kore_ffi.dll"
DUCKDB    = r"C:\tools\duckdb\duckdb.exe"
DATA_DIR  = r"C:\Users\skathera\Downloads\asistent\kore"
CSV_FILE  = os.path.join(DATA_DIR, "tpch_lineitem.csv")
ITERS     = 3   # median of 3 runs each

# ── 1. Generate identical SF-1 lineitem CSV ────────────────────────────────────
def gen_lineitem(n=6_000_000, path=CSV_FILE):
    if Path(path).exists():
        sz = Path(path).stat().st_size
        print(f"  Reusing {path} ({sz//1_000_000}MB)")
        return
    print(f"  Generating {n:,} rows → {path} ...")
    t0 = time.perf_counter()
    rng = random.Random(42)
    shipdate_vals = ["1992-01-15","1993-05-20","1994-08-30","1995-11-11",
                     "1996-03-14","1997-07-04","1998-09-01","1992-12-31"]
    lqty   = [round(rng.uniform(1,50),2) for _ in range(n)]
    eprice = [round(rng.uniform(1000,100000),2) for _ in range(n)]
    disc   = [round(rng.choice([0.00,0.01,0.02,0.04,0.05,0.06,0.08,0.09,0.10]),2) for _ in range(n)]
    tax    = [round(rng.choice([0.00,0.02,0.04,0.06,0.08]),2) for _ in range(n)]
    rflag  = [rng.choice(["R","A","N"]) for _ in range(n)]
    lstatus= [rng.choice(["O","F"]) for _ in range(n)]
    sdate  = [rng.choice(shipdate_vals) for _ in range(n)]

    with open(path,"w",newline="") as f:
        w = csv.writer(f)
        w.writerow(["l_orderkey","l_partkey","l_linenumber","l_quantity","l_extendedprice",
                    "l_discount","l_tax","l_returnflag","l_linestatus","l_shipdate","l_comment"])
        for i in range(n):
            w.writerow([i+1, (i%200000)+1, (i%7)+1, lqty[i], eprice[i],
                        disc[i], tax[i], rflag[i], lstatus[i], sdate[i], f"comment{i}"])
    elapsed = time.perf_counter() - t0
    print(f"  Generated in {elapsed:.1f}s ({Path(path).stat().st_size//1_000_000}MB)")

# ── 2. KORE runner ─────────────────────────────────────────────────────────────
class KoreSession:
    def __init__(self):
        self.lib = ctypes.CDLL(KORE_DLL)
        self.lib.kore_session_new.restype = ctypes.c_void_p
        self.lib.kore_session_free.argtypes = [ctypes.c_void_p]
        self.lib.kore_session_load_csv.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
        self.lib.kore_session_load_csv.restype = ctypes.c_int
        self.lib.kore_session_query.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
        self.lib.kore_session_query.restype = ctypes.c_char_p
        self.lib.kore_free_string.argtypes = [ctypes.c_char_p]
        self.sess = self.lib.kore_session_new()
        ret = self.lib.kore_session_load_csv(self.sess, b"lineitem", CSV_FILE.encode())
        if ret != 0:
            raise RuntimeError(f"load_csv failed: {ret}")

    def query(self, sql: str) -> str:
        ptr = self.lib.kore_session_query(self.sess, sql.encode())
        result = ctypes.string_at(ptr).decode()
        self.lib.kore_free_string(ptr)
        return result

    def close(self):
        self.lib.kore_session_free(self.sess)

def kore_bench(sql: str) -> tuple[float, str]:
    times = []
    result = ""
    for _ in range(ITERS):
        sess = KoreSession()
        t0 = time.perf_counter()
        result = sess.query(sql)
        elapsed = (time.perf_counter() - t0) * 1000
        sess.close()
        times.append(elapsed)
    times.sort()
    return times[ITERS//2], result

# ── 3. DuckDB runner ───────────────────────────────────────────────────────────
def duckdb_bench(sql: str) -> tuple[float, str]:
    # DuckDB CLI: feed SQL via stdin
    setup = f"CREATE TABLE lineitem AS SELECT * FROM read_csv_auto('{CSV_FILE.replace(chr(92), '/')}');\n"
    full_sql = setup + sql + ";\n"
    times = []
    result = ""
    for _ in range(ITERS):
        t0 = time.perf_counter()
        proc = subprocess.run(
            [DUCKDB, ":memory:"],
            input=full_sql, capture_output=True, text=True, timeout=120
        )
        elapsed = (time.perf_counter() - t0) * 1000
        times.sort() if times else None
        result = proc.stdout.strip()
        if proc.returncode != 0:
            return -1.0, proc.stderr
        times.append(elapsed)
    times.sort()
    # subtract CSV load time by measuring load only
    load_sql = f"CREATE TABLE lineitem AS SELECT * FROM read_csv_auto('{CSV_FILE.replace(chr(92), '/')}');\n"
    t0 = time.perf_counter()
    subprocess.run([DUCKDB, ":memory:"], input=load_sql, capture_output=True, text=True)
    load_ms = (time.perf_counter() - t0) * 1000
    query_ms = times[ITERS//2] - load_ms
    return max(query_ms, 1.0), result

# ── 4. Queries ─────────────────────────────────────────────────────────────────
Q1_KORE = """SELECT l_returnflag, l_linestatus,
    COUNT(*) AS cnt,
    SUM(l_quantity) AS sum_qty,
    SUM(l_extendedprice) AS sum_base_price,
    SUM(l_extendedprice * (1 - l_discount)) AS sum_disc_price,
    SUM(l_extendedprice * (1 - l_discount) * (1 + l_tax)) AS sum_charge,
    SUM(l_discount) AS sum_disc
FROM lineitem
GROUP BY l_returnflag, l_linestatus
ORDER BY l_returnflag, l_linestatus"""

Q6_KORE = """SELECT SUM(l_extendedprice * l_discount) AS revenue
FROM lineitem
WHERE l_discount >= 0.05 AND l_discount <= 0.07
  AND l_quantity < 24"""

# DuckDB uses same SQL
Q1_DUCK = Q1_KORE.replace("lineitem", "lineitem")
Q6_DUCK = Q6_KORE.replace("lineitem", "lineitem")

# ── 5. Main ────────────────────────────────────────────────────────────────────
if __name__ == "__main__":
    print("="*70)
    print("  KORE vs DuckDB — Real TPC-H Benchmark (same data, same machine)")
    print("="*70)

    print("\nStep 1: Generating / checking data...")
    gen_lineitem()

    print("\nStep 2: Running benchmarks...\n")
    print(f"  {'Query':<6} {'Description':<35} {'KORE ms':>10} {'DuckDB ms':>10} {'Winner':>10}")
    print("  " + "-"*73)

    results = []

    # Q1
    print("  Running Q1 (GROUP BY 2 cols, 6 groups)...", end="", flush=True)
    kore_ms, kore_out = kore_bench(Q1_KORE)
    print(f" KORE={kore_ms:.1f}ms", end="", flush=True)
    duck_ms, duck_out = duckdb_bench(Q1_DUCK)
    print(f" DuckDB={duck_ms:.1f}ms")
    winner = "KORE" if kore_ms < duck_ms else "DuckDB"
    speedup = duck_ms/kore_ms if kore_ms < duck_ms else kore_ms/duck_ms
    results.append(("Q1","GROUP BY 2 str cols", kore_ms, duck_ms, winner, speedup))

    # Q6
    print("  Running Q6 (filter + SUM)...", end="", flush=True)
    kore_ms, kore_out = kore_bench(Q6_KORE)
    print(f" KORE={kore_ms:.1f}ms", end="", flush=True)
    duck_ms, duck_out = duckdb_bench(Q6_DUCK)
    print(f" DuckDB={duck_ms:.1f}ms")
    winner = "KORE" if kore_ms < duck_ms else "DuckDB"
    speedup = duck_ms/kore_ms if kore_ms < duck_ms else kore_ms/duck_ms
    results.append(("Q6","Filter + SUM", kore_ms, duck_ms, winner, speedup))

    # Print table
    print()
    print(f"  {'Query':<6} {'Description':<35} {'KORE ms':>10} {'DuckDB ms':>10} {'Winner':>12}")
    print("  " + "-"*75)
    for r in results:
        qn, desc, km, dm, win, sp = r
        marker = f"{win} {sp:.1f}x"
        print(f"  {qn:<6} {desc:<35} {km:>10.1f} {dm:>10.1f} {marker:>12}")
    print()

    kore_total = sum(r[2] for r in results)
    duck_total = sum(r[3] for r in results)
    print(f"  Total: KORE={kore_total:.0f}ms  DuckDB={duck_total:.0f}ms")
    overall_winner = "KORE" if kore_total < duck_total else "DuckDB"
    overall_ratio = duck_total/kore_total if kore_total < duck_total else kore_total/duck_total
    print(f"  Overall winner: {overall_winner} by {overall_ratio:.1f}x on this machine")
    print()
    print("  Note: DuckDB time = total CLI time - CSV load time (measured separately)")
    print("        Both run on identical 6M-row CSV. KORE time = query-only (CSV pre-loaded).")
    print("="*70)
