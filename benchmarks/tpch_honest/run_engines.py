"""Head-to-head TPC-H run: live PySpark vs the KORE SQL engine on identical data, with result checking.

    python run_engines.py --data data/sf0.1 --sf 0.1                 # both engines, all 22 queries
    python run_engines.py --data data/sf0.1 --sf 0.1 --only Q1,Q6    # subset
    python run_engines.py --data data/sf0.1 --sf 0.1 --engines kore  # one engine

Nothing here is hard-coded: Spark is executed, KORE is executed, and every KORE result is compared with
Spark's result set (by column name, floats to 1e-6 relative). A query is reported as one of
  ok        both engines ran and the results agree
  MISMATCH  both ran but the results differ (one of them is wrong)
  error     KORE rejected or failed the query (message shown)
  timeout   KORE exceeded --kore-timeout seconds
"""
import argparse
import json
import math
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
TABLES = ["region", "nation", "supplier", "part", "partsupp", "customer", "orders", "lineitem"]
sys.path.insert(0, HERE)
from queries import queries  # noqa: E402


def norm(v):
    if v is None:
        return None
    if isinstance(v, bool):
        return float(v)
    if isinstance(v, (int, float)):
        return float(v)
    return str(v).strip()


def rows_equal(a, b):
    """Compare two lists of dict rows by lowercase column name; returns (ok, reason)."""
    if len(a) != len(b):
        return False, f"row count {len(a)} vs {len(b)}"
    if not a:
        return True, ""
    ka, kb = [k.lower() for k in a[0]], {k.lower() for k in b[0]}
    if set(ka) != kb:
        return False, f"columns {sorted(ka)} vs {sorted(kb)}"
    cols = sorted(ka)

    def key(row):
        low = {k.lower(): norm(v) for k, v in row.items()}
        return tuple((0, "") if low[c] is None else (1, f"{low[c]:.2f}" if isinstance(low[c], float) else low[c]) for c in cols)

    ra = sorted(({k.lower(): norm(v) for k, v in r.items()} for r in a), key=lambda r: key(r))
    rb = sorted(({k.lower(): norm(v) for k, v in r.items()} for r in b), key=lambda r: key(r))
    for i, (x, y) in enumerate(zip(ra, rb)):
        for c in cols:
            p, q = x[c], y[c]
            if p is None or q is None:
                if p is not q:
                    return False, f"row {i} col {c}: {p!r} vs {q!r}"
            elif isinstance(p, float) and isinstance(q, float):
                if not math.isclose(p, q, rel_tol=1e-6, abs_tol=1e-4):
                    return False, f"row {i} col {c}: {p!r} vs {q!r}"
            elif p != q:
                return False, f"row {i} col {c}: {p!r} vs {q!r}"
    return True, ""


# ───────────────────────────── KORE worker (one query per process, so a hang can be killed) ──────────
def kore_worker(data, sf, qname, repeats):
    sys.path.insert(0, os.path.join(ROOT, "kore-ffi", "bindings", "python"))
    dll = os.path.join(ROOT, "target", "release", "kore_ffi.dll" if os.name == "nt" else "libkore_ffi.so")
    os.environ.setdefault("KORE_LIB", dll)
    import kore  # noqa: E402
    sql = queries(sf)[qname]
    out = {"status": "error"}
    try:
        s = kore.KoreSession()
        t = time.perf_counter()
        for tb in TABLES:
            s.load_csv(tb, os.path.join(data, f"{tb}.csv"))
        out["load_s"] = round(time.perf_counter() - t, 2)
        times, rows = [], None
        for i in range(repeats + 1):  # first run is a warm-up
            t = time.perf_counter()
            rows = s.query(sql)
            dt = (time.perf_counter() - t) * 1000
            if i > 0:
                times.append(dt)
        out.update(status="ok", ms=round(min(times), 1), rows=rows)
    except Exception as e:  # noqa: BLE001 - report whatever the engine said
        out["error"] = f"{type(e).__name__}: {e}"[:300]
    sys.stdout.write("\n@@RESULT@@" + json.dumps(out, default=str) + "\n")


def run_kore(data, sf, names, repeats, timeout):
    res = {}
    for q in names:
        cmd = [sys.executable, os.path.abspath(__file__), "--kore-worker", q, "--data", data, "--sf", str(sf), "--repeats", str(repeats)]
        try:
            p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
            tail = p.stdout.split("@@RESULT@@")
            if len(tail) < 2:
                res[q] = {"status": "error", "error": f"worker crashed (exit {p.returncode}): {(p.stderr or p.stdout)[-200:].strip()}"}
            else:
                res[q] = json.loads(tail[-1])
        except subprocess.TimeoutExpired:
            res[q] = {"status": "timeout"}
        r = res[q]
        print(f"  KORE  {q:4} {r['status']:8} " + (f"{r['ms']:>9.1f} ms" if r["status"] == "ok" else r.get("error", "")[:90]), flush=True)
    return res


