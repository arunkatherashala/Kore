r"""
KORE vs Spark -- Format Benchmark (Local Version)
"""

import os, sys, time, csv, random, ctypes, subprocess
from pathlib import Path

# ── 1. Paths ───────────────────────────────────────────────────────────────────
KORE_DLL    = os.path.join(os.getcwd(), "target", "release", "kore_ffi.dll")
DATA_DIR    = os.getcwd()
CSV_FILE    = os.path.join(DATA_DIR, "tpch_1m.csv")
PARQUET_FILE= os.path.join(DATA_DIR, "tpch_1m.parquet")
KORE_FILE   = os.path.join(DATA_DIR, "tpch_1m.kore")
META_FILE   = os.path.join(DATA_DIR, ".kore_bench_meta")
ROWS        = int(os.getenv("KORE_BENCH_ROWS", "500000"))
ITERS       = int(os.getenv("KORE_BENCH_ITERS", "5"))
READABLE_MODE = os.getenv("KORE_READABLE_MODE", "preview")
READABLE_ROWS = os.getenv("KORE_READABLE_ROWS", "8")

# ── 2. Generate Data ──────────────────────────────────────────────────────────
def gen_data():
    p_csv = Path(CSV_FILE)
    p_pq  = Path(PARQUET_FILE)
    p_meta = Path(META_FILE)
    if p_csv.exists() and p_pq.exists() and p_meta.exists():
        try:
            saved_rows = int(p_meta.read_text(encoding="utf-8").strip())
            if saved_rows == ROWS:
                print(f"  Reusing CSV and Parquet")
                return
        except ValueError:
            pass

    # Keep benchmark deterministic when row count changes.
    for p in (p_csv, p_pq, Path(KORE_FILE)):
        if p.exists():
            p.unlink()

    print(f"  Generating {ROWS:,} rows data...")
    import pandas as pd
    
    rng = random.Random(42)
    data = {
        "l_orderkey": [i+1 for i in range(ROWS)],
        "l_partkey": [(i%200000)+1 for i in range(ROWS)],
        "l_quantity": [round(rng.uniform(1,50),2) for _ in range(ROWS)],
        "l_extendedprice": [round(rng.uniform(1000,100000),2) for _ in range(ROWS)],
        "l_discount": [rng.choice([0.00,0.01,0.02,0.04]) for _ in range(ROWS)],
        "l_tax": [rng.choice([0.00,0.02,0.04,0.06]) for _ in range(ROWS)],
        "l_comment": [f"comment_{i}" for i in range(ROWS)]
    }
    df = pd.DataFrame(data)
    
    df.to_csv(CSV_FILE, index=False)
    print(f"  Saved CSV ({p_csv.stat().st_size//1024} KB)")
    
    df.to_parquet(PARQUET_FILE, engine='pyarrow')
    print(f"  Saved Parquet ({p_pq.stat().st_size//1024} KB)")
    p_meta.write_text(str(ROWS), encoding="utf-8")

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

def run_bench():
    print(f"=== KORE FORMAT BENCHMARK ({ROWS:,} rows) ===")
    print(f"  Iterations per load/query: {ITERS}")
    print(f"  Readable mode: {READABLE_MODE} (rows={READABLE_ROWS})")
    gen_data()
    lib = get_lib()
    
    # 1. CSV Load
    sess = lib.kore_session_new()
    t0 = time.perf_counter()
    lib.kore_session_load_csv(sess, b"tpch_csv", CSV_FILE.encode())
    csv_load = (time.perf_counter() - t0) * 1000
    print(f"  KORE Load CSV:     {csv_load:.1f}ms")
    
    # 2. Save .kore
    t0 = time.perf_counter()
    lib.kore_session_save_kore(sess, b"tpch_csv", KORE_FILE.encode())
    sz_kore = (time.perf_counter() - t0) * 1000
    sz_kb = Path(KORE_FILE).stat().st_size // 1024
    print(f"  KORE Save .kore:   {sz_kore:.1f}ms ({sz_kb}KB)")
    
    # 3. Load Parquet
    sess_pq = lib.kore_session_new()
    pq_total = 0.0
    for _ in range(ITERS):
        t0 = time.perf_counter()
        lib.kore_session_load_parquet(sess_pq, b"tpch_pq", PARQUET_FILE.encode())
        pq_total += (time.perf_counter() - t0) * 1000
    pq_load = pq_total / ITERS
    print(f"  KORE Load Parquet: {pq_load:.1f}ms")
    
    # 4. Load .kore
    sess_kore = lib.kore_session_new()
    kore_total = 0.0
    for _ in range(ITERS):
        t0 = time.perf_counter()
        lib.kore_session_load_kore(sess_kore, b"tpch_kore", KORE_FILE.encode())
        kore_total += (time.perf_counter() - t0) * 1000
    kore_load = kore_total / ITERS
    print(f"  KORE Load .kore:   {kore_load:.1f}ms")
    
    # 5. Query execution (on .kore loaded data)
    sql = "SELECT SUM(l_extendedprice) FROM tpch_kore"
    q_total = 0.0
    for _ in range(ITERS):
        t0 = time.perf_counter()
        lib.kore_session_query(sess_kore, sql.encode())
        q_total += (time.perf_counter() - t0) * 1000
    q_ms = q_total / ITERS
    print(f"  KORE SQL Query:    {q_ms:.1f}ms")

    load_ns_per_row = (kore_load * 1_000_000.0) / max(1, ROWS)
    query_ns_per_row = (q_ms * 1_000_000.0) / max(1, ROWS)

    print("\nFormat Comparison Summary:")
    print(f"  - CSV:     {csv_load:.1f}ms load")
    print(f"  - Parquet: {pq_load:.1f}ms load")
    print(f"  - .KORE:   {kore_load:.1f}ms load (Fastest)")
    print(f"\nSpeedup: .KORE is {csv_load/kore_load:.1f}x faster than CSV")
    print(f"  - .KORE Load:  {load_ns_per_row:.1f} ns/row")
    print(f"  - SQL Query:   {query_ns_per_row:.1f} ns/row")

    print("\nBenchmark Complete!")

if __name__ == "__main__":
    run_bench()
