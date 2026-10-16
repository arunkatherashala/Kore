"""TPC-DS head-to-head: DuckDB (reference) vs KORE on the 99 stock queries, with answer checking.

    python run_ds.py duckdb --data data/sf0.1 --sf 0.1             # reference answers + timings -> results/
    python run_ds.py kore   --data data/sf0.1 --sf 0.1             # each query in its own KORE process
    python run_ds.py report --sf 0.1                               # classify every query -> results/summary_sf0.1.{json,txt}

Verdicts per query: OK, OK_TIES (ORDER BY..LIMIT whose rows differ only by tie choice: every KORE row exists in the
un-limited DuckDB answer; counted separately, review by hand), WRONG, ERROR (message kept), TIMEOUT, CRASH,
REF_ERROR (DuckDB itself failed). Reference engine is DuckDB (tpcds_answers() has no SF1 answers in duckdb 1.5.6).
Columns are compared by position (KORE is asked for `SELECT * FROM (<q>) AS t(c0,c1,..)`) because TPC-DS result sets
have duplicate/unnamed column names. Numbers compare to 1e-6 relative; strings are stripped; timestamps at midnight
equal dates. KORE timing is of the unwrapped query (one warm-up + best of --repeats).
"""
import argparse
import json
import math
import os
import re
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
TABLES = ["call_center", "catalog_page", "catalog_returns", "catalog_sales", "customer", "customer_address",
          "customer_demographics", "date_dim", "household_demographics", "income_band", "inventory", "item",
          "promotion", "reason", "ship_mode", "store", "store_returns", "store_sales", "time_dim", "warehouse",
          "web_page", "web_returns", "web_sales", "web_site"]


def load_queries():
    q = json.load(open(os.path.join(HERE, "queries.json"), encoding="utf-8"))
    return {k: q[k] for k in sorted(q, key=lambda s: int(s[1:]))}


# ---- documented dialect rewrites, applied to the text given to BOTH engines (see README) ----
REWRITES = []  # list of (name, compiled regex, replacement)


def rewrite(sql):
    for _, rx, rep in REWRITES:
        sql = rx.sub(rep, sql)
    return sql.strip().rstrip(";").strip()


def has_limit(sql):
    return re.search(r"\blimit\s+\d+\s*$", sql.strip().rstrip(";"), re.I) is not None


def strip_limit(sql):
    return re.sub(r"\blimit\s+\d+\s*$", "", sql.strip().rstrip(";"), flags=re.I)


# ---- comparison ----
def norm(v):
    if v is None:
        return None
    if isinstance(v, bool):
        return float(v)
    if isinstance(v, (int, float)):
        return float(v)
    if hasattr(v, "isoformat"):
        v = v.isoformat(sep=" ")
    s = str(v).strip()
    if s.endswith(" 00:00:00"):
        s = s[:-9]
    return s


def _close(p, q):
    if p is None or q is None:
        return p is None and q is None
    if isinstance(p, float) and isinstance(q, float):
        return math.isclose(p, q, rel_tol=1e-6, abs_tol=1e-4)
    return p == q


def _cmp_row(x, y):
    return all(_close(a, b) for a, b in zip(x, y))


def rows_equal(a, b):
    """a, b: lists of lists (positional). Order-insensitive multiset comparison with float tolerance."""
    if len(a) != len(b):
        return False, f"row count {len(a)} (kore) vs {len(b)} (ref)"
    if not a:
        return True, ""
    if len(a[0]) != len(b[0]):
        return False, f"column count {len(a[0])} vs {len(b[0])}"
    ra = [[norm(c) for c in r] for r in a]
    rb = [[norm(c) for c in r] for r in b]

    def key(r):
        return tuple((0, "") if c is None else (1, f"{c:.1f}") if isinstance(c, float) else (2, c) for c in r)

    ra.sort(key=key)
    rb.sort(key=key)
    bad = [i for i, (x, y) in enumerate(zip(ra, rb)) if not _cmp_row(x, y)]
    if not bad:
        return True, ""
    # fallback: greedy multiset matching (sort-order jitter from rounding at tolerance edges)
    from collections import defaultdict
    groups = defaultdict(list)
    for y in rb:
        groups[tuple(c for c in y if not isinstance(c, float))].append(y)
    for x in ra:
        g = groups.get(tuple(c for c in x if not isinstance(c, float)), [])
        for j, y in enumerate(g):
            if _cmp_row(x, y):
                g.pop(j)
                break
        else:
            i = bad[0]
            return False, f"row {i}: {ra[i]!r} vs {rb[i]!r}"[:300]
    return True, ""


