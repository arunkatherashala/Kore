"""Non-TPC-H micro-benchmarks for KORE: sort, DISTINCT, high-cardinality GROUP BY, skewed join, window, string scans.

    python micro_bench.py --gen            # writes data/*.csv (10M-row fact table t, 5M-row w, 100k-row dim)
    python micro_bench.py [--only M1,M5]   # prints min-of-N ms per query plus the first result rows

Queries are wrapped (count/limit) so the FFI JSON stays small. Compare the printed rows by eye / against
another engine (pandas, Spark) when changing operators; timings are noisy by roughly +/-20%.
"""
import argparse
import json
import os
import sys
import time

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
DATA = os.path.join(HERE, "data")


def gen(n=10_000_000, nw=5_000_000):
    import pyarrow as pa
    import pyarrow.csv as pc
    os.makedirs(DATA, exist_ok=True)
    rng = np.random.default_rng(7)
    ids = rng.permutation(n).astype(np.int64)
    k_hi = rng.integers(0, 2_000_000, n)
    k_low = rng.integers(0, 1000, n)
    k_skew = np.minimum(rng.zipf(1.3, n), 100_000).astype(np.int64)   # heavy hitters: key 1 holds a large share
    v = np.round(rng.random(n) * 1000, 2)
    words = np.array(["alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet"])
    s = np.char.add(np.char.add(words[rng.integers(0, 10, n)], "_"), k_hi.astype(str))
    s = np.char.add(s, np.where(rng.random(n) < 0.01, "_special", ""))
    d = (np.datetime64("2000-01-01") + rng.integers(0, 9000, n).astype("timedelta64[D]")).astype("datetime64[D]").astype(str)
    t = pa.table({"id": ids, "k_hi": k_hi, "k_low": k_low, "k_skew": k_skew, "v": v, "s": s, "d": d})
    pc.write_csv(t, os.path.join(DATA, "t.csv"))
    pc.write_csv(t.slice(0, nw).select(["id", "k_low", "v"]), os.path.join(DATA, "w.csv"))
    dk = np.arange(1, 100_001)
    pc.write_csv(pa.table({"k": dk, "grp": dk % 50, "w": np.round(rng.random(len(dk)), 4)}), os.path.join(DATA, "dim.csv"))
    print("generated", n, "rows")


