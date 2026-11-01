"""Build the before/after tables for docs/format_improvement_r2.md.

Usage: python -I format_compare_tables.py RESDIR PREFIX N [N ...]
Reads RESDIR/PREFIX_N_{r1,f2,f3}_rK.json written by run_format_compare_r2.sh.
Each cell: median over rounds of the per-run median, with [min-max] over all per-run values.
"""
import glob
import json
import statistics
import sys

res, prefix, sizes = sys.argv[1], sys.argv[2], [int(x) for x in sys.argv[3:]]
VARS = [("r1", "BEFORE (round-1 binary, format 2)"), ("f2", "AFTER, format 2"), ("f3", "AFTER, format 3 (journal)")]


def load(n, v):
    out = []
    for f in sorted(glob.glob(f"{res}/{prefix}_{n}_{v}_r*.json")):
        out.append(json.load(open(f)))
    return out


def get(d, path):
    for p in path:
        if not isinstance(d, dict) or p not in d:
            return None
        d = d[p]
    return d


def fmt_t(s):
    if s is None:
        return "n/a"
    if s < 1e-6:
        return f"{s*1e9:.0f} ns"
    if s < 1e-3:
        return f"{s*1e6:.1f} us"
    if s < 1:
        return f"{s*1e3:.2f} ms"
    return f"{s:.2f} s"


def cell(runs, path, f=fmt_t):
    meds, lo, hi = [], [], []
    for r in runs:
        st = get(r, path)
        if st is None:
            continue
        if isinstance(st, dict):
            meds.append(st["median"]); lo.append(st["min"]); hi.append(st["max"])
        else:
            meds.append(st); lo.append(st); hi.append(st)
    if not meds:
        return "n/a"
    return f"{f(statistics.median(meds))} [{f(min(lo))}-{f(max(hi))}] (runs={len(meds)})"


def mb(x):
    return f"{x/1e6:.2f} MB"


def rate(x):
    return f"{x:,.0f}/s"


def row(title, n, path, f=fmt_t):
    cells = [cell(load(n, v), path, f) for v, _ in VARS]
    return f"| {title} | " + " | ".join(cells) + " |"


for n in sizes:
    print(f"\n#### N={n:,} ({prefix})\n")
    print("| metric | " + " | ".join(l for _, l in VARS) + " |")
    print("|---|---|---|---|")
    print(row("total size on disk", n, ["size_bytes"], mb))
    for t in ["events", "revisions", "revision_causes", "MANIFEST"]:
        print(row(f"  {t}", n, ["size_by_table", t], mb))
    print(row("bulk build, events/s (batches of 1000)", n, ["build_b1000_events_per_s"], rate))
    print(row("open()", n, ["open_s"]))
    print(row("open() + decode latest snapshot", n, ["open_plus_latest_snapshot_s"]))
    print(row("verify()", n, ["verify_s"]))
    print(row("lookup: has_event (existence)", n, ["lookup", "has_event_existence_only_per_op_s"]))
    print(row("lookup: scan of in-memory snapshot", n, ["lookup", "native_scan_per_op_s"]))
    print(row("lookup: get_event(id) (new API)", n, ["get_event_per_op_s"]))
    print(row("lookup: SQL, registered ctx", n, ["lookup", "sql_registered_ctx_per_op_s"]))
    print(row("append 1 event/commit, per commit", n, ["append", "b1_ms_per_commit"], lambda x: f"{x:.2f} ms"))
    print(row("append 1000-event batch", n, ["append", "b1000_ms_per_batch"], lambda x: f"{x:.2f} ms"))
    print(row("range query, SQL registered ctx", n, ["range", "sql_registered_ctx_s"]))
    print(row("as-of mid version, open_at", n, ["asof", str(max(1, (n // 1000) // 2)), "open_at_s"]))
    # one-shot facts
    print()
    print("| fact | " + " | ".join(l for _, l in VARS) + " |")
    print("|---|---|---|---|")

    def fact(title, fn):
        cells = []
        for v, _ in VARS:
            runs = load(n, v)
            vals = sorted({str(fn(r)) for r in runs}) if runs else ["n/a"]
            cells.append(" / ".join(vals))
        print(f"| {title} | " + " | ".join(cells) + " |")

    fact("file count", lambda r: r.get("file_count"))
    fact("re-ingest same events: new versions", lambda r: get(r, ["reingest_events", "new_versions_created"]))
    fact("re-ingest same events: seconds (median of runs shown individually)", lambda r: f"{get(r, ['reingest_events','seconds']):.2f}" if get(r, ['reingest_events','seconds']) is not None else None)
    fact("revision replay: revisions before->after", lambda r: f"{get(r, ['reingest_revisions_replay','revisions_before'])}->{get(r, ['reingest_revisions_replay','revisions_after'])}")
    fact("revision replay: new versions", lambda r: get(r, ["reingest_revisions_replay", "new_versions"]))
    fact("tamper trials / detected by verify / detected only by open / silent / harmless", lambda r: (lambda t: f"{t['trials']}/{t['detected_by_check']}/{t['detected_only_by_open']}/{t['silent_data_change']}/{t['harmless_no_change']}" if t else None)(r.get("tamper")))
    fact("log_format recorded", lambda r: r.get("log_format", 1))
    # consistency: query checks identical across variants and rounds
    checks = {}
    for v, _ in VARS:
        for r in load(n, v):
            for k, p in [("lookup", ["lookup", "check"]), ("range", ["range", "check"]), ("fullagg", ["fullagg", "check"])]:
                checks.setdefault(k, set()).add(str(get(r, p)))
            for ver, d in (r.get("asof") or {}).items():
                checks.setdefault(f"asof v{ver}", set()).add(d["check"])
    print()
    print("Query-hash equality across all runs and formats: " + ", ".join(f"{k}={'MATCH' if len(s) == 1 else 'MISMATCH ' + str(s)}" for k, s in sorted(checks.items())))
