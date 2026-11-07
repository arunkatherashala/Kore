"""Tables for docs/log_format4.md from the JSON results of run_log_format4.sh.

    python -I log_format4_tables.py RESULT_DIR [PREFIX]

Per size and log format: median over rounds of the per-run median, [min-max over rounds]; paired ratios
divide format 4 by format 1 (or 3) of the SAME round. Also checks that every query hash is equal
across the three formats in every round, and summarises the machine load (typeperf samples).
"""
import glob
import json
import os
import re
import statistics
import sys

d = sys.argv[1]
prefix = sys.argv[2] if len(sys.argv) > 2 else "rdata"
runs = {}
for p in glob.glob(os.path.join(d, f"{prefix}_*_f*_r*.json")):
    m = re.search(r"_(\d+)_f(\d)_r(\d+)\.json$", p)
    n, f, r = int(m.group(1)), int(m.group(2)), int(m.group(3))
    runs[(n, f, r)] = json.load(open(p))

def mid_asof(j):
    ks = sorted(j["asof"], key=int)
    return j["asof"][ks[len(ks) // 2]]

METRICS = [
    ("size, B/event", lambda j, n: j["size_bytes"] / n, "{:.1f}"),
    ("bulk build, events/s (b1000)", lambda j, n: j["build_b1000_events_per_s"]["median"], "{:,.0f}"),
    ("commit, 1 event, ms", lambda j, n: j["append"]["b1_ms_per_commit"]["median"], "{:.2f}"),
    ("commit, 1000 events, ms", lambda j, n: j["append"]["b1000_ms_per_batch"]["median"], "{:.1f}"),
    ("open, s", lambda j, n: j["open_s"]["median"], "{:.3f}"),
    ("open + latest snapshot, s", lambda j, n: j["open_plus_latest_snapshot_s"]["median"], "{:.3f}"),
    ("open_at(mid version), s", lambda j, n: mid_asof(j)["open_at_s"]["median"], "{:.3f}"),
    ("get_event, ms/op (fresh handle)", lambda j, n: j["get_event_per_op_s"]["median"] * 1e3, "{:.3f}"),
    ("verify, s", lambda j, n: j["verify_s"]["median"], "{:.3f}"),
    ("re-ingest of all events (no-op), s", lambda j, n: j["reingest_events"]["seconds"], "{:.2f}"),
]

def fmt_stat(xs, f):
    if not xs:
        return "-"
    return f"{f.format(statistics.median(xs))} [{f.format(min(xs))}-{f.format(max(xs))}]"

def ratio(xs):
    if not xs:
        return "-"
    return f"{statistics.median(xs):.2f} [{min(xs):.2f}-{max(xs):.2f}]"

def checks(j):
    c = {"range": j["range"]["check"], "fullagg": j["fullagg"]["check"], "lookup": j["lookup"]["check"]}
    for k, v in j["asof"].items():
        c["asof" + k] = v["check"]
    return c

sizes = sorted({k[0] for k in runs})
for n in sizes:
    rounds = sorted({k[2] for k in runs if k[0] == n})
    print(f"\n### N={n:,} ({len(rounds)} rounds)\n")
    print("| metric | format 1 | format 3 | format 4 | f4/f1 paired | f4/f3 paired |")
    print("|---|---:|---:|---:|---:|---:|")
    for name, get, f in METRICS:
        cols = []
        for fm in (1, 3, 4):
            cols.append(fmt_stat([get(runs[(n, fm, r)], n) for r in rounds if (n, fm, r) in runs], f))
        r41 = [get(runs[(n, 4, r)], n) / get(runs[(n, 1, r)], n) for r in rounds if (n, 4, r) in runs and (n, 1, r) in runs]
        r43 = [get(runs[(n, 4, r)], n) / get(runs[(n, 3, r)], n) for r in rounds if (n, 4, r) in runs and (n, 3, r) in runs]
        print(f"| {name} | {cols[0]} | {cols[1]} | {cols[2]} | {ratio(r41)} | {ratio(r43)} |")
    j4 = runs[(n, 4, rounds[0])]
    print(f"\nformat 4 dictionaries: {j4.get('dicts_count')} object(s), {j4.get('dicts_bytes'):,} bytes; events payload bytes per column: {j4.get('events_column_bytes_v4_payload')}")
    for fm in (1, 3, 4):
        j = runs[(n, fm, rounds[0])]
        print(f"format {fm} bytes by table: {j['size_by_table']}, files {j['file_count']}")
    same = all(checks(runs[(n, fm, r)]) == checks(runs[(n, 1, r)]) for r in rounds for fm in (3, 4) if (n, fm, r) in runs)
    print(f"query hashes (range, full aggregate, lookup, every AS OF version) equal across formats 1/3/4 in every round: {'MATCH' if same else 'DIFFER'}")
    t = [runs[k].get("tamper") for k in runs if k[0] == n and runs[k].get("tamper")]
    if t:
        by_fmt = {}
        for k in runs:
            if k[0] == n and runs[k].get("tamper"):
                tt = runs[k]["tamper"]
                a = by_fmt.setdefault(k[1], [0, 0, 0, 0])
                a[0] += tt["trials"]; a[1] += tt["detected_by_check"]; a[2] += tt["silent_data_change"]; a[3] += tt["harmless_no_change"]
        print("tamper (random bit flips): " + "; ".join(f"format {fm}: {a[1]}/{a[0]} detected by verify, {a[2]} silent, {a[3]} harmless" for fm, a in sorted(by_fmt.items())))
    ri = [(k[1], runs[k]["reingest_events"]) for k in runs if k[0] == n]
    ok = all(x["new_versions_created"] == 0 and x["events_rows_before"] == x["events_rows_after"] and x["verify_ok"] for _, x in ri)
    print(f"re-ingest of every event creates no version and no row in every run: {ok}")

lp = os.path.join(d, "load.txt")
if os.path.exists(lp):
    vals = [float(x) for x in re.findall(r'"[^"]*","([0-9.]+)"', open(lp).read())]
    if vals:
        print(f"\nmachine load (typeperf total CPU, {len(vals)} samples): mean {statistics.mean(vals):.0f} %, min {min(vals):.0f} %, max {max(vals):.0f} %")