QUERIES = {
    "M1 sort 10M top-5":        "select id, v from t order by v desc, id limit 5",
    "M1b sort 10M full (tail)": "select id, v from t order by v, id limit 5 offset 9999995",
    "M1c sort 10M by string":   "select id from t order by s, id limit 5 offset 9999995",
    "M2 distinct high-card":    "select count(*) as c from (select distinct k_hi from t) x",
    "M2b distinct low-card":    "select count(*) as c from (select distinct k_low from t) x",
    "M2c distinct 2 cols":      "select count(*) as c from (select distinct k_low, k_skew from t) x",
    "M2d count(distinct hi)":   "select count(distinct k_hi) as c from t",
    "M3 groupby 2M keys":       "select count(*) as c, sum(m) as sm from (select k_hi, sum(v) as m, count(*) as n from t group by k_hi) x where n > 5",
    "M3b groupby 2 keys":       "select count(*) as c from (select k_hi, k_low, sum(v) as m from t group by k_hi, k_low) x",
    "M3c groupby string hi":    "select count(*) as c from (select s, count(*) as n from t group by s) x",
    "M3d groupby low + avg":    "select k_low, avg(v) as a, min(v) as lo, max(v) as hi, count(*) as n from t group by k_low order by k_low limit 3",
    "M3e groupby date string":  "select substr(d, 1, 7) as ym, count(*) as n, sum(v) as sv from t group by substr(d, 1, 7) order by ym limit 3",
    "M4 join skewed keys":      "select count(*) as c, sum(t.v * dim.w) as sv from t join dim on t.k_skew = dim.k",
    "M4b left join":            "select count(*) as c, count(dim.k) as m from t left join dim on t.k_hi = dim.k",
    "M4c join + group":         "select dim.grp, count(*) as c, sum(t.v) as sv from t join dim on t.k_skew = dim.k group by dim.grp order by dim.grp limit 3",
    "M5 window row_number 5M":  "select count(*) as c, sum(rn) as s from (select row_number() over (partition by k_low order by v) as rn from w) x",
    "M5b window sum part 5M":   "select count(*) as c, sum(sv) as s from (select sum(v) over (partition by k_low) as sv from w) x",
    "M5c window rank+lag 5M":   "select count(*) as c, sum(r) as s from (select rank() over (partition by k_low order by v) as r, lag(v) over (partition by k_low order by id) as lg from w) x",
    "M5d running sum 5M":       "select count(*) as c, max(rs) as m from (select sum(v) over (order by id rows between unbounded preceding and current row) as rs from w) x",
    "M6 like %x%":              "select count(*) as c from t where s like '%alpha_1%'",
    "M6b like prefix":          "select count(*) as c from t where s like 'delta_9%'",
    "M6c like suffix":          "select count(*) as c from t where s like '%_special'",
    "M6d like a%b%c":           "select count(*) as c from t where s like '%echo%99%special'",
    "M6e upper/length":         "select count(*) as c, sum(length(upper(s))) as l from t where length(s) > 12",
    "M6f substr eq":            "select count(*) as c from t where substr(d, 1, 4) = '2010'",
    "M6g date range string":    "select count(*) as c, sum(v) as sv from t where d >= '2010-01-01' and d < '2011-01-01'",
    "M6h substr in list":       "select count(*) as c from t where substr(s, 1, 5) in ('alpha', 'bravo', 'delta')",
    "M7 case agg":              "select sum(case when v > 500 then 1 else 0 end) as hi, sum(case when k_low < 10 then v else 0 end) as lo from t",
    "M8 scalar agg":            "select count(*) as c, sum(v) as s, avg(v) as a, min(id) as lo, max(id) as hi from t",
    "M9 order by 2 keys top":   "select k_low, v, id from t order by k_low desc, v limit 5",
    "M10 corr subquery":        "select count(*) as c from w where v > (select avg(v) from w w2 where w2.k_low = w.k_low)",
    "M11 union all + group":    "select count(*) as c from (select k_low from t union all select k_low from w) x group by k_low order by c desc limit 3",
}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--gen", action="store_true")
    ap.add_argument("--only", default="", help="comma list of query ids, e.g. M1,M5b")
    ap.add_argument("--repeats", type=int, default=2)
    ap.add_argument("--out", default="")
    a = ap.parse_args()
    if a.gen:
        gen()
        return
    sys.path.insert(0, os.path.join(ROOT, "kore-ffi", "bindings", "python"))
    os.environ.setdefault("KORE_LIB", os.path.join(ROOT, "target", "release", "kore_ffi.dll" if os.name == "nt" else "libkore_ffi.so"))
    import kore  # noqa: E402
    s = kore.KoreSession()
    t0 = time.perf_counter()
    for tb in ("t", "w", "dim"):
        s.load_csv(tb, os.path.join(DATA, tb + ".csv"))
    print(f"load {time.perf_counter() - t0:.1f}s", flush=True)
    only = [x.strip() for x in a.only.split(",") if x.strip()]
    res = {}
    for name, sql in QUERIES.items():
        if only and name.split()[0] not in only:
            continue
        try:
            times, rows = [], None
            for i in range(a.repeats + 1):
                t = time.perf_counter()
                rows = s.query(sql)
                dt = (time.perf_counter() - t) * 1000
                if i > 0:
                    times.append(dt)
            res[name] = {"ms": round(min(times), 1), "rows": rows[:3]}
            print(f"{name:28} {min(times):>10.1f} ms  {json.dumps(rows[:2], default=str)[:110]}", flush=True)
        except Exception as e:  # noqa: BLE001
            res[name] = {"error": str(e)[:200]}
            print(f"{name:28} ERROR {str(e)[:150]}", flush=True)
    if a.out:
        json.dump(res, open(a.out, "w"), default=str)


if __name__ == "__main__":
    main()
