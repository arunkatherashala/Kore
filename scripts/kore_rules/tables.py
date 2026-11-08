"""Markdown tables for docs/kore_rule_mode.md from the bench_rules, py_side, py_baselines and sweep results.

  python -I tables.py RESULT_DIR [SWEEP.json]
"""
import glob
import json
import os
import sys

ORDER = ["dump_nyc_taxi_2024_01", "orders", "logs", "sensors", "dump_tpcds_store_sales", "dump_tpcds_web_sales", "dump_tpcds_customer",
         "dump_tpcds_customer_address", "dump_tpcds_item", "events", "sequences", "random", "random_norule"]
LABEL = {"dump_nyc_taxi_2024_01": "NYC TLC Yellow Taxi trip records, Jan 2024, SHA-256 c4d59da7... (real data)", "orders": "orders (generated)", "logs": "logs (generated)", "sensors": "sensors (generated)",
         "dump_tpcds_store_sales": "TPC-DS store_sales (benchmark, generated)", "dump_tpcds_web_sales": "TPC-DS web_sales (benchmark, generated)",
         "dump_tpcds_customer": "TPC-DS customer (benchmark, generated)", "dump_tpcds_customer_address": "TPC-DS customer_address (benchmark, generated)",
         "dump_tpcds_item": "TPC-DS item (benchmark, generated)", "events": "events (realistic generator, kore_size.md)",
         "sequences": "sequences (capability demo, built for the rules)", "random": "random (control)", "random_norule": "random_norule (control, no rule)"}


