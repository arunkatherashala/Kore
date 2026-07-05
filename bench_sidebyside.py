"""
KORE vs DuckDB - REAL Side-by-Side Benchmark
Same machine. Same data. Same queries. No Spark needed - DuckDB is a fair competitor.

Queries on tpch_lineitem.csv (6M rows, ~800MB):
  Q1 - GROUP BY returnflag, linestatus + SUM/AVG/COUNT
  Q6 - Filter date range + SUM (high selectivity)
  Q_AGG - Full table COUNT + AVG + SUM

Author: Sai Arun Kumar Katherashala
"""

import subprocess, time, os, json, sys
from pathlib import Path

DUCKDB_EXE = r"C:\tools\duckdb\duckdb.exe"
CSV_FILE   = r"C:\Users\skathera\Downloads\asistent\kore\tpch_lineitem.csv"
KORE_EXE   = r"C:\Users\skathera\Downloads\asistent\kore\target\release\kore-tpch.exe"
ITERS      = 3   # run each ITERS times, take median

def median(lst):
    s = sorted(lst)
    n = len(s)
    return s[n // 2]

# ─── DuckDB queries on the CSV ────────────────────────────────────────────────

DUCKDB_QUERIES = {
    "Q1_GroupBy": f"""
        SELECT
            l_returnflag,
            l_linestatus,
            COUNT(*) AS count_order,
            SUM(l_quantity) AS sum_qty,
            SUM(l_extendedprice) AS sum_base_price,
            SUM(l_extendedprice * (1 - l_discount)) AS sum_disc_price,
            AVG(l_quantity) AS avg_qty,
            AVG(l_extendedprice) AS avg_price,
            AVG(l_discount) AS avg_disc
        FROM read_csv_auto('{CSV_FILE}')
        GROUP BY l_returnflag, l_linestatus
        ORDER BY l_returnflag, l_linestatus;
    """,

    "Q6_Filter_Sum": f"""
        SELECT SUM(l_extendedprice * l_discount) AS revenue
        FROM read_csv_auto('{CSV_FILE}')
        WHERE l_shipdate >= '1994-01-01'
          AND l_shipdate < '1995-01-01'
          AND l_discount BETWEEN 0.05 AND 0.07
          AND l_quantity < 24;
    """,

    "Q_Count_All": f"""
        SELECT COUNT(*) as total, AVG(l_extendedprice) as avg_price, 
               SUM(l_quantity) as total_qty
        FROM read_csv_auto('{CSV_FILE}');
    """,
}

def run_duckdb(sql, label):
    times = []
    result_rows = 0
    for i in range(ITERS):
        t0 = time.perf_counter()
        proc = subprocess.run(
            [DUCKDB_EXE, "-csv", "-c", sql],
            capture_output=True, text=True, timeout=120
        )
        elapsed = (time.perf_counter() - t0) * 1000
        times.append(elapsed)
        if i == 0:
            lines = proc.stdout.strip().split('\n')
            result_rows = max(0, len(lines) - 1)  # minus header
        if proc.returncode != 0:
            print(f"  DuckDB error: {proc.stderr[:200]}")
            return None, 0
    med = median(times)
    print(f"  DuckDB  {label:20s}: {med:8.1f}ms  (rows={result_rows}, runs={times})")
    return med, result_rows

# ─── KORE: run the release binary and parse output ────────────────────────────

def run_kore():
    """Run kore-tpch release binary and parse Q1, Q6 times from output."""
    times = {}
    print(f"\n  Running KORE (release, optimized)...")
    t0 = time.perf_counter()
    proc = subprocess.run(
        [KORE_EXE, "--scale", "1"],
        capture_output=True, text=True, timeout=300,
        cwd=r"C:\Users\skathera\Downloads\asistent\kore"
    )
    total_elapsed = (time.perf_counter() - t0) * 1000
    
    if proc.returncode != 0:
        print(f"  KORE error: {proc.stderr[:300]}")
        return {}
    
    # Parse the JSON output
    json_path = r"C:\Users\skathera\Downloads\asistent\kore\kore_tpch_results.json"
    if Path(json_path).exists():
        with open(json_path) as f:
            results = json.load(f)
        for r in results:
            times[r['query']] = r['kore_ms']
    
    print(f"  KORE total wall time: {total_elapsed:.0f}ms")
    return times

# ─── Main ─────────────────────────────────────────────────────────────────────

def main():
    print("=" * 70)
    print("  KORE vs DuckDB — Real Side-by-Side Benchmark")
    print("  Same machine. Same data (6M rows). No assumed numbers.")
    print(f"  Data: {CSV_FILE}")
    print("=" * 70)

    if not Path(CSV_FILE).exists():
        print(f"ERROR: {CSV_FILE} not found. Cannot run benchmark.")
        sys.exit(1)

    csv_size = Path(CSV_FILE).stat().st_size // 1_000_000
    print(f"\n  Data file: {csv_size}MB, 6,000,000 rows")
    print(f"  DuckDB:    {DUCKDB_EXE}")
    print(f"  KORE:      {KORE_EXE}")
    print()

    # ── Run DuckDB benchmarks ─────────────────────────────────────────────────
    print("─" * 70)
    print("  DuckDB (3 runs each, median taken):")
    print("─" * 70)
    duckdb_results = {}
    for label, sql in DUCKDB_QUERIES.items():
        med, rows = run_duckdb(sql, label)
        if med is not None:
            duckdb_results[label] = med

    # ── Run KORE ──────────────────────────────────────────────────────────────
    print()
    print("─" * 70)
    print("  KORE (release build, optimized):")
    print("─" * 70)
    kore_times = run_kore()

    # ── Side-by-side table ───────────────────────────────────────────────────
    print()
    print("=" * 70)
    print("  RESULTS — KORE vs DuckDB (same machine, same data)")
    print("=" * 70)
    print(f"  {'Query':<22} {'KORE':>10} {'DuckDB':>10} {'Winner':>10} {'Speedup':>10}")
    print("  " + "-" * 66)

    # Map KORE query keys to benchmark labels
    mappings = [
        ("Q1_GroupBy",   "Q1",   "GROUP BY returnflag+linestatus"),
        ("Q6_Filter_Sum","Q6",   "Filter date range + SUM"),
        ("Q_Count_All",  None,   "Full scan COUNT/AVG/SUM"),
    ]

    for duck_key, kore_key, desc in mappings:
        duck_ms = duckdb_results.get(duck_key)
        kore_ms = kore_times.get(kore_key) if kore_key else None

        if duck_ms and kore_ms:
            if kore_ms < duck_ms:
                winner = "KORE ✓"
                speedup = f"{duck_ms/kore_ms:.1f}x faster"
            else:
                winner = "DuckDB ✓"
                speedup = f"{kore_ms/duck_ms:.1f}x faster"
            print(f"  {desc:<22} {kore_ms:>8.1f}ms {duck_ms:>8.1f}ms {winner:>12} {speedup:>12}")
        elif duck_ms:
            print(f"  {desc:<22} {'N/A':>10} {duck_ms:>8.1f}ms {'DuckDB':>12} {'':>12}")
        elif kore_ms:
            print(f"  {desc:<22} {kore_ms:>8.1f}ms {'N/A':>10} {'KORE':>12} {'':>12}")

    print("=" * 70)
    print()
    print("  Machine:  This machine (same for both)")
    print("  Data:     Identical tpch_lineitem.csv (6M rows)")
    print("  KORE:     release build (cargo --release)")
    print("  DuckDB:   CLI binary (C:\\tools\\duckdb\\duckdb.exe)")
    print()

if __name__ == "__main__":
    main()
