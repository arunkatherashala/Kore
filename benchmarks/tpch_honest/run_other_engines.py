"""Run the 22 queries on DuckDB / Polars / DataFusion with the same data and SQL text as run_engines.py.

    python run_other_engines.py --engine duckdb --data data/sf1 --sf 1
    python run_other_engines.py --engine polars --data data/sf1 --sf 1 --only Q1,Q6

Results go to results/<engine>_cache_sf<sf>.json in the same shape as the Spark cache
({"Qn": {"status": "ok", "ms": ..., "rows": [...]}}), and run_engines.py compares them with Spark and KORE.
Needs a Python environment with the engine installed (it does not need pyspark).
Timing: tables loaded into memory first, one warm-up run, then the minimum of --repeats runs. Rows are materialised
as Python dicts inside the timed region, like Spark's collect() and the JSON KORE returns.
"""
import argparse
import json
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from queries import queries  # noqa: E402

TABLES = ["region", "nation", "supplier", "part", "partsupp", "customer", "orders", "lineitem"]


def make_duckdb(data):
    import duckdb
    con = duckdb.connect(":memory:")
    t = time.perf_counter()
    for tb in TABLES:
        con.execute(f"create table {tb} as select * from read_parquet('{os.path.join(data, tb + '.parquet').replace(chr(92), '/')}')")
    load_s = round(time.perf_counter() - t, 2)

    def run(sql):
        cur = con.execute(sql)
        names = [d[0] for d in cur.description]
        return [dict(zip(names, r)) for r in cur.fetchall()]
    return run, load_s, "duckdb " + duckdb.__version__


def make_polars(data):
    import polars as pl
    ctx = pl.SQLContext()
    t = time.perf_counter()
    for tb in TABLES:
        ctx.register(tb, pl.read_parquet(os.path.join(data, tb + ".parquet")).lazy())
    load_s = round(time.perf_counter() - t, 2)

    def run(sql):
        df = ctx.execute(sql).collect()
        return df.to_dicts()
    return run, load_s, "polars " + pl.__version__


def make_datafusion(data):
    import datafusion
    ctx = datafusion.SessionContext()
    t = time.perf_counter()
    for tb in TABLES:
        ctx.register_parquet(tb, os.path.join(data, tb + ".parquet"))
    load_s = round(time.perf_counter() - t, 2)

    def run(sql):
        return ctx.sql(sql).to_arrow_table().to_pylist()
    return run, load_s, "datafusion " + datafusion.__version__


MAKERS = {"duckdb": make_duckdb, "polars": make_polars, "datafusion": make_datafusion}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--engine", required=True, choices=sorted(MAKERS))
    ap.add_argument("--data", required=True)
    ap.add_argument("--sf", type=float, required=True)
    ap.add_argument("--only", default="")
    ap.add_argument("--repeats", type=int, default=3)
    ap.add_argument("--out", default=os.path.join(HERE, "results"))
    a = ap.parse_args()
    data = os.path.abspath(a.data)
    run, load_s, version = MAKERS[a.engine](data)
    qs = queries(a.sf)
    names = [q.strip() for q in a.only.split(",") if q.strip()] or list(qs)
    res = {"_load_s": load_s, "_version": version}
    for q in names:
        try:
            times, rows = [], None
            for i in range(a.repeats + 1):
                t = time.perf_counter()
                rows = run(qs[q])
                dt = (time.perf_counter() - t) * 1000
                if i > 0:
                    times.append(dt)
            res[q] = {"status": "ok", "ms": round(min(times), 1), "rows": rows}
        except Exception as e:  # noqa: BLE001 - an unsupported query is a result, not a crash
            res[q] = {"status": "error", "error": str(e).replace("\n", " ")[:200]}
        r = res[q]
        print(f"  {a.engine} {q:4} {r['status']:8} " + (f"{r['ms']:>9.1f} ms" if r["status"] == "ok" else r.get("error", "")[:90]), flush=True)
    os.makedirs(a.out, exist_ok=True)
    cache = os.path.join(a.out, f"{a.engine}_cache_sf{a.sf}.json")
    old = json.load(open(cache, encoding="utf-8")) if os.path.exists(cache) else {}
    old.update(res)
    json.dump(old, open(cache, "w", encoding="utf-8"), default=str)
    print(f"wrote {cache}")


if __name__ == "__main__":
    main()