# ───────────────────────────── Spark ─────────────────────────────
def run_spark(data, sf, names, repeats):
    from pyspark.sql import SparkSession
    spark = (SparkSession.builder.master(f"local[{os.cpu_count()}]").appName("tpch-honest")
             .config("spark.driver.memory", "6g").config("spark.sql.shuffle.partitions", "16")
             .config("spark.ui.enabled", "false").getOrCreate())
    spark.sparkContext.setLogLevel("ERROR")
    t = time.perf_counter()
    for tb in TABLES:
        df = spark.read.parquet(os.path.join(data, f"{tb}.parquet")).cache()
        df.count()  # materialise the cache so queries measure compute, like KORE's in-memory tables
        df.createOrReplaceTempView(tb)
    load_s = round(time.perf_counter() - t, 2)
    res = {"_load_s": load_s, "_version": spark.version}
    qs = queries(sf)
    for q in names:
        try:
            times, rows = [], None
            for i in range(repeats + 1):
                t = time.perf_counter()
                rows = [r.asDict() for r in spark.sql(qs[q]).collect()]
                dt = (time.perf_counter() - t) * 1000
                if i > 0:
                    times.append(dt)
            res[q] = {"status": "ok", "ms": round(min(times), 1), "rows": rows}
        except Exception as e:  # noqa: BLE001
            res[q] = {"status": "error", "error": str(e)[:200]}
        r = res[q]
        print(f"  Spark {q:4} {r['status']:8} " + (f"{r['ms']:>9.1f} ms" if r["status"] == "ok" else r.get("error", "")[:90]), flush=True)
    spark.stop()
    return res


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", required=True)
    ap.add_argument("--sf", type=float, required=True)
    ap.add_argument("--engines", default="spark,kore")
    ap.add_argument("--only", default="")
    ap.add_argument("--repeats", type=int, default=3)
    ap.add_argument("--kore-timeout", type=int, default=180)
    ap.add_argument("--kore-worker", default="")
    ap.add_argument("--out", default=os.path.join(HERE, "results"))
    a = ap.parse_args()
    data = os.path.abspath(a.data)
    if a.kore_worker:
        return kore_worker(data, a.sf, a.kore_worker, a.repeats)

    names = [q.strip() for q in a.only.split(",") if q.strip()] or list(queries(a.sf))
    engines = a.engines.split(",")
    spark = kore = None
    cache = os.path.join(a.out, f"spark_cache_sf{a.sf}.json")
    if "spark" in engines:
        print("Running Spark (live)...", flush=True)
        spark = run_spark(data, a.sf, names, a.repeats)
        os.makedirs(a.out, exist_ok=True)
        old = json.load(open(cache, encoding="utf-8")) if os.path.exists(cache) else {}
        old.update(spark)  # merge, so a subset run does not lose the other queries
        json.dump(old, open(cache, "w", encoding="utf-8"), default=str)
    elif os.path.exists(cache):
        spark = json.load(open(cache, encoding="utf-8"))
        print(f"Using Spark results cached in {cache} (measured earlier on this machine)", flush=True)
    if "kore" in engines:
        print("Running KORE...", flush=True)
        kore = run_kore(data, a.sf, names, a.repeats, a.kore_timeout)
        os.makedirs(a.out, exist_ok=True)  # cached like the other engines so compare_all.py can use it
        kc = os.path.join(a.out, f"kore_cache_sf{a.sf}.json")
        kold = json.load(open(kc, encoding="utf-8")) if os.path.exists(kc) else {}
        kold.update(kore)
        json.dump(kold, open(kc, "w", encoding="utf-8"), default=str)

    lines = [f"TPC-H-shaped data, SF {a.sf}, {os.cpu_count()} cores, best of {a.repeats} after a warm-up run"]
    if spark:
        lines.append(f"Spark {spark['_version']} local mode, tables cached in memory (cache load {spark['_load_s']}s)")
    lines += ["", f"{'query':5} {'Spark ms':>10} {'KORE ms':>10} {'KORE/Spark':>11}  verdict"]
    summary = {"ok": 0, "MISMATCH": 0, "error": 0, "timeout": 0}
    for q in names:
        s, k = (spark or {}).get(q), (kore or {}).get(q)
        verdict, ratio = "", ""
        if k is not None:
            if k["status"] != "ok":
                verdict = k["status"] + (": " + k.get("error", "")[:70] if k["status"] == "error" else "")
                summary["error" if k["status"] == "error" else "timeout"] += 1
            elif s and s["status"] == "ok":
                same, why = rows_equal(k["rows"], s["rows"])
                verdict = "ok" if same else f"MISMATCH ({why})"
                summary["ok" if same else "MISMATCH"] += 1
                if same:
                    ratio = f"{k['ms'] / s['ms']:.2f}x"
        lines.append(f"{q:5} {('%.1f' % s['ms']) if s and s['status'] == 'ok' else (s['status'] if s else '-'):>10} "
                     f"{('%.1f' % k['ms']) if k and k['status'] == 'ok' else (k['status'] if k else '-'):>10} {ratio:>11}  {verdict}")
    if kore and spark:
        lines += ["", "KORE vs Spark: " + ", ".join(f"{v} {k}" for k, v in summary.items()),
                  "KORE/Spark < 1 means KORE was faster. Only queries whose results agree count as comparable."]
    text = "\n".join(lines)
    print("\n" + text)
    os.makedirs(a.out, exist_ok=True)
    stamp = time.strftime("%Y%m%d_%H%M%S")
    with open(os.path.join(a.out, f"tpch_sf{a.sf}_{stamp}.txt"), "w", encoding="utf-8") as f:
        f.write(text + "\n")


if __name__ == "__main__":
    main()
