"""
KORE Full Validation Suite
Tests: Unit tests, SQL features (26 features), Parquet LOAD TABLE, Benchmarks
"""
import subprocess, json, time, sys, os
from pathlib import Path

KORE  = r"C:\Users\skathera\Downloads\asistent\kore\target\debug\kore-self.exe"
CWD   = r"C:\Users\skathera\Downloads\asistent\kore"
DUCKDB= r"C:\tools\duckdb\duckdb.exe"
PQ    = r"C:\Users\skathera\Downloads\asistent\kore\tpch_1m.parquet"

PASS = "[PASS]"; FAIL = "[FAIL]"; SKIP = "[SKIP]"
results = []

def check(label, ok, detail=""):
    icon = PASS if ok else FAIL
    results.append((label, ok))
    print(f"  {icon} {label:<45} {detail}")
    return ok

def kore_dml(sql):
    init = json.dumps({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}})
    msg  = json.dumps({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"self_dml","arguments":{"sql":sql}}})
    try:
        p = subprocess.run([KORE,"arun"], input=(init+"\n"+msg+"\n").encode(), capture_output=True, timeout=15, cwd=CWD)
        for line in p.stdout.decode(errors="replace").split("\n"):
            try:
                r = json.loads(line)
                if r.get("id")==2:
                    text = r["result"]["content"][0]["text"]
                    return "error" not in text.lower()[:20], text[:200]
            except: pass
        return False, "no response"
    except Exception as e: return False, str(e)[:80]

def kore_sql(sql):
    init = json.dumps({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}})
    msg  = json.dumps({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"self_query","arguments":{"sql":sql}}})
    try:
        p = subprocess.run([KORE,"arun"], input=(init+"\n"+msg+"\n").encode(), capture_output=True, timeout=15, cwd=CWD)
        for line in p.stdout.decode(errors="replace").split("\n"):
            try:
                r = json.loads(line)
                if r.get("id")==2:
                    text = r["result"]["content"][0]["text"]
                    return "Query error" not in text, text[:200]
            except: pass
        return False, "no response"
    except Exception as e: return False, str(e)[:80]

print("="*70)
print("  KORE VALIDATION SUITE")
print("="*70)

# ── 1. BINARY EXISTS ─────────────────────────────────────────────────────────
print("\n  [1] Binary Checks")
check("kore-self debug binary",    Path(KORE).exists(),   KORE)
check("kore-tpch release binary",  Path(CWD+"/target/release/kore-tpch.exe").exists())
check("DuckDB binary",             Path(DUCKDB).exists(), DUCKDB)
check("tpch_lineitem.csv (427MB)", Path(CWD+"/tpch_lineitem.csv").exists())
check("tpch_1m.parquet",           Path(PQ).exists(),     PQ)
check("kore_tpch_results.json",    Path(CWD+"/kore_tpch_results.json").exists())

