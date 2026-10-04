"""Combined table: Spark, KORE, DuckDB, Polars, DataFusion on the same data and SQL.

    python compare_all.py --sf 1

Reads results/<engine>_cache_sf<sf>.json written by run_engines.py (spark, kore) and run_other_engines.py
(duckdb, polars, datafusion). Spark is the reference: another engine's time only counts as comparable when its
rows equal Spark's (same rule and tolerance as run_engines.py). Times are best-of-N after a warm-up.
"""
import argparse
import json
import math
import os

from run_engines import rows_equal

HERE = os.path.dirname(os.path.abspath(__file__))
ENGINES = ["spark", "kore", "duckdb", "datafusion", "polars"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sf", type=float, required=True)
    ap.add_argument("--results", default=os.path.join(HERE, "results"))
    a = ap.parse_args()
    data, meta = {}, {}
    for e in ENGINES:
        f = os.path.join(a.results, f"{e}_cache_sf{a.sf}.json")
        if os.path.exists(f):
            data[e] = json.load(open(f, encoding="utf-8"))
            meta[e] = data[e].get("_version", e)
    ref = data["spark"]
    names = sorted((q for q in ref if q.startswith("Q")), key=lambda q: int(q[1:]))
    cols = [e for e in ENGINES if e in data]
    head = f"{'query':5}" + "".join(f"{e:>13}" for e in cols)
    lines = [head]
    ok = {e: 0 for e in cols}
    bad = {e: [] for e in cols}
    geo = {e: [] for e in cols}
    best_count = {e: 0 for e in cols}
    for q in names:
        row, times = f"{q:5}", {}
        for e in cols:
            r = data[e].get(q)
            if r is None:
                cell = "-"
            elif r["status"] != "ok":
                cell, _ = "unsupported", bad[e].append(q)
            elif e == "spark":
                cell, times[e] = f"{r['ms']:.0f}", r["ms"]
                ok[e] += 1
                geo[e].append(r["ms"])
            else:
                same, why = rows_equal(r["rows"], ref[q]["rows"])
                if same:
                    cell, times[e] = f"{r['ms']:.0f}", r["ms"]
                    ok[e] += 1
                    geo[e].append(r["ms"])
                else:
                    cell = "MISMATCH"
                    bad[e].append(q)
            row += f"{cell:>13}"
        if times:
            best_count[min(times, key=times.get)] += 1
        lines.append(row)
    lines.append("")
    lines.append("correct (agrees with Spark): " + ", ".join(f"{e} {ok[e]}/{len(names)}" for e in cols))
    for e in cols:
        if bad[e]:
            lines.append(f"  {e} not comparable on: {', '.join(bad[e])}")
    lines.append("fastest on N queries: " + ", ".join(f"{e} {best_count[e]}" for e in cols))
    # geometric mean over queries every engine answered correctly
    common = [q for q in names if all(q in [n for n in names] and data[e].get(q, {}).get("status") == "ok"
                                      and (e == "spark" or rows_equal(data[e][q]["rows"], ref[q]["rows"])[0]) for e in cols)]
    lines.append(f"geometric mean over the {len(common)} queries all engines answered correctly (ms):")
    for e in cols:
        g = math.exp(sum(math.log(data[e][q]["ms"]) for q in common) / len(common)) if common else float("nan")
        lines.append(f"  {e:11} {g:8.1f}")
    lines.append("versions: " + ", ".join(f"{e}={meta[e]}" for e in cols))
    print("\n".join(lines))


if __name__ == "__main__":
    main()