# ---- DuckDB reference ----
def run_duckdb(a):
    import duckdb
    qs = load_queries()
    names = [q for q in a.only.split(",") if q] or list(qs)
    con = duckdb.connect(":memory:")
    t = time.perf_counter()
    for tb in TABLES:
        p = os.path.join(os.path.abspath(a.data), tb + ".parquet").replace("\\", "/")
        con.execute(f"create table {tb} as select * from read_parquet('{p}')")
    res = {"_load_s": round(time.perf_counter() - t, 2), "_version": "duckdb " + duckdb.__version__}
    for q in names:
        sql = rewrite(qs[q])
        try:
            times, rows, ncols = [], None, 0
            for i in range(a.repeats + 1):
                t = time.perf_counter()
                cur = con.execute(sql)
                rows = [list(r) for r in cur.fetchall()]
                ncols = len(cur.description)
                dt = (time.perf_counter() - t) * 1000
                if i > 0:
                    times.append(dt)
            r = {"status": "ok", "ms": round(min(times), 1), "rows": rows, "ncols": ncols}
            if has_limit(sql):
                r["full_rows"] = [list(x) for x in con.execute(strip_limit(sql)).fetchall()]
            orig = qs[q].strip().rstrip(";").strip()
            if sql != orig:  # sanity: rewrites must not change DuckDB's own answer
                o = [list(x) for x in con.execute(orig).fetchall()]
                same, why = rows_equal(o, rows)
                r["rewrite_check"] = "same" if same else "DIFFERS: " + why
            res[q] = r
        except Exception as e:  # noqa: BLE001
            res[q] = {"status": "error", "error": str(e).replace("\n", " ")[:300]}
        r = res[q]
        print(f"  duckdb {q:4} {r['status']:6} " + (f"{r['ms']:>9.1f} ms {len(r['rows'])} rows" if r["status"] == "ok" else r["error"][:90]), flush=True)
    os.makedirs(os.path.join(HERE, "results"), exist_ok=True)
    f = os.path.join(HERE, "results", f"duckdb_sf{a.sf}.json")
    old = json.load(open(f, encoding="utf-8")) if os.path.exists(f) else {}
    old.update(res)
    json.dump(old, open(f, "w", encoding="utf-8"), default=str)


# ---- KORE worker: one query per process ----
def kore_worker(a):
    sys.path.insert(0, os.path.join(ROOT, "kore-ffi", "bindings", "python"))
    dll = os.path.join(ROOT, "target", "release", "kore_ffi.dll" if os.name == "nt" else "libkore_ffi.so")
    os.environ.setdefault("KORE_LIB", dll)
    import kore  # noqa: E402
    sql = rewrite(load_queries()[a.kore_worker])
    ncols = int(a.ncols)
    out = {"status": "error"}
    try:
        s = kore.KoreSession()
        t = time.perf_counter()
        for tb in TABLES:
            s.load_csv(tb, os.path.join(a.data, f"{tb}.csv"))
        out["load_s"] = round(time.perf_counter() - t, 2)
        t = time.perf_counter()
        s.query(sql)  # warm-up; also gives the engine's own error message for the unwrapped query
        first = (time.perf_counter() - t) * 1000
        wrapped = f"SELECT * FROM ({sql}) AS t({', '.join('c%d' % i for i in range(ncols))})"
        rows = [[r.get(f"c{i}") for i in range(ncols)] for r in s.query(wrapped)]
        times = []
        for _ in range(a.repeats):
            t = time.perf_counter()
            s.query(sql)
            times.append((time.perf_counter() - t) * 1000)
        out.update(status="ok", ms=round(min(times + [first]), 1), rows=rows)
    except Exception as e:  # noqa: BLE001
        out["error"] = f"{type(e).__name__}: {e}"[:400]
    sys.stdout.write("\n@@RESULT@@" + json.dumps(out, default=str) + "\n")


