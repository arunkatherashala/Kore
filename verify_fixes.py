"""Quick verification: just SQL features section with fixes applied."""
import subprocess, json, os, sys
sys.path.insert(0, r"C:\Users\skathera\Downloads\asistent\kore")

# Import and run only the SQL feature section
exec(open(r"C:\Users\skathera\Downloads\asistent\kore\run_all_tests.py").read().split("def final_verdict")[0])

P, F, W = "✅", "❌", "⚠️ "
SPARK_SC = r"C:\Users\skathera\Downloads\asistent\kore\_spark_all_tests.py"
PY_MC    = r"C:\Users\skathera\AppData\Local\miniconda3\python.exe"
CSV      = r"C:\Users\skathera\Downloads\asistent\kore\tpch_lineitem.csv"

print("Running Spark SQL feature tests...")
p = subprocess.run([PY_MC, SPARK_SC, CSV], capture_output=True, text=True, timeout=300,
        env={**os.environ,"PYSPARK_PYTHON":PY_MC})
spark_res = {}
for line in (p.stdout+p.stderr).split('\n'):
    if line.startswith("SPARK_TEST:"):
        pts = line.split(":")
        spark_res[pts[1]] = pts[2]

rows = run_sql_features()

print(f"\n  KORE:   {sum(1 for r in rows if r[1]==P)} PASS / {sum(1 for r in rows if r[2]==P)} DuckDB PASS / {sum(1 for r in rows if r[3]==P)} Spark PASS")
print(f"  KORE fails: {[r[0] for r in rows if r[1]==F]}")
print(f"  DuckDB fails: {[r[0] for r in rows if r[2]==F]}")
print(f"  Spark fails: {[r[0] for r in rows if r[3]==F]}")
