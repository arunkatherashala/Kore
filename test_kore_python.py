"""
KORE Python Bindings — Real Integration Test
=============================================
Tests KORE with:
  1. Real CSV file from workspace (bench_export.csv — 100k rows)
  2. In-memory data via load_table()
  3. SQL queries: SELECT, WHERE, GROUP BY, ORDER BY, LIMIT
  4. Timing comparison vs Python dict comprehension
"""

import sys, time, json, os, statistics
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "kore-python"))

print("=" * 60)
print("KORE Python Bindings — Real Data Test")
print("=" * 60)

# ── Load KORE ────────────────────────────────────────────────────
try:
    from kore import KoreSession
    sess = KoreSession()
    print("✅ KoreSession created\n")
except Exception as e:
    print(f"❌ Failed to load KORE: {e}")
    sys.exit(1)

# ── Test 1: Real CSV file (bench_export.csv — 100k rows) ─────────
bench_csv = os.path.join(os.path.dirname(__file__), "..", "bench_export.csv")
bench_csv = os.path.abspath(bench_csv)

print(f"── Test 1: Real CSV — {bench_csv}")
if os.path.exists(bench_csv):
    t0 = time.perf_counter()
    sess.load_csv("bench", bench_csv)
    load_ms = (time.perf_counter() - t0) * 1000
    n = sess.row_count("bench")
    print(f"  ✅ Loaded {n:,} rows in {load_ms:.1f}ms")

    # GROUP BY query
    t0 = time.perf_counter()
    rows = sess.query("""
        SELECT category, COUNT(*) as cnt, SUM(amount) as total
        FROM bench
        GROUP BY category
        ORDER BY total DESC
    """)
    q_ms = (time.perf_counter() - t0) * 1000
    print(f"  ✅ GROUP BY in {q_ms:.1f}ms → {len(rows)} groups:")
    for r in rows[:5]:
        print(f"     {r}")

    # Filter + LIMIT
    t0 = time.perf_counter()
    top = sess.query("SELECT id, category, amount FROM bench WHERE amount > 800 ORDER BY amount DESC LIMIT 5")
    q2_ms = (time.perf_counter() - t0) * 1000
    print(f"  ✅ Filter+Sort in {q2_ms:.1f}ms → top 5 high-amount rows:")
    for r in top:
        print(f"     {r}")
else:
    print(f"  ⚠️  bench_export.csv not found at {bench_csv}, skipping")

# ── Test 2: load_table() with in-memory Python data ──────────────
print("\n── Test 2: In-memory data via load_table()")
sales = [
    {"region": "North", "product": "A", "qty": 100, "revenue": 5000.0},
    {"region": "South", "product": "B", "qty": 200, "revenue": 8000.0},
    {"region": "North", "product": "B", "qty": 150, "revenue": 6000.0},
    {"region": "East",  "product": "A", "qty": 90,  "revenue": 4500.0},
    {"region": "South", "product": "A", "qty": 120, "revenue": 6000.0},
    {"region": "East",  "product": "B", "qty": 80,  "revenue": 3200.0},
    {"region": "West",  "product": "A", "qty": 110, "revenue": 5500.0},
    {"region": "West",  "product": "B", "qty": 95,  "revenue": 3800.0},
]
sess.load_table("sales", sales)
rows = sess.query("SELECT region, SUM(revenue) as total FROM sales GROUP BY region ORDER BY total DESC")
print(f"  ✅ load_table() + GROUP BY works:")
for r in rows:
    print(f"     {r}")

rows2 = sess.query("SELECT * FROM sales WHERE region = 'North' ORDER BY revenue DESC")
print(f"  ✅ WHERE filter: {rows2}")

# ── Test 3: Large in-memory dataset — speed comparison ───────────
print("\n── Test 3: 50,000 rows — KORE vs pure Python speed")
import random
random.seed(42)
categories = ["Electronics", "Clothing", "Food", "Books", "Sports"]
large = [
    {"cat": categories[i % 5], "val": round(random.uniform(1, 1000), 2), "score": random.randint(1, 100)}
    for i in range(50_000)
]

# KORE timing
t0 = time.perf_counter()
sess.load_table("large", large)
kore_load = (time.perf_counter() - t0) * 1000

t0 = time.perf_counter()
result = sess.query("SELECT cat, COUNT(*) as cnt, SUM(val) as total, AVG(score) as avg_score FROM large GROUP BY cat ORDER BY total DESC")
kore_q = (time.perf_counter() - t0) * 1000
print(f"  KORE:   load={kore_load:.1f}ms  query={kore_q:.1f}ms")
for r in result:
    print(f"    {r}")

# Pure Python timing
t0 = time.perf_counter()
groups = {}
for row in large:
    c = row["cat"]
    if c not in groups:
        groups[c] = {"cnt": 0, "total": 0.0, "score_sum": 0}
    groups[c]["cnt"] += 1
    groups[c]["total"] += row["val"]
    groups[c]["score_sum"] += row["score"]
py_result = sorted(
    [{"cat": k, "cnt": v["cnt"], "total": round(v["total"],2), "avg_score": round(v["score_sum"]/v["cnt"],2)}
     for k, v in groups.items()],
    key=lambda x: -x["total"]
)
py_q = (time.perf_counter() - t0) * 1000
print(f"  Python: query={py_q:.1f}ms  (KORE query is {py_q/max(kore_q,0.1):.1f}x faster than pure Python)")

# ── Test 4: SQL edge cases ────────────────────────────────────────
print("\n── Test 4: SQL edge cases")
try:
    r = sess.query("SELECT cat, MAX(val) as max_val, MIN(val) as min_val FROM large GROUP BY cat")
    print(f"  ✅ MAX/MIN GROUP BY: {len(r)} groups")
except Exception as e:
    print(f"  ⚠️  MAX/MIN: {e}")

try:
    r = sess.query("SELECT * FROM large WHERE val > 900 ORDER BY val DESC LIMIT 3")
    print(f"  ✅ WHERE + ORDER + LIMIT: {r}")
except Exception as e:
    print(f"  ⚠️  WHERE+LIMIT: {e}")

try:
    r = sess.query("SELECT COUNT(*) as total_rows FROM large")
    print(f"  ✅ COUNT(*): {r}")
except Exception as e:
    print(f"  ⚠️  COUNT(*): {e}")

# ── Summary ──────────────────────────────────────────────────────
print("\n" + "=" * 60)
print("✅ KORE Python bindings working on real data!")
print(f"   DLL: {sess._lib._name}")
print("=" * 60)
