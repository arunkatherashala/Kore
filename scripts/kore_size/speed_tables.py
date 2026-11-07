"""Markdown tables for docs/kore_speed_v4.md from one run.sh output directory (store side built from
fileformat-improve-3: batch writer, explicit fast settings, key index).

  python -I speed_tables.py OUTDIR > tables.md
"""
import glob
import json
import os
import statistics
import sys

OUT = sys.argv[1]
SETS = {"r": "realistic", "u": "uniform"}
REF = ["Parquet zstd (default level)", "DuckDB .duckdb after CHECKPOINT", "SQLite"]


def load(p):
    return json.load(open(p)) if os.path.exists(p) else None


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
        if x["min"] == x["max"]:
            return t(x["median"])
        return f"{t(x['median'])} [{t(x['min'])}-{t(x['max'])}]"
    if isinstance(x, (int, float)):
        return t(x)
    return x if x else "not measured"


def rt(x):
    if not isinstance(x, dict):
        return "-"
    if x["min"] == x["max"]:
        return f"{x['median']:.2f}"
    return f"{x['median']:.2f} [{x['min']:.2f}-{x['max']:.2f}]"


def sz(b, n):
    return f"{b / 1e6:.2f} MB, {b / n:.1f} B/event"


def sizes():
    return sorted({int(os.path.basename(p).split("_")[-1].split(".")[0]) for p in glob.glob(os.path.join(OUT, "store_*.json"))})


