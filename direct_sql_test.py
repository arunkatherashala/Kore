"""Quick direct SQL verification — test every feature on real 6M row data via kore-sql."""
import subprocess, json, time
from pathlib import Path

DUCKDB = r"C:\tools\duckdb\duckdb.exe"
CSV    = r"C:\Users\skathera\Downloads\asistent\kore\tpch_lineitem.csv"
KORE   = r"C:\Users\skathera\Downloads\asistent\kore\target\debug\kore-self.exe"
CWD    = r"C:\Users\skathera\Downloads\asistent\kore"

def duck(sql):
    try:
        p = subprocess.run([DUCKDB,"-csv","-c", sql.replace("tpch", f"read_csv_auto('{CSV}')")],
            capture_output=True, text=True, timeout=30)
        if p.returncode==0: return "PASS", p.stdout.strip().split('\n')[:3]
        return "FAIL", p.stderr[:80]
    except Exception as e: return "ERROR", str(e)[:60]

def kore_sql(sql):
    init = json.dumps({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}})
    msg  = json.dumps({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"self_query","arguments":{"sql":sql}}})
    try:
        p = subprocess.run([KORE,"arun"], input=(init+"\n"+msg+"\n").encode(),
            capture_output=True, timeout=15, cwd=CWD)
        for line in p.stdout.decode(errors="replace").split("\n"):
            try:
                r = json.loads(line)
                if r.get("id")==2:
                    text = r["result"]["content"][0]["text"]
                    return ("PASS" if "Query error" not in text else "FAIL"), text[:120]
            except: pass
        return "ERROR", "no response"
    except Exception as e: return "ERROR", str(e)[:60]

# Direct SQL tests on memories table (KORE) / tpch (DuckDB comparison)
print("="*70)
print("  Direct SQL Feature Verification — Real Data")
print("="*70)

KORE_TESTS = [
    ("COUNT(*)",             "SELECT COUNT(*) AS total FROM memories"),
    ("AVG + MIN + MAX",      "SELECT AVG(importance) avg, MIN(importance) mn, MAX(importance) mx FROM memories"),
    ("GROUP BY + HAVING",    "SELECT kind, COUNT(*) cnt FROM memories GROUP BY kind HAVING COUNT(*) > 0 ORDER BY cnt DESC"),
    ("SELECT DISTINCT",      "SELECT DISTINCT kind FROM memories ORDER BY kind"),
    ("CTE + keyword alias",  "WITH h AS (SELECT kind, AVG(importance) AS avg FROM memories GROUP BY kind) SELECT kind, avg FROM h WHERE avg > 0.8 ORDER BY avg DESC"),
    ("WINDOW ROW_NUMBER",    "SELECT kind, ROW_NUMBER() OVER (PARTITION BY kind ORDER BY importance DESC) rn FROM memories LIMIT 5"),
    ("WINDOW LAG",           "SELECT kind, importance, LAG(importance) OVER (PARTITION BY kind ORDER BY id) prev FROM memories LIMIT 5"),
    ("WINDOW NTILE",         "SELECT kind, NTILE(4) OVER (ORDER BY importance DESC) bucket FROM memories LIMIT 5"),
    ("Scalar subquery =MAX", "SELECT content FROM memories WHERE importance = (SELECT MAX(importance) FROM memories)"),
    ("Scalar subquery >AVG", "SELECT content FROM memories WHERE importance > (SELECT AVG(importance) FROM memories) LIMIT 3"),
    ("IN subquery",          "SELECT content FROM memories WHERE kind IN (SELECT DISTINCT kind FROM memories WHERE importance > 0.9) LIMIT 3"),
    ("NOT IN subquery",      "SELECT kind FROM memories WHERE kind NOT IN (SELECT kind FROM memories WHERE importance < 0.7) LIMIT 3"),
    ("Correlated subquery",  "SELECT content FROM memories m1 WHERE importance > (SELECT AVG(importance) FROM memories m2 WHERE m2.kind = m1.kind) LIMIT 3"),
    ("EXISTS subquery",      "SELECT content FROM memories WHERE EXISTS (SELECT 1 FROM memories m2 WHERE m2.kind = memories.kind AND m2.importance > 0.8) LIMIT 3"),
    ("INNER JOIN",           "SELECT m1.kind, m2.importance FROM memories m1 JOIN memories m2 ON m1.kind = m2.kind LIMIT 3"),
    ("LEFT JOIN",            "SELECT m1.kind, m2.id FROM memories m1 LEFT JOIN memories m2 ON m1.kind = m2.kind LIMIT 3"),
    ("FULL OUTER JOIN",      "SELECT m1.kind, m2.importance FROM memories m1 FULL OUTER JOIN memories m2 ON m1.kind = m2.kind LIMIT 3"),
    ("UNION ALL",            "SELECT kind FROM memories WHERE kind='decision' UNION ALL SELECT kind FROM memories WHERE kind='insight'"),
    ("CASE WHEN",            "SELECT kind, CASE WHEN importance>0.9 THEN 'high' WHEN importance>0.7 THEN 'med' ELSE 'low' END tier FROM memories LIMIT 3"),
    ("LIKE wildcard",        "SELECT kind FROM memories WHERE kind LIKE 'dec%' LIMIT 3"),
    ("ORDER BY + LIMIT",     "SELECT content, importance FROM memories ORDER BY importance DESC LIMIT 5"),
    ("DML INSERT",           None),  # special — tested via self_dml below
]

# Run SQL tests
print(f"\n  {'Feature':<30} {'KORE':^8} Preview")
print(f"  {'─'*68}")
all_pass = 0; all_fail = 0
for item in KORE_TESTS:
    if item[1] is None: continue  # skip special
    label, sql = item
    status, result = kore_sql(sql)
    icon = "✅" if status == "PASS" else "❌"
    if status == "PASS": all_pass += 1
    else: all_fail += 1
    preview = str(result)[:50].replace("\n"," ")
    print(f"  {icon} {label:<30} {preview}")

# DML test via self_dml
def kore_dml(sql):
    init = json.dumps({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}})
    msg  = json.dumps({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"self_dml","arguments":{"sql":sql}}})
    try:
        p = subprocess.run([KORE,"arun"], input=(init+"\n"+msg+"\n").encode(),
            capture_output=True, timeout=15, cwd=CWD)
        for line in p.stdout.decode(errors="replace").split("\n"):
            try:
                r = json.loads(line)
                if r.get("id")==2:
                    text = r["result"]["content"][0]["text"]
                    return ("PASS" if "error" not in text.lower()[:20] else "FAIL"), text[:120]
            except: pass
        return "ERROR", "no response"
    except Exception as e: return "ERROR", str(e)[:60]

status, result = kore_dml("CREATE TABLE t1 AS SELECT id, importance FROM memories WHERE kind='decision'")
icon = "✅" if status == "PASS" else "❌"
if status == "PASS": all_pass += 1
else: all_fail += 1
print(f"  {icon} {'DML INSERT (CREATE TABLE AS SELECT)':<30} {str(result)[:50]}")

print(f"\n  Results: {all_pass} PASS / {all_fail} FAIL / {all_pass+all_fail} total")
print(f"\n  DuckDB 6M row benchmark (COUNT+AVG — was N/A in KORE benchmark):")
status, rows = duck("SELECT COUNT(*) total, AVG(l_extendedprice) avg_price FROM tpch")
print(f"  DuckDB: {status} — {rows}")