# ── 2. SQL FEATURES ──────────────────────────────────────────────────────────
print("\n  [2] SQL Feature Tests (26 features, memories table)")
sql_tests = [
    ("COUNT(*)",             "SELECT COUNT(*) total FROM memories",                        False),
    ("AVG/MIN/MAX alias",    "SELECT AVG(importance) avg, MIN(importance) mn FROM memories",False),
    ("GROUP BY + HAVING",    "SELECT kind, COUNT(*) cnt FROM memories GROUP BY kind HAVING COUNT(*) > 0",False),
    ("SELECT DISTINCT",      "SELECT DISTINCT kind FROM memories ORDER BY kind",            False),
    ("CTE + keyword alias",  "WITH h AS (SELECT kind, AVG(importance) AS avg FROM memories GROUP BY kind) SELECT kind, avg FROM h WHERE avg > 0.5",False),
    ("ROW_NUMBER OVER",      "SELECT kind, ROW_NUMBER() OVER (PARTITION BY kind ORDER BY importance DESC) rn FROM memories LIMIT 3",False),
    ("LAG OVER",             "SELECT kind, LAG(importance) OVER (PARTITION BY kind ORDER BY id) prev FROM memories LIMIT 3",False),
    ("Scalar subquery",      "SELECT content FROM memories WHERE importance = (SELECT MAX(importance) FROM memories)",False),
    ("IN subquery",          "SELECT content FROM memories WHERE kind IN (SELECT DISTINCT kind FROM memories WHERE importance > 0.8) LIMIT 3",False),
    ("EXISTS subquery",      "SELECT content FROM memories WHERE EXISTS (SELECT 1 FROM memories m2 WHERE m2.kind=memories.kind AND m2.importance>0.8) LIMIT 3",False),
    ("Correlated subquery",  "SELECT content FROM memories m1 WHERE importance > (SELECT AVG(importance) FROM memories m2 WHERE m2.kind=m1.kind) LIMIT 2",False),
    ("INNER JOIN",           "SELECT m1.kind, m2.importance FROM memories m1 JOIN memories m2 ON m1.kind=m2.kind LIMIT 3",False),
    ("LEFT JOIN",            "SELECT m1.kind, m2.id FROM memories m1 LEFT JOIN memories m2 ON m1.kind=m2.kind LIMIT 3",False),
    ("FULL OUTER JOIN",      "SELECT m1.kind, m2.importance FROM memories m1 FULL OUTER JOIN memories m2 ON m1.kind=m2.kind LIMIT 3",False),
    ("UNION ALL",            "SELECT kind FROM memories WHERE kind='decision' UNION ALL SELECT kind FROM memories WHERE kind='insight'",False),
    ("CASE WHEN",            "SELECT kind, CASE WHEN importance>0.9 THEN 'high' ELSE 'low' END tier FROM memories LIMIT 3",False),
    ("LIKE wildcard",        "SELECT kind FROM memories WHERE kind LIKE 'dec%' LIMIT 3",   False),
    ("ORDER BY + LIMIT",     "SELECT content, importance FROM memories ORDER BY importance DESC LIMIT 5",False),
    ("DML INSERT SELECT",    "INSERT INTO val_ins SELECT id,content FROM memories WHERE kind='insight'",True),
    ("DML CREATE TABLE AS",  "CREATE TABLE val_t1 AS SELECT id,importance FROM memories WHERE kind='decision'",True),
    ("ROLLUP",               "SELECT kind, SUM(importance) s FROM memories GROUP BY ROLLUP(kind)",False),
    ("INTERSECT",            "SELECT kind FROM memories WHERE importance>0.8 INTERSECT SELECT kind FROM memories WHERE importance>0.5",False),
    ("EXCEPT",               "SELECT kind FROM memories EXCEPT SELECT kind FROM memories WHERE importance<0.5",False),
    ("DATE NOW()",           "SELECT NOW() today FROM memories LIMIT 1",False),
    ("YEAR/MONTH/DAY",       "SELECT id, YEAR(created_at) yr, MONTH(created_at) mo FROM memories LIMIT 1",False),
    ("MERGE",                "MERGE INTO memories USING memories src ON memories.id=src.id WHEN MATCHED THEN UPDATE SET importance=src.importance",True),
]
for label, sql, is_dml in sql_tests:
    ok, detail = (kore_dml(sql) if is_dml else kore_sql(sql))
    check(label, ok, detail[:60] if not ok else "")

# ── 3. PARQUET LOAD TABLE ────────────────────────────────────────────────────
print("\n  [3] Parquet LOAD TABLE (new feature)")
if Path(PQ).exists():
    # Load table AND query it in the same subprocess session (multi-message)
    init = json.dumps({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}})
    load_msg = json.dumps({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"self_dml","arguments":{"sql":f"LOAD TABLE tpch_pq FROM '{PQ}'"}}})
    sel_msg  = json.dumps({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"self_query","arguments":{"sql":"SELECT COUNT(*) total FROM tpch_pq"}}})
    try:
        p = subprocess.run([KORE,"arun"],
            input=(init+"\n"+load_msg+"\n"+sel_msg+"\n").encode(),
            capture_output=True, timeout=20, cwd=CWD)
        out = p.stdout.decode(errors="replace")
        load_ok = False; count_ok = False
        for line in out.split("\n"):
            try:
                r = json.loads(line)
                if r.get("id")==2:
                    t = r["result"]["content"][0]["text"]
                    load_ok = "error" not in t.lower()[:20]
                if r.get("id")==3:
                    t = r["result"]["content"][0]["text"]
                    count_ok = "Query error" not in t
            except: pass
        check("LOAD TABLE FROM .parquet", load_ok, "loaded OK" if load_ok else "load failed")
        check("SELECT COUNT(*) after LOAD (same session)", count_ok, "count OK" if count_ok else "session persistence issue")
    except Exception as e:
        check("LOAD TABLE FROM .parquet", False, str(e)[:80])
        check("SELECT COUNT(*) after LOAD", False, "exception")