def main():
    ns = sizes()
    print(f"Run directory (not committed): `{OUT}`\n")
    lt = os.path.join(OUT, "load.txt")
    if os.path.exists(lt):
        vals = []
        for line in open(lt):
            try:
                vals.append(float(line.strip().split(",")[-1].strip('"')))
            except ValueError:
                pass
        if vals:
            print(f"Machine load (typeperf total CPU, {len(vals)} samples at start/middle/end of every dataset and size): "
                  f"mean {statistics.mean(vals):.0f} %, min {min(vals):.0f} %, max {max(vals):.0f} %.\n")
    sk = os.path.join(OUT, "skipped.txt")
    if os.path.exists(sk):
        print("Skipped: " + "; ".join(l.strip() for l in open(sk) if l.strip()) + "\n")
    for s, label in SETS.items():
        js = {n: load(os.path.join(OUT, f"store_{s}_{n}.json")) for n in ns}
        if not any(js.values()):
            continue
        py = {n: load(os.path.join(OUT, f"py_{s}_{n}.json")) for n in ns}
        f1 = {n: load(os.path.join(OUT, f"dl_f1_{s}_{n}.json")) for n in ns}
        f3 = {n: load(os.path.join(OUT, f"dl_f3_{s}_{n}.json")) for n in ns}
        have = [n for n in ns if js[n]]

        print(f"### Size, {label} dataset\n")
        print("| | " + " | ".join(f"N={n:,}" for n in have) + " |")
        print("|---|" + "---:|" * len(have))
        rows = [
            ("KORE v4 (data files + dictionary + MANIFEST; same bytes before and after)", lambda j, n: j["v4"]["size_bytes"]),
            ("key index object (.kidx) alone", lambda j, n: j["key_index"]["v4_index_bytes"]),
            ("**KORE v4 + key index**", lambda j, n: j["v4"]["size_bytes"] + j["key_index"]["v4_index_bytes"]),
            ("KORE v4, `CompactOptions::fast()` (explicit, other bytes)", lambda j, n: j["v4_fast"]["size_bytes"]),
            ("KORE v4, `fast().with_zstd_level(3)` (explicit, other bytes)", lambda j, n: j["v4_fast_level3"]["size_bytes"]),
            ("KORE default writer", lambda j, n: j["default"]["size_bytes"]),
        ]
        for name, f in rows:
            print(f"| {name} | " + " | ".join(sz(f(js[n], n), n) for n in have) + " |")
        if any(js[n].get("v4_max_level") for n in have):
            print("| KORE v4, zstd level 19 | " + " | ".join(sz(js[n]["v4_max_level"]["size_bytes"], n) if js[n].get("v4_max_level") else "not measured" for n in have) + " |")
        for name in REF:
            print(f"| {name} | " + " | ".join(sz(py[n]["formats"][name]["size_bytes"], n) if py[n] and isinstance(py[n]["formats"].get(name), dict) else "not measured" for n in have) + " |")
        print("| Digital Life log format 3 | " + " | ".join(sz(f3[n]["size_bytes"], n) if f3[n] else "not measured" for n in have) + " |")
        print()

        print(f"### Write, {label} dataset (median [min-max] over repetitions)\n")
        print("| | " + " | ".join(f"N={n:,}" for n in have) + " |")
        print("|---|" + "---|" * len(have))
        wr = [
            ("KORE default writer, one segment at a time", lambda j: st(j["default"]["write_s"])),
            ("**KORE v4** (`to_bytes_compact`; writer code and bytes as bbbefd2c), one segment at a time", lambda j: st(j["v4"]["write_s"])),
            ("paired ratio v4 / default", lambda j: rt(j["paired_ratio_v4_over_default"]["write"])),
            ("**KORE v4 batch API** (`to_bytes_compact_many`, all segments at once, same bytes)", lambda j: st(j["v4_batch_writer"]["write_s"])),
            ("paired ratio v4 batch / v4 one at a time", lambda j: rt(j["v4_batch_writer"]["paired_ratio_over_v4"])),
            ("paired ratio v4 batch / default one at a time", lambda j: rt(j["v4_batch_writer"]["paired_ratio_over_default"])),
            ("KORE default writer, all segments at once (fair reference for the batch row)", lambda j: st(j["default_batch_writer"]["write_s"])),
            ("paired ratio v4 batch / default batch", lambda j: rt(j["default_batch_writer"]["paired_ratio_v4_batch_over_default_batch"])),
            ("KORE v4 `fast()` (explicit, other bytes), one segment at a time", lambda j: st(j["v4_fast"]["write_s"])),
            ("paired ratio v4 fast / v4", lambda j: rt(j["v4_fast"]["paired_ratio_over_v4"])),
            ("paired ratio v4 fast / default", lambda j: rt(j["v4_fast"]["paired_ratio_over_default"])),
            ("KORE v4 `fast().with_zstd_level(3)` (explicit, other bytes), one at a time", lambda j: st(j["v4_fast_level3"]["write_s"])),
            ("paired ratio v4 fast+level 3 / v4", lambda j: rt(j["v4_fast_level3"]["paired_ratio_over_v4"])),
            ("paired ratio v4 fast+level 3 / default", lambda j: rt(j["v4_fast_level3"]["paired_ratio_over_default"])),
            ("KORE v4 zstd level 19 (one run)", lambda j: st(j["v4_max_level"]["write_s"]) if j.get("v4_max_level") else "not measured"),
            ("key index build (keys in memory + sha256 of every events file + write)", lambda j: st(j["key_index"]["build_s"])),
        ]
        for name, f in wr:
            print(f"| {name} | " + " | ".join(f(js[n]) for n in have) + " |")
        for name in REF:
            print(f"| {name} | " + " | ".join(st(py[n]["formats"][name]["write_s"]) if py[n] and isinstance(py[n]["formats"].get(name), dict) else "not measured" for n in have) + " |")
        print("| Digital Life log format 3, build via log API (fsync per commit) | " + " | ".join(st(f3[n]["build_s"]) if f3[n] else "not measured" for n in have) + " |")
        print()

        print(f"### Point lookup by event id, {label} dataset (text of one event; per operation)\n")
        print("| | " + " | ".join(f"N={n:,}" for n in have) + " |")
        print("|---|" + "---|" * len(have))
        lk = [
            ("KORE v4 before: scan of the event_id column of each segment, then text", lambda j: st(j["v4"]["lookup_per_op_s"])),
            ("**KORE v4 + key index, warm** (index and dictionary in memory; one file read, sha256-checked)", lambda j: st(j["key_index"]["lookup_v4_sha256_per_op_s"])),
            ("paired ratio indexed / scan", lambda j: rt(j["key_index"]["paired_ratio_indexed_over_scan_v4"])),
            ("KORE v4 + key index, warm, `Verify::Length` (file CRCs only)", lambda j: st(j["key_index"]["lookup_v4_length_only_per_op_s"])),
            ("KORE v4 + key index, index and dictionary loaded from disk and verified in every operation", lambda j: st(j["key_index"]["lookup_open_index_each_op_s"])),
            ("KORE v4 + key index, missing key (warm)", lambda j: st(j["key_index"]["lookup_missing_per_op_s"]) + f" ({j['key_index']['missing_file_reads']} file reads for {j['key_index']['missing_keys']} keys)"),
            ("KORE default files + key index, warm", lambda j: st(j["key_index"]["lookup_default_files_per_op_s"])),
            ("KORE default files, scan", lambda j: st(j["default"]["lookup_per_op_s"])),
            ("key index load + verify (read file, sha256, structure)", lambda j: st(j["key_index"]["load_verify_s"])),
        ]
        for name, f in lk:
            print(f"| {name} | " + " | ".join(f(js[n]) for n in have) + " |")
        for name in REF:
            print(f"| {name} | " + " | ".join(st(py[n]["formats"][name]["lookup_per_op_s"]) if py[n] and isinstance(py[n]["formats"].get(name), dict) else "not measured" for n in have) + " |")
        print("| Digital Life log format 3, get_event (in-memory id index built by open) | " + " | ".join(st(f3[n]["lookup_per_op_s"]) if f3[n] else "not measured" for n in have) + " |")
        print("| Digital Life log format 1, get_event | " + " | ".join(st(f1[n]["lookup_per_op_s"]) if f1[n] else "not measured" for n in have) + " |")
        print()

        print(f"### Full read and projection, {label} dataset (reader code unchanged; measured for completeness)\n")
        print("| | " + " | ".join(f"N={n:,}" for n in have) + " |")
        print("|---|" + "---|" * len(have))
        for name, f in [
            ("KORE v4 full read", lambda j: st(j["v4"]["full_read_s"])),
            ("KORE default full read", lambda j: st(j["default"]["full_read_s"])),
            ("paired ratio v4 / default, full read", lambda j: rt(j["paired_ratio_v4_over_default"]["full_read"])),
            ("KORE v4 projection (events.kind)", lambda j: st(j["v4"]["projection_kind_s"])),
            ("KORE default projection", lambda j: st(j["default"]["projection_kind_s"])),
            ("paired ratio v4 / default, projection", lambda j: rt(j["paired_ratio_v4_over_default"]["projection"])),
        ]:
            print(f"| {name} | " + " | ".join(f(js[n]) for n in have) + " |")
        for name in REF:
            print(f"| {name} full read / projection | " + " | ".join(
                (st(py[n]["formats"][name]["full_read_s"]) + " / " + st(py[n]["formats"][name]["projection_s"])) if py[n] and isinstance(py[n]["formats"].get(name), dict) else "not measured" for n in have) + " |")
        print()
        print(f"Key index ({label}): " + "; ".join(
            f"N={n:,}: {js[n]['key_index']['keys']:,} keys over {js[n]['key_index']['files']} files, {js[n]['key_index']['v4_index_bytes']:,} bytes "
            f"({js[n]['key_index']['v4_index_bytes'] / js[n]['key_index']['keys']:.2f} B/key)" for n in have) + ".\n")


main()
