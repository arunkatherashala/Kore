"""Markdown tables for docs/kore_rule_mode.md from the bench_rules + py_side results.

  python -I tables.py RESULT_DIR
"""
import glob
import json
import os
import sys


def nb(b):
    return "not measured" if b is None else f"{b:,}"


def main():
    d = sys.argv[1]
    rows = []
    for f in sorted(glob.glob(os.path.join(d, "store_*.json"))):
        s = json.load(open(f))
        p = os.path.join(d, os.path.basename(f).replace("store_", "py_"))
        py = json.load(open(p)) if os.path.exists(p) else {}
        rows.append((s, py))
    order = {"events": 0, "sensors": 1, "orders": 2, "logs": 3, "sequences": 4, "random": 5}
    rows.sort(key=lambda r: (order.get(r[0]["set"], 9), r[0]["rows"]))

    print("### Size (bytes per value = file bytes / (rows x columns); ratio = v4 default bytes / format bytes)\n")
    print("| dataset | rows x cols | v4 default | v4 level 19 | **rule mode** | rule mode, level 19 | Parquet zstd-22 | CSV + xz-9 | raw buffers + xz-9 |")
    print("|---|---:|---:|---:|---:|---:|---:|---:|---:|")
    for s, py in rows:
        v = s["values"]
        base = s["v4_default"]

        def cell(b, bold=False):
            if b is None:
                return "not measured"
            t = f"{b / v:.3f} B ({base / b:.2f}x)"
            return f"**{t}**" if bold else t
        print(f"| {s['set']} | {s['rows']:,} x {s['cols']} | {cell(base)} | {cell(s['v4_19'])} | {cell(s['rules_default'], True)} | {cell(s['rules_19'])} | "
              f"{cell(py.get('parquet_zstd22'))} | {cell(py.get('csv_xz9'))} | {cell(py.get('raw_xz9'))} |")

    print("\n### Bytes\n")
    print("| dataset | rows | v4 default | v4 level 19 | rule mode | rule mode, level 19 | rule mode / v4 default | Parquet zstd-22 | CSV + xz-9 | raw + xz-9 |")
    print("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    for s, py in rows:
        print(f"| {s['set']} | {s['rows']:,} | {s['v4_default']:,} | {s['v4_19']:,} | {s['rules_default']:,} | {s['rules_19']:,} | "
              f"{s['rules_default'] / s['v4_default']:.4f} | {nb(py.get("parquet_zstd22"))} | {nb(py.get("csv_xz9"))} | {nb(py.get("raw_xz9"))} |")

    print("\n### Speed (median over the repetitions; ratios are paired, same process, alternating order)\n")
    print("| dataset | rows | encode v4 | encode rule mode | encode ratio rule/v4 (per rep) | decode v4 | decode rule mode | decode ratio rule/v4 (per rep) | rule-mode encode MB/s (raw) | rule-mode decode MB/s (raw) |")
    print("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|")

    def med(x):
        x = sorted(x)
        return x[len(x) // 2]
    for s, _ in rows:
        mb = s["raw_bytes"] / 1e6
        er = ", ".join(f"{x:.2f}" for x in s["encode_ratio"])
        dr = ", ".join(f"{x:.2f}" for x in s["decode_ratio"])
        print(f"| {s['set']} | {s['rows']:,} | {med(s['encode_v4_s']) * 1e3:.0f} ms | {med(s['encode_rules_s']) * 1e3:.0f} ms | {s['encode_ratio_median']:.2f} ({er}) | "
              f"{med(s['decode_v4_s']) * 1e3:.1f} ms | {med(s['decode_rules_s']) * 1e3:.1f} ms | {s['decode_ratio_median']:.2f} ({dr}) | "
              f"{mb / med(s['encode_rules_s']):.0f} | {mb / med(s['decode_rules_s']):.0f} |")

    print("\n### Per column, largest size of each dataset (v4 default payload vs the rule that won)\n")
    seen = {}
    for s, _ in rows:
        seen[s["set"]] = s
    for name, s in seen.items():
        print(f"\n**{name}, {s['rows']:,} rows**\n")
        print("| column | v4 payload bytes | rule-mode payload bytes | rule |")
        print("|---|---:|---:|---|")
        for c in s["columns"]:
            rb = c["rule_bytes"]
            print(f"| {c['name']} | {c['v4_bytes']:,} | {"-" if rb is None else format(rb, ",")} | {c['rule'] or 'none (v4 payload kept)'} |")


if __name__ == "__main__":
    main()
