"""Paired ratios for docs/format_improvement_r2.md.

Usage: python -I format_paired_r2.py RESDIR PREFIX N [N ...]
For every round K the AFTER value is divided by the BEFORE value of the SAME round (the variants
run back to back); the cell is the median of those ratios with [min-max] over rounds, so slow
phases of a shared machine cancel to first order. < 1 means AFTER is faster/smaller (for rates,
> 1 is better; marked "/s").
"""
import glob
import json
import statistics
import sys

res, prefix, sizes = sys.argv[1], sys.argv[2], [int(x) for x in sys.argv[3:]]

METRICS = [
    ("bulk build events/s", ["build_b1000_events_per_s", "median"]),
    ("open()", ["open_s", "median"]),
    ("open() + decode latest", ["open_plus_latest_snapshot_s", "median"]),
    ("verify()", ["verify_s", "median"]),
    ("get_event(id)", ["get_event_per_op_s", "median"]),
    ("append 1 event/commit", ["append", "b1_ms_per_commit", "median"]),
    ("append 1000-event batch", ["append", "b1000_ms_per_batch", "median"]),
    ("re-ingest same events (s)", ["reingest_events", "seconds"]),
    ("total size on disk", ["size_bytes"]),
]


def get(d, path):
    for p in path:
        if not isinstance(d, dict) or p not in d:
            return None
        d = d[p]
    return d


def load(n, v, k):
    try:
        return json.load(open(f"{res}/{prefix}_{n}_{v}_r{k}.json"))
    except OSError:
        return None


def asof_mid(d):
    a = d.get("asof") or {}
    if not a:
        return None
    keys = sorted(a, key=int)
    return a[keys[len(keys) // 2]]["open_at_s"]["median"]


def main():
    for n in sizes:
        print(f"#### N={n:,} paired ratios (AFTER / BEFORE, same round)\n")
        print("| metric | f2 / r1 | f3 / r1 | rounds |")
        print("|---|---|---|---|")
        rounds = sorted({int(f.rsplit("_r", 1)[1].split(".")[0]) for f in glob.glob(f"{res}/{prefix}_{n}_r1_r*.json")})
        rows = list(METRICS) + [("as-of mid version, open_at", None)]
        for name, path in rows:
            cells = []
            for v in ("f2", "f3"):
                rs = []
                for k in rounds:
                    b, a = load(n, "r1", k), load(n, v, k)
                    if not b or not a:
                        continue
                    if path is None:
                        x, y = asof_mid(b), asof_mid(a)
                    else:
                        x, y = get(b, path), get(a, path)
                    if x and y:
                        rs.append(y / x)
                if rs:
                    cells.append(f"{statistics.median(rs):.2f} [{min(rs):.2f}-{max(rs):.2f}]")
                else:
                    cells.append("n/a")
            print(f"| {name} | {cells[0]} | {cells[1]} | {len(rounds)} |")
        print()


main()