def med(x):
    x = sorted(x)
    return x[len(x) // 2] if x else float("nan")


def load(d):
    rows = []
    for f in glob.glob(os.path.join(d, "store_*.json")):
        s = json.load(open(f))
        base = os.path.basename(f)[len("store_"):]
        py = os.path.join(d, "py_" + base)
        pyb = os.path.join(d, "pyb_" + base)
        rows.append((s, json.load(open(py)) if os.path.exists(py) else {}, json.load(open(pyb)) if os.path.exists(pyb) else {}))
    rows.sort(key=lambda r: (ORDER.index(r[0]["set"]) if r[0]["set"] in ORDER else 99, r[0]["rows"]))
    return rows


def main():
    d = sys.argv[1]
    rows = load(d)
    lab = lambda s: LABEL.get(s["set"], s["set"])

    print("### Headline: size against the maximum-level baselines (B/value = file bytes / (rows x columns); ratio = baseline bytes / rule-mode bytes)\n")
    print("Rule mode at zstd level 19 (`CompactOptions::default().with_zstd_level(19).with_rules()`), so every column is compared with v4 at the same level.\n")
    print("| dataset | rows x cols | **rule mode, level 19** | v4 level 19 | Parquet zstd-22 | CSV + xz-9 | raw buffers + xz-9 |")
    print("|---|---:|---:|---:|---:|---:|---:|")
    for s, py, _ in rows:
        v = s["values"]
        r = s["rules_19"]

        def c(b):
            return "not measured" if b is None else f"{b / v:.3f} B, {b:,} bytes ({b / r:.2f}x)"
        print(f"| {lab(s)} | {s['rows']:,} x {s['cols']} | **{r / v:.3f} B, {r:,} bytes** | {c(s['v4_19'])} | "
              f"{c(py.get('parquet_zstd22'))} | {c(py.get('csv_xz9'))} | {c(py.get('raw_xz9'))} |")

    print("\n### Rule mode against both version-4 settings (bytes and B/value; ratio = version-4 bytes / rule-mode bytes)\n")
    print("| dataset | rows | v4 default | rule mode, default | ratio | v4 level 19 | rule mode, level 19 | ratio | v4-19 / rule default | v4 default / rule-19 |")
    print("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    for s, _, _ in rows:
        v = s["values"]
        f = lambda b: f"{b:,} ({b / v:.3f} B)"
        print(f"| {lab(s)} | {s['rows']:,} | {f(s['v4_default'])} | {f(s['rules_default'])} | {s['v4_default'] / s['rules_default']:.2f}x | "
              f"{f(s['v4_19'])} | {f(s['rules_19'])} | {s['v4_19'] / s['rules_19']:.2f}x | {s['v4_19'] / s['rules_default']:.2f}x | {s['v4_default'] / s['rules_19']:.2f}x |")

    print("\n### More baselines (bytes; ratio = baseline / rule mode level 19)\n")
    print("| dataset | rows | rule mode, level 19 | DuckDB file | SQLite | SQLite + key index | Parquet zstd-22 dictionary off | dictionary on | sorted by key (dictionary on), key |")
    print("|---|---:|---:|---:|---:|---:|---:|---:|---|")
    for s, _, b in rows:
        if not b:
            continue
        r = s["rules_19"]
        c = lambda x: f"{x:,} ({x / r:.2f}x)"
        print(f"| {lab(s)} | {s['rows']:,} | {r:,} | {c(b['duckdb_file'])} | {c(b['sqlite'])} | {c(b['sqlite_with_key_index'])} | "
              f"{c(b['parquet_zstd22_dict_off'])} | {c(b['parquet_zstd22_dict_on'])} | {c(b['parquet_zstd22_sorted_by_key'])}, `{b['key']}` |")

    print("\n### Encode (paired: v4 default and rule mode default, alternating order; medians of 3; one run each at level 19)\n")
    print("| dataset | rows | v4 default | rule mode default | ratio per repetition | v4 level 19 | rule mode level 19 |")
    print("|---|---:|---:|---:|---:|---:|---:|")
    for s, _, _ in rows:
        er = ", ".join(f"{x:.2f}" for x in s["encode_ratio"])
        print(f"| {lab(s)} | {s['rows']:,} | {med(s['encode_v4_s']) * 1e3:.0f} ms | {med(s['encode_rules_s']) * 1e3:.0f} ms | {er} | "
              f"{s['encode_v4_19_s'] * 1e3:.0f} ms | {s['encode_rules_19_s'] * 1e3:.0f} ms |")

    print("\n### Read (full read and one derived column; the 4 files read in rotating order per repetition; medians of 3; ratios = rule mode level 19 / version 4, per repetition)\n")
    print("Noise floor: in the first run two byte-identical files read by the same code gave paired ratios of 1.31 and 1.50 on this machine (`random_norule`).\n")
    print("| dataset | rows | full read v4 default / v4-19 / rule-19 | ratio vs v4 default | ratio vs v4-19 | column | one column v4 default / v4-19 / rule-19 | ratio vs v4 default | ratio vs v4-19 |")
    print("|---|---:|---:|---:|---:|---|---:|---:|---:|")
    for s, _, _ in rows:
        f, p = s["read_full_s"], s["read_col_s"]
        ms = lambda x: f"{med(x) * 1e3:.1f}"
        rr = lambda x: ", ".join(f"{y:.2f}" for y in x) or "-"
        col = s.get("read_col") or "-"
        pc = f"{ms(p['v4_default'])} / {ms(p['v4_19'])} / {ms(p['rules_19'])} ms" if p["rules_19"] else "-"
        print(f"| {lab(s)} | {s['rows']:,} | {ms(f['v4_default'])} / {ms(f['v4_19'])} / {ms(f['rules_19'])} ms | "
              f"{rr(s['read_full_ratio_rules19_v4_default'])} | {rr(s['read_full_ratio_rules19_v4_19'])} | `{col}` | {pc} | "
              f"{rr(s['read_col_ratio_rules19_v4_default'])} | {rr(s['read_col_ratio_rules19_v4_19'])} |")

    print("\n### Per column, rule mode level 19 (largest size of each dataset)\n")
    seen = {}
    for s, _, _ in rows:
        seen[s["set"]] = s
    for name, s in seen.items():
        print(f"\n**{LABEL.get(name, name)}, {s['rows']:,} rows** (v4 level 19 file {s['v4_19']:,} bytes, rule mode level 19 {s['rules_19']:,} bytes)\n")
        print("| column | v4-19 payload | rule-mode payload | rule |")
        print("|---|---:|---:|---|")
        for c in s["columns_19"]:
            rb = c["rule_bytes"]
            print(f"| {c['name']} | {c['v4_bytes']:,} | {'-' if rb is None else format(rb, ',')} | {c['rule'] or 'none (v4 payload kept)'} |")

    if len(sys.argv) > 2:
        sw = json.load(open(sys.argv[2]))
        print("\n### Exception-rate sweep (200,000 rows, level 19; bytes of the column payload)\n")
        print("| data | column | exceptions | v4 default | v4 level 19 | best rule candidate (encoded in full) | stored by rule mode | stored as |")
        print("|---|---|---:|---:|---:|---:|---:|---|")
        for r in sw:
            cb = r["best_candidate_bytes"]
            cand = "not encoded (sample test failed)" if cb is None else f"{cb:,}" + (" (larger than v4-19)" if cb > r["v4_19"] else "")
            print(f"| {r['kind']} | {r['column']} | {r['rate_pct']:g} % | {r['v4_default']:,} | {r['v4_19']:,} | {cand} | {r['stored']:,} | {r['stored_rule'] or 'v4 payload'} |")


if __name__ == "__main__":
    main()