def run_kore(a):
    qs = load_queries()
    names = [q for q in a.only.split(",") if q] or list(qs)
    ref = json.load(open(os.path.join(HERE, "results", f"duckdb_sf{a.sf}.json"), encoding="utf-8"))
    res = {}
    for q in names:
        ncols = ref.get(q, {}).get("ncols", 1)
        cmd = [sys.executable, os.path.abspath(__file__), "kore", "--kore-worker", q, "--ncols", str(ncols),
               "--data", os.path.abspath(a.data), "--sf", str(a.sf), "--repeats", str(a.repeats)]
        try:
            p = subprocess.run(cmd, capture_output=True, text=True, timeout=a.timeout)
            tail = p.stdout.split("@@RESULT@@")
            if len(tail) < 2:
                res[q] = {"status": "crash", "error": f"exit {p.returncode}: {(p.stderr or p.stdout)[-300:].strip()}"}
            else:
                res[q] = json.loads(tail[-1])
        except subprocess.TimeoutExpired:
            res[q] = {"status": "timeout"}
        r = res[q]
        print(f"  KORE {q:4} {r['status']:8} " + (f"{r['ms']:>9.1f} ms {len(r['rows'])} rows" if r["status"] == "ok" else r.get("error", "")[:100]), flush=True)
    f = os.path.join(HERE, "results", f"kore_sf{a.sf}.json")
    old = json.load(open(f, encoding="utf-8")) if os.path.exists(f) else {}
    old.update(res)
    json.dump(old, open(f, "w", encoding="utf-8"), default=str)


# ---- classify ----
def report(a):
    d = os.path.join(HERE, "results")
    ref = json.load(open(os.path.join(d, f"duckdb_sf{a.sf}.json"), encoding="utf-8"))
    kore = json.load(open(os.path.join(d, f"kore_sf{a.sf}.json"), encoding="utf-8"))
    out, counts, lines = {}, {}, []
    for q in load_queries():
        r, k = ref.get(q), kore.get(q)
        if r is None or k is None:
            continue
        v, note = "", ""
        if r["status"] != "ok":
            v, note = "REF_ERROR", r["error"][:80]
        elif k["status"] == "timeout":
            v = "TIMEOUT"
        elif k["status"] == "crash":
            v, note = "CRASH", k.get("error", "")[:100]
        elif k["status"] != "ok":
            v, note = "ERROR", k.get("error", "")[:140]
        else:
            same, why = rows_equal(k["rows"], r["rows"])
            if same:
                v = "OK"
            elif "full_rows" in r and len(k["rows"]) == len(r["rows"]):
                full = [[norm(c) for c in x] for x in r["full_rows"]]
                kr = [[norm(c) for c in x] for x in k["rows"]]
                ok = all(any(_cmp_row(x, y) for y in full) for x in kr)
                v, note = ("OK_TIES", why) if ok else ("WRONG", why)
            else:
                v, note = "WRONG", why
        counts[v] = counts.get(v, 0) + 1
        out[q] = {"verdict": v, "note": note, "kore_ms": k.get("ms"), "ref_ms": r.get("ms"), "rows": len(r.get("rows", []))}
        lines.append(f"{q:4} {v:9} ref {r.get('ms', '-')!s:>8} kore {k.get('ms', '-')!s:>8} rows {out[q]['rows']:>5}  {note}")
    comp = [x for x in out.values() if x["verdict"] in ("OK", "OK_TIES") and x["kore_ms"] and x["ref_ms"]]
    if comp:
        def g(xs):
            return math.exp(sum(math.log(max(x, 0.1)) for x in xs) / len(xs))
        lines.append(f"\n{counts}\ngeomean over {len(comp)} agreeing queries: KORE {g([x['kore_ms'] for x in comp]):.1f} ms, "
                     f"DuckDB {g([x['ref_ms'] for x in comp]):.1f} ms")
    text = "\n".join(lines)
    print(text)
    json.dump(out, open(os.path.join(d, f"summary_sf{a.sf}.json"), "w"), indent=1)
    open(os.path.join(d, f"summary_sf{a.sf}.txt"), "w", encoding="utf-8").write(text + "\n")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["duckdb", "kore", "report"])
    ap.add_argument("--data", default="")
    ap.add_argument("--sf", type=float, required=True)
    ap.add_argument("--only", default="")
    ap.add_argument("--repeats", type=int, default=3)
    ap.add_argument("--timeout", type=int, default=180)
    ap.add_argument("--kore-worker", default="")
    ap.add_argument("--ncols", default="1")
    a = ap.parse_args()
    if a.mode == "kore" and a.kore_worker:
        return kore_worker(a)
    {"duckdb": run_duckdb, "kore": run_kore, "report": report}[a.mode](a)


if __name__ == "__main__":
    main()
