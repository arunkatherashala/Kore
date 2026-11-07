"""Markdown tables for docs/kore_size.md from one run.sh output directory.

  python -I tables.py OUTDIR > tables.md
"""
import glob
import json
import os
import sys

OUT = sys.argv[1]
SETS = {"r": "realistic (format_bench_gen_realistic.py)", "u": "uniform (format_bench_gen.py)"}


def load(p):
    return json.load(open(p)) if os.path.exists(p) else None


def mb(b):
    return f"{b / 1e6:.2f} MB"


def t(s):
    if not isinstance(s, (int, float)):
        return str(s)
    if s >= 1:
        return f"{s:.2f} s"
    if s >= 1e-3:
        return f"{s * 1e3:.1f} ms"
    return f"{s * 1e6:.0f} us"


def st(x):
    if isinstance(x, dict):
        return f"{t(x['median'])} [{t(x['min'])}-{t(x['max'])}]"
    if isinstance(x, (int, float)):
        return t(x)
    return x if x else "not available"


def sizes():
    return sorted({int(os.path.basename(p).split("_")[-1].split(".")[0]) for p in glob.glob(os.path.join(OUT, "store_*.json"))})


def rows_for(s, n):
    """[(name, size_bytes, write, full, proj, lookup, note)]"""
    st_ = load(os.path.join(OUT, f"store_{s}_{n}.json"))
    f1 = load(os.path.join(OUT, f"dl_f1_{s}_{n}.json"))
    f3 = load(os.path.join(OUT, f"dl_f3_{s}_{n}.json"))
    py = load(os.path.join(OUT, f"py_{s}_{n}.json"))
    out = []
    if st_:
        v, d = st_["v4"], st_["default"]
        out.append(("**KORE v4 compact (opt-in, this branch)**", v["size_bytes"], v["write_s"], v["full_read_s"], v["projection_kind_s"], v["lookup_per_op_s"],
                    "segments + MANIFEST + 1 dictionary object; lookup = scan of event_id columns (no index)"))
        if st_.get("v4_max_level"):
            m = st_["v4_max_level"]
            out.append((f"KORE v4 compact, zstd level {m['zstd_level']} (opt-in)", m["size_bytes"], m["write_s"], None, None, None,
                        "same layout and decoder as the row above; one timed write; reads not repeated"))
        out.append(("KORE default writer (6e2e6a7b) = DL log format 1", d["size_bytes"], d["write_s"], d["full_read_s"], d["projection_kind_s"], d["lookup_per_op_s"],
                    f"same files as the format 1 log ({st_['default_rewrite_identical_files']}/{st_['segments']} segments rewritten byte-identical)"))
    if f3:
        out.append(("Digital Life log format 3 (own codec)", f3["size_bytes"], f3["build_s"], f3["full_read_s"], None, f3["lookup_per_op_s"],
                    "write = build via log API (fsync per commit, sha256, manifest); full read = open + latest snapshot; lookup = get_event (in-memory id index); projection not exposed by the API"))
    if f1:
        out.append(("Digital Life log format 1 via log API", f1["size_bytes"], f1["build_s"], f1["full_read_s"], None, f1["lookup_per_op_s"],
                    "same bytes as the KORE default row; times through the log API"))
    if py:
        for name, e in py["formats"].items():
            if isinstance(e, str):
                out.append((name, None, None, None, None, None, e))
            else:
                out.append((name, e["size_bytes"], e["write_s"], e["full_read_s"], e["projection_s"], e["lookup_per_op_s"], e["note"]))
    return out


def main():
    ns = sizes()
    print(f"Run directory: `{OUT}`\n")
    for s, label in SETS.items():
        if not any(os.path.exists(os.path.join(OUT, f"store_{s}_{n}.json")) for n in ns):
            continue
        print(f"### Size, {label} dataset\n")
        per = {n: rows_for(s, n) for n in ns}
        names = [r[0] for r in per[ns[0]]]
        for n in ns[1:]:
            for r in per[n]:
                if r[0] not in names:
                    names.append(r[0])
        print("| format | " + " | ".join(f"N={n:,}" for n in ns) + " |")
        print("|---|" + "---:|" * len(ns))
        for name in names:
            cells = []
            for n in ns:
                r = next((r for r in per[n] if r[0] == name), None)
                if r is None or r[1] is None:
                    cells.append("not measured")
                else:
                    cells.append(f"{mb(r[1])}, {r[1] / n:.1f} B/event")
            print(f"| {name} | " + " | ".join(cells) + " |")
        print()
        for n in ns:
            print(f"#### Times, {label}, N={n:,} (median [min-max]; machine under load)\n")
            print("| format | write | full read | projection (events.kind) | point lookup per op | how |")
            print("|---|---|---|---|---|---|")
            for r in per[n]:
                if r[1] is None:
                    continue
                print(f"| {r[0]} | {st(r[2])} | {st(r[3])} | {st(r[4])} | {st(r[5])} | {r[6]} |")
            print()
        print(f"#### KORE v4 / KORE default, paired ratios ({label}; same repetition, below 1 = v4 faster)\n")
        print("| N | write | full read | projection | lookup |")
        print("|---|---|---|---|---|")
        for n in ns:
            j = load(os.path.join(OUT, f"store_{s}_{n}.json"))
            if not j:
                continue
            p = j["paired_ratio_v4_over_default"]
            f = lambda x: f"{x['median']:.2f} [{x['min']:.2f}-{x['max']:.2f}]"
            print(f"| {n:,} | {f(p['write'])} | {f(p['full_read'])} | {f(p['projection'])} | {f(p['lookup'])} |")
        print()
        print(f"#### KORE v4 per column bytes ({label})\n")
        print("| column | " + " | ".join(f"N={n:,}" for n in ns) + " | codec (transform/stage) at largest N |")
        print("|---|" + "---:|" * len(ns) + "---|")
        js = {n: load(os.path.join(OUT, f"store_{s}_{n}.json")) for n in ns}
        last = js[ns[-1]]
        for c in last["v4"]["columns"]:
            cells = [f"{js[n]['v4']['columns'].get(c, 0):,}" if js[n] else "-" for n in ns]
            print(f"| {c} | " + " | ".join(cells) + f" | {last['v4']['codec_choice'].get(c)} |")
        print(f"| dictionary object | " + " | ".join(f"{js[n]['v4']['dict_object_bytes']:,}" if js[n] else "-" for n in ns) + " | raw text of the first events segment (tail) |")
        print()


main()