else:
    check("LOAD TABLE FROM .parquet", False, "tpch_1m.parquet not found")
    check("SELECT COUNT(*) after LOAD", False, "no parquet file")

# ── 4. KORE store LOAD ───────────────────────────────────────────────────────
print("\n  [4] Native .kore persistence")
kore_path = CWD + "/test_persist.kore"
if Path(kore_path).exists():
    ok, detail = kore_dml(f"LOAD TABLE kore_tbl FROM '{kore_path}'")
    check("LOAD TABLE FROM .kore", ok, detail[:80] if not ok else "loaded OK")
else:
    check("LOAD TABLE FROM .kore", None, "test_persist.kore not found — skip")

# ── 5. DELTA / ACID ──────────────────────────────────────────────────────────
print("\n  [5] ACID Delta")
ok, detail = kore_dml("CREATE TABLE delta_val AS SELECT id, importance FROM memories WHERE importance > 0.9")
check("ACID: CREATE TABLE AS SELECT", ok, detail[:60] if not ok else "")

# ── 6. BENCHMARK SANITY ──────────────────────────────────────────────────────
print("\n  [6] Benchmark Sanity (KORE results exist)")
try:
    with open(CWD+"/kore_tpch_results.json") as f:
        bench = json.load(f)
    q1 = next((r for r in bench if r["query"]=="Q1"), None)
    q6 = next((r for r in bench if r["query"]=="Q6"), None)
    check("Q1 GROUP BY < 50ms",    q1 and q1["kore_ms"] < 50,   f"{q1['kore_ms']:.1f}ms" if q1 else "missing")
    check("Q6 Filter+SUM < 100ms", q6 and q6["kore_ms"] < 100,  f"{q6['kore_ms']:.1f}ms" if q6 else "missing")
    check("Q1 beats Spark",        q1 and q1["kore_ms"] < q1["spark_ms"], f"{q1['kore_ms']:.1f}ms vs {q1['spark_ms']:.0f}ms" if q1 else "")
except Exception as e:
    check("Benchmark JSON readable", False, str(e))

# ── 7. DuckDB COMPARISON ─────────────────────────────────────────────────────
print("\n  [7] DuckDB comparison query")
csv = CWD + "/tpch_lineitem.csv"
if Path(DUCKDB).exists() and Path(csv).exists():
    t0 = time.perf_counter()
    p = subprocess.run([DUCKDB,"-csv","-c",f"SELECT COUNT(*) FROM read_csv_auto('{csv}')"], capture_output=True, text=True, timeout=30)
    ms = (time.perf_counter()-t0)*1000
    ok = p.returncode==0 and "6000000" in p.stdout
    check("DuckDB COUNT 6M rows", ok, f"{ms:.0f}ms, result={p.stdout.strip()[:30]}")
else:
    check("DuckDB COUNT 6M rows", False, "DuckDB or CSV not found")

# ── SUMMARY ──────────────────────────────────────────────────────────────────
print("\n" + "="*70)
passed = sum(1 for _,ok in results if ok is True)
failed = sum(1 for _,ok in results if ok is False)
total  = len([r for r in results if r[1] is not None])
print(f"  TOTAL: {passed} PASS  /  {failed} FAIL  /  {total} tests")
if failed == 0:
    print(f"\n  ✅  ALL VALIDATIONS PASSED — KORE is fully functional!")
else:
    print(f"\n  ❌  {failed} validation(s) failed — check above")
print("="*70)
